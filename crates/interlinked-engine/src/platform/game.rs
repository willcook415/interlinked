use super::kernels::{KernelPartitionState, KernelPerfMetrics, StrategicRefreshReason};
use super::{NetworkEdit, ScenarioStore};
use crate::sim::SimulationOutput;

#[derive(Debug, Clone)]
pub struct GameState {
    pub tick_s: f64,
    pub store: ScenarioStore,
    pub sim_state: crate::sim::SimState,
    pub run_cfg: crate::sim::RunConfig,
    pub last_quick_kpis: Option<crate::sim::Kpis>,
    pub last_output: Option<SimulationOutput>,
    pub history: crate::sim::SimHistory,
    pub kernel_state: KernelPartitionState,
}

#[derive(Debug, Clone, Default)]
pub struct GameStepRequest {
    pub edits: Vec<NetworkEdit>,
    pub recompute_quick_kpis: bool,
    pub force_strategic_refresh: bool,
}

#[derive(Debug, Clone)]
pub struct GameStepOutput {
    pub tick_s: f64,
    pub quick_kpis: Option<crate::sim::Kpis>,
    pub strategic_refresh_executed: bool,
    pub strategic_refresh_reason: Option<StrategicRefreshReason>,
    pub kernel_perf: KernelPerfMetrics,
}
