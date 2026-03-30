export type AppRoute =
  | "home"
  | "new_game"
  | "load_game"
  | "new_scenario"
  | "load_scenario"
  | "session_game"
  | "session_scenario";

export type SessionKind = "game" | "scenario";
export type Difficulty = "easy" | "standard" | "hard";
export type SimulationSpeed = 1 | 2 | 4;
export type CurrencyCode = "GBP" | "USD" | "EUR";
export type DifficultyProfile = {
  profile_id: string;
  demand_mult: number;
  capex_mult: number;
  opex_mult: number;
  maintenance_mult: number;
  penalty_mult: number;
  ancillary_revenue_mult: number;
  unlock_cost_mult: number;
};

export type ZoneLite = {
  id: string;
  x: number;
  y: number;
  population: number;
  jobs: number;
  country_iso2?: string | null;
};

export type StopLite = {
  id: string;
  name?: string | null;
  x: number;
  y: number;
  country_iso2?: string | null;
  interchange_id?: string | null;
  stop_type?: string | null;
};

export type LinkLite = {
  id: string;
  from_stop: string;
  to_stop: string;
  distance_m: number;
  mode: string;
  mode_variant?: string | null;
  speed_mps: number;
  geometry?: [number, number][] | null;
  line_id?: string | null;
  capacity_per_hour?: number | null;
};

export type ServiceLite = {
  id: string;
  line_id?: string | null;
  name?: string | null;
  mode: string;
  mode_variant?: string | null;
  stop_sequence: string[];
  direction?: string | null;
  direction_name?: string | null;
  display_color?: string | null;
  service_enabled?: boolean | null;
  operating_tph?: number | null;
  stock_tier_id?: string | null;
  stock_units_owned?: number | null;
  stock_units_assigned?: number | null;
  rolling_stock_profile?: RollingStockProfileLite | null;
  schedule_profile?: LineScheduleProfileLite | null;
  headway_s: number;
  dwell_s: number;
  vehicle_capacity: number;
  board_penalty_s?: number | null;
};

export type PurchaseOrderLite = {
  order_id: string;
  units: number;
  label?: string | null;
  status?: string | null;
  unit_cost_base?: number | null;
  total_cost_base?: number | null;
  placed_at_tick_s?: number | null;
  eta_at_tick_s?: number | null;
};

export type RollingStockProfileLite = {
  package_id?: string | null;
  units_owned?: number | null;
  cars_per_unit?: number | null;
  speed_level?: string | null;
  comfort_level?: string | null;
  pending_orders?: PurchaseOrderLite[] | null;
};

export type LineScheduleProfileLite = {
  peak_start_minute?: number | null;
  peak_end_minute?: number | null;
  overnight_start_minute?: number | null;
  overnight_end_minute?: number | null;
  tph_peak?: number | null;
  tph_off_peak?: number | null;
  tph_overnight?: number | null;
};

export type TransferLite = {
  from_stop: string;
  to_stop: string;
  time_s: number;
  penalty_s?: number | null;
  allowed_modes?: string[] | null;
};

export type ScenarioLite = {
  meta?: { name?: string; crs?: { type?: string } | null } | null;
  world: {
    zones: ZoneLite[];
    stops: StopLite[];
    links: LinkLite[];
    services: ServiceLite[];
    transfers: TransferLite[];
    transfer_rules?: unknown[] | null;
    demand_cells?: unknown[];
  };
};

export type ScenarioDocumentLite = {
  schema_version: number;
  scenario: ScenarioLite;
};

export type SimulationClock = {
  sim_datetime_utc: string;
  tick_seconds: number;
  running: boolean;
  speed: SimulationSpeed;
};

export type StartLocation = {
  country_iso2: string;
  country_name: string;
  city_id: number;
  city_name: string;
  city_lon: number;
  city_lat: number;
  city_population?: number | null;
};

export type GameProgressMetrics = {
  budget: number;
  currency: CurrencyCode;
  ridership: number;
  coverage: number;
  milestones: number;
};

export type RegionStateManifest = {
  unlocked_region_ids: string[];
  primary_focus_region_id?: string | null;
  active_region_ids: string[];
};

export type SimulationScopeManifest = {
  max_active_zones: number;
  remote_regions_mode: string;
  remote_update_interval_ticks: number;
  focus_max_active_zones: number;
  adjacent_max_active_zones: number;
  remote_max_active_zones: number;
  adjacent_update_interval_ticks: number;
};

export type FarePolicyManifest = {
  enabled: boolean;
  fare_mode_bus_base: number;
  fare_mode_tram_base: number;
  fare_mode_metro_base: number;
  fare_mode_rail_base: number;
  fare_mode_ferry_base: number;
  fare_mode_default_base: number;
  transfer_window_s: number;
  free_transfers_per_trip: number;
};

export type EconomyManifest = {
  currency: CurrencyCode;
  difficulty: string;
  difficulty_profile?: DifficultyProfile;
  economy_revision?: number;
  starting_budget_base: number;
  current_balance_base: number;
  cumulative_capex_base: number;
  cumulative_opex_base: number;
  cumulative_revenue_base: number;
  cumulative_lost_demand_penalty_base: number;
  unlocked_countries: string[];
  fare_policy: FarePolicyManifest;
  region_ledger?: Record<
    string,
    {
      revenue_base: number;
      opex_base: number;
      capex_base: number;
      penalties_base: number;
      net_base: number;
    }
  >;
  maintenance_rate?: number;
  ancillary_revenue_rate?: number;
  quality_penalty_rates?: {
    overcrowding_base_per_passenger: number;
    reliability_base_per_passenger: number;
  };
  monthly_financials?: Array<{
    month_index: number;
    revenue_base: number;
    opex_base: number;
    capex_base: number;
    penalties_base: number;
    net_base: number;
  }>;
};

export type ProjectManifest = {
  project_id: string;
  name: string;
  created_at: string;
  updated_at: string;
  session_kind: SessionKind;
  engine_schema_version: number;
  ui_schema_version: number;
  last_opened_run_id?: string | null;
  recent_runs: string[];
  clock_state: SimulationClock;
  progress_metrics?: GameProgressMetrics | null;
  start_location?: StartLocation | null;
  region_state?: RegionStateManifest;
  simulation_scope?: SimulationScopeManifest;
  economy?: EconomyManifest;
};

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

export type RunMeta = {
  run_id: string;
  created_at: string;
  scenario_name: string;
  seed: number;
  horizon_s: number;
  time_bin_s: number;
  time_of_day_s?: number | null;
  output_path: string;
  summary_path: string;
  meta_path: string;
};

export type SnapshotMeta = {
  snapshot_id: string;
  name: string;
  notes?: string | null;
  created_at: string;
  tick_seconds: number;
};

export type OpenSessionResult = {
  project_path: string;
  manifest: ProjectManifest;
  scenario: ScenarioDocumentLite;
  runs: RunMeta[];
  snapshots: SnapshotMeta[];
  clock: SimulationClock;
  start_location?: StartLocation | null;
};

export type SaveResult = {
  ok: boolean;
  updated_at: string;
  written_files: string[];
};

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

export type GameSaveMeta = {
  project_id: string;
  project_path: string;
  name: string;
  last_opened_at: string;
  sim_datetime_utc: string;
  start_country?: string | null;
  start_city?: string | null;
  unlocked_countries: number;
  network_stops: number;
  network_links: number;
  network_services: number;
  total_link_km: number;
  progress_metrics: GameProgressMetrics;
};

export type ScenarioSaveMeta = {
  project_id: string;
  project_path: string;
  name: string;
  last_opened_at: string;
  latest_run_id?: string | null;
  latest_run_created_at?: string | null;
  latest_share_trips_served?: number | null;
  latest_mean_generalized_cost_s?: number | null;
  latest_total_boardings_denied?: number | null;
  latest_projected_net_balance?: number | null;
  start_country?: string | null;
  start_city?: string | null;
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

export type FinancialDashboardRequest = {
  mode?: string | null;
  line_id?: string | null;
  region_id?: string | null;
  granularity?: "day" | "week" | "month" | "year" | null;
  periods?: number | null;
};

export type FinancialDashboardPoint = {
  period_index: number;
  label: string;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialModeBreakdownRow = {
  mode: string;
  lines: number;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialLineBreakdownRow = {
  line_id: string;
  line_name: string;
  mode: string;
  region_id?: string | null;
  estimated_capex_base: number;
  estimated_opex_per_hour_base: number;
  staff_opex_per_hour_base: number;
  fleet_value_base: number;
  units_owned: number;
  units_pending: number;
  units_assigned: number;
};

export type FinancialRegionBreakdownRow = {
  region_id: string;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialDashboardResponse = {
  currency: CurrencyCode | string;
  granularity: string;
  periods: number;
  current_balance_base: number;
  total_revenue_base: number;
  total_opex_base: number;
  total_capex_base: number;
  total_penalties_base: number;
  total_net_base: number;
  points: FinancialDashboardPoint[];
  mode_breakdown: FinancialModeBreakdownRow[];
  line_breakdown: FinancialLineBreakdownRow[];
  region_breakdown: FinancialRegionBreakdownRow[];
};

export type DemandRebuildResult = {
  loaded_countries: string[];
  missing_countries: string[];
  total_cells: number;
};

export type GeoJsonGeometry =
  | {
      type: "Polygon";
      coordinates: number[][][];
    }
  | {
      type: "MultiPolygon";
      coordinates: number[][][][];
    };

export type GeoJsonPointGeometry = {
  type: "Point";
  coordinates: [number, number];
};

export type GeoJsonLineStringGeometry = {
  type: "LineString";
  coordinates: [number, number][];
};

export type GeoJsonAnyGeometry =
  | GeoJsonPointGeometry
  | GeoJsonLineStringGeometry
  | GeoJsonGeometry;

export type GeoJsonFeature = {
  type: "Feature";
  geometry: GeoJsonAnyGeometry;
  properties?: Record<string, string | number | boolean | null> | null;
};

export type GeoJsonFeatureCollection = {
  type: "FeatureCollection";
  features: GeoJsonFeature[];
};

export type RegionStatus = {
  region_id: string;
  country_iso2: string;
  name: string;
  admin_level: string;
  nation?: string | null;
  source_code?: string | null;
  unlocked: boolean;
  active: boolean;
  adjacent_region_ids: string[];
  unlock_cost_base: number;
  residents_smooth: number;
  jobs_smooth: number;
  employment_estimate?: number;
  cells_res8: number;
  geometry?: GeoJsonGeometry | null;
};

export type WorldCountryFeatureProperties = {
  country_iso2: string;
  name: string;
  playable_now: boolean;
  coming_soon: boolean;
};

export type RoadFeatureProperties = {
  road_class: string;
  name?: string | null;
  ref?: string | null;
  bridge?: boolean | null;
  tunnel?: boolean | null;
};

export type CountryMapContext = {
  country_iso2: string;
  world_context: GeoJsonFeatureCollection;
  major_roads: GeoJsonFeatureCollection;
  default_bounds?: [[number, number], [number, number]] | null;
};

export type RegionStreetContext = {
  region_id: string;
  country_iso2: string;
  roads: GeoJsonFeatureCollection;
};

export type MapRuntimeConfig = {
  country_iso2: string;
  style_url?: string | null;
  world_context_url: string;
  counties_url?: string | null;
  major_roads_url?: string | null;
  county_basemap_mid_url_template?: string | null;
  county_basemap_full_url_template?: string | null;
  default_bounds?: [[number, number], [number, number]] | null;
  map_pack_version?: string | null;
  map_ready: boolean;
};

export type UnlockResult = {
  region_id: string;
  charged_base: number;
  current_balance_base: number;
  unlocked_regions: number;
};

export type FocusResult = {
  primary_focus_region_id: string;
  active_region_ids: string[];
  materialized_cells: number;
};

export type UnlockFocusResult = {
  region_id: string;
  charged_base: number;
  current_balance_base: number;
  unlocked_regions: number;
  primary_focus_region_id: string;
  active_region_ids: string[];
  materialized_cells: number;
};

export type SimulationScopeUpdate = {
  max_active_zones?: number;
  remote_regions_mode?: string;
  active_region_ids?: string[];
};

export type ScopeState = {
  max_active_zones: number;
  remote_regions_mode: string;
  active_region_ids: string[];
  materialized_cells: number;
};

export type DeletedSaveMeta = {
  deleted_id: string;
  project_id: string;
  name: string;
  session_kind: SessionKind;
  deleted_at: string;
};

export type DeleteSaveResult = {
  deleted_id: string;
  ok: boolean;
};

export type RestoreSaveResult = {
  project_id: string;
  ok: boolean;
};

export type PurgeSaveResult = {
  deleted_id: string;
  ok: boolean;
};

export type CountryOption = {
  iso2: string;
  name: string;
};

export type CountryPackBuildState = "missing" | "installed" | "outdated" | "building" | "failed";

export type CountryPackStatus = {
  country_iso2: string;
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

export type CityOption = {
  geonameid: number;
  name: string;
  lat: number;
  lon: number;
  population: number;
};

export type PlanningRunConfig = {
  deterministic_seed?: number | null;
  horizon_s?: number | null;
  time_bin_s?: number | null;
  time_of_day_s?: number | null;
};

export type CompareResult = {
  base_run_id: string;
  candidate_run_id: string;
  base: {
    total_trips: number;
    share_trips_served: number;
    mean_generalized_cost_s: number;
    mean_wait_time_s: number;
  };
  candidate: {
    total_trips: number;
    share_trips_served: number;
    mean_generalized_cost_s: number;
    mean_wait_time_s: number;
  };
  delta: {
    kpis: {
      total_trips: number;
      mean_generalized_cost_s: number;
      mean_wait_time_s: number;
      mean_walk_time_s: number;
      mean_transfers: number;
    };
  };
};

export type SandboxStateLite = {
  snapshot: SnapshotMeta;
  scenario: ScenarioDocumentLite;
  history_frames: number;
};

export type HistoryFrameLite = {
  t_s?: number;
  queue_summary?: { total_queue?: number; max_queue?: number };
  kpis?: { share_trips_served?: number; mean_wait_time_s?: number };
  service_loads?: Array<{ service_id: string; load_to_capacity: number }>;
};

export type GameCreatePayload = {
  name: string;
  country_iso2: string;
  country_name: string;
  city_id: number;
  city_name: string;
  city_lon: number;
  city_lat: number;
  city_population?: number | null;
  difficulty: Difficulty;
  currency: CurrencyCode;
  starting_budget: number;
};

export type ScenarioCreatePayload = {
  name: string;
};

export type Mission = {
  id: string;
  title: string;
  description: string;
  status: "active" | "completed" | "blocked";
};

export type AlertItem = {
  id: string;
  title: string;
  detail?: string | null;
  severity: "info" | "warn" | "critical";
  action_label?: string | null;
  target?:
    | {
        kind: "line" | "stop" | "region";
        id: string;
      }
    | null;
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
