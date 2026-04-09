import type { Map as MapLibreMap } from "maplibre-gl";

import { EMPTY_LINE_FILTER } from "../common";
import { SRC_LINKS, SRC_STOPS, SRC_TRANSFERS } from "../sources";

export function ensureNetworkCoreLayers(map: MapLibreMap): void {
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
}

export function ensureNetworkSelectionLayers(map: MapLibreMap): void {
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
}

export function ensureNetworkHitLayer(map: MapLibreMap): void {
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
}

export function ensureStopNetworkLayers(map: MapLibreMap): void {
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
}

export function ensureStopShapeLayer(map: MapLibreMap): void {
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
}
