use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::persistence::persist_runtime_manifest_now;
use super::scheduling::plan_runtime_catchup;
use super::snapshots::{
    latest_runtime_fast_snapshot_for_project, publish_runtime_snapshots,
    publish_strategic_snapshot_for_tick, push_runtime_fast_snapshot,
};
use crate::{
    merge_runtime_manifest_state, normalize_speed, read_manifest, run_simulation_tick,
    ProjectManifest, RuntimeSnapshot,
};

pub(crate) struct RuntimeWorkerTickCycleContext<'a> {
    pub app: &'a AppHandle,
    pub project_root: &'a Path,
    pub pending_actions: &'a Arc<AtomicUsize>,
    pub running_state: &'a Arc<AtomicBool>,
    pub speed_state: &'a Arc<AtomicU32>,
    pub clock_revision_state: &'a Arc<AtomicU64>,
    pub manifest: &'a mut ProjectManifest,
    pub running: &'a mut bool,
    pub speed: &'a mut u32,
    pub accumulator_s: &'a mut f64,
    pub last_wall: &'a mut Instant,
    pub last_manifest_reload: &'a mut Instant,
    pub tick_index: &'a mut u64,
    pub last_checkpoint_tick: &'a mut u64,
    pub force_checkpoint: &'a mut bool,
    pub perf_wall_elapsed_s: &'a mut f64,
    pub perf_target_game_elapsed_s: &'a mut f64,
    pub perf_game_elapsed_s: &'a mut f64,
    pub perf_cycle_count: &'a mut u64,
    pub perf_cycle_total_ms: &'a mut f64,
    pub perf_step_count: &'a mut u64,
    pub perf_step_total_ms: &'a mut f64,
    pub cycle_start: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeWorkerTickCycleOutcome {
    pub yielded_without_tick: bool,
    pub checkpoint_written: bool,
    pub published_snapshot: bool,
    pub updated_backlog_snapshot: bool,
}

pub(crate) fn run_runtime_worker_tick_cycle(
    ctx: &mut RuntimeWorkerTickCycleContext<'_>,
) -> RuntimeWorkerTickCycleOutcome {
    let mut outcome = RuntimeWorkerTickCycleOutcome::default();

    if ctx.last_manifest_reload.elapsed() >= Duration::from_millis(250) {
        if let Ok(reloaded) = read_manifest(ctx.project_root) {
            *ctx.manifest = merge_runtime_manifest_state(reloaded, ctx.manifest);
            *ctx.running = ctx.running_state.load(Ordering::SeqCst);
            *ctx.speed = normalize_speed(ctx.speed_state.load(Ordering::SeqCst));
            ctx.manifest.clock_state.running = *ctx.running;
            ctx.manifest.clock_state.speed = *ctx.speed;
        }
        *ctx.last_manifest_reload = Instant::now();
    }

    let now = Instant::now();
    let elapsed = now.saturating_duration_since(*ctx.last_wall).as_secs_f64();
    *ctx.last_wall = now;
    if !*ctx.running || !ctx.manifest.runtime_scheduling.enabled {
        if *ctx.force_checkpoint {
            let _ = persist_runtime_manifest_now(ctx.project_root, ctx.manifest);
            *ctx.force_checkpoint = false;
            outcome.checkpoint_written = true;
        }
        outcome.yielded_without_tick = true;
        return outcome;
    }

    let fixed_step_s = ctx
        .manifest
        .runtime_scheduling
        .fixed_step_s
        .clamp(0.05, 1.0);
    let max_steps_per_cycle = ctx
        .manifest
        .runtime_scheduling
        .max_steps_per_cycle
        .clamp(1, 128);
    *ctx.perf_wall_elapsed_s += elapsed.max(0.0);
    *ctx.perf_target_game_elapsed_s += elapsed.max(0.0) * *ctx.speed as f64;
    *ctx.accumulator_s += elapsed.max(0.0) * *ctx.speed as f64;
    let catchup = plan_runtime_catchup(
        *ctx.accumulator_s,
        fixed_step_s,
        max_steps_per_cycle as usize,
    );
    let mut steps = 0usize;
    let mut latest_snapshot: Option<RuntimeSnapshot> = None;
    let strategic_interval = ctx
        .manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1) as u64;
    while steps < catchup.steps_to_run {
        *ctx.tick_index = (*ctx.tick_index).saturating_add(1);
        let clock_revision = ctx
            .clock_revision_state
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        let state = ctx.app.state::<crate::AppState>();
        let emit_runtime_views = steps + 1 == catchup.steps_to_run;
        let strategic_refresh_due =
            emit_runtime_views && (*ctx.tick_index).is_multiple_of(strategic_interval);
        if let Ok(snapshot) = run_simulation_tick(
            &state,
            ctx.project_root,
            ctx.manifest,
            fixed_step_s,
            fixed_step_s,
            (*ctx.tick_index).is_multiple_of(8),
            *ctx.tick_index,
            clock_revision,
            ctx.pending_actions.load(Ordering::SeqCst),
            0,
            emit_runtime_views,
            strategic_refresh_due,
        ) {
            *ctx.perf_step_count = (*ctx.perf_step_count).saturating_add(1);
            *ctx.perf_step_total_ms += snapshot.telemetry.tick_total_ms.max(0.0);
            if emit_runtime_views {
                latest_snapshot = Some(snapshot);
            }
        }
        *ctx.accumulator_s = (*ctx.accumulator_s - fixed_step_s).max(0.0);
        *ctx.perf_game_elapsed_s += fixed_step_s;
        steps += 1;
    }

    // Ordering matters: telemetry and snapshot publishing are computed only after all
    // catch-up steps for this cycle, so fast/strategic publication reflects a single cycle view.
    let cycle_elapsed_ms = ctx.cycle_start.elapsed().as_secs_f64() * 1000.0;
    *ctx.perf_cycle_count = (*ctx.perf_cycle_count).saturating_add(1);
    *ctx.perf_cycle_total_ms += cycle_elapsed_ms.max(0.0);
    let avg_cycle_elapsed_ms = if *ctx.perf_cycle_count > 0 {
        *ctx.perf_cycle_total_ms / *ctx.perf_cycle_count as f64
    } else {
        0.0
    };
    let avg_sim_step_ms = if *ctx.perf_step_count > 0 {
        *ctx.perf_step_total_ms / *ctx.perf_step_count as f64
    } else {
        0.0
    };
    let achieved_speed_ratio = if *ctx.perf_wall_elapsed_s > 0.0 {
        (*ctx.perf_game_elapsed_s / *ctx.perf_wall_elapsed_s).max(0.0)
    } else {
        0.0
    };
    let achieved_vs_target_ratio = if *ctx.perf_target_game_elapsed_s > 0.0 {
        (*ctx.perf_game_elapsed_s / *ctx.perf_target_game_elapsed_s).max(0.0)
    } else {
        1.0
    };
    if let Some(mut snapshot) = latest_snapshot {
        snapshot.telemetry.executed_steps_this_cycle = catchup.steps_to_run as u32;
        snapshot.telemetry.max_steps_per_cycle = max_steps_per_cycle;
        snapshot.telemetry.backlog_steps = catchup.backlog_steps as u32;
        snapshot.telemetry.backlog_s = (*ctx.accumulator_s).max(0.0);
        snapshot.telemetry.accumulator_s = (*ctx.accumulator_s).max(0.0);
        snapshot.telemetry.cycle_elapsed_ms = cycle_elapsed_ms.max(0.0);
        snapshot.telemetry.avg_cycle_elapsed_ms = avg_cycle_elapsed_ms.max(0.0);
        snapshot.telemetry.avg_sim_step_ms = avg_sim_step_ms.max(0.0);
        snapshot.telemetry.real_elapsed_s = (*ctx.perf_wall_elapsed_s).max(0.0);
        snapshot.telemetry.game_elapsed_s = (*ctx.perf_game_elapsed_s).max(0.0);
        snapshot.telemetry.target_game_elapsed_s = (*ctx.perf_target_game_elapsed_s).max(0.0);
        snapshot.telemetry.target_speed_ratio = *ctx.speed as f64;
        snapshot.telemetry.achieved_speed_ratio = achieved_speed_ratio;
        snapshot.telemetry.achieved_vs_target_ratio = achieved_vs_target_ratio;
        snapshot.telemetry.under_sustained_speed =
            catchup.backlog_steps > 0 || achieved_vs_target_ratio < 0.98;
        let state = ctx.app.state::<crate::AppState>();
        let publish_strategic = publish_strategic_snapshot_for_tick(&snapshot);
        let _ = publish_runtime_snapshots(
            &state,
            snapshot,
            ctx.manifest.runtime_scheduling.snapshot_ring,
            publish_strategic,
        );
        outcome.published_snapshot = true;
    } else if catchup.backlog_steps > 0 {
        let state = ctx.app.state::<crate::AppState>();
        if let Ok(Some(mut snapshot)) =
            latest_runtime_fast_snapshot_for_project(&state, &ctx.project_root.to_string_lossy())
        {
            snapshot.telemetry.backlog_steps = catchup.backlog_steps as u32;
            snapshot.telemetry.backlog_s = (*ctx.accumulator_s).max(0.0);
            snapshot.telemetry.accumulator_s = (*ctx.accumulator_s).max(0.0);
            snapshot.telemetry.max_steps_per_cycle = max_steps_per_cycle;
            snapshot.telemetry.executed_steps_this_cycle = catchup.steps_to_run as u32;
            snapshot.telemetry.real_elapsed_s = (*ctx.perf_wall_elapsed_s).max(0.0);
            snapshot.telemetry.game_elapsed_s = (*ctx.perf_game_elapsed_s).max(0.0);
            snapshot.telemetry.target_game_elapsed_s = (*ctx.perf_target_game_elapsed_s).max(0.0);
            snapshot.telemetry.target_speed_ratio = *ctx.speed as f64;
            snapshot.telemetry.achieved_speed_ratio = achieved_speed_ratio;
            snapshot.telemetry.achieved_vs_target_ratio = achieved_vs_target_ratio;
            snapshot.telemetry.under_sustained_speed = true;
            let _ = push_runtime_fast_snapshot(
                &state,
                snapshot,
                ctx.manifest.runtime_scheduling.snapshot_ring,
            );
            outcome.updated_backlog_snapshot = true;
        }
    }

    let checkpoint_interval = ctx
        .manifest
        .runtime_scheduling
        .checkpoint_interval_ticks
        .max(1) as u64;
    if *ctx.force_checkpoint
        || (*ctx.tick_index).saturating_sub(*ctx.last_checkpoint_tick) >= checkpoint_interval
    {
        let _ = persist_runtime_manifest_now(ctx.project_root, ctx.manifest);
        *ctx.last_checkpoint_tick = *ctx.tick_index;
        *ctx.force_checkpoint = false;
        outcome.checkpoint_written = true;
    }
    outcome
}
