use crate::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct SaveIndex {
    pub version: u32,
    pub projects: Vec<SaveIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SaveIndexEntry {
    pub project_id: String,
    pub project_path: String,
    pub name: String,
    pub session_kind: SessionKind,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct DeletedIndex {
    pub version: u32,
    pub entries: Vec<DeletedIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeletedIndexEntry {
    pub deleted_id: String,
    pub project_id: String,
    pub name: String,
    pub session_kind: SessionKind,
    pub deleted_at: String,
    pub trash_path: String,
    pub original_path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyManifest {
    project_id: Option<String>,
    name: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    default_mode: Option<String>,
    engine_schema_version: Option<u32>,
    ui_schema_version: Option<u32>,
    last_opened_run_id: Option<String>,
    recent_runs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct CountryPackIndex {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) packs: Vec<CountryPackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CountryPackEntry {
    pub(crate) country_iso2: String,
    pub(crate) build_state: String,
    #[serde(default)]
    pub(crate) surface_version: Option<String>,
    #[serde(default)]
    pub(crate) cells_count: usize,
    #[serde(default)]
    pub(crate) last_updated_at: Option<String>,
    #[serde(default)]
    pub(crate) checksum: Option<String>,
    #[serde(default)]
    pub(crate) provenance: Option<String>,
}

pub(crate) fn app_root(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let root = base.join(APP_DIR_NAME);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

pub(crate) fn projects_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app_root(app)?.join("projects");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

pub(crate) fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_root(app)?.join(INDEX_FILE_NAME))
}

pub(crate) fn location_catalog_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(LOCATION_CATALOG_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(crate) fn demand_surfaces_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(DEMAND_SURFACE_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(crate) fn country_packs_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(COUNTRY_PACKS_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(crate) fn country_pack_index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(country_packs_root(app)?.join(COUNTRY_PACK_INDEX_FILE))
}

pub(crate) fn trash_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(TRASH_DIR_NAME);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub(crate) fn deleted_index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_root(app)?.join(DELETED_INDEX_FILE_NAME))
}

pub(crate) fn read_country_pack_index(app: &AppHandle) -> Result<CountryPackIndex, String> {
    let path = country_pack_index_path(app)?;
    if !path.exists() {
        return Ok(CountryPackIndex {
            version: 1,
            packs: vec![],
        });
    }
    let mut idx: CountryPackIndex = read_json_file(&path)?;
    idx.version = idx.version.max(1);
    let mut dedup = BTreeMap::<String, CountryPackEntry>::new();
    for mut entry in idx.packs.into_iter() {
        let Some(iso) = canonical_country_iso2(&entry.country_iso2) else {
            continue;
        };
        entry.country_iso2 = iso.clone();
        dedup.insert(iso, entry);
    }
    idx.packs = dedup.into_values().collect();
    idx.packs
        .sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    Ok(idx)
}

pub(crate) fn write_country_pack_index(
    app: &AppHandle,
    idx: &CountryPackIndex,
) -> Result<(), String> {
    write_json_file(&country_pack_index_path(app)?, idx)
}

pub(crate) fn manifest_path(project_root: &Path) -> PathBuf {
    project_root.join(MANIFEST_FILE)
}

pub(crate) fn scenario_path(project_root: &Path) -> PathBuf {
    project_root.join(SCENARIO_FILE)
}

pub(crate) fn sandbox_state_path(project_root: &Path) -> PathBuf {
    project_root.join(SANDBOX_STATE_FILE)
}

pub(crate) fn ui_layouts_path(project_root: &Path) -> PathBuf {
    project_root.join(UI_LAYOUTS_FILE)
}

pub(crate) fn snapshots_dir(project_root: &Path) -> PathBuf {
    project_root.join("sandbox").join("snapshots")
}

pub(crate) fn runs_dir(project_root: &Path) -> PathBuf {
    project_root.join("runs")
}

pub(crate) fn ensure_project_dirs(project_root: &Path) -> Result<(), String> {
    fs::create_dir_all(project_root.join("scenario")).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("sandbox")).map_err(|e| e.to_string())?;
    fs::create_dir_all(snapshots_dir(project_root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(runs_dir(project_root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("ui")).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("assets")).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "json write path has no parent directory".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "json write path has invalid filename".to_string())?;
    let temp_name = format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        now_epoch_ms()
    );
    let temp_path = parent.join(temp_name);
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut temp_file, json.as_bytes()).map_err(|e| e.to_string())?;
    temp_file.sync_all().map_err(|e| e.to_string())?;
    drop(temp_file);
    if let Err(error) = replace_file_atomic(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_file_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    fn to_utf16_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let source_w = to_utf16_null(source.as_os_str());
    let destination_w = to_utf16_null(destination.as_os_str());
    // Replace manifest/index files atomically so readers never observe truncated JSON.
    let moved = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            destination_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|e| e.to_string())
}

pub(crate) fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str::<T>(&raw).map_err(|e| e.to_string())
}

pub(crate) fn read_manifest(project_root: &Path) -> Result<ProjectManifest, String> {
    let path = manifest_path(project_root);
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if let Ok(mut parsed) = serde_json::from_str::<ProjectManifest>(&raw) {
        parsed.clock_state.speed = normalize_speed(parsed.clock_state.speed);
        parsed.simulation_scope.max_active_zones =
            parsed.simulation_scope.max_active_zones.clamp(120, 5000);
        parsed.simulation_scope.remote_regions_mode =
            normalize_scope(&parsed.simulation_scope.remote_regions_mode);
        parsed.simulation_scope.remote_update_interval_ticks = parsed
            .simulation_scope
            .remote_update_interval_ticks
            .max(default_remote_update_interval_ticks());
        parsed.simulation_scope.focus_max_active_zones = parsed
            .simulation_scope
            .focus_max_active_zones
            .clamp(120, 6000);
        parsed.simulation_scope.adjacent_max_active_zones = parsed
            .simulation_scope
            .adjacent_max_active_zones
            .clamp(40, parsed.simulation_scope.focus_max_active_zones);
        parsed.simulation_scope.remote_max_active_zones = parsed
            .simulation_scope
            .remote_max_active_zones
            .clamp(20, parsed.simulation_scope.adjacent_max_active_zones);
        parsed.simulation_scope.adjacent_update_interval_ticks = parsed
            .simulation_scope
            .adjacent_update_interval_ticks
            .max(default_adjacent_update_interval_ticks());
        parsed.runtime_scheduling.fixed_step_s =
            parsed.runtime_scheduling.fixed_step_s.clamp(0.05, 1.0);
        parsed.runtime_scheduling.max_steps_per_cycle =
            parsed.runtime_scheduling.max_steps_per_cycle.clamp(1, 128);
        parsed.runtime_scheduling.checkpoint_interval_ticks =
            parsed.runtime_scheduling.checkpoint_interval_ticks.max(1);
        parsed.runtime_scheduling.snapshot_ring =
            parsed.runtime_scheduling.snapshot_ring.clamp(4, 256);
        parsed.runtime_scheduling.target_tick_ms =
            parsed.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0);
        if parsed.session_kind == SessionKind::Game && parsed.progress_metrics.is_none() {
            parsed.progress_metrics = Some(default_progress_metrics());
        }
        if let Some(metrics) = parsed.progress_metrics.as_mut() {
            metrics.currency = normalize_currency(Some(&metrics.currency));
        }
        parsed.economy.currency = normalize_currency(Some(&parsed.economy.currency));
        if parsed.economy.difficulty.trim().is_empty() {
            parsed.economy.difficulty = default_difficulty_label();
        }
        if parsed
            .economy
            .difficulty_profile
            .profile_id
            .trim()
            .is_empty()
        {
            parsed.economy.difficulty_profile =
                difficulty_profile_for_label(parsed.economy.difficulty.as_str());
        }
        parsed.economy.unlocked_countries =
            canonicalize_country_codes(parsed.economy.unlocked_countries.clone());
        sanitize_economy_manifest(&mut parsed.economy);
        canonicalize_region_ledger(&mut parsed.economy.region_ledger);
        if parsed.demand_surface.is_none() {
            parsed.demand_surface = Some(default_demand_surface_manifest());
        }
        if let Some(ds) = parsed.demand_surface.as_mut() {
            ds.loaded_countries = canonicalize_country_codes(ds.loaded_countries.clone());
        }
        if let Some(start) = parsed.start_location.as_mut() {
            if let Some(canonical_iso2) = canonical_country_iso2(&start.country_iso2) {
                start.country_iso2 = canonical_iso2.clone();
                if !display_country_name(&canonical_iso2).is_empty() {
                    start.country_name = display_country_name(&canonical_iso2).to_string();
                }
            }
        }
        canonicalize_region_state_manifest(&mut parsed.region_state);
        parsed.pack_refs = parsed
            .pack_refs
            .into_iter()
            .filter_map(|mut p| {
                let iso = canonical_country_iso2(&p.country_iso2)?;
                p.country_iso2 = iso;
                Some(p)
            })
            .collect();
        enforce_game_runtime_hardcut(&mut parsed);
        return Ok(parsed);
    }

    if let Ok(mut value) = serde_json::from_str::<JsonValue>(&raw) {
        if let Some(obj) = value.as_object_mut() {
            let session_kind =
                if let Some(kind) = obj.get("session_kind").and_then(JsonValue::as_str) {
                    parse_session_kind(Some(kind))
                } else {
                    parse_session_kind(obj.get("default_mode").and_then(JsonValue::as_str))
                };
            let session_kind_label = match session_kind {
                SessionKind::Game => "game",
                SessionKind::Scenario => "scenario",
            };
            obj.insert(
                "session_kind".to_string(),
                JsonValue::String(session_kind_label.to_string()),
            );

            if !obj.contains_key("project_id") {
                obj.insert(
                    "project_id".to_string(),
                    JsonValue::String(new_id("project")),
                );
            }
            if !obj.contains_key("name") {
                obj.insert(
                    "name".to_string(),
                    JsonValue::String("Interlinked Project".to_string()),
                );
            }
            if !obj.contains_key("created_at") {
                obj.insert("created_at".to_string(), JsonValue::String(now_string()));
            }
            if !obj.contains_key("updated_at") {
                obj.insert("updated_at".to_string(), JsonValue::String(now_string()));
            }
            if !obj.contains_key("engine_schema_version") {
                obj.insert(
                    "engine_schema_version".to_string(),
                    JsonValue::from(ScenarioDocument::CURRENT_SCHEMA_VERSION),
                );
            }
            if !obj.contains_key("ui_schema_version") {
                obj.insert("ui_schema_version".to_string(), JsonValue::from(2));
            }
            if !obj.contains_key("recent_runs") {
                obj.insert("recent_runs".to_string(), JsonValue::Array(Vec::new()));
            }
            if !obj.contains_key("last_opened_run_id") {
                obj.insert("last_opened_run_id".to_string(), JsonValue::Null);
            }
            if !obj.contains_key("start_location") {
                obj.insert("start_location".to_string(), JsonValue::Null);
            }
            if !obj.contains_key("economy") {
                obj.insert(
                    "economy".to_string(),
                    serde_json::to_value(default_economy_manifest()).map_err(|e| e.to_string())?,
                );
            }
            if !obj.contains_key("demand_surface") {
                obj.insert(
                    "demand_surface".to_string(),
                    serde_json::to_value(default_demand_surface_manifest())
                        .map_err(|e| e.to_string())?,
                );
            }
            if !obj.contains_key("region_state") {
                obj.insert(
                    "region_state".to_string(),
                    serde_json::to_value(RegionStateManifest::default())
                        .map_err(|e| e.to_string())?,
                );
            }
            if !obj.contains_key("simulation_scope") {
                obj.insert(
                    "simulation_scope".to_string(),
                    serde_json::to_value(default_simulation_scope_manifest())
                        .map_err(|e| e.to_string())?,
                );
            }
            if !obj.contains_key("runtime_scheduling") {
                obj.insert(
                    "runtime_scheduling".to_string(),
                    serde_json::to_value(default_runtime_scheduling_manifest())
                        .map_err(|e| e.to_string())?,
                );
            }
            if !obj.contains_key("pack_refs") {
                obj.insert("pack_refs".to_string(), JsonValue::Array(Vec::new()));
            }

            if !obj.contains_key("clock_state") {
                obj.insert(
                    "clock_state".to_string(),
                    serde_json::to_value(default_clock_for(&session_kind))
                        .map_err(|e| e.to_string())?,
                );
            }
            if let Some(clock) = obj
                .get_mut("clock_state")
                .and_then(JsonValue::as_object_mut)
            {
                let speed = normalize_speed(parse_speed_value(clock.get("speed")));
                clock.insert("speed".to_string(), JsonValue::from(speed));
                if !clock.contains_key("tick_seconds") {
                    clock.insert("tick_seconds".to_string(), JsonValue::from(0.0));
                }
                if !clock.contains_key("sim_datetime_utc") {
                    clock.insert(
                        "sim_datetime_utc".to_string(),
                        JsonValue::String(DEFAULT_SIM_START_UTC.to_string()),
                    );
                }
                if !clock.contains_key("running") {
                    clock.insert(
                        "running".to_string(),
                        JsonValue::Bool(session_kind == SessionKind::Game),
                    );
                }
            }

            if !obj.contains_key("progress_metrics") {
                let progress = if session_kind == SessionKind::Game {
                    serde_json::to_value(default_progress_metrics()).map_err(|e| e.to_string())?
                } else {
                    JsonValue::Null
                };
                obj.insert("progress_metrics".to_string(), progress);
            }
        }

        if let Ok(mut parsed) = serde_json::from_value::<ProjectManifest>(value) {
            parsed.clock_state.speed = normalize_speed(parsed.clock_state.speed);
            parsed.simulation_scope.max_active_zones =
                parsed.simulation_scope.max_active_zones.clamp(120, 5000);
            parsed.simulation_scope.remote_regions_mode =
                normalize_scope(&parsed.simulation_scope.remote_regions_mode);
            parsed.simulation_scope.remote_update_interval_ticks = parsed
                .simulation_scope
                .remote_update_interval_ticks
                .max(default_remote_update_interval_ticks());
            parsed.simulation_scope.focus_max_active_zones = parsed
                .simulation_scope
                .focus_max_active_zones
                .clamp(120, 6000);
            parsed.simulation_scope.adjacent_max_active_zones = parsed
                .simulation_scope
                .adjacent_max_active_zones
                .clamp(40, parsed.simulation_scope.focus_max_active_zones);
            parsed.simulation_scope.remote_max_active_zones = parsed
                .simulation_scope
                .remote_max_active_zones
                .clamp(20, parsed.simulation_scope.adjacent_max_active_zones);
            parsed.simulation_scope.adjacent_update_interval_ticks = parsed
                .simulation_scope
                .adjacent_update_interval_ticks
                .max(default_adjacent_update_interval_ticks());
            parsed.runtime_scheduling.fixed_step_s =
                parsed.runtime_scheduling.fixed_step_s.clamp(0.05, 1.0);
            parsed.runtime_scheduling.max_steps_per_cycle =
                parsed.runtime_scheduling.max_steps_per_cycle.clamp(1, 128);
            parsed.runtime_scheduling.checkpoint_interval_ticks =
                parsed.runtime_scheduling.checkpoint_interval_ticks.max(1);
            parsed.runtime_scheduling.snapshot_ring =
                parsed.runtime_scheduling.snapshot_ring.clamp(4, 256);
            parsed.runtime_scheduling.target_tick_ms =
                parsed.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0);
            if parsed.session_kind == SessionKind::Game && parsed.progress_metrics.is_none() {
                parsed.progress_metrics = Some(default_progress_metrics());
            }
            if let Some(metrics) = parsed.progress_metrics.as_mut() {
                metrics.currency = normalize_currency(Some(&metrics.currency));
            }
            parsed.economy.currency = normalize_currency(Some(&parsed.economy.currency));
            if parsed.economy.difficulty.trim().is_empty() {
                parsed.economy.difficulty = default_difficulty_label();
            }
            if parsed
                .economy
                .difficulty_profile
                .profile_id
                .trim()
                .is_empty()
            {
                parsed.economy.difficulty_profile =
                    difficulty_profile_for_label(parsed.economy.difficulty.as_str());
            }
            parsed.economy.unlocked_countries =
                canonicalize_country_codes(parsed.economy.unlocked_countries.clone());
            sanitize_economy_manifest(&mut parsed.economy);
            canonicalize_region_ledger(&mut parsed.economy.region_ledger);
            if parsed.demand_surface.is_none() {
                parsed.demand_surface = Some(default_demand_surface_manifest());
            }
            if let Some(ds) = parsed.demand_surface.as_mut() {
                ds.loaded_countries = canonicalize_country_codes(ds.loaded_countries.clone());
            }
            if let Some(start) = parsed.start_location.as_mut() {
                if let Some(canonical_iso2) = canonical_country_iso2(&start.country_iso2) {
                    start.country_iso2 = canonical_iso2.clone();
                    if !display_country_name(&canonical_iso2).is_empty() {
                        start.country_name = display_country_name(&canonical_iso2).to_string();
                    }
                }
            }
            canonicalize_region_state_manifest(&mut parsed.region_state);
            parsed.pack_refs = parsed
                .pack_refs
                .into_iter()
                .filter_map(|mut p| {
                    let iso = canonical_country_iso2(&p.country_iso2)?;
                    p.country_iso2 = iso;
                    Some(p)
                })
                .collect();
            enforce_game_runtime_hardcut(&mut parsed);
            return Ok(parsed);
        }
    }

    let legacy = serde_json::from_str::<LegacyManifest>(&raw).map_err(|e| e.to_string())?;
    let kind = parse_session_kind(legacy.default_mode.as_deref());
    let now = now_string();
    let mut parsed = ProjectManifest {
        project_id: legacy.project_id.unwrap_or_else(|| new_id("project")),
        name: legacy
            .name
            .unwrap_or_else(|| "Interlinked Project".to_string()),
        created_at: legacy.created_at.unwrap_or_else(|| now.clone()),
        updated_at: legacy.updated_at.unwrap_or_else(|| now.clone()),
        session_kind: kind.clone(),
        engine_schema_version: legacy
            .engine_schema_version
            .unwrap_or(ScenarioDocument::CURRENT_SCHEMA_VERSION),
        ui_schema_version: legacy.ui_schema_version.unwrap_or(2),
        last_opened_run_id: legacy.last_opened_run_id,
        recent_runs: legacy.recent_runs.unwrap_or_default(),
        clock_state: default_clock_for(&kind),
        progress_metrics: if kind == SessionKind::Game {
            Some(default_progress_metrics())
        } else {
            None
        },
        start_location: None,
        economy: default_economy_manifest(),
        demand_surface: Some(default_demand_surface_manifest()),
        region_state: RegionStateManifest::default(),
        simulation_scope: default_simulation_scope_manifest(),
        runtime_scheduling: default_runtime_scheduling_manifest(),
        pack_refs: vec![],
    };
    enforce_game_runtime_hardcut(&mut parsed);
    Ok(parsed)
}

pub(crate) fn write_manifest(
    project_root: &Path,
    manifest: &ProjectManifest,
) -> Result<(), String> {
    write_json_file(&manifest_path(project_root), manifest)
}

pub(crate) fn read_index(app: &AppHandle) -> Result<SaveIndex, String> {
    let path = index_path(app)?;
    if !path.exists() {
        return Ok(SaveIndex {
            version: 1,
            projects: vec![],
        });
    }
    read_json_file(&path)
}

pub(crate) fn write_index(app: &AppHandle, idx: &SaveIndex) -> Result<(), String> {
    write_json_file(&index_path(app)?, idx)
}

pub(crate) fn read_deleted_index(app: &AppHandle) -> Result<DeletedIndex, String> {
    let path = deleted_index_path(app)?;
    if !path.exists() {
        return Ok(DeletedIndex {
            version: 1,
            entries: vec![],
        });
    }
    read_json_file(&path)
}

pub(crate) fn write_deleted_index(app: &AppHandle, idx: &DeletedIndex) -> Result<(), String> {
    write_json_file(&deleted_index_path(app)?, idx)
}

pub(crate) fn upsert_index_entry(app: &AppHandle, entry: SaveIndexEntry) -> Result<(), String> {
    let mut idx = read_index(app)?;
    idx.version = 1;
    idx.projects.retain(|p| p.project_id != entry.project_id);
    idx.projects.push(entry);
    idx.projects
        .sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
    write_index(app, &idx)
}

pub(crate) fn remove_index_entry(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let mut idx = read_index(app)?;
    idx.projects.retain(|p| p.project_id != project_id);
    write_index(app, &idx)
}

pub(crate) fn update_index_opened(
    app: &AppHandle,
    project_root: &Path,
    manifest: &ProjectManifest,
) -> Result<(), String> {
    upsert_index_entry(
        app,
        SaveIndexEntry {
            project_id: manifest.project_id.clone(),
            project_path: project_root.to_string_lossy().to_string(),
            name: manifest.name.clone(),
            session_kind: manifest.session_kind.clone(),
            last_opened_at: now_string(),
        },
    )
}

pub(crate) fn load_persisted_sandbox_state(
    project_root: &Path,
) -> Option<PersistedSandboxStateFile> {
    let path = sandbox_state_path(project_root);
    if !path.exists() {
        return None;
    }
    read_json_file::<PersistedSandboxStateFile>(&path).ok()
}
