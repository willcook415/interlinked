import type { Map as MapLibreMap } from "maplibre-gl";

import { SRC_OCEAN_LABELS, SRC_STOPS, SRC_TRANSFERS, SRC_WORLD_LABELS } from "../sources";

export function ensureWorldLabelLayers(map: MapLibreMap): void {
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
}

export function ensureTransferFocusLabelLayer(map: MapLibreMap): void {
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
}

export function ensureStopFocusLabelLayers(map: MapLibreMap): void {
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
}
