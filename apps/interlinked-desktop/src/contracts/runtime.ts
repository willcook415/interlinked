import type { SimulationClock, SimulationSpeed } from "./session";

export type SimulationAdvanceEconomy = {
  current_balance_base: number;
  cumulative_revenue_base: number;
  cumulative_opex_base: number;
  budget_display: number;
};

export type SimulationAdvanceResult = {
  frame: HistoryFrameLite;
  clock: SimulationClock;
  economy: SimulationAdvanceEconomy;
  delta_revenue_base: number;
  delta_opex_base: number;
  delta_net_base: number;
};

export type RuntimePerfTelemetry = {
  tick_index: number;
  dt_s: number;
  fixed_step_s?: number;
  stage_prepare_ms: number;
  stage_step_ms: number;
  stage_economy_ms: number;
  stage_runtime_ops_ms?: number;
  tick_total_ms: number;
  snapshot_publish_ms?: number;
  fast_snapshot_bytes?: number;
  strategic_snapshot_bytes?: number;
  queue_depth: number;
  snapshot_age_ms: number;
  dropped_steps: number;
  executed_steps_this_cycle?: number;
  max_steps_per_cycle?: number;
  backlog_steps?: number;
  backlog_s?: number;
  accumulator_s?: number;
  cycle_elapsed_ms?: number;
  avg_cycle_elapsed_ms?: number;
  avg_sim_step_ms?: number;
  real_elapsed_s?: number;
  game_elapsed_s?: number;
  target_game_elapsed_s?: number;
  target_speed_ratio?: number;
  achieved_speed_ratio?: number;
  achieved_vs_target_ratio?: number;
  under_sustained_speed?: boolean;
  adaptive_max_active_zones: number;
  strategic_refresh_due?: boolean;
  strategic_refresh_interval_ticks?: number;
  runtime_views_materialized?: boolean;
};

export type TrainRuntimeView = {
  train_id: string;
  service_id: string;
  line_id: string;
  line_name: string;
  vehicle_ordinal: number;
  direction_label: string;
  destination_stop_id: string;
  destination_label: string;
  mode: string;
  mode_variant?: string | null;
  stock_tier_id?: string | null;
  vehicle_capacity: number;
  onboard_pax: number;
  x: number;
  y: number;
  at_stop_id?: string | null;
  in_motion: boolean;
  provenance: string;
};

export type StationRuntimeView = {
  stop_id: string;
  current_inside_pax: number;
  capacity_pax: number;
  declined_last_hour: number;
  entries_per_hour: number;
  exits_per_hour: number;
  avg_wait_to_board_s: number;
  provenance: string;
};

export type LineOpsRuntimeView = {
  line_id: string;
  active_trains: number;
  boardings_attempted_per_hour?: number;
  boarded_per_hour: number;
  alighted_per_hour: number;
  denied_boardings_per_hour: number;
  queue_end_pax?: number;
  mean_wait_s: number;
  provenance: string;
};

export type RuntimeSnapshot = {
  project_path: string;
  clock_revision: number;
  clock: SimulationClock;
  economy: SimulationAdvanceEconomy;
  frame?: HistoryFrameLite | null;
  delta_revenue_base: number;
  delta_opex_base: number;
  delta_net_base: number;
  captured_at_epoch_ms: number;
  telemetry: RuntimePerfTelemetry;
  trains?: TrainRuntimeView[];
  stations?: StationRuntimeView[];
  line_ops?: LineOpsRuntimeView[];
  provenance_warnings?: string[];
  trains_authoritative?: boolean;
};

export type RuntimeFastSnapshot = {
  project_path: string;
  clock_revision: number;
  clock: SimulationClock;
  captured_at_epoch_ms: number;
  telemetry: RuntimePerfTelemetry;
  trains?: TrainRuntimeView[];
  stations?: StationRuntimeView[];
  line_ops?: LineOpsRuntimeView[];
  provenance_warnings?: string[];
  trains_authoritative?: boolean;
};

export type RuntimeStrategicSnapshot = {
  project_path: string;
  clock_revision: number;
  clock: SimulationClock;
  economy: SimulationAdvanceEconomy;
  frame?: HistoryFrameLite | null;
  delta_revenue_base: number;
  delta_opex_base: number;
  delta_net_base: number;
  captured_at_epoch_ms: number;
  telemetry: RuntimePerfTelemetry;
  provenance_warnings?: string[];
  trains_authoritative?: boolean;
};

export type RuntimeLoopStatus = {
  project_path: string;
  running: boolean;
  speed?: SimulationSpeed;
  clock_revision?: number;
  queue_depth: number;
  enabled: boolean;
};

export type RuntimeTemporalDiagnostics = {
  last_fast_snapshot_interval_ms: number | null;
  stale_fast_snapshots_rejected: number;
  latest_fast_clock_revision: number;
  latest_fast_tick_index: number;
};

export type HistoryFrameLite = {
  t_s?: number;
  queue_summary?: { total_queue?: number; max_queue?: number };
  kpis?: { share_trips_served?: number; mean_wait_time_s?: number };
  service_loads?: Array<{ service_id: string; load_to_capacity: number }>;
};
