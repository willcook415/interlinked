import type { Map as MapLibreMap } from "maplibre-gl";

import {
  landuseFilter,
  localRoadFilter,
  localRoadOpacityExpression,
  railFilter,
  roadCasingWidthExpression,
  roadColorExpression,
  roadWidthExpression,
  waterFilter,
} from "../common";
import {
  SRC_REGIONS,
  SRC_COUNTY_BASEMAP,
  SRC_HEX_COVERAGE_GAPS,
  SRC_MAJOR_ROADS,
  SRC_REGION_HEXES,
  SRC_ZONES,
} from "../sources";

export function ensureRegionBasemapLayers(map: MapLibreMap): void {
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
  // Compatibility-only GeoJSON basemap layers (legacy non-vector map tiers).
  // Active UK gameplay render path is vector style + runtime region overlays.
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
}

export function ensureRegionBoundaryLayers(map: MapLibreMap): void {
  const localDetailAnchor =
    (map.getLayer("road-casing") ? "road-casing" : null) ??
    (map.getLayer("road-main") ? "road-main" : null) ??
    (map.getLayer("place-city-major") ? "place-city-major" : null);

  if (!map.getLayer("region-fill")) {
    map.addLayer(
      {
        id: "region-fill",
        type: "fill",
        source: SRC_REGIONS,
        paint: {
          "fill-color": [
            "case",
            ["==", ["get", "focus"], 1],
            "#d9eafe",
            ["==", ["get", "unlocked"], 1],
            "#eaf4ff",
            "#9ca8b6",
          ],
          "fill-opacity": [
            "interpolate",
            ["linear"],
            ["zoom"],
            4,
            [
              "case",
              ["==", ["get", "focus"], 1],
              0.14,
              ["==", ["get", "unlocked"], 1],
              0.06,
              0.12,
            ],
            8,
            [
              "case",
              ["==", ["get", "focus"], 1],
              0.1,
              ["==", ["get", "unlocked"], 1],
              0.045,
              0.085,
            ],
            11.5,
            [
              "case",
              ["==", ["get", "focus"], 1],
              0.07,
              ["==", ["get", "unlocked"], 1],
              0.03,
              0.055,
            ],
            14,
            [
              "case",
              ["==", ["get", "focus"], 1],
              0.05,
              ["==", ["get", "unlocked"], 1],
              0.02,
              0.04,
            ],
          ],
        },
      } as never,
      localDetailAnchor ?? undefined
    );
  }
  if (!map.getLayer("region-outline")) {
    map.addLayer({
      id: "region-outline",
      type: "line",
      source: SRC_REGIONS,
      paint: {
        "line-color": [
          "case",
          ["==", ["get", "unlocked"], 1],
          "#6f859d",
          "#546478",
        ],
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 0.8, 8.5, 1.1, 11.5, 1.8],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 4, 0.52, 8.5, 0.74, 11.5, 0.88],
      },
    } as never);
  }
  if (!map.getLayer("region-focus-outline")) {
    map.addLayer({
      id: "region-focus-outline",
      type: "line",
      source: SRC_REGIONS,
      filter: ["==", ["get", "focus"], 1],
      paint: {
        "line-color": "#2560a8",
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 1.4, 11, 3.1],
      },
    } as never);
  }
  if (!map.getLayer("region-unlocked-outline")) {
    map.addLayer({
      id: "region-unlocked-outline",
      type: "line",
      source: SRC_REGIONS,
      filter: ["==", ["get", "unlocked"], 1],
      paint: {
        "line-color": "rgba(56,116,183,0.78)",
        "line-width": ["interpolate", ["linear"], ["zoom"], 4, 0.8, 11, 1.6],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 4, 0.36, 11, 0.68],
      },
    } as never);
  }
}

export function ensureZoneCentroidLayer(map: MapLibreMap): void {
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
}

export function ensureRegionSelectedOutlineLayer(map: MapLibreMap): void {
  if (!map.getLayer("region-selected-outline")) {
    map.addLayer({
      id: "region-selected-outline",
      type: "line",
      source: SRC_REGIONS,
      filter: ["==", ["get", "selected"], 1],
      paint: { "line-color": "#2a6bbb", "line-width": ["interpolate", ["linear"], ["zoom"], 4, 1.4, 11, 3.2] },
    } as never);
  }
}

export function ensurePlanningHexAuthoringLayers(map: MapLibreMap): void {
  if (!map.getLayer("planning-hex-fill")) {
    map.addLayer({
      id: "planning-hex-fill",
      type: "fill",
      source: SRC_REGION_HEXES,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "fill-color": [
          "case",
          ["==", ["get", "hex_unassigned"], 1],
          "rgba(210,52,52,0.40)",
          ["==", ["get", "hex_manual_assigned"], 1],
          "rgba(63,111,171,0.18)",
          ["==", ["get", "unlocked"], 1],
          "rgba(88,112,142,0.18)",
          "rgba(108,118,130,0.13)",
        ],
        "fill-opacity": [
          "interpolate",
          ["linear"],
          ["zoom"],
          5.2,
          ["case", ["==", ["get", "hex_unassigned"], 1], 0.18, 0.06],
          9.5,
          ["case", ["==", ["get", "hex_unassigned"], 1], 0.28, 0.09],
          13,
          ["case", ["==", ["get", "hex_unassigned"], 1], 0.36, 0.14],
        ],
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-outline")) {
    map.addLayer({
      id: "planning-hex-outline",
      type: "line",
      source: SRC_REGION_HEXES,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "line-color": [
          "case",
          ["==", ["get", "hex_unassigned"], 1],
          "rgba(188,33,33,0.96)",
          ["==", ["get", "hex_manual_assigned"], 1],
          "rgba(32,84,145,0.88)",
          ["==", ["get", "unlocked"], 1],
          "rgba(35,88,150,0.84)",
          "rgba(70,82,99,0.78)",
        ],
        "line-width": [
          "interpolate",
          ["linear"],
          ["zoom"],
          5.2,
          ["case", ["==", ["get", "hex_unassigned"], 1], 0.9, 0.48],
          9.5,
          ["case", ["==", ["get", "hex_unassigned"], 1], 1.5, 0.84],
          13,
          ["case", ["==", ["get", "hex_unassigned"], 1], 2.2, 1.4],
        ],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 5.2, 0.55, 9.5, 0.78, 13, 0.94],
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-number")) {
    map.addLayer({
      id: "planning-hex-number",
      type: "symbol",
      source: SRC_REGION_HEXES,
      minzoom: 7.2,
      layout: {
        visibility: "none",
        "text-field": ["to-string", ["coalesce", ["get", "hex_num"], ""]],
        "text-font": ["Noto Sans Bold"],
        "text-size": ["interpolate", ["linear"], ["zoom"], 7.2, 9.5, 10.5, 11.5, 14, 13.5],
        "text-allow-overlap": true,
        "text-ignore-placement": true,
        "text-padding": 1,
      },
      paint: {
        "text-color": [
          "case",
          ["==", ["get", "hex_unassigned"], 1],
          "#8e1e1e",
          "#0f355d",
        ],
        "text-halo-color": "rgba(255,255,255,0.99)",
        "text-halo-width": 1.7,
        "text-opacity": ["interpolate", ["linear"], ["zoom"], 7.2, 0.82, 10.5, 0.96],
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-selected-outline")) {
    map.addLayer({
      id: "planning-hex-selected-outline",
      type: "line",
      source: SRC_REGION_HEXES,
      minzoom: 5.2,
      layout: { visibility: "none" },
      filter: ["==", ["get", "hex_id"], "__none__"],
      paint: {
        "line-color": "#0f5cae",
        "line-width": ["interpolate", ["linear"], ["zoom"], 5.2, 1.0, 11.5, 2.3],
        "line-opacity": 0.98,
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-hover-outline")) {
    map.addLayer({
      id: "planning-hex-hover-outline",
      type: "line",
      source: SRC_REGION_HEXES,
      minzoom: 5.2,
      layout: { visibility: "none" },
      filter: ["==", ["get", "hex_id"], "__none__"],
      paint: {
        "line-color": "#f7b046",
        "line-width": ["interpolate", ["linear"], ["zoom"], 5.2, 0.9, 11.5, 2.0],
        "line-opacity": 0.96,
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-hit")) {
    map.addLayer({
      id: "planning-hex-hit",
      type: "fill",
      source: SRC_REGION_HEXES,
      minzoom: 4.5,
      layout: { visibility: "none" },
      paint: {
        "fill-color": "rgba(0,0,0,0)",
        "fill-opacity": 0.0001,
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-gap-fill")) {
    map.addLayer({
      id: "planning-hex-gap-fill",
      type: "fill",
      source: SRC_HEX_COVERAGE_GAPS,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "fill-color": "rgba(222,40,40,0.28)",
        "fill-opacity": ["interpolate", ["linear"], ["zoom"], 5.2, 0.1, 9.5, 0.16, 13, 0.22],
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-gap-hit")) {
    map.addLayer({
      id: "planning-hex-gap-hit",
      type: "fill",
      source: SRC_HEX_COVERAGE_GAPS,
      minzoom: 4.5,
      layout: { visibility: "none" },
      paint: {
        "fill-color": "rgba(0,0,0,0)",
        "fill-opacity": 0.0001,
      },
    } as never);
  }
  if (!map.getLayer("planning-hex-gap-outline")) {
    map.addLayer({
      id: "planning-hex-gap-outline",
      type: "line",
      source: SRC_HEX_COVERAGE_GAPS,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "line-color": "rgba(168,14,14,0.98)",
        "line-width": ["interpolate", ["linear"], ["zoom"], 5.2, 0.85, 9.5, 1.35, 13, 1.95],
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 5.2, 0.62, 9.5, 0.86, 13, 0.98],
      },
    } as never);
  }
  // Keep active authoring affordances above coverage-gap overlays.
  if (map.getLayer("planning-hex-number")) map.moveLayer("planning-hex-number");
  if (map.getLayer("planning-hex-selected-outline")) map.moveLayer("planning-hex-selected-outline");
  if (map.getLayer("planning-hex-hover-outline")) map.moveLayer("planning-hex-hover-outline");
}
