use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::persistence::persist_runtime_manifest_now;
use super::worker_actions::{handle_runtime_worker_actions, RuntimeWorkerActionContext};
use super::worker_control::decrement_pending_counter;
use super::worker_tick_cycle::{run_runtime_worker_tick_cycle, RuntimeWorkerTickCycleContext};
use crate::{normalize_speed, read_manifest, RuntimeAction};

const RT_WORKER_ACTION_LOG_INTERVAL: u64 = 20;

pub(crate) fn runtime_worker_loop(
    app: AppHandle,
    project_path: String,
    rx: Receiver<RuntimeAction>,
    pending_actions: Arc<AtomicUsize>,
    running_state: Arc<AtomicBool>,
    speed_state: Arc<AtomicU32>,
    clock_revision_state: Arc<AtomicU64>,
) {
    let project_root = PathBuf::from(&project_path);
    let mut manifest = match read_manifest(&project_root) {
        Ok(m) => m,
        Err(_) => return,
    };
    manifest.clock_state.speed = normalize_speed(manifest.clock_state.speed);
    let mut running = manifest.clock_state.running;
    let mut speed = manifest.clock_state.speed;
    running_state.store(running, std::sync::atomic::Ordering::SeqCst);
    speed_state.store(speed, std::sync::atomic::Ordering::SeqCst);
    let mut accumulator_s = 0.0_f64;
    let mut last_wall = Instant::now();
    let mut tick_index = 0_u64;
    let mut last_checkpoint_tick = 0_u64;
    let mut force_checkpoint = false;
    let mut last_manifest_reload = Instant::now();
    let mut perf_wall_elapsed_s = 0.0_f64;
    let mut perf_target_game_elapsed_s = 0.0_f64;
    let mut perf_game_elapsed_s = 0.0_f64;
    let mut perf_cycle_count = 0_u64;
    let mut perf_cycle_total_ms = 0.0_f64;
    let mut perf_step_count = 0_u64;
    let mut perf_step_total_ms = 0.0_f64;
    let mut last_gate_signature: Option<(bool, bool)> = None;
    let mut warmup_catchup_pending = false;
    let mut action_batch_count = 0_u64;
    loop {
        let cycle_start = Instant::now();
        let timeout = if running {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(120)
        };
        let mut actions = Vec::<RuntimeAction>::new();
        match rx.recv_timeout(timeout) {
            Ok(action) => {
                decrement_pending_counter(&pending_actions);
                actions.push(action);
                while let Ok(next) = rx.try_recv() {
                    decrement_pending_counter(&pending_actions);
                    actions.push(next);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let action_started = Instant::now();
        let action_count = actions.len();
        let action_outcome = handle_runtime_worker_actions(
            actions,
            &mut RuntimeWorkerActionContext {
                app: &app,
                project_path: &project_path,
                project_root: &project_root,
                pending_actions: &pending_actions,
                running_state: &running_state,
                speed_state: &speed_state,
                clock_revision_state: &clock_revision_state,
                manifest: &mut manifest,
                running: &mut running,
                speed: &mut speed,
                accumulator_s: &mut accumulator_s,
                last_wall: &mut last_wall,
                tick_index: &mut tick_index,
                force_checkpoint: &mut force_checkpoint,
                perf_wall_elapsed_s: &mut perf_wall_elapsed_s,
                perf_target_game_elapsed_s: &mut perf_target_game_elapsed_s,
                perf_game_elapsed_s: &mut perf_game_elapsed_s,
                perf_cycle_count: &mut perf_cycle_count,
                perf_cycle_total_ms: &mut perf_cycle_total_ms,
                perf_step_count: &mut perf_step_count,
                perf_step_total_ms: &mut perf_step_total_ms,
                cycle_start,
            },
        );
        action_batch_count = action_batch_count.saturating_add(1);
        let action_ms = action_started.elapsed().as_secs_f64() * 1000.0;
        if action_count > 0
            || action_ms > 30.0
            || action_batch_count.is_multiple_of(RT_WORKER_ACTION_LOG_INTERVAL)
        {
            eprintln!(
                "[rt-loop] action_batch project={} batch={} actions={} action_ms={:.2} running={} speed={} pending_actions={} should_exit={} checkpoint_requested={} control_snapshot_emitted={} invalidated_materialization={} advance_once_executed={}",
                project_path,
                action_batch_count,
                action_count,
                action_ms.max(0.0),
                running,
                speed,
                pending_actions.load(std::sync::atomic::Ordering::SeqCst),
                action_outcome.should_exit,
                action_outcome.checkpoint_requested,
                action_outcome.control_snapshot_emitted,
                action_outcome.invalidated_materialization,
                action_outcome.advance_once_executed,
            );
        }
        if action_outcome.should_exit {
            break;
        }

        let tick_cycle_outcome =
            run_runtime_worker_tick_cycle(&mut RuntimeWorkerTickCycleContext {
                app: &app,
                project_root: &project_root,
                pending_actions: &pending_actions,
                running_state: &running_state,
                speed_state: &speed_state,
                clock_revision_state: &clock_revision_state,
                manifest: &mut manifest,
                running: &mut running,
                speed: &mut speed,
                accumulator_s: &mut accumulator_s,
                last_wall: &mut last_wall,
                last_manifest_reload: &mut last_manifest_reload,
                tick_index: &mut tick_index,
                last_checkpoint_tick: &mut last_checkpoint_tick,
                force_checkpoint: &mut force_checkpoint,
                perf_wall_elapsed_s: &mut perf_wall_elapsed_s,
                perf_target_game_elapsed_s: &mut perf_target_game_elapsed_s,
                perf_game_elapsed_s: &mut perf_game_elapsed_s,
                perf_cycle_count: &mut perf_cycle_count,
                perf_cycle_total_ms: &mut perf_cycle_total_ms,
                perf_step_count: &mut perf_step_count,
                perf_step_total_ms: &mut perf_step_total_ms,
                last_gate_signature: &mut last_gate_signature,
                warmup_catchup_pending: &mut warmup_catchup_pending,
                cycle_start,
            });
        if tick_cycle_outcome.yielded_without_tick {
            continue;
        }
    }
    manifest.clock_state.running = running;
    manifest.clock_state.speed = speed;
    let _ = persist_runtime_manifest_now(&project_root, &mut manifest);
}
