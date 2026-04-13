import type { Map as MapLibreMap } from "maplibre-gl";

import { ensureMapLayerOrder } from "./layerOrder";
import { EMPTY_LINE_FILTER, EMPTY_STOP_FILTER, EMPTY_VEHICLE_FILTER } from "./common";
import {
  ensureMapSources,
  SRC_BUILD_PREVIEW,
  SRC_REGIONS,
  SRC_REGION_HEXES,
  SRC_HEX_COVERAGE_GAPS,
  SRC_COUNTY_BASEMAP,
  SRC_LINKS,
  SRC_MAJOR_ROADS,
  SRC_OCEAN,
  SRC_OCEAN_LABELS,
  SRC_STOPS,
  SRC_TRANSFERS,
  SRC_VEHICLES,
  SRC_WORLD,
  SRC_WORLD_LABELS,
  SRC_ZONES,
} from "./sources";

export {
  EMPTY_LINE_FILTER,
  EMPTY_STOP_FILTER,
  EMPTY_VEHICLE_FILTER,
  SRC_BUILD_PREVIEW,
  SRC_REGIONS,
  SRC_REGION_HEXES,
  SRC_HEX_COVERAGE_GAPS,
  SRC_COUNTY_BASEMAP,
  SRC_LINKS,
  SRC_MAJOR_ROADS,
  SRC_OCEAN,
  SRC_OCEAN_LABELS,
  SRC_STOPS,
  SRC_TRANSFERS,
  SRC_VEHICLES,
  SRC_WORLD,
  SRC_WORLD_LABELS,
  SRC_ZONES,
};

export function ensureMapLayers(map: MapLibreMap): void {
  ensureMapSources(map);
  ensureMapLayerOrder(map);
}
