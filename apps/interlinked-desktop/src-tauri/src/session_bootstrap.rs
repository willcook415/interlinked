use crate::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

fn start_location_from_manifest(manifest: &ProjectManifest) -> Option<StartLocation> {
    manifest.start_location.clone()
}

pub(crate) fn open_session_internal(
    app: &AppHandle,
    state: &tauri::State<AppState>,
    project_root: &Path,
) -> Result<OpenSessionResult, String> {
    let project_path_string = project_root.to_string_lossy().to_string();
    let should_stop_runtime = {
        let guard = state
            .runtime_loop
            .lock()
            .map_err(|_| "runtime_loop mutex poisoned".to_string())?;
        guard
            .as_ref()
            .map(|h| h.project_path != project_path_string)
            .unwrap_or(false)
    };
    if should_stop_runtime {
        let _ = stop_runtime_loop_internal(state.inner())?;
    } else {
        let _ = enqueue_runtime_action_internal(
            state.inner(),
            &project_path_string,
            RuntimeAction::SetRunning(false),
        );
        let _ = enqueue_runtime_action_internal(
            state.inner(),
            &project_path_string,
            RuntimeAction::ForceCheckpoint,
        );
        thread::sleep(Duration::from_millis(12));
    }
    {
        let mut snapshots = state
            .runtime_snapshots
            .lock()
            .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
        snapshots.clear();
    }
    {
        let mut materialization = state
            .runtime_materialization
            .lock()
            .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
        *materialization = None;
    }
    let persisted_sandbox_state = load_persisted_sandbox_state(project_root);
    let persisted_runtime_state = persisted_sandbox_state
        .as_ref()
        .and_then(|state_file| state_file.runtime.clone());
    let mut manifest = read_manifest(project_root)?;
    if manifest.session_kind == SessionKind::Game {
        manifest.clock_state.running = false;
        let persisted_tick = persisted_runtime_state
            .as_ref()
            .map(|runtime| runtime.tick_s)
            .or_else(|| {
                persisted_sandbox_state
                    .as_ref()
                    .map(|state_file| state_file.tick_s)
            });
        if let Some(tick_s) = persisted_tick {
            if tick_s.is_finite() && tick_s >= 0.0 {
                manifest.clock_state.tick_seconds = tick_s;
            }
        }
    }
    seed_unlocked_countries(&mut manifest);
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();
    write_manifest(project_root, &manifest)?;

    let mut doc =
        ScenarioService::load_from_path(scenario_path(project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    if manifest.session_kind == SessionKind::Game {
        let (center_lon, center_lat, city_population, country_iso2) = manifest
            .start_location
            .as_ref()
            .map(|s| {
                (
                    s.city_lon,
                    s.city_lat,
                    s.city_population,
                    Some(s.country_iso2.as_str()),
                )
            })
            .unwrap_or((-1.5491, 53.8008, None, None));
        let mut changed = migrate_legacy_synthetic_demand(&mut doc.scenario);
        changed |= ensure_game_bootstrap_network(
            &mut doc.scenario,
            center_lon,
            center_lat,
            city_population,
            country_iso2,
        );
        let coverage =
            ensure_unlocked_country_surfaces_loaded(app, &mut manifest, &mut doc.scenario)?;
        if !coverage.is_empty() {
            changed = true;
        }
        if coverage.iter().any(|c| !c.installed) {
            manifest.demand_surface = Some({
                let mut ds = manifest
                    .demand_surface
                    .clone()
                    .unwrap_or_else(default_demand_surface_manifest);
                ds.last_rebuild_at = Some(now_string());
                ds
            });
        }
        if changed {
            ScenarioService::save_to_path(
                scenario_path(project_root).to_string_lossy().as_ref(),
                &doc,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let _country_charge = apply_country_entry_charges(&mut manifest, &doc.scenario);
    manifest.updated_at = now_string();
    write_manifest(project_root, &manifest)?;
    let scenario = ScenarioDocumentLite {
        schema_version: doc.schema_version,
        scenario: doc.scenario,
    };

    let mut gs = SimulationService::init_game_state(&ScenarioDocument {
        schema_version: scenario.schema_version,
        scenario: scenario.scenario.clone(),
    });
    gs.tick_s = manifest.clock_state.tick_seconds;
    gs.sim_state.t_s = manifest.clock_state.tick_seconds;
    if let Some(runtime) = persisted_runtime_state.as_ref() {
        apply_persisted_runtime_state_to_game(&mut gs, &scenario.scenario, runtime);
    }
    if gs.tick_s.is_finite()
        && gs.tick_s >= 0.0
        && (manifest.clock_state.tick_seconds - gs.tick_s).abs() > 1e-6
    {
        manifest.clock_state.tick_seconds = gs.tick_s;
        manifest.updated_at = now_string();
        write_manifest(project_root, &manifest)?;
    }
    let mut game_guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    *game_guard = Some(gs);

    let mut current_guard = state
        .current_project
        .lock()
        .map_err(|_| "current_project mutex poisoned".to_string())?;
    *current_guard = Some(project_path_string.clone());
    drop(current_guard);

    if let Some(runtime) = persisted_runtime_state.as_ref() {
        if let Some(ops_wire) = runtime.runtime_ops.as_ref() {
            let mut ops = state
                .runtime_ops
                .lock()
                .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
            *ops = Some(runtime_ops_from_persisted(ops_wire, &project_path_string));
        }
        if let Some(snapshot) = runtime.latest_snapshot.clone() {
            let mut restored = snapshot;
            restored.project_path = project_path_string.clone();
            restored.clock.tick_seconds = manifest.clock_state.tick_seconds;
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
    if manifest.session_kind == SessionKind::Game {
        if let Ok(bootstrap) = bootstrap_runtime_snapshot_from_state(
            state.inner(),
            &project_path_string,
            &manifest,
            &scenario.scenario,
            0,
        ) {
            if let Ok(mut snapshots) = state.runtime_snapshots.lock() {
                snapshots.clear();
                snapshots.push_back(bootstrap);
            }
        }
    }
    commands::runtime_sandbox::reset_runtime_tick(state, &project_path_string)?;

    let mut runs = Vec::<RunMeta>::new();
    for run_id in &manifest.recent_runs {
        let meta_path = runs_dir(project_root).join(run_id).join("meta.json");
        if let Ok(meta) = read_json_file::<RunMeta>(&meta_path) {
            runs.push(meta);
        }
    }

    let mut snapshots = Vec::<SnapshotMeta>::new();
    let snap_dir = snapshots_dir(project_root);
    if snap_dir.exists() {
        for ent in fs::read_dir(&snap_dir).map_err(|e| e.to_string())? {
            let ent = ent.map_err(|e| e.to_string())?;
            let p = ent.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(snap_file) = read_json_file::<SandboxSnapshotFile>(&p) {
                snapshots.push(snap_file.snapshot);
            }
        }
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    update_index_opened(app, project_root, &manifest)?;

    Ok(OpenSessionResult {
        project_path: project_root.to_string_lossy().to_string(),
        manifest: manifest.clone(),
        scenario,
        runs,
        snapshots,
        clock: manifest.clock_state.clone(),
        start_location: start_location_from_manifest(&manifest),
    })
}

pub(crate) fn sync_progress_budget_from_economy(manifest: &mut ProjectManifest) {
    if let Some(metrics) = manifest.progress_metrics.as_mut() {
        let cfg = economy_config();
        let currency = normalize_currency(Some(&manifest.economy.currency));
        metrics.currency = currency.clone();
        metrics.budget = from_base_currency(manifest.economy.current_balance_base, &currency, &cfg);
    }
}

pub(crate) fn project_is_current(
    state: &tauri::State<AppState>,
    project_path: &str,
) -> Result<bool, String> {
    let current = state
        .current_project
        .lock()
        .map_err(|_| "current_project mutex poisoned".to_string())?;
    Ok(current
        .as_deref()
        .map(|value| value == project_path)
        .unwrap_or(false))
}

fn seed_unlocked_countries(manifest: &mut ProjectManifest) {
    if let Some(start) = manifest.start_location.as_ref() {
        if let Some(code) = canonical_country_iso2(&start.country_iso2) {
            let mut set = manifest
                .economy
                .unlocked_countries
                .iter()
                .filter_map(|x| canonical_country_iso2(x))
                .collect::<BTreeSet<_>>();
            set.insert(code);
            manifest.economy.unlocked_countries = set.into_iter().collect();
        }
    }
}

fn ensure_unlocked_country_surfaces_loaded(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
) -> Result<Vec<DemandCoverageResult>, String> {
    rematerialize_unlocked_country_surfaces(app, manifest, scenario)
}

pub(crate) fn apply_country_entry_charges(
    manifest: &mut ProjectManifest,
    scenario: &Scenario,
) -> f64 {
    let cfg = economy_config();
    let unlocked = manifest
        .economy
        .unlocked_countries
        .iter()
        .filter_map(|x| canonical_country_iso2(x))
        .collect::<BTreeSet<_>>();
    let scenario_countries = countries_in_scenario(scenario)
        .into_iter()
        .filter_map(|country_iso2| canonical_country_iso2(&country_iso2))
        .collect::<BTreeSet<_>>();
    let charge_base = scenario_countries
        .iter()
        .filter(|c| !unlocked.contains(*c))
        .count() as f64
        * cfg.country_entry_fee_base;
    if charge_base > 0.0 {
        manifest.economy.current_balance_base -= charge_base;
        manifest.economy.cumulative_capex_base += charge_base;
        update_region_ledger(manifest, 0.0, 0.0, 0.0, charge_base);
        record_monthly_financial_delta(manifest, 0.0, 0.0, charge_base, 0.0);
    }
    let merged = unlocked
        .union(&scenario_countries)
        .cloned()
        .collect::<BTreeSet<_>>();
    manifest.economy.unlocked_countries = merged.into_iter().collect();
    sync_progress_budget_from_economy(manifest);
    charge_base
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn legacy_projects_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("projects")
}

pub(crate) fn bootstrap_legacy_projects(app: &AppHandle) -> Result<(), String> {
    let idx = read_index(app)?;
    if !idx.projects.is_empty() {
        return Ok(());
    }

    let legacy_root = legacy_projects_root();
    if !legacy_root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&legacy_root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source = entry.path();
        if !source.is_dir() || source.extension().and_then(|x| x.to_str()) != Some("interlinked") {
            continue;
        }
        if !manifest_path(&source).exists() || !scenario_path(&source).exists() {
            continue;
        }
        let legacy_manifest = match read_manifest(&source) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let target = projects_root(app)?.join(
            source
                .file_name()
                .and_then(|x| x.to_str())
                .unwrap_or("legacy.interlinked"),
        );
        if !target.exists() {
            copy_dir_recursive(&source, &target)?;
        }
        upsert_index_entry(
            app,
            SaveIndexEntry {
                project_id: legacy_manifest.project_id.clone(),
                project_path: target.to_string_lossy().to_string(),
                name: legacy_manifest.name.clone(),
                session_kind: legacy_manifest.session_kind.clone(),
                last_opened_at: now_string(),
            },
        )?;
    }
    Ok(())
}
