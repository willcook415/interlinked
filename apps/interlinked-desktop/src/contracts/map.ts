import type { ScenarioDocumentLite } from "./session";

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
  region_kind: string;
  region_token: string;
  h3_cell_id?: string | null;
  name: string;
  admin_level: string;
  nation?: string | null;
  source_code?: string | null;
  adjacency_source: string;
  geometry_source: string;
  unlocked: boolean;
  active: boolean;
  adjacent_region_ids: string[];
  unlock_cost_base: number;
  population?: number | null;
  jobs?: number | null;
  residents_smooth: number;
  jobs_smooth: number;
  employment_estimate?: number;
  cells_res8: number;
  geometry?: GeoJsonGeometry | null;
  /** Backend-authoritative hex number from substrate hex numbering.
   *  This is the canonical number that `manual_regions.json` hex_numbers refer to. */
  canonical_hex_number?: number | null;
  /** Parallel array to `geometry` polygons. If `geometry` is a MultiPolygon,
   *  this array contains the canonical hex number for each polygon in order. */
  constituent_hex_numbers?: number[] | null;
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
  // Compatibility-only legacy fields for non-vector fallback tiers.
  // Active UK render path uses style_url + world_context_url as authority.
  counties_url?: string | null;
  major_roads_url?: string | null;
  county_basemap_mid_url_template?: string | null;
  county_basemap_full_url_template?: string | null;
  county_roads_url_template?: string | null;
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
  current_balance_base: number;
  unlocked_region_ids: string[];
  unlocked_countries: string[];
  scenario: ScenarioDocumentLite;
};

export type UnlockFocusResult = {
  region_id: string;
  charged_base: number;
  current_balance_base: number;
  unlocked_regions: number;
  primary_focus_region_id: string;
  active_region_ids: string[];
  materialized_cells: number;
  unlocked_region_ids: string[];
  unlocked_countries: string[];
  scenario: ScenarioDocumentLite;
};

export type SimulationScopeUpdate = {
  max_active_zones?: number;
  remote_regions_mode?: string;
  remote_update_interval_ticks?: number;
  focus_max_active_zones?: number;
  adjacent_max_active_zones?: number;
  remote_max_active_zones?: number;
  adjacent_update_interval_ticks?: number;
  active_region_ids?: string[];
};

export type ScopeState = {
  max_active_zones: number;
  remote_regions_mode: string;
  remote_update_interval_ticks: number;
  focus_max_active_zones: number;
  adjacent_max_active_zones: number;
  remote_max_active_zones: number;
  adjacent_update_interval_ticks: number;
  active_region_ids: string[];
  materialized_cells: number;
};

export type DemandOverlayType =
  | "residential_allocation"
  | "employment_allocation"
  | "total_allocation"
  | "raw_residential_weight"
  | "raw_employment_weight"
  | "fallback_cells";

export type DemandOverlayRegionDatum = {
  region_id: string;
  region_name: string;
  lon: number;
  lat: number;
  intensity_score: number;
  service_gap_score: number;
  service_gap_ratio: number;
};

export type DemandOverlayCorridorDatum = {
  origin_region_id: string;
  destination_region_id: string;
  origin_lon: number;
  origin_lat: number;
  destination_lon: number;
  destination_lat: number;
  corridor_score: number;
  latent_passengers: number;
  realised_passengers: number;
  unserved_passengers: number;
  is_underserved: boolean;
};

export type DemandOverlayCellDatum = {
  cell_id: string;
  planning_region_id?: string | null;
  display_geometry?: GeoJsonGeometry | null;
  display_geometry_clipped?: boolean;
  lon: number;
  lat: number;
  area_m2: number;
  residents_night: number;
  jobs_day: number;
  centrality_score: number;
  data_quality_score: number;
  activity_mix_residential: number;
  activity_mix_office: number;
  activity_mix_retail: number;
  activity_mix_recreation: number;
  activity_mix_industrial: number;
  activity_mix_education: number;
  activity_mix_health: number;
  raw_weight_residential: number;
  raw_weight_employment: number;
  allocated_residential_mass: number;
  allocated_employment_mass: number;
  fallback_reason?: string | null;
};

export type DemandOverlayPayload = {
  available: boolean;
  reason?: string | null;
  intensity_available?: boolean;
  intensity_reason?: string | null;
  service_gap_available?: boolean;
  service_gap_reason?: string | null;
  corridor_desire_available?: boolean;
  corridor_desire_reason?: string | null;
  run_id?: string | null;
  cell_data_total?: number;
  cell_data_mappable?: number;
  cell_fallback_count?: number;
  overlay_unlocked_region_count?: number;
  overlay_unlocked_geometry_region_count?: number;
  overlay_unlocked_geometry_missing_region_count?: number;
  overlay_unlocked_geometry_available?: boolean;
  overlay_explicit_geometry_region_count?: number;
  overlay_h3_fallback_region_count?: number;
  overlay_unlocked_union_area?: number;
  overlay_rendered_union_area?: number;
  overlay_uncovered_unlocked_area?: number;
  overlay_uncovered_ratio?: number;
  overlay_outside_rendered_area?: number;
  overlay_outside_ratio?: number;
  overlay_expected_intersecting_cell_count?: number;
  overlay_existing_intersecting_cell_count?: number;
  overlay_missing_intersecting_cell_count?: number;
  overlay_filtered_intersecting_cell_count?: number;
  overlay_invalid_clipped_geometry_count?: number;
  overlay_clipped_geometry_failed_count?: number;
  overlay_coverage_debug_enabled?: boolean;
  overlay_coverage_debug_failed?: boolean;
  overlay_coverage_debug_error?: string | null;
  overlay_cells_fully_inside?: number;
  overlay_cells_clipped?: number;
  overlay_cells_outside_unlocked?: number;
  overlay_duplicate_cell_ids?: number;
  cell_data: DemandOverlayCellDatum[];
  region_data: DemandOverlayRegionDatum[];
  corridor_data: DemandOverlayCorridorDatum[];
};
