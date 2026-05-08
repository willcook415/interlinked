use crate::*;

use super::session::SimulationClock;

/// Passenger/fare/counter provenance contract.
///
/// authoritative_sim: closest current simulation truth.
/// strategic_estimate: planner/model forecast or periodic strategic output.
/// runtime_projection: live approximation/projection for operational feel.
/// animation_only: visual-only value.
/// debug_legacy: diagnostic or old-system counter not safe for player-facing truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterProvenance {
    AuthoritativeSim,
    StrategicEstimate,
    #[serde(alias = "derived_calibrated")]
    RuntimeProjection,
    AnimationOnly,
    DebugLegacy,
}

impl Default for CounterProvenance {
    fn default() -> Self {
        CounterProvenance::DebugLegacy
    }
}

pub fn default_counter_provenance_strategic_estimate() -> CounterProvenance {
    CounterProvenance::StrategicEstimate
}

pub fn default_counter_provenance_runtime_projection() -> CounterProvenance {
    CounterProvenance::RuntimeProjection
}

pub fn default_counter_provenance_animation_only() -> CounterProvenance {
    CounterProvenance::AnimationOnly
}

pub fn default_counter_provenance_debug_legacy() -> CounterProvenance {
    CounterProvenance::DebugLegacy
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
    #[serde(default)]
    pub lifecycle_diagnostics: Option<RuntimeLifecycleDiagnostics>,
    #[serde(default)]
    pub fare_source_diagnostic: Option<RuntimeFareSourceTelemetry>,
}

const RUNTIME_LIFECYCLE_DIAGNOSTIC_EPS: f64 = 1e-6;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeLifecycleDiagnostics {
    #[serde(default)]
    pub source_label: String,
    #[serde(default)]
    pub queue_start_pax: f64,
    #[serde(default)]
    pub new_waiting_pax: f64,
    #[serde(default)]
    pub boarded_pax: f64,
    #[serde(default)]
    pub queue_overflow_dropped_pax: f64,
    #[serde(default)]
    pub queue_end_pax: f64,
    #[serde(default)]
    pub queue_balance_error: f64,
    #[serde(default)]
    pub onboard_start_pax: f64,
    #[serde(default)]
    pub onboard_end_pax: f64,
    #[serde(default)]
    pub alighted_pax: f64,
    #[serde(default)]
    pub onboard_balance_error: f64,
    #[serde(default)]
    pub fare_recognized_pax: f64,
    #[serde(default)]
    pub fare_recognized_base: f64,
    #[serde(default)]
    pub missing_fare_basis_pax: f64,
    #[serde(default)]
    pub has_queue_conservation_error: bool,
    #[serde(default)]
    pub has_onboard_conservation_error: bool,
    #[serde(default)]
    pub has_missing_fare_basis: bool,
}

impl RuntimeLifecycleDiagnostics {
    pub fn from_engine_summary(
        summary: &interlinked_engine::sim::LifecycleConservationSummary,
    ) -> Self {
        // Developer/runtime observability only: this covers fast-kernel
        // aggregate queue/onboard/fare conservation, not full strategic
        // demand-model conservation or desktop projection/animation state.
        Self {
            source_label: summary.source_label.clone(),
            queue_start_pax: summary.queue_start_pax.max(0.0),
            new_waiting_pax: summary.new_waiting_pax.max(0.0),
            boarded_pax: summary.boarded_pax.max(0.0),
            queue_overflow_dropped_pax: summary.queue_overflow_dropped_pax.max(0.0),
            queue_end_pax: summary.queue_end_pax.max(0.0),
            queue_balance_error: summary.queue_balance_error,
            onboard_start_pax: summary.onboard_start_pax.max(0.0),
            onboard_end_pax: summary.onboard_end_pax.max(0.0),
            alighted_pax: summary.alighted_pax.max(0.0),
            onboard_balance_error: summary.onboard_balance_error,
            fare_recognized_pax: summary.fare_recognized_pax.max(0.0),
            fare_recognized_base: summary.fare_recognized_base.max(0.0),
            missing_fare_basis_pax: summary.missing_fare_basis_pax.max(0.0),
            has_queue_conservation_error: summary.queue_balance_error.abs()
                > RUNTIME_LIFECYCLE_DIAGNOSTIC_EPS,
            has_onboard_conservation_error: summary.onboard_balance_error.abs()
                > RUNTIME_LIFECYCLE_DIAGNOSTIC_EPS,
            has_missing_fare_basis: summary.missing_fare_basis_pax
                > RUNTIME_LIFECYCLE_DIAGNOSTIC_EPS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeFareSourceTelemetry {
    pub selected_provenance: CounterProvenance,
    pub selected_source_label: String,
    #[serde(default)]
    pub selected_fare_delta_base: f64,
    #[serde(default)]
    pub selected_passenger_count: f64,
    #[serde(default)]
    pub used_authoritative_fare: bool,
    #[serde(default)]
    pub used_strategic_fare_fallback: bool,
    #[serde(default)]
    pub used_runtime_projection_fare_fallback: bool,
    #[serde(default)]
    pub fallback_used: bool,
}

impl Default for RuntimeFareSourceTelemetry {
    fn default() -> Self {
        Self {
            selected_provenance: CounterProvenance::DebugLegacy,
            selected_source_label: String::new(),
            selected_fare_delta_base: 0.0,
            selected_passenger_count: 0.0,
            used_authoritative_fare: false,
            used_strategic_fare_fallback: false,
            used_runtime_projection_fare_fallback: false,
            fallback_used: false,
        }
    }
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
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub passenger_counter_provenance: CounterProvenance,
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
    #[serde(default = "default_counter_provenance_strategic_estimate")]
    pub passenger_counter_provenance: CounterProvenance,
    #[serde(default = "default_counter_provenance_strategic_estimate")]
    pub fare_counter_provenance: CounterProvenance,
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
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub passenger_counter_provenance: CounterProvenance,
    #[serde(default = "default_counter_provenance_strategic_estimate")]
    pub fare_counter_provenance: CounterProvenance,
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
    #[serde(default = "default_counter_provenance_animation_only")]
    pub provenance: CounterProvenance,
    #[serde(default = "default_counter_provenance_animation_only")]
    pub passenger_counter_provenance: CounterProvenance,
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
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub provenance: CounterProvenance,
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub passenger_counter_provenance: CounterProvenance,
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
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub provenance: CounterProvenance,
    #[serde(default = "default_counter_provenance_runtime_projection")]
    pub passenger_counter_provenance: CounterProvenance,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_provenance_serializes_as_stable_snake_case() {
        assert_eq!(
            serde_json::to_string(&CounterProvenance::AuthoritativeSim).unwrap(),
            "\"authoritative_sim\""
        );
        assert_eq!(
            serde_json::to_string(&CounterProvenance::StrategicEstimate).unwrap(),
            "\"strategic_estimate\""
        );
        assert_eq!(
            serde_json::to_string(&CounterProvenance::RuntimeProjection).unwrap(),
            "\"runtime_projection\""
        );
        assert_eq!(
            serde_json::to_string(&CounterProvenance::AnimationOnly).unwrap(),
            "\"animation_only\""
        );
        assert_eq!(
            serde_json::to_string(&CounterProvenance::DebugLegacy).unwrap(),
            "\"debug_legacy\""
        );
    }

    #[test]
    fn desktop_runtime_view_counters_are_not_marked_authoritative_by_default() {
        let train = TrainRuntimeView {
            train_id: "train_1".to_string(),
            service_id: "svc_1".to_string(),
            line_id: "line_1".to_string(),
            line_name: "Line 1".to_string(),
            vehicle_ordinal: 1,
            direction_label: "Outbound".to_string(),
            destination_stop_id: "B".to_string(),
            destination_label: "To B".to_string(),
            mode: "metro".to_string(),
            mode_variant: None,
            stock_tier_id: None,
            vehicle_capacity: 100.0,
            onboard_pax: 10.0,
            x: 0.0,
            y: 0.0,
            at_stop_id: None,
            in_motion: true,
            provenance: CounterProvenance::AnimationOnly,
            passenger_counter_provenance: CounterProvenance::AnimationOnly,
        };
        let station = StationRuntimeView {
            stop_id: "A".to_string(),
            current_inside_pax: 5.0,
            capacity_pax: 100.0,
            declined_last_hour: 0.0,
            entries_per_hour: 12.0,
            exits_per_hour: 8.0,
            avg_wait_to_board_s: 90.0,
            provenance: CounterProvenance::RuntimeProjection,
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
        };
        let line = LineOpsRuntimeView {
            line_id: "line_1".to_string(),
            active_trains: 1,
            boardings_attempted_per_hour: 12.0,
            boarded_per_hour: 10.0,
            alighted_per_hour: 9.0,
            denied_boardings_per_hour: 2.0,
            queue_end_pax: 5.0,
            mean_wait_s: 90.0,
            provenance: CounterProvenance::RuntimeProjection,
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
        };

        assert_ne!(
            train.passenger_counter_provenance,
            CounterProvenance::AuthoritativeSim
        );
        assert_ne!(
            station.passenger_counter_provenance,
            CounterProvenance::AuthoritativeSim
        );
        assert_ne!(
            line.passenger_counter_provenance,
            CounterProvenance::AuthoritativeSim
        );
    }

    #[test]
    fn runtime_lifecycle_diagnostics_preserve_engine_summary_and_flags() {
        let summary = interlinked_engine::sim::LifecycleConservationSummary {
            source_label: "engine_fast_lifecycle_conservation".to_string(),
            queue_start_pax: 10.0,
            new_waiting_pax: 5.0,
            boarded_pax: 4.0,
            queue_overflow_dropped_pax: 0.0,
            queue_end_pax: 11.25,
            queue_balance_error: -0.25,
            onboard_start_pax: 3.0,
            onboard_end_pax: 4.0,
            alighted_pax: 2.0,
            onboard_balance_error: 1.0,
            fare_recognized_pax: 2.0,
            fare_recognized_base: 5.0,
            missing_fare_basis_pax: 1.0,
        };

        let diagnostics = RuntimeLifecycleDiagnostics::from_engine_summary(&summary);

        assert_eq!(
            diagnostics.source_label,
            "engine_fast_lifecycle_conservation"
        );
        assert_eq!(diagnostics.queue_start_pax, 10.0);
        assert_eq!(diagnostics.fare_recognized_base, 5.0);
        assert!(diagnostics.has_queue_conservation_error);
        assert!(diagnostics.has_onboard_conservation_error);
        assert!(diagnostics.has_missing_fare_basis);
    }

    #[test]
    fn runtime_lifecycle_diagnostics_do_not_flag_balanced_summary() {
        let summary = interlinked_engine::sim::LifecycleConservationSummary {
            source_label: "engine_fast_lifecycle_conservation".to_string(),
            queue_start_pax: 10.0,
            new_waiting_pax: 5.0,
            boarded_pax: 4.0,
            queue_overflow_dropped_pax: 0.0,
            queue_end_pax: 11.0,
            queue_balance_error: 0.0,
            onboard_start_pax: 3.0,
            onboard_end_pax: 5.0,
            alighted_pax: 2.0,
            onboard_balance_error: 0.0,
            fare_recognized_pax: 2.0,
            fare_recognized_base: 5.0,
            missing_fare_basis_pax: 0.0,
        };

        let diagnostics = RuntimeLifecycleDiagnostics::from_engine_summary(&summary);

        assert!(!diagnostics.has_queue_conservation_error);
        assert!(!diagnostics.has_onboard_conservation_error);
        assert!(!diagnostics.has_missing_fare_basis);
    }
}
