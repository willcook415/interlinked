import type { Map as MapLibreMap } from "maplibre-gl";

import { EMPTY_VEHICLE_FILTER } from "../common";
import { SRC_VEHICLES } from "../sources";

export function ensureRuntimeLayers(map: MapLibreMap): void {
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
}
