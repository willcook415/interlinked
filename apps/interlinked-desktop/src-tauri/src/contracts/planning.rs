use crate::*;

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
