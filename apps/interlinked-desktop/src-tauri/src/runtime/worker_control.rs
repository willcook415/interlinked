use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use tauri::AppHandle;

use super::snapshots::{latest_runtime_snapshot_for_project, publish_runtime_snapshots};
use super::worker_loop::runtime_worker_loop;
use crate::{
    bootstrap_runtime_snapshot_from_state, default_runtime_snapshot_for_manifest, normalize_speed,
    now_epoch_ms, read_manifest, AppState, ProjectManifest, RuntimeAction, RuntimeLoopHandle,
    RuntimeLoopStatus,
};

pub(crate) fn runtime_control_state_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<Option<(bool, u32, u64, usize)>, String> {
    let guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    let Some(handle) = guard.as_ref() else {
        return Ok(None);
    };
    if handle.project_path != project_path {
        return Ok(None);
    }
    Ok(Some((
        handle.running.load(Ordering::SeqCst),
        normalize_speed(handle.speed.load(Ordering::SeqCst)),
        handle.clock_revision.load(Ordering::SeqCst),
        handle.pending_actions.load(Ordering::SeqCst),
    )))
}

pub(crate) fn emit_runtime_control_snapshot(
    state: &AppState,
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
    queue_depth: usize,
    ring_capacity: usize,
) -> Result<(), String> {
    let mut snapshot =
        latest_runtime_snapshot_for_project(state, project_path)?.unwrap_or_else(|| {
            default_runtime_snapshot_for_manifest(project_path, manifest, clock_revision)
        });
    snapshot.clock.running = manifest.clock_state.running;
    snapshot.clock.speed = normalize_speed(manifest.clock_state.speed);
    snapshot.clock.tick_seconds = manifest.clock_state.tick_seconds;
    snapshot.clock_revision = clock_revision;
    snapshot.captured_at_epoch_ms = now_epoch_ms();
    snapshot.telemetry.queue_depth = queue_depth;
    snapshot.telemetry.snapshot_age_ms = 0;
    publish_runtime_snapshots(state, snapshot, ring_capacity, true)
}

pub(crate) fn enqueue_runtime_action_internal(
    state: &AppState,
    project_path: &str,
    action: RuntimeAction,
) -> Result<bool, String> {
    let guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    let Some(handle) = guard.as_ref() else {
        return Ok(false);
    };
    if handle.project_path != project_path {
        return Ok(false);
    }
    handle.pending_actions.fetch_add(1, Ordering::SeqCst);
    if handle.tx.send(action).is_ok() {
        return Ok(true);
    }
    decrement_pending_counter(&handle.pending_actions);
    Ok(false)
}

pub(crate) fn runtime_loop_matches_project(
    state: &AppState,
    project_path: &str,
) -> Result<bool, String> {
    let guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    Ok(guard
        .as_ref()
        .map(|handle| handle.project_path == project_path)
        .unwrap_or(false))
}

pub(crate) fn enqueue_runtime_action_with_retry(
    state: &AppState,
    project_path: &str,
    action: RuntimeAction,
) -> Result<bool, String> {
    let loop_was_active = runtime_loop_matches_project(state, project_path)?;
    if !loop_was_active {
        return Ok(false);
    }
    if enqueue_runtime_action_internal(state, project_path, action.clone())? {
        return Ok(true);
    }
    let _ = runtime_loop_status_for_project(state, project_path)?;
    if enqueue_runtime_action_internal(state, project_path, action)? {
        return Ok(true);
    }
    if runtime_loop_matches_project(state, project_path)? {
        return Err("failed to enqueue runtime action for active runtime loop".to_string());
    }
    Ok(false)
}

pub(crate) fn decrement_pending_counter(counter: &AtomicUsize) {
    let mut current = counter.load(Ordering::SeqCst);
    while current > 0 {
        match counter.compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

pub(crate) fn stop_runtime_loop_internal(state: &AppState) -> Result<bool, String> {
    let mut guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    let mut handle = match guard.take() {
        Some(h) => h,
        None => return Ok(false),
    };
    handle.pending_actions.fetch_add(1, Ordering::SeqCst);
    let _ = handle.tx.send(RuntimeAction::Stop);
    // Drop the runtime loop lock before joining so other callers are not blocked on shutdown.
    drop(guard);
    if let Some(join) = handle.join.take() {
        let _ = join.join();
    }
    Ok(true)
}

pub(crate) fn start_runtime_loop_internal(
    app: &AppHandle,
    state: &AppState,
    project_path: &str,
) -> Result<RuntimeLoopStatus, String> {
    let manifest = read_manifest(Path::new(project_path))?;
    {
        let guard = state
            .runtime_loop
            .lock()
            .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
        if let Some(existing) = guard.as_ref() {
            if existing.project_path == project_path {
                return Ok(RuntimeLoopStatus {
                    project_path: project_path.to_string(),
                    running: existing.running.load(Ordering::SeqCst),
                    speed: normalize_speed(existing.speed.load(Ordering::SeqCst)),
                    clock_revision: existing.clock_revision.load(Ordering::SeqCst),
                    queue_depth: existing.pending_actions.load(Ordering::SeqCst),
                    enabled: manifest.runtime_scheduling.enabled,
                });
            }
        }
    }
    let _ = stop_runtime_loop_internal(state)?;
    {
        let mut snapshots = state
            .runtime_snapshots
            .lock()
            .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
        snapshots.clear();
    }
    {
        let mut snapshots = state
            .runtime_fast_snapshots
            .lock()
            .map_err(|_| "runtime_fast_snapshots mutex poisoned".to_string())?;
        snapshots.clear();
    }
    {
        let mut snapshots = state
            .runtime_strategic_snapshots
            .lock()
            .map_err(|_| "runtime_strategic_snapshots mutex poisoned".to_string())?;
        snapshots.clear();
    }
    {
        let mut materialization = state
            .runtime_materialization
            .lock()
            .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
        *materialization = None;
    }
    {
        let mut ops = state
            .runtime_ops
            .lock()
            .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
        if ops
            .as_ref()
            .map(|value| value.project_path.as_str() != project_path)
            .unwrap_or(false)
        {
            *ops = None;
        }
    }
    let (tx, rx) = mpsc::channel::<RuntimeAction>();
    let pending = Arc::new(AtomicUsize::new(0));
    let project_for_thread = project_path.to_string();
    let app_for_thread = app.clone();
    let pending_for_thread = Arc::clone(&pending);
    let running = Arc::new(std::sync::atomic::AtomicBool::new(
        manifest.clock_state.running,
    ));
    let speed = Arc::new(std::sync::atomic::AtomicU32::new(normalize_speed(
        manifest.clock_state.speed,
    )));
    let clock_revision = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let running_for_thread = Arc::clone(&running);
    let speed_for_thread = Arc::clone(&speed);
    let clock_revision_for_thread = Arc::clone(&clock_revision);
    let join = thread::Builder::new()
        .name("interlinked-runtime-loop".to_string())
        .spawn(move || {
            runtime_worker_loop(
                app_for_thread,
                project_for_thread,
                rx,
                pending_for_thread,
                running_for_thread,
                speed_for_thread,
                clock_revision_for_thread,
            )
        })
        .map_err(|e| e.to_string())?;
    let mut guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    *guard = Some(RuntimeLoopHandle {
        project_path: project_path.to_string(),
        tx,
        pending_actions: Arc::clone(&pending),
        running: Arc::clone(&running),
        speed: Arc::clone(&speed),
        clock_revision: Arc::clone(&clock_revision),
        join: Some(join),
    });
    let scenario_for_bootstrap = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?
        .as_ref()
        .map(|gs| gs.store.scenario().clone());
    if let Some(scenario) = scenario_for_bootstrap {
        if let Ok(snapshot) =
            bootstrap_runtime_snapshot_from_state(state, project_path, &manifest, &scenario, 0)
        {
            let _ = publish_runtime_snapshots(
                state,
                snapshot,
                manifest.runtime_scheduling.snapshot_ring,
                true,
            );
        }
    }
    Ok(RuntimeLoopStatus {
        project_path: project_path.to_string(),
        running: manifest.clock_state.running,
        speed: normalize_speed(manifest.clock_state.speed),
        clock_revision: 0,
        queue_depth: pending.load(Ordering::SeqCst),
        enabled: manifest.runtime_scheduling.enabled,
    })
}

pub(crate) fn runtime_loop_status_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<RuntimeLoopStatus, String> {
    let guard = state
        .runtime_loop
        .lock()
        .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
    let (loop_active, running, speed, clock_revision, queue_depth) =
        if let Some(handle) = guard.as_ref() {
            if handle.project_path == project_path {
                (
                    true,
                    handle.running.load(Ordering::SeqCst),
                    normalize_speed(handle.speed.load(Ordering::SeqCst)),
                    handle.clock_revision.load(Ordering::SeqCst),
                    handle.pending_actions.load(Ordering::SeqCst),
                )
            } else {
                (false, false, normalize_speed(1), 0, 0)
            }
        } else {
            (false, false, normalize_speed(1), 0, 0)
        };
    drop(guard);
    let (enabled, manifest_running, manifest_speed) = read_manifest(Path::new(project_path))
        .map(|m| {
            (
                m.runtime_scheduling.enabled,
                m.clock_state.running,
                normalize_speed(m.clock_state.speed),
            )
        })
        .unwrap_or((true, false, normalize_speed(1)));
    Ok(RuntimeLoopStatus {
        project_path: project_path.to_string(),
        running: if loop_active {
            running
        } else {
            manifest_running
        },
        speed: if loop_active { speed } else { manifest_speed },
        clock_revision: if loop_active { clock_revision } else { 0 },
        queue_depth,
        enabled,
    })
}
