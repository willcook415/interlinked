use crate::*;

use super::economy::EconomyManifest;
use super::planning::RunMeta;
use super::runtime::RuntimeSchedulingManifest;

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
    pub sim_tick_seconds: f64,
    pub start_country: Option<String>,
    pub start_city: Option<String>,
    pub unlocked_countries: usize,
    pub network_stops: usize,
    pub network_links: usize,
    pub network_services: usize,
    pub total_link_km: f64,
    pub peak_ridership_pph: Option<f64>,
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
