import type { Map as MapLibreMap } from "maplibre-gl";

import { SRC_WORLD } from "../sources";

export function ensureBaseLayers(map: MapLibreMap): void {
  const localDetailAnchor =
    (map.getLayer("road-casing") ? "road-casing" : null) ??
    (map.getLayer("road-main") ? "road-main" : null) ??
    (map.getLayer("place-city-major") ? "place-city-major" : null);

  if (!map.getLayer("world-country-fill")) {
    map.addLayer(
      {
        id: "world-country-fill",
        type: "fill",
        source: SRC_WORLD,
        maxzoom: 11.8,
        paint: {
          "fill-color": [
            "case",
            ["==", ["get", "unlocked_now"], 1],
            "#edf4ff",
            "#c9d2dd",
          ],
          // Fade earlier so UK local roads/places stay visually legible under progression overlays.
          "fill-opacity": [
            "interpolate",
            ["linear"],
            ["zoom"],
            2.2,
            ["case", ["==", ["get", "unlocked_now"], 1], 0.64, 0.8],
            6.8,
            ["case", ["==", ["get", "unlocked_now"], 1], 0.24, 0.42],
            9.2,
            ["case", ["==", ["get", "unlocked_now"], 1], 0.08, 0.14],
            11.2,
            0,
          ],
        },
      } as never,
      localDetailAnchor ?? undefined
    );
  }
  if (!map.getLayer("world-country-outline")) {
    map.addLayer(
      {
        id: "world-country-outline",
        type: "line",
        source: SRC_WORLD,
        maxzoom: 12.5,
        paint: {
          "line-color": "#9ca9b7",
          "line-width": ["interpolate", ["linear"], ["zoom"], 2, 0.5, 6, 0.9, 11, 0.7],
          "line-opacity": [
            "interpolate",
            ["linear"],
            ["zoom"],
            2,
            0.86,
            8.5,
            0.42,
            10.8,
            0.24,
            12.5,
            0.08,
          ],
        },
      } as never,
      localDetailAnchor ?? undefined
    );
  }
}
