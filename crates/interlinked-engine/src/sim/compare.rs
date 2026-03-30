use super::types::SimulationOutput;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationDelta {
    pub meta: DeltaMeta,
    pub kpis: KpiDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaMeta {
    pub base_scenario: String,
    pub compare_scenario: String,
    pub results_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KpiDelta {
    pub total_trips: f64,
    pub mean_generalized_cost_s: f64,
    pub mean_in_vehicle_time_s: f64,
    pub mean_wait_time_s: f64,
    pub mean_walk_time_s: f64,
    pub mean_transfer_time_s: f64,
    pub mean_transfer_penalty_s: f64,
    pub mean_transfers: f64,
    pub mean_boardings: f64,
}

pub fn compare_outputs(base: &SimulationOutput, other: &SimulationOutput) -> SimulationDelta {
    SimulationDelta {
        meta: DeltaMeta {
            base_scenario: base.meta.scenario_name.clone(),
            compare_scenario: other.meta.scenario_name.clone(),
            results_version: base.meta.results_version.clone(),
        },
        kpis: KpiDelta {
            total_trips: other.kpis.total_trips - base.kpis.total_trips,
            mean_generalized_cost_s: other.kpis.mean_generalized_cost_s
                - base.kpis.mean_generalized_cost_s,
            mean_in_vehicle_time_s: other.kpis.mean_in_vehicle_time_s
                - base.kpis.mean_in_vehicle_time_s,
            mean_wait_time_s: other.kpis.mean_wait_time_s - base.kpis.mean_wait_time_s,
            mean_walk_time_s: other.kpis.mean_walk_time_s - base.kpis.mean_walk_time_s,
            mean_transfer_time_s: other.kpis.mean_transfer_time_s - base.kpis.mean_transfer_time_s,
            mean_transfer_penalty_s: other.kpis.mean_transfer_penalty_s
                - base.kpis.mean_transfer_penalty_s,
            mean_transfers: other.kpis.mean_transfers - base.kpis.mean_transfers,
            mean_boardings: other.kpis.mean_boardings - base.kpis.mean_boardings,
        },
    }
}
