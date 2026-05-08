use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionStatus {
    pub region_id: String,
    pub country_iso2: String,
    pub region_kind: String,
    pub region_token: String,
    #[serde(default)]
    pub h3_cell_id: Option<String>,
    pub name: String,
    pub admin_level: String,
    pub nation: Option<String>,
    pub source_code: Option<String>,
    pub adjacency_source: String,
    pub geometry_source: String,
    pub unlocked: bool,
    pub active: bool,
    pub adjacent_region_ids: Vec<String>,
    pub unlock_cost_base: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub population: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<u64>,
    pub residents_smooth: f64,
    pub jobs_smooth: f64,
    #[serde(default)]
    pub employment_estimate: f64,
    pub cells_res8: usize,
    pub geometry: Option<JsonValue>,
    /// Backend-authoritative hex number from substrate hex numbering.
    /// This is the canonical number that `manual_regions.json` `hex_numbers` refer to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_hex_number: Option<usize>,
    /// Parallel array to `geometry` polygons. If `geometry` is a MultiPolygon,
    /// this array contains the canonical hex number for each polygon in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constituent_hex_numbers: Vec<usize>,
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
    // Compatibility-only legacy fields for non-vector fallback tiers.
    // Active UK render path uses style_url + world_context_url as authority.
    #[serde(default)]
    pub counties_url: Option<String>,
    pub major_roads_url: Option<String>,
    pub county_basemap_mid_url_template: Option<String>,
    pub county_basemap_full_url_template: Option<String>,
    pub county_roads_url_template: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemandOverlayRegionDatum {
    pub region_id: String,
    pub region_name: String,
    pub lon: f64,
    pub lat: f64,
    pub intensity_score: f64,
    pub service_gap_score: f64,
    #[serde(default)]
    pub service_gap_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemandOverlayCorridorDatum {
    pub origin_region_id: String,
    pub destination_region_id: String,
    pub origin_lon: f64,
    pub origin_lat: f64,
    pub destination_lon: f64,
    pub destination_lat: f64,
    pub corridor_score: f64,
    pub latent_passengers: f64,
    pub realised_passengers: f64,
    pub unserved_passengers: f64,
    #[serde(default)]
    pub is_underserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemandOverlayCellDatum {
    pub cell_id: String,
    #[serde(default)]
    pub planning_region_id: Option<String>,
    pub lon: f64,
    pub lat: f64,
    #[serde(default)]
    pub area_m2: f64,
    #[serde(default)]
    pub residents_night: f64,
    #[serde(default)]
    pub jobs_day: f64,
    #[serde(default)]
    pub centrality_score: f64,
    #[serde(default)]
    pub data_quality_score: f64,
    #[serde(default)]
    pub activity_mix_residential: f64,
    #[serde(default)]
    pub activity_mix_office: f64,
    #[serde(default)]
    pub activity_mix_retail: f64,
    #[serde(default)]
    pub activity_mix_recreation: f64,
    #[serde(default)]
    pub activity_mix_industrial: f64,
    #[serde(default)]
    pub activity_mix_education: f64,
    #[serde(default)]
    pub activity_mix_health: f64,
    #[serde(default)]
    pub raw_weight_residential: f64,
    #[serde(default)]
    pub raw_weight_employment: f64,
    #[serde(default)]
    pub allocated_residential_mass: f64,
    #[serde(default)]
    pub allocated_employment_mass: f64,
    #[serde(default)]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemandOverlayPayload {
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub intensity_available: bool,
    #[serde(default)]
    pub intensity_reason: Option<String>,
    #[serde(default)]
    pub service_gap_available: bool,
    #[serde(default)]
    pub service_gap_reason: Option<String>,
    #[serde(default)]
    pub corridor_desire_available: bool,
    #[serde(default)]
    pub corridor_desire_reason: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub cell_data_total: usize,
    #[serde(default)]
    pub cell_data_mappable: usize,
    #[serde(default)]
    pub cell_fallback_count: usize,
    #[serde(default)]
    pub cell_data: Vec<DemandOverlayCellDatum>,
    #[serde(default)]
    pub region_data: Vec<DemandOverlayRegionDatum>,
    #[serde(default)]
    pub corridor_data: Vec<DemandOverlayCorridorDatum>,
}
