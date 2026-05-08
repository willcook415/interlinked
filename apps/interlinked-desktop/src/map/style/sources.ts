import type { Map as MapLibreMap } from "maplibre-gl";

type GeoFeature = {
  type: "Feature";
  geometry: { type: "Point" | "LineString" | "Polygon" | "MultiPolygon"; coordinates: unknown };
  properties: Record<string, string | number | boolean | null>;
};
type GeoCollection = { type: "FeatureCollection"; features: GeoFeature[] };

function fc(features: GeoFeature[] = []): GeoCollection {
  return { type: "FeatureCollection", features };
}

export const SRC_WORLD = "world-context";
export const SRC_WORLD_LABELS = "world-country-labels";
export const SRC_OCEAN = "world-ocean";
export const SRC_OCEAN_LABELS = "world-ocean-labels";
export const SRC_REGIONS = "regions";
// Back-compat alias for older region layer/source naming.
export const SRC_COUNTIES = SRC_REGIONS;
export const SRC_REGION_HEXES = "planning-region-hexes";
export const SRC_HEX_COVERAGE_GAPS = "planning-region-hex-coverage-gaps";
export const SRC_MAJOR_ROADS = "major-roads";
export const SRC_COUNTY_BASEMAP = "county-basemap";
export const SRC_LINKS = "links";
export const SRC_TRANSFERS = "transfers";
export const SRC_STOPS = "stops";
export const SRC_ZONES = "zones";
export const SRC_DEMAND_CELLS = "demand-overlay-cells";
// Back-compat alias for older demand source naming.
export const SRC_DEMAND_ZONES = SRC_DEMAND_CELLS;
export const SRC_DEMAND_CORRIDORS = "demand-overlay-corridors";
export const SRC_BUILD_PREVIEW = "build-preview";
export const SRC_VEHICLES = "vehicles";

const WORLD_OCEAN_FILL: GeoCollection = fc([
  {
    type: "Feature",
    geometry: {
      type: "Polygon",
      coordinates: [
        [
          [-180, -85],
          [180, -85],
          [180, 85],
          [-180, 85],
          [-180, -85],
        ],
      ],
    },
    properties: { kind: "ocean" },
  },
]);

const WORLD_OCEAN_LABELS: GeoCollection = fc([
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [-38, 18] },
    properties: { name: "Atlantic Ocean", rank: 1 },
  },
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [76, -16] },
    properties: { name: "Indian Ocean", rank: 1 },
  },
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [-160, 2] },
    properties: { name: "Pacific Ocean", rank: 1 },
  },
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [14, 64] },
    properties: { name: "Norwegian Sea", rank: 2 },
  },
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [-6, 56] },
    properties: { name: "North Sea", rank: 2 },
  },
  {
    type: "Feature",
    geometry: { type: "Point", coordinates: [16, 34] },
    properties: { name: "Mediterranean Sea", rank: 2 },
  },
]);

export function ensureMapSources(map: MapLibreMap): void {
  if (!map.getSource(SRC_WORLD)) map.addSource(SRC_WORLD, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_WORLD_LABELS)) {
    map.addSource(SRC_WORLD_LABELS, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_OCEAN)) map.addSource(SRC_OCEAN, { type: "geojson", data: WORLD_OCEAN_FILL as never });
  if (!map.getSource(SRC_OCEAN_LABELS)) {
    map.addSource(SRC_OCEAN_LABELS, { type: "geojson", data: WORLD_OCEAN_LABELS as never });
  }
  if (!map.getSource(SRC_REGIONS)) map.addSource(SRC_REGIONS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_REGION_HEXES)) {
    map.addSource(SRC_REGION_HEXES, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_HEX_COVERAGE_GAPS)) {
    map.addSource(SRC_HEX_COVERAGE_GAPS, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_MAJOR_ROADS)) map.addSource(SRC_MAJOR_ROADS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_COUNTY_BASEMAP)) map.addSource(SRC_COUNTY_BASEMAP, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_LINKS)) map.addSource(SRC_LINKS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_TRANSFERS)) map.addSource(SRC_TRANSFERS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_STOPS)) map.addSource(SRC_STOPS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_ZONES)) map.addSource(SRC_ZONES, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_DEMAND_CELLS)) {
    map.addSource(SRC_DEMAND_CELLS, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_DEMAND_CORRIDORS)) {
    map.addSource(SRC_DEMAND_CORRIDORS, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_VEHICLES)) map.addSource(SRC_VEHICLES, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_BUILD_PREVIEW)) {
    map.addSource(SRC_BUILD_PREVIEW, { type: "geojson", data: fc() as never });
  }
}
