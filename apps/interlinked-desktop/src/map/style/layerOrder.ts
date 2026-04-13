import type { Map as MapLibreMap } from "maplibre-gl";

import { ensureBaseLayers } from "./layers/base";
import { ensureBuildPreviewLayers, ensureStopSelectionLayers } from "./layers/build";
import {
  ensureStopFocusLabelLayers,
  ensureTransferFocusLabelLayer,
  ensureWorldLabelLayers,
} from "./layers/labels";
import {
  ensureNetworkCoreLayers,
  ensureNetworkHitLayer,
  ensureNetworkSelectionLayers,
  ensureStopNetworkLayers,
  ensureStopShapeLayer,
} from "./layers/network";
import {
  ensureRegionSelectedOutlineLayer,
  ensurePlanningHexAuthoringLayers,
  ensureRegionBasemapLayers,
  ensureRegionBoundaryLayers,
  ensureZoneCentroidLayer,
} from "./layers/regions";
import { ensureRuntimeLayers } from "./layers/runtime";

export function ensureMapLayerOrder(map: MapLibreMap): void {
  ensureBaseLayers(map);
  ensureWorldLabelLayers(map);
  ensureRegionBasemapLayers(map);
  ensureRegionBoundaryLayers(map);
  ensurePlanningHexAuthoringLayers(map);
  ensureNetworkCoreLayers(map);
  ensureTransferFocusLabelLayer(map);
  ensureNetworkSelectionLayers(map);
  ensureNetworkHitLayer(map);
  ensureStopNetworkLayers(map);
  ensureStopFocusLabelLayers(map);
  ensureStopSelectionLayers(map);
  ensureStopShapeLayer(map);
  ensureRuntimeLayers(map);
  ensureZoneCentroidLayer(map);
  ensureBuildPreviewLayers(map);
  ensureRegionSelectedOutlineLayer(map);
}
