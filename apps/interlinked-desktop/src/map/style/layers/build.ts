import type { Map as MapLibreMap } from "maplibre-gl";

import { EMPTY_STOP_FILTER } from "../common";
import { SRC_BUILD_PREVIEW, SRC_STOPS } from "../sources";

export function ensureStopSelectionLayers(map: MapLibreMap): void {
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
        "circle-radius": ["interpolate", ["linear"], ["zoom"], 6, 5.8, 13, 8.8, 15, 10.2],
        "circle-stroke-width": ["interpolate", ["linear"], ["zoom"], 6, 1.2, 13, 1.7, 15, 2.0],
        "circle-stroke-color": "#ffffff",
        "circle-opacity": 0.94,
      },
    } as never);
  }
}

export function ensureBuildPreviewLayers(map: MapLibreMap): void {
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
}
