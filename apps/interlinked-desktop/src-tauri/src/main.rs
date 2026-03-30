#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod builder_support;
mod commands;
mod map_assets;
mod runtime;

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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{command, AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use builder_support::{
    apply_build_budget, default_build_defaults, inspect_line_from_scenario,
    inspect_station_from_scenario, materialize_line_operations_for_minute, mutation_cost_breakdown,
    settle_pending_purchase_orders, summarize_network_mutation, BuildDefaults, LineInspection,
    MutationPathValidationMeta, NetworkMutationPreviewResult, NetworkMutationResult,
    NetworkMutationSummary, StationInspection,
};
use commands::planning_reports::{
    compare_runs, export_scenario_report_csv, export_scenario_report_json, load_scenario,
    run_planning, run_planning_scenario,
};
use map_assets::server::ensure_map_asset_server;
use runtime::snapshots::{
    latest_runtime_fast_snapshot_for_project, latest_runtime_snapshot_for_project,
    latest_runtime_strategic_snapshot_for_project, publish_runtime_snapshots,
    publish_strategic_snapshot_for_tick, runtime_snapshot_from_parts,
};
use runtime::worker_control::{
    enqueue_runtime_action_internal, enqueue_runtime_action_with_retry, runtime_control_state_for_project,
    runtime_loop_matches_project, runtime_loop_status_for_project, start_runtime_loop_internal,
    stop_runtime_loop_internal,
};
use interlinked_engine::model::{
    lonlat_to_web_mercator_m, web_mercator_m_to_lonlat, web_mercator_m_to_world_xy,
    world_xy_to_web_mercator_m, Crs, DemandCell, DemandMeta, Link, Meta, Params, PurchaseOrder,
    Scenario, Service, World, Zone,
};
use interlinked_engine::platform::{
    countries_in_scenario, default_economy_config, from_base_currency, normalize_currency_code,
    scenario_network_stats, to_base_currency, EconomyConfig, ScenarioDocument, ScenarioService,
    ScenarioStore, SimulationScope, SimulationService,
};
use interlinked_engine::sim::{
    fare_mode_bucket_from_tokens, init_sim_state, FareModeBucket, Kpis, QueueSummary, RunConfig,
    SimHistory, SimulationDelta, SimulationOutput,
};

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
const AUTO_REVERSE_SERVICE_PREFIX: &str = "auto_reverse::";
const AUTO_REVERSE_LINK_PREFIX: &str = "auto_reverse_link::";
const ECONOMY_MONTH_SECONDS: f64 = 30.0 * 24.0 * 3600.0;
const ECONOMY_MONTHLY_FINANCIAL_CAP: usize = 24;
const UK_EMPLOYMENT_BASELINE_RATIO: f64 = 0.48;
const DEFAULT_EMPLOYMENT_BASELINE_RATIO: f64 = 0.44;
const FLEET_EXPEDITE_MULTIPLIER: f64 = 1.75;
const FLEET_EXPEDITE_MIN_SURCHARGE_BASE: f64 = 100_000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioDocumentLite {
    pub schema_version: u32,
    pub scenario: Scenario,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Game,
    Scenario,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    Standard,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyProfile {
    #[serde(default)]
    pub profile_id: String,
    #[serde(default = "default_profile_multiplier_missing")]
    pub demand_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub capex_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub opex_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub maintenance_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub penalty_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub ancillary_revenue_mult: f64,
    #[serde(default = "default_profile_multiplier_missing")]
    pub unlock_cost_mult: f64,
}

impl Default for DifficultyProfile {
    fn default() -> Self {
        difficulty_profile_for_label("standard")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationClock {
    pub sim_datetime_utc: String,
    pub tick_seconds: f64,
    pub running: bool,
    pub speed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartLocation {
    pub country_iso2: String,
    pub country_name: String,
    pub city_id: i64,
    pub city_name: String,
    pub city_lon: f64,
    pub city_lat: f64,
    #[serde(default)]
    pub city_population: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProgressMetrics {
    pub budget: f64,
    #[serde(default = "default_currency_code")]
    pub currency: String,
    pub ridership: f64,
    pub coverage: f64,
    pub milestones: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyManifest {
    #[serde(default = "default_currency_code")]
    pub currency: String,
    #[serde(default = "default_difficulty_label")]
    pub difficulty: String,
    pub starting_budget_base: f64,
    pub current_balance_base: f64,
    pub cumulative_capex_base: f64,
    pub cumulative_opex_base: f64,
    #[serde(default)]
    pub cumulative_revenue_base: f64,
    #[serde(default)]
    pub cumulative_lost_demand_penalty_base: f64,
    #[serde(default)]
    pub difficulty_profile: DifficultyProfile,
    #[serde(default = "default_economy_revision")]
    pub economy_revision: u64,
    #[serde(default)]
    pub fare_revenue_deferred_base: f64,
    #[serde(default)]
    pub fare_boardings_deferred_pax: f64,
    #[serde(default = "default_fare_policy_manifest")]
    pub fare_policy: FarePolicyManifest,
    #[serde(default)]
    pub unlocked_countries: Vec<String>,
    #[serde(default)]
    pub region_ledger: BTreeMap<String, RegionEconomyLedger>,
    #[serde(default = "default_maintenance_rate")]
    pub maintenance_rate: f64,
    #[serde(default = "default_ancillary_revenue_rate")]
    pub ancillary_revenue_rate: f64,
    #[serde(default = "default_quality_penalty_rates")]
    pub quality_penalty_rates: QualityPenaltyRates,
    #[serde(default)]
    pub monthly_financials: Vec<MonthlyFinancialSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegionEconomyLedger {
    #[serde(default)]
    pub revenue_base: f64,
    #[serde(default)]
    pub opex_base: f64,
    #[serde(default)]
    pub capex_base: f64,
    #[serde(default)]
    pub penalties_base: f64,
    #[serde(default)]
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPenaltyRates {
    #[serde(default = "default_overcrowding_penalty_rate")]
    pub overcrowding_base_per_passenger: f64,
    #[serde(default = "default_reliability_penalty_rate")]
    pub reliability_base_per_passenger: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyFinancialSnapshot {
    pub month_index: u64,
    #[serde(default)]
    pub revenue_base: f64,
    #[serde(default)]
    pub opex_base: f64,
    #[serde(default)]
    pub capex_base: f64,
    #[serde(default)]
    pub penalties_base: f64,
    #[serde(default)]
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarePolicyManifest {
    #[serde(default = "default_fare_enabled_manifest")]
    pub enabled: bool,
    #[serde(default = "default_fare_mode_bus_base")]
    pub fare_mode_bus_base: f64,
    #[serde(default = "default_fare_mode_tram_base")]
    pub fare_mode_tram_base: f64,
    #[serde(default = "default_fare_mode_metro_base")]
    pub fare_mode_metro_base: f64,
    #[serde(default = "default_fare_mode_rail_base")]
    pub fare_mode_rail_base: f64,
    #[serde(default = "default_fare_mode_ferry_base")]
    pub fare_mode_ferry_base: f64,
    #[serde(default = "default_fare_mode_default_base")]
    pub fare_mode_default_base: f64,
    #[serde(default = "default_fare_transfer_window_s")]
    pub transfer_window_s: f64,
    #[serde(default = "default_fare_free_transfers_per_trip")]
    pub free_transfers_per_trip: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FarePolicyPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub fare_mode_bus_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_tram_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_metro_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_rail_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_ferry_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_default_base: Option<f64>,
    #[serde(default)]
    pub transfer_window_s: Option<f64>,
    #[serde(default)]
    pub free_transfers_per_trip: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSurfaceManifest {
    pub surface_version: String,
    #[serde(default)]
    pub loaded_countries: Vec<String>,
    pub pack_version: Option<String>,
    pub last_rebuild_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegionStateManifest {
    #[serde(default)]
    pub unlocked_region_ids: Vec<String>,
    pub primary_focus_region_id: Option<String>,
    #[serde(default)]
    pub active_region_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScopeManifest {
    #[serde(default = "default_max_active_zones")]
    pub max_active_zones: usize,
    #[serde(default = "default_remote_regions_mode")]
    pub remote_regions_mode: String,
    #[serde(default = "default_remote_update_interval_ticks")]
    pub remote_update_interval_ticks: u32,
    #[serde(default = "default_focus_max_active_zones")]
    pub focus_max_active_zones: usize,
    #[serde(default = "default_adjacent_max_active_zones")]
    pub adjacent_max_active_zones: usize,
    #[serde(default = "default_remote_max_active_zones")]
    pub remote_max_active_zones: usize,
    #[serde(default = "default_adjacent_update_interval_ticks")]
    pub adjacent_update_interval_ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSchedulingManifest {
    #[serde(default = "default_runtime_enabled")]
    pub enabled: bool,
    #[serde(default = "default_runtime_fixed_step_s")]
    pub fixed_step_s: f64,
    #[serde(default = "default_runtime_max_steps_per_cycle")]
    pub max_steps_per_cycle: u32,
    #[serde(default = "default_runtime_checkpoint_interval_ticks")]
    pub checkpoint_interval_ticks: u32,
    #[serde(default = "default_runtime_snapshot_ring")]
    pub snapshot_ring: usize,
    #[serde(default = "default_runtime_target_tick_ms")]
    pub target_tick_ms: f64,
    #[serde(default = "default_runtime_strategic_refresh_interval_ticks")]
    pub strategic_refresh_interval_ticks: u32,
    #[serde(default = "default_runtime_lightweight_tick_outputs")]
    pub lightweight_tick_outputs: bool,
    #[serde(default = "default_runtime_ops_kernel_v1")]
    pub runtime_ops_kernel_v1: bool,
    #[serde(default = "default_ui_runtime_trains_v1")]
    pub ui_runtime_trains_v1: bool,
    #[serde(default = "default_fare_recognition_v1")]
    pub fare_recognition_v1: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryPackRef {
    pub country_iso2: String,
    #[serde(default)]
    pub surface_version: Option<String>,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project_id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub session_kind: SessionKind,
    pub engine_schema_version: u32,
    pub ui_schema_version: u32,
    pub last_opened_run_id: Option<String>,
    pub recent_runs: Vec<String>,
    pub clock_state: SimulationClock,
    pub progress_metrics: Option<GameProgressMetrics>,
    pub start_location: Option<StartLocation>,
    pub economy: EconomyManifest,
    #[serde(default)]
    pub demand_surface: Option<DemandSurfaceManifest>,
    #[serde(default)]
    pub region_state: RegionStateManifest,
    #[serde(default = "default_simulation_scope_manifest")]
    pub simulation_scope: SimulationScopeManifest,
    #[serde(default = "default_runtime_scheduling_manifest")]
    pub runtime_scheduling: RuntimeSchedulingManifest,
    #[serde(default)]
    pub pack_refs: Vec<CountryPackRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSessionResult {
    pub project_path: String,
    pub manifest: ProjectManifest,
    pub scenario: ScenarioDocumentLite,
    pub runs: Vec<RunMeta>,
    pub snapshots: Vec<SnapshotMeta>,
    pub clock: SimulationClock,
    pub start_location: Option<StartLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveResult {
    pub ok: bool,
    pub updated_at: String,
    pub written_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetDeliveryExpediteResult {
    pub line_id: String,
    pub order_id: String,
    pub delivered_units: u32,
    pub remaining_order_units: u32,
    pub expedite_cost_base: f64,
    pub balance_after_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub snapshot_id: String,
    pub name: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub tick_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStateLite {
    pub snapshot: SnapshotMeta,
    pub scenario: ScenarioDocumentLite,
    pub history_frames: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningRunConfig {
    pub deterministic_seed: Option<u64>,
    pub horizon_s: Option<f64>,
    pub time_bin_s: Option<f64>,
    pub time_of_day_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub run_id: String,
    pub created_at: String,
    pub scenario_name: String,
    pub seed: u64,
    pub horizon_s: f64,
    pub time_bin_s: f64,
    pub time_of_day_s: Option<f64>,
    pub output_path: String,
    pub summary_path: String,
    pub meta_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub total_trips: f64,
    pub share_trips_served: f64,
    pub mean_generalized_cost_s: f64,
    pub mean_wait_time_s: f64,
    pub total_boardings_denied: f64,
    pub estimated_capex: f64,
    pub estimated_opex_per_hour: f64,
    pub country_entry_charges: f64,
    pub projected_net_balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub run_id: String,
    pub out_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KpiSlice {
    pub total_trips: f64,
    pub share_trips_served: f64,
    pub mean_generalized_cost_s: f64,
    pub mean_wait_time_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub base_run_id: String,
    pub candidate_run_id: String,
    pub base: KpiSlice,
    pub candidate: KpiSlice,
    pub delta: SimulationDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinancialDashboardRequest {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub line_id: Option<String>,
    #[serde(default)]
    pub region_id: Option<String>,
    #[serde(default)]
    pub granularity: Option<String>,
    #[serde(default)]
    pub periods: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialDashboardPoint {
    pub period_index: i64,
    pub label: String,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialModeBreakdownRow {
    pub mode: String,
    pub lines: usize,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialLineBreakdownRow {
    pub line_id: String,
    pub line_name: String,
    pub mode: String,
    pub region_id: Option<String>,
    pub estimated_capex_base: f64,
    pub estimated_opex_per_hour_base: f64,
    pub staff_opex_per_hour_base: f64,
    pub fleet_value_base: f64,
    pub units_owned: usize,
    pub units_pending: usize,
    pub units_assigned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialRegionBreakdownRow {
    pub region_id: String,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialDashboardResponse {
    pub currency: String,
    pub granularity: String,
    pub periods: usize,
    pub current_balance_base: f64,
    pub total_revenue_base: f64,
    pub total_opex_base: f64,
    pub total_capex_base: f64,
    pub total_penalties_base: f64,
    pub total_net_base: f64,
    pub points: Vec<FinancialDashboardPoint>,
    pub mode_breakdown: Vec<FinancialModeBreakdownRow>,
    pub line_breakdown: Vec<FinancialLineBreakdownRow>,
    pub region_breakdown: Vec<FinancialRegionBreakdownRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandLayerStats {
    pub cells: usize,
    pub residents_min: f64,
    pub residents_p50: f64,
    pub residents_max: f64,
    pub jobs_min: f64,
    pub jobs_p50: f64,
    pub jobs_max: f64,
    pub activity_min: f64,
    pub activity_p50: f64,
    pub activity_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameCreatePayload {
    pub name: String,
    pub country_iso2: String,
    pub country_name: String,
    pub city_id: i64,
    pub city_name: String,
    pub city_lon: f64,
    pub city_lat: f64,
    #[serde(default)]
    pub city_population: Option<u64>,
    pub difficulty: Difficulty,
    #[serde(default)]
    pub currency: Option<String>,
    pub starting_budget: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioCreatePayload {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSaveMeta {
    pub project_id: String,
    pub project_path: String,
    pub name: String,
    pub last_opened_at: String,
    pub sim_datetime_utc: String,
    pub start_country: Option<String>,
    pub start_city: Option<String>,
    pub unlocked_countries: usize,
    pub network_stops: usize,
    pub network_links: usize,
    pub network_services: usize,
    pub total_link_km: f64,
    pub progress_metrics: GameProgressMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSaveMeta {
    pub project_id: String,
    pub project_path: String,
    pub name: String,
    pub last_opened_at: String,
    pub latest_run_id: Option<String>,
    pub latest_run_created_at: Option<String>,
    pub latest_share_trips_served: Option<f64>,
    pub latest_mean_generalized_cost_s: Option<f64>,
    pub latest_total_boardings_denied: Option<f64>,
    pub latest_projected_net_balance: Option<f64>,
    pub start_country: Option<String>,
    pub start_city: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryOption {
    pub iso2: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CityOption {
    pub geonameid: i64,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub population: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSessionPayload {
    pub scenario_document: Option<ScenarioDocumentLite>,
    pub sandbox_state: Option<JsonValue>,
    pub ui_layouts: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationAdvanceResult {
    pub frame: HistoryFrameLite,
    pub clock: SimulationClock,
    pub economy: SimulationAdvanceEconomy,
    pub delta_revenue_base: f64,
    pub delta_opex_base: f64,
    pub delta_net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLoopStatus {
    pub project_path: String,
    pub running: bool,
    #[serde(default = "default_sim_speed")]
    pub speed: u32,
    #[serde(default)]
    pub clock_revision: u64,
    pub queue_depth: usize,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimePerfTelemetry {
    #[serde(default)]
    pub tick_index: u64,
    #[serde(default)]
    pub dt_s: f64,
    #[serde(default)]
    pub fixed_step_s: f64,
    #[serde(default)]
    pub stage_prepare_ms: f64,
    #[serde(default)]
    pub stage_step_ms: f64,
    #[serde(default)]
    pub stage_economy_ms: f64,
    #[serde(default)]
    pub stage_runtime_ops_ms: f64,
    #[serde(default)]
    pub tick_total_ms: f64,
    #[serde(default)]
    pub snapshot_publish_ms: f64,
    #[serde(default)]
    pub fast_snapshot_bytes: usize,
    #[serde(default)]
    pub strategic_snapshot_bytes: usize,
    #[serde(default)]
    pub queue_depth: usize,
    #[serde(default)]
    pub snapshot_age_ms: u64,
    #[serde(default)]
    pub dropped_steps: u32,
    #[serde(default)]
    pub executed_steps_this_cycle: u32,
    #[serde(default)]
    pub max_steps_per_cycle: u32,
    #[serde(default)]
    pub backlog_steps: u32,
    #[serde(default)]
    pub backlog_s: f64,
    #[serde(default)]
    pub accumulator_s: f64,
    #[serde(default)]
    pub cycle_elapsed_ms: f64,
    #[serde(default)]
    pub avg_cycle_elapsed_ms: f64,
    #[serde(default)]
    pub avg_sim_step_ms: f64,
    #[serde(default)]
    pub real_elapsed_s: f64,
    #[serde(default)]
    pub game_elapsed_s: f64,
    #[serde(default)]
    pub target_game_elapsed_s: f64,
    #[serde(default)]
    pub target_speed_ratio: f64,
    #[serde(default)]
    pub achieved_speed_ratio: f64,
    #[serde(default)]
    pub achieved_vs_target_ratio: f64,
    #[serde(default)]
    pub under_sustained_speed: bool,
    #[serde(default)]
    pub adaptive_max_active_zones: usize,
    #[serde(default)]
    pub strategic_refresh_due: bool,
    #[serde(default)]
    pub strategic_refresh_interval_ticks: u32,
    #[serde(default)]
    pub runtime_views_materialized: bool,
    #[serde(default)]
    pub engine_fast_steps: u64,
    #[serde(default)]
    pub engine_strategic_steps: u64,
    #[serde(default)]
    pub engine_fast_last_ms: f64,
    #[serde(default)]
    pub engine_strategic_last_ms: f64,
    #[serde(default)]
    pub engine_fast_avg_ms: f64,
    #[serde(default)]
    pub engine_strategic_avg_ms: f64,
    #[serde(default)]
    pub engine_steps_since_last_strategic: u32,
    #[serde(default)]
    pub engine_strategic_cache_hits: u64,
    #[serde(default)]
    pub engine_strategic_cache_misses: u64,
    #[serde(default)]
    pub engine_strategic_refresh_executed: bool,
    #[serde(default)]
    pub engine_strategic_refresh_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeFastSnapshot {
    pub project_path: String,
    #[serde(default)]
    pub clock_revision: u64,
    pub clock: SimulationClock,
    pub captured_at_epoch_ms: u64,
    pub telemetry: RuntimePerfTelemetry,
    #[serde(default)]
    pub trains: Vec<TrainRuntimeView>,
    #[serde(default)]
    pub stations: Vec<StationRuntimeView>,
    #[serde(default)]
    pub line_ops: Vec<LineOpsRuntimeView>,
    #[serde(default)]
    pub provenance_warnings: Vec<String>,
    #[serde(default)]
    pub trains_authoritative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStrategicSnapshot {
    pub project_path: String,
    #[serde(default)]
    pub clock_revision: u64,
    pub clock: SimulationClock,
    pub economy: SimulationAdvanceEconomy,
    #[serde(default)]
    pub frame: Option<HistoryFrameLite>,
    #[serde(default)]
    pub delta_revenue_base: f64,
    #[serde(default)]
    pub delta_opex_base: f64,
    #[serde(default)]
    pub delta_net_base: f64,
    pub captured_at_epoch_ms: u64,
    pub telemetry: RuntimePerfTelemetry,
    #[serde(default)]
    pub provenance_warnings: Vec<String>,
    #[serde(default)]
    pub trains_authoritative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub project_path: String,
    #[serde(default)]
    pub clock_revision: u64,
    pub clock: SimulationClock,
    pub economy: SimulationAdvanceEconomy,
    #[serde(default)]
    pub frame: Option<HistoryFrameLite>,
    #[serde(default)]
    pub delta_revenue_base: f64,
    #[serde(default)]
    pub delta_opex_base: f64,
    #[serde(default)]
    pub delta_net_base: f64,
    pub captured_at_epoch_ms: u64,
    pub telemetry: RuntimePerfTelemetry,
    #[serde(default)]
    pub trains: Vec<TrainRuntimeView>,
    #[serde(default)]
    pub stations: Vec<StationRuntimeView>,
    #[serde(default)]
    pub line_ops: Vec<LineOpsRuntimeView>,
    #[serde(default)]
    pub provenance_warnings: Vec<String>,
    #[serde(default)]
    pub trains_authoritative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainRuntimeView {
    pub train_id: String,
    pub service_id: String,
    pub line_id: String,
    pub line_name: String,
    pub vehicle_ordinal: u32,
    pub direction_label: String,
    pub destination_stop_id: String,
    pub destination_label: String,
    pub mode: String,
    #[serde(default)]
    pub mode_variant: Option<String>,
    #[serde(default)]
    pub stock_tier_id: Option<String>,
    pub vehicle_capacity: f64,
    pub onboard_pax: f64,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub at_stop_id: Option<String>,
    pub in_motion: bool,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationRuntimeView {
    pub stop_id: String,
    pub current_inside_pax: f64,
    pub capacity_pax: f64,
    pub declined_last_hour: f64,
    pub entries_per_hour: f64,
    pub exits_per_hour: f64,
    pub avg_wait_to_board_s: f64,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineOpsRuntimeView {
    pub line_id: String,
    pub active_trains: u32,
    #[serde(default)]
    pub boardings_attempted_per_hour: f64,
    pub boarded_per_hour: f64,
    pub alighted_per_hour: f64,
    pub denied_boardings_per_hour: f64,
    #[serde(default)]
    pub queue_end_pax: f64,
    pub mean_wait_s: f64,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeActionRequest {
    pub action: String,
    #[serde(default)]
    pub running: Option<bool>,
    #[serde(default)]
    pub speed: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryFrameLite {
    pub t_s: f64,
    pub kpis: Kpis,
    pub queue_summary: QueueSummary,
    #[serde(default)]
    pub service_loads: Vec<LiveServiceLoadLite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveServiceLoadLite {
    pub service_id: String,
    pub load_to_capacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationAdvanceEconomy {
    pub current_balance_base: f64,
    pub cumulative_revenue_base: f64,
    pub cumulative_opex_base: f64,
    pub budget_display: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedSaveMeta {
    pub deleted_id: String,
    pub project_id: String,
    pub name: String,
    pub session_kind: SessionKind,
    pub deleted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSaveResult {
    pub deleted_id: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreSaveResult {
    pub project_id: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeSaveResult {
    pub deleted_id: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandCoverageMeta {
    pub country_iso2: String,
    pub installed: bool,
    pub loaded_in_scenario: bool,
    pub cells: usize,
    pub surface_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandCoverageResult {
    pub country_iso2: String,
    pub installed: bool,
    pub loaded: bool,
    pub cells_loaded: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandRebuildResult {
    pub loaded_countries: Vec<String>,
    pub missing_countries: Vec<String>,
    pub total_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryPackStatus {
    pub country_iso2: String,
    pub build_state: String,
    pub surface_version: Option<String>,
    pub cells_count: usize,
    pub last_updated_at: Option<String>,
    #[serde(default)]
    pub map_installed: bool,
    #[serde(default)]
    pub map_ready: bool,
    #[serde(default)]
    pub map_pack_version: Option<String>,
    #[serde(default)]
    pub map_size_bytes: Option<u64>,
    #[serde(default)]
    pub demand_installed: bool,
    #[serde(default)]
    pub fully_playable: bool,
    pub eligible: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub country_iso2: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallResult {
    pub country_iso2: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionStatus {
    pub region_id: String,
    pub country_iso2: String,
    pub name: String,
    pub admin_level: String,
    pub nation: Option<String>,
    pub source_code: Option<String>,
    pub unlocked: bool,
    pub active: bool,
    pub adjacent_region_ids: Vec<String>,
    pub unlock_cost_base: f64,
    pub residents_smooth: f64,
    pub jobs_smooth: f64,
    #[serde(default)]
    pub employment_estimate: f64,
    pub cells_res8: usize,
    pub geometry: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountryMapContext {
    pub country_iso2: String,
    pub world_context: JsonValue,
    pub major_roads: JsonValue,
    pub default_bounds: Option<[[f64; 2]; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionStreetContext {
    pub region_id: String,
    pub country_iso2: String,
    pub roads: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRuntimeConfig {
    pub country_iso2: String,
    #[serde(default)]
    pub style_url: Option<String>,
    pub world_context_url: String,
    #[serde(default)]
    pub counties_url: Option<String>,
    pub major_roads_url: Option<String>,
    pub county_basemap_mid_url_template: Option<String>,
    pub county_basemap_full_url_template: Option<String>,
    pub default_bounds: Option<[[f64; 2]; 2]>,
    pub map_pack_version: Option<String>,
    pub map_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockResult {
    pub region_id: String,
    pub charged_base: f64,
    pub current_balance_base: f64,
    pub unlocked_regions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusResult {
    pub primary_focus_region_id: String,
    pub active_region_ids: Vec<String>,
    pub materialized_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockFocusResult {
    pub region_id: String,
    pub charged_base: f64,
    pub current_balance_base: f64,
    pub unlocked_regions: usize,
    pub primary_focus_region_id: String,
    pub active_region_ids: Vec<String>,
    pub materialized_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScopeUpdate {
    pub max_active_zones: Option<usize>,
    pub remote_regions_mode: Option<String>,
    pub remote_update_interval_ticks: Option<u32>,
    pub focus_max_active_zones: Option<usize>,
    pub adjacent_max_active_zones: Option<usize>,
    pub remote_max_active_zones: Option<usize>,
    pub adjacent_update_interval_ticks: Option<u32>,
    pub active_region_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeState {
    pub max_active_zones: usize,
    pub remote_regions_mode: String,
    pub remote_update_interval_ticks: u32,
    pub focus_max_active_zones: usize,
    pub adjacent_max_active_zones: usize,
    pub remote_max_active_zones: usize,
    pub adjacent_update_interval_ticks: u32,
    pub active_region_ids: Vec<String>,
    pub materialized_cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandTileSourceMeta {
    pub layer: String,
    pub source: String,
    pub countries_loaded: Vec<String>,
    pub cells: usize,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandSurfaceCountryWire {
    country_iso2: String,
    surface_version: String,
    source_provenance: JsonValue,
    cells_res6: Vec<DemandSurfaceCellWire>,
    cells_res7: Vec<DemandSurfaceCellWire>,
    cells_res8: Vec<DemandSurfaceCellWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandSurfaceCellWire {
    cell_id: String,
    h3_res: u8,
    lon: f64,
    lat: f64,
    x: f64,
    y: f64,
    area_m2: f64,
    country_iso2: String,
    residents_raw: f64,
    jobs_raw: f64,
    residents_smooth: f64,
    jobs_smooth: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_residential: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_office: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_retail: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_recreation: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_industrial: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_education: f64,
    #[serde(default = "default_surface_activity_mix")]
    activity_mix_health: f64,
    quality: f64,
}

#[derive(Debug, Clone)]
struct SurfaceRegionInfo {
    region_id: String,
    country_iso2: String,
    name: String,
    admin_level: String,
    nation: Option<String>,
    source_code: Option<String>,
    cell_id: String,
    x: f64,
    y: f64,
    area_m2: f64,
    residents_smooth: f64,
    jobs_smooth: f64,
    activity_mix_residential: f64,
    activity_mix_office: f64,
    activity_mix_retail: f64,
    activity_mix_recreation: f64,
    activity_mix_industrial: f64,
    activity_mix_education: f64,
    activity_mix_health: f64,
    adjacent_region_ids: Vec<String>,
    geometry: Option<JsonValue>,
}

#[derive(Debug, Clone)]
struct SurfaceRegionCatalog {
    regions: Vec<SurfaceRegionInfo>,
    by_id: HashMap<String, SurfaceRegionInfo>,
    cells_res8_by_region: HashMap<String, Vec<DemandSurfaceCellWire>>,
}

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexFile {
    counties: Vec<UkCountyIndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexEntry {
    county_id: String,
    name: String,
    nation: String,
    country_iso2: String,
    source_code: String,
}

#[derive(Debug, Clone)]
struct CountyBoundary {
    county_id: String,
    name: String,
    nation: String,
    country_iso2: String,
    source_code: String,
    geometry: MultiPolygon<f64>,
    geometry_json: JsonValue,
    bbox_center_lon: f64,
    bbox_center_lat: f64,
}

#[derive(Debug, Clone)]
struct CountyBoundaryCatalog {
    counties: Vec<CountyBoundary>,
}

#[derive(Debug, Clone, Copy)]
struct GeoSegment {
    a_lon: f64,
    a_lat: f64,
    b_lon: f64,
    b_lat: f64,
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
}

#[derive(Debug, Clone, Default)]
struct CountyModeConstraintData {
    road_segments: Vec<GeoSegment>,
    water_polygons: Vec<MultiPolygon<f64>>,
    water_segments: Vec<GeoSegment>,
}

#[derive(Debug, Clone)]
struct LanduseSample {
    x_m: f64,
    y_m: f64,
    weight: f64,
    intensity: f64,
    mix: [f64; 7],
}

#[derive(Debug, Clone, Default)]
struct CountyLanduseProfile {
    samples: Vec<LanduseSample>,
}

#[derive(Debug, Clone, Deserialize)]
struct GbCountyAliasFile {
    aliases: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SandboxSnapshotFile {
    pub snapshot: SnapshotMeta,
    pub scenario: ScenarioDocumentLite,
    pub history: SimHistory,
    #[serde(default)]
    pub runtime: Option<PersistedRuntimeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedRuntimeState {
    #[serde(default)]
    pub tick_s: f64,
    #[serde(default)]
    pub sim_state: Option<PersistedSimState>,
    #[serde(default)]
    pub run_cfg: Option<RunConfig>,
    #[serde(default)]
    pub history: Option<SimHistory>,
    #[serde(default)]
    pub last_output: Option<SimulationOutput>,
    #[serde(default)]
    pub last_quick_kpis: Option<Kpis>,
    #[serde(default)]
    pub runtime_ops: Option<PersistedRuntimeOpsState>,
    #[serde(default)]
    pub latest_snapshot: Option<RuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedSandboxStateFile {
    #[serde(default)]
    pub tick_s: f64,
    #[serde(default)]
    pub runtime: Option<PersistedRuntimeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedSimState {
    #[serde(default)]
    pub t_s: f64,
    #[serde(default)]
    pub queue: Vec<PersistedServiceStopValue>,
    #[serde(default)]
    pub queue_cohorts: Vec<PersistedServiceStopDestinationValue>,
    #[serde(default)]
    pub time_to_next_departure_s: Vec<PersistedServiceStopValue>,
    #[serde(default)]
    pub pending_od_trips: Vec<PersistedZonePairValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedRuntimeOpsState {
    #[serde(default)]
    pub topology_hash: u64,
    #[serde(default)]
    pub trains: Vec<RuntimeTrainState>,
    #[serde(default)]
    pub queue_cohorts: Vec<PersistedServiceStopDestinationValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedServiceStopValue {
    pub service_id: String,
    pub stop_id: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedServiceStopDestinationValue {
    pub service_id: String,
    pub board_stop_id: String,
    pub destination_stop_id: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedZonePairValue {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SaveIndex {
    pub version: u32,
    pub projects: Vec<SaveIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SaveIndexEntry {
    pub project_id: String,
    pub project_path: String,
    pub name: String,
    pub session_kind: SessionKind,
    pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeletedIndex {
    pub version: u32,
    pub entries: Vec<DeletedIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeletedIndexEntry {
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

#[derive(Debug, Clone, Deserialize)]
struct CatalogCountryWire {
    iso2: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogCityWire {
    geonameid: i64,
    name: String,
    lat: f64,
    lon: f64,
    population: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CountryPackIndex {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    packs: Vec<CountryPackEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CountryPackEntry {
    country_iso2: String,
    build_state: String,
    #[serde(default)]
    surface_version: Option<String>,
    #[serde(default)]
    cells_count: usize,
    #[serde(default)]
    last_updated_at: Option<String>,
    #[serde(default)]
    checksum: Option<String>,
    #[serde(default)]
    provenance: Option<String>,
}

pub struct AppState {
    pub game: Mutex<Option<interlinked_engine::platform::GameState>>,
    pub current_project: Mutex<Option<String>>,
    pub runtime_tick: Mutex<Option<RuntimeTick>>,
    pub(crate) runtime_loop: Mutex<Option<RuntimeLoopHandle>>,
    pub runtime_snapshots: Mutex<VecDeque<RuntimeSnapshot>>,
    pub runtime_fast_snapshots: Mutex<VecDeque<RuntimeFastSnapshot>>,
    pub runtime_strategic_snapshots: Mutex<VecDeque<RuntimeStrategicSnapshot>>,
    pub(crate) runtime_materialization: Mutex<Option<RuntimeMaterializationState>>,
    pub(crate) runtime_ops: Mutex<Option<RuntimeOpsState>>,
}

struct RuntimeLoopHandle {
    project_path: String,
    tx: Sender<RuntimeAction>,
    pending_actions: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    speed: Arc<AtomicU32>,
    clock_revision: Arc<AtomicU64>,
    join: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
struct RuntimeMaterializationState {
    project_path: String,
    topology_hash: u64,
    scope_hash: u64,
    fare_hash: u64,
    minute_of_day: u32,
    last_materialized_tick: u64,
    adaptive_max_active_zones: usize,
    last_tick_ms: f64,
}

#[derive(Debug, Clone, Default)]
struct RuntimeOpsState {
    project_path: String,
    topology_hash: u64,
    profiles_by_service: HashMap<String, RuntimeServiceProfile>,
    stop_name_by_id: HashMap<String, String>,
    reverse_service_by_service: HashMap<String, String>,
    stop_ids_by_service: HashMap<String, HashSet<String>>,
    fare_base_by_service: HashMap<String, f64>,
    dispatch_service_ids: HashSet<String>,
    trains: BTreeMap<String, RuntimeTrainState>,
    queue_cohorts: HashMap<(String, String, String), f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeTrainPhase {
    Dwell,
    Moving,
    Layover,
}

fn default_runtime_train_phase() -> RuntimeTrainPhase {
    RuntimeTrainPhase::Dwell
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeTrainState {
    train_id: String,
    service_id: String,
    line_id: String,
    line_name: String,
    mode: String,
    mode_variant: Option<String>,
    stock_tier_id: Option<String>,
    vehicle_capacity: f64,
    current_stop_index: usize,
    direction_step: i8,
    #[serde(default = "default_runtime_train_phase")]
    phase: RuntimeTrainPhase,
    progress: f64,
    remaining_s: f64,
    onboard_pax: f64,
    onboard_cohorts: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct RuntimeFareEvents {
    boarded_pax: f64,
    completed_alightings_pax: f64,
    liability_accrued_base: f64,
}

#[derive(Debug, Clone)]
struct RuntimeServiceProfile {
    service_id: String,
    line_id: String,
    line_name: String,
    mode: String,
    mode_variant: Option<String>,
    stock_tier_id: Option<String>,
    dwell_s: f64,
    turnaround_s: f64,
    speed_mps: f64,
    vehicle_capacity: f64,
    vehicles_on_service: usize,
    stop_ids: Vec<String>,
    stop_xy: Vec<(f64, f64)>,
    segment_lengths_m: Vec<f64>,
}

#[derive(Debug, Clone)]
enum RuntimeAction {
    Stop,
    SetRunning(bool),
    SetSpeed(u32),
    InvalidateMaterialization,
    ForceCheckpoint,
    AdvanceOnce { recompute_quick_kpis: bool },
}

#[derive(Debug, Clone)]
struct MapAssetServer {
    base_url: String,
}

static COUNTRY_MAP_CONTEXT_CACHE: OnceLock<Mutex<HashMap<String, CountryMapContext>>> =
    OnceLock::new();
static REGION_STREET_CONTEXT_CACHE: OnceLock<Mutex<HashMap<String, RegionStreetContext>>> =
    OnceLock::new();
static MAP_ASSET_SERVER: OnceLock<MapAssetServer> = OnceLock::new();
static GB_COUNTY_LANDUSE_CACHE: OnceLock<Mutex<HashMap<String, CountyLanduseProfile>>> =
    OnceLock::new();
static GB_COUNTY_MODE_CONSTRAINT_CACHE: OnceLock<
    Mutex<HashMap<String, Arc<CountyModeConstraintData>>>,
> = OnceLock::new();

pub struct RuntimeTick {
    pub project_path: String,
    pub last_step: Instant,
}

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

fn default_difficulty_label() -> String {
    "standard".to_string()
}

fn default_profile_multiplier_missing() -> f64 {
    -1.0
}

fn default_economy_revision() -> u64 {
    0
}

fn difficulty_profile_for_label(label: &str) -> DifficultyProfile {
    let token = label.trim().to_ascii_lowercase();
    match token.as_str() {
        "easy" => DifficultyProfile {
            profile_id: "easy".to_string(),
            demand_mult: 0.85,
            capex_mult: 0.90,
            opex_mult: 0.92,
            maintenance_mult: 0.85,
            penalty_mult: 0.85,
            ancillary_revenue_mult: 1.08,
            unlock_cost_mult: 0.90,
        },
        "hard" => DifficultyProfile {
            profile_id: "hard".to_string(),
            demand_mult: 1.20,
            capex_mult: 1.15,
            opex_mult: 1.18,
            maintenance_mult: 1.25,
            penalty_mult: 1.25,
            ancillary_revenue_mult: 0.92,
            unlock_cost_mult: 1.15,
        },
        _ => DifficultyProfile {
            profile_id: "standard".to_string(),
            demand_mult: 1.0,
            capex_mult: 1.0,
            opex_mult: 1.0,
            maintenance_mult: 1.0,
            penalty_mult: 1.0,
            ancillary_revenue_mult: 1.0,
            unlock_cost_mult: 1.0,
        },
    }
}

fn difficulty_profile_for(difficulty: Difficulty) -> DifficultyProfile {
    match difficulty {
        Difficulty::Easy => difficulty_profile_for_label("easy"),
        Difficulty::Standard => difficulty_profile_for_label("standard"),
        Difficulty::Hard => difficulty_profile_for_label("hard"),
    }
}

fn sanitize_multiplier_or_fallback(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn sanitize_difficulty_profile(profile: &mut DifficultyProfile, difficulty_label: &str) {
    let fallback = difficulty_profile_for_label(difficulty_label);
    if profile.profile_id.trim().is_empty() {
        *profile = fallback;
        return;
    }
    profile.demand_mult =
        sanitize_multiplier_or_fallback(profile.demand_mult, fallback.demand_mult).clamp(0.2, 3.0);
    profile.capex_mult =
        sanitize_multiplier_or_fallback(profile.capex_mult, fallback.capex_mult).clamp(0.2, 3.0);
    profile.opex_mult =
        sanitize_multiplier_or_fallback(profile.opex_mult, fallback.opex_mult).clamp(0.2, 3.0);
    profile.maintenance_mult =
        sanitize_multiplier_or_fallback(profile.maintenance_mult, fallback.maintenance_mult)
            .clamp(0.2, 3.0);
    profile.penalty_mult =
        sanitize_multiplier_or_fallback(profile.penalty_mult, fallback.penalty_mult)
            .clamp(0.2, 3.0);
    profile.ancillary_revenue_mult = sanitize_multiplier_or_fallback(
        profile.ancillary_revenue_mult,
        fallback.ancillary_revenue_mult,
    )
    .clamp(0.2, 3.0);
    profile.unlock_cost_mult =
        sanitize_multiplier_or_fallback(profile.unlock_cost_mult, fallback.unlock_cost_mult)
            .clamp(0.2, 3.0);
}

fn resolved_difficulty_profile(manifest: &ProjectManifest) -> DifficultyProfile {
    let mut profile = manifest.economy.difficulty_profile.clone();
    sanitize_difficulty_profile(&mut profile, &manifest.economy.difficulty);
    profile
}

fn bump_economy_revision(manifest: &mut ProjectManifest) {
    manifest.economy.economy_revision = manifest.economy.economy_revision.saturating_add(1);
}

fn economy_config() -> EconomyConfig {
    default_economy_config()
}

fn difficulty_label(difficulty: Difficulty) -> String {
    match difficulty {
        Difficulty::Easy => "easy".to_string(),
        Difficulty::Standard => "standard".to_string(),
        Difficulty::Hard => "hard".to_string(),
    }
}

fn default_starting_budget_display(difficulty: Difficulty, currency: &str) -> f64 {
    let cfg = economy_config();
    let base = match difficulty {
        Difficulty::Easy => 3_000_000_000.0,
        Difficulty::Standard => 1_500_000_000.0,
        Difficulty::Hard => 750_000_000.0,
    };
    from_base_currency(base, currency, &cfg)
}

fn app_root(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let root = base.join(APP_DIR_NAME);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

fn projects_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app_root(app)?.join("projects");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_root(app)?.join(INDEX_FILE_NAME))
}

fn location_catalog_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(LOCATION_CATALOG_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn demand_surfaces_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(DEMAND_SURFACE_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn country_packs_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(COUNTRY_PACKS_DIR);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn country_pack_index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(country_packs_root(app)?.join(COUNTRY_PACK_INDEX_FILE))
}

fn trash_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app_root(app)?.join(TRASH_DIR_NAME);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn deleted_index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_root(app)?.join(DELETED_INDEX_FILE_NAME))
}

fn read_country_pack_index(app: &AppHandle) -> Result<CountryPackIndex, String> {
    let path = country_pack_index_path(app)?;
    if !path.exists() {
        return Ok(CountryPackIndex {
            version: 1,
            packs: vec![],
        });
    }
    read_json_file(&path)
}

fn write_country_pack_index(app: &AppHandle, idx: &CountryPackIndex) -> Result<(), String> {
    write_json_file(&country_pack_index_path(app)?, idx)
}

fn manifest_path(project_root: &Path) -> PathBuf {
    project_root.join(MANIFEST_FILE)
}

fn scenario_path(project_root: &Path) -> PathBuf {
    project_root.join(SCENARIO_FILE)
}

fn sandbox_state_path(project_root: &Path) -> PathBuf {
    project_root.join(SANDBOX_STATE_FILE)
}

fn ui_layouts_path(project_root: &Path) -> PathBuf {
    project_root.join(UI_LAYOUTS_FILE)
}

fn start_location_from_manifest(manifest: &ProjectManifest) -> Option<StartLocation> {
    manifest.start_location.clone()
}

fn snapshots_dir(project_root: &Path) -> PathBuf {
    project_root.join("sandbox").join("snapshots")
}

fn runs_dir(project_root: &Path) -> PathBuf {
    project_root.join("runs")
}

fn ensure_project_dirs(project_root: &Path) -> Result<(), String> {
    fs::create_dir_all(project_root.join("scenario")).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("sandbox")).map_err(|e| e.to_string())?;
    fs::create_dir_all(snapshots_dir(project_root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(runs_dir(project_root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("ui")).map_err(|e| e.to_string())?;
    fs::create_dir_all(project_root.join("assets")).map_err(|e| e.to_string())?;
    Ok(())
}

fn reset_runtime_tick(state: &tauri::State<AppState>, project_path: &str) -> Result<(), String> {
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

fn hash_string_seq(values: &[String], hasher: &mut std::collections::hash_map::DefaultHasher) {
    for value in values {
        value.hash(hasher);
    }
}

fn is_auto_reverse_service_id(id: &str) -> bool {
    id.starts_with(AUTO_REVERSE_SERVICE_PREFIX)
}

fn is_auto_reverse_link_id(id: &str) -> bool {
    id.starts_with(AUTO_REVERSE_LINK_PREFIX)
}

fn strip_auto_reverse_runtime_artifacts(scenario: &mut Scenario) {
    scenario
        .world
        .services
        .retain(|service| !is_auto_reverse_service_id(&service.id));
    scenario
        .world
        .links
        .retain(|link| !is_auto_reverse_link_id(&link.id));
}

fn normalized_mode_token(mode: &str) -> String {
    mode.trim().to_ascii_lowercase()
}

fn normalized_variant_token(variant: Option<&str>) -> String {
    variant
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn normalized_line_token(line_id: Option<&str>) -> String {
    line_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn service_line_runtime_id(service: &Service) -> String {
    service
        .line_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| service.id.clone())
}

fn is_pending_purchase_order_status(status: Option<&str>) -> bool {
    let normalized = status
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    normalized.is_empty() || normalized == "pending"
}

fn estimate_unit_purchase_cost_base_for_service(
    service: &Service,
    defaults: &BuildDefaults,
) -> Option<f64> {
    let mode = service.mode.trim();
    let service_variant = service
        .mode_variant
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let preset = defaults.presets.iter().find(|candidate| {
        candidate.engine_mode.eq_ignore_ascii_case(mode)
            && candidate
                .mode_variant
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                == service_variant
    })?;
    let profile = service.rolling_stock_profile.as_ref();
    let package_id = profile
        .and_then(|value| value.package_id.as_deref())
        .or(service.stock_tier_id.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    let package_multiplier = preset
        .package_options
        .iter()
        .find(|tier| tier.id.eq_ignore_ascii_case(&package_id))
        .or_else(|| {
            preset
                .package_options
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.package_options.first())
        .or_else(|| {
            preset
                .tiers
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case(&package_id))
        })
        .or_else(|| {
            preset
                .tiers
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.tiers.first())
        .map(|tier| tier.purchase_cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let speed_id = profile
        .and_then(|value| value.speed_level.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "balanced".to_string());
    let speed_multiplier = preset
        .speed_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&speed_id))
        .or_else(|| {
            preset
                .speed_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("balanced"))
        })
        .or_else(|| preset.speed_levels.first())
        .map(|item| item.cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let comfort_id = profile
        .and_then(|value| value.comfort_level.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    let comfort_multiplier = preset
        .comfort_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&comfort_id))
        .or_else(|| {
            preset
                .comfort_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.comfort_levels.first())
        .map(|item| item.cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let cars_per_unit = profile
        .and_then(|value| value.cars_per_unit)
        .unwrap_or(1)
        .max(1) as f64;
    let cars_multiplier = if preset.supports_carriages {
        let base = preset.cars_default.max(1) as f64;
        (cars_per_unit / base).max(0.5)
    } else {
        1.0
    };
    let unit_cost = preset.base_unit_purchase_cost_base.max(0.0)
        * package_multiplier
        * speed_multiplier
        * comfort_multiplier
        * cars_multiplier;
    if unit_cost.is_finite() && unit_cost > 0.0 {
        Some(unit_cost)
    } else {
        None
    }
}

fn resolve_order_unit_cost_base(order: &PurchaseOrder, fallback: Option<f64>) -> Option<f64> {
    if let Some(unit_cost) = order.unit_cost_base {
        if unit_cost.is_finite() && unit_cost > 0.0 {
            return Some(unit_cost);
        }
    }
    if let Some(total_cost) = order.total_cost_base {
        if total_cost.is_finite() && total_cost >= 0.0 && order.units > 0 {
            let per_unit = total_cost / order.units as f64;
            if per_unit.is_finite() && per_unit > 0.0 {
                return Some(per_unit);
            }
        }
    }
    fallback.filter(|value| value.is_finite() && *value > 0.0)
}

fn reverse_direction_fields(
    direction: Option<&str>,
    direction_name: Option<&str>,
) -> (Option<String>, Option<String>) {
    let direction_token = direction
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let direction_name_token = direction_name
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let reversed_direction = if direction_token.contains("forward")
        || direction_token.contains("outbound")
        || direction_token.contains("clockwise")
    {
        "reverse"
    } else if direction_token.contains("reverse")
        || direction_token.contains("inbound")
        || direction_token.contains("backward")
        || direction_token.contains("counterclockwise")
    {
        "forward"
    } else {
        "reverse"
    };
    let reversed_name = if direction_name_token.contains("outbound")
        || direction_name_token.contains("clockwise")
    {
        "Inbound".to_string()
    } else if direction_name_token.contains("inbound")
        || direction_name_token.contains("counterclockwise")
    {
        "Outbound".to_string()
    } else if reversed_direction == "reverse" {
        "Inbound".to_string()
    } else {
        "Outbound".to_string()
    };
    (Some(reversed_direction.to_string()), Some(reversed_name))
}

fn link_key_exact(
    from_stop: &str,
    to_stop: &str,
    line_id: Option<&str>,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_line_token(line_id),
        normalized_mode_token(mode),
        normalized_variant_token(mode_variant)
    )
}

fn link_key_no_line(
    from_stop: &str,
    to_stop: &str,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_mode_token(mode),
        normalized_variant_token(mode_variant)
    )
}

fn reverse_link_geometry(geometry: &Option<Vec<[f64; 2]>>) -> Option<Vec<[f64; 2]>> {
    geometry.as_ref().map(|coords| {
        let mut reversed = coords.clone();
        reversed.reverse();
        reversed
    })
}

fn synthetic_reverse_link_id(
    line_id: &str,
    from_stop: &str,
    to_stop: &str,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    let mut id = format!(
        "{AUTO_REVERSE_LINK_PREFIX}{line_id}::{}->{}::{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_mode_token(mode)
    );
    let variant = normalized_variant_token(mode_variant);
    if !variant.is_empty() {
        id.push_str("::");
        id.push_str(&variant);
    }
    id
}

fn estimate_link_distance_from_stops(
    stop_xy: &HashMap<String, (f64, f64)>,
    from_stop: &str,
    to_stop: &str,
) -> f64 {
    let Some((from_x, from_y)) = stop_xy.get(from_stop) else {
        return 1_000.0;
    };
    let Some((to_x, to_y)) = stop_xy.get(to_stop) else {
        return 1_000.0;
    };
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist.is_finite() && dist > 10.0 {
        dist
    } else {
        1_000.0
    }
}

fn synthesize_auto_reverse_runtime_services(scenario: &mut Scenario) -> usize {
    let mut exact_link_map = HashMap::<String, Link>::new();
    let mut no_line_link_map = HashMap::<String, Link>::new();
    let mut exact_link_keys = HashSet::<String>::new();
    let mut no_line_link_keys = HashSet::<String>::new();
    let mut existing_link_ids = HashSet::<String>::new();
    for link in &scenario.world.links {
        existing_link_ids.insert(link.id.clone());
        let exact_key = link_key_exact(
            &link.from_stop,
            &link.to_stop,
            link.line_id.as_deref(),
            &link.mode,
            link.mode_variant.as_deref(),
        );
        let no_line_key = link_key_no_line(
            &link.from_stop,
            &link.to_stop,
            &link.mode,
            link.mode_variant.as_deref(),
        );
        exact_link_keys.insert(exact_key.clone());
        no_line_link_keys.insert(no_line_key.clone());
        exact_link_map
            .entry(exact_key)
            .or_insert_with(|| link.clone());
        no_line_link_map
            .entry(no_line_key)
            .or_insert_with(|| link.clone());
    }

    let stop_xy = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), (stop.x, stop.y)))
        .collect::<HashMap<_, _>>();

    let mut sequences_by_line = HashMap::<String, HashSet<Vec<String>>>::new();
    for service in &scenario.world.services {
        if is_auto_reverse_service_id(&service.id) || service.stop_sequence.len() < 2 {
            continue;
        }
        sequences_by_line
            .entry(service_line_runtime_id(service))
            .or_default()
            .insert(service.stop_sequence.clone());
    }

    let mut existing_service_ids = scenario
        .world
        .services
        .iter()
        .map(|service| service.id.clone())
        .collect::<HashSet<_>>();
    let mut synthetic_services = Vec::<Service>::new();
    let mut synthetic_links = Vec::<Link>::new();
    let base_services = scenario.world.services.clone();
    for service in base_services {
        if is_auto_reverse_service_id(&service.id) || service.stop_sequence.len() < 2 {
            continue;
        }
        let line_id = service_line_runtime_id(&service);
        let mut reverse_sequence = service.stop_sequence.clone();
        reverse_sequence.reverse();
        if reverse_sequence == service.stop_sequence {
            continue;
        }
        if sequences_by_line
            .get(&line_id)
            .map(|sequences| sequences.contains(&reverse_sequence))
            .unwrap_or(false)
        {
            continue;
        }
        let synthetic_service_id =
            format!("{AUTO_REVERSE_SERVICE_PREFIX}{}::{}", line_id, service.id);
        if existing_service_ids.contains(&synthetic_service_id) {
            continue;
        }
        for segment in reverse_sequence.windows(2) {
            let from_stop = &segment[0];
            let to_stop = &segment[1];
            let reverse_exact_key = link_key_exact(
                from_stop,
                to_stop,
                service.line_id.as_deref(),
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let reverse_no_line_key = link_key_no_line(
                from_stop,
                to_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            if exact_link_keys.contains(&reverse_exact_key)
                || no_line_link_keys.contains(&reverse_no_line_key)
            {
                continue;
            }
            let forward_exact_key = link_key_exact(
                to_stop,
                from_stop,
                service.line_id.as_deref(),
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let forward_no_line_key = link_key_no_line(
                to_stop,
                from_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let template = exact_link_map
                .get(&forward_exact_key)
                .or_else(|| no_line_link_map.get(&forward_no_line_key))
                .cloned();
            let synthetic_link_id = synthetic_reverse_link_id(
                &line_id,
                from_stop,
                to_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            if existing_link_ids.contains(&synthetic_link_id) {
                exact_link_keys.insert(reverse_exact_key.clone());
                no_line_link_keys.insert(reverse_no_line_key.clone());
                continue;
            }
            let reverse_link = if let Some(forward) = template {
                Link {
                    id: synthetic_link_id.clone(),
                    from_stop: from_stop.clone(),
                    to_stop: to_stop.clone(),
                    distance_m: forward.distance_m.max(1.0),
                    mode: forward.mode.clone(),
                    speed_mps: forward.speed_mps.max(0.1),
                    geometry: reverse_link_geometry(&forward.geometry),
                    line_id: service.line_id.clone().or(forward.line_id.clone()),
                    mode_variant: service
                        .mode_variant
                        .clone()
                        .or(forward.mode_variant.clone()),
                    capacity_per_hour: forward.capacity_per_hour,
                }
            } else {
                Link {
                    id: synthetic_link_id.clone(),
                    from_stop: from_stop.clone(),
                    to_stop: to_stop.clone(),
                    distance_m: estimate_link_distance_from_stops(&stop_xy, from_stop, to_stop),
                    mode: service.mode.clone(),
                    speed_mps: 12.0,
                    geometry: None,
                    line_id: service.line_id.clone(),
                    mode_variant: service.mode_variant.clone(),
                    capacity_per_hour: None,
                }
            };
            exact_link_keys.insert(reverse_exact_key.clone());
            no_line_link_keys.insert(reverse_no_line_key.clone());
            exact_link_map.insert(reverse_exact_key, reverse_link.clone());
            no_line_link_map.insert(reverse_no_line_key, reverse_link.clone());
            existing_link_ids.insert(synthetic_link_id);
            synthetic_links.push(reverse_link);
        }

        let (reverse_direction, reverse_direction_name) = reverse_direction_fields(
            service.direction.as_deref(),
            service.direction_name.as_deref(),
        );
        let mut reverse_service = service.clone();
        reverse_service.id = synthetic_service_id.clone();
        reverse_service.stop_sequence = reverse_sequence.clone();
        reverse_service.direction = reverse_direction;
        reverse_service.direction_name = reverse_direction_name;
        synthetic_services.push(reverse_service);
        existing_service_ids.insert(synthetic_service_id);
        sequences_by_line
            .entry(line_id)
            .or_default()
            .insert(reverse_sequence);
    }

    if !synthetic_links.is_empty() {
        scenario.world.links.extend(synthetic_links);
    }
    let added_services = synthetic_services.len();
    if added_services > 0 {
        scenario.world.services.extend(synthetic_services);
    }
    added_services
}

fn scenario_topology_hash(scenario: &Scenario) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let real_link_count = scenario
        .world
        .links
        .iter()
        .filter(|link| !is_auto_reverse_link_id(&link.id))
        .count();
    let real_service_count = scenario
        .world
        .services
        .iter()
        .filter(|service| !is_auto_reverse_service_id(&service.id))
        .count();
    scenario.world.stops.len().hash(&mut hasher);
    real_link_count.hash(&mut hasher);
    real_service_count.hash(&mut hasher);
    for stop in scenario.world.stops.iter().take(256) {
        stop.id.hash(&mut hasher);
        stop.stop_type.hash(&mut hasher);
        stop.x.to_bits().hash(&mut hasher);
        stop.y.to_bits().hash(&mut hasher);
    }
    for link in scenario
        .world
        .links
        .iter()
        .filter(|link| !is_auto_reverse_link_id(&link.id))
        .take(512)
    {
        link.id.hash(&mut hasher);
        link.from_stop.hash(&mut hasher);
        link.to_stop.hash(&mut hasher);
        link.mode.hash(&mut hasher);
        link.mode_variant.hash(&mut hasher);
        link.distance_m.to_bits().hash(&mut hasher);
    }
    for service in scenario
        .world
        .services
        .iter()
        .filter(|service| !is_auto_reverse_service_id(&service.id))
        .take(512)
    {
        service.id.hash(&mut hasher);
        service.line_id.hash(&mut hasher);
        service.mode.hash(&mut hasher);
        service.mode_variant.hash(&mut hasher);
        service.headway_s.to_bits().hash(&mut hasher);
        service.dwell_s.to_bits().hash(&mut hasher);
        service.vehicle_capacity.to_bits().hash(&mut hasher);
        hash_string_seq(&service.stop_sequence, &mut hasher);
    }
    hasher.finish()
}

fn scope_hash(manifest: &ProjectManifest) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    manifest.simulation_scope.max_active_zones.hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_regions_mode
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_update_interval_ticks
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .focus_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .adjacent_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .adjacent_update_interval_ticks
        .hash(&mut hasher);
    hash_string_seq(&manifest.region_state.active_region_ids, &mut hasher);
    manifest
        .region_state
        .primary_focus_region_id
        .hash(&mut hasher);
    hasher.finish()
}

fn fare_hash(policy: &FarePolicyManifest) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    policy.enabled.hash(&mut hasher);
    policy.fare_mode_bus_base.to_bits().hash(&mut hasher);
    policy.fare_mode_tram_base.to_bits().hash(&mut hasher);
    policy.fare_mode_metro_base.to_bits().hash(&mut hasher);
    policy.fare_mode_rail_base.to_bits().hash(&mut hasher);
    policy.fare_mode_ferry_base.to_bits().hash(&mut hasher);
    policy.fare_mode_default_base.to_bits().hash(&mut hasher);
    policy.transfer_window_s.to_bits().hash(&mut hasher);
    policy.free_transfers_per_trip.hash(&mut hasher);
    hasher.finish()
}

fn active_ledger_key(manifest: &ProjectManifest) -> String {
    manifest
        .region_state
        .primary_focus_region_id
        .as_ref()
        .and_then(|rid| canonicalize_region_id(rid))
        .unwrap_or_else(|| "global".to_string())
}

fn update_region_ledger(
    manifest: &mut ProjectManifest,
    delta_revenue_base: f64,
    delta_opex_base: f64,
    delta_penalty_base: f64,
    delta_capex_base: f64,
) {
    let key = active_ledger_key(manifest);
    let entry = manifest.economy.region_ledger.entry(key).or_default();
    entry.revenue_base += delta_revenue_base;
    entry.opex_base += delta_opex_base;
    entry.penalties_base += delta_penalty_base;
    entry.capex_base += delta_capex_base;
    entry.net_base = entry.revenue_base - entry.opex_base - entry.penalties_base - entry.capex_base;
}

fn sanitize_quality_penalty_rates(rates: &mut QualityPenaltyRates) {
    if !rates.overcrowding_base_per_passenger.is_finite() {
        rates.overcrowding_base_per_passenger = default_overcrowding_penalty_rate();
    }
    if !rates.reliability_base_per_passenger.is_finite() {
        rates.reliability_base_per_passenger = default_reliability_penalty_rate();
    }
    rates.overcrowding_base_per_passenger = rates.overcrowding_base_per_passenger.clamp(0.0, 100.0);
    rates.reliability_base_per_passenger = rates.reliability_base_per_passenger.clamp(0.0, 100.0);
}

fn sanitize_monthly_financials(monthly: &mut Vec<MonthlyFinancialSnapshot>) {
    monthly.retain(|entry| {
        entry.revenue_base.is_finite()
            && entry.opex_base.is_finite()
            && entry.capex_base.is_finite()
            && entry.penalties_base.is_finite()
            && entry.net_base.is_finite()
    });
    monthly.sort_by(|a, b| a.month_index.cmp(&b.month_index));
    if monthly.len() > ECONOMY_MONTHLY_FINANCIAL_CAP {
        let drain = monthly.len().saturating_sub(ECONOMY_MONTHLY_FINANCIAL_CAP);
        monthly.drain(0..drain);
    }
    for entry in monthly.iter_mut() {
        entry.revenue_base = sanitize_non_negative(entry.revenue_base);
        entry.opex_base = sanitize_non_negative(entry.opex_base);
        entry.capex_base = sanitize_non_negative(entry.capex_base);
        entry.penalties_base = sanitize_non_negative(entry.penalties_base);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
}

fn sanitize_economy_manifest(economy: &mut EconomyManifest) {
    economy.starting_budget_base = sanitize_non_negative(economy.starting_budget_base);
    if !economy.current_balance_base.is_finite() {
        economy.current_balance_base = 0.0;
    }
    economy.cumulative_capex_base = sanitize_non_negative(economy.cumulative_capex_base);
    economy.cumulative_opex_base = sanitize_non_negative(economy.cumulative_opex_base);
    economy.cumulative_revenue_base = sanitize_non_negative(economy.cumulative_revenue_base);
    economy.cumulative_lost_demand_penalty_base =
        sanitize_non_negative(economy.cumulative_lost_demand_penalty_base);
    economy.fare_revenue_deferred_base = sanitize_non_negative(economy.fare_revenue_deferred_base);
    economy.fare_boardings_deferred_pax =
        sanitize_non_negative(economy.fare_boardings_deferred_pax);
    if !economy.maintenance_rate.is_finite() {
        economy.maintenance_rate = default_maintenance_rate();
    }
    if !economy.ancillary_revenue_rate.is_finite() {
        economy.ancillary_revenue_rate = default_ancillary_revenue_rate();
    }
    economy.maintenance_rate = economy.maintenance_rate.clamp(0.0, 0.05);
    economy.ancillary_revenue_rate = economy.ancillary_revenue_rate.clamp(0.0, 0.75);
    sanitize_quality_penalty_rates(&mut economy.quality_penalty_rates);
    sanitize_difficulty_profile(&mut economy.difficulty_profile, &economy.difficulty);
    for entry in economy.region_ledger.values_mut() {
        if !entry.revenue_base.is_finite() {
            entry.revenue_base = 0.0;
        }
        if !entry.opex_base.is_finite() {
            entry.opex_base = 0.0;
        }
        if !entry.capex_base.is_finite() {
            entry.capex_base = 0.0;
        }
        if !entry.penalties_base.is_finite() {
            entry.penalties_base = 0.0;
        }
        if !entry.net_base.is_finite() {
            entry.net_base = 0.0;
        }
        entry.revenue_base = sanitize_non_negative(entry.revenue_base);
        entry.opex_base = sanitize_non_negative(entry.opex_base);
        entry.capex_base = sanitize_non_negative(entry.capex_base);
        entry.penalties_base = sanitize_non_negative(entry.penalties_base);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    sanitize_monthly_financials(&mut economy.monthly_financials);
    sanitize_fare_policy(&mut economy.fare_policy);
}

fn month_index_for_tick_seconds(tick_seconds: f64) -> u64 {
    if !tick_seconds.is_finite() || tick_seconds <= 0.0 {
        return 0;
    }
    (tick_seconds / ECONOMY_MONTH_SECONDS).floor().max(0.0) as u64
}

fn record_monthly_financial_delta(
    manifest: &mut ProjectManifest,
    revenue_base: f64,
    opex_base: f64,
    capex_base: f64,
    penalties_base: f64,
) {
    let revenue = sanitize_non_negative(revenue_base);
    let opex = sanitize_non_negative(opex_base);
    let capex = sanitize_non_negative(capex_base);
    let penalties = sanitize_non_negative(penalties_base);
    if revenue <= 0.0 && opex <= 0.0 && capex <= 0.0 && penalties <= 0.0 {
        return;
    }
    let month_index = month_index_for_tick_seconds(manifest.clock_state.tick_seconds);
    if let Some(entry) = manifest
        .economy
        .monthly_financials
        .iter_mut()
        .find(|entry| entry.month_index == month_index)
    {
        entry.revenue_base += revenue;
        entry.opex_base += opex;
        entry.capex_base += capex;
        entry.penalties_base += penalties;
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    } else {
        manifest
            .economy
            .monthly_financials
            .push(MonthlyFinancialSnapshot {
                month_index,
                revenue_base: revenue,
                opex_base: opex,
                capex_base: capex,
                penalties_base: penalties,
                net_base: revenue - opex - capex - penalties,
            });
    }
    sanitize_monthly_financials(&mut manifest.economy.monthly_financials);
}

fn normalize_financial_granularity(value: Option<&str>) -> String {
    let token = value
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "month".to_string());
    match token.as_str() {
        "day" | "week" | "month" | "year" => token,
        _ => "month".to_string(),
    }
}

fn financial_points_from_monthly(
    monthly: &[MonthlyFinancialSnapshot],
) -> Vec<FinancialDashboardPoint> {
    let mut rows = monthly.to_vec();
    rows.sort_by_key(|row| row.month_index);
    rows.into_iter()
        .map(|row| FinancialDashboardPoint {
            period_index: row.month_index as i64,
            label: format!("M{}", row.month_index.saturating_add(1)),
            revenue_base: row.revenue_base.max(0.0),
            opex_base: row.opex_base.max(0.0),
            capex_base: row.capex_base.max(0.0),
            penalties_base: row.penalties_base.max(0.0),
            net_base: row.net_base,
        })
        .collect()
}

fn distribute_month_points(
    points: &[FinancialDashboardPoint],
    slices_per_month: usize,
    prefix: &str,
) -> Vec<FinancialDashboardPoint> {
    if slices_per_month == 0 {
        return vec![];
    }
    let mut out = Vec::<FinancialDashboardPoint>::new();
    for point in points {
        for idx in 0..slices_per_month {
            let denominator = slices_per_month as f64;
            out.push(FinancialDashboardPoint {
                period_index: point.period_index * slices_per_month as i64 + idx as i64,
                label: format!("{}{}-{}", prefix, point.label, idx + 1),
                revenue_base: point.revenue_base / denominator,
                opex_base: point.opex_base / denominator,
                capex_base: point.capex_base / denominator,
                penalties_base: point.penalties_base / denominator,
                net_base: point.net_base / denominator,
            });
        }
    }
    out
}

fn aggregate_year_points(points: &[FinancialDashboardPoint]) -> Vec<FinancialDashboardPoint> {
    let mut grouped = BTreeMap::<i64, FinancialDashboardPoint>::new();
    for point in points {
        let year_index = (point.period_index / 12).max(0);
        let entry = grouped
            .entry(year_index)
            .or_insert(FinancialDashboardPoint {
                period_index: year_index,
                label: format!("Y{}", year_index + 1),
                revenue_base: 0.0,
                opex_base: 0.0,
                capex_base: 0.0,
                penalties_base: 0.0,
                net_base: 0.0,
            });
        entry.revenue_base += point.revenue_base.max(0.0);
        entry.opex_base += point.opex_base.max(0.0);
        entry.capex_base += point.capex_base.max(0.0);
        entry.penalties_base += point.penalties_base.max(0.0);
        entry.net_base += point.net_base;
    }
    grouped.into_values().collect()
}

fn financial_points_for_granularity(
    monthly_points: &[FinancialDashboardPoint],
    granularity: &str,
    periods: usize,
    scale: f64,
) -> Vec<FinancialDashboardPoint> {
    let scaled_monthly = monthly_points
        .iter()
        .map(|point| FinancialDashboardPoint {
            period_index: point.period_index,
            label: point.label.clone(),
            revenue_base: point.revenue_base * scale,
            opex_base: point.opex_base * scale,
            capex_base: point.capex_base * scale,
            penalties_base: point.penalties_base * scale,
            net_base: point.net_base * scale,
        })
        .collect::<Vec<_>>();
    let expanded = match granularity {
        "day" => distribute_month_points(&scaled_monthly, 30, "D"),
        "week" => distribute_month_points(&scaled_monthly, 4, "W"),
        "year" => aggregate_year_points(&scaled_monthly),
        _ => scaled_monthly,
    };
    if expanded.is_empty() {
        return vec![];
    }
    let keep = periods.max(1).min(expanded.len());
    expanded[expanded.len().saturating_sub(keep)..].to_vec()
}

fn apply_economy_realism_tick(
    manifest: &mut ProjectManifest,
    frame: &HistoryFrameLite,
    accrued_fare_revenue_base: f64,
    accrued_boardings_pax: f64,
    completed_alightings_for_revenue: f64,
    service_opex_per_hour: f64,
    staff_opex_per_hour: f64,
    dt_s: f64,
) -> (f64, f64, f64) {
    let difficulty_profile = resolved_difficulty_profile(manifest);
    let accrued_fare_revenue_base = sanitize_non_negative(accrued_fare_revenue_base);
    let accrued_boardings_pax = sanitize_non_negative(accrued_boardings_pax);
    manifest.economy.fare_revenue_deferred_base = sanitize_non_negative(
        manifest.economy.fare_revenue_deferred_base + accrued_fare_revenue_base,
    );
    manifest.economy.fare_boardings_deferred_pax =
        sanitize_non_negative(manifest.economy.fare_boardings_deferred_pax + accrued_boardings_pax);
    let completed_alightings = sanitize_non_negative(completed_alightings_for_revenue);
    let recognized_boardings =
        completed_alightings.min(manifest.economy.fare_boardings_deferred_pax.max(0.0));
    let recognition_ratio = if manifest.economy.fare_boardings_deferred_pax > 0.0 {
        (recognized_boardings / manifest.economy.fare_boardings_deferred_pax).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let fare_revenue_base =
        (manifest.economy.fare_revenue_deferred_base * recognition_ratio).max(0.0);
    manifest.economy.fare_boardings_deferred_pax =
        (manifest.economy.fare_boardings_deferred_pax - recognized_boardings).max(0.0);
    manifest.economy.fare_revenue_deferred_base =
        (manifest.economy.fare_revenue_deferred_base - fare_revenue_base).max(0.0);
    let ancillary_revenue_base = fare_revenue_base
        * manifest.economy.ancillary_revenue_rate.max(0.0)
        * difficulty_profile.ancillary_revenue_mult.max(0.0);
    let delta_revenue_base = fare_revenue_base + ancillary_revenue_base;
    let base_opex = (service_opex_per_hour + staff_opex_per_hour).max(0.0)
        * difficulty_profile.opex_mult.max(0.0)
        * (dt_s / 3600.0);
    let maintenance_reserve = manifest.economy.cumulative_capex_base.max(0.0)
        * manifest.economy.maintenance_rate.max(0.0)
        * difficulty_profile.maintenance_mult.max(0.0)
        * (dt_s / ECONOMY_MONTH_SECONDS);
    let delta_opex_base = base_opex + maintenance_reserve;
    let overcrowding_penalty_base = frame.kpis.total_overflow_dropped.max(0.0)
        * manifest
            .economy
            .quality_penalty_rates
            .overcrowding_base_per_passenger
            .max(0.0);
    let reliability_gap =
        (frame.kpis.total_boardings_attempted - frame.kpis.total_boardings_served).max(0.0);
    let reliability_penalty_base = reliability_gap
        * manifest
            .economy
            .quality_penalty_rates
            .reliability_base_per_passenger
            .max(0.0);
    let delta_penalty_base = (overcrowding_penalty_base + reliability_penalty_base)
        * difficulty_profile.penalty_mult.max(0.0);
    let delta_net_base = delta_revenue_base - delta_opex_base - delta_penalty_base;
    manifest.economy.current_balance_base += delta_net_base;
    manifest.economy.cumulative_revenue_base += delta_revenue_base;
    manifest.economy.cumulative_opex_base += delta_opex_base;
    manifest.economy.cumulative_lost_demand_penalty_base += delta_penalty_base;
    update_region_ledger(
        manifest,
        delta_revenue_base,
        delta_opex_base,
        delta_penalty_base,
        0.0,
    );
    record_monthly_financial_delta(
        manifest,
        delta_revenue_base,
        delta_opex_base,
        0.0,
        delta_penalty_base,
    );
    if delta_revenue_base.abs() > 1e-9
        || delta_opex_base.abs() > 1e-9
        || delta_penalty_base.abs() > 1e-9
    {
        bump_economy_revision(manifest);
    }
    (delta_revenue_base, delta_opex_base, delta_net_base)
}

fn build_live_service_loads(
    gs: &interlinked_engine::platform::GameState,
) -> Vec<LiveServiceLoadLite> {
    #[derive(Default)]
    struct ServiceLoadAccumulator {
        departures_observed: usize,
        vehicle_capacity: f64,
        boarded_total: f64,
        boarded_by_stop: HashMap<String, f64>,
        alighted_by_stop: HashMap<String, f64>,
    }

    let mut service_load_map = BTreeMap::<String, ServiceLoadAccumulator>::new();
    if let Some(output) = gs.last_output.as_ref() {
        for board_load in &output.board_loads {
            let service_id = board_load.service_id.trim();
            let stop_id = board_load.stop_id.trim();
            if service_id.is_empty() || stop_id.is_empty() {
                continue;
            }
            let boarded = (board_load.served_from_arrivals + board_load.served_from_queue).max(0.0);
            let alighted = board_load.alightings_served.max(0.0);
            let vehicle_capacity = board_load.vehicle_capacity.max(0.0);
            let accumulator = service_load_map.entry(service_id.to_string()).or_default();
            accumulator.departures_observed = accumulator
                .departures_observed
                .max(board_load.departures_observed);
            accumulator.vehicle_capacity = accumulator.vehicle_capacity.max(vehicle_capacity);
            accumulator.boarded_total += boarded;
            *accumulator
                .boarded_by_stop
                .entry(stop_id.to_string())
                .or_insert(0.0) += boarded;
            *accumulator
                .alighted_by_stop
                .entry(stop_id.to_string())
                .or_insert(0.0) += alighted;
        }
    }

    let mut sequence_by_service = HashMap::<String, Vec<String>>::new();
    for service in &gs.store.scenario().world.services {
        let service_id = service.id.trim();
        if service_id.is_empty() || service.stop_sequence.is_empty() {
            continue;
        }
        sequence_by_service.insert(service_id.to_string(), service.stop_sequence.clone());
    }

    service_load_map
        .into_iter()
        .map(|(service_id, accumulator)| {
            let departures = accumulator.departures_observed as f64;
            let vehicle_capacity = accumulator.vehicle_capacity.max(0.0);
            let mut peak_onboard = 0.0_f64;

            if let Some(sequence) = sequence_by_service.get(&service_id) {
                let mut onboard = 0.0_f64;
                for stop_id in sequence {
                    if let Some(alighted) = accumulator.alighted_by_stop.get(stop_id) {
                        onboard = (onboard - alighted.max(0.0)).max(0.0);
                    }
                    if let Some(boarded) = accumulator.boarded_by_stop.get(stop_id) {
                        onboard += boarded.max(0.0);
                    }
                    peak_onboard = peak_onboard.max(onboard);
                }
            } else {
                peak_onboard = accumulator.boarded_total.max(0.0);
            }

            let per_departure_peak = if departures > 0.0 {
                peak_onboard / departures
            } else {
                0.0
            };
            let load_to_capacity = if vehicle_capacity > 0.0 {
                per_departure_peak / vehicle_capacity
            } else {
                0.0
            };

            LiveServiceLoadLite {
                service_id,
                load_to_capacity: load_to_capacity.clamp(0.0, 1.0),
            }
        })
        .collect::<Vec<_>>()
}

fn runtime_service_units_assigned(service: &Service) -> usize {
    service
        .stock_units_assigned
        .or_else(|| {
            service
                .rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.units_owned)
        })
        .or(service.stock_units_owned)
        .unwrap_or(0) as usize
}

fn runtime_service_enabled(service: &Service) -> bool {
    if matches!(service.service_enabled, Some(false)) {
        return false;
    }
    if !service.headway_s.is_finite() || service.headway_s <= 0.0 || service.headway_s >= 86_399.0 {
        return false;
    }
    if let Some(tph) = service.operating_tph {
        if !tph.is_finite() || tph <= 0.0 {
            return false;
        }
    }
    runtime_service_units_assigned(service) > 0
}

fn runtime_stop_display_name(stop: &interlinked_engine::model::Stop) -> String {
    stop.name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| stop.id.clone())
}

fn runtime_service_line_name(service: &Service) -> String {
    service
        .name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| service_line_runtime_id(service))
}

fn build_runtime_service_profiles(
    scenario: &Scenario,
) -> (
    HashMap<String, RuntimeServiceProfile>,
    HashMap<String, String>,
) {
    #[derive(Debug, Clone)]
    struct RuntimeServiceDraft {
        service_id: String,
        line_id: String,
        line_name: String,
        mode: String,
        mode_variant: Option<String>,
        stock_tier_id: Option<String>,
        dwell_s: f64,
        turnaround_s: f64,
        speed_mps: f64,
        vehicle_capacity: f64,
        stop_ids: Vec<String>,
        stop_xy: Vec<(f64, f64)>,
        segment_lengths_m: Vec<f64>,
        unit_weight: f64,
    }

    let mut stop_xy_by_id = HashMap::<String, (f64, f64)>::new();
    let mut stop_name_by_id = HashMap::<String, String>::new();
    for stop in &scenario.world.stops {
        stop_xy_by_id.insert(stop.id.clone(), (stop.x, stop.y));
        stop_name_by_id.insert(stop.id.clone(), runtime_stop_display_name(stop));
    }
    let mut link_speed_by_pair = HashMap::<(String, String, String), f64>::new();
    for link in &scenario.world.links {
        let key = (
            link.from_stop.clone(),
            link.to_stop.clone(),
            normalized_mode_token(&link.mode),
        );
        link_speed_by_pair
            .entry(key)
            .or_insert(link.speed_mps.max(0.1));
    }

    let mut drafts = Vec::<RuntimeServiceDraft>::new();
    let mut line_units_cap = HashMap::<String, usize>::new();
    for service in &scenario.world.services {
        if !runtime_service_enabled(service) {
            continue;
        }
        let units_assigned = runtime_service_units_assigned(service);
        if units_assigned == 0 || service.stop_sequence.len() < 2 {
            continue;
        }
        let mut stop_ids = Vec::<String>::new();
        let mut stop_xy = Vec::<(f64, f64)>::new();
        for stop_id in &service.stop_sequence {
            if let Some((x, y)) = stop_xy_by_id.get(stop_id).copied() {
                stop_ids.push(stop_id.clone());
                stop_xy.push((x, y));
            }
        }
        if stop_ids.len() < 2 || stop_xy.len() < 2 {
            continue;
        }
        let mut segment_lengths_m = Vec::<f64>::new();
        let mut speed_sum = 0.0_f64;
        let mut speed_count = 0usize;
        for idx in 1..stop_xy.len() {
            let (from_x, from_y) = stop_xy[idx - 1];
            let (to_x, to_y) = stop_xy[idx];
            let dx = to_x - from_x;
            let dy = to_y - from_y;
            let segment_m = (dx * dx + dy * dy).sqrt().max(1.0);
            segment_lengths_m.push(segment_m);
            let speed_key = (
                stop_ids[idx - 1].clone(),
                stop_ids[idx].clone(),
                normalized_mode_token(&service.mode),
            );
            if let Some(speed) = link_speed_by_pair.get(&speed_key).copied() {
                speed_sum += speed.max(0.1);
                speed_count += 1;
            }
        }
        if segment_lengths_m.is_empty() {
            continue;
        }
        let speed_mps = if speed_count > 0 {
            (speed_sum / speed_count as f64).max(0.5)
        } else {
            12.0
        };
        let dwell_s = service.dwell_s.max(8.0);
        let turnaround_s = dwell_s.max(20.0);
        let vehicle_capacity = service.vehicle_capacity.max(0.0);
        let line_id = service_line_runtime_id(service);
        line_units_cap
            .entry(line_id.clone())
            .and_modify(|value| *value = (*value).max(units_assigned))
            .or_insert(units_assigned);
        let unit_weight = service
            .operating_tph
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or_else(|| (3600.0 / service.headway_s.max(1.0)).max(0.0))
            .max(0.1);
        drafts.push(RuntimeServiceDraft {
            service_id: service.id.clone(),
            line_id,
            line_name: runtime_service_line_name(service),
            mode: service.mode.clone(),
            mode_variant: service.mode_variant.clone(),
            stock_tier_id: service
                .rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.package_id.clone())
                .or_else(|| service.stock_tier_id.clone()),
            dwell_s,
            turnaround_s,
            speed_mps,
            vehicle_capacity,
            stop_ids,
            stop_xy,
            segment_lengths_m,
            unit_weight,
        });
    }

    let mut draft_indices_by_line = BTreeMap::<String, Vec<usize>>::new();
    for (idx, draft) in drafts.iter().enumerate() {
        draft_indices_by_line
            .entry(draft.line_id.clone())
            .or_default()
            .push(idx);
    }

    let mut vehicles_by_service = HashMap::<String, usize>::new();
    for (line_id, indices) in draft_indices_by_line {
        if indices.is_empty() {
            continue;
        }
        let total_units = line_units_cap
            .get(&line_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, 64);
        if total_units == 0 {
            continue;
        }
        let mut allocations = vec![0usize; indices.len()];
        let weight_sum: f64 = indices
            .iter()
            .map(|idx| drafts[*idx].unit_weight.max(0.0))
            .sum::<f64>()
            .max(1e-9);
        let mut fractional = Vec::<(usize, f64, String)>::new();
        let mut assigned = 0usize;
        for (pos, idx) in indices.iter().enumerate() {
            let weight = drafts[*idx].unit_weight.max(0.0);
            let raw = (total_units as f64) * (weight / weight_sum);
            let base = raw.floor().max(0.0) as usize;
            allocations[pos] = base;
            assigned = assigned.saturating_add(base);
            fractional.push((pos, raw - base as f64, drafts[*idx].service_id.clone()));
        }

        let mut remainder = total_units.saturating_sub(assigned);
        fractional.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });
        for (pos, _frac, _service_id) in &fractional {
            if remainder == 0 {
                break;
            }
            allocations[*pos] = allocations[*pos].saturating_add(1);
            remainder = remainder.saturating_sub(1);
        }

        if total_units >= indices.len() {
            let mut donor_order = (0..indices.len()).collect::<Vec<_>>();
            donor_order.sort_by(|a, b| {
                allocations[*b].cmp(&allocations[*a]).then_with(|| {
                    drafts[indices[*a]]
                        .service_id
                        .cmp(&drafts[indices[*b]].service_id)
                })
            });
            for pos in 0..indices.len() {
                if allocations[pos] > 0 {
                    continue;
                }
                if let Some(donor) = donor_order
                    .iter()
                    .copied()
                    .find(|idx| allocations[*idx] > 1)
                {
                    allocations[donor] = allocations[donor].saturating_sub(1);
                    allocations[pos] = 1;
                }
            }
        }

        for (pos, idx) in indices.iter().enumerate() {
            vehicles_by_service.insert(drafts[*idx].service_id.clone(), allocations[pos]);
        }
    }

    let mut out = HashMap::<String, RuntimeServiceProfile>::new();
    for draft in drafts {
        let vehicles_on_service = vehicles_by_service
            .get(&draft.service_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, 64);
        if vehicles_on_service == 0 {
            continue;
        }
        out.insert(
            draft.service_id.clone(),
            RuntimeServiceProfile {
                service_id: draft.service_id,
                line_id: draft.line_id,
                line_name: draft.line_name,
                mode: draft.mode,
                mode_variant: draft.mode_variant,
                stock_tier_id: draft.stock_tier_id,
                dwell_s: draft.dwell_s,
                turnaround_s: draft.turnaround_s,
                speed_mps: draft.speed_mps,
                vehicle_capacity: draft.vehicle_capacity,
                vehicles_on_service,
                stop_ids: draft.stop_ids,
                stop_xy: draft.stop_xy,
                segment_lengths_m: draft.segment_lengths_m,
            },
        );
    }
    (out, stop_name_by_id)
}

fn build_runtime_reverse_service_pairs(
    profiles_by_service: &HashMap<String, RuntimeServiceProfile>,
) -> HashMap<String, String> {
    let mut services_by_line = HashMap::<String, Vec<&RuntimeServiceProfile>>::new();
    for profile in profiles_by_service.values() {
        services_by_line
            .entry(profile.line_id.clone())
            .or_default()
            .push(profile);
    }

    for services in services_by_line.values_mut() {
        services.sort_by(|a, b| a.service_id.cmp(&b.service_id));
    }

    let mut reverse_by_service = HashMap::<String, String>::new();
    for services in services_by_line.values() {
        for profile in services {
            if profile.stop_ids.len() < 2 {
                continue;
            }
            let mut reverse_sequence = profile.stop_ids.clone();
            reverse_sequence.reverse();
            if reverse_sequence == profile.stop_ids {
                continue;
            }
            let reverse_service = services
                .iter()
                .filter(|candidate| candidate.service_id != profile.service_id)
                .find(|candidate| candidate.stop_ids == reverse_sequence)
                .map(|candidate| candidate.service_id.clone());
            if let Some(reverse_id) = reverse_service {
                reverse_by_service.insert(profile.service_id.clone(), reverse_id);
            }
        }
    }
    reverse_by_service
}

fn new_runtime_train_state(
    profile: &RuntimeServiceProfile,
    unit_index: usize,
) -> RuntimeTrainState {
    let segment_count = profile.segment_lengths_m.len().max(1);
    let base_segment = unit_index % segment_count;
    let progress = if profile.vehicles_on_service > 0 {
        (unit_index as f64 / profile.vehicles_on_service as f64).fract()
    } else {
        0.0
    };
    let mut state = RuntimeTrainState {
        train_id: format!(
            "train::{}::{}::{}",
            profile.line_id,
            profile.service_id,
            unit_index.saturating_add(1)
        ),
        service_id: profile.service_id.clone(),
        line_id: profile.line_id.clone(),
        line_name: profile.line_name.clone(),
        mode: profile.mode.clone(),
        mode_variant: profile.mode_variant.clone(),
        stock_tier_id: profile.stock_tier_id.clone(),
        vehicle_capacity: profile.vehicle_capacity.max(0.0),
        current_stop_index: base_segment.min(profile.stop_ids.len().saturating_sub(1)),
        direction_step: 1,
        phase: RuntimeTrainPhase::Moving,
        progress,
        remaining_s: 0.0,
        onboard_pax: 0.0,
        onboard_cohorts: HashMap::new(),
    };
    if profile.stop_ids.len() < 2 {
        state.phase = RuntimeTrainPhase::Dwell;
        state.progress = 0.0;
        state.remaining_s = profile.dwell_s;
        state.current_stop_index = 0;
    }
    state
}

fn runtime_next_stop_index(
    current_stop_index: usize,
    direction_step: i8,
    stop_count: usize,
) -> Option<usize> {
    if stop_count < 2 {
        return None;
    }
    if direction_step >= 0 {
        if current_stop_index + 1 < stop_count {
            Some(current_stop_index + 1)
        } else {
            None
        }
    } else if current_stop_index >= 1 {
        Some(current_stop_index - 1)
    } else {
        None
    }
}

fn runtime_train_onboard_total(train: &RuntimeTrainState) -> f64 {
    train
        .onboard_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>()
        .max(0.0)
}

fn apply_runtime_departure_boarding(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    stop_id: &str,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeFareEvents {
    let mut events = RuntimeFareEvents::default();
    let mut onboard_total = runtime_train_onboard_total(train);
    if let Some(alight_here) = train.onboard_cohorts.remove(stop_id) {
        if alight_here > 0.0 {
            onboard_total = (onboard_total - alight_here).max(0.0);
            events.completed_alightings_pax += alight_here;
        }
    }

    let capacity = profile.vehicle_capacity.max(0.0);
    let mut residual_capacity = (capacity - onboard_total).max(0.0);
    if residual_capacity > 0.0 {
        let stop_index = train
            .current_stop_index
            .min(profile.stop_ids.len().saturating_sub(1));
        if train.direction_step >= 0 {
            for idx in (stop_index + 1)..profile.stop_ids.len() {
                if residual_capacity <= 1e-6 {
                    break;
                }
                let destination = &profile.stop_ids[idx];
                let key = (
                    profile.service_id.clone(),
                    stop_id.to_string(),
                    destination.clone(),
                );
                let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
                if queued <= 0.0 {
                    continue;
                }
                let boarded = residual_capacity.min(queued);
                if boarded <= 0.0 {
                    continue;
                }
                let remaining = (queued - boarded).max(0.0);
                if remaining > 1e-6 {
                    queue_cohorts.insert(key, remaining);
                } else {
                    queue_cohorts.remove(&key);
                }
                *train
                    .onboard_cohorts
                    .entry(destination.clone())
                    .or_insert(0.0) += boarded;
                events.boarded_pax += boarded;
                events.liability_accrued_base += boarded * fare_base_per_boarding.max(0.0);
                residual_capacity = (residual_capacity - boarded).max(0.0);
            }
        } else {
            for idx in (0..stop_index).rev() {
                if residual_capacity <= 1e-6 {
                    break;
                }
                let destination = &profile.stop_ids[idx];
                let key = (
                    profile.service_id.clone(),
                    stop_id.to_string(),
                    destination.clone(),
                );
                let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
                if queued <= 0.0 {
                    continue;
                }
                let boarded = residual_capacity.min(queued);
                if boarded <= 0.0 {
                    continue;
                }
                let remaining = (queued - boarded).max(0.0);
                if remaining > 1e-6 {
                    queue_cohorts.insert(key, remaining);
                } else {
                    queue_cohorts.remove(&key);
                }
                *train
                    .onboard_cohorts
                    .entry(destination.clone())
                    .or_insert(0.0) += boarded;
                events.boarded_pax += boarded;
                events.liability_accrued_base += boarded * fare_base_per_boarding.max(0.0);
                residual_capacity = (residual_capacity - boarded).max(0.0);
            }
        }
    }
    train.onboard_pax = runtime_train_onboard_total(train);
    events
}

fn advance_runtime_train(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    dt_s: f64,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeFareEvents {
    let mut events = RuntimeFareEvents::default();
    if profile.stop_ids.len() < 2 {
        train.phase = RuntimeTrainPhase::Dwell;
        train.current_stop_index = 0;
        train.progress = 0.0;
        train.remaining_s = profile.dwell_s;
        train.onboard_pax = 0.0;
        train.onboard_cohorts.clear();
        return events;
    }
    if train.current_stop_index >= profile.stop_ids.len() {
        train.current_stop_index = profile.stop_ids.len() - 1;
    }
    let mut remaining_dt = dt_s.max(0.0);
    let mut hops = 0usize;
    while remaining_dt > 1e-6 && hops < 24 {
        hops += 1;
        match train.phase {
            RuntimeTrainPhase::Moving => {
                let Some(next_stop_index) = runtime_next_stop_index(
                    train.current_stop_index,
                    train.direction_step,
                    profile.stop_ids.len(),
                ) else {
                    train.phase = RuntimeTrainPhase::Layover;
                    train.progress = 0.0;
                    train.remaining_s = profile.turnaround_s;
                    train.direction_step *= -1;
                    continue;
                };
                let seg_idx = train.current_stop_index.min(next_stop_index);
                let travel_s = (profile.segment_lengths_m[seg_idx].max(1.0)
                    / profile.speed_mps.max(0.5))
                .max(0.1);
                let move_remaining = (1.0 - train.progress.clamp(0.0, 1.0)) * travel_s;
                if remaining_dt < move_remaining {
                    train.progress = (train.progress + (remaining_dt / travel_s)).clamp(0.0, 1.0);
                    remaining_dt = 0.0;
                } else {
                    remaining_dt -= move_remaining;
                    train.current_stop_index = next_stop_index;
                    train.progress = 0.0;
                    train.phase = RuntimeTrainPhase::Dwell;
                    train.remaining_s = profile.dwell_s;
                }
            }
            RuntimeTrainPhase::Dwell | RuntimeTrainPhase::Layover => {
                let phase_remaining = train.remaining_s.max(0.0);
                if remaining_dt < phase_remaining {
                    train.remaining_s -= remaining_dt;
                    remaining_dt = 0.0;
                } else {
                    remaining_dt -= phase_remaining;
                    train.remaining_s = 0.0;
                    let stop_id = profile.stop_ids[train.current_stop_index].clone();
                    let delta = apply_runtime_departure_boarding(
                        train,
                        profile,
                        &stop_id,
                        queue_cohorts,
                        fare_base_per_boarding,
                    );
                    events.boarded_pax += delta.boarded_pax;
                    events.completed_alightings_pax += delta.completed_alightings_pax;
                    events.liability_accrued_base += delta.liability_accrued_base;
                    if runtime_next_stop_index(
                        train.current_stop_index,
                        train.direction_step,
                        profile.stop_ids.len(),
                    )
                    .is_some()
                    {
                        train.phase = RuntimeTrainPhase::Moving;
                        train.progress = 0.0;
                    } else {
                        train.phase = RuntimeTrainPhase::Layover;
                        train.progress = 0.0;
                        train.direction_step *= -1;
                        train.remaining_s = profile.turnaround_s;
                    }
                }
            }
        }
    }
    events
}

fn runtime_train_position_xy(
    train: &RuntimeTrainState,
    profile: &RuntimeServiceProfile,
) -> (f64, f64, Option<String>, bool) {
    if profile.stop_xy.is_empty() || train.current_stop_index >= profile.stop_xy.len() {
        return (0.0, 0.0, None, false);
    }
    if train.phase == RuntimeTrainPhase::Moving {
        if let Some(next_stop_index) = runtime_next_stop_index(
            train.current_stop_index,
            train.direction_step,
            profile.stop_xy.len(),
        ) {
            let (from_x, from_y) = profile.stop_xy[train.current_stop_index];
            let (to_x, to_y) = profile.stop_xy[next_stop_index];
            let t = train.progress.clamp(0.0, 1.0);
            return (
                from_x + (to_x - from_x) * t,
                from_y + (to_y - from_y) * t,
                None,
                true,
            );
        }
    }
    let (x, y) = profile.stop_xy[train.current_stop_index];
    (
        x,
        y,
        Some(profile.stop_ids[train.current_stop_index].clone()),
        false,
    )
}

fn build_runtime_ops_views(
    state: &AppState,
    project_path: &str,
    scenario: &Scenario,
    output: Option<&SimulationOutput>,
    fare_policy: &FarePolicyManifest,
    dt_s: f64,
    topology_hash: u64,
    emit_runtime_views: bool,
) -> Result<
    (
        Vec<TrainRuntimeView>,
        Vec<StationRuntimeView>,
        Vec<LineOpsRuntimeView>,
        Vec<String>,
        RuntimeFareEvents,
    ),
    String,
> {
    #[derive(Debug, Clone, Default)]
    struct StationAgg {
        current_inside_pax: f64,
        capacity_pax: f64,
        declined_last_hour: f64,
        entries_per_hour: f64,
        exits_per_hour: f64,
        weighted_wait_sum_s: f64,
        weighted_wait_pax: f64,
    }
    #[derive(Debug, Clone, Default)]
    struct LineAgg {
        boardings_attempted_per_hour: f64,
        boarded_per_hour: f64,
        alighted_per_hour: f64,
        denied_boardings_per_hour: f64,
        queue_end_pax: f64,
        weighted_wait_sum_s: f64,
        weighted_wait_pax: f64,
    }
    let mut station_agg = HashMap::<String, StationAgg>::new();
    let mut line_agg = HashMap::<String, LineAgg>::new();
    let line_id_by_service = scenario
        .world
        .services
        .iter()
        .map(|service| (service.id.clone(), service_line_runtime_id(service)))
        .collect::<HashMap<_, _>>();

    if emit_runtime_views {
        if let Some(sim_output) = output {
            for load in &sim_output.board_loads {
                let served_total = (load.served_from_arrivals + load.served_from_queue).max(0.0);
                let alightings = load.alightings_served.max(0.0);
                let period_s = if load.departures_observed > 0 && load.headway_s > 0.0 {
                    (load.departures_observed as f64 * load.headway_s).max(1.0)
                } else if load.departures_in_period > 0.0 && load.headway_s > 0.0 {
                    (load.departures_in_period * load.headway_s).max(1.0)
                } else {
                    300.0
                };
                let to_hour = 3600.0 / period_s.max(1.0);
                let queue_end = load.queue_end.max(0.0);
                let queue_cap = load.station_queue_capacity_pax.max(0.0);
                let overflow = load.overflow_dropped.max(0.0);
                let arrivals = load.arrivals.max(0.0);
                let admitted_entries = (arrivals - overflow).max(0.0);
                let wait_s = load.extra_wait_s.max(0.0);

                let st_entry = station_agg.entry(load.stop_id.clone()).or_default();
                st_entry.current_inside_pax += queue_end;
                st_entry.capacity_pax = st_entry.capacity_pax.max(queue_cap);
                st_entry.declined_last_hour += overflow * to_hour;
                st_entry.entries_per_hour += admitted_entries * to_hour;
                st_entry.exits_per_hour += alightings * to_hour;
                st_entry.weighted_wait_sum_s += wait_s * served_total;
                st_entry.weighted_wait_pax += served_total;

                if let Some(line_id) = line_id_by_service.get(&load.service_id) {
                    let ln_entry = line_agg.entry(line_id.clone()).or_default();
                    ln_entry.boardings_attempted_per_hour += arrivals * to_hour;
                    ln_entry.boarded_per_hour += served_total * to_hour;
                    ln_entry.alighted_per_hour += alightings * to_hour;
                    ln_entry.denied_boardings_per_hour += load.denied_boardings.max(0.0) * to_hour;
                    ln_entry.queue_end_pax += queue_end;
                    ln_entry.weighted_wait_sum_s += wait_s * served_total;
                    ln_entry.weighted_wait_pax += served_total;
                }
            }
        }
    }

    let mut guard = state
        .runtime_ops
        .lock()
        .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
    let should_reset = guard
        .as_ref()
        .map(|ops| ops.project_path != project_path)
        .unwrap_or(true);
    if should_reset {
        *guard = Some(RuntimeOpsState {
            project_path: project_path.to_string(),
            topology_hash,
            profiles_by_service: HashMap::new(),
            stop_name_by_id: HashMap::new(),
            reverse_service_by_service: HashMap::new(),
            stop_ids_by_service: HashMap::new(),
            fare_base_by_service: HashMap::new(),
            dispatch_service_ids: HashSet::new(),
            trains: BTreeMap::new(),
            queue_cohorts: HashMap::new(),
        });
    }
    let Some(ops) = guard.as_mut() else {
        return Ok((
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RuntimeFareEvents::default(),
        ));
    };
    if ops.topology_hash != topology_hash || ops.profiles_by_service.is_empty() {
        let (profiles_by_service, stop_name_by_id) = build_runtime_service_profiles(scenario);
        let dispatch_service_ids = profiles_by_service.keys().cloned().collect::<HashSet<_>>();
        let reverse_service_by_service = build_runtime_reverse_service_pairs(&profiles_by_service);
        let stop_ids_by_service = scenario
            .world
            .services
            .iter()
            .map(|service| {
                (
                    service.id.clone(),
                    service
                        .stop_sequence
                        .iter()
                        .cloned()
                        .collect::<HashSet<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        let fare_base_by_service = profiles_by_service
            .iter()
            .map(|(service_id, profile)| {
                (
                    service_id.clone(),
                    runtime_fare_base_per_boarding(fare_policy, &profile.mode),
                )
            })
            .collect::<HashMap<_, _>>();
        ops.profiles_by_service = profiles_by_service;
        ops.stop_name_by_id = stop_name_by_id;
        ops.reverse_service_by_service = reverse_service_by_service;
        ops.stop_ids_by_service = stop_ids_by_service;
        ops.fare_base_by_service = fare_base_by_service;
        ops.dispatch_service_ids = dispatch_service_ids;
    }
    ops.topology_hash = topology_hash;
    ops.queue_cohorts
        .retain(|(service_id, board_stop_id, destination_stop_id), queued| {
            ops.dispatch_service_ids.contains(service_id)
                && ops
                    .stop_ids_by_service
                    .get(service_id)
                    .map(|stops| {
                        stops.contains(board_stop_id) && stops.contains(destination_stop_id)
                    })
                    .unwrap_or(false)
                && queued.is_finite()
                && *queued > 1e-6
        });
    if let Some(sim_output) = output {
        let mut arrivals_by_key = HashMap::<(String, String, String), f64>::new();
        for cohort in &sim_output.passenger_cohorts {
            if !ops.dispatch_service_ids.contains(&cohort.service_id) {
                continue;
            }
            let arrivals = cohort.attempted_pax.max(0.0);
            if arrivals <= 0.0 {
                continue;
            }
            let key = (
                cohort.service_id.clone(),
                cohort.board_stop_id.clone(),
                cohort.destination_stop_id.clone(),
            );
            *arrivals_by_key.entry(key).or_insert(0.0) += arrivals;
        }
        let mut sorted_arrivals = arrivals_by_key.into_iter().collect::<Vec<_>>();
        sorted_arrivals.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, arrivals) in sorted_arrivals {
            let current = ops.queue_cohorts.get(&key).copied().unwrap_or(0.0);
            let next = (current + arrivals).max(0.0);
            if next > 1e-6 {
                ops.queue_cohorts.insert(key, next);
            } else {
                ops.queue_cohorts.remove(&key);
            }
        }
    }
    ops.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > 1e-6);

    let mut expected_train_ids = HashSet::<String>::new();
    for profile in ops.profiles_by_service.values() {
        for unit_index in 0..profile.vehicles_on_service {
            let train_id = format!(
                "train::{}::{}::{}",
                profile.line_id,
                profile.service_id,
                unit_index.saturating_add(1)
            );
            expected_train_ids.insert(train_id.clone());
            let entry = ops
                .trains
                .entry(train_id.clone())
                .or_insert_with(|| new_runtime_train_state(profile, unit_index));
            entry.train_id = train_id;
            // Preserve any active reverse-service handoff for this physical train slot.
            if !ops.profiles_by_service.contains_key(&entry.service_id) {
                entry.service_id = profile.service_id.clone();
            }
            if let Some(active_profile) = ops.profiles_by_service.get(&entry.service_id) {
                if active_profile.line_id != profile.line_id {
                    entry.service_id = profile.service_id.clone();
                }
            }
            let active_profile = ops
                .profiles_by_service
                .get(&entry.service_id)
                .unwrap_or(profile);
            entry.line_id = active_profile.line_id.clone();
            entry.line_name = active_profile.line_name.clone();
            entry.mode = active_profile.mode.clone();
            entry.mode_variant = active_profile.mode_variant.clone();
            entry.stock_tier_id = active_profile.stock_tier_id.clone();
            entry.vehicle_capacity = active_profile.vehicle_capacity.max(0.0);
            if entry.current_stop_index >= active_profile.stop_ids.len() {
                entry.current_stop_index = 0;
                entry.phase = RuntimeTrainPhase::Dwell;
                entry.progress = 0.0;
                entry.remaining_s = active_profile.dwell_s;
                entry.direction_step = 1;
                entry.onboard_pax = 0.0;
                entry.onboard_cohorts.clear();
            }
        }
    }
    ops.trains
        .retain(|train_id, _| expected_train_ids.contains(train_id));

    let queue_total_before_boarding = ops
        .queue_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>();
    let mut fare_events = RuntimeFareEvents::default();
    for train in ops.trains.values_mut() {
        if let Some(profile) = ops.profiles_by_service.get(&train.service_id) {
            let fare_base_per_boarding = ops
                .fare_base_by_service
                .get(&train.service_id)
                .copied()
                .unwrap_or(0.0);
            let delta = advance_runtime_train(
                train,
                profile,
                dt_s,
                &mut ops.queue_cohorts,
                fare_base_per_boarding,
            );
            fare_events.boarded_pax += delta.boarded_pax;
            fare_events.completed_alightings_pax += delta.completed_alightings_pax;
            fare_events.liability_accrued_base += delta.liability_accrued_base;
            train.onboard_pax = runtime_train_onboard_total(train);

            if train.phase == RuntimeTrainPhase::Layover && train.direction_step < 0 {
                if let Some(reverse_service_id) =
                    ops.reverse_service_by_service.get(&train.service_id)
                {
                    if let Some(reverse_profile) = ops.profiles_by_service.get(reverse_service_id) {
                        let current_stop_id = profile
                            .stop_ids
                            .get(train.current_stop_index)
                            .cloned()
                            .unwrap_or_default();
                        if let Some(reverse_index) = reverse_profile
                            .stop_ids
                            .iter()
                            .position(|stop_id| stop_id == &current_stop_id)
                        {
                            train.service_id = reverse_profile.service_id.clone();
                            train.line_id = reverse_profile.line_id.clone();
                            train.line_name = reverse_profile.line_name.clone();
                            train.mode = reverse_profile.mode.clone();
                            train.mode_variant = reverse_profile.mode_variant.clone();
                            train.stock_tier_id = reverse_profile.stock_tier_id.clone();
                            train.vehicle_capacity = reverse_profile.vehicle_capacity.max(0.0);
                            train.current_stop_index = reverse_index;
                            train.direction_step = 1;
                        }
                    }
                }
            }
        }
    }
    let queue_total_after_boarding = ops
        .queue_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>();

    if !emit_runtime_views {
        let mut provenance_warnings = Vec::<String>::new();
        if !ops.profiles_by_service.is_empty() {
            provenance_warnings.push(
                "derived_calibrated: runtime ops advanced without materializing views on this tick"
                    .to_string(),
            );
        }
        return Ok((
            Vec::new(),
            Vec::new(),
            Vec::new(),
            provenance_warnings,
            fare_events,
        ));
    }

    let mut trains_sorted = ops.trains.values().cloned().collect::<Vec<_>>();
    trains_sorted.sort_by(|a, b| {
        a.line_id
            .cmp(&b.line_id)
            .then_with(|| a.train_id.cmp(&b.train_id))
    });
    let mut line_ordinals = HashMap::<String, u32>::new();
    let mut train_views = Vec::<TrainRuntimeView>::new();
    for train in trains_sorted {
        let Some(profile) = ops.profiles_by_service.get(&train.service_id) else {
            continue;
        };
        let (x, y, at_stop_id, in_motion) = runtime_train_position_xy(&train, profile);
        let direction_label = if train.direction_step >= 0 {
            "Outbound".to_string()
        } else {
            "Inbound".to_string()
        };
        let destination_index = if train.direction_step >= 0 {
            profile.stop_ids.len().saturating_sub(1)
        } else {
            0
        };
        let destination_stop_id = profile
            .stop_ids
            .get(destination_index)
            .cloned()
            .unwrap_or_default();
        let destination_label = ops
            .stop_name_by_id
            .get(&destination_stop_id)
            .map(|name| format!("To {name}"))
            .unwrap_or_else(|| direction_label.clone());
        let vehicle_ordinal = line_ordinals
            .entry(train.line_id.clone())
            .and_modify(|v| *v = v.saturating_add(1))
            .or_insert(1);
        train_views.push(TrainRuntimeView {
            train_id: train.train_id.clone(),
            service_id: train.service_id.clone(),
            line_id: train.line_id.clone(),
            line_name: train.line_name.clone(),
            vehicle_ordinal: *vehicle_ordinal,
            direction_label,
            destination_stop_id,
            destination_label,
            mode: train.mode.clone(),
            mode_variant: train.mode_variant.clone(),
            stock_tier_id: train.stock_tier_id.clone(),
            vehicle_capacity: train.vehicle_capacity.max(0.0),
            onboard_pax: train.onboard_pax.max(0.0),
            x,
            y,
            at_stop_id,
            in_motion,
            provenance: "derived_calibrated".to_string(),
        });
    }

    let mut queue_inside_by_stop = HashMap::<String, f64>::new();
    for ((_service_id, stop_id, _destination_stop_id), queued) in &ops.queue_cohorts {
        *queue_inside_by_stop.entry(stop_id.clone()).or_insert(0.0) += queued.max(0.0);
    }

    let mut station_ids = station_agg.keys().cloned().collect::<BTreeSet<_>>();
    station_ids.extend(queue_inside_by_stop.keys().cloned());

    let mut station_views = Vec::<StationRuntimeView>::new();
    for stop_id in station_ids {
        let agg = station_agg.get(&stop_id).cloned().unwrap_or_default();
        let queue_inside = queue_inside_by_stop.get(&stop_id).copied().unwrap_or(0.0);
        let current_inside = if agg.capacity_pax > 0.0 {
            queue_inside.min(agg.capacity_pax)
        } else {
            queue_inside
        };
        let avg_wait = if agg.weighted_wait_pax > 0.0 {
            agg.weighted_wait_sum_s / agg.weighted_wait_pax
        } else {
            0.0
        };
        station_views.push(StationRuntimeView {
            stop_id,
            current_inside_pax: current_inside.max(0.0),
            capacity_pax: agg.capacity_pax.max(0.0),
            declined_last_hour: agg.declined_last_hour.max(0.0),
            entries_per_hour: agg.entries_per_hour.max(0.0),
            exits_per_hour: agg.exits_per_hour.max(0.0),
            avg_wait_to_board_s: avg_wait.max(0.0),
            provenance: "derived_calibrated".to_string(),
        });
    }
    station_views.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut active_trains_by_line = HashMap::<String, u32>::new();
    for train in &train_views {
        *active_trains_by_line
            .entry(train.line_id.clone())
            .or_insert(0) += 1;
    }
    let mut queue_end_by_line = HashMap::<String, f64>::new();
    for ((service_id, _stop_id, _destination_stop_id), queued) in &ops.queue_cohorts {
        if let Some(profile) = ops.profiles_by_service.get(service_id) {
            *queue_end_by_line
                .entry(profile.line_id.clone())
                .or_insert(0.0) += queued.max(0.0);
        }
    }
    let mut line_ids = line_agg
        .keys()
        .cloned()
        .chain(active_trains_by_line.keys().cloned())
        .chain(queue_end_by_line.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    line_ids.sort();
    let mut line_ops = Vec::<LineOpsRuntimeView>::new();
    for line_id in line_ids {
        let agg = line_agg.get(&line_id).cloned().unwrap_or_default();
        let mean_wait_s = if agg.weighted_wait_pax > 0.0 {
            agg.weighted_wait_sum_s / agg.weighted_wait_pax
        } else {
            0.0
        };
        line_ops.push(LineOpsRuntimeView {
            line_id: line_id.clone(),
            active_trains: *active_trains_by_line.get(&line_id).unwrap_or(&0),
            boardings_attempted_per_hour: agg.boardings_attempted_per_hour.max(0.0),
            boarded_per_hour: agg.boarded_per_hour.max(0.0),
            alighted_per_hour: agg.alighted_per_hour.max(0.0),
            denied_boardings_per_hour: agg.denied_boardings_per_hour.max(0.0),
            queue_end_pax: queue_end_by_line
                .get(&line_id)
                .copied()
                .unwrap_or(0.0)
                .max(0.0),
            mean_wait_s: mean_wait_s.max(0.0),
            provenance: "derived_calibrated".to_string(),
        });
    }

    let mut provenance_warnings = Vec::<String>::new();
    if !ops.profiles_by_service.is_empty() {
        provenance_warnings.push(
            "derived_calibrated: train onboard and station flow are reconstructed from deterministic service-stop cohorts and board-load events"
                .to_string(),
        );
    }
    let queue_drop = (queue_total_before_boarding - queue_total_after_boarding).max(0.0);
    let boarding_mass_mismatch = (queue_drop - fare_events.boarded_pax.max(0.0)).abs();
    if boarding_mass_mismatch > 1e-3 {
        provenance_warnings.push(format!(
            "derived_calibrated: queue/boarding conservation mismatch detected ({boarding_mass_mismatch:.3} pax)"
        ));
    }
    if scenario.world.services.iter().any(|service| {
        let service_intent = !matches!(service.service_enabled, Some(false))
            && service.headway_s.is_finite()
            && service.headway_s > 0.0
            && service.headway_s < 86_399.0
            && service.operating_tph.unwrap_or(1.0) > 0.0;
        service_intent && runtime_service_units_assigned(service) == 0
    }) {
        provenance_warnings.push(
            "authored: service has zero assigned stock and is suppressed from dispatch".to_string(),
        );
    }

    Ok((
        train_views,
        station_views,
        line_ops,
        provenance_warnings,
        fare_events,
    ))
}

fn runtime_fare_base_per_boarding(policy: &FarePolicyManifest, mode: &str) -> f64 {
    if !policy.enabled {
        return 0.0;
    }
    match fare_mode_bucket_from_tokens(mode, None, 0.0) {
        FareModeBucket::Bus => policy.fare_mode_bus_base.max(0.0),
        FareModeBucket::Tram => policy.fare_mode_tram_base.max(0.0),
        FareModeBucket::Metro => policy.fare_mode_metro_base.max(0.0),
        FareModeBucket::Rail => policy.fare_mode_rail_base.max(0.0),
        FareModeBucket::Ferry => policy.fare_mode_ferry_base.max(0.0),
        FareModeBucket::Default => policy.fare_mode_default_base.max(0.0),
    }
}

fn fare_flow_for_economy(gs: &interlinked_engine::platform::GameState) -> (f64, f64, f64) {
    let Some(output) = gs.last_output.as_ref() else {
        return (0.0, 0.0, 0.0);
    };
    let liability_base = output.fare_flow.liability_accrued_base.max(0.0);
    let liability_pax = output.fare_flow.liability_accrued_pax.max(0.0);
    let completed_pax = output.fare_flow.completed_journeys_pax.max(0.0);
    if liability_base > 0.0 || liability_pax > 0.0 || completed_pax > 0.0 {
        return (liability_base, liability_pax, completed_pax);
    }

    // Backward-compatible fallback for older outputs without fare_flow population.
    let mut fallback_completed = 0.0_f64;
    for load in &output.board_loads {
        let alightings = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        if alightings <= 0.0 {
            continue;
        }
        if load.departures_observed > 0 {
            fallback_completed += alightings;
        }
    }
    (
        output.kpis.total_fare_revenue_base.max(0.0),
        output.kpis.total_boardings_served.max(0.0),
        fallback_completed.max(0.0),
    )
}

fn adaptive_runtime_zone_cap(
    manifest: &ProjectManifest,
    scenario: &Scenario,
    previous_cap: usize,
    previous_tick_ms: f64,
) -> usize {
    let focus_cap = manifest
        .simulation_scope
        .focus_max_active_zones
        .clamp(120, 6000);
    let floor_cap = manifest
        .simulation_scope
        .remote_max_active_zones
        .clamp(20, focus_cap);
    let base_cap = manifest
        .simulation_scope
        .max_active_zones
        .clamp(floor_cap, focus_cap);
    let stop_count = scenario.world.stops.len();
    let network_cap = if stop_count <= 25 {
        48
    } else if stop_count <= 80 {
        96
    } else if stop_count <= 200 {
        160
    } else if stop_count <= 500 {
        240
    } else if stop_count <= 1_200 {
        320
    } else {
        420
    };
    let mut cap = previous_cap
        .clamp(floor_cap, focus_cap)
        .min(base_cap)
        .min(network_cap);
    let target_ms = manifest.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0);
    if previous_tick_ms > 0.0 {
        if previous_tick_ms > target_ms * 1.35 {
            cap = ((cap as f64) * 0.82).round() as usize;
        } else if previous_tick_ms < target_ms * 0.70 {
            cap = ((cap as f64) * 1.08).round() as usize;
        }
    }
    cap.clamp(floor_cap, focus_cap)
}

fn ensure_runtime_materialized_scenario(
    state: &AppState,
    project_path: &str,
    gs: &mut interlinked_engine::platform::GameState,
    manifest: &ProjectManifest,
    minute_of_day: u32,
    tick_index: u64,
) -> Result<usize, String> {
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let active_scope_hash = scope_hash(manifest);
    let active_fare_hash = fare_hash(&manifest.economy.fare_policy);
    let existing = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?
        .clone();
    let mut materialization = existing.unwrap_or(RuntimeMaterializationState {
        project_path: project_path.to_string(),
        topology_hash: 0,
        scope_hash: 0,
        fare_hash: 0,
        minute_of_day,
        last_materialized_tick: 0,
        adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
        last_tick_ms: 0.0,
    });
    if materialization.project_path != project_path {
        materialization = RuntimeMaterializationState {
            project_path: project_path.to_string(),
            topology_hash: 0,
            scope_hash: 0,
            fare_hash: 0,
            minute_of_day,
            last_materialized_tick: 0,
            adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
            last_tick_ms: 0.0,
        };
    }
    let adaptive_cap = adaptive_runtime_zone_cap(
        manifest,
        gs.store.scenario(),
        materialization.adaptive_max_active_zones,
        materialization.last_tick_ms,
    );
    let remote_interval = manifest
        .simulation_scope
        .remote_update_interval_ticks
        .max(1) as u64;
    let cap_delta = adaptive_cap.abs_diff(materialization.adaptive_max_active_zones);
    let cap_rebalance_due = cap_delta >= 32
        && tick_index.saturating_sub(materialization.last_materialized_tick) >= remote_interval;
    let needs_materialization = topology_hash != materialization.topology_hash
        || active_scope_hash != materialization.scope_hash
        || active_fare_hash != materialization.fare_hash
        || minute_of_day != materialization.minute_of_day
        || cap_rebalance_due;
    if needs_materialization {
        let cfg = economy_config();
        let mut materialized = gs.store.scenario().clone();
        strip_auto_reverse_runtime_artifacts(&mut materialized);
        apply_game_runtime_demand_tuning(&mut materialized.params);
        apply_fare_policy_to_params(&mut materialized.params, &manifest.economy.fare_policy);
        synthesize_auto_reverse_runtime_services(&mut materialized);
        materialize_line_operations_for_minute(&mut materialized, &cfg, minute_of_day);
        apply_game_runtime_perf_budget(&mut materialized, adaptive_cap);
        gs.store = ScenarioStore::new(materialized);
        materialization.topology_hash = topology_hash;
        materialization.scope_hash = active_scope_hash;
        materialization.fare_hash = active_fare_hash;
        materialization.minute_of_day = minute_of_day;
        materialization.last_materialized_tick = tick_index;
        materialization.adaptive_max_active_zones = adaptive_cap;
    } else {
        materialization.adaptive_max_active_zones = adaptive_cap;
    }
    let mut guard = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
    *guard = Some(materialization);
    Ok(adaptive_cap)
}

fn runtime_snapshot_to_advance(snapshot: &RuntimeSnapshot) -> Option<SimulationAdvanceResult> {
    snapshot.frame.clone().map(|frame| SimulationAdvanceResult {
        frame,
        clock: snapshot.clock.clone(),
        economy: snapshot.economy.clone(),
        delta_revenue_base: snapshot.delta_revenue_base,
        delta_opex_base: snapshot.delta_opex_base,
        delta_net_base: snapshot.delta_net_base,
    })
}

fn merge_runtime_manifest_state(
    mut reloaded: ProjectManifest,
    runtime: &ProjectManifest,
) -> ProjectManifest {
    let use_runtime_economy = runtime.economy.economy_revision >= reloaded.economy.economy_revision;
    reloaded.clock_state.tick_seconds = reloaded
        .clock_state
        .tick_seconds
        .max(runtime.clock_state.tick_seconds);
    if use_runtime_economy {
        reloaded.economy = runtime.economy.clone();
        match (
            reloaded.progress_metrics.as_mut(),
            runtime.progress_metrics.as_ref(),
        ) {
            (Some(reloaded_metrics), Some(runtime_metrics)) => {
                reloaded_metrics.budget = runtime_metrics.budget;
                reloaded_metrics.ridership = runtime_metrics.ridership;
            }
            (None, Some(runtime_metrics)) => {
                reloaded.progress_metrics = Some(runtime_metrics.clone());
            }
            _ => {}
        }
    } else {
        sync_progress_budget_from_economy(&mut reloaded);
        match (
            reloaded.progress_metrics.as_mut(),
            runtime.progress_metrics.as_ref(),
        ) {
            (Some(reloaded_metrics), Some(runtime_metrics)) => {
                reloaded_metrics.ridership =
                    reloaded_metrics.ridership.max(runtime_metrics.ridership);
            }
            (None, Some(runtime_metrics)) => {
                reloaded.progress_metrics = Some(runtime_metrics.clone());
            }
            _ => {}
        }
    }
    reloaded
}

fn runtime_has_due_purchase_orders(scenario: &Scenario, now_tick_s: f64) -> bool {
    if !now_tick_s.is_finite() || now_tick_s < 0.0 {
        return false;
    }
    scenario.world.services.iter().any(|service| {
        service
            .rolling_stock_profile
            .as_ref()
            .map(|profile| {
                profile.pending_orders.iter().any(|order| {
                    order
                        .eta_at_tick_s
                        .map(|eta| eta.is_finite() && eta <= now_tick_s + 1e-6)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

fn run_simulation_tick(
    state: &AppState,
    project_root: &Path,
    manifest: &mut ProjectManifest,
    dt_s: f64,
    fixed_step_s: f64,
    recompute_quick_kpis: bool,
    tick_index: u64,
    clock_revision: u64,
    queue_depth: usize,
    dropped_steps: u32,
    emit_runtime_views: bool,
    strategic_refresh_due_hint: bool,
) -> Result<RuntimeSnapshot, String> {
    let tick_start = Instant::now();
    let mut telemetry = RuntimePerfTelemetry {
        tick_index,
        dt_s,
        fixed_step_s,
        queue_depth,
        dropped_steps,
        ..RuntimePerfTelemetry::default()
    };
    let mut guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    let gs = guard
        .as_mut()
        .ok_or_else(|| "game not initialised for current session".to_string())?;
    let prepare_start = Instant::now();
    if let Some(step_kernel) = gs.run_cfg.step_kernel.as_mut() {
        step_kernel.k_paths = 1;
        step_kernel.msa_max_iters = 1;
        step_kernel.convergence_rel = 1.0;
        step_kernel.route_choice_theta = 0.002;
    }
    gs.run_cfg.enable_kernel_partitioning = manifest.runtime_scheduling.runtime_ops_kernel_v1;
    gs.run_cfg.strategic_refresh_interval_steps = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
    if runtime_has_due_purchase_orders(gs.store.scenario(), manifest.clock_state.tick_seconds) {
        let mut scenario_with_orders = gs.store.scenario().clone();
        let delivered_orders = settle_pending_purchase_orders(
            &mut scenario_with_orders,
            manifest.clock_state.tick_seconds,
        );
        if delivered_orders > 0 {
            gs.store = ScenarioStore::new(scenario_with_orders);
        }
    }
    let minute_of_day = clock_minute_of_day(&manifest.clock_state);
    let adaptive_cap = ensure_runtime_materialized_scenario(
        state,
        &project_root.to_string_lossy(),
        gs,
        manifest,
        minute_of_day,
        tick_index,
    )?;
    telemetry.adaptive_max_active_zones = adaptive_cap;
    telemetry.stage_prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.0;

    let scope = SimulationScope {
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        remote_regions_mode: manifest.simulation_scope.remote_regions_mode.clone(),
        max_active_zones: adaptive_cap,
    };
    let step_start = Instant::now();
    let req = interlinked_engine::platform::GameStepRequest {
        recompute_quick_kpis,
        edits: Vec::new(),
        force_strategic_refresh: false,
    };
    gs.run_cfg.lightweight_outputs = manifest.runtime_scheduling.lightweight_tick_outputs;
    let step_output = SimulationService::step_game_scoped(gs, dt_s, req, &scope)?;
    telemetry.stage_step_ms = step_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.engine_strategic_refresh_executed = step_output.strategic_refresh_executed;
    telemetry.engine_strategic_refresh_reason = step_output
        .strategic_refresh_reason
        .map(|reason| format!("{reason:?}"));
    telemetry.engine_fast_steps = step_output.kernel_perf.fast_steps;
    telemetry.engine_strategic_steps = step_output.kernel_perf.strategic_steps;
    telemetry.engine_fast_last_ms = step_output.kernel_perf.last_fast_ms;
    telemetry.engine_strategic_last_ms = step_output.kernel_perf.last_strategic_ms;
    telemetry.engine_fast_avg_ms = step_output.kernel_perf.avg_fast_ms();
    telemetry.engine_strategic_avg_ms = step_output.kernel_perf.avg_strategic_ms();
    telemetry.engine_steps_since_last_strategic =
        step_output.kernel_perf.steps_since_last_strategic;
    telemetry.engine_strategic_cache_hits = step_output.kernel_perf.strategic_cache_hits;
    telemetry.engine_strategic_cache_misses = step_output.kernel_perf.strategic_cache_misses;

    let econ_start = Instant::now();
    let cfg = economy_config();
    let frame = interlinked_engine::platform::history_last_frame(gs)
        .ok_or_else(|| "no history frame available after step".to_string())?;
    let service_opex_per_hour = interlinked_engine::platform::estimate_service_opex_per_hour_base(
        gs.store.scenario(),
        &cfg,
    );
    let staff_opex_per_hour =
        builder_support::estimate_staff_opex_per_hour_base(gs.store.scenario(), &cfg);
    manifest.clock_state.tick_seconds = gs.tick_s;
    let frame_lite = HistoryFrameLite {
        t_s: frame.t_s,
        kpis: frame.kpis.clone(),
        queue_summary: frame.queue_summary.clone(),
        service_loads: if step_output.strategic_refresh_executed {
            build_live_service_loads(gs)
        } else {
            Vec::new()
        },
    };
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let trains_authoritative = runtime_trains_authoritative_for_manifest(manifest);
    let ops_start = Instant::now();
    let (
        runtime_trains,
        runtime_stations,
        runtime_line_ops,
        provenance_warnings,
        runtime_fare_events,
    ) = if trains_authoritative {
        let (trains, stations, line_ops, warnings, fare_events) = build_runtime_ops_views(
            state,
            &project_root.to_string_lossy(),
            gs.store.scenario(),
            gs.last_output.as_ref(),
            &manifest.economy.fare_policy,
            dt_s,
            topology_hash,
            emit_runtime_views,
        )?;
        (trains, stations, line_ops, warnings, fare_events)
    } else {
        if let Ok(mut guard) = state.runtime_ops.lock() {
            *guard = None;
        }
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RuntimeFareEvents::default(),
        )
    };
    telemetry.stage_runtime_ops_ms = ops_start.elapsed().as_secs_f64() * 1000.0;
    let fare_recognition_enabled = runtime_fare_recognition_enabled_for_manifest(manifest);
    let (accrued_fare_revenue_base, accrued_boardings_pax, mut completed_alightings_pax) =
        if fare_recognition_enabled {
            if manifest.session_kind == SessionKind::Game && trains_authoritative {
                let boarded = runtime_fare_events.boarded_pax.max(0.0);
                let completed = runtime_fare_events.completed_alightings_pax.max(0.0);
                let liability = runtime_fare_events.liability_accrued_base.max(0.0);
                (liability, boarded, completed)
            } else {
                fare_flow_for_economy(gs)
            }
        } else {
            let served = frame_lite.kpis.total_boardings_served.max(0.0);
            (
                frame_lite.kpis.total_fare_revenue_base.max(0.0),
                served,
                served,
            )
        };
    completed_alightings_pax = completed_alightings_pax.max(0.0);
    let (delta_revenue_base, delta_opex_base, delta_net_base) = apply_economy_realism_tick(
        manifest,
        &frame_lite,
        accrued_fare_revenue_base,
        accrued_boardings_pax,
        completed_alightings_pax,
        service_opex_per_hour,
        staff_opex_per_hour,
        dt_s,
    );
    if let Some(metrics) = manifest.progress_metrics.as_mut() {
        metrics.ridership += frame.kpis.total_trips_served.max(0.0);
    }
    sync_progress_budget_from_economy(manifest);
    let economy = SimulationAdvanceEconomy {
        current_balance_base: manifest.economy.current_balance_base,
        cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
        cumulative_opex_base: manifest.economy.cumulative_opex_base,
        budget_display: manifest
            .progress_metrics
            .as_ref()
            .map(|m| m.budget)
            .unwrap_or(manifest.economy.current_balance_base),
    };
    telemetry.stage_economy_ms = econ_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.strategic_refresh_due = strategic_refresh_due_hint;
    telemetry.strategic_refresh_interval_ticks = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
    telemetry.runtime_views_materialized = emit_runtime_views && trains_authoritative;
    telemetry.tick_total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
    {
        let mut materialization = state
            .runtime_materialization
            .lock()
            .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
        if let Some(current) = materialization.as_mut() {
            if current.project_path == project_root.to_string_lossy() {
                current.last_tick_ms = telemetry.tick_total_ms;
            }
        }
    }
    Ok(RuntimeSnapshot {
        project_path: project_root.to_string_lossy().to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        economy,
        frame: Some(frame_lite),
        delta_revenue_base,
        delta_opex_base,
        delta_net_base,
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry,
        trains: runtime_trains,
        stations: runtime_stations,
        line_ops: runtime_line_ops,
        provenance_warnings,
        trains_authoritative,
    })
}

fn default_runtime_fast_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeFastSnapshot {
    RuntimeFastSnapshot {
        project_path: project_path.to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry: RuntimePerfTelemetry::default(),
        trains: Vec::new(),
        stations: Vec::new(),
        line_ops: Vec::new(),
        provenance_warnings: Vec::new(),
        trains_authoritative: runtime_trains_authoritative_for_manifest(manifest),
    }
}

fn default_runtime_strategic_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeStrategicSnapshot {
    RuntimeStrategicSnapshot {
        project_path: project_path.to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        economy: SimulationAdvanceEconomy {
            current_balance_base: manifest.economy.current_balance_base,
            cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
            cumulative_opex_base: manifest.economy.cumulative_opex_base,
            budget_display: manifest
                .progress_metrics
                .as_ref()
                .map(|m| m.budget)
                .unwrap_or(manifest.economy.current_balance_base),
        },
        frame: None,
        delta_revenue_base: 0.0,
        delta_opex_base: 0.0,
        delta_net_base: 0.0,
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry: RuntimePerfTelemetry::default(),
        provenance_warnings: Vec::new(),
        trains_authoritative: runtime_trains_authoritative_for_manifest(manifest),
    }
}

fn default_runtime_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeSnapshot {
    let fast = default_runtime_fast_snapshot_for_manifest(project_path, manifest, clock_revision);
    let strategic =
        default_runtime_strategic_snapshot_for_manifest(project_path, manifest, clock_revision);
    runtime_snapshot_from_parts(&fast, Some(&strategic))
}

fn bootstrap_runtime_snapshot_from_state(
    state: &AppState,
    project_path: &str,
    manifest: &ProjectManifest,
    scenario: &Scenario,
    clock_revision: u64,
) -> Result<RuntimeSnapshot, String> {
    let mut snapshot =
        default_runtime_snapshot_for_manifest(project_path, manifest, clock_revision);
    snapshot.clock.running = manifest.clock_state.running;
    snapshot.clock.speed = normalize_speed(manifest.clock_state.speed);
    snapshot.clock.tick_seconds = manifest.clock_state.tick_seconds;
    snapshot.clock_revision = clock_revision;
    snapshot.captured_at_epoch_ms = now_epoch_ms();
    snapshot.telemetry.snapshot_age_ms = 0;
    let trains_authoritative = runtime_trains_authoritative_for_manifest(manifest);
    snapshot.trains_authoritative = trains_authoritative;
    if trains_authoritative {
        let topology_hash = scenario_topology_hash(scenario);
        let (trains, stations, line_ops, warnings, _fare_events) = build_runtime_ops_views(
            state,
            project_path,
            scenario,
            None,
            &manifest.economy.fare_policy,
            0.0,
            topology_hash,
            true,
        )?;
        snapshot.trains = trains;
        snapshot.stations = stations;
        snapshot.line_ops = line_ops;
        snapshot.provenance_warnings = warnings;
    }
    Ok(snapshot)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str::<T>(&raw).map_err(|e| e.to_string())
}

fn empty_feature_collection_json() -> JsonValue {
    serde_json::json!({
        "type": "FeatureCollection",
        "features": []
    })
}

fn read_feature_collection_json(path: &Path) -> Result<JsonValue, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value =
        serde_json::from_str::<JsonValue>(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let is_feature_collection = value
        .get("type")
        .and_then(|v| v.as_str())
        .map(|v| v == "FeatureCollection")
        .unwrap_or(false);
    if !is_feature_collection {
        return Err(format!("{} is not a FeatureCollection", path.display()));
    }
    Ok(value)
}

fn country_map_context_cache() -> &'static Mutex<HashMap<String, CountryMapContext>> {
    COUNTRY_MAP_CONTEXT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn region_street_context_cache() -> &'static Mutex<HashMap<String, RegionStreetContext>> {
    REGION_STREET_CONTEXT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gb_county_landuse_cache() -> &'static Mutex<HashMap<String, CountyLanduseProfile>> {
    GB_COUNTY_LANDUSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gb_county_mode_constraint_cache(
) -> &'static Mutex<HashMap<String, Arc<CountyModeConstraintData>>> {
    GB_COUNTY_MODE_CONSTRAINT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn parse_session_kind(value: Option<&str>) -> SessionKind {
    match value.unwrap_or("game").to_ascii_lowercase().as_str() {
        "scenario" => SessionKind::Scenario,
        "sandbox" => SessionKind::Game,
        _ => SessionKind::Game,
    }
}

fn default_clock_for(_kind: &SessionKind) -> SimulationClock {
    SimulationClock {
        sim_datetime_utc: DEFAULT_SIM_START_UTC.to_string(),
        tick_seconds: 0.0,
        running: false,
        speed: 1,
    }
}

fn normalize_speed(speed: u32) -> u32 {
    match speed {
        1 | 2 | 4 => speed,
        _ => 1,
    }
}

fn default_sim_speed() -> u32 {
    1
}

fn default_currency_code() -> String {
    "GBP".to_string()
}

fn normalize_currency(value: Option<&str>) -> String {
    normalize_currency_code(value.unwrap_or("GBP"))
}

fn parse_speed_value(value: Option<&JsonValue>) -> u32 {
    match value {
        Some(JsonValue::Number(n)) => n.as_u64().map(|v| v as u32).unwrap_or(1),
        Some(JsonValue::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "x1" => 1,
            "2" | "x2" => 2,
            "4" | "x4" => 4,
            _ => 1,
        },
        _ => 1,
    }
}

fn default_progress_metrics() -> GameProgressMetrics {
    GameProgressMetrics {
        budget: 0.0,
        currency: default_currency_code(),
        ridership: 0.0,
        coverage: 0.0,
        milestones: 0,
    }
}

fn default_fare_enabled_manifest() -> bool {
    true
}

fn default_fare_mode_bus_base() -> f64 {
    1.8
}

fn default_fare_mode_tram_base() -> f64 {
    2.3
}

fn default_fare_mode_metro_base() -> f64 {
    2.7
}

fn default_fare_mode_rail_base() -> f64 {
    3.6
}

fn default_fare_mode_ferry_base() -> f64 {
    3.0
}

fn default_fare_mode_default_base() -> f64 {
    2.5
}

fn default_fare_transfer_window_s() -> f64 {
    2700.0
}

fn default_fare_free_transfers_per_trip() -> u8 {
    1
}

fn default_fare_policy_manifest() -> FarePolicyManifest {
    FarePolicyManifest {
        enabled: default_fare_enabled_manifest(),
        fare_mode_bus_base: default_fare_mode_bus_base(),
        fare_mode_tram_base: default_fare_mode_tram_base(),
        fare_mode_metro_base: default_fare_mode_metro_base(),
        fare_mode_rail_base: default_fare_mode_rail_base(),
        fare_mode_ferry_base: default_fare_mode_ferry_base(),
        fare_mode_default_base: default_fare_mode_default_base(),
        transfer_window_s: default_fare_transfer_window_s(),
        free_transfers_per_trip: default_fare_free_transfers_per_trip(),
    }
}

fn default_maintenance_rate() -> f64 {
    // Monthly maintenance reserve as a fraction of cumulative capex.
    0.003
}

fn default_ancillary_revenue_rate() -> f64 {
    // Ancillary revenue as a fraction of fare revenue.
    0.06
}

fn default_overcrowding_penalty_rate() -> f64 {
    1.2
}

fn default_reliability_penalty_rate() -> f64 {
    0.4
}

fn default_quality_penalty_rates() -> QualityPenaltyRates {
    QualityPenaltyRates {
        overcrowding_base_per_passenger: default_overcrowding_penalty_rate(),
        reliability_base_per_passenger: default_reliability_penalty_rate(),
    }
}

fn default_economy_manifest() -> EconomyManifest {
    EconomyManifest {
        currency: default_currency_code(),
        difficulty: default_difficulty_label(),
        difficulty_profile: difficulty_profile_for_label("standard"),
        economy_revision: default_economy_revision(),
        starting_budget_base: 0.0,
        current_balance_base: 0.0,
        cumulative_capex_base: 0.0,
        cumulative_opex_base: 0.0,
        cumulative_revenue_base: 0.0,
        cumulative_lost_demand_penalty_base: 0.0,
        fare_revenue_deferred_base: 0.0,
        fare_boardings_deferred_pax: 0.0,
        fare_policy: default_fare_policy_manifest(),
        unlocked_countries: vec![],
        region_ledger: BTreeMap::new(),
        maintenance_rate: default_maintenance_rate(),
        ancillary_revenue_rate: default_ancillary_revenue_rate(),
        quality_penalty_rates: default_quality_penalty_rates(),
        monthly_financials: Vec::new(),
    }
}

fn sanitize_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn sanitize_fare_policy(policy: &mut FarePolicyManifest) {
    policy.fare_mode_bus_base = sanitize_non_negative(policy.fare_mode_bus_base);
    policy.fare_mode_tram_base = sanitize_non_negative(policy.fare_mode_tram_base);
    policy.fare_mode_metro_base = sanitize_non_negative(policy.fare_mode_metro_base);
    policy.fare_mode_rail_base = sanitize_non_negative(policy.fare_mode_rail_base);
    policy.fare_mode_ferry_base = sanitize_non_negative(policy.fare_mode_ferry_base);
    policy.fare_mode_default_base = sanitize_non_negative(policy.fare_mode_default_base);
    policy.transfer_window_s = sanitize_non_negative(policy.transfer_window_s);
    if policy.free_transfers_per_trip > 4 {
        policy.free_transfers_per_trip = 4;
    }
}

fn merge_fare_policy(policy: &mut FarePolicyManifest, patch: &FarePolicyPatch) {
    if let Some(v) = patch.enabled {
        policy.enabled = v;
    }
    if let Some(v) = patch.fare_mode_bus_base {
        policy.fare_mode_bus_base = v;
    }
    if let Some(v) = patch.fare_mode_tram_base {
        policy.fare_mode_tram_base = v;
    }
    if let Some(v) = patch.fare_mode_metro_base {
        policy.fare_mode_metro_base = v;
    }
    if let Some(v) = patch.fare_mode_rail_base {
        policy.fare_mode_rail_base = v;
    }
    if let Some(v) = patch.fare_mode_ferry_base {
        policy.fare_mode_ferry_base = v;
    }
    if let Some(v) = patch.fare_mode_default_base {
        policy.fare_mode_default_base = v;
    }
    if let Some(v) = patch.transfer_window_s {
        policy.transfer_window_s = v;
    }
    if let Some(v) = patch.free_transfers_per_trip {
        policy.free_transfers_per_trip = v;
    }
    sanitize_fare_policy(policy);
}

fn apply_fare_policy_to_params(params: &mut Params, policy: &FarePolicyManifest) {
    params.fare_enabled = policy.enabled;
    params.fare_mode_bus_base = policy.fare_mode_bus_base;
    params.fare_mode_tram_base = policy.fare_mode_tram_base;
    params.fare_mode_metro_base = policy.fare_mode_metro_base;
    params.fare_mode_rail_base = policy.fare_mode_rail_base;
    params.fare_mode_ferry_base = policy.fare_mode_ferry_base;
    params.fare_mode_default_base = policy.fare_mode_default_base;
    params.fare_transfer_window_s = policy.transfer_window_s;
    params.fare_free_transfers_per_trip = policy.free_transfers_per_trip;
}

fn apply_game_runtime_demand_tuning(params: &mut Params) {
    // Game mode should prioritize visible transit usage over pure walk-only shortest paths.
    // Keep this runtime-only so scenario/planning analysis remains neutral.
    params.walk_weight = params.walk_weight.max(4.0);
    params.trips_per_person = params.trips_per_person.max(3.0);
    params.gravity_beta = params.gravity_beta.min(0.00025);
}

fn apply_game_runtime_perf_budget(scenario: &mut Scenario, max_cells: usize) {
    if max_cells == 0 || scenario.world.demand_cells.len() <= max_cells {
        return;
    }

    let stop_points = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.x, stop.y))
        .collect::<Vec<_>>();
    let proximity_scale_m = 2_500.0_f64;
    let mut ranked = scenario
        .world
        .demand_cells
        .iter()
        .enumerate()
        .map(|(idx, cell)| {
            let mass = (cell.residents_night.max(0.0) + cell.jobs_day.max(0.0)).max(1.0);
            let proximity = if stop_points.is_empty() {
                0.0
            } else {
                let mut best_d2 = f64::INFINITY;
                for (sx, sy) in &stop_points {
                    let dx = cell.x - *sx;
                    let dy = cell.y - *sy;
                    let d2 = dx * dx + dy * dy;
                    if d2 < best_d2 {
                        best_d2 = d2;
                    }
                }
                if best_d2.is_finite() {
                    let rel = best_d2.sqrt() / proximity_scale_m;
                    1.0 / (1.0 + rel * rel)
                } else {
                    0.0
                }
            };
            let score = mass * (1.0 + 2.5 * proximity);
            (idx, score, cell.cell_id.clone())
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
    });

    let keep_indices = ranked
        .into_iter()
        .take(max_cells)
        .map(|(idx, _, _)| idx)
        .collect::<HashSet<_>>();
    let mut trimmed = Vec::with_capacity(max_cells);
    for (idx, cell) in scenario.world.demand_cells.iter().enumerate() {
        if keep_indices.contains(&idx) {
            trimmed.push(cell.clone());
        }
    }
    scenario.world.demand_cells = trimmed;
}

fn default_demand_surface_manifest() -> DemandSurfaceManifest {
    DemandSurfaceManifest {
        surface_version: "v4".to_string(),
        loaded_countries: vec![],
        pack_version: None,
        last_rebuild_at: None,
    }
}

fn default_max_active_zones() -> usize {
    600
}

fn default_remote_regions_mode() -> String {
    "aggregate".to_string()
}

fn default_remote_update_interval_ticks() -> u32 {
    10
}

fn default_focus_max_active_zones() -> usize {
    480
}

fn default_adjacent_max_active_zones() -> usize {
    220
}

fn default_remote_max_active_zones() -> usize {
    80
}

fn default_adjacent_update_interval_ticks() -> u32 {
    4
}

fn default_runtime_enabled() -> bool {
    true
}

fn default_runtime_fixed_step_s() -> f64 {
    0.5
}

fn default_runtime_max_steps_per_cycle() -> u32 {
    12
}

fn default_runtime_checkpoint_interval_ticks() -> u32 {
    20
}

fn default_runtime_snapshot_ring() -> usize {
    32
}

fn default_runtime_target_tick_ms() -> f64 {
    16.0
}

fn default_runtime_strategic_refresh_interval_ticks() -> u32 {
    8
}

fn default_runtime_lightweight_tick_outputs() -> bool {
    true
}

fn default_runtime_ops_kernel_v1() -> bool {
    true
}

fn default_ui_runtime_trains_v1() -> bool {
    true
}

fn default_fare_recognition_v1() -> bool {
    true
}

fn default_runtime_scheduling_manifest() -> RuntimeSchedulingManifest {
    RuntimeSchedulingManifest {
        enabled: default_runtime_enabled(),
        fixed_step_s: default_runtime_fixed_step_s(),
        max_steps_per_cycle: default_runtime_max_steps_per_cycle(),
        checkpoint_interval_ticks: default_runtime_checkpoint_interval_ticks(),
        snapshot_ring: default_runtime_snapshot_ring(),
        target_tick_ms: default_runtime_target_tick_ms(),
        strategic_refresh_interval_ticks: default_runtime_strategic_refresh_interval_ticks(),
        lightweight_tick_outputs: default_runtime_lightweight_tick_outputs(),
        runtime_ops_kernel_v1: default_runtime_ops_kernel_v1(),
        ui_runtime_trains_v1: default_ui_runtime_trains_v1(),
        fare_recognition_v1: default_fare_recognition_v1(),
    }
}

fn runtime_trains_authoritative_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.runtime_ops_kernel_v1
        && manifest.runtime_scheduling.ui_runtime_trains_v1
}

fn runtime_fare_recognition_enabled_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.fare_recognition_v1
}

fn enforce_game_runtime_hardcut(manifest: &mut ProjectManifest) {
    if manifest.session_kind != SessionKind::Game {
        return;
    }
    manifest.runtime_scheduling.enabled = true;
    manifest.runtime_scheduling.lightweight_tick_outputs = true;
    manifest.runtime_scheduling.runtime_ops_kernel_v1 = true;
    manifest.runtime_scheduling.ui_runtime_trains_v1 = true;
    manifest.runtime_scheduling.fare_recognition_v1 = true;
}

fn default_simulation_scope_manifest() -> SimulationScopeManifest {
    SimulationScopeManifest {
        max_active_zones: default_max_active_zones(),
        remote_regions_mode: default_remote_regions_mode(),
        remote_update_interval_ticks: default_remote_update_interval_ticks(),
        focus_max_active_zones: default_focus_max_active_zones(),
        adjacent_max_active_zones: default_adjacent_max_active_zones(),
        remote_max_active_zones: default_remote_max_active_zones(),
        adjacent_update_interval_ticks: default_adjacent_update_interval_ticks(),
    }
}

fn normalize_scope(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "aggregate" => "aggregate".to_string(),
        _ => "aggregate".to_string(),
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let qq = q.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f64 * qq).round() as usize;
    sorted[idx]
}

fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
    let mut cleaned = values.map(|v| if v.is_finite() && v > 0.0 { v } else { 0.0 });
    let sum: f64 = cleaned.iter().sum();
    if sum <= 1e-9 {
        return [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }
    for v in &mut cleaned {
        *v /= sum;
    }
    cleaned
}

fn default_surface_activity_mix() -> f64 {
    0.0
}

fn legacy_mix_from_residents_jobs(residents: f64, jobs: f64, area_m2: f64) -> [f64; 7] {
    let r = residents.max(0.0);
    let j = jobs.max(0.0);
    if r + j <= 1e-9 {
        return [0.70, 0.08, 0.08, 0.05, 0.05, 0.02, 0.02];
    }
    let total = r + j;
    let resident_balance = (r / total).clamp(0.0, 1.0);
    let employment_balance = (j / total).clamp(0.0, 1.0);
    let area_km2 = (area_m2.max(1.0) / 1_000_000.0).max(0.05);
    let density = total / area_km2;
    let urbanity = ((density + 1.0).log10() / 4.0).clamp(0.0, 1.0);

    normalize_activity_mix([
        0.52 + 0.30 * resident_balance - 0.25 * urbanity,
        0.05 + 0.22 * employment_balance + 0.10 * urbanity,
        0.05 + 0.09 * urbanity + 0.03 * employment_balance,
        0.04 + 0.07 * urbanity,
        0.04 + 0.12 * employment_balance + 0.04 * urbanity,
        0.03 + 0.04 * urbanity,
        0.03 + 0.03 * urbanity,
    ])
}

fn backfill_legacy_surface_mix(surface: &mut DemandSurfaceCountryWire) {
    let fill = |cells: &mut [DemandSurfaceCellWire]| {
        for c in cells {
            let current = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            let current_sum: f64 = current.iter().filter(|v| v.is_finite() && **v >= 0.0).sum();
            let normalized = if current_sum > 1e-9 {
                normalize_activity_mix(current)
            } else {
                legacy_mix_from_residents_jobs(c.residents_smooth, c.jobs_smooth, c.area_m2)
            };
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
    };
    fill(&mut surface.cells_res8);
    fill(&mut surface.cells_res7);
    fill(&mut surface.cells_res6);
}

fn read_manifest(project_root: &Path) -> Result<ProjectManifest, String> {
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
        parsed.economy.unlocked_countries = parsed
            .economy
            .unlocked_countries
            .iter()
            .map(|x| x.trim().to_ascii_uppercase())
            .filter(|x| x.len() == 2)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        sanitize_economy_manifest(&mut parsed.economy);
        canonicalize_region_ledger(&mut parsed.economy.region_ledger);
        if parsed.demand_surface.is_none() {
            parsed.demand_surface = Some(default_demand_surface_manifest());
        }
        parsed.region_state.unlocked_region_ids = parsed
            .region_state
            .unlocked_region_ids
            .iter()
            .filter_map(|x| canonicalize_region_id(x))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        parsed.region_state.active_region_ids = parsed
            .region_state
            .active_region_ids
            .iter()
            .filter_map(|x| canonicalize_region_id(x))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        parsed.region_state.primary_focus_region_id = parsed
            .region_state
            .primary_focus_region_id
            .as_deref()
            .and_then(canonicalize_region_id);
        parsed.pack_refs = parsed
            .pack_refs
            .into_iter()
            .filter_map(|mut p| {
                let iso = p.country_iso2.trim().to_ascii_uppercase();
                if iso.len() != 2 {
                    return None;
                }
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
            parsed.economy.unlocked_countries = parsed
                .economy
                .unlocked_countries
                .iter()
                .map(|x| x.trim().to_ascii_uppercase())
                .filter(|x| x.len() == 2)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            sanitize_economy_manifest(&mut parsed.economy);
            canonicalize_region_ledger(&mut parsed.economy.region_ledger);
            if parsed.demand_surface.is_none() {
                parsed.demand_surface = Some(default_demand_surface_manifest());
            }
            parsed.region_state.unlocked_region_ids = parsed
                .region_state
                .unlocked_region_ids
                .iter()
                .filter_map(|x| canonicalize_region_id(x))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            parsed.region_state.active_region_ids = parsed
                .region_state
                .active_region_ids
                .iter()
                .filter_map(|x| canonicalize_region_id(x))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            parsed.region_state.primary_focus_region_id = parsed
                .region_state
                .primary_focus_region_id
                .as_deref()
                .and_then(canonicalize_region_id);
            parsed.pack_refs = parsed
                .pack_refs
                .into_iter()
                .filter_map(|mut p| {
                    let iso = p.country_iso2.trim().to_ascii_uppercase();
                    if iso.len() != 2 {
                        return None;
                    }
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

fn write_manifest(project_root: &Path, manifest: &ProjectManifest) -> Result<(), String> {
    write_json_file(&manifest_path(project_root), manifest)
}

fn read_index(app: &AppHandle) -> Result<SaveIndex, String> {
    let path = index_path(app)?;
    if !path.exists() {
        return Ok(SaveIndex {
            version: 1,
            projects: vec![],
        });
    }
    read_json_file(&path)
}

fn write_index(app: &AppHandle, idx: &SaveIndex) -> Result<(), String> {
    write_json_file(&index_path(app)?, idx)
}

fn read_deleted_index(app: &AppHandle) -> Result<DeletedIndex, String> {
    let path = deleted_index_path(app)?;
    if !path.exists() {
        return Ok(DeletedIndex {
            version: 1,
            entries: vec![],
        });
    }
    read_json_file(&path)
}

fn write_deleted_index(app: &AppHandle, idx: &DeletedIndex) -> Result<(), String> {
    write_json_file(&deleted_index_path(app)?, idx)
}

fn upsert_index_entry(app: &AppHandle, entry: SaveIndexEntry) -> Result<(), String> {
    let mut idx = read_index(app)?;
    idx.version = 1;
    idx.projects.retain(|p| p.project_id != entry.project_id);
    idx.projects.push(entry);
    idx.projects
        .sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
    write_index(app, &idx)
}

fn remove_index_entry(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let mut idx = read_index(app)?;
    idx.projects.retain(|p| p.project_id != project_id);
    write_index(app, &idx)
}

fn update_index_opened(
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

fn default_params() -> Params {
    Params {
        walk_weight: 1.0,
        wait_weight: 2.0,
        ivt_weight: 1.0,
        transfer_penalty_s: 300.0,
        access_walk_speed_mps: 1.4,
        access_radius_m: 1200.0,
        gravity_beta: 0.0003,
        trips_per_person: 1.0,
        purpose_share_home_work: 0.52,
        purpose_share_home_education: 0.12,
        purpose_share_home_retail: 0.18,
        purpose_share_home_recreation: 0.10,
        purpose_share_other: 0.08,
        attraction_weight_office: 1.0,
        attraction_weight_retail: 0.9,
        attraction_weight_recreation: 0.7,
        attraction_weight_industrial: 1.1,
        attraction_weight_education: 0.8,
        attraction_weight_health: 0.75,
        route_choice_k: 3,
        route_choice_theta: 0.002,
        assignment_max_iters: 8,
        assignment_convergence_rel: 0.01,
        capacity_enabled: true,
        queue_max_extra_wait_s: 3600.0,
        fare_enabled: true,
        fare_value_of_time_base_per_hour: 12.0,
        fare_elasticity: 0.35,
        fare_reference_base: 2.5,
        fare_transfer_window_s: 2700.0,
        fare_free_transfers_per_trip: 1,
        fare_overflow_retry_share: 0.15,
        fare_mode_bus_base: 1.8,
        fare_mode_tram_base: 2.3,
        fare_mode_metro_base: 2.7,
        fare_mode_rail_base: 3.6,
        fare_mode_ferry_base: 3.0,
        fare_mode_default_base: 2.5,
        station_capacity_scale_boarding: 1.0,
        station_capacity_scale_alighting: 1.0,
        station_queue_capacity_scale: 1.0,
        debug_sample_origin_zone: None,
        debug_sample_dest_zone: None,
        demand_profile: vec![],
        demand_purpose_profile: vec![],
    }
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

fn stable_noise_01(a: i32, b: i32, k: f64) -> f64 {
    let n = (a as f64 * 12.9898 + b as f64 * 78.233 + k * 437.585453).sin() * 43758.5453123;
    let frac = n - n.floor();
    if frac.is_finite() {
        frac
    } else {
        0.5
    }
}

fn synthesize_city_demand(
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> (Vec<Zone>, Vec<DemandCell>) {
    let (x, y) = lonlat_to_web_mercator_m(center_lon, center_lat);
    let city_pop = city_population
        .unwrap_or(750_000)
        .clamp(120_000, 30_000_000) as f64;
    let residents_total = city_pop * 1.35;
    let employment_ratio = (0.36 + (city_pop.log10() - 5.0) * 0.08).clamp(0.32, 0.52);
    let jobs_total = residents_total * employment_ratio;
    let phase = center_lon * 0.31 + center_lat * 0.23;
    let city_scale = (city_pop / 750_000.0).powf(0.35).clamp(0.65, 3.0);
    let radius_cells = (4.0 + 2.0 * city_scale).round().clamp(4.0, 9.0) as i32;
    let hex_size_m = (640.0 * city_scale.powf(0.35)).clamp(560.0, 980.0);
    let sqrt3 = 3.0_f64.sqrt();
    let spread_m = (radius_cells as f64 * hex_size_m * 1.8).max(1.0);
    let country = country_iso2
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2);

    #[derive(Clone)]
    struct CellDraft {
        cell_id: String,
        x: f64,
        y: f64,
        residents_weight: f64,
        jobs_weight: f64,
        residential: f64,
        office: f64,
        retail: f64,
        recreation: f64,
        industrial: f64,
        education: f64,
        health: f64,
        centrality: f64,
    }

    let mut drafts = Vec::<CellDraft>::new();
    let mut residents_weight_sum = 0.0;
    let mut jobs_weight_sum = 0.0;

    for q in -radius_cells..=radius_cells {
        let r_min = (-radius_cells).max(-q - radius_cells);
        let r_max = radius_cells.min(-q + radius_cells);
        for r in r_min..=r_max {
            let qf = q as f64;
            let rf = r as f64;
            let px = hex_size_m * (sqrt3 * qf + 0.5 * sqrt3 * rf);
            let py = hex_size_m * (1.5 * rf);
            let jx = (stable_noise_01(q, r, phase + 1.7) - 0.5) * hex_size_m * 0.55;
            let jy = (stable_noise_01(q, r, phase - 2.1) - 0.5) * hex_size_m * 0.55;
            let dx = px + jx;
            let dy = py + jy;

            let ux = dx / spread_m;
            let uy = dy / spread_m;
            let rr = (ux * ux + uy * uy).sqrt();
            let angle = uy.atan2(ux);

            let cbd = (-(ux * ux + uy * uy) / 0.06).exp();
            let c1x = 0.42 * phase.cos();
            let c1y = 0.42 * phase.sin();
            let c2x = 0.56 * (phase + 2.25).cos();
            let c2y = 0.56 * (phase + 2.25).sin();
            let c3x = 0.50 * (phase - 1.95).cos();
            let c3y = 0.50 * (phase - 1.95).sin();
            let sub_center = ((-((ux - c1x).powi(2) + (uy - c1y).powi(2)) / 0.030).exp()
                + (-((ux - c2x).powi(2) + (uy - c2y).powi(2)) / 0.040).exp()
                + (-((ux - c3x).powi(2) + (uy - c3y).powi(2)) / 0.045).exp())
                / 3.0;
            let inner_ring = (-((rr - 0.34).powi(2)) / 0.030).exp();
            let residential_belt = (-((rr - 0.70).powi(2)) / 0.065).exp();
            let periphery = clamp01((rr - 0.58) / 0.45);
            let corridor = (1.0
                + 0.34 * (2.0 * angle + phase).cos()
                + 0.18 * (3.0 * angle - 0.7 * phase).sin())
            .max(0.18);

            let residents_weight = (0.53 * residential_belt
                + 0.20 * periphery
                + 0.13 * inner_ring
                + 0.08 * corridor
                + 0.06 * (1.0 - cbd)
                + 0.04 * (1.0 - sub_center))
                .max(0.01);
            let jobs_weight = (0.56 * cbd
                + 0.24 * sub_center
                + 0.10 * inner_ring
                + 0.07 * corridor
                + 0.03 * (1.0 - periphery))
                .max(0.01);

            let mut residential = (0.48 + 0.50 * residential_belt + 0.16 * periphery
                - 0.30 * cbd
                - 0.08 * sub_center)
                .max(0.01);
            let mut office = (0.06 + 0.92 * cbd + 0.52 * sub_center + 0.08 * corridor
                - 0.22 * residential_belt)
                .max(0.01);
            let mut retail =
                (0.05 + 0.34 * inner_ring + 0.25 * corridor + 0.24 * sub_center + 0.14 * cbd)
                    .max(0.01);
            let mut recreation =
                (0.04 + 0.24 * residential_belt + 0.16 * periphery + 0.08 * (1.0 - rr).max(0.0))
                    .max(0.01);
            let mut industrial =
                (0.04 + 0.30 * periphery + 0.18 * corridor + 0.10 * (1.0 - cbd)).max(0.01);
            let mut education =
                (0.04 + 0.16 * inner_ring + 0.10 * residential_belt + 0.05 * sub_center).max(0.01);
            let mut health =
                (0.03 + 0.14 * cbd + 0.12 * inner_ring + 0.08 * residential_belt).max(0.01);
            let mix_sum =
                residential + office + retail + recreation + industrial + education + health;
            residential /= mix_sum;
            office /= mix_sum;
            retail /= mix_sum;
            recreation /= mix_sum;
            industrial /= mix_sum;
            education /= mix_sum;
            health /= mix_sum;
            let centrality = clamp01(0.60 * cbd + 0.27 * sub_center + 0.13 * (corridor / 1.52));

            residents_weight_sum += residents_weight;
            jobs_weight_sum += jobs_weight;
            drafts.push(CellDraft {
                cell_id: format!("dc:{q}:{r}"),
                x: x + dx,
                y: y + dy,
                residents_weight,
                jobs_weight,
                residential,
                office,
                retail,
                recreation,
                industrial,
                education,
                health,
                centrality,
            });
        }
    }

    let mut zones = Vec::<Zone>::with_capacity(drafts.len());
    let mut demand_cells = Vec::<DemandCell>::with_capacity(drafts.len());
    for d in drafts {
        let residents_night =
            (residents_total * d.residents_weight / residents_weight_sum).max(50.0);
        let jobs_day = (jobs_total * d.jobs_weight / jobs_weight_sum).max(20.0);
        zones.push(Zone {
            id: format!("z:{}", d.cell_id),
            x: d.x,
            y: d.y,
            population: residents_night,
            jobs: jobs_day,
            country_iso2: country.clone(),
        });
        demand_cells.push(DemandCell {
            cell_id: d.cell_id,
            x: d.x,
            y: d.y,
            area_m2: (3.0 * sqrt3 / 2.0) * hex_size_m * hex_size_m,
            residents_night,
            jobs_day,
            activity_mix_residential: d.residential,
            activity_mix_office: d.office,
            activity_mix_retail: d.retail,
            activity_mix_recreation: d.recreation,
            activity_mix_industrial: d.industrial,
            activity_mix_education: d.education,
            activity_mix_health: d.health,
            centrality_score: d.centrality,
            data_quality_score: 0.72,
            country_iso2: country.clone(),
        });
    }
    (zones, demand_cells)
}

#[allow(dead_code)]
fn looks_like_legacy_lattice(scenario: &Scenario) -> bool {
    if scenario.world.demand_cells.len() != 81 {
        return false;
    }
    if !scenario
        .world
        .demand_cells
        .iter()
        .all(|c| c.cell_id.starts_with("df:"))
    {
        return false;
    }
    let mut xs = BTreeSet::<i64>::new();
    let mut ys = BTreeSet::<i64>::new();
    for c in &scenario.world.demand_cells {
        xs.insert((c.x / 100.0).round() as i64);
        ys.insert((c.y / 100.0).round() as i64);
    }
    xs.len() == 9 && ys.len() == 9
}

#[allow(dead_code)]
fn synthesize_country_demand(
    app: &AppHandle,
    country_iso2: &str,
    start_location: Option<&StartLocation>,
) -> Result<(Vec<Zone>, Vec<DemandCell>), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two letters".to_string());
    }

    let mut cities = list_cities_internal(app, &iso)?;
    if let Some(start) = start_location {
        if !cities.iter().any(|c| c.geonameid == start.city_id) {
            cities.push(CityOption {
                geonameid: start.city_id,
                name: start.city_name.clone(),
                lat: start.city_lat,
                lon: start.city_lon,
                population: start.city_population.unwrap_or(250_000),
            });
        }
    }
    if cities.is_empty() {
        return Err(format!("no city catalog rows for country {iso}"));
    }
    cities.sort_by(|a, b| {
        b.population
            .cmp(&a.population)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut major = cities
        .into_iter()
        .filter(|c| {
            c.population >= 20_000 || Some(c.geonameid) == start_location.map(|s| s.city_id)
        })
        .collect::<Vec<_>>();
    if major.is_empty() {
        return Err(format!("no usable city rows for country {iso}"));
    }
    major.truncate(180);
    let top_pop = major
        .first()
        .map(|c| c.population)
        .unwrap_or(500_000)
        .max(1) as f64;
    let golden = 2.399963229728653_f64;

    #[derive(Clone)]
    struct LocalDraft {
        x: f64,
        y: f64,
        residents_weight: f64,
        jobs_weight: f64,
        residential: f64,
        office: f64,
        retail: f64,
        recreation: f64,
        industrial: f64,
        education: f64,
        health: f64,
        centrality: f64,
    }

    let mut zones = Vec::<Zone>::new();
    let mut demand_cells = Vec::<DemandCell>::new();

    for (city_rank, city) in major.iter().enumerate() {
        let pop = city.population.max(30_000) as f64;
        let city_scale = (pop / 300_000.0).powf(0.36).clamp(0.55, 3.2);
        let city_weight = (pop / top_pop).powf(0.52).clamp(0.12, 1.0);
        let n_cells = ((20.0 + 30.0 * city_scale) * (0.72 + 0.28 * city_weight))
            .round()
            .clamp(18.0, 94.0) as usize;
        let radius_m = (11_000.0 + 26_000.0 * city_scale).clamp(10_000.0, 58_000.0);
        let phase = city.lon * 0.35 + city.lat * 0.24 + city.geonameid as f64 * 0.0000003;
        let city_residents_total = pop * (1.18 + 0.08 * city_weight);
        let employment_ratio = (0.34 + (pop.log10() - 5.0) * 0.07).clamp(0.26, 0.56);
        let city_jobs_total = city_residents_total * employment_ratio;
        let (cx, cy) = lonlat_to_web_mercator_m(city.lon, city.lat);

        let mut local = Vec::<LocalDraft>::with_capacity(n_cells);
        let mut residents_sum = 0.0;
        let mut jobs_sum = 0.0;
        for i in 0..n_cells {
            let t = (i as f64 + 0.5) / n_cells as f64;
            let spiral_r = radius_m * t.sqrt();
            let theta = i as f64 * golden + phase;
            let radial_jitter =
                (stable_noise_01(i as i32, city_rank as i32, phase) - 0.5) * radius_m * 0.08;
            let theta_jitter =
                (stable_noise_01(city.geonameid as i32, i as i32, phase + 1.13) - 0.5) * 0.28;
            let r = (spiral_r + radial_jitter).max(radius_m * 0.035);
            let dx = r * (theta + theta_jitter).cos();
            let dy = r * (theta + theta_jitter).sin();

            let u = (r / radius_m).clamp(0.0, 1.5);
            let cbd = (-4.6 * u * u).exp();
            let inner_ring = (-((u - 0.42).powi(2)) / 0.050).exp();
            let suburban = (-((u - 0.75).powi(2)) / 0.085).exp();
            let periphery = clamp01((u - 0.60) / 0.45);
            let corridor = (1.0
                + 0.30 * (2.0 * theta + phase).cos()
                + 0.17 * (3.0 * theta - 0.8 * phase).sin())
            .max(0.22);

            let residents_weight = (0.50 * suburban
                + 0.22 * periphery
                + 0.15 * inner_ring
                + 0.08 * corridor
                + 0.05 * (1.0 - cbd))
                .max(0.01);
            let jobs_weight =
                (0.57 * cbd + 0.20 * inner_ring + 0.15 * corridor + 0.08 * suburban).max(0.01);

            let mut residential =
                (0.50 + 0.46 * suburban + 0.14 * periphery - 0.30 * cbd).max(0.01);
            let mut office = (0.06 + 0.94 * cbd + 0.16 * corridor - 0.20 * suburban).max(0.01);
            let mut retail = (0.05 + 0.31 * inner_ring + 0.27 * corridor + 0.12 * cbd).max(0.01);
            let mut recreation = (0.04 + 0.24 * suburban + 0.16 * periphery).max(0.01);
            let mut industrial =
                (0.04 + 0.31 * periphery + 0.17 * corridor + 0.09 * (1.0 - cbd)).max(0.01);
            let mut education = (0.04 + 0.14 * inner_ring + 0.09 * suburban).max(0.01);
            let mut health = (0.03 + 0.12 * cbd + 0.10 * inner_ring + 0.07 * suburban).max(0.01);
            let mix_sum =
                residential + office + retail + recreation + industrial + education + health;
            residential /= mix_sum;
            office /= mix_sum;
            retail /= mix_sum;
            recreation /= mix_sum;
            industrial /= mix_sum;
            education /= mix_sum;
            health /= mix_sum;
            let centrality = clamp01(0.64 * cbd + 0.24 * (corridor / 1.47) + 0.12 * inner_ring);

            residents_sum += residents_weight;
            jobs_sum += jobs_weight;
            local.push(LocalDraft {
                x: cx + dx,
                y: cy + dy,
                residents_weight,
                jobs_weight,
                residential,
                office,
                retail,
                recreation,
                industrial,
                education,
                health,
                centrality,
            });
        }

        let area_m2 = (std::f64::consts::PI * radius_m * radius_m / n_cells as f64).max(40_000.0);
        let quality = (0.56 + 0.30 * city_weight).clamp(0.56, 0.9);
        for (i, d) in local.into_iter().enumerate() {
            let residents_night =
                (city_residents_total * d.residents_weight / residents_sum).max(30.0);
            let jobs_day = (city_jobs_total * d.jobs_weight / jobs_sum).max(12.0);
            let cell_id = format!("dc:{iso}:{}:{i}", city.geonameid);
            zones.push(Zone {
                id: format!("z:{cell_id}"),
                x: d.x,
                y: d.y,
                population: residents_night,
                jobs: jobs_day,
                country_iso2: Some(iso.clone()),
            });
            demand_cells.push(DemandCell {
                cell_id,
                x: d.x,
                y: d.y,
                area_m2,
                residents_night,
                jobs_day,
                activity_mix_residential: d.residential,
                activity_mix_office: d.office,
                activity_mix_retail: d.retail,
                activity_mix_recreation: d.recreation,
                activity_mix_industrial: d.industrial,
                activity_mix_education: d.education,
                activity_mix_health: d.health,
                centrality_score: d.centrality,
                data_quality_score: quality,
                country_iso2: Some(iso.clone()),
            });
        }
    }

    if demand_cells.is_empty() {
        return Err(format!("generated empty demand for country {iso}"));
    }
    Ok((zones, demand_cells))
}

#[allow(dead_code)]
fn ensure_country_demand_coverage(
    app: &AppHandle,
    manifest: &ProjectManifest,
    scenario: &mut Scenario,
) -> bool {
    let mut unlocked = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2)
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            unlocked.insert(code);
        }
    }

    let mut changed = false;
    for iso in unlocked {
        let same_country_cells = scenario
            .world
            .demand_cells
            .iter()
            .filter(|c| {
                c.country_iso2
                    .as_deref()
                    .map(|v| v.eq_ignore_ascii_case(&iso))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        let has_countrywide_ids = same_country_cells
            .iter()
            .any(|c| c.cell_id.starts_with(&format!("dc:{iso}:")));
        let all_bootstrap_style = !same_country_cells.is_empty()
            && same_country_cells
                .iter()
                .all(|c| c.cell_id.starts_with("dc:") || c.cell_id.starts_with("df:"));
        let needs_generation =
            same_country_cells.is_empty() || (all_bootstrap_style && !has_countrywide_ids);
        if !needs_generation {
            continue;
        }

        let start_for_country = manifest
            .start_location
            .as_ref()
            .filter(|s| s.country_iso2.eq_ignore_ascii_case(&iso));
        let generated = match synthesize_country_demand(app, &iso, start_for_country) {
            Ok(v) => v,
            Err(_) => continue,
        };

        scenario.world.demand_cells.retain(|c| {
            let same_country = c
                .country_iso2
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case(&iso))
                .unwrap_or(false);
            !(same_country && (c.cell_id.starts_with("dc:") || c.cell_id.starts_with("df:")))
        });
        scenario.world.zones.retain(|z| {
            let same_country = z
                .country_iso2
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case(&iso))
                .unwrap_or(false);
            !(same_country && (z.id.starts_with("z:dc:") || z.id.starts_with("z:df:")))
        });
        scenario.world.zones.extend(generated.0);
        scenario.world.demand_cells.extend(generated.1);
        changed = true;
    }
    changed
}

#[allow(dead_code)]
fn has_significant_variation(values: &[f64]) -> bool {
    if values.len() < 6 {
        return false;
    }
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        min_v = min_v.min(v);
        max_v = max_v.max(v);
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        return false;
    }
    (max_v - min_v) > (max_v.abs().max(1.0) * 0.08)
}

#[allow(dead_code)]
fn demand_variation_is_healthy(scenario: &Scenario) -> bool {
    if !scenario.world.demand_cells.is_empty() {
        let residents = scenario
            .world
            .demand_cells
            .iter()
            .map(|c| c.residents_night)
            .collect::<Vec<_>>();
        let jobs = scenario
            .world
            .demand_cells
            .iter()
            .map(|c| c.jobs_day)
            .collect::<Vec<_>>();
        return has_significant_variation(&residents) && has_significant_variation(&jobs);
    }
    let residents = scenario
        .world
        .zones
        .iter()
        .map(|z| z.population)
        .collect::<Vec<_>>();
    let jobs = scenario
        .world
        .zones
        .iter()
        .map(|z| z.jobs)
        .collect::<Vec<_>>();
    has_significant_variation(&residents) && has_significant_variation(&jobs)
}

fn default_scenario_template(
    name: &str,
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> Scenario {
    let (zones, demand_cells) =
        synthesize_city_demand(center_lon, center_lat, city_population, country_iso2);
    let country = country_iso2
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2);
    Scenario {
        meta: Meta {
            name: name.to_string(),
            seed: 42,
            time_period_hours: 1.0,
            crs: Crs::Epsg3857,
        },
        params: default_params(),
        world: World {
            zones,
            stops: vec![],
            links: vec![],
            services: vec![],
            transfers: vec![],
            transfer_rules: None,
            demand_cells,
            demand_meta: Some(DemandMeta {
                surface_version: "legacy-bootstrap".to_string(),
                loaded_countries: country.clone().into_iter().collect(),
                source: "legacy_synthetic".to_string(),
            }),
        },
    }
}

fn default_template_doc(project_name: &str) -> ScenarioDocument {
    ScenarioDocument::new_current(default_scenario_template(
        project_name,
        -1.5491,
        53.8008,
        None,
        None,
    ))
}

fn default_template_doc_at_location(
    project_name: &str,
    center_lon: f64,
    center_lat: f64,
    city_population: Option<u64>,
    country_iso2: Option<&str>,
) -> ScenarioDocument {
    ScenarioDocument::new_current(default_scenario_template(
        project_name,
        center_lon,
        center_lat,
        city_population,
        country_iso2,
    ))
}

fn ensure_game_bootstrap_network(
    _scenario: &mut Scenario,
    _center_lon: f64,
    _center_lat: f64,
    _city_population: Option<u64>,
    _country_iso2: Option<&str>,
) -> bool {
    false
}

fn open_session_internal(
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
    reset_runtime_tick(state, &project_path_string)?;

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


fn sync_progress_budget_from_economy(manifest: &mut ProjectManifest) {
    if let Some(metrics) = manifest.progress_metrics.as_mut() {
        let cfg = economy_config();
        let currency = normalize_currency(Some(&manifest.economy.currency));
        metrics.currency = currency.clone();
        metrics.budget = from_base_currency(manifest.economy.current_balance_base, &currency, &cfg);
    }
}

fn project_is_current(state: &tauri::State<AppState>, project_path: &str) -> Result<bool, String> {
    let current = state
        .current_project
        .lock()
        .map_err(|_| "current_project mutex poisoned".to_string())?;
    Ok(current
        .as_deref()
        .map(|value| value == project_path)
        .unwrap_or(false))
}

fn load_persisted_sandbox_state(project_root: &Path) -> Option<PersistedSandboxStateFile> {
    let path = sandbox_state_path(project_root);
    if !path.exists() {
        return None;
    }
    read_json_file::<PersistedSandboxStateFile>(&path).ok()
}

fn persisted_sim_state_from_sim_state(
    sim_state: &interlinked_engine::sim::SimState,
) -> PersistedSimState {
    let mut queue = sim_state
        .queue
        .iter()
        .filter_map(|((service_id, stop_id), value)| {
            if !value.is_finite() || *value <= 1e-9 {
                return None;
            }
            Some(PersistedServiceStopValue {
                service_id: service_id.clone(),
                stop_id: stop_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    queue.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.stop_id.cmp(&b.stop_id))
    });

    let mut queue_cohorts = sim_state
        .queue_cohorts
        .iter()
        .filter_map(
            |((service_id, board_stop_id, destination_stop_id), value)| {
                if !value.is_finite() || *value <= 1e-9 {
                    return None;
                }
                Some(PersistedServiceStopDestinationValue {
                    service_id: service_id.clone(),
                    board_stop_id: board_stop_id.clone(),
                    destination_stop_id: destination_stop_id.clone(),
                    value: *value,
                })
            },
        )
        .collect::<Vec<_>>();
    queue_cohorts.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.board_stop_id.cmp(&b.board_stop_id))
            .then_with(|| a.destination_stop_id.cmp(&b.destination_stop_id))
    });

    let mut time_to_next_departure_s = sim_state
        .time_to_next_departure_s
        .iter()
        .filter_map(|((service_id, stop_id), value)| {
            if !value.is_finite() || *value < 0.0 {
                return None;
            }
            Some(PersistedServiceStopValue {
                service_id: service_id.clone(),
                stop_id: stop_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    time_to_next_departure_s.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.stop_id.cmp(&b.stop_id))
    });

    let mut pending_od_trips = sim_state
        .pending_od_trips
        .iter()
        .filter_map(|((origin_zone_id, destination_zone_id), value)| {
            if !value.is_finite() || *value <= 1e-9 {
                return None;
            }
            Some(PersistedZonePairValue {
                origin_zone_id: origin_zone_id.clone(),
                destination_zone_id: destination_zone_id.clone(),
                value: *value,
            })
        })
        .collect::<Vec<_>>();
    pending_od_trips.sort_by(|a, b| {
        a.origin_zone_id
            .cmp(&b.origin_zone_id)
            .then_with(|| a.destination_zone_id.cmp(&b.destination_zone_id))
    });

    PersistedSimState {
        t_s: if sim_state.t_s.is_finite() {
            sim_state.t_s.max(0.0)
        } else {
            0.0
        },
        queue,
        queue_cohorts,
        time_to_next_departure_s,
        pending_od_trips,
    }
}

fn sim_state_from_persisted(
    scenario: &Scenario,
    persisted: &PersistedSimState,
    fallback_t_s: f64,
) -> interlinked_engine::sim::SimState {
    let mut restored = init_sim_state(scenario, &RunConfig::default());
    let persisted_t = if persisted.t_s.is_finite() && persisted.t_s >= 0.0 {
        persisted.t_s
    } else {
        fallback_t_s.max(0.0)
    };
    restored.t_s = persisted_t;
    restored.queue.clear();
    restored.queue_cohorts.clear();
    restored.pending_od_trips.clear();

    let valid_service_stop_keys = scenario
        .world
        .services
        .iter()
        .flat_map(|service| {
            service
                .stop_sequence
                .iter()
                .cloned()
                .map(move |stop_id| (service.id.clone(), stop_id))
        })
        .collect::<HashSet<_>>();
    let service_stop_set = scenario
        .world
        .services
        .iter()
        .map(|service| {
            (
                service.id.clone(),
                service
                    .stop_sequence
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let valid_zone_ids = scenario
        .world
        .zones
        .iter()
        .map(|zone| zone.id.clone())
        .collect::<HashSet<_>>();

    for entry in &persisted.queue {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        if !valid_service_stop_keys.contains(&(entry.service_id.clone(), entry.stop_id.clone())) {
            continue;
        }
        restored.queue.insert(
            (entry.service_id.clone(), entry.stop_id.clone()),
            entry.value,
        );
    }

    for entry in &persisted.queue_cohorts {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        let Some(stop_set) = service_stop_set.get(&entry.service_id) else {
            continue;
        };
        if !stop_set.contains(&entry.board_stop_id)
            || !stop_set.contains(&entry.destination_stop_id)
        {
            continue;
        }
        restored.queue_cohorts.insert(
            (
                entry.service_id.clone(),
                entry.board_stop_id.clone(),
                entry.destination_stop_id.clone(),
            ),
            entry.value,
        );
    }

    for entry in &persisted.time_to_next_departure_s {
        if !entry.value.is_finite() || entry.value < 0.0 {
            continue;
        }
        if !valid_service_stop_keys.contains(&(entry.service_id.clone(), entry.stop_id.clone())) {
            continue;
        }
        restored.time_to_next_departure_s.insert(
            (entry.service_id.clone(), entry.stop_id.clone()),
            entry.value,
        );
    }

    for entry in &persisted.pending_od_trips {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        if !valid_zone_ids.contains(&entry.origin_zone_id)
            || !valid_zone_ids.contains(&entry.destination_zone_id)
        {
            continue;
        }
        restored.pending_od_trips.insert(
            (
                entry.origin_zone_id.clone(),
                entry.destination_zone_id.clone(),
            ),
            entry.value,
        );
    }

    restored
}

fn persisted_runtime_ops_from_runtime_ops(ops: &RuntimeOpsState) -> PersistedRuntimeOpsState {
    let mut trains = ops.trains.values().cloned().collect::<Vec<_>>();
    trains.sort_by(|a, b| a.train_id.cmp(&b.train_id));
    let mut queue_cohorts = ops
        .queue_cohorts
        .iter()
        .filter_map(
            |((service_id, board_stop_id, destination_stop_id), value)| {
                if !value.is_finite() || *value <= 1e-9 {
                    return None;
                }
                Some(PersistedServiceStopDestinationValue {
                    service_id: service_id.clone(),
                    board_stop_id: board_stop_id.clone(),
                    destination_stop_id: destination_stop_id.clone(),
                    value: *value,
                })
            },
        )
        .collect::<Vec<_>>();
    queue_cohorts.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.board_stop_id.cmp(&b.board_stop_id))
            .then_with(|| a.destination_stop_id.cmp(&b.destination_stop_id))
    });
    PersistedRuntimeOpsState {
        topology_hash: ops.topology_hash,
        trains,
        queue_cohorts,
    }
}

fn runtime_ops_from_persisted(
    persisted: &PersistedRuntimeOpsState,
    project_path: &str,
) -> RuntimeOpsState {
    let mut trains = BTreeMap::<String, RuntimeTrainState>::new();
    for mut train in persisted.trains.clone() {
        let train_id = train.train_id.trim().to_string();
        if train_id.is_empty() {
            continue;
        }
        if !train.vehicle_capacity.is_finite() || train.vehicle_capacity < 0.0 {
            train.vehicle_capacity = 0.0;
        }
        if !train.progress.is_finite() || train.progress < 0.0 {
            train.progress = 0.0;
        }
        if !train.remaining_s.is_finite() || train.remaining_s < 0.0 {
            train.remaining_s = 0.0;
        }
        if train.direction_step == 0 {
            train.direction_step = 1;
        }
        train
            .onboard_cohorts
            .retain(|_, value| value.is_finite() && *value > 1e-6);
        train.onboard_pax = runtime_train_onboard_total(&train);
        train.train_id = train_id.clone();
        trains.insert(train_id, train);
    }
    let mut queue_cohorts = HashMap::<(String, String, String), f64>::new();
    for entry in &persisted.queue_cohorts {
        if !entry.value.is_finite() || entry.value <= 1e-9 {
            continue;
        }
        queue_cohorts.insert(
            (
                entry.service_id.clone(),
                entry.board_stop_id.clone(),
                entry.destination_stop_id.clone(),
            ),
            entry.value,
        );
    }
    RuntimeOpsState {
        project_path: project_path.to_string(),
        topology_hash: persisted.topology_hash,
        profiles_by_service: HashMap::new(),
        stop_name_by_id: HashMap::new(),
        reverse_service_by_service: HashMap::new(),
        stop_ids_by_service: HashMap::new(),
        fare_base_by_service: HashMap::new(),
        dispatch_service_ids: HashSet::new(),
        trains,
        queue_cohorts,
    }
}

fn capture_persisted_runtime_state(
    state: &tauri::State<AppState>,
    project_path: &str,
) -> Result<Option<PersistedRuntimeState>, String> {
    if !project_is_current(state, project_path)? {
        return Ok(None);
    }
    let game = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?
        .clone();
    let Some(game_state) = game else {
        return Ok(None);
    };
    let runtime_ops = state
        .runtime_ops
        .lock()
        .map_err(|_| "runtime_ops mutex poisoned".to_string())?
        .as_ref()
        .filter(|ops| ops.project_path == project_path)
        .map(persisted_runtime_ops_from_runtime_ops);
    let latest_snapshot = latest_runtime_snapshot_for_project(state.inner(), project_path)?;
    let tick_s = if game_state.tick_s.is_finite() && game_state.tick_s >= 0.0 {
        game_state.tick_s
    } else {
        0.0
    };
    Ok(Some(PersistedRuntimeState {
        tick_s,
        sim_state: Some(persisted_sim_state_from_sim_state(&game_state.sim_state)),
        run_cfg: Some(game_state.run_cfg.clone()),
        history: None,
        last_output: None,
        last_quick_kpis: None,
        runtime_ops,
        latest_snapshot,
    }))
}

fn apply_persisted_runtime_state_to_game(
    game_state: &mut interlinked_engine::platform::GameState,
    scenario: &Scenario,
    persisted: &PersistedRuntimeState,
) {
    game_state.kernel_state = interlinked_engine::platform::KernelPartitionState::default();
    if let Some(run_cfg) = persisted.run_cfg.as_ref() {
        game_state.run_cfg = run_cfg.clone();
    }
    let fallback_tick = if persisted.tick_s.is_finite() && persisted.tick_s >= 0.0 {
        persisted.tick_s
    } else {
        game_state.tick_s.max(0.0)
    };
    if let Some(sim_state) = persisted.sim_state.as_ref() {
        game_state.sim_state = sim_state_from_persisted(scenario, sim_state, fallback_tick);
    } else {
        game_state.sim_state.t_s = fallback_tick;
    }
    if game_state.sim_state.t_s.is_finite() && game_state.sim_state.t_s >= 0.0 {
        game_state.tick_s = game_state.sim_state.t_s;
    } else {
        game_state.tick_s = fallback_tick;
        game_state.sim_state.t_s = fallback_tick;
    }
    game_state.run_cfg.deterministic_seed = Some(scenario.meta.seed);
    if let Some(history) = persisted.history.as_ref() {
        game_state.history = history.clone();
    }
    game_state.last_quick_kpis = persisted.last_quick_kpis.clone();
    game_state.last_output = persisted.last_output.clone();
}

fn rehydrate_game_state_scenario(
    game_state: &mut interlinked_engine::platform::GameState,
    scenario: &Scenario,
) {
    let previous_tick_s = game_state.tick_s;
    let previous_state = game_state.sim_state.clone();
    let previous_run_cfg = game_state.run_cfg.clone();
    let valid_keys = scenario
        .world
        .services
        .iter()
        .flat_map(|service| {
            service
                .stop_sequence
                .iter()
                .cloned()
                .map(move |stop_id| (service.id.clone(), stop_id))
        })
        .collect::<HashSet<_>>();
    let doc = ScenarioDocument::new_current(scenario.clone());
    let mut next = SimulationService::init_game_state(&doc);
    next.tick_s = previous_tick_s;
    next.sim_state.t_s = previous_state.t_s;
    next.run_cfg = previous_run_cfg;
    next.run_cfg.deterministic_seed = Some(scenario.meta.seed);
    for (key, value) in previous_state.queue {
        if valid_keys.contains(&key) {
            next.sim_state.queue.insert(key, value);
        }
    }
    for (key, value) in previous_state.time_to_next_departure_s {
        if valid_keys.contains(&key) {
            next.sim_state.time_to_next_departure_s.insert(key, value);
        }
    }
    *game_state = next;
}

fn clock_minute_of_day(clock: &SimulationClock) -> u32 {
    let base_minute = clock
        .sim_datetime_utc
        .split('T')
        .nth(1)
        .and_then(|tail| tail.split('Z').next())
        .and_then(|time_part| {
            let mut parts = time_part.split(':');
            let hour = parts.next()?.parse::<u32>().ok()?;
            let minute = parts.next()?.parse::<u32>().ok()?;
            Some((hour % 24) * 60 + (minute % 60))
        })
        .unwrap_or(8 * 60);
    let delta_minutes = (clock.tick_seconds / 60.0).floor() as i64;
    ((base_minute as i64 + delta_minutes).rem_euclid(1440)) as u32
}

fn run_ephemeral_inspection_output(
    scenario: &Scenario,
    apply_game_runtime_overrides: bool,
) -> Result<SimulationOutput, String> {
    let doc = ScenarioDocument::new_current(scenario.clone());
    let mut clone = SimulationService::init_game_state(&doc);
    clone.run_cfg.lightweight_outputs = false;
    let mut materialized = clone.store.scenario().clone();
    if apply_game_runtime_overrides {
        strip_auto_reverse_runtime_artifacts(&mut materialized);
        apply_game_runtime_demand_tuning(&mut materialized.params);
        synthesize_auto_reverse_runtime_services(&mut materialized);
    }
    materialize_line_operations_for_minute(&mut materialized, &economy_config(), 8 * 60);
    clone.store = ScenarioStore::new(materialized);
    let _ = SimulationService::step_game(
        &mut clone,
        300.0,
        interlinked_engine::platform::GameStepRequest {
            recompute_quick_kpis: true,
            edits: Vec::new(),
            force_strategic_refresh: true,
        },
    )?;
    clone
        .last_output
        .ok_or_else(|| "inspection analysis did not produce simulation output".to_string())
}

fn inspection_output_for_project(
    state: &tauri::State<AppState>,
    project_path: &str,
    scenario: &Scenario,
) -> Result<SimulationOutput, String> {
    let apply_game_runtime_overrides = read_manifest(Path::new(project_path))
        .map(|manifest| manifest.session_kind == SessionKind::Game)
        .unwrap_or(false);
    if project_is_current(state, project_path)? {
        let guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_ref() {
            if let Some(output) = game_state.last_output.clone() {
                if !output.meta.results_version.ends_with("-lite") {
                    return Ok(output);
                }
            }
            let mut clone = game_state.clone();
            clone.run_cfg.lightweight_outputs = false;
            drop(guard);
            let mut materialized = clone.store.scenario().clone();
            if apply_game_runtime_overrides {
                strip_auto_reverse_runtime_artifacts(&mut materialized);
                apply_game_runtime_demand_tuning(&mut materialized.params);
                synthesize_auto_reverse_runtime_services(&mut materialized);
            }
            materialize_line_operations_for_minute(&mut materialized, &economy_config(), 8 * 60);
            clone.store = ScenarioStore::new(materialized);
            let _ = SimulationService::step_game(
                &mut clone,
                300.0,
                interlinked_engine::platform::GameStepRequest {
                    recompute_quick_kpis: true,
                    edits: Vec::new(),
                    force_strategic_refresh: true,
                },
            )?;
            return clone.last_output.ok_or_else(|| {
                "inspection analysis did not produce simulation output".to_string()
            });
        }
    }
    run_ephemeral_inspection_output(scenario, apply_game_runtime_overrides)
}

fn seed_unlocked_countries(manifest: &mut ProjectManifest) {
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            let mut set = manifest
                .economy
                .unlocked_countries
                .iter()
                .map(|x| x.trim().to_ascii_uppercase())
                .filter(|x| x.len() == 2)
                .collect::<BTreeSet<_>>();
            set.insert(code);
            manifest.economy.unlocked_countries = set.into_iter().collect();
        }
    }
}

fn unlocked_country_codes(manifest: &ProjectManifest) -> Vec<String> {
    let mut set = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            set.insert(code);
        }
    }
    set.into_iter().collect()
}

fn load_surface_wire(path: &Path) -> Result<DemandSurfaceCountryWire, String> {
    let mut surface: DemandSurfaceCountryWire = read_json_file(path)?;
    if surface.surface_version.eq_ignore_ascii_case("v3") {
        backfill_legacy_surface_mix(&mut surface);
        surface.surface_version = "v4".to_string();
    } else if !surface.surface_version.eq_ignore_ascii_case("v4") {
        return Err(format!(
            "unsupported demand surface version '{}' in {} (expected v4 or v3)",
            surface.surface_version,
            path.display()
        ));
    }

    let validate_cells = |cells: &mut [DemandSurfaceCellWire], layer: &str| -> Result<(), String> {
        for c in cells {
            let values = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(format!(
                    "invalid activity mix in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let sum: f64 = values.iter().sum();
            if sum <= 1e-9 {
                return Err(format!(
                    "activity mix sum must be > 0 in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let normalized = normalize_activity_mix(values);
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
        Ok(())
    };

    validate_cells(&mut surface.cells_res8, "res8")?;
    validate_cells(&mut surface.cells_res7, "res7")?;
    validate_cells(&mut surface.cells_res6, "res6")?;
    Ok(surface)
}

fn normalize_loaded_countries(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn simplify_county_geometry(geometry: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    geometry.simplify(&0.0018)
}

static GB_COUNTY_BOUNDARY_CACHE: OnceLock<Result<CountyBoundaryCatalog, String>> = OnceLock::new();
static GB_COUNTY_ALIAS_CACHE: OnceLock<Result<HashMap<String, String>, String>> = OnceLock::new();

fn repo_boundaries_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("boundaries")
}

fn repo_map_style_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("map_assets")
        .join("style")
}

fn normalized_iso_a2(value: Option<&str>) -> Option<String> {
    let iso = value?.trim().to_ascii_uppercase();
    if iso.len() != 2 || iso == "-99" || !iso.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(iso)
}

fn world_context_iso2_from_props(props: &JsonValue) -> Option<String> {
    normalized_iso_a2(props.get("ISO_A2").and_then(|v| v.as_str()))
        .or_else(|| normalized_iso_a2(props.get("ISO_A2_EH").and_then(|v| v.as_str())))
}

fn world_context_from_countries_geojson(value: JsonValue) -> Result<JsonValue, String> {
    let Some(features) = value.get("features").and_then(|v| v.as_array()) else {
        return Err("countries.geojson must contain a features array".to_string());
    };
    let remapped = features
        .iter()
        .filter_map(|feature| {
            let geometry = feature.get("geometry")?.clone();
            let props = feature.get("properties")?;
            let iso = world_context_iso2_from_props(props)?;
            let name = props
                .get("NAME_EN")
                .and_then(|v| v.as_str())
                .or_else(|| props.get("ADMIN").and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": {
                    "country_iso2": iso,
                    "name": name,
                    "playable_now": iso == "GB",
                    "coming_soon": iso != "GB"
                }
            }))
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "type": "FeatureCollection",
        "features": remapped
    }))
}

fn counties_bounds(counties: &[CountyBoundary]) -> Option<[[f64; 2]; 2]> {
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for county in counties {
        for polygon in &county.geometry.0 {
            for point in polygon.exterior().points() {
                min_lon = min_lon.min(point.x());
                min_lat = min_lat.min(point.y());
                max_lon = max_lon.max(point.x());
                max_lat = max_lat.max(point.y());
            }
        }
    }
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return None;
    }
    Some([[min_lon, min_lat], [max_lon, max_lat]])
}

fn point_eq(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

fn close_ring_coords(mut coords: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if coords.len() >= 2 && !point_eq(coords[0], *coords.last().unwrap_or(&(0.0, 0.0))) {
        coords.push(coords[0]);
    }
    coords
}

fn multipolygon_to_geojson_value(geometry: &MultiPolygon<f64>) -> JsonValue {
    let coordinates = geometry
        .0
        .iter()
        .map(|polygon| {
            let mut rings = Vec::<Vec<Vec<f64>>>::new();
            rings.push(
                polygon
                    .exterior()
                    .points()
                    .map(|point| vec![point.x(), point.y()])
                    .collect(),
            );
            for interior in polygon.interiors() {
                rings.push(
                    interior
                        .points()
                        .map(|point| vec![point.x(), point.y()])
                        .collect(),
                );
            }
            rings
        })
        .collect::<Vec<_>>();
    serde_json::to_value(GeoJsonGeometry::new(GeoJsonValue::MultiPolygon(
        coordinates,
    )))
    .unwrap_or(JsonValue::Null)
}

fn multipolygon_bbox_center(geometry: &MultiPolygon<f64>) -> Option<(f64, f64)> {
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for polygon in &geometry.0 {
        for point in polygon.exterior().points() {
            min_lon = min_lon.min(point.x());
            min_lat = min_lat.min(point.y());
            max_lon = max_lon.max(point.x());
            max_lat = max_lat.max(point.y());
        }
    }
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return None;
    }
    Some(((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5))
}

fn geojson_coords_to_polygon(coords: &[Vec<Vec<f64>>]) -> Option<Polygon<f64>> {
    let exterior = coords.first()?;
    let exterior_ring = close_ring_coords(
        exterior
            .iter()
            .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
            .collect(),
    );
    let exterior_line = LineString::from(exterior_ring);
    let interiors = coords
        .iter()
        .skip(1)
        .filter_map(|ring| {
            let pts = close_ring_coords(
                ring.iter()
                    .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
                    .collect(),
            );
            (pts.len() >= 4).then_some(LineString::from(pts))
        })
        .collect::<Vec<_>>();
    Some(Polygon::new(exterior_line, interiors))
}

fn parse_uk_counties_canonical_geojson(
    index: &[UkCountyIndexEntry],
) -> Result<Vec<CountyBoundary>, String> {
    let path = repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson");
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let GeoJson::FeatureCollection(fc) = raw.parse::<GeoJson>().map_err(|e| e.to_string())? else {
        return Err(
            "gb_ceremonial_counties_canonical.geojson must be a FeatureCollection".to_string(),
        );
    };
    if fc.features.is_empty() {
        return Ok(vec![]);
    }
    let mut by_id = index
        .iter()
        .cloned()
        .map(|entry| (entry.county_id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::<CountyBoundary>::new();
    for feature in fc.features {
        let Some(props) = feature.properties.as_ref() else {
            continue;
        };
        let Some(county_id) = props
            .get("county_id")
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
        else {
            continue;
        };
        let Some(entry) = by_id.remove(&county_id) else {
            continue;
        };
        let Some(geom) = feature.geometry else {
            continue;
        };
        let geometry = match &geom.value {
            GeoJsonValue::Polygon(coords) => {
                let Some(poly) = geojson_coords_to_polygon(coords) else {
                    continue;
                };
                MultiPolygon(vec![poly])
            }
            GeoJsonValue::MultiPolygon(multi) => {
                let polys = multi
                    .iter()
                    .filter_map(|coords| geojson_coords_to_polygon(coords))
                    .collect::<Vec<_>>();
                if polys.is_empty() {
                    continue;
                }
                MultiPolygon(polys)
            }
            _ => continue,
        };
        let geometry = simplify_county_geometry(&geometry);
        let Some((bbox_center_lon, bbox_center_lat)) = multipolygon_bbox_center(&geometry) else {
            continue;
        };
        out.push(CountyBoundary {
            county_id: entry.county_id,
            name: entry.name,
            nation: entry.nation,
            country_iso2: entry.country_iso2,
            source_code: entry.source_code,
            geometry_json: multipolygon_to_geojson_value(&geometry),
            geometry,
            bbox_center_lon,
            bbox_center_lat,
        });
    }
    Ok(out)
}

fn load_gb_county_boundaries() -> Result<CountyBoundaryCatalog, String> {
    let cached = GB_COUNTY_BOUNDARY_CACHE.get_or_init(|| {
        let index_path = repo_boundaries_root().join("gb_ceremonial_counties_index.json");
        let index_file: UkCountyIndexFile = read_json_file(&index_path)?;
        let counties = parse_uk_counties_canonical_geojson(&index_file.counties)?;
        if counties.is_empty() {
            return Err(format!(
                "GB county geometry missing in {}",
                repo_boundaries_root()
                    .join("gb_ceremonial_counties_canonical.geojson")
                    .display()
            ));
        }
        Ok(CountyBoundaryCatalog { counties })
    });
    cached.clone()
}

fn load_gb_county_aliases() -> Result<HashMap<String, String>, String> {
    let cached = GB_COUNTY_ALIAS_CACHE.get_or_init(|| {
        let alias_path = repo_boundaries_root().join("gb_ceremonial_county_aliases.json");
        if !alias_path.exists() {
            return Ok(HashMap::new());
        }
        let aliases: GbCountyAliasFile = read_json_file(&alias_path)?;
        Ok(aliases.aliases)
    });
    cached.clone()
}

fn normalize_region_id(value: &str) -> Option<String> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, token] => {
            let tier = tier.trim().to_ascii_lowercase();
            let iso = iso.trim().to_ascii_uppercase();
            let token = token.trim();
            if (tier != "r6" && tier != "r7" && tier != "county")
                || iso.len() != 2
                || token.is_empty()
            {
                return None;
            }
            let token = token.to_ascii_lowercase();
            Some(format!("{tier}:{iso}:{token}"))
        }
        _ => None,
    }
}

fn canonicalize_region_id(value: &str) -> Option<String> {
    let mut normalized = normalize_region_id(value)?;
    if !normalized.starts_with("county:GB:") {
        return Some(normalized);
    }
    if let Ok(aliases) = load_gb_county_aliases() {
        for _ in 0..8 {
            let Some(mapped) = aliases.get(&normalized) else {
                break;
            };
            if mapped == &normalized {
                break;
            }
            let Some(next) = normalize_region_id(mapped) else {
                break;
            };
            normalized = next;
        }
    }
    Some(normalized)
}

fn canonicalize_region_ledger(ledger: &mut BTreeMap<String, RegionEconomyLedger>) {
    let mut merged = BTreeMap::<String, RegionEconomyLedger>::new();
    let old = std::mem::take(ledger);
    for (key, value) in old {
        let canonical = canonicalize_region_id(&key).unwrap_or(key);
        let entry = merged.entry(canonical).or_default();
        entry.revenue_base += value.revenue_base;
        entry.opex_base += value.opex_base;
        entry.capex_base += value.capex_base;
        entry.penalties_base += value.penalties_base;
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    *ledger = merged;
}

fn region_country_iso2(region_id: &str) -> Option<String> {
    let parts = region_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, _token]
            if (tier.to_ascii_lowercase().starts_with('r')
                || tier.eq_ignore_ascii_case("county"))
                && iso.len() == 2 =>
        {
            Some(iso.to_ascii_uppercase())
        }
        _ => None,
    }
}

fn cell_id_is_legacy_generated(cell_id: &str) -> bool {
    cell_id.starts_with("df:") || cell_id.starts_with("dc:")
}

fn zone_id_is_legacy_generated(zone_id: &str) -> bool {
    zone_id.starts_with("z:df:") || zone_id.starts_with("z:dc:")
}

fn migrate_legacy_synthetic_demand(scenario: &mut Scenario) -> bool {
    let mut changed = false;
    let old_cells = scenario.world.demand_cells.len();
    scenario
        .world
        .demand_cells
        .retain(|c| !cell_id_is_legacy_generated(&c.cell_id));
    if scenario.world.demand_cells.len() != old_cells {
        changed = true;
    }

    let old_zones = scenario.world.zones.len();
    scenario.world.zones.retain(|z| {
        if zone_id_is_legacy_generated(&z.id) {
            return false;
        }
        if let Some(cell_id) = z.id.strip_prefix("z:") {
            return !cell_id_is_legacy_generated(cell_id);
        }
        true
    });
    if scenario.world.zones.len() != old_zones {
        changed = true;
    }

    let should_reset_meta = scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.source != "surface_v4_region_scope")
        .unwrap_or(true);
    if changed || should_reset_meta {
        scenario.world.demand_meta = Some(DemandMeta {
            surface_version: "v4".to_string(),
            loaded_countries: vec![],
            source: "surface_v4_region_scope".to_string(),
        });
        changed = true;
    }
    changed
}

fn is_surface_generated_cell_id(cell_id: &str) -> bool {
    cell_id.starts_with("ds:v3:") || cell_id.starts_with("ds:v4:") || cell_id.starts_with("ds:v4m:")
}

fn is_surface_generated_zone_id(zone_id: &str) -> bool {
    if zone_id.starts_with("ds:v3:")
        || zone_id.starts_with("ds:v4:")
        || zone_id.starts_with("ds:v4m:")
    {
        return true;
    }
    zone_id
        .strip_prefix("z:")
        .map(is_surface_generated_cell_id)
        .unwrap_or(false)
}

fn region_id_from_res6(iso: &str, res6_cell_id: &str) -> String {
    format!("r6:{}:{}", iso.trim().to_ascii_uppercase(), res6_cell_id)
}

fn region_id_from_county(iso: &str, county_id: &str) -> String {
    format!(
        "county:{}:{}",
        iso.trim().to_ascii_uppercase(),
        county_id.trim()
    )
}

fn preferred_home_county_id(start: &StartLocation) -> Option<&'static str> {
    let city = start.city_name.trim().to_ascii_lowercase();
    match city.as_str() {
        "london" | "city of london" => Some("greater-london"),
        "manchester" => Some("greater-manchester"),
        "leeds" => Some("west-yorkshire"),
        _ => None,
    }
}

fn county_for_lon_lat(counties: &[CountyBoundary], lon: f64, lat: f64) -> Option<&CountyBoundary> {
    let point = Point::new(lon, lat);
    counties
        .iter()
        .find(|county| county.geometry.contains(&point))
}

fn nearest_county_for_lon_lat(
    counties: &[CountyBoundary],
    lon: f64,
    lat: f64,
) -> Option<&CountyBoundary> {
    counties.iter().min_by(|a, b| {
        let da = (a.bbox_center_lon - lon).powi(2) + (a.bbox_center_lat - lat).powi(2);
        let db = (b.bbox_center_lon - lon).powi(2) + (b.bbox_center_lat - lat).powi(2);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn landuse_class_profile(class_name: &str) -> Option<([f64; 7], f64)> {
    let class_name = class_name.trim().to_ascii_lowercase();
    let (mix, intensity) = match class_name.as_str() {
        "residential" => ([0.78, 0.06, 0.05, 0.04, 0.01, 0.04, 0.02], 1.10),
        "commercial" => ([0.07, 0.56, 0.22, 0.03, 0.05, 0.04, 0.03], 2.55),
        "retail" => ([0.10, 0.22, 0.52, 0.08, 0.02, 0.03, 0.03], 2.30),
        "industrial" => ([0.03, 0.28, 0.10, 0.03, 0.48, 0.04, 0.04], 2.05),
        "park" | "meadow" | "grass" | "forest" | "natural" => {
            ([0.18, 0.02, 0.04, 0.70, 0.01, 0.03, 0.02], 0.45)
        }
        _ => return None,
    };
    Some((normalize_activity_mix(mix), intensity))
}

fn update_lonlat_bounds(point: &[JsonValue], bounds: &mut Option<(f64, f64, f64, f64)>) {
    if point.len() < 2 {
        return;
    }
    let Some(lon) = point[0].as_f64() else { return };
    let Some(lat) = point[1].as_f64() else { return };
    if !lon.is_finite() || !lat.is_finite() {
        return;
    }
    if let Some((min_lon, min_lat, max_lon, max_lat)) = bounds.as_mut() {
        *min_lon = (*min_lon).min(lon);
        *min_lat = (*min_lat).min(lat);
        *max_lon = (*max_lon).max(lon);
        *max_lat = (*max_lat).max(lat);
    } else {
        *bounds = Some((lon, lat, lon, lat));
    }
}

fn geometry_lonlat_bounds(geometry: &JsonValue) -> Option<(f64, f64, f64, f64)> {
    let gtype = geometry.get("type").and_then(|v| v.as_str())?;
    let coords = geometry.get("coordinates")?;
    let mut bounds = None::<(f64, f64, f64, f64)>;
    match gtype {
        "Polygon" => {
            let rings = coords.as_array()?;
            for ring in rings {
                let Some(points) = ring.as_array() else {
                    continue;
                };
                for point in points {
                    if let Some(pair) = point.as_array() {
                        update_lonlat_bounds(pair, &mut bounds);
                    }
                }
            }
        }
        "MultiPolygon" => {
            let polygons = coords.as_array()?;
            for polygon in polygons {
                let Some(rings) = polygon.as_array() else {
                    continue;
                };
                for ring in rings {
                    let Some(points) = ring.as_array() else {
                        continue;
                    };
                    for point in points {
                        if let Some(pair) = point.as_array() {
                            update_lonlat_bounds(pair, &mut bounds);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    bounds
}

fn parse_county_landuse_profile(path: &Path) -> Result<CountyLanduseProfile, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value =
        serde_json::from_str::<JsonValue>(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let features = value
        .get("features")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("{} is missing feature collection data", path.display()))?;
    let mut samples = Vec::<LanduseSample>::new();
    for feature in features {
        let Some(props) = feature.get("properties").and_then(|v| v.as_object()) else {
            continue;
        };
        let layer = props
            .get("feature_layer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if layer != "landuse" {
            continue;
        }
        let Some(class_name) = props.get("landuse_class").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((mix, intensity)) = landuse_class_profile(class_name) else {
            continue;
        };
        let Some(geometry) = feature.get("geometry") else {
            continue;
        };
        let Some((min_lon, min_lat, max_lon, max_lat)) = geometry_lonlat_bounds(geometry) else {
            continue;
        };
        let center_lon = (min_lon + max_lon) * 0.5;
        let center_lat = (min_lat + max_lat) * 0.5;
        if !center_lon.is_finite() || !center_lat.is_finite() {
            continue;
        }
        let (min_x, min_y) = lonlat_to_web_mercator_m(min_lon, min_lat);
        let (max_x, max_y) = lonlat_to_web_mercator_m(max_lon, max_lat);
        let bbox_area_m2 = ((max_x - min_x).abs() * (max_y - min_y).abs()).max(1.0);
        let size_weight = (bbox_area_m2.sqrt() / 120.0).clamp(0.35, 4.25);
        let (x_m, y_m) = lonlat_to_web_mercator_m(center_lon, center_lat);
        samples.push(LanduseSample {
            x_m,
            y_m,
            weight: size_weight,
            intensity,
            mix,
        });
    }
    Ok(CountyLanduseProfile { samples })
}

fn county_landuse_file(app: &AppHandle, country_iso2: &str, county_id: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let county = county_id.trim();
    if county.is_empty() {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    for dir_name in ["county_basemap_full", "county_basemap_mid"] {
        let path = pack_dir
            .join("map")
            .join(dir_name)
            .join(format!("{county}.geojson"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn load_county_landuse_profile(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Result<CountyLanduseProfile, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let county = county_id.trim().to_ascii_lowercase();
    if iso != "GB" || county.is_empty() {
        return Ok(CountyLanduseProfile::default());
    }
    let cache_key = format!("{iso}:{county}");
    {
        let cache = gb_county_landuse_cache()
            .lock()
            .map_err(|_| "county landuse cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    let profile = if let Some(path) = county_landuse_file(app, &iso, &county) {
        parse_county_landuse_profile(&path).unwrap_or_default()
    } else {
        CountyLanduseProfile::default()
    };
    gb_county_landuse_cache()
        .lock()
        .map_err(|_| "county landuse cache poisoned".to_string())?
        .insert(cache_key, profile.clone());
    Ok(profile)
}

fn estimate_legacy_demand_scale(scenario: &Scenario) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for cell in &scenario.world.demand_cells {
        let mass = cell.residents_night.max(0.0) + cell.jobs_day.max(0.0);
        if mass <= 0.0 || !mass.is_finite() {
            continue;
        }
        total += mass;
        count += 1;
    }
    if count == 0 {
        return 1.0;
    }
    let avg = total / count as f64;
    if !avg.is_finite() || avg <= 0.0 {
        return 1.0;
    }
    (120.0 / avg).clamp(1.0, 18.0).sqrt()
}

fn enrich_station_inspection_with_landuse(
    app: &AppHandle,
    scenario: &Scenario,
    inspection: &mut StationInspection,
) -> Result<(), String> {
    let (stop_mx, stop_my) =
        world_xy_to_web_mercator_m(&scenario.meta.crs, inspection.x, inspection.y);
    let (stop_lon, stop_lat) = web_mercator_m_to_lonlat(stop_mx, stop_my);
    if !stop_lon.is_finite() || !stop_lat.is_finite() {
        return Ok(());
    }
    let county_catalog = load_gb_county_boundaries()?;
    let Some(county) = county_for_lon_lat(&county_catalog.counties, stop_lon, stop_lat)
        .or_else(|| nearest_county_for_lon_lat(&county_catalog.counties, stop_lon, stop_lat))
    else {
        return Ok(());
    };
    let landuse = load_county_landuse_profile(app, "GB", &county.county_id)?;
    if landuse.samples.is_empty() {
        return Ok(());
    }

    let radius_m = inspection.catchment_radius_m.clamp(500.0, 2200.0);
    let sigma = (radius_m * 0.55).max(240.0);
    let radius2 = radius_m * radius_m;

    let mut selected = Vec::<(&LanduseSample, f64, f64)>::new();
    for sample in &landuse.samples {
        let dx = sample.x_m - stop_mx;
        let dy = sample.y_m - stop_my;
        let d2 = dx * dx + dy * dy;
        if d2 > radius2 {
            continue;
        }
        let gaussian = (-d2 / (2.0 * sigma * sigma)).exp();
        if gaussian <= 0.0 {
            continue;
        }
        let weight = gaussian * sample.weight * sample.intensity.max(0.15);
        if weight <= 0.0 {
            continue;
        }
        selected.push((sample, d2.sqrt(), weight));
    }
    if selected.is_empty() {
        let mut nearest = landuse
            .samples
            .iter()
            .map(|sample| {
                let dx = sample.x_m - stop_mx;
                let dy = sample.y_m - stop_my;
                let dist = (dx * dx + dy * dy).sqrt();
                (sample, dist)
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let scale = radius_m
            .max(nearest.first().map(|entry| entry.1).unwrap_or(1.0))
            .max(1.0);
        for (sample, dist) in nearest.into_iter().take(48) {
            let rel = dist / scale;
            let weight = (1.0 / (1.0 + rel * rel)) * sample.weight * sample.intensity.max(0.15);
            if weight <= 0.0 {
                continue;
            }
            selected.push((sample, dist, weight));
        }
    }
    if selected.is_empty() {
        return Ok(());
    }

    let mut mix_sum = [0.0_f64; 7];
    let mut weight_sum = 0.0_f64;
    let mut intensity_weighted = 0.0_f64;
    let mut max_dist = 0.0_f64;
    for (sample, dist, weight) in &selected {
        weight_sum += *weight;
        intensity_weighted += sample.intensity.max(0.0) * *weight;
        max_dist = max_dist.max(*dist);
        for (idx, value) in sample.mix.iter().enumerate() {
            mix_sum[idx] += value.max(0.0) * *weight;
        }
    }
    if weight_sum <= 0.0 {
        return Ok(());
    }
    let mut micro_mix = [0.0_f64; 7];
    for idx in 0..7 {
        micro_mix[idx] = mix_sum[idx] / weight_sum;
    }
    micro_mix = normalize_activity_mix(micro_mix);

    let mut base_mix = normalize_activity_mix([
        inspection.catchment_mix_residential,
        inspection.catchment_mix_office,
        inspection.catchment_mix_retail,
        inspection.catchment_mix_recreation,
        inspection.catchment_mix_industrial,
        inspection.catchment_mix_education,
        inspection.catchment_mix_health,
    ]);
    let blend = (weight_sum / (weight_sum + 22.0)).clamp(0.35, 0.9);
    for idx in 0..7 {
        base_mix[idx] = base_mix[idx] * (1.0 - blend) + micro_mix[idx] * blend;
    }
    base_mix = normalize_activity_mix(base_mix);

    let intensity_avg = (intensity_weighted / weight_sum).clamp(0.25, 3.4);
    let legacy_scale = estimate_legacy_demand_scale(scenario);
    let urban_scale = (0.55 + intensity_avg).clamp(0.65, 3.4);
    let total_scale = (legacy_scale * urban_scale).clamp(1.0, 12.0);
    let base_total = (inspection.catchment_residents + inspection.catchment_jobs).max(0.0);
    let total_estimate = if base_total > 0.0 {
        base_total * total_scale
    } else {
        weight_sum * 24.0
    };
    let jobs_pull = base_mix[1] * 1.0
        + base_mix[2] * 0.9
        + base_mix[4] * 0.95
        + base_mix[5] * 0.65
        + base_mix[6] * 0.6
        + base_mix[3] * 0.35
        + base_mix[0] * 0.15;
    let residents_pull = base_mix[0] * 1.05
        + base_mix[3] * 0.28
        + base_mix[2] * 0.2
        + base_mix[1] * 0.08
        + base_mix[4] * 0.05
        + base_mix[5] * 0.05
        + base_mix[6] * 0.05;
    let pull_sum = (jobs_pull + residents_pull).max(1e-6);
    let jobs_share = (jobs_pull / pull_sum).clamp(0.08, 0.92);
    let residents_share = 1.0 - jobs_share;
    let micro_jobs = (total_estimate * jobs_share).max(0.0);
    let micro_residents = (total_estimate * residents_share).max(0.0);

    inspection.catchment_mix_residential = base_mix[0];
    inspection.catchment_mix_office = base_mix[1];
    inspection.catchment_mix_retail = base_mix[2];
    inspection.catchment_mix_recreation = base_mix[3];
    inspection.catchment_mix_industrial = base_mix[4];
    inspection.catchment_mix_education = base_mix[5];
    inspection.catchment_mix_health = base_mix[6];
    inspection.catchment_residents =
        (inspection.catchment_residents * (1.0 - blend) + micro_residents * blend).max(0.0);
    inspection.catchment_jobs =
        (inspection.catchment_jobs * (1.0 - blend) + micro_jobs * blend).max(0.0);
    inspection.catchment_cells = inspection.catchment_cells.max(selected.len());
    inspection.catchment_radius_m = inspection.catchment_radius_m.max(max_dist).max(radius_m);
    Ok(())
}

fn gb_county_adjacency_map(counties: &[CountyBoundary]) -> HashMap<String, Vec<String>> {
    let mut adjacency = counties
        .iter()
        .map(|county| {
            (
                region_id_from_county("GB", &county.county_id),
                Vec::<String>::new(),
            )
        })
        .collect::<HashMap<_, _>>();
    for i in 0..counties.len() {
        for j in (i + 1)..counties.len() {
            let a = &counties[i];
            let b = &counties[j];
            if !a.geometry.intersects(&b.geometry) {
                continue;
            }
            let a_id = region_id_from_county("GB", &a.county_id);
            let b_id = region_id_from_county("GB", &b.county_id);
            adjacency
                .entry(a_id.clone())
                .or_default()
                .push(b_id.clone());
            adjacency.entry(b_id).or_default().push(a_id);
        }
    }
    for values in adjacency.values_mut() {
        values.sort();
        values.dedup();
    }
    adjacency
}

fn nearest_region_ids_by_xy(
    regions: &[SurfaceRegionInfo],
    x: f64,
    y: f64,
    limit: usize,
    exclude_region_id: Option<&str>,
) -> Vec<String> {
    let mut nearest = regions
        .iter()
        .filter(|r| {
            exclude_region_id
                .map(|id| id != r.region_id)
                .unwrap_or(true)
        })
        .map(|r| {
            let d2 = (r.x - x).powi(2) + (r.y - y).powi(2);
            (d2, r.region_id.clone())
        })
        .collect::<Vec<_>>();
    nearest.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    nearest.into_iter().take(limit).map(|(_, id)| id).collect()
}

fn build_surface_region_catalog(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> SurfaceRegionCatalog {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let mut regions = surface
        .cells_res6
        .iter()
        .map(|c| SurfaceRegionInfo {
            region_id: region_id_from_res6(&iso, &c.cell_id),
            country_iso2: iso.clone(),
            name: format!("{} {}", iso, &c.cell_id),
            admin_level: "h3_r6_proxy".to_string(),
            nation: None,
            source_code: None,
            cell_id: c.cell_id.clone(),
            x: c.x,
            y: c.y,
            area_m2: c.area_m2.max(0.0),
            residents_smooth: c.residents_smooth.max(0.0),
            jobs_smooth: c.jobs_smooth.max(0.0),
            activity_mix_residential: c.activity_mix_residential,
            activity_mix_office: c.activity_mix_office,
            activity_mix_retail: c.activity_mix_retail,
            activity_mix_recreation: c.activity_mix_recreation,
            activity_mix_industrial: c.activity_mix_industrial,
            activity_mix_education: c.activity_mix_education,
            activity_mix_health: c.activity_mix_health,
            adjacent_region_ids: vec![],
            geometry: None,
        })
        .collect::<Vec<_>>();

    let mut region_id_by_h3_res6 = HashMap::<CellIndex, String>::new();
    for region in &regions {
        if let Ok(cell) = region.cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                region_id_by_h3_res6.insert(cell, region.region_id.clone());
            }
        }
    }

    for i in 0..regions.len() {
        let region_id = regions[i].region_id.clone();
        let region_cell_id = regions[i].cell_id.clone();
        let region_x = regions[i].x;
        let region_y = regions[i].y;
        let mut adjacent_region_ids = Vec::<String>::new();

        if let Ok(cell) = region_cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                for neighbor in cell.grid_disk::<Vec<_>>(1) {
                    if neighbor == cell {
                        continue;
                    }
                    if let Some(neighbor_region_id) = region_id_by_h3_res6.get(&neighbor) {
                        if neighbor_region_id != &region_id
                            && !adjacent_region_ids.contains(neighbor_region_id)
                        {
                            adjacent_region_ids.push(neighbor_region_id.clone());
                        }
                    }
                }
            }
        }

        if adjacent_region_ids.is_empty() {
            adjacent_region_ids =
                nearest_region_ids_by_xy(&regions, region_x, region_y, 6, Some(region_id.as_str()));
        }
        regions[i].adjacent_region_ids = adjacent_region_ids;
    }

    for region in &mut regions {
        let normalized = normalize_activity_mix([
            region.activity_mix_residential,
            region.activity_mix_office,
            region.activity_mix_retail,
            region.activity_mix_recreation,
            region.activity_mix_industrial,
            region.activity_mix_education,
            region.activity_mix_health,
        ]);
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        let mut region_id = cell
            .cell_id
            .parse::<CellIndex>()
            .ok()
            .and_then(|idx| idx.parent(Resolution::Six))
            .and_then(|parent| region_id_by_h3_res6.get(&parent).cloned());
        if region_id.is_none() {
            region_id = nearest_region_ids_by_xy(&regions, cell.x, cell.y, 1, None)
                .into_iter()
                .next();
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    let by_id = regions
        .iter()
        .map(|r| (r.region_id.clone(), r.clone()))
        .collect::<HashMap<_, _>>();

    SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
    }
}

fn merge_surface_region_catalog_aliases(catalog: SurfaceRegionCatalog) -> SurfaceRegionCatalog {
    if catalog.regions.is_empty() {
        return catalog;
    }

    let mut canonical_by_region = HashMap::<String, String>::new();
    for region in &catalog.regions {
        let canonical =
            canonicalize_region_id(&region.region_id).unwrap_or_else(|| region.region_id.clone());
        canonical_by_region.insert(region.region_id.clone(), canonical);
    }

    let canonical_for = |region_id: &str, lookup: &HashMap<String, String>| {
        lookup
            .get(region_id)
            .cloned()
            .or_else(|| canonicalize_region_id(region_id))
            .unwrap_or_else(|| region_id.to_string())
    };

    let mut grouped = HashMap::<String, Vec<SurfaceRegionInfo>>::new();
    for mut region in catalog.regions {
        let canonical = canonical_for(&region.region_id, &canonical_by_region);
        region.adjacent_region_ids = region
            .adjacent_region_ids
            .iter()
            .map(|neighbor| canonical_for(neighbor, &canonical_by_region))
            .filter(|neighbor| neighbor != &canonical)
            .collect();
        region.region_id = canonical.clone();
        grouped.entry(canonical).or_default().push(region);
    }

    let mut merged_regions = Vec::<SurfaceRegionInfo>::new();
    for (canonical_region_id, mut group) in grouped {
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| {
            let a_score = a.area_m2.max(a.residents_smooth + a.jobs_smooth);
            let b_score = b.area_m2.max(b.residents_smooth + b.jobs_smooth);
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged = group[0].clone();
        merged.region_id = canonical_region_id.clone();
        let mut adjacency = merged.adjacent_region_ids.clone();

        let mut weighted_total = 0.0_f64;
        let mut weighted_x = 0.0_f64;
        let mut weighted_y = 0.0_f64;
        let mut mix_sums = [0.0_f64; 7];
        let mut total_residents = 0.0_f64;
        let mut total_jobs = 0.0_f64;
        let mut total_area = 0.0_f64;

        for region in group {
            let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
            weighted_total += weight;
            weighted_x += region.x * weight;
            weighted_y += region.y * weight;
            total_residents += region.residents_smooth.max(0.0);
            total_jobs += region.jobs_smooth.max(0.0);
            total_area += region.area_m2.max(0.0);
            mix_sums[0] += region.activity_mix_residential.max(0.0) * weight;
            mix_sums[1] += region.activity_mix_office.max(0.0) * weight;
            mix_sums[2] += region.activity_mix_retail.max(0.0) * weight;
            mix_sums[3] += region.activity_mix_recreation.max(0.0) * weight;
            mix_sums[4] += region.activity_mix_industrial.max(0.0) * weight;
            mix_sums[5] += region.activity_mix_education.max(0.0) * weight;
            mix_sums[6] += region.activity_mix_health.max(0.0) * weight;
            adjacency.extend(region.adjacent_region_ids.clone());
        }

        if weighted_total > 0.0 {
            merged.x = weighted_x / weighted_total;
            merged.y = weighted_y / weighted_total;
        }
        merged.residents_smooth = total_residents;
        merged.jobs_smooth = total_jobs;
        merged.area_m2 = total_area;
        let normalized_mix = normalize_activity_mix([
            mix_sums[0] / weighted_total.max(1e-9),
            mix_sums[1] / weighted_total.max(1e-9),
            mix_sums[2] / weighted_total.max(1e-9),
            mix_sums[3] / weighted_total.max(1e-9),
            mix_sums[4] / weighted_total.max(1e-9),
            mix_sums[5] / weighted_total.max(1e-9),
            mix_sums[6] / weighted_total.max(1e-9),
        ]);
        merged.activity_mix_residential = normalized_mix[0];
        merged.activity_mix_office = normalized_mix[1];
        merged.activity_mix_retail = normalized_mix[2];
        merged.activity_mix_recreation = normalized_mix[3];
        merged.activity_mix_industrial = normalized_mix[4];
        merged.activity_mix_education = normalized_mix[5];
        merged.activity_mix_health = normalized_mix[6];
        merged.adjacent_region_ids = adjacency;
        merged_regions.push(merged);
    }

    let valid_region_ids = merged_regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut merged_regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid) && rid != &region.region_id);
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }

    let mut merged_cells = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for (region_id, cells) in catalog.cells_res8_by_region {
        let canonical = canonical_for(&region_id, &canonical_by_region);
        merged_cells.entry(canonical).or_default().extend(cells);
    }

    merged_regions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    let by_id = merged_regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    SurfaceRegionCatalog {
        regions: merged_regions,
        by_id,
        cells_res8_by_region: merged_cells,
    }
}

fn build_region_catalog_for_surface(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso == "GB" {
        return build_gb_county_region_catalog(surface).map(merge_surface_region_catalog_aliases);
    }
    Ok(merge_surface_region_catalog_aliases(
        build_surface_region_catalog(&iso, surface),
    ))
}

fn build_gb_county_region_catalog(
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let county_catalog = load_gb_county_boundaries()?;
    let counties = county_catalog.counties;
    if counties.is_empty() {
        return Err("no GB counties available".to_string());
    }

    let mut regions = counties
        .iter()
        .map(|county| SurfaceRegionInfo {
            region_id: region_id_from_county("GB", &county.county_id),
            country_iso2: county.country_iso2.clone(),
            name: county.name.clone(),
            admin_level: "uk_county".to_string(),
            nation: Some(county.nation.clone()),
            source_code: Some(county.source_code.clone()),
            cell_id: county.county_id.clone(),
            x: 0.0,
            y: 0.0,
            area_m2: 0.0,
            residents_smooth: 0.0,
            jobs_smooth: 0.0,
            activity_mix_residential: 0.0,
            activity_mix_office: 0.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            adjacent_region_ids: vec![],
            geometry: Some(county.geometry_json.clone()),
        })
        .collect::<Vec<_>>();
    let county_index = counties
        .iter()
        .enumerate()
        .map(|(idx, county)| (county.county_id.clone(), idx))
        .collect::<HashMap<_, _>>();
    let adjacency_map = gb_county_adjacency_map(&counties);
    let mut res6_owner = HashMap::<String, usize>::new();

    for cell in &surface.cells_res6 {
        let county = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat));
        let Some(county) = county else { continue };
        let Some(&idx) = county_index.get(&county.county_id) else {
            continue;
        };
        res6_owner.insert(cell.cell_id.clone(), idx);
        let weight = (cell.residents_smooth + cell.jobs_smooth).max(1.0);
        let region = &mut regions[idx];
        region.area_m2 += cell.area_m2.max(0.0);
        region.residents_smooth += cell.residents_smooth.max(0.0);
        region.jobs_smooth += cell.jobs_smooth.max(0.0);
        region.x += cell.x * weight;
        region.y += cell.y * weight;
    }

    for region in &mut regions {
        let total_weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
        if total_weight > 0.0 {
            region.x /= total_weight;
            region.y /= total_weight;
        } else if let Some(county) = county_index
            .get(&region.cell_id)
            .and_then(|idx| counties.get(*idx))
        {
            let (x, y) = lonlat_to_web_mercator_m(county.bbox_center_lon, county.bbox_center_lat);
            region.x = x;
            region.y = y;
        }
    }

    for region in &mut regions {
        region.adjacent_region_ids = adjacency_map
            .get(&region.region_id)
            .cloned()
            .unwrap_or_default();
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        // Assign res8 cells by actual county geometry first.
        // Parent-res6 ownership can smear small counties and blur city-center detail.
        let mut region_id = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat))
            .and_then(|county| county_index.get(&county.county_id).copied())
            .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        if region_id.is_none() {
            region_id = cell
                .cell_id
                .parse::<CellIndex>()
                .ok()
                .and_then(|idx| idx.parent(Resolution::Six))
                .and_then(|parent| res6_owner.get(&parent.to_string()).copied())
                .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    for region in &mut regions {
        let normalized = if let Some(cells) = cells_res8_by_region.get(&region.region_id) {
            let mut w_sum = 0.0_f64;
            let mut r_sum = 0.0_f64;
            let mut o_sum = 0.0_f64;
            let mut rt_sum = 0.0_f64;
            let mut rc_sum = 0.0_f64;
            let mut i_sum = 0.0_f64;
            let mut e_sum = 0.0_f64;
            let mut h_sum = 0.0_f64;
            for c in cells {
                let w = (c.residents_smooth + c.jobs_smooth).max(1e-6);
                w_sum += w;
                r_sum += c.activity_mix_residential.max(0.0) * w;
                o_sum += c.activity_mix_office.max(0.0) * w;
                rt_sum += c.activity_mix_retail.max(0.0) * w;
                rc_sum += c.activity_mix_recreation.max(0.0) * w;
                i_sum += c.activity_mix_industrial.max(0.0) * w;
                e_sum += c.activity_mix_education.max(0.0) * w;
                h_sum += c.activity_mix_health.max(0.0) * w;
            }
            let denom = w_sum.max(1e-9);
            normalize_activity_mix([
                r_sum / denom,
                o_sum / denom,
                rt_sum / denom,
                rc_sum / denom,
                i_sum / denom,
                e_sum / denom,
                h_sum / denom,
            ])
        } else {
            normalize_activity_mix([
                region.activity_mix_residential,
                region.activity_mix_office,
                region.activity_mix_retail,
                region.activity_mix_recreation,
                region.activity_mix_industrial,
                region.activity_mix_education,
                region.activity_mix_health,
            ])
        };
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let valid_region_ids = regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid));
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }
    let by_id = regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    Ok(SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
    })
}

fn nearest_region_for_start(
    catalog: &SurfaceRegionCatalog,
    start: Option<&StartLocation>,
    country_iso2: &str,
) -> Option<String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let Some(s) = start.filter(|x| x.country_iso2.eq_ignore_ascii_case(&iso)) else {
        return catalog
            .regions
            .iter()
            .max_by(|a, b| {
                (a.residents_smooth + a.jobs_smooth)
                    .partial_cmp(&(b.residents_smooth + b.jobs_smooth))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.region_id.clone());
    };
    if iso == "GB" {
        if let Some(county_id) = preferred_home_county_id(s) {
            let region_id = region_id_from_county(&iso, county_id);
            if catalog.by_id.contains_key(&region_id) {
                return Some(region_id);
            }
        }
    }
    let (sx, sy) = lonlat_to_web_mercator_m(s.city_lon, s.city_lat);
    catalog
        .regions
        .iter()
        .min_by(|a, b| {
            let da = (a.x - sx).powi(2) + (a.y - sy).powi(2);
            let db = (b.x - sx).powi(2) + (b.y - sy).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|r| r.region_id.clone())
}

fn default_active_regions_for_focus(
    catalog: &SurfaceRegionCatalog,
    focus_region_id: &str,
    unlocked: &HashSet<String>,
) -> Vec<String> {
    let mut out = vec![focus_region_id.to_string()];
    let Some(focus) = catalog.by_id.get(focus_region_id) else {
        return out;
    };
    for rid in &focus.adjacent_region_ids {
        if unlocked.contains(rid) && !out.contains(rid) {
            out.push(rid.clone());
        }
        if out.len() >= 4 {
            break;
        }
    }
    out
}

fn region_unlock_cost_base(region: &SurfaceRegionInfo) -> f64 {
    let scale = (region.residents_smooth + region.jobs_smooth)
        .max(1.0)
        .sqrt();
    20_000_000.0 + scale * 22_000.0
}

fn region_unlock_cost_base_for_manifest(
    manifest: &ProjectManifest,
    region: &SurfaceRegionInfo,
) -> f64 {
    let profile = resolved_difficulty_profile(manifest);
    region_unlock_cost_base(region) * profile.unlock_cost_mult.max(0.0)
}

fn country_employment_baseline_ratio(country_iso2: &str) -> f64 {
    match country_iso2.trim().to_ascii_uppercase().as_str() {
        "GB" => UK_EMPLOYMENT_BASELINE_RATIO,
        _ => DEFAULT_EMPLOYMENT_BASELINE_RATIO,
    }
}

fn region_employment_raw_score(region: &SurfaceRegionInfo) -> f64 {
    let residents = region.residents_smooth.max(0.0);
    if residents <= 0.0 {
        return 0.0;
    }
    let weighted_mix = 0.32 * region.activity_mix_residential.max(0.0)
        + 1.42 * region.activity_mix_office.max(0.0)
        + 0.96 * region.activity_mix_retail.max(0.0)
        + 0.66 * region.activity_mix_recreation.max(0.0)
        + 1.24 * region.activity_mix_industrial.max(0.0)
        + 0.98 * region.activity_mix_education.max(0.0)
        + 1.04 * region.activity_mix_health.max(0.0);
    residents * weighted_mix.max(0.06)
}

fn sync_country_region_state(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let valid = catalog
        .regions
        .iter()
        .map(|r| r.region_id.clone())
        .collect::<HashSet<_>>();
    let mut unlocked_for_country = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .filter(|id| valid.contains(id))
        .collect::<BTreeSet<_>>();

    if unlocked_for_country.is_empty() {
        if let Some(seed_region) =
            nearest_region_for_start(catalog, manifest.start_location.as_ref(), &iso)
        {
            unlocked_for_country.insert(seed_region);
        }
    }
    if unlocked_for_country.is_empty() {
        return Err(format!("no regions available for country {iso}"));
    }

    let mut primary = manifest
        .region_state
        .primary_focus_region_id
        .as_deref()
        .and_then(canonicalize_region_id)
        .filter(|rid| unlocked_for_country.contains(rid));
    if primary.is_none() {
        primary = unlocked_for_country.iter().next().cloned();
    }
    let primary = primary.ok_or_else(|| format!("failed to select primary region for {iso}"))?;

    let unlocked_set = unlocked_for_country.iter().cloned().collect::<HashSet<_>>();
    let mut active_for_country = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| unlocked_set.contains(id))
        .collect::<Vec<_>>();
    if active_for_country.is_empty() || !active_for_country.contains(&primary) {
        active_for_country = default_active_regions_for_focus(catalog, &primary, &unlocked_set);
    }
    active_for_country.truncate(8);

    let mut merged_unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() != Some(iso.as_str()))
        .collect::<BTreeSet<_>>();
    for rid in &unlocked_for_country {
        merged_unlocked.insert(rid.clone());
    }
    manifest.region_state.unlocked_region_ids = merged_unlocked.into_iter().collect();

    let mut merged_active = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() != Some(iso.as_str()))
        .collect::<BTreeSet<_>>();
    for rid in &active_for_country {
        merged_active.insert(rid.clone());
    }
    manifest.region_state.active_region_ids = merged_active.into_iter().collect();
    manifest.region_state.primary_focus_region_id = Some(primary);

    Ok((
        unlocked_for_country.into_iter().collect::<Vec<_>>(),
        active_for_country,
    ))
}

fn materialize_country_surface_scoped(
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let catalog = build_region_catalog_for_surface(&iso, surface)?;
    let (unlocked_regions, active_regions) = sync_country_region_state(manifest, &catalog, &iso)?;

    let max_active_zones = manifest.simulation_scope.max_active_zones.clamp(
        manifest.simulation_scope.remote_max_active_zones.max(20),
        manifest
            .simulation_scope
            .focus_max_active_zones
            .clamp(120, 6000),
    );
    let active_count = active_regions.len().max(1);
    let per_region_cap = (max_active_zones / active_count).clamp(40, 400);
    let active_set = active_regions.into_iter().collect::<HashSet<_>>();

    let mut loaded_cells = 0usize;
    for region_id in &unlocked_regions {
        if active_set.contains(region_id) {
            let mut cells = catalog
                .cells_res8_by_region
                .get(region_id)
                .cloned()
                .unwrap_or_default();
            cells.sort_by(|a, b| {
                (b.residents_smooth + b.jobs_smooth)
                    .partial_cmp(&(a.residents_smooth + a.jobs_smooth))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            cells.truncate(per_region_cap);
            for c in cells {
                let residents = c.residents_smooth.max(0.0);
                let jobs = c.jobs_smooth.max(0.0);
                let (wx, wy) = web_mercator_m_to_world_xy(&scenario.meta.crs, c.x, c.y);
                let [residential, office, retail, recreation, industrial, education, health] =
                    normalize_activity_mix([
                        c.activity_mix_residential,
                        c.activity_mix_office,
                        c.activity_mix_retail,
                        c.activity_mix_recreation,
                        c.activity_mix_industrial,
                        c.activity_mix_education,
                        c.activity_mix_health,
                    ]);
                let cid = format!("ds:v4:{}:{}", iso, c.cell_id);
                scenario.world.demand_cells.push(DemandCell {
                    cell_id: cid.clone(),
                    x: wx,
                    y: wy,
                    area_m2: c.area_m2.max(0.0),
                    residents_night: residents,
                    jobs_day: jobs,
                    activity_mix_residential: residential,
                    activity_mix_office: office,
                    activity_mix_retail: retail,
                    activity_mix_recreation: recreation,
                    activity_mix_industrial: industrial,
                    activity_mix_education: education,
                    activity_mix_health: health,
                    centrality_score: c.quality.clamp(0.0, 1.0),
                    data_quality_score: c.quality.clamp(0.0, 1.0),
                    country_iso2: Some(iso.clone()),
                });
                scenario.world.zones.push(Zone {
                    id: cid,
                    x: wx,
                    y: wy,
                    population: residents,
                    jobs,
                    country_iso2: Some(iso.clone()),
                });
                loaded_cells += 1;
            }
            continue;
        }

        if let Some(region) = catalog.by_id.get(region_id) {
            let residents = region.residents_smooth.max(0.0);
            let jobs = region.jobs_smooth.max(0.0);
            let (wx, wy) = web_mercator_m_to_world_xy(&scenario.meta.crs, region.x, region.y);
            let [residential, office, retail, recreation, industrial, education, health] =
                normalize_activity_mix([
                    region.activity_mix_residential,
                    region.activity_mix_office,
                    region.activity_mix_retail,
                    region.activity_mix_recreation,
                    region.activity_mix_industrial,
                    region.activity_mix_education,
                    region.activity_mix_health,
                ]);
            let cid = format!("ds:v4m:{}:{}", iso, region.cell_id);
            scenario.world.demand_cells.push(DemandCell {
                cell_id: cid.clone(),
                x: wx,
                y: wy,
                area_m2: region.area_m2.max(0.0),
                residents_night: residents,
                jobs_day: jobs,
                activity_mix_residential: residential,
                activity_mix_office: office,
                activity_mix_retail: retail,
                activity_mix_recreation: recreation,
                activity_mix_industrial: industrial,
                activity_mix_education: education,
                activity_mix_health: health,
                centrality_score: 0.45,
                data_quality_score: 0.65,
                country_iso2: Some(iso.clone()),
            });
            scenario.world.zones.push(Zone {
                id: cid,
                x: wx,
                y: wy,
                population: residents,
                jobs,
                country_iso2: Some(iso.clone()),
            });
            loaded_cells += 1;
        }
    }
    Ok(loaded_cells)
}

fn upsert_pack_ref(manifest: &mut ProjectManifest, iso: &str, surface: &DemandSurfaceCountryWire) {
    let iso = iso.trim().to_ascii_uppercase();
    manifest.pack_refs.retain(|p| p.country_iso2 != iso);
    manifest.pack_refs.push(CountryPackRef {
        country_iso2: iso,
        surface_version: Some(surface.surface_version.clone()),
        checksum: None,
    });
    manifest
        .pack_refs
        .sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
}

fn rematerialize_unlocked_country_surfaces(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
) -> Result<Vec<DemandCoverageResult>, String> {
    scenario
        .world
        .demand_cells
        .retain(|c| !is_surface_generated_cell_id(&c.cell_id));
    scenario
        .world
        .zones
        .retain(|z| !is_surface_generated_zone_id(&z.id));

    let mut out = Vec::<DemandCoverageResult>::new();
    let mut loaded_countries = Vec::<String>::new();
    let mut surface_version = None::<String>;
    for iso in unlocked_country_codes(manifest) {
        let Some(path) = demand_surface_file(app, &iso) else {
            out.push(DemandCoverageResult {
                country_iso2: iso,
                installed: false,
                loaded: false,
                cells_loaded: 0,
                message: "Demand data not installed for country".to_string(),
            });
            continue;
        };
        let surface = load_surface_wire(&path)?;
        let loaded_count = materialize_country_surface_scoped(manifest, scenario, &iso, &surface)?;
        loaded_countries.push(iso.clone());
        surface_version = Some(surface.surface_version.clone());
        upsert_pack_ref(manifest, &iso, &surface);
        out.push(DemandCoverageResult {
            country_iso2: iso,
            installed: true,
            loaded: true,
            cells_loaded: loaded_count,
            message: format!(
                "Loaded scoped region demand from {}",
                path.to_string_lossy()
            ),
        });
    }

    loaded_countries = normalize_loaded_countries(loaded_countries);
    scenario.world.demand_meta = Some(DemandMeta {
        surface_version: surface_version.unwrap_or_else(|| "v4".to_string()),
        loaded_countries: loaded_countries.clone(),
        source: "surface_v4_region_scope".to_string(),
    });
    let mut ds = manifest
        .demand_surface
        .clone()
        .unwrap_or_else(default_demand_surface_manifest);
    if let Some(version) = scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.surface_version.clone())
    {
        ds.surface_version = version;
    }
    ds.loaded_countries = loaded_countries;
    ds.last_rebuild_at = Some(now_string());
    manifest.demand_surface = Some(ds);
    Ok(out)
}

fn ensure_country_surface_loaded(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
) -> Result<DemandCoverageResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two letters".to_string());
    }
    let mut countries = unlocked_country_codes(manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    let all = rematerialize_unlocked_country_surfaces(app, manifest, scenario)?;
    Ok(all
        .into_iter()
        .find(|c| c.country_iso2.eq_ignore_ascii_case(&iso))
        .unwrap_or(DemandCoverageResult {
            country_iso2: iso,
            installed: false,
            loaded: false,
            cells_loaded: 0,
            message: "Demand data not installed for country".to_string(),
        }))
}

fn ensure_unlocked_country_surfaces_loaded(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
) -> Result<Vec<DemandCoverageResult>, String> {
    rematerialize_unlocked_country_surfaces(app, manifest, scenario)
}

fn load_region_catalog_for_country(
    app: &AppHandle,
    country_iso2: &str,
) -> Result<Option<SurfaceRegionCatalog>, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Ok(None);
    }
    let Some(path) = demand_surface_file(app, &iso) else {
        return Ok(None);
    };
    let surface = load_surface_wire(&path)?;
    Ok(Some(build_region_catalog_for_surface(&iso, &surface)?))
}

fn region_status_rows_for_manifest(
    app: &AppHandle,
    manifest: &ProjectManifest,
) -> Result<Vec<RegionStatus>, String> {
    let unlocked_regions = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let active_regions = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();

    let mut rows = Vec::<RegionStatus>::new();
    for iso in unlocked_country_codes(manifest) {
        let Some(catalog) = load_region_catalog_for_country(app, &iso)? else {
            continue;
        };
        let residents_total = catalog
            .regions
            .iter()
            .map(|region| region.residents_smooth.max(0.0))
            .sum::<f64>();
        let raw_employment_total = catalog
            .regions
            .iter()
            .map(region_employment_raw_score)
            .sum::<f64>();
        let target_employment_total =
            residents_total * country_employment_baseline_ratio(iso.as_str());
        let employment_scale = if raw_employment_total > 0.0 {
            target_employment_total / raw_employment_total
        } else {
            0.0
        };
        for region in catalog.regions {
            let cells_res8 = catalog
                .cells_res8_by_region
                .get(&region.region_id)
                .map(|v| v.len())
                .unwrap_or(0);
            let employment_estimate =
                (region_employment_raw_score(&region) * employment_scale).max(0.0);
            rows.push(RegionStatus {
                region_id: region.region_id.clone(),
                country_iso2: region.country_iso2.clone(),
                name: region.name.clone(),
                admin_level: region.admin_level.clone(),
                nation: region.nation.clone(),
                source_code: region.source_code.clone(),
                unlocked: unlocked_regions.contains(&region.region_id),
                active: active_regions.contains(&region.region_id),
                adjacent_region_ids: region.adjacent_region_ids.clone(),
                unlock_cost_base: region_unlock_cost_base_for_manifest(manifest, &region),
                residents_smooth: region.residents_smooth,
                jobs_smooth: region.jobs_smooth,
                employment_estimate,
                cells_res8,
                geometry: if region.country_iso2.eq_ignore_ascii_case("GB")
                    && counties_file(app, "GB").is_some()
                {
                    None
                } else {
                    region.geometry.clone()
                },
            });
        }
    }
    rows.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    Ok(rows)
}

fn apply_country_entry_charges(manifest: &mut ProjectManifest, scenario: &Scenario) -> f64 {
    let cfg = economy_config();
    let unlocked = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>();
    let scenario_countries = countries_in_scenario(scenario);
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

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
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

fn bootstrap_legacy_projects(app: &AppHandle) -> Result<(), String> {
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

fn repo_location_catalog_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("location_catalog")
}

fn repo_demand_surfaces_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("demand_surfaces")
}

fn repo_country_packs_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("country_packs")
}

fn managed_country_pack_dir(app: &AppHandle, country_iso2: &str) -> Result<PathBuf, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    Ok(country_packs_root(app)?.join(&iso))
}

fn repo_country_pack_dir(country_iso2: &str) -> PathBuf {
    repo_country_packs_root().join(country_iso2.trim().to_ascii_uppercase())
}

fn country_pack_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let managed = managed_country_pack_dir(app, country_iso2).ok()?;
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_country_pack_dir(country_iso2);
    if repo.exists() {
        return Some(repo);
    }
    None
}

fn rollout_supported_countries() -> BTreeSet<String> {
    ["GB"]
        .iter()
        .map(|x| x.to_string())
        .collect::<BTreeSet<_>>()
}

fn location_catalog_file(app: &AppHandle, relative: &Path) -> Option<PathBuf> {
    let managed = location_catalog_root(app).ok()?.join(relative);
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_location_catalog_root().join(relative);
    if repo.exists() {
        return Some(repo);
    }
    None
}

fn demand_surface_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return None;
    }
    let managed_pack_surface = managed_country_pack_dir(app, &iso)
        .ok()?
        .join("surfaces")
        .join(format!("{iso}.surface.json"));
    if managed_pack_surface.exists() {
        return Some(managed_pack_surface);
    }
    let repo_pack_surface = repo_country_pack_dir(&iso)
        .join("surfaces")
        .join(format!("{iso}.surface.json"));
    if repo_pack_surface.exists() {
        return Some(repo_pack_surface);
    }
    let managed = demand_surfaces_root(app)
        .ok()?
        .join(format!("{iso}.surface.json"));
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_demand_surfaces_root().join(format!("{iso}.surface.json"));
    if repo.exists() {
        return Some(repo);
    }
    None
}

fn managed_demand_surface_file(app: &AppHandle, country_iso2: &str) -> Result<PathBuf, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    Ok(demand_surfaces_root(app)?.join(format!("{iso}.surface.json")))
}

fn world_context_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let canonical = repo_boundaries_root().join("countries.geojson");
    if canonical.exists() {
        return Some(canonical);
    }
    let repo = repo_boundaries_root().join("world_context.geojson");
    if repo.exists() {
        return Some(repo);
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir.join("map").join("world_context.geojson");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    None
}

fn counties_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir.join("map").join("counties.geojson");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    let repo = repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson");
    repo.exists().then_some(repo)
}

fn basemap_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir.join("map").join("gb_basemap.mbtiles");
    path.exists().then_some(path)
}

fn style_template_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir
            .join("map")
            .join("style")
            .join("interlinked-light.json");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    let repo = repo_map_style_root().join("interlinked-light.json");
    repo.exists().then_some(repo)
}

fn major_roads_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir.join("map").join("gb_major_roads.geojson");
    path.exists().then_some(path)
}

fn county_roads_file(app: &AppHandle, country_iso2: &str, county_id: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir
        .join("map")
        .join("county_roads")
        .join(format!("{}.geojson", county_id.trim()));
    path.exists().then_some(path)
}

fn county_basemap_mid_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let fallback = pack_dir
        .join("map")
        .join("county_basemap_mid")
        .join(format!("{}.geojson", county_id.trim()));
    if fallback.exists() {
        return Some(fallback);
    }
    county_roads_file(app, &iso, county_id)
}

fn county_basemap_full_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let fallback = pack_dir
        .join("map")
        .join("county_basemap_full")
        .join(format!("{}.geojson", county_id.trim()));
    if fallback.exists() {
        return Some(fallback);
    }
    county_roads_file(app, &iso, county_id)
}

fn county_basemap_mid_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let dir = pack_dir.join("map").join("county_basemap_mid");
    dir.exists().then_some(dir)
}

fn county_basemap_full_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let dir = pack_dir.join("map").join("county_basemap_full");
    dir.exists().then_some(dir)
}

fn directory_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total = total.saturating_add(directory_size_bytes(&path));
        } else if let Ok(metadata) = entry.metadata() {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}


fn upsert_country_pack_entry(
    app: &AppHandle,
    country_iso2: &str,
    build_state: &str,
    surface_version: Option<String>,
    cells_count: usize,
    provenance: Option<String>,
) -> Result<(), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let mut idx = read_country_pack_index(app)?;
    idx.version = 1;
    idx.packs.retain(|p| p.country_iso2 != iso);
    idx.packs.push(CountryPackEntry {
        country_iso2: iso,
        build_state: build_state.to_string(),
        surface_version,
        cells_count,
        last_updated_at: Some(now_string()),
        checksum: None,
        provenance,
    });
    idx.packs
        .sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    write_country_pack_index(app, &idx)
}

fn country_pack_status_for(
    app: &AppHandle,
    index: &CountryPackIndex,
    supported_rollout: &BTreeSet<String>,
    country_iso2: &str,
) -> CountryPackStatus {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let idx_entry = index.packs.iter().find(|p| p.country_iso2 == iso);
    let surface_path = demand_surface_file(app, &iso);
    let mut surface_version = idx_entry.and_then(|p| p.surface_version.clone());
    let mut cells_count = idx_entry.map(|p| p.cells_count).unwrap_or(0);
    let mut last_updated_at = idx_entry.and_then(|p| p.last_updated_at.clone());
    let mut build_state = idx_entry
        .map(|p| p.build_state.clone())
        .unwrap_or_else(|| "missing".to_string());
    if let Some(path) = surface_path.as_ref() {
        if let Ok(surface) = load_surface_wire(path) {
            surface_version = Some(surface.surface_version.clone());
            cells_count = surface.cells_res8.len();
        }
        if build_state != "building" {
            build_state = "installed".to_string();
        }
        if last_updated_at.is_none() {
            last_updated_at = Some(now_string());
        }
    } else if build_state == "installed" {
        build_state = "missing".to_string();
    }

    let vector_map_ready = world_context_file(app, &iso).is_some()
        && counties_file(app, &iso).is_some()
        && basemap_file(app, &iso).is_some()
        && style_template_file(app, &iso).is_some();
    let legacy_map_ready = world_context_file(app, &iso).is_some()
        && (county_basemap_mid_dir(app, &iso).is_some()
            || county_basemap_full_dir(app, &iso).is_some()
            || country_pack_dir(app, &iso)
                .map(|dir| dir.join("map").join("county_roads").exists())
                .unwrap_or(false));
    let map_installed = vector_map_ready
        || legacy_map_ready
        || (world_context_file(app, &iso).is_some() && major_roads_file(app, &iso).is_some());
    let map_ready = vector_map_ready || legacy_map_ready;
    let map_size_bytes = country_pack_dir(app, &iso)
        .map(|dir| dir.join("map"))
        .filter(|dir| dir.exists())
        .map(|dir| directory_size_bytes(&dir));
    let demand_installed = surface_path.is_some();
    let fully_playable = demand_installed && map_ready;
    let map_pack_version = Some(
        if vector_map_ready {
            "vector-mbtiles-v1"
        } else if county_basemap_full_dir(app, &iso).is_some()
            || county_basemap_mid_dir(app, &iso).is_some()
        {
            "geojson-basemap-v2"
        } else if map_ready {
            "geojson-roads-v1"
        } else {
            "missing"
        }
        .to_string(),
    );
    let supported = supported_rollout.contains(&iso);
    let (eligible, reason) = if fully_playable && supported {
        (true, None)
    } else if !supported {
        (false, Some("Coming Soon".to_string()))
    } else if !map_ready {
        (false, Some("Map Pack Required".to_string()))
    } else if !demand_installed {
        (false, Some("Demand Pack Required".to_string()))
    } else {
        (false, Some("Install Required".to_string()))
    };

    CountryPackStatus {
        country_iso2: iso,
        build_state,
        surface_version,
        cells_count,
        last_updated_at,
        map_installed,
        map_ready,
        map_pack_version,
        map_size_bytes,
        demand_installed,
        fully_playable,
        eligible,
        reason,
    }
}

fn fallback_countries() -> Vec<CountryOption> {
    let mut rows = vec![
        ("AU", "Australia"),
        ("BE", "Belgium"),
        ("BR", "Brazil"),
        ("CA", "Canada"),
        ("CH", "Switzerland"),
        ("CN", "China"),
        ("DE", "Germany"),
        ("DK", "Denmark"),
        ("ES", "Spain"),
        ("FI", "Finland"),
        ("FR", "France"),
        ("GB", "Great Britain"),
        ("IE", "Ireland"),
        ("IN", "India"),
        ("IT", "Italy"),
        ("JP", "Japan"),
        ("KR", "South Korea"),
        ("MX", "Mexico"),
        ("NL", "Netherlands"),
        ("NO", "Norway"),
        ("NZ", "New Zealand"),
        ("PL", "Poland"),
        ("PT", "Portugal"),
        ("SE", "Sweden"),
        ("SG", "Singapore"),
        ("TR", "Turkey"),
        ("US", "United States"),
    ]
    .into_iter()
    .map(|(iso2, name)| CountryOption {
        iso2: iso2.to_string(),
        name: name.to_string(),
    })
    .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

fn fallback_cities(country_iso2: &str) -> Vec<CityOption> {
    let rows = match country_iso2 {
        "AU" => vec![
            (2147714, "Sydney", -33.8679, 151.2073, 5_230_000),
            (2172517, "Canberra", -35.2835, 149.1281, 381_000),
        ],
        "BE" => vec![
            (2800866, "Brussels", 50.8505, 4.3488, 1_200_000),
            (2797656, "Antwerp", 51.2199, 4.4035, 530_000),
        ],
        "BR" => vec![
            (3469058, "Brasilia", -15.7939, -47.8828, 3_100_000),
            (3448439, "Sao Paulo", -23.5505, -46.6333, 12_330_000),
        ],
        "CA" => vec![
            (6094817, "Ottawa", 45.4215, -75.6972, 1_070_000),
            (6077243, "Montreal", 45.5019, -73.5674, 1_780_000),
        ],
        "CH" => vec![
            (2661552, "Bern", 46.9481, 7.4474, 133_000),
            (2657896, "Zurich", 47.3769, 8.5417, 420_000),
        ],
        "CN" => vec![
            (1816670, "Beijing", 39.9042, 116.4074, 21_890_000),
            (1796236, "Shanghai", 31.2304, 121.4737, 24_870_000),
        ],
        "DE" => vec![
            (2950159, "Berlin", 52.52, 13.405, 3_850_000),
            (2867714, "Munich", 48.1374, 11.5755, 1_490_000),
        ],
        "DK" => vec![
            (2618425, "Copenhagen", 55.6761, 12.5683, 808_000),
            (2624652, "Aarhus", 56.1629, 10.2039, 285_000),
        ],
        "ES" => vec![
            (3117735, "Madrid", 40.4168, -3.7038, 3_280_000),
            (3128760, "Barcelona", 41.3851, 2.1734, 1_620_000),
        ],
        "FI" => vec![
            (658225, "Helsinki", 60.1699, 24.9384, 658_000),
            (634964, "Tampere", 61.4981, 23.761, 255_000),
        ],
        "FR" => vec![
            (2988507, "Paris", 48.8566, 2.3522, 2_160_000),
            (2996944, "Lyon", 45.764, 4.8357, 530_000),
        ],
        "GB" => vec![
            (2643743, "London", 51.5072, -0.1276, 9_000_000),
            (2643123, "Manchester", 53.4808, -2.2426, 570_000),
            (2644688, "Leeds", 53.8008, -1.5491, 536_000),
        ],
        "IE" => vec![
            (2964574, "Dublin", 53.3498, -6.2603, 593_000),
            (2965140, "Cork", 51.8985, -8.4756, 224_000),
        ],
        "IN" => vec![
            (1273294, "Delhi", 28.6139, 77.209, 16_787_000),
            (1275339, "Mumbai", 19.076, 72.8777, 12_442_000),
        ],
        "IT" => vec![
            (3169070, "Rome", 41.9028, 12.4964, 2_870_000),
            (3173435, "Milan", 45.4642, 9.19, 1_396_000),
        ],
        "JP" => vec![
            (1850147, "Tokyo", 35.6762, 139.6503, 13_960_000),
            (1853909, "Osaka", 34.6937, 135.5023, 2_750_000),
        ],
        "KR" => vec![
            (1835848, "Seoul", 37.5665, 126.978, 9_410_000),
            (1838519, "Busan", 35.1796, 129.0756, 3_350_000),
        ],
        "MX" => vec![
            (3530597, "Mexico City", 19.4326, -99.1332, 9_200_000),
            (3521081, "Guadalajara", 20.6597, -103.3496, 1_385_000),
        ],
        "NL" => vec![
            (2759794, "Amsterdam", 52.3676, 4.9041, 935_000),
            (2747891, "Rotterdam", 51.9244, 4.4777, 670_000),
        ],
        "NO" => vec![
            (3143244, "Oslo", 59.9139, 10.7522, 710_000),
            (3161732, "Bergen", 60.3913, 5.3221, 288_000),
        ],
        "NZ" => vec![
            (2179537, "Wellington", -41.2866, 174.7756, 216_000),
            (2193734, "Auckland", -36.8485, 174.7633, 1_530_000),
        ],
        "PL" => vec![
            (756135, "Warsaw", 52.2297, 21.0122, 1_860_000),
            (3094802, "Krakow", 50.0647, 19.945, 804_000),
        ],
        "PT" => vec![
            (2267057, "Lisbon", 38.7223, -9.1393, 545_000),
            (2735943, "Porto", 41.1579, -8.6291, 237_000),
        ],
        "SE" => vec![
            (2673730, "Stockholm", 59.3293, 18.0686, 984_000),
            (2711537, "Gothenburg", 57.7089, 11.9746, 603_000),
        ],
        "SG" => vec![(1880252, "Singapore", 1.2897, 103.8501, 5_918_000)],
        "TR" => vec![
            (745044, "Ankara", 39.9334, 32.8597, 5_750_000),
            (745042, "Istanbul", 41.0082, 28.9784, 15_700_000),
        ],
        "US" => vec![
            (4140963, "Washington", 38.8951, -77.0364, 702_000),
            (5128581, "New York", 40.7128, -74.006, 8_804_000),
            (5368361, "Los Angeles", 34.0522, -118.2437, 3_900_000),
        ],
        _ => vec![],
    };
    let capital_id = rows.first().map(|(geonameid, _, _, _, _)| *geonameid);
    let mut out = rows
        .into_iter()
        .map(|(geonameid, name, lat, lon, population)| CityOption {
            geonameid,
            name: name.to_string(),
            lat,
            lon,
            population,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| {
        let a_cap = capital_id == Some(a.geonameid);
        let b_cap = capital_id == Some(b.geonameid);
        b_cap
            .cmp(&a_cap)
            .then_with(|| b.population.cmp(&a.population))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn constrain_gb_start_cities(cities: Vec<CityOption>) -> Vec<CityOption> {
    let allowed_ids = BTreeMap::from([
        (2643743_i64, 0_usize), // London
        (2643123_i64, 1_usize), // Manchester
        (2644688_i64, 2_usize), // Leeds
    ]);
    let mut filtered = cities
        .into_iter()
        .filter(|city| allowed_ids.contains_key(&city.geonameid))
        .collect::<Vec<_>>();
    filtered.sort_by(|a, b| {
        let a_rank = allowed_ids.get(&a.geonameid).copied().unwrap_or(usize::MAX);
        let b_rank = allowed_ids.get(&b.geonameid).copied().unwrap_or(usize::MAX);
        a_rank
            .cmp(&b_rank)
            .then_with(|| b.population.cmp(&a.population))
            .then_with(|| a.name.cmp(&b.name))
    });
    filtered
}

fn list_cities_internal(app: &AppHandle, country_iso2: &str) -> Result<Vec<CityOption>, String> {
    let iso = country_iso2.trim().to_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let rel = Path::new("cities").join(format!("{iso}.json"));
    if let Some(path) = location_catalog_file(app, &rel) {
        let raw: Vec<CatalogCityWire> = read_json_file(&path)?;
        let capital_id = location_catalog_file(app, Path::new("capitals.json"))
            .and_then(|cap_path| read_json_file::<HashMap<String, i64>>(&cap_path).ok())
            .and_then(|caps| caps.get(&iso).copied());
        let mut out = raw
            .into_iter()
            .map(|c| CityOption {
                geonameid: c.geonameid,
                name: c.name,
                lat: c.lat,
                lon: c.lon,
                population: c.population,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| {
            let a_cap = capital_id == Some(a.geonameid);
            let b_cap = capital_id == Some(b.geonameid);
            b_cap
                .cmp(&a_cap)
                .then_with(|| b.population.cmp(&a.population))
                .then_with(|| a.name.cmp(&b.name))
        });
        if iso == "GB" {
            let constrained = constrain_gb_start_cities(out);
            if !constrained.is_empty() {
                return Ok(constrained);
            }
            let fallback = constrain_gb_start_cities(fallback_cities(&iso));
            if !fallback.is_empty() {
                return Ok(fallback);
            }
            return Err("no GB start cities available".to_string());
        }
        return Ok(out);
    }
    let fallback = fallback_cities(&iso);
    if fallback.is_empty() {
        return Err(format!("no cities available for country {iso}"));
    }
    if iso == "GB" {
        return Ok(constrain_gb_start_cities(fallback));
    }
    Ok(fallback)
}

#[command]
fn list_scenario_saves(app: AppHandle) -> Result<Vec<ScenarioSaveMeta>, String> {
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
fn load_scenario_save(
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
async fn pick_scenario_file(app: AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Scenario JSON", &["json"])
        .blocking_pick_file();
    let path = file
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().to_string());
    Ok(path)
}

#[command]
async fn pick_export_path(app: AppHandle, file_kind: String) -> Result<Option<String>, String> {
    let (label, ext) = match file_kind.to_ascii_lowercase().as_str() {
        "csv" => ("CSV Report", "csv"),
        "json" => ("JSON Report", "json"),
        _ => return Err("file_kind must be csv or json".to_string()),
    };
    let file = app
        .dialog()
        .file()
        .add_filter(label, &[ext])
        .blocking_save_file();
    let path = file
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().to_string());
    Ok(path)
}

#[command]
fn list_countries(app: AppHandle) -> Result<Vec<CountryOption>, String> {
    let path = location_catalog_file(&app, Path::new("countries.json"));
    if let Some(p) = path {
        let raw: Vec<CatalogCountryWire> = read_json_file(&p)?;
        let mut out = raw
            .into_iter()
            .map(|c| CountryOption {
                iso2: c.iso2,
                name: c.name,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        return Ok(out);
    }
    Ok(fallback_countries())
}

#[command]
fn list_cities(app: AppHandle, country_iso2: String) -> Result<Vec<CityOption>, String> {
    list_cities_internal(&app, &country_iso2)
}

#[command]
fn list_country_pack_status(app: AppHandle) -> Result<Vec<CountryPackStatus>, String> {
    let countries = list_countries(app.clone())?;
    let index = read_country_pack_index(&app)?;
    let rollout = rollout_supported_countries();
    let mut out = countries
        .into_iter()
        .map(|country| country_pack_status_for(&app, &index, &rollout, &country.iso2))
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    Ok(out)
}

#[command]
fn install_country_pack(app: AppHandle, country_iso2: String) -> Result<InstallResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let repo_pack_dir = repo_country_pack_dir(&iso);
    if repo_pack_dir.exists() {
        let destination = managed_country_pack_dir(&app, &iso)?;
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&repo_pack_dir, &destination)?;
        let surface_path = destination
            .join("surfaces")
            .join(format!("{iso}.surface.json"));
        let surface = load_surface_wire(&surface_path)?;
        upsert_country_pack_entry(
            &app,
            &iso,
            "installed",
            Some(surface.surface_version.clone()),
            surface.cells_res8.len(),
            Some("repo_pack_copy".to_string()),
        )?;
        country_map_context_cache()
            .lock()
            .map_err(|_| "country_map_context cache poisoned".to_string())?
            .remove(&iso);
        return Ok(InstallResult {
            country_iso2: iso,
            ok: true,
            message: format!(
                "Installed country pack to {}",
                destination.to_string_lossy()
            ),
        });
    }

    let source = repo_demand_surfaces_root().join(format!("{iso}.surface.json"));
    if !source.exists() {
        return Ok(InstallResult {
            country_iso2: iso,
            ok: false,
            message: "Country surface pack not found in local repository".to_string(),
        });
    }
    let surface = load_surface_wire(&source)?;
    let destination = managed_demand_surface_file(&app, &iso)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(&source, &destination).map_err(|e| e.to_string())?;
    upsert_country_pack_entry(
        &app,
        &iso,
        "installed",
        Some(surface.surface_version.clone()),
        surface.cells_res8.len(),
        Some("repo_copy".to_string()),
    )?;
    Ok(InstallResult {
        country_iso2: iso,
        ok: true,
        message: format!(
            "Installed country pack to {}",
            destination.to_string_lossy()
        ),
    })
}

#[command]
fn uninstall_country_pack(app: AppHandle, country_iso2: String) -> Result<UninstallResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let managed_pack_dir = managed_country_pack_dir(&app, &iso)?;
    if managed_pack_dir.exists() {
        fs::remove_dir_all(&managed_pack_dir).map_err(|e| e.to_string())?;
    }
    let managed = managed_demand_surface_file(&app, &iso)?;
    if managed.exists() {
        fs::remove_file(&managed).map_err(|e| e.to_string())?;
    }
    upsert_country_pack_entry(&app, &iso, "missing", None, 0, Some("removed".to_string()))?;
    country_map_context_cache()
        .lock()
        .map_err(|_| "country_map_context cache poisoned".to_string())?
        .remove(&iso);
    Ok(UninstallResult {
        country_iso2: iso,
        ok: true,
        message: "Country pack removed from managed storage".to_string(),
    })
}

#[command]
fn create_game(
    app: AppHandle,
    state: tauri::State<AppState>,
    payload: GameCreatePayload,
) -> Result<OpenSessionResult, String> {
    if payload.country_iso2.trim().len() != 2 {
        return Err("country_iso2 must be a two-letter ISO code".to_string());
    }
    if !payload.city_lon.is_finite() || !payload.city_lat.is_finite() {
        return Err("city coordinates must be finite".to_string());
    }
    let country_iso2 = payload.country_iso2.trim().to_ascii_uppercase();
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
    doc.scenario.world.demand_meta = Some(DemandMeta {
        surface_version: "v4".to_string(),
        loaded_countries: vec![],
        source: "surface_v4_region_scope".to_string(),
    });
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
            country_iso2: payload.country_iso2,
            country_name: payload.country_name,
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
    write_manifest(&project_root, &manifest)?;
    update_index_opened(&app, &project_root, &manifest)?;
    open_session_internal(&app, &state, &project_root)
}

#[command]
fn create_scenario(
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
fn import_scenario(
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
fn list_game_saves(app: AppHandle) -> Result<Vec<GameSaveMeta>, String> {
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
        let scenario_doc =
            ScenarioService::load_from_path(scenario_path(&root).to_string_lossy().as_ref())
                .map_err(|e| e.to_string())
                .ok();
        let stats = scenario_doc
            .as_ref()
            .map(|d| scenario_network_stats(&d.scenario));
        let mut metrics = manifest
            .progress_metrics
            .unwrap_or_else(default_progress_metrics);
        metrics.currency = normalize_currency(Some(&metrics.currency));
        out.push(GameSaveMeta {
            project_id: manifest.project_id.clone(),
            project_path: ent.project_path,
            name: manifest.name,
            last_opened_at: ent.last_opened_at,
            sim_datetime_utc: manifest.clock_state.sim_datetime_utc,
            start_country: manifest
                .start_location
                .as_ref()
                .map(|x| x.country_name.clone()),
            start_city: manifest
                .start_location
                .as_ref()
                .map(|x| x.city_name.clone()),
            unlocked_countries: manifest.economy.unlocked_countries.len(),
            network_stops: stats.as_ref().map(|x| x.stops).unwrap_or(0),
            network_links: stats.as_ref().map(|x| x.links).unwrap_or(0),
            network_services: stats.as_ref().map(|x| x.services).unwrap_or(0),
            total_link_km: stats.as_ref().map(|x| x.total_link_km).unwrap_or(0.0),
            progress_metrics: metrics,
        });
    }
    out.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));
    Ok(out)
}

#[command]
fn continue_latest_game(
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<OpenSessionResult, String> {
    let saves = list_game_saves(app.clone())?;
    let latest = saves
        .first()
        .ok_or_else(|| "no game saves available".to_string())?;
    open_session_internal(&app, &state, Path::new(&latest.project_path))
}

#[command]
fn load_game_save(
    app: AppHandle,
    state: tauri::State<AppState>,
    save_id: String,
) -> Result<OpenSessionResult, String> {
    let idx = read_index(&app)?;
    let entry = idx
        .projects
        .iter()
        .find(|x| x.project_id == save_id)
        .ok_or_else(|| format!("save id not found: {save_id}"))?;
    open_session_internal(&app, &state, Path::new(&entry.project_path))
}

#[command]
fn open_project(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<OpenSessionResult, String> {
    open_session_internal(&app, &state, Path::new(&project_path))
}

#[command]
fn list_deleted_saves(app: AppHandle) -> Result<Vec<DeletedSaveMeta>, String> {
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
fn delete_save(app: AppHandle, save_id: String) -> Result<DeleteSaveResult, String> {
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
fn restore_deleted_save(app: AppHandle, deleted_id: String) -> Result<RestoreSaveResult, String> {
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
fn purge_deleted_save(app: AppHandle, deleted_id: String) -> Result<PurgeSaveResult, String> {
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
fn load_build_defaults() -> Result<BuildDefaults, String> {
    Ok(default_build_defaults(&economy_config()))
}

fn county_iso_and_id_from_region_id(region_id: &str) -> Option<(String, String)> {
    let mut parts = region_id.split(':');
    let tier = parts.next()?.trim();
    let iso = parts.next()?.trim().to_ascii_uppercase();
    let county_id = parts.next()?.trim().to_ascii_lowercase();
    if !tier.eq_ignore_ascii_case("county") || iso.len() != 2 || county_id.is_empty() {
        return None;
    }
    Some((iso, county_id))
}

fn world_xy_to_lonlat_safe(crs: &Crs, x: f64, y: f64) -> Option<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let (mx, my) = world_xy_to_web_mercator_m(crs, x, y);
    let (lon, lat) = web_mercator_m_to_lonlat(mx, my);
    if lon.is_finite() && lat.is_finite() {
        Some((lon, lat))
    } else {
        None
    }
}

fn link_path_lonlat(
    link: &Link,
    stop_xy_by_id: &HashMap<String, (f64, f64)>,
    crs: &Crs,
) -> Vec<(f64, f64)> {
    let mut out = Vec::<(f64, f64)>::new();
    if let Some(geometry) = link.geometry.as_ref() {
        for point in geometry {
            if point.len() < 2 {
                continue;
            }
            if let Some((lon, lat)) = world_xy_to_lonlat_safe(crs, point[0], point[1]) {
                if out.last().copied() != Some((lon, lat)) {
                    out.push((lon, lat));
                }
            }
        }
    }
    if out.len() >= 2 {
        return out;
    }
    let endpoint_ids = [link.from_stop.as_str(), link.to_stop.as_str()];
    for stop_id in endpoint_ids {
        let Some((x, y)) = stop_xy_by_id.get(stop_id).copied() else {
            continue;
        };
        if let Some((lon, lat)) = world_xy_to_lonlat_safe(crs, x, y) {
            if out.last().copied() != Some((lon, lat)) {
                out.push((lon, lat));
            }
        }
    }
    out
}

fn path_hits_county(path: &[(f64, f64)], county: &CountyBoundary) -> bool {
    if path.is_empty() {
        return false;
    }
    for (lon, lat) in path {
        if county.geometry.contains(&Point::new(*lon, *lat)) {
            return true;
        }
    }
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let line = Line::new(Coord { x: a.0, y: a.1 }, Coord { x: b.0, y: b.1 });
        if county.geometry.intersects(&line) {
            return true;
        }
    }
    false
}

fn is_drivable_road_class(road_class: &str) -> bool {
    let class = road_class.trim().to_ascii_lowercase();
    !matches!(
        class.as_str(),
        "pedestrian" | "footway" | "cycleway" | "path" | "bridleway" | "steps" | "track"
    )
}

fn geo_segment_from_points(a: (f64, f64), b: (f64, f64)) -> Option<GeoSegment> {
    if !a.0.is_finite()
        || !a.1.is_finite()
        || !b.0.is_finite()
        || !b.1.is_finite()
        || ((a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12)
    {
        return None;
    }
    Some(GeoSegment {
        a_lon: a.0,
        a_lat: a.1,
        b_lon: b.0,
        b_lat: b.1,
        min_lon: a.0.min(b.0),
        min_lat: a.1.min(b.1),
        max_lon: a.0.max(b.0),
        max_lat: a.1.max(b.1),
    })
}

fn collect_linestring_segments(coords: &[Vec<f64>], out: &mut Vec<GeoSegment>) {
    for pair in coords.windows(2) {
        if pair[0].len() < 2 || pair[1].len() < 2 {
            continue;
        }
        if let Some(seg) =
            geo_segment_from_points((pair[0][0], pair[0][1]), (pair[1][0], pair[1][1]))
        {
            out.push(seg);
        }
    }
}

fn collect_polygon_ring_segments(ring: &[Vec<f64>], out: &mut Vec<GeoSegment>) {
    let mut points = ring
        .iter()
        .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return;
    }
    if points.first() != points.last() {
        if let Some(first) = points.first().copied() {
            points.push(first);
        }
    }
    for pair in points.windows(2) {
        if let Some(seg) = geo_segment_from_points(pair[0], pair[1]) {
            out.push(seg);
        }
    }
}

fn parse_county_mode_constraints_geojson(
    path: &Path,
    include_roads: bool,
    include_water: bool,
) -> Result<CountyModeConstraintData, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let geojson = raw
        .parse::<GeoJson>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let GeoJson::FeatureCollection(fc) = geojson else {
        return Err(format!("{} is not a FeatureCollection", path.display()));
    };
    let mut out = CountyModeConstraintData::default();
    for feature in fc.features {
        let Some(geometry) = feature.geometry else {
            continue;
        };
        let layer = feature
            .properties
            .as_ref()
            .and_then(|props| props.get("feature_layer"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if include_roads && layer == "road" {
            let road_class = feature
                .properties
                .as_ref()
                .and_then(|props| props.get("road_class"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !is_drivable_road_class(road_class) {
                continue;
            }
            match geometry.value {
                GeoJsonValue::LineString(coords) => {
                    collect_linestring_segments(&coords, &mut out.road_segments)
                }
                GeoJsonValue::MultiLineString(lines) => {
                    for coords in lines {
                        collect_linestring_segments(&coords, &mut out.road_segments);
                    }
                }
                _ => {}
            }
        } else if include_water && layer == "water" {
            match geometry.value {
                GeoJsonValue::Polygon(coords) => {
                    for ring in &coords {
                        collect_polygon_ring_segments(ring, &mut out.water_segments);
                    }
                    if let Some(poly) = geojson_coords_to_polygon(&coords) {
                        out.water_polygons.push(MultiPolygon(vec![poly]));
                    }
                }
                GeoJsonValue::MultiPolygon(multi) => {
                    for coords in multi {
                        for ring in &coords {
                            collect_polygon_ring_segments(ring, &mut out.water_segments);
                        }
                        if let Some(poly) = geojson_coords_to_polygon(&coords) {
                            out.water_polygons.push(MultiPolygon(vec![poly]));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

fn load_county_mode_constraints(
    app: &AppHandle,
    county_id: &str,
) -> Result<Arc<CountyModeConstraintData>, String> {
    let county = county_id.trim().to_ascii_lowercase();
    if county.is_empty() {
        return Ok(Arc::new(CountyModeConstraintData::default()));
    }
    {
        let cache = gb_county_mode_constraint_cache()
            .lock()
            .map_err(|_| "county mode constraint cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&county) {
            return Ok(cached.clone());
        }
    }

    let roads_path = county_roads_file(app, "GB", &county)
        .or_else(|| county_basemap_full_file(app, "GB", &county))
        .or_else(|| county_basemap_mid_file(app, "GB", &county));
    let water_path = county_basemap_full_file(app, "GB", &county)
        .or_else(|| county_basemap_mid_file(app, "GB", &county));

    let mut merged = CountyModeConstraintData::default();
    if let Some(path) = roads_path.as_ref() {
        let include_water = water_path
            .as_ref()
            .map(|candidate| candidate == path)
            .unwrap_or(false);
        let parsed = parse_county_mode_constraints_geojson(path, true, include_water)?;
        merged.road_segments.extend(parsed.road_segments);
        merged.water_polygons.extend(parsed.water_polygons);
        merged.water_segments.extend(parsed.water_segments);
    }
    if let Some(path) = water_path.as_ref() {
        let already_loaded = roads_path
            .as_ref()
            .map(|candidate| candidate == path)
            .unwrap_or(false);
        if !already_loaded {
            let parsed = parse_county_mode_constraints_geojson(path, false, true)?;
            merged.water_polygons.extend(parsed.water_polygons);
            merged.water_segments.extend(parsed.water_segments);
        }
    }

    let shared = Arc::new(merged);
    gb_county_mode_constraint_cache()
        .lock()
        .map_err(|_| "county mode constraint cache poisoned".to_string())?
        .insert(county, shared.clone());
    Ok(shared)
}

fn lonlat_distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let avg_lat_rad = ((a.1 + b.1) * 0.5 * std::f64::consts::PI) / 180.0;
    let dx = (b.0 - a.0) * 111_320.0 * avg_lat_rad.cos().abs().max(0.2);
    let dy = (b.1 - a.1) * 110_540.0;
    (dx * dx + dy * dy).sqrt()
}

fn sample_path_points(path: &[(f64, f64)], step_m: f64) -> Vec<(f64, f64)> {
    if path.is_empty() {
        return vec![];
    }
    let mut out = vec![path[0]];
    let step = step_m.max(25.0);
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let len = lonlat_distance_m(a, b);
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        let samples = (len / step).ceil().max(1.0) as usize;
        for idx in 1..=samples {
            let t = idx as f64 / samples as f64;
            let point = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if out.last().copied() != Some(point) {
                out.push(point);
            }
        }
    }
    out
}

fn point_to_segment_distance_m(lon: f64, lat: f64, seg: &GeoSegment) -> f64 {
    let lon_scale = 111_320.0 * lat.to_radians().cos().abs().max(0.2);
    let lat_scale = 110_540.0;
    let ax = seg.a_lon * lon_scale;
    let ay = seg.a_lat * lat_scale;
    let bx = seg.b_lon * lon_scale;
    let by = seg.b_lat * lat_scale;
    let px = lon * lon_scale;
    let py = lat * lat_scale;
    let abx = bx - ax;
    let aby = by - ay;
    let apx = px - ax;
    let apy = py - ay;
    let ab2 = abx * abx + aby * aby;
    if ab2 <= 1e-9 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0);
    let cx = ax + abx * t;
    let cy = ay + aby * t;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn point_near_any_segment_m(lon: f64, lat: f64, segments: &[GeoSegment], threshold_m: f64) -> bool {
    if segments.is_empty() {
        return false;
    }
    let lon_tol = threshold_m / (111_320.0 * lat.to_radians().cos().abs().max(0.2));
    let lat_tol = threshold_m / 110_540.0;
    for seg in segments {
        if lon < seg.min_lon - lon_tol
            || lon > seg.max_lon + lon_tol
            || lat < seg.min_lat - lat_tol
            || lat > seg.max_lat + lat_tol
        {
            continue;
        }
        if point_to_segment_distance_m(lon, lat, seg) <= threshold_m {
            return true;
        }
    }
    false
}

fn county_ids_for_path(path: &[(f64, f64)], counties: &[CountyBoundary]) -> HashSet<String> {
    let mut out = HashSet::<String>::new();
    for county in counties {
        if path_hits_county(path, county) {
            out.insert(county.county_id.clone());
        }
    }
    if out.is_empty() {
        if let Some((lon, lat)) = path.first().copied() {
            if let Some(county) = county_for_lon_lat(counties, lon, lat)
                .or_else(|| nearest_county_for_lon_lat(counties, lon, lat))
            {
                out.insert(county.county_id.clone());
            }
        }
    }
    out
}

const BUS_ROAD_SNAP_MAX_M: f64 = 120.0;
const FERRY_WATER_PATH_TOLERANCE_M: f64 = 120.0;
const FERRY_WATER_TERMINAL_TOLERANCE_M: f64 = 350.0;

fn point_near_roads_in_layers(
    lon: f64,
    lat: f64,
    layers: &[Arc<CountyModeConstraintData>],
) -> bool {
    for layer in layers {
        if point_near_any_segment_m(lon, lat, &layer.road_segments, BUS_ROAD_SNAP_MAX_M) {
            return true;
        }
    }
    false
}

fn point_in_water_layers(lon: f64, lat: f64, layers: &[Arc<CountyModeConstraintData>]) -> bool {
    let point = Point::new(lon, lat);
    for layer in layers {
        for polygon in &layer.water_polygons {
            if polygon.contains(&point) {
                return true;
            }
        }
    }
    false
}

fn point_near_water_layers(
    lon: f64,
    lat: f64,
    layers: &[Arc<CountyModeConstraintData>],
    threshold_m: f64,
) -> bool {
    for layer in layers {
        if point_near_any_segment_m(lon, lat, &layer.water_segments, threshold_m) {
            return true;
        }
    }
    false
}

fn bus_path_matches_roads(path: &[(f64, f64)], layers: &[Arc<CountyModeConstraintData>]) -> bool {
    let has_data = layers.iter().any(|layer| !layer.road_segments.is_empty());
    if !has_data {
        return true;
    }
    for (lon, lat) in sample_path_points(path, 120.0) {
        if !point_near_roads_in_layers(lon, lat, layers) {
            return false;
        }
    }
    true
}

fn ferry_path_matches_water(path: &[(f64, f64)], layers: &[Arc<CountyModeConstraintData>]) -> bool {
    let has_data = layers
        .iter()
        .any(|layer| !layer.water_polygons.is_empty() || !layer.water_segments.is_empty());
    if !has_data {
        return true;
    }
    let Some(start) = path.first().copied() else {
        return false;
    };
    let Some(end) = path.last().copied() else {
        return false;
    };
    if !point_in_water_layers(start.0, start.1, layers)
        && !point_near_water_layers(start.0, start.1, layers, FERRY_WATER_TERMINAL_TOLERANCE_M)
    {
        return false;
    }
    if !point_in_water_layers(end.0, end.1, layers)
        && !point_near_water_layers(end.0, end.1, layers, FERRY_WATER_TERMINAL_TOLERANCE_M)
    {
        return false;
    }

    let samples = sample_path_points(path, 150.0);
    let mut has_open_water = false;
    for (idx, (lon, lat)) in samples.iter().copied().enumerate() {
        let endpoint = idx == 0 || idx + 1 == samples.len();
        if point_in_water_layers(lon, lat, layers) {
            has_open_water = true;
            continue;
        }
        let threshold_m = if endpoint {
            FERRY_WATER_TERMINAL_TOLERANCE_M
        } else {
            FERRY_WATER_PATH_TOLERANCE_M
        };
        if !point_near_water_layers(lon, lat, layers, threshold_m) {
            return false;
        }
    }
    has_open_water || samples.len() <= 2
}

fn stop_type_requires_road(stop_type: Option<&str>) -> bool {
    stop_type
        .map(|value| value.trim().to_ascii_lowercase().contains("bus"))
        .unwrap_or(false)
}

fn stop_type_requires_water(stop_type: Option<&str>) -> bool {
    stop_type
        .map(|value| value.trim().to_ascii_lowercase().contains("ferry"))
        .unwrap_or(false)
}

fn default_mutation_path_validation_meta() -> MutationPathValidationMeta {
    MutationPathValidationMeta {
        path_validation_mode: "proximity".to_string(),
        road_snap_tolerance_m: BUS_ROAD_SNAP_MAX_M,
        water_path_tolerance_m: FERRY_WATER_PATH_TOLERANCE_M,
        water_terminal_tolerance_m: FERRY_WATER_TERMINAL_TOLERANCE_M,
        ..MutationPathValidationMeta::default()
    }
}

fn validate_mutation_respects_unlocked_gb_counties(
    app: &AppHandle,
    current: &Scenario,
    next: &Scenario,
    manifest: &ProjectManifest,
) -> Result<MutationPathValidationMeta, String> {
    let mut validation_meta = default_mutation_path_validation_meta();
    if manifest.session_kind != SessionKind::Game {
        return Ok(validation_meta);
    }
    let country_iso2 = primary_project_country_iso2(manifest).unwrap_or_default();
    if !country_iso2.eq_ignore_ascii_case("GB") {
        return Ok(validation_meta);
    }

    let catalog = load_gb_county_boundaries()?;
    let counties = catalog.counties;
    if counties.is_empty() {
        return Ok(validation_meta);
    }

    let unlocked_county_ids = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter_map(|id| county_iso_and_id_from_region_id(&id))
        .filter(|(iso, _)| iso.eq_ignore_ascii_case("GB"))
        .map(|(_, county_id)| county_id)
        .collect::<HashSet<_>>();
    if unlocked_county_ids.is_empty() {
        return Ok(validation_meta);
    }

    let locked_counties = counties
        .iter()
        .filter(|county| !unlocked_county_ids.contains(&county.county_id))
        .collect::<Vec<_>>();
    let locked_county_ids = locked_counties
        .iter()
        .map(|county| county.county_id.clone())
        .collect::<HashSet<_>>();

    let current_stop_by_id = current
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), stop))
        .collect::<HashMap<_, _>>();
    let current_link_by_id = current
        .world
        .links
        .iter()
        .map(|link| (link.id.clone(), link))
        .collect::<HashMap<_, _>>();
    let next_stop_xy_by_id = next
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), (stop.x, stop.y)))
        .collect::<HashMap<_, _>>();
    let mut blocked_counties = BTreeSet::<String>::new();
    let mut road_invalid = BTreeSet::<String>::new();
    let mut water_invalid = BTreeSet::<String>::new();

    for stop in &next.world.stops {
        let changed = current_stop_by_id
            .get(&stop.id)
            .map(|prev| (prev.x - stop.x).abs() > 1e-6 || (prev.y - stop.y).abs() > 1e-6)
            .unwrap_or(true);
        if !changed {
            continue;
        }
        let Some((lon, lat)) = world_xy_to_lonlat_safe(&next.meta.crs, stop.x, stop.y) else {
            continue;
        };
        validation_meta.changed_stops_checked =
            validation_meta.changed_stops_checked.saturating_add(1);
        let county = county_for_lon_lat(&counties, lon, lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, lon, lat));
        let Some(county) = county else {
            continue;
        };
        if locked_county_ids.contains(&county.county_id) {
            blocked_counties.insert(county.name.clone());
        }
        let layers = load_county_mode_constraints(app, &county.county_id)?;
        let stop_label = stop
            .name
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| stop.id.clone());
        if stop_type_requires_road(stop.stop_type.as_deref()) {
            validation_meta.road_stops_checked =
                validation_meta.road_stops_checked.saturating_add(1);
            if !layers.road_segments.is_empty()
                && !point_near_any_segment_m(lon, lat, &layers.road_segments, BUS_ROAD_SNAP_MAX_M)
            {
                road_invalid.insert(format!("stop:{stop_label}"));
                validation_meta.road_stops_invalid =
                    validation_meta.road_stops_invalid.saturating_add(1);
            }
        }
        if stop_type_requires_water(stop.stop_type.as_deref()) {
            validation_meta.water_stops_checked =
                validation_meta.water_stops_checked.saturating_add(1);
            if (!layers.water_polygons.is_empty() || !layers.water_segments.is_empty())
                && !point_in_water_layers(lon, lat, std::slice::from_ref(&layers))
                && !point_near_any_segment_m(
                    lon,
                    lat,
                    &layers.water_segments,
                    FERRY_WATER_TERMINAL_TOLERANCE_M,
                )
            {
                water_invalid.insert(format!("stop:{stop_label}"));
                validation_meta.water_stops_invalid =
                    validation_meta.water_stops_invalid.saturating_add(1);
            }
        }
    }

    for link in &next.world.links {
        let changed = current_link_by_id
            .get(&link.id)
            .map(|prev| {
                prev.from_stop != link.from_stop
                    || prev.to_stop != link.to_stop
                    || prev.geometry != link.geometry
                    || prev.mode != link.mode
            })
            .unwrap_or(true);
        if !changed {
            continue;
        }
        let path = link_path_lonlat(link, &next_stop_xy_by_id, &next.meta.crs);
        if path.len() < 2 {
            continue;
        }
        validation_meta.changed_links_checked =
            validation_meta.changed_links_checked.saturating_add(1);
        for county in &locked_counties {
            if path_hits_county(&path, county) {
                blocked_counties.insert(county.name.clone());
            }
        }
        let county_ids = county_ids_for_path(&path, &counties);
        let mut layers = Vec::<Arc<CountyModeConstraintData>>::new();
        for county_id in county_ids {
            layers.push(load_county_mode_constraints(app, &county_id)?);
        }
        let mode = link.mode.trim().to_ascii_lowercase();
        if mode == "bus" {
            validation_meta.bus_links_checked = validation_meta.bus_links_checked.saturating_add(1);
            if !bus_path_matches_roads(&path, &layers) {
                road_invalid.insert(format!("link:{}", link.id));
                validation_meta.bus_links_invalid =
                    validation_meta.bus_links_invalid.saturating_add(1);
            }
        }
        if mode == "ferry" {
            validation_meta.ferry_links_checked =
                validation_meta.ferry_links_checked.saturating_add(1);
            if !ferry_path_matches_water(&path, &layers) {
                water_invalid.insert(format!("link:{}", link.id));
                validation_meta.ferry_links_invalid =
                    validation_meta.ferry_links_invalid.saturating_add(1);
            }
        }
    }

    let mut errors = Vec::<String>::new();
    if !blocked_counties.is_empty() {
        let counties_list = blocked_counties.into_iter().collect::<Vec<_>>();
        validation_meta.locked_county_hits = counties_list.len();
        errors.push(format!(
            "LockedCountyViolation: unlock before building in: {}",
            counties_list.join(", ")
        ));
    }
    if !road_invalid.is_empty() {
        let offenders = road_invalid.into_iter().collect::<Vec<_>>();
        errors.push(format!(
            "ModePathInvalidRoad: bus geometry must follow drivable roads ({})",
            offenders.join(", ")
        ));
    }
    if !water_invalid.is_empty() {
        let offenders = water_invalid.into_iter().collect::<Vec<_>>();
        errors.push(format!(
            "ModePathInvalidWater: ferry geometry must remain on water with shoreline terminals ({})",
            offenders.join(", ")
        ));
    }
    if errors.is_empty() {
        Ok(validation_meta)
    } else {
        Err(errors.join(" | "))
    }
}

fn apply_difficulty_to_mutation_summary(
    summary: &mut NetworkMutationSummary,
    profile: &DifficultyProfile,
) {
    let capex_mult = profile.capex_mult.max(0.0);
    let opex_mult = profile.opex_mult.max(0.0);
    let previous_apply_total = summary.apply_total_delta_base;
    summary.capex_delta_base *= capex_mult;
    summary.infra_capex_delta_base *= capex_mult;
    summary.fleet_purchase_base *= capex_mult;
    summary.fleet_upgrade_base *= capex_mult;
    summary.fleet_transfer_fees_base *= capex_mult;
    summary.fleet_salvage_refund_base *= capex_mult;
    summary.net_capex_delta_base *= capex_mult;
    summary.construction_cost_delta_base *= capex_mult;
    summary.fleet_purchase_delta_base *= capex_mult;
    summary.fleet_configuration_delta_base *= capex_mult;
    summary.apply_total_delta_base *= capex_mult;
    summary.estimated_total_capex_base *= capex_mult;
    summary.projected_opex_per_hour_base *= opex_mult;
    summary.projected_staff_opex_per_hour_base *= opex_mult;
    summary.estimated_total_opex_per_hour_base *= opex_mult;
    if let Some(balance_after_apply) = summary.projected_balance_after_apply_base {
        let implied_balance_before = balance_after_apply + previous_apply_total;
        summary.projected_balance_after_apply_base =
            Some(implied_balance_before - summary.apply_total_delta_base);
    }
}

#[command]
fn preview_network_mutation(
    app: AppHandle,
    project_path: String,
    scenario_document: ScenarioDocumentLite,
) -> Result<NetworkMutationPreviewResult, String> {
    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;

    let current_doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let next_doc = ScenarioService::migrate_to_current(ScenarioDocument {
        schema_version: scenario_document.schema_version,
        scenario: scenario_document.scenario,
    })
    .map_err(|e| e.to_string())?;
    ScenarioService::validate(&next_doc.scenario).map_err(|e| e.to_string())?;
    let manifest = read_manifest(&project_root)?;
    let path_validation = validate_mutation_respects_unlocked_gb_counties(
        &app,
        &current_doc.scenario,
        &next_doc.scenario,
        &manifest,
    )?;
    let cfg = economy_config();
    let summary = summarize_network_mutation(
        &current_doc.scenario,
        &next_doc.scenario,
        &cfg,
        Some(manifest.economy.current_balance_base),
    );
    let mut summary = summary;
    let profile = resolved_difficulty_profile(&manifest);
    apply_difficulty_to_mutation_summary(&mut summary, &profile);
    let cost_breakdown = mutation_cost_breakdown(&summary);
    Ok(NetworkMutationPreviewResult {
        summary,
        cost_breakdown,
        path_validation,
    })
}

#[command]
fn apply_network_mutation(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    scenario_document: ScenarioDocumentLite,
    capex_override_base: Option<f64>,
) -> Result<NetworkMutationResult, String> {
    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;

    let current_doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let next_doc = ScenarioService::migrate_to_current(ScenarioDocument {
        schema_version: scenario_document.schema_version,
        scenario: scenario_document.scenario,
    })
    .map_err(|e| e.to_string())?;
    ScenarioService::validate(&next_doc.scenario).map_err(|e| e.to_string())?;

    let mut manifest = read_manifest(&project_root)?;
    let path_validation = validate_mutation_respects_unlocked_gb_counties(
        &app,
        &current_doc.scenario,
        &next_doc.scenario,
        &manifest,
    )?;
    let cfg = economy_config();
    let summary = summarize_network_mutation(
        &current_doc.scenario,
        &next_doc.scenario,
        &cfg,
        Some(manifest.economy.current_balance_base),
    );
    let mut summary = summary;
    let profile = resolved_difficulty_profile(&manifest);
    apply_difficulty_to_mutation_summary(&mut summary, &profile);
    let cost_breakdown = mutation_cost_breakdown(&summary);
    let capex_override_scaled = capex_override_base
        .filter(|value| value.is_finite())
        .map(|value| value * profile.capex_mult.max(0.0));
    apply_build_budget(&mut manifest, &cfg, &summary, capex_override_scaled)?;
    let applied_total_delta_base = capex_override_scaled
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(summary.apply_total_delta_base);
    if applied_total_delta_base.is_finite() {
        if applied_total_delta_base >= 0.0 {
            update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, applied_total_delta_base);
            record_monthly_financial_delta(&mut manifest, 0.0, 0.0, applied_total_delta_base, 0.0);
        } else {
            let refund_base = -applied_total_delta_base;
            manifest.economy.cumulative_revenue_base += refund_base;
            update_region_ledger(&mut manifest, refund_base, 0.0, 0.0, 0.0);
            record_monthly_financial_delta(&mut manifest, refund_base, 0.0, 0.0, 0.0);
        }
        bump_economy_revision(&mut manifest);
    }
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();

    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &next_doc,
    )
    .map_err(|e| e.to_string())?;
    write_manifest(&project_root, &manifest)?;

    if project_is_current(&state, &project_path)? {
        let mut guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_mut() {
            rehydrate_game_state_scenario(game_state, &next_doc.scenario);
        }
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path,
            RuntimeAction::InvalidateMaterialization,
        )?;
    }
    Ok(NetworkMutationResult {
        scenario: ScenarioDocumentLite {
            schema_version: next_doc.schema_version,
            scenario: next_doc.scenario,
        },
        manifest,
        summary,
        cost_breakdown,
        path_validation,
    })
}

#[command]
fn save_session(
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
fn inspect_station(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    stop_id: String,
) -> Result<StationInspection, String> {
    let project_root = PathBuf::from(&project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let output = inspection_output_for_project(&state, &project_path, &doc.scenario).ok();
    let mut inspection = inspect_station_from_scenario(&doc.scenario, output.as_ref(), &stop_id)?;
    if let Ok(Some(snapshot)) = latest_runtime_snapshot_for_project(state.inner(), &project_path) {
        if let Some(runtime_station) = snapshot.stations.iter().find(|s| s.stop_id == stop_id) {
            inspection.station_load_current_pax = runtime_station.current_inside_pax.max(0.0);
            inspection.station_queue_capacity_pax = runtime_station.capacity_pax.max(0.0);
            inspection.passengers_declined_last_hour = runtime_station.declined_last_hour.max(0.0);
            inspection.station_entries_per_hour = runtime_station.entries_per_hour.max(0.0);
            inspection.station_exits_per_hour = runtime_station.exits_per_hour.max(0.0);
            inspection.average_wait_to_board_s = runtime_station.avg_wait_to_board_s.max(0.0);
            inspection.queue_end = runtime_station.current_inside_pax.max(0.0);
        }
    }
    let _ = enrich_station_inspection_with_landuse(&app, &doc.scenario, &mut inspection);
    Ok(inspection)
}

#[command]
fn inspect_line(
    state: tauri::State<AppState>,
    project_path: String,
    line_id: String,
) -> Result<LineInspection, String> {
    let project_root = PathBuf::from(&project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let output = inspection_output_for_project(&state, &project_path, &doc.scenario).ok();
    let manifest = read_manifest(&project_root)?;
    let minute_of_day = clock_minute_of_day(&manifest.clock_state);
    let mut inspection = inspect_line_from_scenario(
        &doc.scenario,
        output.as_ref(),
        &line_id,
        &economy_config(),
        Some(minute_of_day),
    )?;
    if let Ok(Some(snapshot)) = latest_runtime_snapshot_for_project(state.inner(), &project_path) {
        if let Some(runtime_line) = snapshot
            .line_ops
            .iter()
            .find(|line| line.line_id == line_id)
        {
            inspection.boardings_attempted = runtime_line.boardings_attempted_per_hour.max(0.0);
            inspection.boardings_served = runtime_line.boarded_per_hour.max(0.0);
            inspection.alightings_served = runtime_line.alighted_per_hour.max(0.0);
            inspection.denied_boardings = runtime_line.denied_boardings_per_hour.max(0.0);
            inspection.queue_end = runtime_line.queue_end_pax.max(0.0);
            inspection.avg_wait_s = Some(runtime_line.mean_wait_s.max(0.0));
            inspection.operations_now.avg_wait_s = Some(runtime_line.mean_wait_s.max(0.0));
        }
    }
    Ok(inspection)
}

#[command]
fn save_and_quit(
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
    Ok(result)
}

#[command]
fn start_runtime_loop(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
) -> Result<RuntimeLoopStatus, String> {
    start_runtime_loop_internal(&app, state.inner(), &project_path)
}

#[command]
fn stop_runtime_loop(
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
fn get_runtime_snapshot(
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
fn get_runtime_fast_snapshot(
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
fn get_runtime_strategic_snapshot(
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
fn enqueue_runtime_action(
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
fn set_simulation_speed(
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
fn set_simulation_running(
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
fn get_fare_policy(project_path: String) -> Result<FarePolicyManifest, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    sanitize_fare_policy(&mut manifest.economy.fare_policy);
    Ok(manifest.economy.fare_policy)
}

#[command]
fn set_fare_policy(
    state: tauri::State<AppState>,
    project_path: String,
    policy_patch: FarePolicyPatch,
) -> Result<FarePolicyManifest, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    merge_fare_policy(&mut manifest.economy.fare_policy, &policy_patch);
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let project_path_string = project_root.to_string_lossy().to_string();
    let _ = enqueue_runtime_action_with_retry(
        state.inner(),
        &project_path_string,
        RuntimeAction::InvalidateMaterialization,
    )?;
    Ok(manifest.economy.fare_policy)
}

#[command]
fn expedite_fleet_delivery(
    state: tauri::State<AppState>,
    project_path: String,
    line_id: String,
    order_id: String,
) -> Result<FleetDeliveryExpediteResult, String> {
    let normalized_line_id = line_id.trim();
    if normalized_line_id.is_empty() {
        return Err("line_id is required".to_string());
    }
    let normalized_order_id = order_id.trim();
    if normalized_order_id.is_empty() {
        return Err("order_id is required".to_string());
    }

    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let mut manifest = read_manifest(&project_root)?;
    let defaults = default_build_defaults(&economy_config());

    let service_indexes = doc
        .scenario
        .world
        .services
        .iter()
        .enumerate()
        .filter_map(|(index, service)| {
            if service_line_runtime_id(service) == normalized_line_id {
                Some(index)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let Some(first_service_index) = service_indexes.first().copied() else {
        return Err("line not found".to_string());
    };
    let sample = doc
        .scenario
        .world
        .services
        .get(first_service_index)
        .cloned()
        .ok_or_else(|| "line service could not be resolved".to_string())?;
    let sample_profile = sample.rolling_stock_profile.clone().unwrap_or_default();
    let current_units_owned = sample_profile
        .units_owned
        .unwrap_or_else(|| sample.stock_units_owned.unwrap_or(0));
    let mut pending_orders = sample_profile.pending_orders.clone();
    if pending_orders.is_empty() {
        return Err("line has no pending purchase orders".to_string());
    }
    let Some(order_position) = pending_orders.iter().position(|order| {
        order.order_id.trim() == normalized_order_id
            && order.units > 0
            && is_pending_purchase_order_status(order.status.as_deref())
    }) else {
        return Err("pending purchase order not found on the selected line".to_string());
    };

    let fallback_unit_cost_base = estimate_unit_purchase_cost_base_for_service(&sample, &defaults);
    let unit_cost_base =
        resolve_order_unit_cost_base(&pending_orders[order_position], fallback_unit_cost_base)
            .ok_or_else(|| "unable to resolve unit purchase cost for this order".to_string())?;
    let expedite_cost_base = (unit_cost_base * FLEET_EXPEDITE_MULTIPLIER)
        .max(unit_cost_base + FLEET_EXPEDITE_MIN_SURCHARGE_BASE);
    if !expedite_cost_base.is_finite() || expedite_cost_base <= 0.0 {
        return Err("failed to compute expedite cost".to_string());
    }
    if manifest.economy.current_balance_base + 1e-6 < expedite_cost_base {
        return Err(format!(
            "Insufficient funds: requires {:.0} base currency, available {:.0}.",
            expedite_cost_base.max(0.0).round(),
            manifest.economy.current_balance_base.max(0.0).round()
        ));
    }

    let mut remaining_order_units = 0_u32;
    if let Some(order) = pending_orders.get_mut(order_position) {
        let per_unit_for_totals =
            resolve_order_unit_cost_base(order, Some(unit_cost_base)).unwrap_or(unit_cost_base);
        if order.units <= 1 {
            pending_orders.remove(order_position);
        } else {
            order.units = order.units.saturating_sub(1);
            remaining_order_units = order.units;
            if per_unit_for_totals.is_finite() && per_unit_for_totals > 0.0 {
                order.unit_cost_base = Some(per_unit_for_totals);
                order.total_cost_base = Some(per_unit_for_totals * order.units as f64);
            }
        }
    }

    let next_units_owned = current_units_owned.saturating_add(1);
    for service_index in service_indexes {
        let Some(service) = doc.scenario.world.services.get_mut(service_index) else {
            continue;
        };
        let mut profile = service.rolling_stock_profile.clone().unwrap_or_default();
        profile.units_owned = Some(next_units_owned);
        profile.pending_orders = pending_orders.clone();
        service.stock_units_owned = Some(next_units_owned);
        service.stock_units_assigned = Some(next_units_owned);
        service.rolling_stock_profile = Some(profile);
    }

    manifest.economy.current_balance_base -= expedite_cost_base;
    manifest.economy.cumulative_capex_base += expedite_cost_base;
    update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, expedite_cost_base);
    record_monthly_financial_delta(&mut manifest, 0.0, 0.0, expedite_cost_base, 0.0);
    bump_economy_revision(&mut manifest);
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();

    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    write_manifest(&project_root, &manifest)?;

    if project_is_current(&state, &project_path)? {
        let mut guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_mut() {
            rehydrate_game_state_scenario(game_state, &doc.scenario);
        }
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path,
            RuntimeAction::InvalidateMaterialization,
        )?;
    }

    Ok(FleetDeliveryExpediteResult {
        line_id: normalized_line_id.to_string(),
        order_id: normalized_order_id.to_string(),
        delivered_units: 1,
        remaining_order_units,
        expedite_cost_base,
        balance_after_base: manifest.economy.current_balance_base,
    })
}

#[command]
fn advance_simulation(
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
fn save_sandbox_snapshot(
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
fn load_sandbox_snapshot(
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

#[command]
fn ensure_country_demand_surface(
    app: AppHandle,
    project_path: String,
    country_iso2: String,
) -> Result<DemandCoverageResult, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let result =
        ensure_country_surface_loaded(&app, &mut manifest, &mut doc.scenario, &country_iso2)?;
    if result.loaded {
        ScenarioService::save_to_path(
            scenario_path(&project_root).to_string_lossy().as_ref(),
            &doc,
        )
        .map_err(|e| e.to_string())?;
    }
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(result)
}

#[command]
fn list_demand_coverage(
    app: AppHandle,
    project_path: String,
) -> Result<Vec<DemandCoverageMeta>, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let unlocked = unlocked_country_codes(&manifest);
    let mut out = Vec::<DemandCoverageMeta>::new();
    for iso in unlocked {
        let cells = doc
            .scenario
            .world
            .demand_cells
            .iter()
            .filter(|c| {
                c.country_iso2
                    .as_deref()
                    .map(|v| v.eq_ignore_ascii_case(&iso))
                    .unwrap_or(false)
            })
            .count();
        out.push(DemandCoverageMeta {
            country_iso2: iso.clone(),
            installed: demand_surface_file(&app, &iso).is_some(),
            loaded_in_scenario: cells > 0,
            cells,
            surface_version: manifest
                .demand_surface
                .as_ref()
                .map(|d| d.surface_version.clone()),
        });
    }
    out.sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    Ok(out)
}

fn primary_project_country_iso2(manifest: &ProjectManifest) -> Option<String> {
    manifest
        .start_location
        .as_ref()
        .map(|start| start.country_iso2.trim().to_ascii_uppercase())
        .filter(|iso| iso.len() == 2)
        .or_else(|| unlocked_country_codes(manifest).into_iter().next())
}

#[command]
fn load_map_runtime_config(
    app: AppHandle,
    project_path: String,
) -> Result<MapRuntimeConfig, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let iso = primary_project_country_iso2(&manifest)
        .ok_or_else(|| "project has no country context".to_string())?;
    let server = ensure_map_asset_server(&app)?;
    let world_path =
        world_context_file(&app, &iso).ok_or_else(|| "world context asset missing".to_string())?;
    let vector_ready = world_path.exists()
        && counties_file(&app, &iso).is_some()
        && basemap_file(&app, &iso).is_some()
        && style_template_file(&app, &iso).is_some();
    let has_mid = county_basemap_mid_dir(&app, &iso).is_some()
        || country_pack_dir(&app, &iso)
            .map(|dir| dir.join("map").join("county_roads").exists())
            .unwrap_or(false);
    let has_full = county_basemap_full_dir(&app, &iso).is_some()
        || country_pack_dir(&app, &iso)
            .map(|dir| dir.join("map").join("county_roads").exists())
            .unwrap_or(false);
    let default_bounds = if iso == "GB" {
        load_gb_county_boundaries()
            .ok()
            .and_then(|catalog| counties_bounds(&catalog.counties))
    } else {
        None
    };
    let legacy_ready = world_path.exists() && (has_mid || has_full);
    let map_ready = vector_ready || legacy_ready;
    Ok(MapRuntimeConfig {
        country_iso2: iso.clone(),
        style_url: vector_ready.then(|| {
            format!(
                "{}/map/{}/style.json",
                server.base_url,
                iso.to_ascii_lowercase()
            )
        }),
        world_context_url: format!("{}/map/{}/world_context.geojson", server.base_url, iso),
        counties_url: counties_file(&app, &iso)
            .map(|_| format!("{}/map/{}/counties.geojson", server.base_url, iso)),
        major_roads_url: (!vector_ready)
            .then_some(())
            .and_then(|_| major_roads_file(&app, &iso))
            .map(|_| format!("{}/map/{}/major_roads.geojson", server.base_url, iso)),
        county_basemap_mid_url_template: (!vector_ready && has_mid).then(|| {
            format!(
                "{}/map/{}/county_basemap_mid/{{county_id}}.geojson",
                server.base_url, iso
            )
        }),
        county_basemap_full_url_template: (!vector_ready && has_full).then(|| {
            format!(
                "{}/map/{}/county_basemap_full/{{county_id}}.geojson",
                server.base_url, iso
            )
        }),
        default_bounds,
        map_pack_version: Some(
            if vector_ready {
                "vector-mbtiles-v1"
            } else if county_basemap_full_dir(&app, &iso).is_some()
                || county_basemap_mid_dir(&app, &iso).is_some()
            {
                "geojson-basemap-v2"
            } else {
                "geojson-roads-v1"
            }
            .to_string(),
        ),
        map_ready,
    })
}

#[command]
fn load_country_map_context(
    app: AppHandle,
    project_path: String,
) -> Result<CountryMapContext, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let iso = primary_project_country_iso2(&manifest)
        .ok_or_else(|| "project has no country context".to_string())?;

    {
        let cache = country_map_context_cache()
            .lock()
            .map_err(|_| "country_map_context cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&iso) {
            return Ok(cached.clone());
        }
    }

    let world_path =
        world_context_file(&app, &iso).ok_or_else(|| "world context asset missing".to_string())?;
    let major_path = major_roads_file(&app, &iso);
    let mut world_context = read_feature_collection_json(&world_path)?;
    if world_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("countries.geojson"))
        .unwrap_or(false)
    {
        world_context = world_context_from_countries_geojson(world_context)?;
    }
    let major_roads = match major_path {
        Some(path) => {
            read_feature_collection_json(&path).unwrap_or_else(|_| empty_feature_collection_json())
        }
        None => empty_feature_collection_json(),
    };
    let default_bounds = if iso == "GB" {
        load_gb_county_boundaries()
            .ok()
            .and_then(|catalog| counties_bounds(&catalog.counties))
    } else {
        None
    };
    let context = CountryMapContext {
        country_iso2: iso.clone(),
        world_context,
        major_roads,
        default_bounds,
    };
    country_map_context_cache()
        .lock()
        .map_err(|_| "country_map_context cache poisoned".to_string())?
        .insert(iso, context.clone());
    Ok(context)
}

#[command]
fn load_region_street_context(
    app: AppHandle,
    _project_path: String,
    region_id: String,
) -> Result<RegionStreetContext, String> {
    let normalized =
        normalize_region_id(&region_id).ok_or_else(|| "invalid region_id".to_string())?;
    let normalized =
        canonicalize_region_id(&normalized).ok_or_else(|| "invalid region_id".to_string())?;
    {
        let cache = region_street_context_cache()
            .lock()
            .map_err(|_| "region_street_context cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&normalized) {
            return Ok(cached.clone());
        }
    }
    let iso =
        region_country_iso2(&normalized).ok_or_else(|| "region country missing".to_string())?;
    let county_id = normalized
        .split(':')
        .nth(2)
        .ok_or_else(|| "region county token missing".to_string())?;
    let roads = if normalized.starts_with("county:") {
        match county_roads_file(&app, &iso, county_id) {
            Some(path) => read_feature_collection_json(&path)
                .unwrap_or_else(|_| empty_feature_collection_json()),
            None => empty_feature_collection_json(),
        }
    } else {
        empty_feature_collection_json()
    };
    let context = RegionStreetContext {
        region_id: normalized.clone(),
        country_iso2: iso,
        roads,
    };
    region_street_context_cache()
        .lock()
        .map_err(|_| "region_street_context cache poisoned".to_string())?
        .insert(normalized, context.clone());
    Ok(context)
}

#[command]
fn rebuild_demand_for_unlocked(
    app: AppHandle,
    project_path: String,
) -> Result<DemandRebuildResult, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let mut loaded = Vec::<String>::new();
    let mut missing = Vec::<String>::new();
    for iso in unlocked_country_codes(&manifest) {
        let status = ensure_country_surface_loaded(&app, &mut manifest, &mut doc.scenario, &iso)?;
        if status.loaded {
            loaded.push(iso);
        } else {
            missing.push(iso);
        }
    }
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(DemandRebuildResult {
        loaded_countries: loaded,
        missing_countries: missing,
        total_cells: doc.scenario.world.demand_cells.len(),
    })
}

#[command]
fn get_financial_dashboard(
    _app: AppHandle,
    project_path: String,
    request: FinancialDashboardRequest,
) -> Result<FinancialDashboardResponse, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let cfg = economy_config();
    let minute_of_day =
        ((manifest.clock_state.tick_seconds / 60.0).floor() as i64).rem_euclid(1440) as u32;

    let granularity = normalize_financial_granularity(request.granularity.as_deref());
    let default_periods = match granularity.as_str() {
        "day" => 30,
        "week" => 16,
        "year" => 8,
        _ => 12,
    };
    let periods = request.periods.unwrap_or(default_periods).clamp(1, 240);
    let mode_filter = request
        .mode
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "all");
    let line_filter = request
        .line_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));
    let region_filter = request
        .region_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"))
        .and_then(|value| canonicalize_region_id(&value).or(Some(value)));
    let filters_active = mode_filter.is_some() || line_filter.is_some() || region_filter.is_some();

    let project_country_iso2 = primary_project_country_iso2(&manifest).unwrap_or_default();
    let gb_counties = if project_country_iso2.eq_ignore_ascii_case("GB") {
        load_gb_county_boundaries()
            .ok()
            .map(|catalog| catalog.counties)
    } else {
        None
    };

    let mut line_ids = BTreeSet::<String>::new();
    for service in &doc.scenario.world.services {
        line_ids.insert(service_line_runtime_id(service));
    }

    let mut all_line_rows = Vec::<FinancialLineBreakdownRow>::new();
    for line_id in line_ids {
        let inspection = match inspect_line_from_scenario(
            &doc.scenario,
            None,
            &line_id,
            &cfg,
            Some(minute_of_day),
        ) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let line_name = if inspection.name.trim().is_empty() {
            line_id.clone()
        } else {
            inspection.name.clone()
        };
        let mode = inspection.mode.trim().to_ascii_lowercase();
        let mut region_id = None::<String>;
        if let Some(counties) = gb_counties.as_ref() {
            let mut counts = HashMap::<String, usize>::new();
            for station in &inspection.stations {
                let Some((lon, lat)) =
                    world_xy_to_lonlat_safe(&doc.scenario.meta.crs, station.x, station.y)
                else {
                    continue;
                };
                let county = county_for_lon_lat(counties, lon, lat)
                    .or_else(|| nearest_county_for_lon_lat(counties, lon, lat));
                let Some(county) = county else { continue };
                let key = canonicalize_region_id(&region_id_from_county("GB", &county.county_id))
                    .unwrap_or_else(|| region_id_from_county("GB", &county.county_id));
                *counts.entry(key).or_insert(0) += 1;
            }
            region_id = counts
                .into_iter()
                .max_by(|(left_id, left_count), (right_id, right_count)| {
                    left_count
                        .cmp(right_count)
                        .then_with(|| left_id.cmp(right_id))
                })
                .map(|(id, _)| id);
        }

        all_line_rows.push(FinancialLineBreakdownRow {
            line_id: line_id.clone(),
            line_name,
            mode,
            region_id,
            estimated_capex_base: inspection.estimated_capex_base.max(0.0),
            estimated_opex_per_hour_base: inspection.estimated_opex_per_hour_base.max(0.0),
            staff_opex_per_hour_base: inspection.cost_story.staff_opex_per_hour_base.max(0.0),
            fleet_value_base: inspection.cost_story.fleet_value_base.max(0.0),
            units_owned: inspection.fleet_state.units_owned,
            units_pending: inspection.fleet_state.units_pending,
            units_assigned: inspection.fleet_state.units_assigned,
        });
    }
    all_line_rows.sort_by(|a, b| {
        b.estimated_opex_per_hour_base
            .partial_cmp(&a.estimated_opex_per_hour_base)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.line_name.cmp(&b.line_name))
    });

    let filtered_line_rows = all_line_rows
        .iter()
        .filter(|row| {
            if let Some(mode) = mode_filter.as_ref() {
                if row.mode != *mode {
                    return false;
                }
            }
            if let Some(line_id) = line_filter.as_ref() {
                if row.line_id != *line_id {
                    return false;
                }
            }
            if let Some(region_id) = region_filter.as_ref() {
                if row.region_id.as_deref() != Some(region_id.as_str()) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut mode_breakdown_map = BTreeMap::<String, FinancialModeBreakdownRow>::new();
    for line in &filtered_line_rows {
        let entry =
            mode_breakdown_map
                .entry(line.mode.clone())
                .or_insert(FinancialModeBreakdownRow {
                    mode: line.mode.clone(),
                    lines: 0,
                    revenue_base: 0.0,
                    opex_base: 0.0,
                    capex_base: 0.0,
                    penalties_base: 0.0,
                    net_base: 0.0,
                });
        entry.lines = entry.lines.saturating_add(1);
        entry.opex_base += line.estimated_opex_per_hour_base.max(0.0);
        entry.capex_base += line.estimated_capex_base.max(0.0);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    let mut mode_breakdown = mode_breakdown_map.into_values().collect::<Vec<_>>();
    mode_breakdown.sort_by(|a, b| a.mode.cmp(&b.mode));

    let mut canonical_region_ledger = manifest.economy.region_ledger.clone();
    canonicalize_region_ledger(&mut canonical_region_ledger);
    let mut region_breakdown = canonical_region_ledger
        .into_iter()
        .map(|(region_id, row)| FinancialRegionBreakdownRow {
            region_id,
            revenue_base: row.revenue_base.max(0.0),
            opex_base: row.opex_base.max(0.0),
            capex_base: row.capex_base.max(0.0),
            penalties_base: row.penalties_base.max(0.0),
            net_base: row.net_base,
        })
        .collect::<Vec<_>>();
    if let Some(region_id) = region_filter.as_ref() {
        region_breakdown.retain(|row| row.region_id == *region_id);
    }
    region_breakdown.sort_by(|a, b| {
        b.net_base
            .partial_cmp(&a.net_base)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });

    let all_line_opex_total = all_line_rows
        .iter()
        .map(|row| row.estimated_opex_per_hour_base.max(0.0))
        .sum::<f64>();
    let filtered_line_opex_total = filtered_line_rows
        .iter()
        .map(|row| row.estimated_opex_per_hour_base.max(0.0))
        .sum::<f64>();
    let mut filter_scale = 1.0_f64;
    if mode_filter.is_some() || line_filter.is_some() {
        filter_scale *= if all_line_opex_total > 0.0 {
            (filtered_line_opex_total / all_line_opex_total).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    if region_filter.is_some() {
        let selected_region_abs = region_breakdown
            .iter()
            .map(|row| row.revenue_base + row.opex_base + row.capex_base + row.penalties_base)
            .sum::<f64>();
        let global_abs = manifest.economy.cumulative_revenue_base.max(0.0)
            + manifest.economy.cumulative_opex_base.max(0.0)
            + manifest.economy.cumulative_capex_base.max(0.0)
            + manifest
                .economy
                .cumulative_lost_demand_penalty_base
                .max(0.0);
        filter_scale *= if global_abs > 0.0 {
            (selected_region_abs / global_abs).clamp(0.0, 1.0)
        } else if selected_region_abs > 0.0 {
            1.0
        } else {
            0.0
        };
    }
    if !filters_active {
        filter_scale = 1.0;
    }

    let mut monthly_points = financial_points_from_monthly(&manifest.economy.monthly_financials);
    if monthly_points.is_empty() {
        monthly_points.push(FinancialDashboardPoint {
            period_index: 0,
            label: "Now".to_string(),
            revenue_base: manifest.economy.cumulative_revenue_base.max(0.0),
            opex_base: manifest.economy.cumulative_opex_base.max(0.0),
            capex_base: manifest.economy.cumulative_capex_base.max(0.0),
            penalties_base: manifest
                .economy
                .cumulative_lost_demand_penalty_base
                .max(0.0),
            net_base: manifest.economy.cumulative_revenue_base.max(0.0)
                - manifest.economy.cumulative_opex_base.max(0.0)
                - manifest.economy.cumulative_capex_base.max(0.0)
                - manifest
                    .economy
                    .cumulative_lost_demand_penalty_base
                    .max(0.0),
        });
    }
    let points = financial_points_for_granularity(
        &monthly_points,
        granularity.as_str(),
        periods,
        filter_scale,
    );

    let (total_revenue_base, total_opex_base, total_capex_base, total_penalties_base) =
        if region_filter.is_some() && !region_breakdown.is_empty() {
            (
                region_breakdown
                    .iter()
                    .map(|row| row.revenue_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.opex_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.capex_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.penalties_base)
                    .sum::<f64>(),
            )
        } else {
            (
                manifest.economy.cumulative_revenue_base.max(0.0) * filter_scale,
                manifest.economy.cumulative_opex_base.max(0.0) * filter_scale,
                manifest.economy.cumulative_capex_base.max(0.0) * filter_scale,
                manifest
                    .economy
                    .cumulative_lost_demand_penalty_base
                    .max(0.0)
                    * filter_scale,
            )
        };
    let total_net_base =
        total_revenue_base - total_opex_base - total_capex_base - total_penalties_base;

    Ok(FinancialDashboardResponse {
        currency: manifest.economy.currency.clone(),
        granularity,
        periods: points.len(),
        current_balance_base: manifest.economy.current_balance_base * filter_scale.max(0.0),
        total_revenue_base,
        total_opex_base,
        total_capex_base,
        total_penalties_base,
        total_net_base,
        points,
        mode_breakdown,
        line_breakdown: filtered_line_rows,
        region_breakdown,
    })
}

#[command]
fn list_regions(app: AppHandle, project_path: String) -> Result<Vec<RegionStatus>, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    region_status_rows_for_manifest(&app, &manifest)
}

#[command]
fn unlock_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockResult, String> {
    let normalized_region =
        canonicalize_region_id(&region_id).ok_or_else(|| "invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!("no installed demand pack for country {iso}"));
    };
    let region = catalog
        .by_id
        .get(&normalized_region)
        .ok_or_else(|| format!("unknown region_id: {normalized_region}"))?;

    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let country_unlocked = unlocked
        .iter()
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .cloned()
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region)
        && !country_unlocked.is_empty()
        && !region
            .adjacent_region_ids
            .iter()
            .any(|rid| country_unlocked.contains(rid))
    {
        return Err("region must be adjacent to an already unlocked region".to_string());
    }

    let charge = if unlocked.contains(&normalized_region) {
        0.0
    } else {
        region_unlock_cost_base_for_manifest(&manifest, region)
    };
    if charge > 0.0 && manifest.economy.current_balance_base < charge {
        return Err(format!(
            "insufficient funds: need {:.0} base units, have {:.0}",
            charge, manifest.economy.current_balance_base
        ));
    }

    if charge > 0.0 {
        manifest.economy.current_balance_base -= charge;
        manifest.economy.cumulative_capex_base += charge;
        update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, charge);
        record_monthly_financial_delta(&mut manifest, 0.0, 0.0, charge, 0.0);
        bump_economy_revision(&mut manifest);
    }
    unlocked.insert(normalized_region.clone());
    manifest.region_state.unlocked_region_ids = unlocked.into_iter().collect();
    if manifest
        .region_state
        .primary_focus_region_id
        .as_deref()
        .and_then(canonicalize_region_id)
        .is_none()
    {
        manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    }

    let mut countries = unlocked_country_codes(&manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    sync_progress_budget_from_economy(&mut manifest);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;

    Ok(UnlockResult {
        region_id: normalized_region,
        charged_base: charge,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_regions: manifest.region_state.unlocked_region_ids.len(),
    })
}

#[command]
fn unlock_and_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockFocusResult, String> {
    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let normalized_region = canonicalize_region_id(&region_id)
        .ok_or_else(|| "InvalidRegionId: invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "InvalidRegionId: invalid region country code".to_string())?;
    let project_iso = primary_project_country_iso2(&manifest).unwrap_or_default();
    if !project_iso.is_empty() && !project_iso.eq_ignore_ascii_case(&iso) {
        return Err(format!(
            "WrongCountryScope: region belongs to {iso}, project country is {project_iso}"
        ));
    }
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!(
            "CountryPackMissing: no installed demand pack for country {iso}"
        ));
    };
    let region = catalog
        .by_id
        .get(&normalized_region)
        .ok_or_else(|| format!("UnknownRegion: unknown region_id: {normalized_region}"))?;

    let mut unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let country_unlocked = unlocked
        .iter()
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .cloned()
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region)
        && !country_unlocked.is_empty()
        && !region
            .adjacent_region_ids
            .iter()
            .any(|rid| country_unlocked.contains(rid))
    {
        return Err(
            "RegionNotAdjacent: region must be adjacent to an already unlocked region".to_string(),
        );
    }

    let charge = if unlocked.contains(&normalized_region) {
        0.0
    } else {
        region_unlock_cost_base_for_manifest(&manifest, region)
    };
    if charge > 0.0 && manifest.economy.current_balance_base < charge {
        return Err(format!(
            "InsufficientFunds: need {:.0} base units, have {:.0}",
            charge, manifest.economy.current_balance_base
        ));
    }

    if charge > 0.0 {
        manifest.economy.current_balance_base -= charge;
        manifest.economy.cumulative_capex_base += charge;
        update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, charge);
        record_monthly_financial_delta(&mut manifest, 0.0, 0.0, charge, 0.0);
        bump_economy_revision(&mut manifest);
    }
    unlocked.insert(normalized_region.clone());
    manifest.region_state.unlocked_region_ids = unlocked.iter().cloned().collect();
    manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    manifest.region_state.active_region_ids =
        default_active_regions_for_focus(&catalog, &normalized_region, &unlocked);

    let mut countries = unlocked_country_codes(&manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    sync_progress_budget_from_economy(&mut manifest);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;

    Ok(UnlockFocusResult {
        region_id: normalized_region.clone(),
        charged_base: charge,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_regions: manifest.region_state.unlocked_region_ids.len(),
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
    })
}

#[command]
fn set_primary_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<FocusResult, String> {
    let normalized_region =
        canonicalize_region_id(&region_id).ok_or_else(|| "invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!("no installed demand pack for country {iso}"));
    };
    if !catalog.by_id.contains_key(&normalized_region) {
        return Err(format!("unknown region_id: {normalized_region}"));
    }

    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region) {
        return Err("region must be unlocked before setting focus".to_string());
    }
    manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    manifest.region_state.active_region_ids =
        default_active_regions_for_focus(&catalog, &normalized_region, &unlocked);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;
    Ok(FocusResult {
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
    })
}

#[command]
fn set_simulation_scope(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    scope: SimulationScopeUpdate,
) -> Result<ScopeState, String> {
    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    if let Some(max_active) = scope.max_active_zones {
        manifest.simulation_scope.max_active_zones = max_active.clamp(120, 5000);
    }
    if let Some(mode) = scope.remote_regions_mode {
        manifest.simulation_scope.remote_regions_mode = normalize_scope(&mode);
    }
    if let Some(interval) = scope.remote_update_interval_ticks {
        manifest.simulation_scope.remote_update_interval_ticks = interval.max(1);
    }
    if let Some(v) = scope.focus_max_active_zones {
        manifest.simulation_scope.focus_max_active_zones = v.clamp(120, 6000);
    }
    if let Some(v) = scope.adjacent_max_active_zones {
        manifest.simulation_scope.adjacent_max_active_zones =
            v.clamp(40, manifest.simulation_scope.focus_max_active_zones);
    }
    if let Some(v) = scope.remote_max_active_zones {
        manifest.simulation_scope.remote_max_active_zones =
            v.clamp(20, manifest.simulation_scope.adjacent_max_active_zones);
    }
    if let Some(v) = scope.adjacent_update_interval_ticks {
        manifest.simulation_scope.adjacent_update_interval_ticks = v.max(1);
    }
    if let Some(active_ids) = scope.active_region_ids {
        manifest.region_state.active_region_ids = active_ids
            .into_iter()
            .filter_map(|id| canonicalize_region_id(&id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let project_path_string = project_root.to_string_lossy().to_string();
    let _ = enqueue_runtime_action_with_retry(
        state.inner(),
        &project_path_string,
        RuntimeAction::InvalidateMaterialization,
    )?;
    let _ = open_session_internal(&app, &state, &project_root)?;
    Ok(ScopeState {
        max_active_zones: manifest.simulation_scope.max_active_zones,
        remote_regions_mode: manifest.simulation_scope.remote_regions_mode.clone(),
        remote_update_interval_ticks: manifest.simulation_scope.remote_update_interval_ticks,
        focus_max_active_zones: manifest.simulation_scope.focus_max_active_zones,
        adjacent_max_active_zones: manifest.simulation_scope.adjacent_max_active_zones,
        remote_max_active_zones: manifest.simulation_scope.remote_max_active_zones,
        adjacent_update_interval_ticks: manifest.simulation_scope.adjacent_update_interval_ticks,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
    })
}

#[command]
fn get_demand_tile_source(
    project_path: String,
    layer: String,
) -> Result<DemandTileSourceMeta, String> {
    let project_root = PathBuf::from(project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let countries = doc
        .scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.loaded_countries.clone())
        .unwrap_or_default();
    Ok(DemandTileSourceMeta {
        layer: layer.clone(),
        source: "scenario.demand_cells".to_string(),
        countries_loaded: countries,
        cells: doc.scenario.world.demand_cells.len(),
        mode: if layer.eq_ignore_ascii_case("population") || layer.eq_ignore_ascii_case("jobs") {
            "smoothed_raster_overlay".to_string()
        } else {
            "unknown".to_string()
        },
    })
}

#[command]
fn get_demand_layer_stats(project_path: String) -> Result<DemandLayerStats, String> {
    let project_root = PathBuf::from(project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;

    let mut residents = Vec::<f64>::new();
    let mut jobs = Vec::<f64>::new();
    let mut activity = Vec::<f64>::new();
    if !doc.scenario.world.demand_cells.is_empty() {
        for c in &doc.scenario.world.demand_cells {
            residents.push(c.residents_night.max(0.0));
            jobs.push(c.jobs_day.max(0.0));
            activity.push(
                c.activity_mix_residential
                    .max(c.activity_mix_office)
                    .max(c.activity_mix_retail)
                    .max(c.activity_mix_recreation)
                    .max(c.activity_mix_industrial)
                    .max(c.activity_mix_education)
                    .max(c.activity_mix_health),
            );
        }
    } else {
        for z in &doc.scenario.world.zones {
            residents.push(z.population.max(0.0));
            jobs.push(z.jobs.max(0.0));
            let denom = (z.population + z.jobs).max(1e-6);
            activity.push((z.population / denom).max(z.jobs / denom));
        }
    }

    residents.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    jobs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    activity.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(DemandLayerStats {
        cells: residents.len(),
        residents_min: residents.first().copied().unwrap_or(0.0),
        residents_p50: percentile(&residents, 0.5),
        residents_max: residents.last().copied().unwrap_or(0.0),
        jobs_min: jobs.first().copied().unwrap_or(0.0),
        jobs_p50: percentile(&jobs, 0.5),
        jobs_max: jobs.last().copied().unwrap_or(0.0),
        activity_min: activity.first().copied().unwrap_or(0.0),
        activity_p50: percentile(&activity, 0.5),
        activity_max: activity.last().copied().unwrap_or(0.0),
    })
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
