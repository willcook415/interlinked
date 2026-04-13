use super::{Link, Service, Stop, Transfer, TransferRule};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    // Persisted zone substrate. Planner may override this at runtime by deriving
    // effective zones from demand_cells when demand_cells are populated.
    #[serde(default)]
    pub zones: Vec<Zone>,

    #[serde(default)]
    pub stops: Vec<Stop>,

    #[serde(default)]
    pub links: Vec<Link>,

    #[serde(default)]
    pub services: Vec<Service>,

    #[serde(default)]
    pub transfers: Vec<Transfer>,

    #[serde(default)]
    pub transfer_rules: Option<Vec<TransferRule>>, // NEW: mode-aware rules

    // Persisted materialized gameplay demand substrate (country-surface derived in game mode).
    #[serde(default)]
    pub demand_cells: Vec<DemandCell>,

    // Persisted provenance for demand_cells/zones materialization source and loaded countries.
    #[serde(default)]
    pub demand_meta: Option<DemandMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub population: f64,
    pub jobs: f64,
    #[serde(default)]
    pub country_iso2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandCell {
    pub cell_id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_demand_cell_area_m2")]
    pub area_m2: f64,
    #[serde(default = "default_demand_cell_residents")]
    pub residents_night: f64,
    #[serde(default = "default_demand_cell_jobs")]
    pub jobs_day: f64,
    #[serde(default = "default_mix_residential")]
    pub activity_mix_residential: f64,
    #[serde(default = "default_mix_office")]
    pub activity_mix_office: f64,
    #[serde(default = "default_mix_retail")]
    pub activity_mix_retail: f64,
    #[serde(default = "default_mix_recreation")]
    pub activity_mix_recreation: f64,
    #[serde(default = "default_mix_industrial")]
    pub activity_mix_industrial: f64,
    #[serde(default = "default_mix_education")]
    pub activity_mix_education: f64,
    #[serde(default = "default_mix_health")]
    pub activity_mix_health: f64,
    #[serde(default = "default_demand_cell_centrality")]
    pub centrality_score: f64,
    #[serde(default = "default_demand_cell_quality")]
    pub data_quality_score: f64,
    #[serde(default)]
    pub country_iso2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandMeta {
    pub surface_version: String,
    #[serde(default)]
    pub loaded_countries: Vec<String>,
    #[serde(default)]
    pub source: String,
}

fn default_demand_cell_area_m2() -> f64 {
    0.0
}

fn default_demand_cell_residents() -> f64 {
    0.0
}

fn default_demand_cell_jobs() -> f64 {
    0.0
}

fn default_mix_residential() -> f64 {
    0.45
}

fn default_mix_office() -> f64 {
    0.18
}

fn default_mix_retail() -> f64 {
    0.12
}

fn default_mix_recreation() -> f64 {
    0.08
}

fn default_mix_industrial() -> f64 {
    0.10
}

fn default_mix_education() -> f64 {
    0.04
}

fn default_mix_health() -> f64 {
    0.03
}

fn default_demand_cell_centrality() -> f64 {
    0.0
}

fn default_demand_cell_quality() -> f64 {
    0.5
}

#[cfg(test)]
mod tests {
    use super::DemandCell;

    #[test]
    fn demand_cell_deserializes_with_missing_mix_fields() {
        let cell: DemandCell = serde_json::from_str(
            r#"{
                "cell_id":"legacy-cell",
                "x":10.0,
                "y":20.0,
                "residents_night":120.0,
                "jobs_day":45.0
            }"#,
        )
        .expect("legacy demand cell should deserialize");
        assert!((cell.activity_mix_residential - 0.45).abs() < 1e-9);
        assert!((cell.activity_mix_office - 0.18).abs() < 1e-9);
        assert!((cell.activity_mix_retail - 0.12).abs() < 1e-9);
        assert!((cell.activity_mix_recreation - 0.08).abs() < 1e-9);
        assert!((cell.activity_mix_industrial - 0.10).abs() < 1e-9);
        assert!((cell.activity_mix_education - 0.04).abs() < 1e-9);
        assert!((cell.activity_mix_health - 0.03).abs() < 1e-9);
    }
}
