import type { OverlayGeoCollection } from "../runtimeVehicleOverlay";
import { xyToLngLat } from "../geo/coords";
import { fc, type GeoCollection, type GeoFeature } from "./contracts";

export type BuildInteractionMode = "view" | "build";
export type BuildActionMode =
  | "select"
  | "place_station"
  | "start_line"
  | "add_station_to_line"
  | "delete";

export function buildBuildPreviewOverlay(args: {
  interactionMode?: BuildInteractionMode;
  buildAction?: BuildActionMode;
  previewAnchorPoint?: { x: number; y: number } | null;
  previewColor?: string | null;
  hoverPoint: { lng: number; lat: number; x: number; y: number } | null;
  crsType?: string;
}): { geojson: GeoCollection; showPreviewLine: boolean; showPreviewPoint: boolean } {
  const {
    interactionMode,
    buildAction = "select",
    previewAnchorPoint,
    previewColor,
    hoverPoint,
    crsType,
  } = args;
  const isBuildMode = interactionMode === "build";
  const showPreviewPoint =
    isBuildMode &&
    (buildAction === "place_station" ||
      buildAction === "start_line" ||
      buildAction === "add_station_to_line");
  const showPreviewLine =
    isBuildMode &&
    (buildAction === "start_line" || buildAction === "add_station_to_line") &&
    Boolean(previewAnchorPoint) &&
    Boolean(hoverPoint);

  const previewFeatures: GeoFeature[] = [];
  if (showPreviewLine && previewAnchorPoint && hoverPoint) {
    const from = xyToLngLat(previewAnchorPoint.x, previewAnchorPoint.y, crsType);
    if (from) {
      previewFeatures.push({
        type: "Feature",
        geometry: {
          type: "LineString",
          coordinates: [
            [from.lng, from.lat],
            [hoverPoint.lng, hoverPoint.lat],
          ],
        },
        properties: { kind: "line", display_color: previewColor ?? "#104894" },
      });
    }
  }
  if (showPreviewPoint && hoverPoint) {
    previewFeatures.push({
      type: "Feature",
      geometry: { type: "Point", coordinates: [hoverPoint.lng, hoverPoint.lat] },
      properties: { kind: "point", display_color: previewColor ?? "#104894" },
    });
  }

  return {
    geojson: fc(previewFeatures),
    showPreviewLine,
    showPreviewPoint,
  };
}

export function activeVehicleOverlayCollection(args: {
  gameMode: boolean;
  trainsAuthoritative?: boolean;
  runtimeVehicleGeoJson: OverlayGeoCollection;
  vehicleGeoJson: OverlayGeoCollection;
}): OverlayGeoCollection {
  const { gameMode, trainsAuthoritative, runtimeVehicleGeoJson, vehicleGeoJson } = args;
  return gameMode || trainsAuthoritative ? runtimeVehicleGeoJson : vehicleGeoJson;
}
