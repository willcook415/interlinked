use crate::*;

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
    pub current_balance_base: f64,
    pub unlocked_region_ids: Vec<String>,
    pub unlocked_countries: Vec<String>,
    pub scenario: ScenarioDocumentLite,
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
    pub unlocked_region_ids: Vec<String>,
    pub unlocked_countries: Vec<String>,
    pub scenario: ScenarioDocumentLite,
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
