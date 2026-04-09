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
import { SRC_COUNTIES, SRC_COUNTY_BASEMAP, SRC_MAJOR_ROADS, SRC_ZONES } from "../sources";

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

export function ensureCountySelectedOutlineLayer(map: MapLibreMap): void {
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
