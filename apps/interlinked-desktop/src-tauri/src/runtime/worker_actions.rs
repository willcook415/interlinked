use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tauri::{AppHandle, Manager};

use super::scheduling::plan_runtime_catchup;
use super::snapshots::{publish_runtime_snapshots, publish_strategic_snapshot_for_tick};
use super::worker_control::emit_runtime_control_snapshot;
use crate::{normalize_speed, run_simulation_tick, ProjectManifest, RuntimeAction};

pub(crate) struct RuntimeWorkerActionContext<'a> {
    pub app: &'a AppHandle,
    pub project_path: &'a str,
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
    pub tick_index: &'a mut u64,
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
pub(crate) struct RuntimeWorkerActionOutcome {
    pub should_exit: bool,
    pub checkpoint_requested: bool,
    pub control_snapshot_emitted: bool,
    pub invalidated_materialization: bool,
    pub advance_once_executed: bool,
}

fn reset_runtime_cycle_trackers(ctx: &mut RuntimeWorkerActionContext<'_>) {
    *ctx.accumulator_s = 0.0;
    *ctx.last_wall = Instant::now();
    *ctx.perf_wall_elapsed_s = 0.0;
    *ctx.perf_target_game_elapsed_s = 0.0;
    *ctx.perf_game_elapsed_s = 0.0;
    *ctx.perf_cycle_count = 0;
    *ctx.perf_cycle_total_ms = 0.0;
    *ctx.perf_step_count = 0;
    *ctx.perf_step_total_ms = 0.0;
}

pub(crate) fn handle_runtime_worker_actions(
    actions: Vec<RuntimeAction>,
    ctx: &mut RuntimeWorkerActionContext<'_>,
) -> RuntimeWorkerActionOutcome {
    let mut outcome = RuntimeWorkerActionOutcome::default();
    for action in actions {
        match action {
            RuntimeAction::Stop => {
                outcome.should_exit = true;
                *ctx.running = false;
                ctx.running_state.store(false, Ordering::SeqCst);
                reset_runtime_cycle_trackers(ctx);
                *ctx.force_checkpoint = true;
                outcome.checkpoint_requested = true;
            }
            RuntimeAction::SetRunning(next_running) => {
                *ctx.running = next_running;
                ctx.manifest.clock_state.running = next_running;
                ctx.running_state.store(next_running, Ordering::SeqCst);
                reset_runtime_cycle_trackers(ctx);
                let clock_revision = ctx
                    .clock_revision_state
                    .fetch_add(1, Ordering::SeqCst)
                    .saturating_add(1);
                let state = ctx.app.state::<crate::AppState>();
                let _ = emit_runtime_control_snapshot(
                    &state,
                    ctx.project_path,
                    ctx.manifest,
                    clock_revision,
                    ctx.pending_actions.load(Ordering::SeqCst),
                    ctx.manifest.runtime_scheduling.snapshot_ring,
                );
                *ctx.force_checkpoint = true;
                outcome.checkpoint_requested = true;
                outcome.control_snapshot_emitted = true;
            }
            RuntimeAction::SetSpeed(next_speed) => {
                *ctx.speed = normalize_speed(next_speed);
                ctx.manifest.clock_state.speed = *ctx.speed;
                ctx.speed_state.store(*ctx.speed, Ordering::SeqCst);
                reset_runtime_cycle_trackers(ctx);
                let clock_revision = ctx
                    .clock_revision_state
                    .fetch_add(1, Ordering::SeqCst)
                    .saturating_add(1);
                let state = ctx.app.state::<crate::AppState>();
                let _ = emit_runtime_control_snapshot(
                    &state,
                    ctx.project_path,
                    ctx.manifest,
                    clock_revision,
                    ctx.pending_actions.load(Ordering::SeqCst),
                    ctx.manifest.runtime_scheduling.snapshot_ring,
                );
                *ctx.force_checkpoint = true;
                outcome.checkpoint_requested = true;
                outcome.control_snapshot_emitted = true;
            }
            RuntimeAction::InvalidateMaterialization => {
                let state = ctx.app.state::<crate::AppState>();
                if let Ok(mut guard) = state.runtime_materialization.lock() {
                    *guard = None;
                };
                if let Ok(mut guard) = state.runtime_ops.lock() {
                    *guard = None;
                };
                outcome.invalidated_materialization = true;
            }
            RuntimeAction::ForceCheckpoint => {
                *ctx.force_checkpoint = true;
                outcome.checkpoint_requested = true;
            }
            RuntimeAction::AdvanceOnce {
                recompute_quick_kpis,
            } => {
                let state = ctx.app.state::<crate::AppState>();
                let fixed_step_s = ctx.manifest.runtime_scheduling.fixed_step_s.clamp(0.05, 1.0);
                let next_tick_index = (*ctx.tick_index).saturating_add(1);
                let strategic_refresh_due = true;
                if let Ok(mut snapshot) = run_simulation_tick(
                    &state,
                    ctx.project_root,
                    ctx.manifest,
                    fixed_step_s,
                    fixed_step_s,
                    recompute_quick_kpis,
                    next_tick_index,
                    ctx.clock_revision_state.load(Ordering::SeqCst),
                    ctx.pending_actions.load(Ordering::SeqCst),
                    0,
                    true,
                    strategic_refresh_due,
                ) {
                    *ctx.tick_index = (*ctx.tick_index).saturating_add(1);
                    *ctx.perf_game_elapsed_s += fixed_step_s;
                    *ctx.perf_step_count = (*ctx.perf_step_count).saturating_add(1);
                    *ctx.perf_step_total_ms += snapshot.telemetry.tick_total_ms.max(0.0);
                    let avg_sim_step_ms = if *ctx.perf_step_count > 0 {
                        *ctx.perf_step_total_ms / *ctx.perf_step_count as f64
                    } else {
                        0.0
                    };
                    let avg_cycle_elapsed_ms = if *ctx.perf_cycle_count > 0 {
                        *ctx.perf_cycle_total_ms / *ctx.perf_cycle_count as f64
                    } else {
                        0.0
                    };
                    snapshot.telemetry.executed_steps_this_cycle = 1;
                    snapshot.telemetry.max_steps_per_cycle = ctx
                        .manifest
                        .runtime_scheduling
                        .max_steps_per_cycle
                        .clamp(1, 128);
                    snapshot.telemetry.backlog_steps = plan_runtime_catchup(
                        *ctx.accumulator_s,
                        fixed_step_s,
                        ctx.manifest.runtime_scheduling.max_steps_per_cycle.clamp(1, 128) as usize,
                    )
                    .backlog_steps as u32;
                    snapshot.telemetry.backlog_s = (*ctx.accumulator_s).max(0.0);
                    snapshot.telemetry.accumulator_s = (*ctx.accumulator_s).max(0.0);
                    snapshot.telemetry.cycle_elapsed_ms =
                        ctx.cycle_start.elapsed().as_secs_f64() * 1000.0;
                    snapshot.telemetry.avg_cycle_elapsed_ms = avg_cycle_elapsed_ms.max(0.0);
                    snapshot.telemetry.avg_sim_step_ms = avg_sim_step_ms.max(0.0);
                    snapshot.telemetry.real_elapsed_s = (*ctx.perf_wall_elapsed_s).max(0.0);
                    snapshot.telemetry.game_elapsed_s = (*ctx.perf_game_elapsed_s).max(0.0);
                    snapshot.telemetry.target_game_elapsed_s =
                        (*ctx.perf_target_game_elapsed_s).max(0.0);
                    snapshot.telemetry.target_speed_ratio = *ctx.speed as f64;
                    snapshot.telemetry.achieved_speed_ratio = if *ctx.perf_wall_elapsed_s > 0.0 {
                        (*ctx.perf_game_elapsed_s / *ctx.perf_wall_elapsed_s).max(0.0)
                    } else {
                        0.0
                    };
                    snapshot.telemetry.achieved_vs_target_ratio = if *ctx.perf_target_game_elapsed_s > 0.0
                    {
                        (*ctx.perf_game_elapsed_s / *ctx.perf_target_game_elapsed_s).max(0.0)
                    } else {
                        1.0
                    };
                    snapshot.telemetry.under_sustained_speed =
                        snapshot.telemetry.achieved_vs_target_ratio < 0.98;
                    let publish_strategic = publish_strategic_snapshot_for_tick(&snapshot);
                    let _ = publish_runtime_snapshots(
                        &state,
                        snapshot,
                        ctx.manifest.runtime_scheduling.snapshot_ring,
                        publish_strategic,
                    );
                    *ctx.force_checkpoint = true;
                    outcome.checkpoint_requested = true;
                    outcome.advance_once_executed = true;
                }
            }
        }
    }
    outcome
}
