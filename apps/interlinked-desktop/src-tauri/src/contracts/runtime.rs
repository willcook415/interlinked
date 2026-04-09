use crate::*;

use super::session::SimulationClock;

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
