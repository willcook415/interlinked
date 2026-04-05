use super::super::*;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{command, AppHandle};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SandboxSnapshotFile {
    pub(crate) snapshot: SnapshotMeta,
    pub(crate) scenario: ScenarioDocumentLite,
    pub(crate) history: SimHistory,
    #[serde(default)]
    pub(crate) runtime: Option<PersistedRuntimeState>,
}

pub(crate) fn reset_runtime_tick(
    state: &tauri::State<AppState>,
    project_path: &str,
) -> Result<(), String> {
    let mut guard = state
        .runtime_tick
        .lock()
        .map_err(|_| "runtime_tick mutex poisoned".to_string())?;
    *guard = Some(RuntimeTick {
        project_path: project_path.to_string(),
        last_step: Instant::now(),
    });
    Ok(())
}

fn compute_smooth_dt_s(
    state: &tauri::State<AppState>,
    project_path: &str,
    speed: u32,
) -> Result<f64, String> {
    let now = Instant::now();
    let mut guard = state
        .runtime_tick
        .lock()
        .map_err(|_| "runtime_tick mutex poisoned".to_string())?;
    let mut elapsed = 0.1_f64;
    match guard.as_mut() {
        Some(rt) if rt.project_path == project_path => {
            elapsed = now.saturating_duration_since(rt.last_step).as_secs_f64();
            rt.last_step = now;
        }
        _ => {
            *guard = Some(RuntimeTick {
                project_path: project_path.to_string(),
                last_step: now,
            });
        }
    }
    // Keep dt large enough for visible passenger movement while still bounding catch-up spikes.
    let clamped = elapsed.clamp(0.05, 2.0);
    Ok(clamped * normalize_speed(speed) as f64)
}

#[command]
pub fn start_runtime_loop(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<RuntimeLoopStatus, String> {
    start_runtime_loop_internal(&app, state.inner(), &project_path)
}

#[command]
pub fn stop_runtime_loop(
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<RuntimeLoopStatus, String> {
    let should_stop = runtime_loop_matches_project(state.inner(), &project_path)?;
    if should_stop {
        let _ = stop_runtime_loop_internal(state.inner())?;
    }
    runtime_loop_status_for_project(state.inner(), &project_path)
}

#[command]
pub fn get_runtime_snapshot(
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<Option<RuntimeSnapshot>, String> {
    let control_state = runtime_control_state_for_project(state.inner(), &project_path)?;
    if let Some(fast) = latest_runtime_fast_snapshot_for_project(state.inner(), &project_path)? {
        let strategic =
            latest_runtime_strategic_snapshot_for_project(state.inner(), &project_path)?;
        let mut snapshot = runtime_snapshot_from_parts(&fast, strategic.as_ref());
        snapshot.telemetry.snapshot_age_ms =
            now_epoch_ms().saturating_sub(snapshot.captured_at_epoch_ms);
        if let Some((running, speed, clock_revision, queue_depth)) = control_state {
            snapshot.telemetry.queue_depth = queue_depth;
            if snapshot.clock_revision <= clock_revision {
                snapshot.clock.running = running;
                snapshot.clock.speed = speed;
                snapshot.clock_revision = clock_revision;
            }
        } else if let Ok(status) = runtime_loop_status_for_project(state.inner(), &project_path) {
            snapshot.telemetry.queue_depth = status.queue_depth;
        }
        return Ok(Some(snapshot));
    }
    let project_root = PathBuf::from(&project_path);
    if !project_root.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&project_root)?;
    let clock_revision = control_state
        .map(|(_, _, revision, _)| revision)
        .unwrap_or(0);
    let scenario_for_bootstrap = if project_is_current(&state, &project_path).unwrap_or(false) {
        state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?
            .as_ref()
            .map(|gs| gs.store.scenario().clone())
    } else {
        None
    };
    if let Some(scenario) = scenario_for_bootstrap {
        if let Ok(snapshot) = bootstrap_runtime_snapshot_from_state(
            state.inner(),
            &project_path,
            &manifest,
            &scenario,
            clock_revision,
        ) {
            return Ok(Some(snapshot));
        }
    }
    let mut fallback =
        default_runtime_snapshot_for_manifest(&project_path, &manifest, clock_revision);
    if let Some((running, speed, clock_revision, queue_depth)) = control_state {
        fallback.clock.running = running;
        fallback.clock.speed = speed;
        fallback.clock_revision = clock_revision;
        fallback.telemetry.queue_depth = queue_depth;
    }
    Ok(Some(fallback))
}

#[command]
pub fn get_runtime_fast_snapshot(
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<Option<RuntimeFastSnapshot>, String> {
    let control_state = runtime_control_state_for_project(state.inner(), &project_path)?;
    if let Some(mut snapshot) =
        latest_runtime_fast_snapshot_for_project(state.inner(), &project_path)?
    {
        snapshot.telemetry.snapshot_age_ms =
            now_epoch_ms().saturating_sub(snapshot.captured_at_epoch_ms);
        if let Some((running, speed, clock_revision, queue_depth)) = control_state {
            snapshot.telemetry.queue_depth = queue_depth;
            if snapshot.clock_revision <= clock_revision {
                snapshot.clock.running = running;
                snapshot.clock.speed = speed;
                snapshot.clock_revision = clock_revision;
            }
        } else if let Ok(status) = runtime_loop_status_for_project(state.inner(), &project_path) {
            snapshot.telemetry.queue_depth = status.queue_depth;
        }
        return Ok(Some(snapshot));
    }

    let project_root = PathBuf::from(&project_path);
    if !project_root.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&project_root)?;
    let clock_revision = control_state
        .map(|(_, _, revision, _)| revision)
        .unwrap_or(0);
    let mut fallback =
        default_runtime_fast_snapshot_for_manifest(&project_path, &manifest, clock_revision);
    if let Some((running, speed, clock_revision, queue_depth)) = control_state {
        fallback.clock.running = running;
        fallback.clock.speed = speed;
        fallback.clock_revision = clock_revision;
        fallback.telemetry.queue_depth = queue_depth;
    }
    Ok(Some(fallback))
}

#[command]
pub fn get_runtime_strategic_snapshot(
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<Option<RuntimeStrategicSnapshot>, String> {
    let control_state = runtime_control_state_for_project(state.inner(), &project_path)?;
    if let Some(mut snapshot) =
        latest_runtime_strategic_snapshot_for_project(state.inner(), &project_path)?
    {
        snapshot.telemetry.snapshot_age_ms =
            now_epoch_ms().saturating_sub(snapshot.captured_at_epoch_ms);
        if let Some((running, speed, clock_revision, queue_depth)) = control_state {
            snapshot.telemetry.queue_depth = queue_depth;
            if snapshot.clock_revision <= clock_revision {
                snapshot.clock.running = running;
                snapshot.clock.speed = speed;
                snapshot.clock_revision = clock_revision;
            }
        } else if let Ok(status) = runtime_loop_status_for_project(state.inner(), &project_path) {
            snapshot.telemetry.queue_depth = status.queue_depth;
        }
        return Ok(Some(snapshot));
    }

    let project_root = PathBuf::from(&project_path);
    if !project_root.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&project_root)?;
    let clock_revision = control_state
        .map(|(_, _, revision, _)| revision)
        .unwrap_or(0);
    let mut fallback =
        default_runtime_strategic_snapshot_for_manifest(&project_path, &manifest, clock_revision);
    if let Some((running, speed, clock_revision, queue_depth)) = control_state {
        fallback.clock.running = running;
        fallback.clock.speed = speed;
        fallback.clock_revision = clock_revision;
        fallback.telemetry.queue_depth = queue_depth;
    }
    Ok(Some(fallback))
}

#[command]
pub fn enqueue_runtime_action(
    state: tauri::State<AppState>,
    project_path: String,
    request: RuntimeActionRequest,
) -> Result<RuntimeLoopStatus, String> {
    let project_root = PathBuf::from(&project_path);
    let action = match request.action.trim().to_ascii_lowercase().as_str() {
        "set_running" => {
            let running = request.running.unwrap_or(false);
            if let Ok(mut manifest) = read_manifest(&project_root) {
                manifest.clock_state.running = running;
                manifest.updated_at = now_string();
                let _ = write_manifest(&project_root, &manifest);
            }
            RuntimeAction::SetRunning(running)
        }
        "set_speed" => {
            let speed = normalize_speed(request.speed.unwrap_or(1));
            if let Ok(mut manifest) = read_manifest(&project_root) {
                manifest.clock_state.speed = speed;
                manifest.updated_at = now_string();
                let _ = write_manifest(&project_root, &manifest);
            }
            RuntimeAction::SetSpeed(speed)
        }
        "invalidate_materialization" => RuntimeAction::InvalidateMaterialization,
        "force_checkpoint" => RuntimeAction::ForceCheckpoint,
        "advance_once" => RuntimeAction::AdvanceOnce {
            recompute_quick_kpis: true,
        },
        _ => return Err("unknown runtime action".to_string()),
    };
    let _ = enqueue_runtime_action_with_retry(state.inner(), &project_path, action)?;
    runtime_loop_status_for_project(state.inner(), &project_path)
}

#[command]
pub fn set_simulation_speed(
    state: tauri::State<AppState>,
    project_path: String,
    speed: u32,
) -> Result<SimulationClock, String> {
    if !matches!(speed, 1 | 2 | 4) {
        return Err("speed must be one of [1,2,4]".to_string());
    }
    let project_root = PathBuf::from(&project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    if runtime_loop_matches_project(state.inner(), &project_path_string)? {
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path_string,
            RuntimeAction::SetSpeed(speed),
        )?;
        let mut status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        for _ in 0..12 {
            if status.speed == speed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        }
        let mut manifest = read_manifest(&project_root)?;
        manifest.clock_state.running = status.running;
        manifest.clock_state.speed = status.speed;
        return Ok(manifest.clock_state);
    }
    let mut manifest = read_manifest(&project_root)?;
    manifest.clock_state.speed = speed;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(manifest.clock_state)
}

#[command]
pub fn set_simulation_running(
    state: tauri::State<AppState>,
    project_path: String,
    running: bool,
) -> Result<SimulationClock, String> {
    let project_root = PathBuf::from(&project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    if running {
        reset_runtime_tick(&state, &project_path_string)?;
    }
    if runtime_loop_matches_project(state.inner(), &project_path_string)? {
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path_string,
            RuntimeAction::SetRunning(running),
        )?;
        if !running {
            let _ = enqueue_runtime_action_with_retry(
                state.inner(),
                &project_path_string,
                RuntimeAction::ForceCheckpoint,
            )?;
        }
        let mut status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        for _ in 0..12 {
            if status.running == running {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        }
        let mut manifest = read_manifest(&project_root)?;
        manifest.clock_state.running = status.running;
        manifest.clock_state.speed = status.speed;
        return Ok(manifest.clock_state);
    }
    let mut manifest = read_manifest(&project_root)?;
    manifest.clock_state.running = running;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(manifest.clock_state)
}

#[command]
pub fn advance_simulation(
    state: tauri::State<AppState>,
    project_path: String,
    recompute_quick_kpis: bool,
) -> Result<SimulationAdvanceResult, String> {
    if let Some(snapshot) = latest_runtime_snapshot_for_project(state.inner(), &project_path)? {
        if let Some(result) = runtime_snapshot_to_advance(&snapshot) {
            return Ok(result);
        }
    }
    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    if manifest.runtime_scheduling.enabled
        && enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path,
            RuntimeAction::AdvanceOnce {
                recompute_quick_kpis,
            },
        )?
    {
        thread::sleep(Duration::from_millis(8));
        if let Some(snapshot) = latest_runtime_snapshot_for_project(state.inner(), &project_path)? {
            if let Some(result) = runtime_snapshot_to_advance(&snapshot) {
                return Ok(result);
            }
        }
    }
    let dt_s = compute_smooth_dt_s(&state, &project_path, manifest.clock_state.speed)?;
    let tick_index = latest_runtime_snapshot_for_project(state.inner(), &project_path)?
        .map(|s| s.telemetry.tick_index.saturating_add(1))
        .unwrap_or(1);
    let clock_revision = runtime_control_state_for_project(state.inner(), &project_path)?
        .map(|(_, _, revision, _)| revision)
        .unwrap_or(0);
    let strategic_interval = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1) as u64;
    let strategic_refresh_due = tick_index % strategic_interval == 0;
    let snapshot = run_simulation_tick(
        state.inner(),
        &project_root,
        &mut manifest,
        dt_s,
        dt_s.max(0.05),
        recompute_quick_kpis,
        tick_index,
        clock_revision,
        0,
        0,
        true,
        strategic_refresh_due,
    )?;
    let publish_strategic = publish_strategic_snapshot_for_tick(&snapshot);
    publish_runtime_snapshots(
        state.inner(),
        snapshot.clone(),
        manifest.runtime_scheduling.snapshot_ring,
        publish_strategic,
    )?;
    runtime_snapshot_to_advance(&snapshot)
        .ok_or_else(|| "missing frame in simulation snapshot".to_string())
}

#[command]
pub fn save_sandbox_snapshot(
    state: tauri::State<AppState>,
    project_path: String,
    name: String,
    notes: Option<String>,
) -> Result<SnapshotMeta, String> {
    let project_root = PathBuf::from(project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    ensure_project_dirs(&project_root)?;

    let guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    let gs = guard
        .as_ref()
        .ok_or_else(|| "game not initialised for current session".to_string())?;
    let snapshot_meta = SnapshotMeta {
        snapshot_id: new_id("snapshot"),
        name,
        notes,
        created_at: now_string(),
        tick_seconds: gs.tick_s,
    };
    let snapshot_file = SandboxSnapshotFile {
        snapshot: snapshot_meta.clone(),
        scenario: ScenarioDocumentLite {
            schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
            scenario: gs.store.scenario().clone(),
        },
        history: interlinked_engine::platform::history_export(gs),
        runtime: capture_persisted_runtime_state(&state, &project_path_string)?,
    };
    let out_path = snapshots_dir(&project_root).join(format!("{}.json", snapshot_meta.snapshot_id));
    write_json_file(&out_path, &snapshot_file)?;

    let mut manifest = read_manifest(&project_root)?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(snapshot_meta)
}

#[command]
pub fn load_sandbox_snapshot(
    state: tauri::State<AppState>,
    project_path: String,
    snapshot_id: String,
) -> Result<SandboxStateLite, String> {
    let project_root = PathBuf::from(project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    let in_path = snapshots_dir(&project_root).join(format!("{snapshot_id}.json"));
    let snapshot_file: SandboxSnapshotFile = read_json_file(&in_path)?;

    let doc = ScenarioDocument {
        schema_version: snapshot_file.scenario.schema_version,
        scenario: snapshot_file.scenario.scenario.clone(),
    };
    let mut gs = SimulationService::init_game_state(&doc);
    gs.tick_s = snapshot_file.snapshot.tick_seconds;
    gs.sim_state.t_s = snapshot_file.snapshot.tick_seconds;
    gs.history = snapshot_file.history.clone();
    if let Some(runtime) = snapshot_file.runtime.as_ref() {
        apply_persisted_runtime_state_to_game(&mut gs, &doc.scenario, runtime);
    }
    let restored_tick_s = gs.tick_s;

    let mut guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    *guard = Some(gs);

    if let Some(runtime) = snapshot_file.runtime.as_ref() {
        if let Some(ops_wire) = runtime.runtime_ops.as_ref() {
            let mut ops = state
                .runtime_ops
                .lock()
                .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
            *ops = Some(runtime_ops_from_persisted(ops_wire, &project_path_string));
        }
        if let Some(snapshot) = runtime.latest_snapshot.clone() {
            let mut restored = snapshot;
            restored.project_path = project_path_string;
            restored.clock.tick_seconds = restored_tick_s;
            restored.clock.running = false;
            restored.clock.speed = normalize_speed(restored.clock.speed);
            restored.captured_at_epoch_ms = now_epoch_ms();
            restored.telemetry.snapshot_age_ms = 0;
            let mut snapshots = state
                .runtime_snapshots
                .lock()
                .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
            snapshots.clear();
            snapshots.push_back(restored);
        }
    }

    let mut manifest = read_manifest(&project_root)?;
    manifest.clock_state.tick_seconds = restored_tick_s;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;

    Ok(SandboxStateLite {
        snapshot: snapshot_file.snapshot,
        scenario: snapshot_file.scenario,
        history_frames: snapshot_file.history.len(),
    })
}
