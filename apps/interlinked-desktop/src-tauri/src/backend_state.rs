use crate::*;

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

pub struct AppState {
    pub game: Mutex<Option<interlinked_engine::platform::GameState>>,
    pub current_project: Mutex<Option<String>>,
    pub(crate) runtime_tick: Mutex<Option<RuntimeTick>>,
    pub(crate) runtime_loop: Mutex<Option<RuntimeLoopHandle>>,
    pub runtime_snapshots: Mutex<VecDeque<RuntimeSnapshot>>,
    pub runtime_fast_snapshots: Mutex<VecDeque<RuntimeFastSnapshot>>,
    pub runtime_strategic_snapshots: Mutex<VecDeque<RuntimeStrategicSnapshot>>,
    pub(crate) runtime_materialization: Mutex<Option<RuntimeMaterializationState>>,
    pub(crate) runtime_ops: Mutex<Option<RuntimeOpsState>>,
}
