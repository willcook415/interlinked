use crate::*;

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
