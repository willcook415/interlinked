#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend_state;
mod contracts;
mod build;
mod builder_support;
mod commands;
mod map_assets;
mod map_county_cache_support;
mod project_persistence;
mod region;
mod region_materialization;
mod runtime;
mod scenario_bootstrap_inspection;
mod service_topology_support;
mod session_bootstrap;
mod simulation_policy;

use geo::algorithm::contains::Contains;
use geo::algorithm::intersects::Intersects;
use geo::algorithm::simplify::Simplify;
use geo::{Coord, Line, LineString, MultiPolygon, Point, Polygon};
use geojson::{GeoJson, Geometry as GeoJsonGeometry, Value as GeoJsonValue};
use h3o::{CellIndex, Resolution};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

pub(crate) use backend_state::*;
use builder_support::{
    apply_build_budget, default_build_defaults, inspect_line_from_scenario,
    inspect_station_from_scenario, materialize_line_operations_for_minute, mutation_cost_breakdown,
    settle_pending_purchase_orders, summarize_network_mutation, BuildDefaults, LineInspection,
    MutationPathValidationMeta, NetworkMutationPreviewResult, NetworkMutationResult,
    NetworkMutationSummary, StationInspection,
};
use commands::build_mutation::{
    apply_network_mutation, inspect_line, inspect_station, load_build_defaults,
    preview_network_mutation,
};
#[cfg(test)]
use commands::build_mutation::{
    bus_path_matches_roads, ferry_path_matches_water, geo_segment_from_points,
};
use commands::content_library::{
    counties_file, country_pack_dir, demand_surface_file, install_country_pack, list_cities,
    list_cities_internal, list_countries, list_country_pack_status, pick_export_path,
    pick_scenario_file, uninstall_country_pack,
};
use commands::map_data_loading::{
    load_country_map_context, load_map_runtime_config, load_region_street_context,
};
use commands::planning_reports::{
    compare_runs, export_scenario_report_csv, export_scenario_report_json, load_scenario,
    run_planning, run_planning_scenario,
};
use commands::project_session_lifecycle::{
    continue_latest_game, create_game, create_scenario, delete_save, import_scenario,
    list_deleted_saves, list_game_saves, list_scenario_saves, load_game_save, load_scenario_save,
    open_project, purge_deleted_save, restore_deleted_save, save_and_quit, save_session,
};
use commands::region_economy::{
    ensure_country_demand_surface, expedite_fleet_delivery, get_demand_layer_stats,
    get_demand_tile_source, get_fare_policy, get_financial_dashboard, list_demand_coverage,
    list_regions, rebuild_demand_for_unlocked, set_fare_policy, set_primary_focus_region,
    set_simulation_scope, unlock_and_focus_region, unlock_region,
};
use commands::runtime_sandbox::{
    advance_simulation, enqueue_runtime_action, get_runtime_fast_snapshot, get_runtime_snapshot,
    get_runtime_strategic_snapshot, load_sandbox_snapshot, save_sandbox_snapshot,
    set_simulation_running, set_simulation_speed, start_runtime_loop, stop_runtime_loop,
};
use interlinked_engine::model::{
    lonlat_to_web_mercator_m, web_mercator_m_to_lonlat, web_mercator_m_to_world_xy,
    world_xy_to_web_mercator_m, Crs, DemandCell, DemandMeta, Link, Meta, Params, PurchaseOrder,
    Scenario, Service, World, Zone,
};
use interlinked_engine::platform::{
    countries_in_scenario, default_economy_config, from_base_currency, normalize_currency_code,
    EconomyConfig, ScenarioDocument, ScenarioService, ScenarioStore, SimulationScope,
    SimulationService,
};
use interlinked_engine::sim::{
    fare_mode_bucket_from_tokens, init_sim_state, FareModeBucket, Kpis, QueueSummary, RunConfig,
    SimHistory, SimulationDelta, SimulationOutput,
};
pub(crate) use map_county_cache_support::*;
pub(crate) use project_persistence::{
    country_packs_root, demand_surfaces_root, ensure_project_dirs, load_persisted_sandbox_state,
    location_catalog_root, manifest_path, projects_root, read_country_pack_index, read_deleted_index,
    read_index, read_json_file, read_manifest, remove_index_entry, runs_dir, sandbox_state_path,
    scenario_path, snapshots_dir, trash_root, ui_layouts_path, update_index_opened,
    upsert_index_entry, write_country_pack_index, write_deleted_index, write_json_file,
    write_manifest, CountryPackEntry, CountryPackIndex, DeletedIndexEntry, SaveIndexEntry,
};
pub(crate) use region_materialization::*;
pub(crate) use runtime::domain::*;
pub(crate) use runtime::persistence::*;
pub(crate) use commands::runtime_sandbox::SandboxSnapshotFile;
pub(crate) use scenario_bootstrap_inspection::*;
pub(crate) use service_topology_support::*;
pub(crate) use simulation_policy::*;
use runtime::snapshots::{
    latest_runtime_fast_snapshot_for_project, latest_runtime_snapshot_for_project,
    latest_runtime_strategic_snapshot_for_project, publish_runtime_snapshots,
    publish_strategic_snapshot_for_tick, runtime_snapshot_from_parts,
};
use runtime::worker_control::{
    enqueue_runtime_action_internal, enqueue_runtime_action_with_retry,
    runtime_control_state_for_project, runtime_loop_matches_project,
    runtime_loop_status_for_project, start_runtime_loop_internal, stop_runtime_loop_internal,
};
pub(crate) use session_bootstrap::*;

const APP_DIR_NAME: &str = "Interlinked";
const INDEX_FILE_NAME: &str = "index.json";
const DELETED_INDEX_FILE_NAME: &str = "deleted_index.json";
const TRASH_DIR_NAME: &str = "trash";
const MANIFEST_FILE: &str = "project.interlinked.json";
const SCENARIO_FILE: &str = "scenario/current.scenario.json";
const SANDBOX_STATE_FILE: &str = "sandbox/state.json";
const UI_LAYOUTS_FILE: &str = "ui/layouts.json";
const DEFAULT_SIM_START_UTC: &str = "2026-01-01T08:00:00Z";
const LOCATION_CATALOG_DIR: &str = "location_catalog";
const DEMAND_SURFACE_DIR: &str = "demand_surfaces";
const COUNTRY_PACKS_DIR: &str = "country_packs";
const COUNTRY_PACK_INDEX_FILE: &str = "index.json";
const ECONOMY_MONTH_SECONDS: f64 = 30.0 * 24.0 * 3600.0;
const ECONOMY_MONTHLY_FINANCIAL_CAP: usize = 24;
const UK_EMPLOYMENT_BASELINE_RATIO: f64 = 0.48;
const DEFAULT_EMPLOYMENT_BASELINE_RATIO: f64 = 0.44;

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_epoch_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn now_string() -> String {
    now_epoch_s().to_string()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", now_epoch_ns())
}

#[cfg(test)]
mod tests {
    use super::*;
    use interlinked_engine::model::{Link, Service, Stop};
    use interlinked_engine::platform::PlanningRunOptions;
    use interlinked_engine::sim::SimulationSettings;
    use std::fs;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("interlinked_tauri_{nanos}_{name}"))
    }

    fn simulate_runtime_scheduler(
        speed: u32,
        iterations: usize,
        real_dt_s: f64,
        fixed_step_s: f64,
        max_steps_per_cycle: usize,
    ) -> (f64, f64) {
        let mut accumulator_s = 0.0_f64;
        let mut game_elapsed_s = 0.0_f64;
        for _ in 0..iterations {
            accumulator_s += real_dt_s * speed as f64;
            let catchup = crate::runtime::scheduling::plan_runtime_catchup(
                accumulator_s,
                fixed_step_s,
                max_steps_per_cycle,
            );
            game_elapsed_s += catchup.steps_to_run as f64 * fixed_step_s;
            accumulator_s =
                (accumulator_s - catchup.steps_to_run as f64 * fixed_step_s).max(0.0_f64);
        }
        (game_elapsed_s, accumulator_s)
    }

    fn test_scenario() -> Scenario {
        Scenario {
            meta: Meta {
                name: "rehydrate-test".to_string(),
                seed: 11,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: default_params(),
            world: World {
                zones: vec![Zone {
                    id: "zone_a".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 1000.0,
                    jobs: 500.0,
                    country_iso2: Some("GB".to_string()),
                }],
                stops: vec![
                    Stop {
                        id: "stop_a".to_string(),
                        name: Some("Alpha".to_string()),
                        x: 0.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "stop_b".to_string(),
                        name: Some("Bravo".to_string()),
                        x: 1000.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                ],
                links: vec![Link {
                    id: "link_ab".to_string(),
                    from_stop: "stop_a".to_string(),
                    to_stop: "stop_b".to_string(),
                    distance_m: 1000.0,
                    mode: "metro".to_string(),
                    speed_mps: 16.0,
                    geometry: None,
                    line_id: Some("line:test".to_string()),
                    mode_variant: None,
                    capacity_per_hour: None,
                }],
                services: vec![Service {
                    id: "svc_keep".to_string(),
                    line_id: Some("line:test".to_string()),
                    name: Some("Test".to_string()),
                    mode: "metro".to_string(),
                    mode_variant: None,
                    stop_sequence: vec!["stop_a".to_string(), "stop_b".to_string()],
                    direction: Some("forward".to_string()),
                    direction_name: Some("Outbound".to_string()),
                    display_color: Some("#123456".to_string()),
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    headway_s: 300.0,
                    dwell_s: 30.0,
                    vehicle_capacity: 500.0,
                    board_penalty_s: None,
                }],
                transfers: vec![],
                transfer_rules: None,
                demand_cells: vec![],
                demand_meta: None,
            },
        }
    }

    fn test_manifest_for_surface(iso: &str) -> ProjectManifest {
        ProjectManifest {
            project_id: "p-test".to_string(),
            name: "Test".to_string(),
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            session_kind: SessionKind::Game,
            engine_schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
            ui_schema_version: 2,
            last_opened_run_id: None,
            recent_runs: vec![],
            clock_state: default_clock_for(&SessionKind::Game),
            progress_metrics: Some(default_progress_metrics()),
            start_location: Some(StartLocation {
                country_iso2: iso.to_string(),
                country_name: "Ireland".to_string(),
                city_id: 1,
                city_name: "Dublin".to_string(),
                city_lon: -6.2603,
                city_lat: 53.3498,
                city_population: Some(1_000_000),
            }),
            economy: EconomyManifest {
                currency: default_currency_code(),
                difficulty: default_difficulty_label(),
                difficulty_profile: difficulty_profile_for_label("standard"),
                economy_revision: 1,
                starting_budget_base: 1_000_000_000.0,
                current_balance_base: 1_000_000_000.0,
                cumulative_capex_base: 0.0,
                cumulative_opex_base: 0.0,
                cumulative_revenue_base: 0.0,
                cumulative_lost_demand_penalty_base: 0.0,
                fare_revenue_deferred_base: 0.0,
                fare_boardings_deferred_pax: 0.0,
                fare_policy: default_fare_policy_manifest(),
                unlocked_countries: vec![iso.to_string()],
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
        }
    }

    fn zero_kpis() -> Kpis {
        Kpis {
            total_trips_attempted: 0.0,
            total_trips_served: 0.0,
            share_trips_served: 0.0,
            total_trips: 0.0,
            mean_generalized_cost_s: 0.0,
            mean_in_vehicle_time_s: 0.0,
            mean_wait_time_s: 0.0,
            mean_walk_time_s: 0.0,
            mean_transfer_time_s: 0.0,
            mean_transfer_penalty_s: 0.0,
            mean_transfers: 0.0,
            mean_boardings: 0.0,
            total_boardings_attempted: 0.0,
            total_boardings_served: 0.0,
            total_boardings_denied: 0.0,
            share_boardings_served: 0.0,
            total_fare_revenue_base: 0.0,
            total_overflow_dropped: 0.0,
            share_demand_overflow_dropped: 0.0,
        }
    }

    #[test]
    fn read_manifest_backfills_runtime_defaults() {
        let root = unique_tmp_path("manifest_runtime_defaults");
        fs::create_dir_all(&root).expect("create temp project root");
        let mut manifest = test_manifest_for_surface("GB");
        manifest.runtime_scheduling = default_runtime_scheduling_manifest();
        manifest.simulation_scope = default_simulation_scope_manifest();
        let mut value = serde_json::to_value(&manifest).expect("serialize manifest");
        let obj = value.as_object_mut().expect("manifest object");
        obj.remove("runtime_scheduling");
        if let Some(scope) = obj
            .get_mut("simulation_scope")
            .and_then(JsonValue::as_object_mut)
        {
            scope.remove("focus_max_active_zones");
            scope.remove("adjacent_max_active_zones");
            scope.remove("remote_max_active_zones");
            scope.remove("adjacent_update_interval_ticks");
        }
        if let Some(econ) = obj.get_mut("economy").and_then(JsonValue::as_object_mut) {
            econ.remove("region_ledger");
        }
        fs::write(
            manifest_path(&root),
            serde_json::to_string_pretty(&value).expect("serialize downgraded manifest"),
        )
        .expect("write downgraded manifest");

        let parsed = read_manifest(&root).expect("read_manifest should succeed");
        assert!(parsed.runtime_scheduling.enabled);
        assert!(parsed.runtime_scheduling.fixed_step_s >= 0.05);
        assert!(parsed.runtime_scheduling.max_steps_per_cycle >= 1);
        assert!(parsed.simulation_scope.focus_max_active_zones >= 120);
        assert!(parsed.simulation_scope.adjacent_max_active_zones >= 40);
        assert!(parsed.simulation_scope.remote_max_active_zones >= 20);
        assert!(parsed.economy.region_ledger.is_empty());
    }

    #[test]
    fn load_surface_wire_migrates_v3_and_rejects_invalid_mix() {
        let base_cell = serde_json::json!({
            "cell_id": "c1",
            "h3_res": 8,
            "lon": -6.2603,
            "lat": 53.3498,
            "x": -697000.0,
            "y": 7047000.0,
            "area_m2": 500000.0,
            "country_iso2": "IE",
            "residents_raw": 100.0,
            "jobs_raw": 80.0,
            "residents_smooth": 100.0,
            "jobs_smooth": 80.0,
            "activity_mix_residential": 0.5,
            "activity_mix_office": 0.2,
            "activity_mix_retail": 0.1,
            "activity_mix_recreation": 0.1,
            "activity_mix_industrial": 0.05,
            "activity_mix_education": 0.03,
            "activity_mix_health": 0.02,
            "quality": 0.8
        });

        let legacy_v3_path = unique_tmp_path("surface_legacy_v3.json");
        let mut legacy_cell = base_cell.clone();
        if let Some(obj) = legacy_cell.as_object_mut() {
            obj.remove("activity_mix_residential");
            obj.remove("activity_mix_office");
            obj.remove("activity_mix_retail");
            obj.remove("activity_mix_recreation");
            obj.remove("activity_mix_industrial");
            obj.remove("activity_mix_education");
            obj.remove("activity_mix_health");
            obj.insert("jobs_raw".to_string(), serde_json::json!(0.0));
            obj.insert("jobs_smooth".to_string(), serde_json::json!(0.0));
        }
        let legacy_v3 = serde_json::json!({
            "country_iso2": "IE",
            "surface_version": "v3",
            "source_provenance": {},
            "cells_res6": [legacy_cell.clone()],
            "cells_res7": [legacy_cell.clone()],
            "cells_res8": [legacy_cell.clone()]
        });
        fs::write(
            &legacy_v3_path,
            serde_json::to_string_pretty(&legacy_v3).expect("serialize legacy-v3"),
        )
        .expect("write legacy-v3");
        let migrated = load_surface_wire(&legacy_v3_path).expect("legacy v3 should migrate");
        assert_eq!(migrated.surface_version, "v4");
        assert!(!migrated.cells_res8.is_empty());
        let migrated_sum = migrated.cells_res8[0].activity_mix_residential
            + migrated.cells_res8[0].activity_mix_office
            + migrated.cells_res8[0].activity_mix_retail
            + migrated.cells_res8[0].activity_mix_recreation
            + migrated.cells_res8[0].activity_mix_industrial
            + migrated.cells_res8[0].activity_mix_education
            + migrated.cells_res8[0].activity_mix_health;
        assert!((migrated_sum - 1.0).abs() < 1e-9);
        assert!(migrated.cells_res8[0].activity_mix_residential < 0.85);
        assert!(migrated.cells_res8[0].activity_mix_office > 0.05);
        assert!(migrated.cells_res8[0].activity_mix_retail > 0.05);

        let bad_version_path = unique_tmp_path("surface_bad_version.json");
        let bad_version = serde_json::json!({
            "country_iso2": "IE",
            "surface_version": "v2",
            "source_provenance": {},
            "cells_res6": [base_cell.clone()],
            "cells_res7": [base_cell.clone()],
            "cells_res8": [base_cell.clone()]
        });
        fs::write(
            &bad_version_path,
            serde_json::to_string_pretty(&bad_version).expect("serialize bad-version"),
        )
        .expect("write bad-version");
        let err =
            load_surface_wire(&bad_version_path).expect_err("unsupported version must be rejected");
        assert!(err.contains("expected v4 or v3"));

        let bad_mix_path = unique_tmp_path("surface_bad_mix.json");
        let mut bad_mix_cell = base_cell.clone();
        bad_mix_cell["activity_mix_office"] = serde_json::json!(-0.1);
        let bad_mix = serde_json::json!({
            "country_iso2": "IE",
            "surface_version": "v4",
            "source_provenance": {},
            "cells_res6": [base_cell.clone()],
            "cells_res7": [base_cell.clone()],
            "cells_res8": [bad_mix_cell]
        });
        fs::write(
            &bad_mix_path,
            serde_json::to_string_pretty(&bad_mix).expect("serialize bad-mix"),
        )
        .expect("write bad-mix");
        let err = load_surface_wire(&bad_mix_path).expect_err("invalid mix should be rejected");
        assert!(err.contains("invalid activity mix"));
    }

    #[test]
    fn landuse_class_profiles_have_expected_biases() {
        let (res_mix, _) = landuse_class_profile("residential").expect("residential profile");
        assert!(res_mix[0] > 0.70);
        let (comm_mix, _) = landuse_class_profile("commercial").expect("commercial profile");
        assert!(comm_mix[1] > comm_mix[0]);
        let (park_mix, _) = landuse_class_profile("park").expect("park profile");
        assert!(park_mix[3] > 0.60);
    }

    #[test]
    fn legacy_scale_detects_underpowered_surfaces() {
        let mut scenario = test_scenario();
        scenario.world.demand_cells = vec![
            DemandCell {
                cell_id: "a".to_string(),
                x: 0.0,
                y: 0.0,
                area_m2: 10_000.0,
                residents_night: 4.0,
                jobs_day: 1.0,
                activity_mix_residential: 0.9,
                activity_mix_office: 0.03,
                activity_mix_retail: 0.03,
                activity_mix_recreation: 0.02,
                activity_mix_industrial: 0.01,
                activity_mix_education: 0.005,
                activity_mix_health: 0.005,
                centrality_score: 0.5,
                data_quality_score: 0.5,
                country_iso2: Some("GB".to_string()),
            },
            DemandCell {
                cell_id: "b".to_string(),
                x: 500.0,
                y: 300.0,
                area_m2: 10_000.0,
                residents_night: 3.0,
                jobs_day: 2.0,
                activity_mix_residential: 0.8,
                activity_mix_office: 0.05,
                activity_mix_retail: 0.05,
                activity_mix_recreation: 0.04,
                activity_mix_industrial: 0.03,
                activity_mix_education: 0.02,
                activity_mix_health: 0.01,
                centrality_score: 0.5,
                data_quality_score: 0.5,
                country_iso2: Some("GB".to_string()),
            },
        ];
        let scale = estimate_legacy_demand_scale(&scenario);
        assert!(scale > 2.0);
    }

    #[test]
    fn materialize_country_surface_scoped_preserves_distinct_cell_mixes() {
        let mut manifest = test_manifest_for_surface("IE");
        let mut scenario = test_scenario();
        scenario.world.zones.clear();
        scenario.world.demand_cells.clear();

        let surface = DemandSurfaceCountryWire {
            country_iso2: "IE".to_string(),
            surface_version: "v4".to_string(),
            source_provenance: serde_json::json!({}),
            cells_res6: vec![DemandSurfaceCellWire {
                cell_id: "r6_a".to_string(),
                h3_res: 6,
                lon: -6.26,
                lat: 53.35,
                x: -697000.0,
                y: 7047000.0,
                area_m2: 3_000_000.0,
                country_iso2: "IE".to_string(),
                residents_raw: 200.0,
                jobs_raw: 200.0,
                residents_smooth: 200.0,
                jobs_smooth: 200.0,
                activity_mix_residential: 0.5,
                activity_mix_office: 0.3,
                activity_mix_retail: 0.1,
                activity_mix_recreation: 0.05,
                activity_mix_industrial: 0.03,
                activity_mix_education: 0.01,
                activity_mix_health: 0.01,
                quality: 0.8,
            }],
            cells_res7: vec![],
            cells_res8: vec![
                DemandSurfaceCellWire {
                    cell_id: "cell_res".to_string(),
                    h3_res: 8,
                    lon: -6.2605,
                    lat: 53.3501,
                    x: -697050.0,
                    y: 7047050.0,
                    area_m2: 500_000.0,
                    country_iso2: "IE".to_string(),
                    residents_raw: 100.0,
                    jobs_raw: 100.0,
                    residents_smooth: 100.0,
                    jobs_smooth: 100.0,
                    activity_mix_residential: 0.85,
                    activity_mix_office: 0.05,
                    activity_mix_retail: 0.05,
                    activity_mix_recreation: 0.02,
                    activity_mix_industrial: 0.01,
                    activity_mix_education: 0.01,
                    activity_mix_health: 0.01,
                    quality: 0.9,
                },
                DemandSurfaceCellWire {
                    cell_id: "cell_office".to_string(),
                    h3_res: 8,
                    lon: -6.2598,
                    lat: 53.3496,
                    x: -696950.0,
                    y: 7046950.0,
                    area_m2: 500_000.0,
                    country_iso2: "IE".to_string(),
                    residents_raw: 100.0,
                    jobs_raw: 100.0,
                    residents_smooth: 100.0,
                    jobs_smooth: 100.0,
                    activity_mix_residential: 0.05,
                    activity_mix_office: 0.85,
                    activity_mix_retail: 0.05,
                    activity_mix_recreation: 0.02,
                    activity_mix_industrial: 0.01,
                    activity_mix_education: 0.01,
                    activity_mix_health: 0.01,
                    quality: 0.9,
                },
            ],
        };

        let loaded =
            materialize_country_surface_scoped(&mut manifest, &mut scenario, "IE", &surface)
                .expect("materialization should succeed");
        assert!(loaded >= 2);
        let a = scenario
            .world
            .demand_cells
            .iter()
            .find(|c| c.cell_id.ends_with(":cell_res"))
            .expect("cell_res should be present");
        let b = scenario
            .world
            .demand_cells
            .iter()
            .find(|c| c.cell_id.ends_with(":cell_office"))
            .expect("cell_office should be present");
        assert!(a.activity_mix_residential > a.activity_mix_office);
        assert!(b.activity_mix_office > b.activity_mix_residential);
    }

    #[test]
    fn rehydrate_game_state_preserves_tick_and_valid_queue_entries() {
        let scenario = test_scenario();
        let doc = ScenarioDocument::new_current(scenario.clone());
        let mut state = SimulationService::init_game_state(&doc);
        state.tick_s = 75.0;
        state.sim_state.t_s = 90.0;
        state
            .sim_state
            .queue
            .insert(("svc_keep".to_string(), "stop_a".to_string()), 7.0);
        state
            .sim_state
            .queue
            .insert(("svc_removed".to_string(), "stop_a".to_string()), 11.0);
        state
            .sim_state
            .time_to_next_departure_s
            .insert(("svc_keep".to_string(), "stop_a".to_string()), 120.0);
        state
            .sim_state
            .time_to_next_departure_s
            .insert(("svc_removed".to_string(), "stop_a".to_string()), 45.0);

        let mut next_scenario = scenario.clone();
        next_scenario.world.stops[1].name = Some("Bravo Central".to_string());

        rehydrate_game_state_scenario(&mut state, &next_scenario);

        assert_eq!(state.tick_s, 75.0);
        assert_eq!(state.sim_state.t_s, 90.0);
        assert_eq!(
            state
                .sim_state
                .queue
                .get(&("svc_keep".to_string(), "stop_a".to_string()))
                .copied(),
            Some(7.0)
        );
        assert!(!state
            .sim_state
            .queue
            .contains_key(&("svc_removed".to_string(), "stop_a".to_string())));
        assert_eq!(
            state
                .sim_state
                .time_to_next_departure_s
                .get(&("svc_keep".to_string(), "stop_a".to_string()))
                .copied(),
            Some(120.0)
        );
        assert!(!state
            .sim_state
            .time_to_next_departure_s
            .contains_key(&("svc_removed".to_string(), "stop_a".to_string())));
        assert_eq!(
            state.store.scenario().world.stops[1].name.as_deref(),
            Some("Bravo Central")
        );
    }

    #[test]
    fn bus_path_validation_requires_road_alignment() {
        let road = geo_segment_from_points((-1.0, 53.0), (-0.98, 53.0)).expect("road segment");
        let layers = vec![Arc::new(CountyModeConstraintData {
            road_segments: vec![road],
            water_polygons: vec![],
            water_segments: vec![],
        })];

        assert!(bus_path_matches_roads(
            &[(-1.0, 53.0), (-0.99, 53.0), (-0.98, 53.0)],
            &layers
        ));
        assert!(!bus_path_matches_roads(
            &[(-1.0, 53.01), (-0.99, 53.01), (-0.98, 53.01)],
            &layers
        ));
    }

    #[test]
    fn ferry_path_validation_requires_water_geometry() {
        let polygon = Polygon::new(
            LineString::from(vec![
                (-1.0, 53.0),
                (-0.98, 53.0),
                (-0.98, 53.02),
                (-1.0, 53.02),
                (-1.0, 53.0),
            ]),
            vec![],
        );
        let shoreline = vec![
            geo_segment_from_points((-1.0, 53.0), (-0.98, 53.0)).expect("segment"),
            geo_segment_from_points((-0.98, 53.0), (-0.98, 53.02)).expect("segment"),
            geo_segment_from_points((-0.98, 53.02), (-1.0, 53.02)).expect("segment"),
            geo_segment_from_points((-1.0, 53.02), (-1.0, 53.0)).expect("segment"),
        ];
        let layers = vec![Arc::new(CountyModeConstraintData {
            road_segments: vec![],
            water_polygons: vec![MultiPolygon(vec![polygon])],
            water_segments: shoreline,
        })];

        assert!(ferry_path_matches_water(
            &[(-0.999, 53.001), (-0.99, 53.01), (-0.981, 53.019)],
            &layers
        ));
        assert!(!ferry_path_matches_water(
            &[(-1.02, 52.99), (-1.01, 53.0), (-1.0, 53.01)],
            &layers
        ));
    }

    #[test]
    fn runtime_scheduler_scales_game_time_for_1x_2x_4x() {
        let real_dt_s = 1.0 / 60.0;
        let iterations = 600; // 10 real seconds
        let fixed_step_s = 0.25;
        let max_steps_per_cycle = 64;
        let real_elapsed_s = real_dt_s * iterations as f64;

        for speed in [1_u32, 2_u32, 4_u32] {
            let (game_elapsed_s, backlog_s) = simulate_runtime_scheduler(
                speed,
                iterations,
                real_dt_s,
                fixed_step_s,
                max_steps_per_cycle,
            );
            let expected_game_s = real_elapsed_s * speed as f64;
            let diff = (game_elapsed_s - expected_game_s).abs();
            let ratio = game_elapsed_s / real_elapsed_s;
            assert!(
                diff <= fixed_step_s + 0.05,
                "speed {speed}x should track target closely: expected {expected_game_s:.3}, got {game_elapsed_s:.3}"
            );
            assert!(
                (ratio - speed as f64).abs() <= 0.05,
                "speed {speed}x ratio should be close to target: got {ratio:.3}"
            );
            assert!(
                backlog_s < fixed_step_s + 1e-9,
                "backlog should stay bounded near one fixed step under sustained capacity"
            );
        }
    }

    #[test]
    fn runtime_scheduler_keeps_backlog_truthful_when_catchup_is_bounded() {
        let fixed_step_s = 0.5;
        let iterations = 8;
        let real_dt_s = 0.5;
        let speed = 4_u32;
        let max_steps_per_cycle = 1;

        let (game_elapsed_s, backlog_s) = simulate_runtime_scheduler(
            speed,
            iterations,
            real_dt_s,
            fixed_step_s,
            max_steps_per_cycle,
        );
        let target_game_s = iterations as f64 * real_dt_s * speed as f64;

        assert!(
            game_elapsed_s < target_game_s,
            "bounded catch-up should lag when overwhelmed"
        );
        assert!(
            backlog_s > 0.0,
            "lag should be preserved as backlog instead of being dropped"
        );
    }

    #[test]
    fn runtime_snapshot_merge_drops_stale_strategic_frame() {
        let manifest = test_manifest_for_surface("GB");
        let mut fast = default_runtime_fast_snapshot_for_manifest("/tmp/test", &manifest, 1);
        fast.telemetry.tick_index = 20;
        let mut strategic =
            default_runtime_strategic_snapshot_for_manifest("/tmp/test", &manifest, 1);
        strategic.telemetry.tick_index = 10;
        strategic.frame = Some(HistoryFrameLite {
            t_s: 10.0,
            kpis: zero_kpis(),
            queue_summary: QueueSummary::default(),
            service_loads: Vec::new(),
        });

        let merged = runtime_snapshot_from_parts(&fast, Some(&strategic));
        assert!(
            merged.frame.is_none(),
            "stale strategic frame must not be exposed as current tick output"
        );

        let mut strategic_fresh = strategic.clone();
        strategic_fresh.telemetry.tick_index = fast.telemetry.tick_index;
        let fresh = runtime_snapshot_from_parts(&fast, Some(&strategic_fresh));
        assert!(
            fresh.frame.is_some(),
            "matched strategic tick should retain frame payload"
        );
    }

    #[test]
    fn runtime_snapshot_merge_preserves_fast_clock_ownership() {
        let manifest = test_manifest_for_surface("GB");
        let mut fast = default_runtime_fast_snapshot_for_manifest("/tmp/test", &manifest, 1);
        fast.telemetry.tick_index = 20;
        fast.clock.tick_seconds = 7_200.0;
        fast.clock.running = true;
        fast.clock.speed = 4;

        let mut strategic =
            default_runtime_strategic_snapshot_for_manifest("/tmp/test", &manifest, 9);
        strategic.telemetry.tick_index = 999;
        strategic.clock.tick_seconds = 3_600.0;
        strategic.clock.running = false;
        strategic.clock.speed = 1;

        let merged = runtime_snapshot_from_parts(&fast, Some(&strategic));
        assert!(
            (merged.clock.tick_seconds - fast.clock.tick_seconds).abs() < 1e-9,
            "merged runtime snapshot must retain fast-clock tick authority"
        );
        assert_eq!(
            merged.clock.running, fast.clock.running,
            "strategic snapshot must not overwrite fast running state"
        );
        assert_eq!(
            merged.clock.speed, fast.clock.speed,
            "strategic snapshot must not overwrite fast speed state"
        );
    }

    #[test]
    fn strategic_snapshot_publication_requires_executed_refresh() {
        let manifest = test_manifest_for_surface("GB");
        let mut snapshot = default_runtime_snapshot_for_manifest("/tmp/test", &manifest, 1);
        snapshot.telemetry.engine_strategic_refresh_executed = false;
        assert!(
            !publish_strategic_snapshot_for_tick(&snapshot),
            "strategic publish must stay off when no strategic refresh executed"
        );

        snapshot.telemetry.engine_strategic_refresh_executed = true;
        assert!(
            publish_strategic_snapshot_for_tick(&snapshot),
            "strategic publish must turn on when strategic refresh executed"
        );
    }

    #[test]
    fn runtime_scheduling_defaults_include_strategic_refresh_interval() {
        let scheduling = default_runtime_scheduling_manifest();
        assert!(
            scheduling.strategic_refresh_interval_ticks >= 1,
            "strategic refresh interval must be positive"
        );
    }

    #[test]
    fn planning_service_and_stateful_paths_are_aligned_for_equivalent_context() {
        let scenario = test_scenario();
        let doc = ScenarioDocument {
            schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
            scenario: scenario.clone(),
        };

        let mut settings = SimulationSettings::from_params(&scenario.params);
        settings.time_bin_s = 240.0;
        let opts = PlanningRunOptions {
            settings_override: Some(settings.clone()),
            deterministic_mode: true,
            deterministic_seed: Some(77),
            time_of_day_s: None,
            service_day_type: Some(interlinked_engine::sim::ServiceDayType::Weekday),
            seasonal_profile: Some(interlinked_engine::sim::SeasonalProfile::Neutral),
            active_event_ids: Some(Vec::new()),
        };
        let service_output =
            SimulationService::run_planning(&doc, opts.clone()).expect("service planning output");

        let run_cfg = RunConfig {
            deterministic_mode: true,
            deterministic_seed: Some(77),
            time_bin_s: settings.time_bin_s,
            clock_start_s: 12.0 * 3600.0,
            service_day_type: opts.service_day_type,
            seasonal_profile: opts.seasonal_profile,
            active_event_ids: opts.active_event_ids.clone(),
            ..Default::default()
        };
        let (stateful_output, _) =
            interlinked_engine::sim::run_planning_stateful(&scenario, &run_cfg, None)
                .expect("stateful planning output");

        assert!((service_output.kpis.total_trips - stateful_output.kpis.total_trips).abs() < 1e-6);
        assert!(
            (service_output.kpis.share_trips_served - stateful_output.kpis.share_trips_served)
                .abs()
                < 1e-6
        );
        assert!(
            (service_output.kpis.mean_generalized_cost_s
                - stateful_output.kpis.mean_generalized_cost_s)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn runtime_fare_mapping_uses_canonical_mode_buckets() {
        let mut policy = default_fare_policy_manifest();
        policy.enabled = true;
        assert_eq!(
            runtime_fare_base_per_boarding(&policy, "regional_rail"),
            policy.fare_mode_rail_base
        );
        assert_eq!(
            runtime_fare_base_per_boarding(&policy, "commuter_rail"),
            policy.fare_mode_rail_base
        );
        assert_eq!(
            runtime_fare_base_per_boarding(&policy, "high_speed_rail"),
            policy.fare_mode_rail_base
        );
        assert_eq!(
            runtime_fare_base_per_boarding(&policy, "ferry"),
            policy.fare_mode_ferry_base
        );
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            game: Mutex::new(None),
            current_project: Mutex::new(None),
            runtime_tick: Mutex::new(None),
            runtime_loop: Mutex::new(None),
            runtime_snapshots: Mutex::new(VecDeque::new()),
            runtime_fast_snapshots: Mutex::new(VecDeque::new()),
            runtime_strategic_snapshots: Mutex::new(VecDeque::new()),
            runtime_materialization: Mutex::new(None),
            runtime_ops: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            list_countries,
            list_cities,
            list_country_pack_status,
            install_country_pack,
            uninstall_country_pack,
            create_game,
            create_scenario,
            import_scenario,
            pick_scenario_file,
            pick_export_path,
            continue_latest_game,
            list_game_saves,
            list_deleted_saves,
            load_game_save,
            open_project,
            list_scenario_saves,
            load_scenario_save,
            delete_save,
            restore_deleted_save,
            purge_deleted_save,
            load_build_defaults,
            preview_network_mutation,
            apply_network_mutation,
            save_session,
            inspect_station,
            inspect_line,
            save_and_quit,
            start_runtime_loop,
            stop_runtime_loop,
            get_runtime_snapshot,
            get_runtime_fast_snapshot,
            get_runtime_strategic_snapshot,
            enqueue_runtime_action,
            set_simulation_speed,
            set_simulation_running,
            get_fare_policy,
            set_fare_policy,
            expedite_fleet_delivery,
            get_financial_dashboard,
            advance_simulation,
            save_sandbox_snapshot,
            load_sandbox_snapshot,
            run_planning,
            export_scenario_report_csv,
            export_scenario_report_json,
            compare_runs,
            ensure_country_demand_surface,
            list_demand_coverage,
            load_map_runtime_config,
            load_country_map_context,
            load_region_street_context,
            rebuild_demand_for_unlocked,
            list_regions,
            unlock_region,
            unlock_and_focus_region,
            set_primary_focus_region,
            set_simulation_scope,
            get_demand_tile_source,
            get_demand_layer_stats,
            load_scenario,
            run_planning_scenario
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
