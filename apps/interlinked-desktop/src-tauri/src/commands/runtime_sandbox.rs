use super::super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
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

fn should_log_fast_snapshot_served(
    project_path: &str,
    tick_seconds: f64,
    running: bool,
    speed: u32,
) -> bool {
    static LAST: OnceLock<Mutex<HashMap<String, (u64, bool, u32)>>> = OnceLock::new();
    let bucket = if tick_seconds.is_finite() && tick_seconds >= 0.0 {
        tick_seconds.floor() as u64
    } else {
        0
    };
    let next = (bucket, running, speed);
    let cache = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut guard) = cache.lock() else {
        return true;
    };
    let changed = guard
        .get(project_path)
        .map(|previous| *previous != next)
        .unwrap_or(true);
    if changed {
        guard.insert(project_path.to_string(), next);
    }
    changed
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
        if should_log_fast_snapshot_served(
            &project_path,
            snapshot.clock.tick_seconds,
            snapshot.clock.running,
            snapshot.clock.speed,
        ) {
            eprintln!(
                "[rt-snap] served project={} tick_seconds={:.3} tick_index={} clock_revision={} running={} speed={} snapshot_age_ms={} queue_depth={} backlog_steps={} executed_steps_this_cycle={} publish_ms={:.2}",
                project_path,
                snapshot.clock.tick_seconds,
                snapshot.telemetry.tick_index,
                snapshot.clock_revision,
                snapshot.clock.running,
                snapshot.clock.speed,
                snapshot.telemetry.snapshot_age_ms,
                snapshot.telemetry.queue_depth,
                snapshot.telemetry.backlog_steps,
                snapshot.telemetry.executed_steps_this_cycle,
                snapshot.telemetry.snapshot_publish_ms.max(0.0),
            );
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
    if should_log_fast_snapshot_served(
        &project_path,
        fallback.clock.tick_seconds,
        fallback.clock.running,
        fallback.clock.speed,
    ) {
        eprintln!(
            "[rt-snap] served_fallback project={} tick_seconds={:.3} tick_index={} clock_revision={} running={} speed={} queue_depth={}",
            project_path,
            fallback.clock.tick_seconds,
            fallback.telemetry.tick_index,
            fallback.clock_revision,
            fallback.clock.running,
            fallback.clock.speed,
            fallback.telemetry.queue_depth,
        );
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

fn latest_authoritative_clock_for_project(
    state: &tauri::State<AppState>,
    project_path: &str,
    fallback: &SimulationClock,
) -> Result<SimulationClock, String> {
    if let Some(mut snapshot) =
        latest_runtime_fast_snapshot_for_project(state.inner(), project_path)?
    {
        if let Some((running, speed, clock_revision, _queue_depth)) =
            runtime_control_state_for_project(state.inner(), project_path)?
        {
            if snapshot.clock_revision <= clock_revision {
                snapshot.clock.running = running;
                snapshot.clock.speed = speed;
                snapshot.clock_revision = clock_revision;
            }
        }
        return Ok(snapshot.clock);
    }
    Ok(fallback.clone())
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
    let loop_matches = runtime_loop_matches_project(state.inner(), &project_path_string)?;
    eprintln!(
        "[rt-loop] set_speed_request project={} requested_speed={} loop_matches={}",
        project_path_string, speed, loop_matches
    );
    if loop_matches {
        let enqueued = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path_string,
            RuntimeAction::SetSpeed(speed),
        )?;
        eprintln!(
            "[rt-loop] set_speed_enqueue project={} requested_speed={} enqueue_ok={}",
            project_path_string, speed, enqueued
        );
        let mut status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        for _ in 0..12 {
            if status.speed == speed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        }
        eprintln!(
            "[rt-loop] set_speed_status project={} running={} speed={} clock_revision={} queue_depth={}",
            project_path_string,
            status.running,
            status.speed,
            status.clock_revision,
            status.queue_depth
        );
        let mut manifest = read_manifest(&project_root)?;
        manifest.clock_state.running = status.running;
        manifest.clock_state.speed = status.speed;
        let clock = latest_authoritative_clock_for_project(
            &state,
            &project_path_string,
            &manifest.clock_state,
        )?;
        eprintln!(
            "[rt-loop] set_speed_authoritative_clock project={} tick_seconds={:.3} running={} speed={}",
            project_path_string, clock.tick_seconds, clock.running, clock.speed
        );
        return Ok(clock);
    }
    let mut manifest = read_manifest(&project_root)?;
    manifest.clock_state.speed = speed;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    eprintln!(
        "[rt-loop] set_speed_manifest_only project={} tick_seconds={:.3} running={} speed={}",
        project_path_string,
        manifest.clock_state.tick_seconds,
        manifest.clock_state.running,
        manifest.clock_state.speed
    );
    Ok(manifest.clock_state)
}

#[command]
pub fn set_simulation_running(
    state: tauri::State<AppState>,
    project_path: String,
    running: bool,
) -> Result<SimulationClock, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    if running {
        reset_runtime_tick(&state, &project_path_string)?;
    }
    let loop_matches = runtime_loop_matches_project(state.inner(), &project_path_string)?;
    eprintln!(
        "[rt-loop] set_running_request project={} requested_running={} loop_matches={}",
        project_path_string, running, loop_matches
    );
    if loop_matches {
        let enqueue_running = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path_string,
            RuntimeAction::SetRunning(running),
        )?;
        let mut enqueue_checkpoint = false;
        if !running {
            enqueue_checkpoint = enqueue_runtime_action_with_retry(
                state.inner(),
                &project_path_string,
                RuntimeAction::ForceCheckpoint,
            )?;
        }
        eprintln!(
            "[rt-loop] set_running_enqueue project={} requested_running={} enqueue_ok={} checkpoint_enqueued={}",
            project_path_string, running, enqueue_running, enqueue_checkpoint
        );
        let mut status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        for _ in 0..12 {
            if status.running == running {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            status = runtime_loop_status_for_project(state.inner(), &project_path_string)?;
        }
        eprintln!(
            "[rt-loop] set_running_status project={} running={} speed={} clock_revision={} queue_depth={}",
            project_path_string,
            status.running,
            status.speed,
            status.clock_revision,
            status.queue_depth
        );
        let mut manifest = read_manifest(&project_root)?;
        manifest.clock_state.running = status.running;
        manifest.clock_state.speed = status.speed;
        let clock = latest_authoritative_clock_for_project(
            &state,
            &project_path_string,
            &manifest.clock_state,
        )?;
        eprintln!(
            "[build-perf] command.set_simulation_running.total: {}ms project={} running={}",
            started.elapsed().as_millis(),
            project_path_string,
            running
        );
        eprintln!(
            "[rt-loop] set_running_authoritative_clock project={} tick_seconds={:.3} running={} speed={}",
            project_path_string, clock.tick_seconds, clock.running, clock.speed
        );
        return Ok(clock);
    }
    let mut manifest = read_manifest(&project_root)?;
    manifest.clock_state.running = running;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    eprintln!(
        "[build-perf] command.set_simulation_running.total: {}ms project={} running={}",
        started.elapsed().as_millis(),
        project_path_string,
        running
    );
    eprintln!(
        "[rt-loop] set_running_manifest_only project={} tick_seconds={:.3} running={} speed={}",
        project_path_string,
        manifest.clock_state.tick_seconds,
        manifest.clock_state.running,
        manifest.clock_state.speed
    );
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
    let tick_index = crate::runtime::snapshots::latest_runtime_tick_index_for_project(
        state.inner(),
        &project_path,
    )?
    .map(|tick_index| tick_index.saturating_add(1))
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
    let advance = runtime_snapshot_to_advance(&snapshot)
        .ok_or_else(|| "missing frame in simulation snapshot".to_string())?;
    let publish_strategic = publish_strategic_snapshot_for_tick(&snapshot);
    publish_runtime_snapshots(
        state.inner(),
        snapshot,
        manifest.runtime_scheduling.snapshot_ring,
        publish_strategic,
    )?;
    Ok(advance)
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
