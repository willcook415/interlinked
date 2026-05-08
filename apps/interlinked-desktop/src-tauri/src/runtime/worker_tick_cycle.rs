use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::persistence::persist_runtime_manifest_now;
use super::scheduling::{
    effective_max_steps_per_cycle, effective_strategic_refresh_interval_ticks, plan_runtime_catchup,
};
use super::snapshots::{
    latest_runtime_fast_snapshot_for_project, publish_runtime_snapshots,
    publish_strategic_snapshot_for_tick, push_runtime_fast_snapshot,
};
use crate::{
    merge_runtime_manifest_state, normalize_speed, read_manifest, run_simulation_tick,
    ProjectManifest, RuntimeSnapshot,
};

const RT_LOOP_LOG_TICK_INTERVAL: u64 = 20;
const RT_LOOP_SUMMARY_CYCLE_INTERVAL: u64 = 60;
const RT_LOOP_SLOW_CYCLE_MS: f64 = 120.0;
const RT_LOOP_SLOW_STEP_MS: f64 = 90.0;

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
    pub last_gate_signature: &'a mut Option<(bool, bool)>,
    pub warmup_catchup_pending: &'a mut bool,
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
    let gate_signature = (*ctx.running, ctx.manifest.runtime_scheduling.enabled);
    let previous_gate_signature = *ctx.last_gate_signature;
    if ctx
        .last_gate_signature
        .map(|previous| previous != gate_signature)
        .unwrap_or(true)
    {
        eprintln!(
            "[rt-loop] gate project={} running={} scheduling_enabled={} elapsed_s={:.4} accumulator_s={:.4}",
            ctx.project_root.to_string_lossy(),
            gate_signature.0,
            gate_signature.1,
            elapsed,
            *ctx.accumulator_s
        );
        *ctx.last_gate_signature = Some(gate_signature);
    }
    let entered_runnable_gate = gate_signature.0
        && gate_signature.1
        && previous_gate_signature
            .map(|previous| !previous.0 || !previous.1)
            .unwrap_or(true);
    if entered_runnable_gate {
        *ctx.warmup_catchup_pending = true;
    }
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
    let max_steps_per_cycle = effective_max_steps_per_cycle(
        ctx.manifest.runtime_scheduling.max_steps_per_cycle,
        *ctx.speed,
    );
    let accumulator_before = *ctx.accumulator_s;
    let step_game_target_s = elapsed.max(0.0) * *ctx.speed as f64;
    *ctx.perf_wall_elapsed_s += elapsed.max(0.0);
    *ctx.perf_target_game_elapsed_s += step_game_target_s;
    *ctx.accumulator_s += step_game_target_s;
    let catchup = plan_runtime_catchup(*ctx.accumulator_s, fixed_step_s, max_steps_per_cycle);
    let mut steps = 0usize;
    let mut latest_snapshot: Option<RuntimeSnapshot> = None;
    let mut suppress_warmup_catchup_debt = false;
    let strategic_interval = effective_strategic_refresh_interval_ticks(
        ctx.manifest
            .runtime_scheduling
            .strategic_refresh_interval_ticks,
        *ctx.speed,
    ) as u64;
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
        match run_simulation_tick(
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
            Ok(snapshot) => {
                *ctx.perf_step_count = (*ctx.perf_step_count).saturating_add(1);
                *ctx.perf_step_total_ms += snapshot.telemetry.tick_total_ms.max(0.0);
                let strategic_missing_cache = snapshot.telemetry.engine_strategic_refresh_executed
                    && snapshot
                        .telemetry
                        .engine_strategic_refresh_reason
                        .as_deref()
                        .map(|reason| reason.contains("MissingCache"))
                        .unwrap_or(false);
                if *ctx.warmup_catchup_pending && strategic_missing_cache {
                    suppress_warmup_catchup_debt = true;
                    *ctx.warmup_catchup_pending = false;
                } else if *ctx.warmup_catchup_pending {
                    // Only arm suppression for the first live step after resume.
                    *ctx.warmup_catchup_pending = false;
                }
                if emit_runtime_views {
                    if (*ctx.tick_index).is_multiple_of(RT_LOOP_LOG_TICK_INTERVAL) {
                        eprintln!(
                            "[rt-loop] tick_ok project={} tick_index={} tick_seconds={:.3} clock_revision={} running={} speed={} step_ms={:.2} stage_prepare_ms={:.2} stage_step_ms={:.2} stage_runtime_ops_ms={:.2} stage_economy_ms={:.2}",
                            ctx.project_root.to_string_lossy(),
                            *ctx.tick_index,
                            snapshot.clock.tick_seconds,
                            snapshot.clock_revision,
                            snapshot.clock.running,
                            snapshot.clock.speed,
                            snapshot.telemetry.tick_total_ms.max(0.0),
                            snapshot.telemetry.stage_prepare_ms.max(0.0),
                            snapshot.telemetry.stage_step_ms.max(0.0),
                            snapshot.telemetry.stage_runtime_ops_ms.max(0.0),
                            snapshot.telemetry.stage_economy_ms.max(0.0),
                        );
                    }
                    latest_snapshot = Some(snapshot);
                }
            }
            Err(error) => {
                eprintln!(
                    "[rt-loop] tick_err project={} tick_index={} clock_revision={} error={}",
                    ctx.project_root.to_string_lossy(),
                    *ctx.tick_index,
                    clock_revision,
                    error
                );
            }
        }
        *ctx.accumulator_s = (*ctx.accumulator_s - fixed_step_s).max(0.0);
        *ctx.perf_game_elapsed_s += fixed_step_s;
        steps += 1;
    }
    if suppress_warmup_catchup_debt {
        // Exclude one-time strategic cache warm-up wall time from catch-up debt on first resume.
        // This keeps initial 1x pacing stable instead of burst-running backlog immediately after load.
        *ctx.last_wall = Instant::now();
        eprintln!(
            "[rt-loop] warmup_rebase project={} tick_index={} reason=missing_cache_startup cycle_elapsed_ms={:.2}",
            ctx.project_root.to_string_lossy(),
            *ctx.tick_index,
            ctx.cycle_start.elapsed().as_secs_f64() * 1000.0
        );
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
    if steps > 0 {
        let should_emit_cycle = (*ctx.tick_index).is_multiple_of(RT_LOOP_LOG_TICK_INTERVAL)
            || catchup.backlog_steps > 0
            || catchup.steps_to_run > 1
            || cycle_elapsed_ms > RT_LOOP_SLOW_CYCLE_MS
            || avg_sim_step_ms > RT_LOOP_SLOW_STEP_MS;
        if should_emit_cycle {
            eprintln!(
                "[rt-loop] cycle project={} tick_index={} running={} speed={} wall_elapsed_s={:.4} target_game_elapsed_s={:.4} game_advanced_s={:.4} fixed_step_s={:.3} accumulator_before_s={:.4} accumulator_after_s={:.4} steps_to_run={} steps_executed={} backlog_steps={} max_steps_per_cycle={} cycle_ms={:.2} avg_cycle_ms={:.2} avg_step_ms={:.2} achieved_speed_ratio={:.3} achieved_vs_target_ratio={:.3} pending_actions={} queue_depth={} target_tick_ms={:.2}",
                ctx.project_root.to_string_lossy(),
                *ctx.tick_index,
                *ctx.running,
                *ctx.speed,
                elapsed.max(0.0),
                step_game_target_s.max(0.0),
                (steps as f64 * fixed_step_s).max(0.0),
                fixed_step_s,
                accumulator_before.max(0.0),
                (*ctx.accumulator_s).max(0.0),
                catchup.steps_to_run,
                steps,
                catchup.backlog_steps,
                max_steps_per_cycle,
                cycle_elapsed_ms.max(0.0),
                avg_cycle_elapsed_ms.max(0.0),
                avg_sim_step_ms.max(0.0),
                achieved_speed_ratio.max(0.0),
                achieved_vs_target_ratio.max(0.0),
                ctx.pending_actions.load(Ordering::SeqCst),
                ctx.pending_actions.load(Ordering::SeqCst),
                ctx.manifest.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0),
            );
        }
    }
    if (*ctx.perf_cycle_count).is_multiple_of(RT_LOOP_SUMMARY_CYCLE_INTERVAL)
        || catchup.backlog_steps > 0
    {
        eprintln!(
            "[rt-summary] loop project={} cycles={} steps={} running={} speed={} real_elapsed_s={:.2} target_game_elapsed_s={:.2} game_elapsed_s={:.2} achieved_speed_ratio={:.3} achieved_vs_target_ratio={:.3} avg_cycle_ms={:.2} avg_step_ms={:.2} backlog_steps={} backlog_s={:.3} max_steps_per_cycle={} fixed_step_s={:.3}",
            ctx.project_root.to_string_lossy(),
            *ctx.perf_cycle_count,
            *ctx.perf_step_count,
            *ctx.running,
            *ctx.speed,
            (*ctx.perf_wall_elapsed_s).max(0.0),
            (*ctx.perf_target_game_elapsed_s).max(0.0),
            (*ctx.perf_game_elapsed_s).max(0.0),
            achieved_speed_ratio.max(0.0),
            achieved_vs_target_ratio.max(0.0),
            avg_cycle_elapsed_ms.max(0.0),
            avg_sim_step_ms.max(0.0),
            catchup.backlog_steps,
            (*ctx.accumulator_s).max(0.0),
            max_steps_per_cycle,
            fixed_step_s,
        );
    }
    if let Some(mut snapshot) = latest_snapshot {
        snapshot.telemetry.executed_steps_this_cycle = catchup.steps_to_run as u32;
        snapshot.telemetry.max_steps_per_cycle = max_steps_per_cycle as u32;
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
            snapshot.telemetry.max_steps_per_cycle = max_steps_per_cycle as u32;
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
