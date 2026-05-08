use interlinked_engine::model::Scenario;
use interlinked_engine::platform::{
    scenario_network_stats, to_base_currency, ScenarioDocument, ScenarioService,
};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::{command, AppHandle};

use super::super::{
    apply_country_entry_charges, bootstrap_legacy_projects,
    build_region_catalog_for_surface_with_app, canonical_country_iso2,
    capture_persisted_runtime_state, default_ancillary_revenue_rate, default_clock_for,
    default_demand_surface_manifest, default_economy_manifest, default_fare_policy_manifest,
    default_maintenance_rate, default_progress_metrics, default_quality_penalty_rates,
    default_runtime_scheduling_manifest, default_simulation_scope_manifest,
    default_starting_budget_display, default_surface_pipeline_demand_meta, default_template_doc,
    default_template_doc_at_location, difficulty_label, difficulty_profile_for,
    display_country_name, economy_config, ensure_project_dirs, load_persisted_sandbox_state,
    load_surface_wire, nearest_region_for_start, new_id, normalize_currency, now_string,
    open_session_internal, projects_root, read_country_pack_index, read_deleted_index, read_index,
    read_json_file, read_manifest, remove_index_entry, runs_dir, sandbox_state_path, scenario_path,
    stop_runtime_loop_internal, sync_progress_budget_from_economy, trash_root, ui_layouts_path,
    update_index_opened, upsert_index_entry, write_deleted_index, write_json_file, write_manifest,
    AppState, DeleteSaveResult, DeletedIndexEntry, DeletedSaveMeta, EconomyManifest,
    GameCreatePayload, GameProgressMetrics, GameSaveMeta, OpenSessionResult,
    PersistedSandboxStateFile, ProjectManifest, PurgeSaveResult, RegionStateManifest,
    RestoreSaveResult, RunMeta, RunSummary, SaveIndexEntry, SaveResult, SaveSessionPayload,
    ScenarioCreatePayload, ScenarioSaveMeta, SessionKind, StartLocation, MANIFEST_FILE,
    SANDBOX_STATE_FILE, SCENARIO_FILE, UI_LAYOUTS_FILE,
};
use super::content_library::{
    country_pack_status_for, resolve_demand_surface_path, rollout_supported_countries,
};

fn perf_log(label: &str, started: Instant) {
    eprintln!("[perf] {label}: {}ms", started.elapsed().as_millis());
}

fn seed_start_region_for_new_game(app: &AppHandle, manifest: &mut ProjectManifest) {
    let Some(start) = manifest.start_location.as_ref() else {
        return;
    };
    let Some(iso) = canonical_country_iso2(&start.country_iso2) else {
        return;
    };
    let Some(resolved_surface) = resolve_demand_surface_path(app, &iso) else {
        return;
    };
    let Ok(surface) = load_surface_wire(&resolved_surface.path) else {
        return;
    };
    let Ok(catalog) = build_region_catalog_for_surface_with_app(app, &iso, &surface) else {
        return;
    };
    let seed_region_id = deterministic_uk_start_region_id(&catalog, start)
        .or_else(|| nearest_region_for_start(&catalog, manifest.start_location.as_ref(), &iso));
    let Some(seed_region_id) = seed_region_id else {
        return;
    };
    // New games should start with a meaningful planning-region foothold around the chosen city.
    manifest.region_state.unlocked_region_ids = vec![seed_region_id.clone()];
    manifest.region_state.primary_focus_region_id = Some(seed_region_id.clone());
    manifest.region_state.active_region_ids = vec![seed_region_id];
}

fn deterministic_uk_start_region_token(start: &StartLocation) -> Option<&'static str> {
    match start.city_id {
        2_643_743 => return Some("central_london"),
        2_643_123 => return Some("central_manchester"),
        2_644_688 => return Some("central_leeds"),
        _ => {}
    }
    let city = start.city_name.trim().to_ascii_lowercase();
    match city.as_str() {
        "london" => Some("central_london"),
        "manchester" => Some("central_manchester"),
        "leeds" => Some("central_leeds"),
        _ => None,
    }
}

fn deterministic_uk_start_region_id(
    catalog: &crate::region::catalog::SurfaceRegionCatalog,
    start: &StartLocation,
) -> Option<String> {
    if !start.country_iso2.eq_ignore_ascii_case("UK")
        && !start.country_iso2.eq_ignore_ascii_case("GB")
    {
        return None;
    }
    let token = deterministic_uk_start_region_token(start)?;
    let mut token_aliases = vec![token.to_string()];
    let dashed = token.replace('_', "-");
    if dashed != token {
        token_aliases.push(dashed);
    }

    for alias in &token_aliases {
        if catalog.by_id.contains_key(alias) {
            return Some(alias.clone());
        }
        let id_uk = format!("r6:UK:{alias}");
        if catalog.by_id.contains_key(&id_uk) {
            return Some(id_uk);
        }
        let id_gb = format!("r6:GB:{alias}");
        if catalog.by_id.contains_key(&id_gb) {
            return Some(id_gb);
        }
    }
    catalog.regions.iter().find_map(|region| {
        token_aliases
            .iter()
            .find(|alias| {
                region.region_token.eq_ignore_ascii_case(alias)
                    || region
                        .region_id
                        .rsplit(':')
                        .next()
                        .map(|value| value.eq_ignore_ascii_case(alias))
                        .unwrap_or(false)
            })
            .map(|_| region.region_id.clone())
    })
}

#[command]
pub fn list_scenario_saves(app: AppHandle) -> Result<Vec<ScenarioSaveMeta>, String> {
    bootstrap_legacy_projects(&app)?;
    let idx = read_index(&app)?;
    let mut out = Vec::<ScenarioSaveMeta>::new();
    for ent in idx.projects {
        if ent.session_kind != SessionKind::Scenario {
            continue;
        }
        let root = PathBuf::from(&ent.project_path);
        if !root.exists() {
            continue;
        }
        let manifest = match read_manifest(&root) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let (
            latest_run_created_at,
            latest_share_trips_served,
            latest_mean_generalized_cost_s,
            latest_total_boardings_denied,
            latest_projected_net_balance,
        ) = if let Some(run_id) = &manifest.last_opened_run_id {
            let run_root = runs_dir(&root).join(run_id);
            let meta = read_json_file::<RunMeta>(&run_root.join("meta.json")).ok();
            let summary = read_json_file::<RunSummary>(&run_root.join("summary.json")).ok();
            (
                meta.map(|m| m.created_at),
                summary.as_ref().map(|s| s.share_trips_served),
                summary.as_ref().map(|s| s.mean_generalized_cost_s),
                summary.as_ref().map(|s| s.total_boardings_denied),
                summary.as_ref().map(|s| s.projected_net_balance),
            )
        } else {
            (None, None, None, None, None)
        };
        out.push(ScenarioSaveMeta {
            project_id: manifest.project_id.clone(),
            project_path: ent.project_path,
            name: manifest.name,
            last_opened_at: ent.last_opened_at,
            latest_run_id: manifest.last_opened_run_id,
            latest_run_created_at,
            latest_share_trips_served,
            latest_mean_generalized_cost_s,
            latest_total_boardings_denied,
            latest_projected_net_balance,
            start_country: manifest
                .start_location
                .as_ref()
                .map(|x| x.country_name.clone()),
            start_city: manifest
                .start_location
                .as_ref()
                .map(|x| x.city_name.clone()),
        });
    }
    out.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
    Ok(out)
}

#[command]
pub fn load_scenario_save(
    app: AppHandle,
    state: tauri::State<AppState>,
    save_id: String,
) -> Result<OpenSessionResult, String> {
    let idx = read_index(&app)?;
    let entry = idx
        .projects
        .iter()
        .find(|x| x.project_id == save_id && x.session_kind == SessionKind::Scenario)
        .ok_or_else(|| format!("scenario save id not found: {save_id}"))?;
    open_session_internal(&app, &state, Path::new(&entry.project_path))
}

#[command]
pub fn create_game(
    app: AppHandle,
    state: tauri::State<AppState>,
    payload: GameCreatePayload,
) -> Result<OpenSessionResult, String> {
    if canonical_country_iso2(&payload.country_iso2).is_none() {
        return Err("country_iso2 must be a two-letter ISO code".to_string());
    }
    if !payload.city_lon.is_finite() || !payload.city_lat.is_finite() {
        return Err("city coordinates must be finite".to_string());
    }
    let country_iso2 = canonical_country_iso2(&payload.country_iso2)
        .ok_or_else(|| "country_iso2 must be a two-letter ISO code".to_string())?;
    let index = read_country_pack_index(&app)?;
    let rollout = rollout_supported_countries();
    let pack_status = country_pack_status_for(&app, &index, &rollout, &country_iso2);
    if !pack_status.eligible {
        let reason = pack_status
            .reason
            .unwrap_or_else(|| "Country pack unavailable".to_string());
        return Err(format!("{reason} for {country_iso2}"));
    }

    let currency = normalize_currency(payload.currency.as_deref());
    let project_root = projects_root(&app)?.join(format!("game_{}.interlinked", new_id("save")));
    ensure_project_dirs(&project_root)?;

    let mut doc = default_template_doc_at_location(
        &payload.name,
        payload.city_lon,
        payload.city_lat,
        payload.city_population,
        Some(country_iso2.as_str()),
    );
    doc.scenario.meta.name = payload.name.clone();
    let difficulty_profile = difficulty_profile_for(payload.difficulty);
    doc.scenario.params.trips_per_person *= difficulty_profile.demand_mult;
    // Game sessions must source demand from installed country surfaces only.
    doc.scenario.world.zones.clear();
    doc.scenario.world.demand_cells.clear();
    doc.scenario.world.demand_meta = Some(default_surface_pipeline_demand_meta());
    for st in &mut doc.scenario.world.stops {
        st.country_iso2 = Some(country_iso2.clone());
    }
    for z in &mut doc.scenario.world.zones {
        z.country_iso2 = Some(country_iso2.clone());
    }
    for c in &mut doc.scenario.world.demand_cells {
        c.country_iso2 = Some(country_iso2.clone());
    }
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;

    write_json_file(
        &sandbox_state_path(&project_root),
        &serde_json::json!({ "tick_s": 0.0 }),
    )?;
    write_json_file(&ui_layouts_path(&project_root), &serde_json::json!({}))?;

    let now = now_string();
    let starting_budget_display = if payload.starting_budget > 0.0 {
        payload.starting_budget
    } else {
        default_starting_budget_display(payload.difficulty, &currency)
    };
    let cfg = economy_config();
    let starting_budget_base = to_base_currency(starting_budget_display, &currency, &cfg);
    let manifest = ProjectManifest {
        project_id: new_id("project"),
        name: payload.name,
        created_at: now.clone(),
        updated_at: now,
        session_kind: SessionKind::Game,
        engine_schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
        ui_schema_version: 2,
        last_opened_run_id: None,
        recent_runs: vec![],
        clock_state: default_clock_for(&SessionKind::Game),
        progress_metrics: Some(GameProgressMetrics {
            budget: starting_budget_display,
            currency: currency.clone(),
            ridership: 0.0,
            coverage: 0.0,
            milestones: 0,
        }),
        start_location: Some(StartLocation {
            country_iso2: country_iso2.clone(),
            country_name: if display_country_name(&country_iso2).is_empty() {
                payload.country_name
            } else {
                display_country_name(&country_iso2).to_string()
            },
            city_id: payload.city_id,
            city_name: payload.city_name,
            city_lon: payload.city_lon,
            city_lat: payload.city_lat,
            city_population: payload.city_population,
        }),
        economy: EconomyManifest {
            currency: currency.clone(),
            difficulty: difficulty_label(payload.difficulty),
            difficulty_profile: difficulty_profile.clone(),
            economy_revision: 1,
            starting_budget_base,
            current_balance_base: starting_budget_base,
            cumulative_capex_base: 0.0,
            cumulative_opex_base: 0.0,
            cumulative_revenue_base: 0.0,
            cumulative_lost_demand_penalty_base: 0.0,
            fare_revenue_deferred_base: 0.0,
            fare_boardings_deferred_pax: 0.0,
            fare_policy: default_fare_policy_manifest(),
            unlocked_countries: vec![country_iso2],
            region_ledger: BTreeMap::new(),
            maintenance_rate: default_maintenance_rate(),
            ancillary_revenue_rate: default_ancillary_revenue_rate(),
            quality_penalty_rates: default_quality_penalty_rates(),
            monthly_financials: Vec::new(),
        },
        demand_surface: Some(default_demand_surface_manifest()),
        region_state: RegionStateManifest::default(),
        simulation_scope: default_simulation_scope_manifest(),
        runtime_scheduling: default_runtime_scheduling_manifest(),
        pack_refs: vec![],
    };
    let mut manifest = manifest;
    seed_start_region_for_new_game(&app, &mut manifest);
    write_manifest(&project_root, &manifest)?;
    update_index_opened(&app, &project_root, &manifest)?;
    open_session_internal(&app, &state, &project_root)
}

#[command]
pub fn create_scenario(
    app: AppHandle,
    state: tauri::State<AppState>,
    payload: ScenarioCreatePayload,
) -> Result<OpenSessionResult, String> {
    let project_root =
        projects_root(&app)?.join(format!("scenario_{}.interlinked", new_id("save")));
    ensure_project_dirs(&project_root)?;

    let mut doc = default_template_doc(&payload.name);
    doc.scenario.meta.name = payload.name.clone();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;

    write_json_file(
        &sandbox_state_path(&project_root),
        &serde_json::json!({ "tick_s": 0.0 }),
    )?;
    write_json_file(&ui_layouts_path(&project_root), &serde_json::json!({}))?;

    let now = now_string();
    let manifest = ProjectManifest {
        project_id: new_id("project"),
        name: payload.name,
        created_at: now.clone(),
        updated_at: now,
        session_kind: SessionKind::Scenario,
        engine_schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
        ui_schema_version: 2,
        last_opened_run_id: None,
        recent_runs: vec![],
        clock_state: default_clock_for(&SessionKind::Scenario),
        progress_metrics: None,
        start_location: None,
        economy: default_economy_manifest(),
        demand_surface: Some(default_demand_surface_manifest()),
        region_state: RegionStateManifest::default(),
        simulation_scope: default_simulation_scope_manifest(),
        runtime_scheduling: default_runtime_scheduling_manifest(),
        pack_refs: vec![],
    };
    write_manifest(&project_root, &manifest)?;
    update_index_opened(&app, &project_root, &manifest)?;
    open_session_internal(&app, &state, &project_root)
}

#[command]
pub fn import_scenario(
    app: AppHandle,
    state: tauri::State<AppState>,
    file_path: String,
    name: Option<String>,
) -> Result<OpenSessionResult, String> {
    let source = PathBuf::from(&file_path);
    if !source.exists() {
        return Err(format!("scenario file does not exist: {file_path}"));
    }
    let doc = ScenarioService::load_from_path(source.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    let scenario_name = name.unwrap_or_else(|| doc.scenario.meta.name.clone());

    let project_root =
        projects_root(&app)?.join(format!("scenario_{}.interlinked", new_id("import")));
    ensure_project_dirs(&project_root)?;
    let mut final_doc = doc.clone();
    final_doc.scenario.meta.name = scenario_name.clone();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &final_doc,
    )
    .map_err(|e| e.to_string())?;
    write_json_file(
        &sandbox_state_path(&project_root),
        &serde_json::json!({ "tick_s": 0.0 }),
    )?;
    write_json_file(&ui_layouts_path(&project_root), &serde_json::json!({}))?;

    let now = now_string();
    let manifest = ProjectManifest {
        project_id: new_id("project"),
        name: scenario_name,
        created_at: now.clone(),
        updated_at: now,
        session_kind: SessionKind::Scenario,
        engine_schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
        ui_schema_version: 2,
        last_opened_run_id: None,
        recent_runs: vec![],
        clock_state: default_clock_for(&SessionKind::Scenario),
        progress_metrics: None,
        start_location: None,
        economy: default_economy_manifest(),
        demand_surface: Some(default_demand_surface_manifest()),
        region_state: RegionStateManifest::default(),
        simulation_scope: default_simulation_scope_manifest(),
        runtime_scheduling: default_runtime_scheduling_manifest(),
        pack_refs: vec![],
    };
    write_manifest(&project_root, &manifest)?;
    update_index_opened(&app, &project_root, &manifest)?;
    open_session_internal(&app, &state, &project_root)
}

#[command]
pub fn list_game_saves(app: AppHandle) -> Result<Vec<GameSaveMeta>, String> {
    bootstrap_legacy_projects(&app)?;
    let idx = read_index(&app)?;
    let mut out = Vec::<GameSaveMeta>::new();
    for ent in idx.projects {
        if ent.session_kind != SessionKind::Game {
            continue;
        }
        let root = PathBuf::from(&ent.project_path);
        if !root.exists() {
            continue;
        }
        let manifest = match read_manifest(&root) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mut metrics = manifest
            .progress_metrics
            .unwrap_or_else(default_progress_metrics);
        metrics.currency = normalize_currency(Some(&metrics.currency));

        let peak_ridership_pph = {
            let fallback = metrics.ridership.max(0.0);
            if fallback.is_finite() {
                Some(fallback)
            } else {
                None
            }
        };
        out.push(GameSaveMeta {
            project_id: manifest.project_id.clone(),
            project_path: ent.project_path,
            name: manifest.name,
            last_opened_at: ent.last_opened_at,
            sim_datetime_utc: manifest.clock_state.sim_datetime_utc,
            sim_tick_seconds: manifest.clock_state.tick_seconds.max(0.0),
            start_country: manifest
                .start_location
                .as_ref()
                .map(|x| x.country_name.clone()),
            start_city: manifest
                .start_location
                .as_ref()
                .map(|x| x.city_name.clone()),
            unlocked_countries: manifest.economy.unlocked_countries.len(),
            network_stops: 0,
            network_links: 0,
            network_services: 0,
            total_link_km: 0.0,
            peak_ridership_pph,
            progress_metrics: metrics,
        });
    }
    out.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
    Ok(out)
}

#[command]
pub fn continue_latest_game(
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<OpenSessionResult, String> {
    let started = Instant::now();
    let saves = list_game_saves(app.clone())?;
    let latest = saves
        .first()
        .ok_or_else(|| "no game saves available".to_string())?;
    let result = open_session_internal(&app, &state, Path::new(&latest.project_path));
    perf_log("command.continue_latest_game", started);
    result
}

#[command]
pub fn load_game_save(
    app: AppHandle,
    state: tauri::State<AppState>,
    save_id: String,
) -> Result<OpenSessionResult, String> {
    let started = Instant::now();
    let idx = read_index(&app)?;
    let entry = idx
        .projects
        .iter()
        .find(|x| x.project_id == save_id)
        .ok_or_else(|| format!("save id not found: {save_id}"))?;
    let result = open_session_internal(&app, &state, Path::new(&entry.project_path));
    perf_log("command.load_game_save", started);
    result
}

#[command]
pub fn open_project(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<OpenSessionResult, String> {
    let started = Instant::now();
    let result = open_session_internal(&app, &state, Path::new(&project_path));
    perf_log("command.open_project", started);
    result
}

#[command]
pub fn list_deleted_saves(app: AppHandle) -> Result<Vec<DeletedSaveMeta>, String> {
    let idx = read_deleted_index(&app)?;
    let mut out = idx
        .entries
        .into_iter()
        .map(|e| DeletedSaveMeta {
            deleted_id: e.deleted_id,
            project_id: e.project_id,
            name: e.name,
            session_kind: e.session_kind,
            deleted_at: e.deleted_at,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    Ok(out)
}

#[command]
pub fn delete_save(app: AppHandle, save_id: String) -> Result<DeleteSaveResult, String> {
    let idx = read_index(&app)?;
    let entry = idx
        .projects
        .iter()
        .find(|x| x.project_id == save_id)
        .ok_or_else(|| format!("save id not found: {save_id}"))?
        .clone();
    let source = PathBuf::from(&entry.project_path);
    if !source.exists() {
        remove_index_entry(&app, &entry.project_id)?;
        return Ok(DeleteSaveResult {
            deleted_id: new_id("deleted"),
            ok: true,
        });
    }
    let deleted_id = new_id("deleted");
    let destination = trash_root(&app)?.join(format!("{deleted_id}.interlinked"));
    fs::rename(&source, &destination).map_err(|e| e.to_string())?;
    remove_index_entry(&app, &entry.project_id)?;

    let mut deleted = read_deleted_index(&app)?;
    deleted.entries.push(DeletedIndexEntry {
        deleted_id: deleted_id.clone(),
        project_id: entry.project_id,
        name: entry.name,
        session_kind: entry.session_kind,
        deleted_at: now_string(),
        trash_path: destination.to_string_lossy().to_string(),
        original_path: entry.project_path,
    });
    write_deleted_index(&app, &deleted)?;
    Ok(DeleteSaveResult {
        deleted_id,
        ok: true,
    })
}

#[command]
pub fn restore_deleted_save(
    app: AppHandle,
    deleted_id: String,
) -> Result<RestoreSaveResult, String> {
    let mut deleted = read_deleted_index(&app)?;
    let pos = deleted
        .entries
        .iter()
        .position(|x| x.deleted_id == deleted_id)
        .ok_or_else(|| format!("deleted id not found: {deleted_id}"))?;
    let entry = deleted.entries.remove(pos);
    let source = PathBuf::from(&entry.trash_path);
    if !source.exists() {
        write_deleted_index(&app, &deleted)?;
        return Err("deleted save payload not found on disk".to_string());
    }
    let mut destination = PathBuf::from(&entry.original_path);
    if destination.exists() {
        destination = projects_root(&app)?.join(format!("restored_{}.interlinked", new_id("save")));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&source, &destination).map_err(|e| e.to_string())?;
    upsert_index_entry(
        &app,
        SaveIndexEntry {
            project_id: entry.project_id.clone(),
            project_path: destination.to_string_lossy().to_string(),
            name: entry.name,
            session_kind: entry.session_kind,
            last_opened_at: now_string(),
        },
    )?;
    write_deleted_index(&app, &deleted)?;
    Ok(RestoreSaveResult {
        project_id: entry.project_id,
        ok: true,
    })
}

#[command]
pub fn purge_deleted_save(app: AppHandle, deleted_id: String) -> Result<PurgeSaveResult, String> {
    let mut deleted = read_deleted_index(&app)?;
    let pos = deleted
        .entries
        .iter()
        .position(|x| x.deleted_id == deleted_id)
        .ok_or_else(|| format!("deleted id not found: {deleted_id}"))?;
    let entry = deleted.entries.remove(pos);
    let source = PathBuf::from(&entry.trash_path);
    if source.exists() {
        fs::remove_dir_all(&source).map_err(|e| e.to_string())?;
    }
    write_deleted_index(&app, &deleted)?;
    Ok(PurgeSaveResult {
        deleted_id,
        ok: true,
    })
}

#[command]
pub fn save_session(
    state: tauri::State<AppState>,
    project_path: String,
    payload: Option<SaveSessionPayload>,
) -> Result<SaveResult, String> {
    let project_root = PathBuf::from(project_path);
    let project_path_string = project_root.to_string_lossy().to_string();
    ensure_project_dirs(&project_root)?;
    let mut written = Vec::<String>::new();
    let mut maybe_saved_scenario: Option<Scenario> = None;
    let mut payload_sandbox_state: Option<JsonValue> = None;

    if let Some(body) = payload {
        if let Some(doc) = body.scenario_document {
            let full_doc = ScenarioDocument {
                schema_version: doc.schema_version,
                scenario: doc.scenario,
            };
            ScenarioService::save_to_path(
                scenario_path(&project_root).to_string_lossy().as_ref(),
                &full_doc,
            )
            .map_err(|e| e.to_string())?;
            written.push(SCENARIO_FILE.to_string());
            maybe_saved_scenario = Some(full_doc.scenario);
        }
        if let Some(state_json) = body.sandbox_state {
            payload_sandbox_state = Some(state_json);
        }
        if let Some(layouts) = body.ui_layouts {
            write_json_file(&ui_layouts_path(&project_root), &layouts)?;
            written.push(UI_LAYOUTS_FILE.to_string());
        }
    }

    let mut manifest = read_manifest(&project_root)?;
    if let Some(scenario) = maybe_saved_scenario.as_ref() {
        let _country_charge = apply_country_entry_charges(&mut manifest, scenario);
    }
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    written.push(MANIFEST_FILE.to_string());

    let mut sandbox_written = false;
    if manifest.session_kind == SessionKind::Game {
        let captured_runtime = capture_persisted_runtime_state(&state, &project_path_string)?;
        if let Some(runtime) = captured_runtime.as_ref() {
            if runtime.tick_s.is_finite() && runtime.tick_s >= 0.0 {
                manifest.clock_state.tick_seconds = runtime.tick_s;
            }
            if let Some(snapshot) = runtime.latest_snapshot.as_ref() {
                manifest.economy.current_balance_base = snapshot.economy.current_balance_base;
                manifest.economy.cumulative_revenue_base = snapshot.economy.cumulative_revenue_base;
                manifest.economy.cumulative_opex_base = snapshot.economy.cumulative_opex_base;
                sync_progress_budget_from_economy(&mut manifest);
            }
            manifest.updated_at = now_string();
            write_manifest(&project_root, &manifest)?;
        }
        let persisted_tick = captured_runtime
            .as_ref()
            .map(|runtime| runtime.tick_s)
            .unwrap_or_else(|| manifest.clock_state.tick_seconds.max(0.0));
        let sandbox_file = PersistedSandboxStateFile {
            tick_s: persisted_tick,
            runtime: captured_runtime,
        };
        write_json_file(&sandbox_state_path(&project_root), &sandbox_file)?;
        written.push(SANDBOX_STATE_FILE.to_string());
        sandbox_written = true;
    }
    if !sandbox_written {
        if let Some(state_json) = payload_sandbox_state {
            write_json_file(&sandbox_state_path(&project_root), &state_json)?;
            written.push(SANDBOX_STATE_FILE.to_string());
        }
    }

    Ok(SaveResult {
        ok: true,
        updated_at: manifest.updated_at,
        written_files: written,
    })
}

#[command]
pub fn save_and_quit(
    state: tauri::State<AppState>,
    project_path: String,
    payload: Option<SaveSessionPayload>,
) -> Result<SaveResult, String> {
    let _ = stop_runtime_loop_internal(state.inner())?;
    let result = save_session(state.clone(), project_path, payload)?;
    let mut guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    *guard = None;
    let mut current = state
        .current_project
        .lock()
        .map_err(|_| "current_project mutex poisoned".to_string())?;
    *current = None;
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
    Ok(result)
}
