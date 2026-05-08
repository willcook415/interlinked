import type {
  ProjectManifest,
  PurchaseOrderLite,
  ScenarioDocumentLite,
} from "./session";
import type { CounterProvenance } from "./runtime";

export type FleetDeliveryExpediteResult = {
  line_id: string;
  order_id: string;
  delivered_units: number;
  remaining_order_units: number;
  expedite_cost_base: number;
  balance_after_base: number;
};

export type RollingStockTierPreset = {
  id: string;
  label: string;
  purchase_cost_multiplier: number;
  capacity_multiplier: number;
  speed_multiplier: number;
  ui_badge: string;
};

export type SpeedLevelPreset = {
  id: string;
  label: string;
  speed_multiplier: number;
  cost_multiplier: number;
};

export type ComfortLevelPreset = {
  id: string;
  label: string;
  demand_multiplier: number;
  cost_multiplier: number;
};

export type ModeBuildPreset = {
  id: string;
  label: string;
  engine_mode: string;
  mode_variant?: string | null;
  default_color: string;
  default_speed_mps: number;
  default_headway_s: number;
  default_dwell_s: number;
  default_vehicle_capacity: number;
  default_stop_type: string;
  base_unit_purchase_cost_base: number;
  tph_min: number;
  tph_max: number;
  tph_step: number;
  salvage_rate: number;
  transfer_fee_per_unit_base: number;
  tiers: RollingStockTierPreset[];
  package_options: RollingStockTierPreset[];
  supports_carriages: boolean;
  cars_min: number;
  cars_max: number;
  cars_default: number;
  speed_levels: SpeedLevelPreset[];
  comfort_levels: ComfortLevelPreset[];
  staff_cost_per_unit_hour_base: number;
  staff_shift_multiplier_peak: number;
  staff_shift_multiplier_overnight: number;
  capex_per_km_base: number;
};

export type BuildDefaults = {
  station_capex_base: number;
  default_interchange_walk_time_s: number;
  presets: ModeBuildPreset[];
};

export type NetworkMutationSummary = {
  // Legacy fields.
  capex_delta_base: number;
  infra_capex_delta_base: number;
  fleet_purchase_base: number;
  fleet_upgrade_base: number;
  fleet_transfer_fees_base: number;
  fleet_salvage_refund_base: number;
  net_capex_delta_base: number;
  // User-facing cost lens fields.
  construction_cost_delta_base: number;
  fleet_purchase_delta_base: number;
  fleet_configuration_delta_base: number;
  apply_total_delta_base: number;
  projected_balance_after_apply_base?: number | null;
  projected_opex_per_hour_base: number;
  projected_staff_opex_per_hour_base: number;
  estimated_total_capex_base: number;
  estimated_total_opex_per_hour_base: number;
};

export type MutationCostBreakdown = {
  construction_base: number;
  fleet_purchase_base: number;
  fleet_configuration_base: number;
  fleet_transfer_fees_base: number;
  fleet_salvage_refund_base: number;
  apply_total_base: number;
  projected_balance_after_apply_base?: number | null;
  projected_opex_per_hour_base: number;
  projected_staff_opex_per_hour_base: number;
};

export type MutationPathValidationMeta = {
  path_validation_mode: string;
  road_snap_tolerance_m: number;
  water_path_tolerance_m: number;
  water_terminal_tolerance_m: number;
  changed_links_checked: number;
  changed_stops_checked: number;
  bus_links_checked: number;
  bus_links_invalid: number;
  ferry_links_checked: number;
  ferry_links_invalid: number;
  road_stops_checked: number;
  road_stops_invalid: number;
  water_stops_checked: number;
  water_stops_invalid: number;
  locked_county_hits: number;
};

export type NetworkMutationResult = {
  scenario: ScenarioDocumentLite;
  manifest: ProjectManifest;
  summary: NetworkMutationSummary;
  cost_breakdown: MutationCostBreakdown;
  path_validation?: MutationPathValidationMeta;
};

export type NetworkMutationPreviewResult = {
  summary: NetworkMutationSummary;
  cost_breakdown: MutationCostBreakdown;
  path_validation?: MutationPathValidationMeta;
};

export type DemandCoverageMeta = {
  country_iso2: string;
  installed: boolean;
  loaded_in_scenario: boolean;
  cells: number;
  surface_version?: string | null;
};

export type DemandCoverageResult = {
  country_iso2: string;
  installed: boolean;
  loaded: boolean;
  cells_loaded: number;
  message: string;
};

export type DemandRebuildResult = {
  loaded_countries: string[];
  missing_countries: string[];
  total_cells: number;
};

export type CountryPackBuildState = "missing" | "installed" | "outdated" | "building" | "failed";

export type CountryPackStatus = {
  country_iso2: string;
  canonical_country_iso2?: string | null;
  runtime_pack_country_iso2?: string | null;
  region_provider_model?: string | null;
  pack_contract_valid?: boolean;
  build_state: CountryPackBuildState | string;
  surface_version?: string | null;
  cells_count: number;
  last_updated_at?: string | null;
  map_installed?: boolean;
  map_ready?: boolean;
  map_pack_version?: string | null;
  map_size_bytes?: number | null;
  demand_installed?: boolean;
  fully_playable?: boolean;
  eligible: boolean;
  reason?: string | null;
};

export type InstallResult = {
  country_iso2: string;
  ok: boolean;
  message: string;
};

export type UninstallResult = {
  country_iso2: string;
  ok: boolean;
  message: string;
};

export type StationJourneyTime = {
  stop_id: string;
  stop_name: string;
  travel_time_s: number;
  stops_away: number;
};

export type StationLineSummary = {
  line_id: string;
  line_name: string;
  mode: string;
  mode_variant?: string | null;
  display_color?: string | null;
  station_index: number;
  station_count: number;
  previous_station_name?: string | null;
  next_station_name?: string | null;
  journey_times: StationJourneyTime[];
};

export type StationRuntimeServiceDiagnostics = {
  counter_provenance?: CounterProvenance | string;
  service_id: string;
  line_id: string;
  planner_attempted_pax: number;
  planner_assigned_pax: number;
  planner_mode_transit_captured_pax: number;
  planner_candidate_paths_raw: number;
  planner_candidate_paths_boardable: number;
  planner_rejected_no_board_or_alight_paths: number;
  planner_rejected_unpaired_board_alight_paths: number;
  runtime_attempted_pax: number;
  planner_board_load_arrivals_pax: number;
  runtime_ingested_pax: number;
  runtime_dropped_not_dispatchable_pax: number;
  runtime_dropped_invalid_stop_pax: number;
  runtime_queue_pax: number;
  runtime_boarding_attempted_pax: number;
  runtime_boarded_pax: number;
  runtime_left_behind_pax: number;
  dispatchable: boolean;
  diagnostic_note?: string | null;
  planner_reason_code?: string | null;
};

export type StationRuntimeDiagnostics = {
  counter_provenance?: CounterProvenance | string;
  tick_index: number;
  planner_attempted_total_pax: number;
  runtime_attempted_total_pax: number;
  planner_cohort_rows: number;
  runtime_queue_total_pax: number;
  snapshot_current_inside_pax: number;
  planner_demand_cells_total: number;
  planner_demand_cells_nonzero_activity: number;
  planner_zones_total: number;
  planner_zones_nonzero_activity: number;
  planner_latent_rows_total: number;
  planner_latent_total_pax: number;
  planner_mode_choice_rows_total: number;
  planner_mode_choice_rows_with_transit_capture: number;
  planner_mode_choice_transit_captured_pax: number;
  planner_mode_choice_candidate_paths_raw_total: number;
  planner_mode_choice_candidate_paths_boardable_total: number;
  planner_mode_choice_rejected_no_board_or_alight_total: number;
  planner_mode_choice_rejected_unpaired_board_alight_total: number;
  planner_assignment_od_rows_with_transit_latent: number;
  planner_assignment_od_rows_with_attempted: number;
  planner_assignment_attempted_total_pax: number;
  planner_assignment_candidate_paths_raw_total: number;
  planner_assignment_candidate_paths_boardable_total: number;
  planner_assignment_rejected_no_board_or_alight_total: number;
  planner_assignment_rejected_unpaired_board_alight_total: number;
  planner_first_zero_stage?: string | null;
  planner_first_zero_reason?: string | null;
  first_zero_or_mismatch?: string | null;
  services: StationRuntimeServiceDiagnostics[];
};

export type StationInspection = {
  stop_id: string;
  name: string;
  x: number;
  y: number;
  stop_type?: string | null;
  interchange_id?: string | null;
  boardings_attempted: number;
  boardings_served: number;
  alightings_served: number;
  denied_boardings: number;
  queue_end: number;
  station_load_current_pax: number;
  passenger_counter_provenance?: CounterProvenance | string;
  station_capacity_boarding_pph: number;
  station_capacity_alighting_pph: number;
  station_queue_capacity_pax: number;
  overflow_dropped: number;
  passengers_declined_last_hour: number;
  station_entries_per_hour: number;
  station_exits_per_hour: number;
  average_wait_to_board_s: number;
  catchment_radius_m: number;
  catchment_cells: number;
  catchment_residents: number;
  catchment_jobs: number;
  catchment_mix_residential: number;
  catchment_mix_office: number;
  catchment_mix_retail: number;
  catchment_mix_recreation: number;
  catchment_mix_industrial: number;
  catchment_mix_education: number;
  catchment_mix_health: number;
  served_lines: StationLineSummary[];
  runtime_diagnostics?: StationRuntimeDiagnostics | null;
};

export type LineStationSummary = {
  stop_id: string;
  name: string;
  x: number;
  y: number;
  cumulative_time_s: number;
};

export type LineDirectionSummary = {
  service_id: string;
  name: string;
  direction?: string | null;
  direction_name?: string | null;
  stop_sequence: string[];
  headway_s: number;
  dwell_s: number;
  vehicle_capacity: number;
};

export type LineActivationReason =
  | "running"
  | "no_target_tph_in_active_band"
  | "no_assigned_units"
  | "no_owned_units"
  | "fleet_insufficient_for_round_trip"
  | "invalid_headway_or_disabled"
  | "no_required_units";

export type LineActivationDiagnostics = {
  minute_of_day: number;
  active_band: string;
  target_tph: number;
  units_owned: number;
  units_assigned: number;
  required_units: number;
  effective_tph: number;
  enabled: boolean;
  reason: LineActivationReason;
};

export type LineInspection = {
  line_id: string;
  name: string;
  mode: string;
  mode_variant?: string | null;
  display_color?: string | null;
  station_count: number;
  service_count: number;
  length_m: number;
  estimated_capex_base: number;
  estimated_opex_per_hour_base: number;
  total_passengers: number;
  boardings_attempted: number;
  boardings_served: number;
  alightings_served: number;
  denied_boardings: number;
  queue_end: number;
  passenger_counter_provenance?: CounterProvenance | string;
  service_enabled: boolean;
  target_tph: number;
  effective_tph: number;
  avg_wait_s: number | null;
  vehicle_capacity_effective: number;
  line_capacity_per_hour: number;
  required_units: number;
  owned_units: number;
  assigned_units: number;
  spare_units: number;
  stock_tier_id?: string | null;
  stock_tier_label?: string | null;
  activation: LineActivationDiagnostics;
  operations_now?: {
    active_band: string;
    live_tph: number;
    avg_wait_s?: number | null;
    capacity_per_hour: number;
  };
  fleet_state?: {
    package_id?: string | null;
    package_label?: string | null;
    cars_per_unit: number;
    speed_level?: string | null;
    comfort_level?: string | null;
    units_owned: number;
    units_committed?: number;
    units_pending?: number;
    units_assigned: number;
    units_required_now: number;
    units_shortage_now: number;
    units_surplus_now: number;
    vehicle_capacity_effective: number;
    pending_orders?: PurchaseOrderLite[];
  };
  schedule_state?: {
    peak_start_minute: number;
    peak_end_minute: number;
    overnight_start_minute: number;
    overnight_end_minute: number;
    tph_peak: number;
    tph_off_peak: number;
    tph_overnight: number;
  };
  cost_story?: {
    fleet_value_base: number;
    fleet_purchase_delta_base: number;
    fleet_configuration_delta_base: number;
    fleet_transfer_fees_base: number;
    fleet_salvage_refund_base: number;
    service_opex_per_hour_base: number;
    staff_opex_per_hour_base: number;
  };
  stations: LineStationSummary[];
  directions: LineDirectionSummary[];
};
