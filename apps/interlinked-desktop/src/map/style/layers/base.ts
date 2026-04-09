import type { Map as MapLibreMap } from "maplibre-gl";

import { SRC_OCEAN, SRC_WORLD } from "../sources";

export function ensureBaseLayers(map: MapLibreMap): void {
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
}
