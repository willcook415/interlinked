import type { EconomyManifest } from "./economy";
import type { RunMeta } from "./planning";

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

export type CityOption = {
  geonameid: number;
  name: string;
  lat: number;
  lon: number;
  population: number;
};

export type SandboxStateLite = {
  snapshot: SnapshotMeta;
  scenario: ScenarioDocumentLite;
  history_frames: number;
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
