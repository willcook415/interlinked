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
export const SRC_COUNTIES = "counties";
export const SRC_MAJOR_ROADS = "major-roads";
export const SRC_COUNTY_BASEMAP = "county-basemap";
export const SRC_LINKS = "links";
export const SRC_TRANSFERS = "transfers";
export const SRC_STOPS = "stops";
export const SRC_ZONES = "zones";
export const SRC_BUILD_PREVIEW = "build-preview";
export const SRC_VEHICLES = "vehicles";
export const EMPTY_STOP_FILTER = ["==", ["get", "id"], "__none__"] as const;
export const EMPTY_LINE_FILTER = ["==", ["get", "line_id"], "__none__"] as const;
export const EMPTY_VEHICLE_FILTER = ["==", ["get", "vehicle_id"], "__none__"] as const;

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

function roadColorExpression(): unknown[] {
  return [
    "match",
    ["get", "road_class"],
    "motorway",
    "#6f7984",
    "motorway_link",
    "#7c8692",
    "trunk",
    "#7b8795",
    "trunk_link",
    "#87929e",
    "primary",
    "#8d99a8",
    "primary_link",
    "#98a3af",
    "secondary",
    "#a1abb6",
    "secondary_link",
    "#aeb7bf",
    "tertiary",
    "#b4bcc3",
    "tertiary_link",
    "#bcc3c9",
    "#c9d0d6",
  ];
}

function roadWidthExpression(): unknown[] {
  return [
    "interpolate",
    ["linear"],
    ["zoom"],
    6.5,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      1.3,
      "motorway_link",
      1.0,
      "trunk",
      1.15,
      "trunk_link",
      0.85,
      "primary",
      0.95,
      "primary_link",
      0.72,
      "secondary",
      0.45,
      "secondary_link",
      0.35,
      "tertiary",
      0.3,
      "tertiary_link",
      0.28,
      "unclassified",
      0.25,
      "residential",
      0.22,
      "living_street",
      0.18,
      "service",
      0.16,
      "pedestrian",
      0.14,
      0.2,
    ],
    12,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      2.8,
      "motorway_link",
      2.0,
      "trunk",
      2.3,
      "trunk_link",
      1.7,
      "primary",
      1.9,
      "primary_link",
      1.4,
      "secondary",
      1.25,
      "secondary_link",
      0.95,
      "tertiary",
      0.9,
      "tertiary_link",
      0.8,
      "unclassified",
      0.7,
      "residential",
      0.62,
      "living_street",
      0.52,
      "service",
      0.45,
      "pedestrian",
      0.42,
      0.55,
    ],
    15,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      5.0,
      "motorway_link",
      3.8,
      "trunk",
      4.2,
      "trunk_link",
      3.2,
      "primary",
      3.4,
      "primary_link",
      2.6,
      "secondary",
      2.4,
      "secondary_link",
      1.9,
      "tertiary",
      1.8,
      "tertiary_link",
      1.5,
      "unclassified",
      1.3,
      "residential",
      1.15,
      "living_street",
      0.95,
      "service",
      0.85,
      "pedestrian",
      0.8,
      1.0,
    ],
  ];
}

function roadCasingWidthExpression(): unknown[] {
  return [
    "interpolate",
    ["linear"],
    ["zoom"],
    6.5,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      1.76,
      "motorway_link",
      1.35,
      "trunk",
      1.55,
      "trunk_link",
      1.15,
      "primary",
      1.28,
      "primary_link",
      0.97,
      "secondary",
      0.61,
      "secondary_link",
      0.47,
      "tertiary",
      0.41,
      "tertiary_link",
      0.38,
      "unclassified",
      0.34,
      "residential",
      0.3,
      "living_street",
      0.24,
      "service",
      0.22,
      "pedestrian",
      0.19,
      0.27,
    ],
    12,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      3.78,
      "motorway_link",
      2.7,
      "trunk",
      3.11,
      "trunk_link",
      2.3,
      "primary",
      2.57,
      "primary_link",
      1.89,
      "secondary",
      1.69,
      "secondary_link",
      1.28,
      "tertiary",
      1.22,
      "tertiary_link",
      1.08,
      "unclassified",
      0.94,
      "residential",
      0.84,
      "living_street",
      0.7,
      "service",
      0.61,
      "pedestrian",
      0.57,
      0.74,
    ],
    15,
    [
      "match",
      ["get", "road_class"],
      "motorway",
      6.75,
      "motorway_link",
      5.13,
      "trunk",
      5.67,
      "trunk_link",
      4.32,
      "primary",
      4.59,
      "primary_link",
      3.51,
      "secondary",
      3.24,
      "secondary_link",
      2.57,
      "tertiary",
      2.43,
      "tertiary_link",
      2.03,
      "unclassified",
      1.76,
      "residential",
      1.55,
      "living_street",
      1.28,
      "service",
      1.15,
      "pedestrian",
      1.08,
      1.35,
    ],
  ];
}

function localRoadOpacityExpression(): unknown[] {
  return ["interpolate", ["linear"], ["zoom"], 6.5, 0.35, 10, 0.62, 13.5, 0.95];
}

function localRoadFilter(): unknown[] {
  return ["==", ["get", "feature_layer"], "road"];
}

function railFilter(): unknown[] {
  return ["==", ["get", "feature_layer"], "rail"];
}

function waterFilter(): unknown[] {
  return ["==", ["get", "feature_layer"], "water"];
}

function landuseFilter(): unknown[] {
  return ["==", ["get", "feature_layer"], "landuse"];
}

export function ensureMapLayers(map: MapLibreMap): void {
  if (!map.getSource(SRC_WORLD)) map.addSource(SRC_WORLD, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_WORLD_LABELS)) {
    map.addSource(SRC_WORLD_LABELS, { type: "geojson", data: fc() as never });
  }
  if (!map.getSource(SRC_OCEAN)) map.addSource(SRC_OCEAN, { type: "geojson", data: WORLD_OCEAN_FILL as never });
  if (!map.getSource(SRC_OCEAN_LABELS)) {
    map.addSource(SRC_OCEAN_LABELS, { type: "geojson", data: WORLD_OCEAN_LABELS as never });
  }
  if (!map.getSource(SRC_COUNTIES)) map.addSource(SRC_COUNTIES, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_MAJOR_ROADS)) map.addSource(SRC_MAJOR_ROADS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_COUNTY_BASEMAP)) map.addSource(SRC_COUNTY_BASEMAP, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_LINKS)) map.addSource(SRC_LINKS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_TRANSFERS)) map.addSource(SRC_TRANSFERS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_STOPS)) map.addSource(SRC_STOPS, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_ZONES)) map.addSource(SRC_ZONES, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_VEHICLES)) map.addSource(SRC_VEHICLES, { type: "geojson", data: fc() as never });
  if (!map.getSource(SRC_BUILD_PREVIEW)) {
    map.addSource(SRC_BUILD_PREVIEW, { type: "geojson", data: fc() as never });
  }

  if (!map.getLayer("world-ocean-fill")) {
    map.addLayer({
      id: "world-ocean-fill",
      type: "fill",
      source: SRC_OCEAN,
      paint: {
        "fill-color": "#d9e7f6",
        "fill-opacity": 1.0,
      },
    } as never);
  }
  if (!map.getLayer("world-country-fill")) {
    map.addLayer({
      id: "world-country-fill",
      type: "fill",
      source: SRC_WORLD,
      filter: ["==", ["get", "unlocked_now"], 0] as never,
      paint: {
        "fill-color": [
          "case",
          ["==", ["get", "unlocked_now"], 1],
          "#edf2f7",
          "#cfd6df",
        ],
        "fill-opacity": 1.0,
      },
    } as never);
  }
  if (!map.getLayer("world-country-outline")) {
    map.addLayer({
      id: "world-country-outline",
      type: "line",
      source: SRC_WORLD,
      paint: {
        "line-color": "#bac4cf",
        "line-width": ["interpolate", ["linear"], ["zoom"], 2, 0.45, 6, 0.8],
      },
    } as never);
  }
  if (!map.getLayer("world-country-label")) {
    map.addLayer({
      id: "world-country-label",
      type: "symbol",
      source: SRC_WORLD_LABELS,
      minzoom: 2.4,
      maxzoom: 7.2,
      layout: {
        "text-field": ["coalesce", ["get", "name"], ""],
        "text-size": ["interpolate", ["linear"], ["zoom"], 2.4, 11, 5.2, 14, 7.2, 13],
        "text-font": ["Noto Sans Bold"],
        "text-letter-spacing": 0.02,
        "text-max-width": 11,
      },
      paint: {
        "text-color": [
          "case",
          ["==", ["get", "unlocked_now"], 1],
          "#4d637d",
          "#64788f",
        ],
        "text-opacity": ["interpolate", ["linear"], ["zoom"], 2.4, 0.84, 5.8, 0.78, 7.2, 0.0],
        "text-halo-color": "rgba(248,251,255,0.96)",
        "text-halo-width": 1.6,
      },
    } as never);
  }
  if (!map.getLayer("world-ocean-label")) {
    map.addLayer({
      id: "world-ocean-label",
      type: "symbol",
      source: SRC_OCEAN_LABELS,
      minzoom: 2.2,
      maxzoom: 8.2,
      layout: {
        "text-field": ["coalesce", ["get", "name"], ""],
        "text-font": ["Noto Sans Italic", "Noto Sans Regular"],
        "text-size": [
          "interpolate",
          ["linear"],
          ["zoom"],
          2.2,
          ["match", ["get", "rank"], 1, 10, 0],
          4.1,
          ["match", ["get", "rank"], 1, 11, 9],
          7.2,
          ["match", ["get", "rank"], 1, 11.4, 10],
          8.2,
          ["match", ["get", "rank"], 1, 11, 10],
        ],
        "text-letter-spacing": 0.08,
        "text-max-width": 16,
        "text-allow-overlap": false,
      },
      paint: {
        "text-color": "rgba(59,107,151,0.82)",
        "text-opacity": [
          "interpolate",
          ["linear"],
          ["zoom"],
          2.2,
          ["match", ["get", "rank"], 1, 0.78, 0],
          4.1,
          ["match", ["get", "rank"], 1, 0.74, 0.3],
          5.6,
          ["match", ["get", "rank"], 1, 0.72, 0.56],
          7.2,
          ["match", ["get", "rank"], 1, 0.2, 0.24],
          8.2,
          0,
        ],
        "text-halo-color": "rgba(242,248,255,0.96)",
        "text-halo-width": 1.3,
      },
    } as never);
  }
  if (!map.getLayer("gb-major-roads")) {
    map.addLayer({
      id: "gb-major-roads",
      type: "line",
      source: SRC_MAJOR_ROADS,
      minzoom: 4.5,
      paint: {
        "line-color": "#9aa5b0",
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 4.5, 0.42, 9, 0.72],
        "line-width": ["interpolate", ["linear"], ["zoom"], 4.5, 0.6, 9, 1.8, 12, 3.0],
      },
    } as never);
  }
  if (!map.getLayer("county-basemap-landuse")) {
    map.addLayer({
      id: "county-basemap-landuse",
      type: "fill",
      source: SRC_COUNTY_BASEMAP,
      filter: landuseFilter() as never,
      minzoom: 7,
      paint: {
        "fill-color": "#edf3e7",
        "fill-opacity": ["interpolate", ["linear"], ["zoom"], 7, 0.24, 11, 0.36, 14, 0.46],
      },
    } as never);
  }
  if (!map.getLayer("county-basemap-water")) {
    map.addLayer({
      id: "county-basemap-water",
      type: "fill",
      source: SRC_COUNTY_BASEMAP,
      filter: waterFilter() as never,
      minzoom: 6.8,
      paint: {
        "fill-color": "#d9e8f5",
        "fill-opacity": ["interpolate", ["linear"], ["zoom"], 6.8, 0.5, 12, 0.72],
      },
    } as never);
  }
  if (!map.getLayer("county-basemap-rail")) {
    map.addLayer({
      id: "county-basemap-rail",
      type: "line",
      source: SRC_COUNTY_BASEMAP,
      filter: railFilter() as never,
      minzoom: 7.5,
      paint: {
        "line-color": "#8f9aa7",
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 7.5, 0.3, 12.5, 0.6],
        "line-width": ["interpolate", ["linear"], ["zoom"], 7.5, 0.5, 12.5, 1.8, 15, 2.4],
      },
    } as never);
  }
  if (!map.getLayer("county-basemap-road-casing")) {
    map.addLayer({
      id: "county-basemap-road-casing",
      type: "line",
      source: SRC_COUNTY_BASEMAP,
      filter: localRoadFilter() as never,
      minzoom: 6.5,
      paint: {
        "line-color": "#ffffff",
        "line-opacity": localRoadOpacityExpression() as never,
        "line-width": roadCasingWidthExpression() as never,
      },
    } as never);
  }
  if (!map.getLayer("county-basemap-roads")) {
    map.addLayer({
      id: "county-basemap-roads",
      type: "line",
      source: SRC_COUNTY_BASEMAP,
      filter: localRoadFilter() as never,
      minzoom: 6.5,
      paint: {
        "line-color": roadColorExpression() as never,
        "line-width": roadWidthExpression() as never,
        "line-opacity": localRoadOpacityExpression() as never,
      },
    } as never);
  }
  if (!map.getLayer("county-fill")) {
    map.addLayer({
      id: "county-fill",
      type: "fill",
      source: SRC_COUNTIES,
      paint: {
        "fill-color": [
          "case",
          ["==", ["get", "focus"], 1],
          "#d8e8fa",
          ["==", ["get", "unlocked"], 1],
          "#f7fbff",
          "#b6c0cb",
        ],
        "fill-opacity": ["case", ["==", ["get", "unlocked"], 1], 0.0, 0.48],
      },
    } as never);
  }
  if (!map.getLayer("county-outline")) {
    map.addLayer({
      id: "county-outline",
      type: "line",
      source: SRC_COUNTIES,
      paint: {
        "line-color": "#7f90a3",
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 0.9, 10, 1.5],
      },
    } as never);
  }
  if (!map.getLayer("county-focus-outline")) {
    map.addLayer({
      id: "county-focus-outline",
      type: "line",
      source: SRC_COUNTIES,
      filter: ["==", ["get", "focus"], 1],
      paint: {
        "line-color": "#2560a8",
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 1.4, 11, 3.1],
      },
    } as never);
  }
  if (!map.getLayer("links-major")) {
    map.addLayer({
      id: "links-major",
      type: "line",
      source: SRC_LINKS,
      filter: ["==", ["get", "major_mode"], 1],
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#1f3e63"],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 3, 0.8, 9, 0.45],
        "line-width": ["interpolate", ["linear"], ["zoom"], 3, 1.2, 9, 2.0],
      },
    } as never);
  }
  if (!map.getLayer("links-trunk")) {
    map.addLayer({
      id: "links-trunk",
      type: "line",
      source: SRC_LINKS,
      minzoom: 7,
      filter: ["any", ["==", ["get", "major_mode"], 1], ["==", ["get", "focus_connector"], 1]],
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#426d9c"],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 7, 0.35, 13, 0.62],
        "line-width": ["interpolate", ["linear"], ["zoom"], 7, 1.0, 13, 2.7],
      },
    } as never);
  }
  if (!map.getLayer("links-focus")) {
    map.addLayer({
      id: "links-focus",
      type: "line",
      source: SRC_LINKS,
      minzoom: 10,
      filter: ["==", ["get", "in_focus"], 1],
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#0f2d4f"],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 10, 0.55, 15, 0.95],
        "line-width": ["interpolate", ["linear"], ["zoom"], 10, 1.5, 15, 5.0],
      },
    } as never);
  }
  if (!map.getLayer("transfers-focus")) {
    map.addLayer({
      id: "transfers-focus",
      type: "line",
      source: SRC_TRANSFERS,
      minzoom: 10.8,
      filter: ["==", ["get", "in_focus"], 1],
      paint: {
        "line-color": "#7d8897",
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 10.8, 0.24, 15, 0.62],
        "line-width": ["interpolate", ["linear"], ["zoom"], 10.8, 0.8, 15, 2.1],
        "line-dasharray": [0.8, 1.0],
      },
    } as never);
  }
  if (!map.getLayer("transfers-focus-label")) {
    map.addLayer({
      id: "transfers-focus-label",
      type: "symbol",
      source: SRC_TRANSFERS,
      minzoom: 12.4,
      filter: ["==", ["get", "in_focus"], 1],
      layout: {
        "symbol-placement": "line-center",
        "text-field": ["coalesce", ["get", "transfer_label"], ""],
        "text-font": ["Noto Sans Regular"],
        "text-size": ["interpolate", ["linear"], ["zoom"], 12.4, 9, 15, 11],
      },
      paint: {
        "text-color": "rgba(78,90,108,0.9)",
        "text-halo-color": "rgba(255,255,255,0.92)",
        "text-halo-width": 1.2,
      },
    } as never);
  }
  if (!map.getLayer("links-selected-glow")) {
    map.addLayer({
      id: "links-selected-glow",
      type: "line",
      source: SRC_LINKS,
      filter: EMPTY_LINE_FILTER as never,
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#6ddcff"],
        "line-opacity": 0.52,
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 9.4, 13, 18.8, 15, 22.0],
      },
    } as never);
  }
  if (!map.getLayer("links-selected-casing")) {
    map.addLayer({
      id: "links-selected-casing",
      type: "line",
      source: SRC_LINKS,
      filter: EMPTY_LINE_FILTER as never,
      paint: {
        "line-color": "#ffffff",
        "line-opacity": 0.92,
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 6.0, 13, 11.4, 15, 13.6],
      },
    } as never);
  }
  if (!map.getLayer("links-selected")) {
    map.addLayer({
      id: "links-selected",
      type: "line",
      source: SRC_LINKS,
      filter: EMPTY_LINE_FILTER as never,
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#ffd15e"],
        "line-opacity": 1.0,
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 4.4, 13, 7.8, 15, 9.6],
      },
    } as never);
  }
  if (!map.getLayer("links-selection-dim")) {
    map.addLayer({
      id: "links-selection-dim",
      type: "line",
      source: SRC_LINKS,
      filter: EMPTY_LINE_FILTER as never,
      paint: {
        "line-color": "#7f93aa",
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 6, 0.16, 13, 0.28, 15, 0.34],
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 1.8, 13, 2.8, 15, 3.2],
      },
    } as never);
  }
  if (!map.getLayer("links-active")) {
    map.addLayer({
      id: "links-active",
      type: "line",
      source: SRC_LINKS,
      filter: EMPTY_LINE_FILTER as never,
      paint: {
        "line-color": "#6ddcff",
        "line-opacity": 0.96,
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 2.4, 13, 4.8, 15, 6.1],
        "line-dasharray": [0.9, 1.3],
      },
    } as never);
  }
  if (!map.getLayer("links-hit")) {
    map.addLayer({
      id: "links-hit",
      type: "line",
      source: SRC_LINKS,
      paint: {
        "line-color": "#000000",
        "line-opacity": 0,
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 19, 10, 26, 15, 34],
      },
    } as never);
  }
  if (!map.getLayer("stops-major")) {
    map.addLayer({
      id: "stops-major",
      type: "circle",
      source: SRC_STOPS,
      filter: ["==", ["get", "major_interchange"], 1],
      paint: {
        "circle-color": "#143759",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 4, 2.0, 12, 5.5],
        "circle-stroke-width": 1.0,
        "circle-stroke-color": "#ffffff",
      },
    } as never);
  }
  if (!map.getLayer("stops-hit")) {
    map.addLayer({
      id: "stops-hit",
      type: "circle",
      source: SRC_STOPS,
      filter: ["!=", ["get", "shape_stop"], 1],
      paint: {
        "circle-color": "#000000",
        "circle-opacity": 0,
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 4, 16, 10, 24, 15, 30],
      },
    } as never);
  }
  if (!map.getLayer("stops-focus")) {
    map.addLayer({
      id: "stops-focus",
      type: "circle",
      source: SRC_STOPS,
      minzoom: 10,
      filter: ["==", ["get", "in_focus"], 1],
      paint: {
        "circle-color": ["coalesce", ["get", "display_color"], "#315f8d"],
        "circle-radius": [
          "interpolate",
          ["linear"],
          ["zoom"],
          10,
          ["match", ["get", "mode_class"], "metro", 2.2, "tram", 2.0, "bus", 1.7, "ferry", 2.1, "commuter_rail", 2.4, "high_speed_rail", 2.6, "rail", 2.3, 1.8],
          15,
          ["match", ["get", "mode_class"], "metro", 6.2, "tram", 5.8, "bus", 5.1, "ferry", 6.0, "commuter_rail", 6.5, "high_speed_rail", 6.8, "rail", 6.4, 5.5]
        ],
        "circle-opacity": ["interpolate", ["linear"], ["zoom"], 10, 0.65, 15, 0.92],
        "circle-stroke-width": ["match", ["get", "mode_class"], "bus", 1.2, "tram", 1.4, "metro", 1.7, "commuter_rail", 1.9, "high_speed_rail", 2.1, "rail", 1.8, 1.3],
        "circle-stroke-color": "#ffffff",
      },
    } as never);
  }
  if (!map.getLayer("stops-focus-symbol")) {
    map.addLayer({
      id: "stops-focus-symbol",
      type: "symbol",
      source: SRC_STOPS,
      minzoom: 10,
      filter: ["all", ["==", ["get", "in_focus"], 1], ["!=", ["get", "shape_stop"], 1]] as never,
      layout: {
        "text-field": ["coalesce", ["get", "stop_symbol"], "●"],
        "text-font": ["Noto Sans Bold"],
        "text-size": [
          "interpolate",
          ["linear"],
          ["zoom"],
          10,
          [
            "match",
            ["get", "mode_class"],
            "bus",
            11,
            "tram",
            11,
            "metro",
            12,
            "ferry",
            12,
            "commuter_rail",
            12,
            "high_speed_rail",
            13,
            "rail",
            12,
            11,
          ],
          15,
          [
            "match",
            ["get", "mode_class"],
            "bus",
            15,
            "tram",
            15,
            "metro",
            16,
            "ferry",
            16,
            "commuter_rail",
            16,
            "high_speed_rail",
            17,
            "rail",
            16,
            15,
          ]
        ],
        "text-allow-overlap": true,
      },
      paint: {
        "text-color": ["coalesce", ["get", "display_color"], "#315f8d"],
        "text-halo-color": "rgba(255,255,255,0.94)",
        "text-halo-width": 1.4,
      },
    } as never);
  }
  if (!map.getLayer("stops-focus-badge")) {
    map.addLayer({
      id: "stops-focus-badge",
      type: "symbol",
      source: SRC_STOPS,
      minzoom: 13.2,
      filter: ["all", ["==", ["get", "in_focus"], 1], ["!=", ["get", "shape_stop"], 1]] as never,
      layout: {
        "text-field": ["coalesce", ["get", "stop_badge"], "S"],
        "text-font": ["Noto Sans Bold"],
        "text-size": ["interpolate", ["linear"], ["zoom"], 13.2, 7, 15.5, 9.5],
        "text-allow-overlap": true,
      },
      paint: {
        "text-color": "#ffffff",
        "text-halo-color": "rgba(16,39,63,0.9)",
        "text-halo-width": 1.0,
      },
    } as never);
  }
  if (!map.getLayer("stops-selected-halo")) {
    map.addLayer({
      id: "stops-selected-halo",
      type: "circle",
      source: SRC_STOPS,
      filter: EMPTY_STOP_FILTER as never,
      paint: {
        "circle-color": ["coalesce", ["get", "display_color"], "#ff8b00"],
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 7.6, 13, 13, 15, 16],
        "circle-opacity": 0.34,
      },
    } as never);
  }
  if (!map.getLayer("stops-build-hover-ring")) {
    map.addLayer({
      id: "stops-build-hover-ring",
      type: "circle",
      source: SRC_STOPS,
      filter: EMPTY_STOP_FILTER as never,
      paint: {
        "circle-color": "rgba(0,0,0,0)",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 8.8, 13, 15.4, 15, 18.6],
        "circle-stroke-width": 3.2,
        "circle-stroke-color": ["coalesce", ["get", "display_color"], "#ffd15e"],
        "circle-opacity": 1,
      },
    } as never);
  }
  if (!map.getLayer("stops-selected")) {
    map.addLayer({
      id: "stops-selected",
      type: "circle",
      source: SRC_STOPS,
      filter: EMPTY_STOP_FILTER as never,
      paint: {
        "circle-color": ["coalesce", ["get", "display_color"], "#ff8b00"],
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 4.8, 13, 8.8, 15, 10.8],
        "circle-stroke-width": 2.4,
        "circle-stroke-color": "#ffffff",
      },
    } as never);
  }
  if (!map.getLayer("stops-selected-line-ring")) {
    map.addLayer({
      id: "stops-selected-line-ring",
      type: "circle",
      source: SRC_STOPS,
      filter: EMPTY_STOP_FILTER as never,
      paint: {
        "circle-color": "rgba(0,0,0,0)",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 8.6, 13, 14.2, 15, 17.4],
        "circle-stroke-width": 3.0,
        "circle-stroke-color": ["coalesce", ["get", "display_color"], "#6ddcff"],
        "circle-opacity": 0.98,
      },
    } as never);
  }
  if (!map.getLayer("stops-shape")) {
    map.addLayer({
      id: "stops-shape",
      type: "circle",
      source: SRC_STOPS,
      filter: ["==", ["get", "shape_stop"], 1],
      paint: {
        "circle-color": "#a06a49",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 10, 1.0, 15, 2.8],
        "circle-opacity": 0.8,
      },
    } as never);
  }
  if (!map.getLayer("vehicles-selected-halo")) {
    map.addLayer({
      id: "vehicles-selected-halo",
      type: "circle",
      source: SRC_VEHICLES,
      filter: EMPTY_VEHICLE_FILTER as never,
      paint: {
        "circle-color": ["coalesce", ["get", "display_color"], "#7de2ff"],
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 8, 8, 14, 13, 16, 16],
        "circle-opacity": 0.34,
      },
    } as never);
  }
  if (!map.getLayer("vehicles-point")) {
    map.addLayer({
      id: "vehicles-point",
      type: "circle",
      source: SRC_VEHICLES,
      minzoom: 4,
      paint: {
        "circle-color": ["coalesce", ["get", "display_color"], "#1f3e63"],
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 4, 2.0, 10, 3.8, 14, 5.2, 16, 6.3],
        "circle-stroke-width": 1.25,
        "circle-stroke-color": "#ffffff",
        "circle-opacity": ["interpolate", ["linear"], ["zoom"], 4, 0.8, 14, 0.96],
      },
    } as never);
  }
  if (!map.getLayer("vehicles-hit")) {
    map.addLayer({
      id: "vehicles-hit",
      type: "circle",
      source: SRC_VEHICLES,
      paint: {
        "circle-color": "#000000",
        "circle-opacity": 0,
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 8, 10, 14, 14, 16, 17],
      },
    } as never);
  }
  if (!map.getLayer("zone-centroids")) {
    map.addLayer({
      id: "zone-centroids",
      type: "circle",
      source: SRC_ZONES,
      paint: {
        "circle-color": "#8c7a5f",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 8, 1.2, 15, 3.4],
        "circle-opacity": 0.55,
      },
    } as never);
  }
  if (!map.getLayer("build-preview-line")) {
    map.addLayer({
      id: "build-preview-line",
      type: "line",
      source: SRC_BUILD_PREVIEW,
      filter: ["==", ["get", "kind"], "line"],
      paint: {
        "line-color": ["coalesce", ["get", "display_color"], "#104894"],
        "line-opacity": 0.95,
        "line-width": ["interpolate", ["linear"], ["zoom"], 6, 2.0, 13, 4.5, 15, 6.0],
        "line-dasharray": [1.2, 1.2],
      },
    } as never);
  }
  if (!map.getLayer("build-preview-point")) {
    map.addLayer({
      id: "build-preview-point",
      type: "circle",
      source: SRC_BUILD_PREVIEW,
      filter: ["==", ["get", "kind"], "point"],
      paint: {
        "circle-color": "#ffffff",
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 4.5, 13, 7.0, 15, 8.5],
        "circle-opacity": 0.94,
        "circle-stroke-width": 2.2,
        "circle-stroke-color": ["coalesce", ["get", "display_color"], "#104894"],
      },
    } as never);
  }
  if (!map.getLayer("county-selected-outline")) {
    map.addLayer({
      id: "county-selected-outline",
      type: "line",
      source: SRC_COUNTIES,
      filter: ["==", ["get", "selected"], 1],
      paint: { "line-color": "#2a6bbb", "line-width": ["interpolate", ["linear"], ["zoom"], 4, 1.4, 11, 3.2] },
    } as never);
  }
}
