import type { Map as MapLibreMap } from "maplibre-gl";

import { SRC_DEMAND_CELLS } from "../sources";

export function ensureDemandOverlayLayers(map: MapLibreMap): void {
  if (!map.getLayer("demand-cell-fill")) {
    map.addLayer({
      id: "demand-cell-fill",
      type: "fill",
      source: SRC_DEMAND_CELLS,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "fill-color": [
          "interpolate",
          ["linear"],
          ["coalesce", ["get", "overlay_norm"], 0],
          0,
          "#d8f3c6",
          0.22,
          "#9bd77b",
          0.45,
          "#f0d35a",
          0.68,
          "#f39a3d",
          0.86,
          "#dc5f34",
          1,
          "#b91f2c",
        ],
        "fill-opacity": [
          "interpolate",
          ["linear"],
          ["zoom"],
          5.2,
          [
            "interpolate",
            ["linear"],
            ["coalesce", ["get", "overlay_norm"], 0],
            0,
            0.16,
            1,
            0.38,
          ],
          9.5,
          [
            "interpolate",
            ["linear"],
            ["coalesce", ["get", "overlay_norm"], 0],
            0,
            0.22,
            1,
            0.52,
          ],
          13,
          [
            "interpolate",
            ["linear"],
            ["coalesce", ["get", "overlay_norm"], 0],
            0,
            0.28,
            1,
            0.66,
          ],
        ],
      },
    } as never);
  }

  if (!map.getLayer("demand-cell-outline")) {
    map.addLayer({
      id: "demand-cell-outline",
      type: "line",
      source: SRC_DEMAND_CELLS,
      minzoom: 5.2,
      layout: { visibility: "none" },
      paint: {
        "line-color": "#42624f",
        "line-opacity": ["interpolate", ["linear"], ["zoom"], 5.2, 0.2, 10.5, 0.32, 14, 0.48],
        "line-width": ["interpolate", ["linear"], ["zoom"], 5.2, 0.22, 10.5, 0.46, 14, 0.82],
      },
    } as never);
  }

  if (!map.getLayer("demand-cell-selected-outline")) {
    map.addLayer({
      id: "demand-cell-selected-outline",
      type: "line",
      source: SRC_DEMAND_CELLS,
      minzoom: 5.2,
      layout: { visibility: "none" },
      filter: ["==", ["get", "cell_id"], "__none__"],
      paint: {
        "line-color": "#ffe082",
        "line-opacity": 0.98,
        "line-width": ["interpolate", ["linear"], ["zoom"], 5.2, 0.9, 10.5, 1.8, 14, 2.8],
      },
    } as never);
  }

  if (!map.getLayer("demand-cell-hit")) {
    map.addLayer({
      id: "demand-cell-hit",
      type: "fill",
      source: SRC_DEMAND_CELLS,
      minzoom: 4.8,
      layout: { visibility: "none" },
      paint: {
        "fill-color": "rgba(0,0,0,0)",
        "fill-opacity": 0.0001,
      },
    } as never);
  }
}
