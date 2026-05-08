import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import maplibregl, {
  type GeoJSONSource,
  type LngLatBoundsLike,
  type Map as MapLibreMap,
  type PointLike,
} from "maplibre-gl";
import type { LinkModeFilter } from "./ui/MapFiltersPanel";
import {
  EMPTY_LINE_FILTER,
  EMPTY_STOP_FILTER,
  SRC_BUILD_PREVIEW,
  SRC_DEMAND_CELLS,
  SRC_HEX_COVERAGE_GAPS,
  SRC_LINKS,
  SRC_MAJOR_ROADS,
  SRC_REGIONS,
  SRC_REGION_HEXES,
  SRC_STOPS,
  SRC_TRANSFERS,
  SRC_VEHICLES,
  SRC_WORLD,
  SRC_WORLD_LABELS,
  SRC_ZONES,
  ensureMapLayers,
} from "./map/style/ensureMapLayers";
import {
  buildServiceById,
  buildVehicleData,
  buildVehicleRouteSeeds,
  minuteOfDayFromClock,
  overlayCollection,
  reconcileRuntimeVehicleGeoJson,
  serviceLineId,
  type OverlayGeoCollection,
  type OverlayGeoFeature,
  type VehicleSnapshot,
} from "./map/runtimeVehicleOverlay";
import type {
  CountryMapContext,
  DemandOverlayPayload,
  DemandOverlayType,
  GeoJsonFeatureCollection,
  GeoJsonGeometry,
  MapRuntimeConfig,
  RegionStatus,
  ScenarioLite,
  SessionKind,
  SimulationClock,
  TrainRuntimeView,
} from "./types";
import { loadCountryMapContext } from "./api/desktopApi";
import { type GeoCollection, fc } from "./map/data/contracts";
import { lngLatToXY, type XY, xyToLngLat } from "./map/geo/coords";
import {
  boundsFromGeometry,
  pointInGeometry,
} from "./map/geo/geometry";
import {
  buildCountyFeatures,
  buildCountyLabelData,
  buildHexCoverageGapFeatures,
  buildPlanningHexFeatures,
  type UkHexCoverageDiagnostics,
  buildUkHexCoverageDiagnostics,
  buildWorldCountryData,
  buildWorldCountryLabelData,
  buildRegionDisplayFeatures,
  fetchFeatureCollection,
  normalizeBasemapFeatureCollection,
  parseRegionGeometry,
} from "./map/data/worldContext";
import { buildRegionDisplayNames } from "./map/data/regionPresentation";
import {
  buildNetworkGeojsonData,
  buildStopPointById,
  formatDistanceKm,
  modeColor,
} from "./map/data/networkGeojson";
import { buildDemandOverlayGeojson } from "./map/data/demandOverlay";
import {
  activeVehicleOverlayCollection,
  buildBuildPreviewOverlay,
} from "./map/data/runtimeOverlays";
import { buildPerfEvent, buildPerfMeasure } from "./perf/buildPerf";
import type { VehicleInspection } from "./app/vehicleInspection";

type LabelMarker = {
  marker: maplibregl.Marker;
  element: HTMLDivElement;
};

type HexInspectDatum = {
  hexNumber: number | null;
  hexId: string;
  regionId: string;
  regionName: string;
  assignmentState: string;
  unassigned: boolean;
  unlocked: boolean;
  resolved: boolean;
};

type RuntimeVehicleKeyframe = {
  tickSeconds: number;
  receivedAtMs: number;
};

type RuntimeVehicleTransition = {
  fromById: Map<string, [number, number]>;
  toById: Map<string, VehicleSnapshot>;
  durationMs: number;
  startedAtMs: number;
};

type PlacementHint = {
  message: string;
  x: number;
  y: number;
};

const EMPTY_UK_HEX_COVERAGE_DIAGNOSTICS: UkHexCoverageDiagnostics = {
  expected_land_hexes: 0,
  covered_hexes: 0,
  missing_land_hexes: 0,
  extra_non_land_hexes: 0,
  missing_hex_ids: [],
  coverage_ratio: 1,
  error: null,
};

export type MapWorldPoint = {
  lng: number;
  lat: number;
  x: number;
  y: number;
};

export type MapStopAction = {
  stopId: string;
  point: MapWorldPoint;
};

export type MapLineAction = {
  lineId: string;
};

type DemandCellInspectDatum = {
  cellId: string;
  planningRegionId: string | null;
  areaM2: number;
  residentsNight: number;
  jobsDay: number;
  centralityScore: number;
  dataQualityScore: number;
  activityMixResidential: number;
  activityMixOffice: number;
  activityMixRetail: number;
  activityMixRecreation: number;
  activityMixIndustrial: number;
  activityMixEducation: number;
  activityMixHealth: number;
  rawWeightResidential: number;
  rawWeightEmployment: number;
  allocatedResidentialMass: number;
  allocatedEmploymentMass: number;
  totalAllocation: number;
  fallbackReason: string | null;
};

function vehicleInspectionFromSnapshot(snapshot: VehicleSnapshot): VehicleInspection {
  return {
    vehicleId: snapshot.vehicleId,
    vehicleOrdinal: snapshot.vehicleOrdinal,
    serviceId: snapshot.serviceId,
    lineId: snapshot.lineId,
    lineName: snapshot.lineName,
    destinationLabel: snapshot.destinationLabel,
    mode: snapshot.mode,
    modeVariant: snapshot.modeVariant,
    stockTierId: snapshot.stockTierId,
    vehicleCapacity: snapshot.vehicleCapacity,
    passengersOnBoard: snapshot.passengersOnBoard,
    headwayS: snapshot.headwayS,
    lng: snapshot.lng,
    lat: snapshot.lat,
    displayColor: snapshot.displayColor,
    provenance: snapshot.provenance,
    passengerCounterProvenance: snapshot.passengerCounterProvenance ?? snapshot.provenance,
  };
}

function setData(map: MapLibreMap, sourceId: string, data: GeoCollection | GeoJsonFeatureCollection): void {
  (map.getSource(sourceId) as GeoJSONSource | undefined)?.setData(data as never);
}

function setVisibility(map: MapLibreMap, layerId: string, visible: boolean): void {
  if (!map.getLayer(layerId)) return;
  map.setLayoutProperty(layerId, "visibility", visible ? "visible" : "none");
}

const PERF_LOG_THRESHOLD_MS = 24;

function perfMeasure<T>(label: string, run: () => T): T {
  const started = performance.now();
  const output = run();
  const elapsed = performance.now() - started;
  if (elapsed >= PERF_LOG_THRESHOLD_MS) {
    console.info(`[perf] ${label}: ${elapsed.toFixed(1)}ms`);
  }
  return output;
}

function demandOverlayLabel(overlayType: DemandOverlayType): string {
  switch (overlayType) {
    case "residential_allocation":
      return "Residential Allocation";
    case "employment_allocation":
      return "Employment Allocation";
    case "total_allocation":
      return "Total Allocation";
    case "raw_residential_weight":
      return "Raw Residential Weight";
    case "raw_employment_weight":
      return "Raw Employment Weight";
    case "fallback_cells":
      return "Fallback Cells";
    default:
      return "Demand Cells";
  }
}

function formatDemandMetric(value: number, digits = 2): string {
  if (!Number.isFinite(value)) return "0";
  if (Math.abs(value) >= 1000) return Math.round(value).toLocaleString();
  return value.toFixed(digits);
}


export default function MapView(props: {
  scenario: ScenarioLite | null;
  projectPath?: string | null;
  mapRuntimeConfig: MapRuntimeConfig | null;
  clock?: SimulationClock | null;
  showShapeStops: boolean;
  showZoneCentroids: boolean;
  showStations: boolean;
  showLinks: boolean;
  linkMode: LinkModeFilter;
  demandOverlayEnabled: boolean;
  demandOverlayType: DemandOverlayType;
  demandOverlayPayload: DemandOverlayPayload | null;
  startCenter: [number, number] | null;
  serviceLoadByServiceId?: Record<string, number>;
  runtimeTrains?: TrainRuntimeView[];
  trainsAuthoritative?: boolean;
  sessionKind?: SessionKind | null;
  visibleCountryIso2: string[] | null;
  regions: RegionStatus[];
  focusRegionId: string | null;
  activeRegionIds: string[];
  selectedRegionId: string | null;
  interactionMode?: "view" | "build";
  buildAction?: "select" | "place_station" | "start_line" | "add_station_to_line" | "delete";
  buildConstraintMode?: string | null;
  selectedStopId?: string | null;
  selectedLineId?: string | null;
  selectedVehicleId?: string | null;
  activeLineId?: string | null;
  focusStopId?: string | null;
  focusStopToken?: number;
  focusVehicleId?: string | null;
  focusVehicleToken?: number;
  previewAnchorPoint?: { x: number; y: number } | null;
  previewColor?: string | null;
  onBootProgress?: (payload: {
    stage: "map_style" | "map_context" | "ready" | "error";
    progress: number;
    message: string;
    error?: string | null;
  }) => void;
  onSelectCounty: (regionId: string) => void;
  onStopAction?: (payload: MapStopAction) => void;
  onLineAction?: (payload: MapLineAction) => void;
  onVehicleAction?: (payload: VehicleInspection) => void;
  onClearVehicleSelection?: () => void;
  onMapPointAction?: (payload: MapWorldPoint) => void;
  onClearSelection?: () => void;
}) {
  const [forceCompatibilityBasemap, setForceCompatibilityBasemap] = useState(false);
  const vectorStyleUrl =
    forceCompatibilityBasemap || !props.mapRuntimeConfig?.map_ready
      ? null
      : props.mapRuntimeConfig?.style_url ?? null;
  const usesVectorBasemap = Boolean(vectorStyleUrl);
  const gameMode = props.sessionKind === "game";
  const allowHexAuthoring = import.meta.env.DEV;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const propsRef = useRef(props);
  const stopPointByIdRef = useRef<Map<string, MapWorldPoint & { name: string }>>(new Map());
  const loadedRef = useRef(false);
  const initialCenteredRef = useRef(false);
  const previousFocusRef = useRef<string | null>(null);
  const labelMarkersRef = useRef<LabelMarker[]>([]);
  const assetCollectionCacheRef = useRef<Map<string, Promise<GeoCollection> | GeoCollection>>(new Map());
  const suppressNextMapClickRef = useRef(false);
  const hoverPointRef = useRef<MapWorldPoint | null>(null);
  const hoverStopIdRef = useRef<string | null>(null);
  const selectedVehicleIdRef = useRef<string | null>(null);
  const selectedDemandCellIdRef = useRef<string | null>(null);
  const vehicleByIdRef = useRef<Map<string, VehicleSnapshot>>(new Map());
  const runtimeVehicleFeatureByIdRef = useRef<Map<string, OverlayGeoFeature>>(new Map());
  const runtimeVehicleGeoJsonRef = useRef<OverlayGeoCollection>(overlayCollection());
  const runtimeVehicleKeyframeRef = useRef<RuntimeVehicleKeyframe | null>(null);
  const runtimeVehicleTransportIntervalMsRef = useRef(120);
  const runtimeVehicleTransitionRef = useRef<RuntimeVehicleTransition | null>(null);
  const runtimeVehicleTransitionFrameRef = useRef<number | null>(null);
  const lastFocusedStopTokenRef = useRef<number | null>(null);
  const lastFocusedVehicleTokenRef = useRef<number | null>(null);
  const authoringModeRef = useRef(false);
  const hoveredHexIdRef = useRef<string | null>(null);
  const cursorHintRef = useRef<HTMLDivElement | null>(null);
  const lockedRegionLookupRef = useRef<(point: XY) => string | null>(() => null);
  const [styleReady, setStyleReady] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [authoringMode, setAuthoringMode] = useState(false);
  const [hoveredHex, setHoveredHex] = useState<HexInspectDatum | null>(null);
  const [selectedHex, setSelectedHex] = useState<HexInspectDatum | null>(null);
  const [visibleHexCount, setVisibleHexCount] = useState(0);
  const [runtimeVehicleGeoJsonVersion, setRuntimeVehicleGeoJsonVersion] = useState(0);
  const [localSelectedVehicleId, setLocalSelectedVehicleId] = useState<string | null>(null);
  const [selectedDemandCell, setSelectedDemandCell] = useState<DemandCellInspectDatum | null>(null);
  const [placementHint, setPlacementHint] = useState<PlacementHint | null>(null);
  const [worldContextData, setWorldContextData] = useState<GeoCollection>(fc());
  const [majorRoadData, setMajorRoadData] = useState<GeoCollection>(fc());
  const hintTimerRef = useRef<number | null>(null);
  const placementHintTimerRef = useRef<number | null>(null);
  const bootReadyEmittedRef = useRef(false);

  const showPlacementHint = useCallback(
    (message: string, point: { x: number; y: number }, durationMs = 1800) => {
      if (placementHintTimerRef.current) window.clearTimeout(placementHintTimerRef.current);
      setPlacementHint({ message, x: point.x, y: point.y });
      placementHintTimerRef.current = window.setTimeout(() => setPlacementHint(null), durationMs);
    },
    []
  );

  useEffect(() => {
    setForceCompatibilityBasemap(false);
    bootReadyEmittedRef.current = false;
  }, [props.mapRuntimeConfig?.style_url, props.projectPath]);

  useEffect(() => {
    propsRef.current = props;
  }, [props]);

  useEffect(() => {
    authoringModeRef.current = authoringMode;
  }, [authoringMode]);

  useEffect(() => {
    if (allowHexAuthoring) return;
    authoringModeRef.current = false;
    setAuthoringMode(false);
  }, [allowHexAuthoring]);

  useEffect(() => {
    if (!allowHexAuthoring) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.altKey && event.key.toLowerCase() === "h") {
        event.preventDefault();
        setAuthoringMode((value) => {
          const next = !value;
          authoringModeRef.current = next;
          if (!next) {
            hoveredHexIdRef.current = null;
            setHoveredHex(null);
            setSelectedHex(null);
            setVisibleHexCount(0);
          }
          return next;
        });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [allowHexAuthoring]);

  const emitBootProgress = useCallback(
    (payload: {
      stage: "map_style" | "map_context" | "ready" | "error";
      progress: number;
      message: string;
      error?: string | null;
    }) => {
      propsRef.current.onBootProgress?.(payload);
    },
    []
  );

  const selectedVehicleId =
    props.selectedVehicleId === undefined ? localSelectedVehicleId : props.selectedVehicleId ?? null;

  const clearVehicleSelection = useCallback(() => {
    selectedVehicleIdRef.current = null;
    if (propsRef.current.selectedVehicleId === undefined) {
      setLocalSelectedVehicleId(null);
      return;
    }
    propsRef.current.onClearVehicleSelection?.();
  }, []);

  const selectVehicleSnapshot = useCallback((snapshot: VehicleSnapshot) => {
    selectedVehicleIdRef.current = snapshot.vehicleId;
    if (propsRef.current.selectedVehicleId === undefined) {
      setLocalSelectedVehicleId(snapshot.vehicleId);
      return;
    }
    propsRef.current.onVehicleAction?.(vehicleInspectionFromSnapshot(snapshot));
  }, []);

  useEffect(() => {
    selectedVehicleIdRef.current = selectedVehicleId;
  }, [selectedVehicleId]);

  useEffect(() => {
    selectedDemandCellIdRef.current =
      selectedDemandCell?.cellId?.trim() ? selectedDemandCell.cellId : null;
  }, [selectedDemandCell]);

  const regionDisplayNameById = useMemo(
    () => buildRegionDisplayNames(props.regions),
    [props.regions]
  );

  const unlockedRegionIds = useMemo(
    () => new Set(props.regions.filter((region) => region.unlocked).map((region) => region.region_id)),
    [props.regions]
  );

  const focusRegion = useMemo(
    () => props.regions.find((region) => region.region_id === props.focusRegionId) ?? null,
    [props.focusRegionId, props.regions]
  );

  const worldCountryData = useMemo<GeoCollection>(
    () => buildWorldCountryData(worldContextData, props.visibleCountryIso2),
    [props.visibleCountryIso2, worldContextData]
  );

  const worldCountryLabelData = useMemo<GeoCollection>(
    () => buildWorldCountryLabelData(worldCountryData),
    [worldCountryData]
  );

  const stopPointById = useMemo(
    () => buildStopPointById(props.scenario),
    [props.scenario?.meta?.crs?.type, props.scenario?.world.stops]
  );

  useEffect(() => {
    stopPointByIdRef.current = stopPointById;
  }, [stopPointById]);

  const clockMinuteOfDay = useMemo(
    () => minuteOfDayFromClock(props.clock),
    [props.clock?.sim_datetime_utc]
  );

  const resolveRegionGeometry = useCallback(
    (region: RegionStatus | null): GeoJsonGeometry | null => {
      if (!region) return null;
      return parseRegionGeometry(region);
    },
    []
  );

  const regionFeatures = useMemo<GeoCollection>(
    () =>
      perfMeasure("map.build_region_features", () => {
      const base = buildCountyFeatures({
        regions: props.regions,
        resolveRegionGeometry,
      });
      return fc(
        base.features.map((feature) => {
          const regionId = String(feature.properties?.region_id ?? "");
          const display = regionDisplayNameById.get(regionId);
          if (!display) return feature;
          return {
            ...feature,
            properties: {
              ...feature.properties,
              name: display,
            },
          };
        })
      );
    }),
    [props.regions, regionDisplayNameById, resolveRegionGeometry]
  );

  const lockedRegionNameAtPoint = useCallback(
    (point: XY): string | null => {
      for (const feature of regionFeatures.features) {
        if (feature.geometry.type === "Point" || feature.geometry.type === "LineString") continue;
        if (Number(feature.properties?.unlocked ?? 0) === 1) continue;
        if (!pointInGeometry(point, feature.geometry as GeoJsonGeometry)) continue;
        const name = feature.properties?.name;
        return typeof name === "string" && name.trim().length > 0 ? name : "this region";
      }
      return null;
    },
    [regionFeatures]
  );

  useEffect(() => {
    lockedRegionLookupRef.current = lockedRegionNameAtPoint;
  }, [lockedRegionNameAtPoint]);

  const regionLabelData = useMemo(
    () =>
      buildCountyLabelData({
        regions: props.regions,
        focusRegionId: null,
        resolveRegionGeometry,
      }).map((label) => ({
        ...label,
        name: regionDisplayNameById.get(label.regionId) ?? label.name,
      })),
    [props.regions, regionDisplayNameById, resolveRegionGeometry]
  );

  const planningHexData = useMemo(
    () =>
      perfMeasure("map.build_planning_hex_data", () => {
      const raw = buildPlanningHexFeatures({
        regions: props.regions,
        regionFeatures,
      });
      // Backend-authoritative canonical hex numbers (from substrate hex numbering).
      // These are the numbers that manual_regions.json hex_numbers refer to.
      const canonicalNumberByFeatureIndex = new Map<number, number>();
      raw.features.forEach((feature, index) => {
        const canonical = Number(feature.properties?.hex_canonical_number ?? NaN);
        if (Number.isFinite(canonical) && canonical > 0) {
          canonicalNumberByFeatureIndex.set(index, Math.trunc(canonical));
        }
      });
      // Fallback: explicit hex numbers from unassigned hex region names (legacy compat).
      const explicitNumberByFeatureIndex = new Map<number, number>();
      raw.features.forEach((feature, index) => {
        if (canonicalNumberByFeatureIndex.has(index)) return;
        const isUnassigned = Number(feature.properties?.hex_unassigned ?? 0) === 1;
        if (!isUnassigned) return;
        const name = String(feature.properties?.name ?? "").trim();
        const match = /^hex\s*#\s*(\d+)$/i.exec(name);
        if (!match) return;
        const parsed = Number(match[1]);
        if (Number.isFinite(parsed) && parsed > 0) {
          explicitNumberByFeatureIndex.set(index, Math.trunc(parsed));
        }
      });
      // Last-resort: stable frontend-synthesized ordering for features with no
      // backend number (e.g. coverage gap backfill hexes, non-UK countries).
      const stableOrder = raw.features
        .map((feature, index) => {
          const hexId = String(feature.properties?.hex_id ?? "").trim();
          const regionId = String(feature.properties?.region_id ?? "").trim();
          const regionName = String(feature.properties?.name ?? "").trim();
          return {
            index,
            key: `${hexId}|${regionId}|${regionName}`,
          };
        })
        .sort((a, b) => a.key.localeCompare(b.key) || a.index - b.index);
      const displayNumberByFeatureIndex = new Map<number, number>();
      stableOrder.forEach((entry, orderIndex) => {
        displayNumberByFeatureIndex.set(entry.index, orderIndex + 1);
      });
      return fc(
        raw.features.map((feature, index) => ({
          ...feature,
          properties: {
            ...feature.properties,
            hex_num:
              canonicalNumberByFeatureIndex.get(index) ??
              explicitNumberByFeatureIndex.get(index) ??
              displayNumberByFeatureIndex.get(index) ??
              index + 1,
          },
        }))
      );
    }),
    [regionFeatures, props.regions]
  );

  const maxPlanningHexNumber = useMemo(
    () =>
      planningHexData.features.reduce((max, feature) => {
        const value = Number(feature.properties?.hex_num ?? 0);
        if (!Number.isFinite(value)) return max;
        return Math.max(max, Math.trunc(value));
      }, 0),
    [planningHexData]
  );

  const planningHexSummary = useMemo(() => {
    const total = planningHexData.features.length;
    const resolved = planningHexData.features.filter(
      (feature) => Number(feature.properties?.hex_resolved ?? 0) === 1
    ).length;
    const unassigned = planningHexData.features.filter(
      (feature) => Number(feature.properties?.hex_unassigned ?? 0) === 1
    ).length;
    const manualAssigned = planningHexData.features.filter(
      (feature) => Number(feature.properties?.hex_manual_assigned ?? 0) === 1
    ).length;
    const otherAssigned = Math.max(total - unassigned - manualAssigned, 0);
    return {
      total,
      resolved,
      unresolved: Math.max(total - resolved, 0),
      unassigned,
      manualAssigned,
      otherAssigned,
    };
  }, [planningHexData]);

  const regionDisplayFeatures = useMemo<GeoCollection>(
    () =>
      perfMeasure("map.build_region_display_features", () =>
        buildRegionDisplayFeatures(regionFeatures, planningHexData, props.regions)
      ),
    [regionFeatures, planningHexData, props.regions]
  );

  const shouldComputeHexCoverageDiagnostics = allowHexAuthoring && authoringMode;

  const ukHexCoverageDiagnostics = useMemo(
    () =>
      shouldComputeHexCoverageDiagnostics
        ? buildUkHexCoverageDiagnostics(worldCountryData, planningHexData)
        : EMPTY_UK_HEX_COVERAGE_DIAGNOSTICS,
    [planningHexData, shouldComputeHexCoverageDiagnostics, worldCountryData]
  );

  const hexCoverageGapData = useMemo(
    () =>
      shouldComputeHexCoverageDiagnostics
        ? buildHexCoverageGapFeatures(
            ukHexCoverageDiagnostics.missing_hex_ids.slice(0, 4000),
            maxPlanningHexNumber + 1
          )
        : fc(),
    [
      maxPlanningHexNumber,
      shouldComputeHexCoverageDiagnostics,
      ukHexCoverageDiagnostics.missing_hex_ids,
    ]
  );

  const networkData = useMemo(
    () =>
      perfMeasure("map.build_network_geojson", () =>
        buildNetworkGeojsonData({
          scenario: props.scenario,
          linkMode: props.linkMode,
          visibleCountryIso2: props.visibleCountryIso2,
          focusRegion,
          resolveRegionGeometry,
        })
      ),
    [
      props.linkMode,
      props.scenario,
      props.visibleCountryIso2,
      focusRegion,
      resolveRegionGeometry,
    ]
  );

  const demandOverlayData = useMemo(
    () =>
      perfMeasure("map.build_demand_overlay_geojson", () =>
        buildDemandOverlayGeojson(props.demandOverlayPayload, props.demandOverlayType)
      ),
    [props.demandOverlayPayload, props.demandOverlayType]
  );
  const demandOverlayFeatureCount = demandOverlayData.cells.features.length;
  const demandOverlayBaseActive = props.interactionMode !== "build" && props.demandOverlayEnabled;
  const showDemandCells = demandOverlayBaseActive && props.demandOverlayPayload?.available === true;
  const demandOverlayAnyVisible = showDemandCells;

  useEffect(() => {
    const payloadRows = props.demandOverlayPayload?.cell_data?.length ?? 0;
    const payloadTotal = props.demandOverlayPayload?.cell_data_total ?? 0;
    const payloadMappable = props.demandOverlayPayload?.cell_data_mappable ?? 0;
    const payloadAvailable = props.demandOverlayPayload?.available === true;
    console.info(
      `[demand-overlay-ui] overlay=${props.demandOverlayType} enabled=${props.demandOverlayEnabled} available=${payloadAvailable} payload_total=${payloadTotal} payload_mappable=${payloadMappable} payload_rows=${payloadRows} geojson_features=${demandOverlayFeatureCount} show=${showDemandCells}`
    );
  }, [
    demandOverlayFeatureCount,
    props.demandOverlayEnabled,
    props.demandOverlayPayload,
    props.demandOverlayType,
    showDemandCells,
  ]);

  useEffect(() => {
    if (showDemandCells) return;
    setSelectedDemandCell(null);
  }, [showDemandCells]);

  const serviceById = useMemo(() => buildServiceById(props.scenario), [props.scenario]);

  const vehicleRouteSeeds = useMemo(
    () =>
      buildVehicleRouteSeeds({
        gameMode,
        trainsAuthoritative: props.trainsAuthoritative,
        scenario: props.scenario,
        stopPointById,
        clockMinuteOfDay,
        xyToLngLat,
        modeColor,
      }),
    [clockMinuteOfDay, gameMode, props.scenario, props.trainsAuthoritative, stopPointById]
  );

  const vehicleData = useMemo(
    () =>
      buildVehicleData({
        gameMode,
        trainsAuthoritative: props.trainsAuthoritative,
        scenario: props.scenario,
        runtimeTrains: props.runtimeTrains,
        serviceById,
        vehicleRouteSeeds,
        clockTickSeconds: props.clock?.tick_seconds ?? 0,
        serviceLoadByServiceId: props.serviceLoadByServiceId,
        xyToLngLat,
        modeColor,
        runtimeVehicleGeoJson: runtimeVehicleGeoJsonRef.current,
      }),
    [
      props.clock?.tick_seconds,
      gameMode,
      props.runtimeTrains,
      props.scenario,
      props.serviceLoadByServiceId,
      serviceById,
      props.trainsAuthoritative,
      vehicleRouteSeeds,
    ]
  );

  const cancelRuntimeVehicleTransition = useCallback(() => {
    if (runtimeVehicleTransitionFrameRef.current !== null) {
      window.cancelAnimationFrame(runtimeVehicleTransitionFrameRef.current);
      runtimeVehicleTransitionFrameRef.current = null;
    }
    runtimeVehicleTransitionRef.current = null;
  }, []);

  const publishRuntimeVehicleOverlay = useCallback(
    (vehicleGeoJson: OverlayGeoCollection) => {
      const map = mapRef.current;
      if (!map || !loadedRef.current || !styleReady || !map.getSource(SRC_VEHICLES)) return;
      const activeVehicleData = activeVehicleOverlayCollection({
        gameMode,
        trainsAuthoritative: props.trainsAuthoritative,
        runtimeVehicleGeoJson: runtimeVehicleGeoJsonRef.current,
        vehicleGeoJson,
      });
      setData(map, SRC_VEHICLES, activeVehicleData);
    },
    [gameMode, props.trainsAuthoritative, styleReady]
  );

  useEffect(() => {
    const authoritativeVehicles = gameMode || Boolean(props.trainsAuthoritative);
    if (!authoritativeVehicles) {
      cancelRuntimeVehicleTransition();
      runtimeVehicleKeyframeRef.current = null;
      runtimeVehicleTransportIntervalMsRef.current = 120;
      const result = reconcileRuntimeVehicleGeoJson({
        gameMode,
        trainsAuthoritative: props.trainsAuthoritative,
        vehicleData,
        runtimeVehicleFeatureById: runtimeVehicleFeatureByIdRef.current,
      });
      if (!result.changed || !result.nextGeojson) return;
      runtimeVehicleGeoJsonRef.current = result.nextGeojson;
      setRuntimeVehicleGeoJsonVersion((prev) => prev + 1);
      return;
    }

    cancelRuntimeVehicleTransition();
    const featuresById = runtimeVehicleFeatureByIdRef.current;
    const nowMs = performance.now();
    const currentTick = Number.isFinite(props.clock?.tick_seconds)
      ? Math.max(props.clock?.tick_seconds ?? 0, 0)
      : runtimeVehicleKeyframeRef.current?.tickSeconds ?? 0;
    const previousKeyframe = runtimeVehicleKeyframeRef.current;
    const monotonicTick = previousKeyframe
      ? Math.max(previousKeyframe.tickSeconds, currentTick)
      : currentTick;

    if (previousKeyframe && currentTick + 1e-6 < previousKeyframe.tickSeconds) {
      console.warn("[runtime-temporal] backward train keyframe tick rejected", {
        previousTickSeconds: previousKeyframe.tickSeconds,
        incomingTickSeconds: currentTick,
      });
    }

    let durationMs = runtimeVehicleTransportIntervalMsRef.current;
    if (previousKeyframe) {
      const simDeltaSeconds = Math.max(monotonicTick - previousKeyframe.tickSeconds, 0);
      const speed = Math.max(props.clock?.speed ?? 1, 1);
      const durationFromSimMs = simDeltaSeconds > 0 ? (simDeltaSeconds / speed) * 1000 : 0;
      const arrivalDeltaMs = Math.max(nowMs - previousKeyframe.receivedAtMs, 0);
      if (arrivalDeltaMs > 1) {
        runtimeVehicleTransportIntervalMsRef.current =
          runtimeVehicleTransportIntervalMsRef.current * 0.7 + arrivalDeltaMs * 0.3;
      }
      const transportIntervalMs = Math.max(
        arrivalDeltaMs,
        runtimeVehicleTransportIntervalMsRef.current
      );
      const baselineMs = Math.max(durationFromSimMs, transportIntervalMs);
      durationMs = Math.min(Math.max(baselineMs || 120, 24), 900);
      if (simDeltaSeconds > 1.25) {
        console.warn("[runtime-temporal] large train keyframe gap", {
          simDeltaSeconds,
          durationMs,
          speed,
        });
      }
    }

    runtimeVehicleKeyframeRef.current = {
      tickSeconds: monotonicTick,
      receivedAtMs: nowMs,
    };

    const targetById = new Map(vehicleData.byId);
    let topologyChanged = false;
    for (const vehicleId of Array.from(featuresById.keys())) {
      if (!targetById.has(vehicleId)) {
        featuresById.delete(vehicleId);
        topologyChanged = true;
      }
    }

    const fromById = new Map<string, [number, number]>();
    for (const snapshot of targetById.values()) {
      const existing = featuresById.get(snapshot.vehicleId);
      if (existing) {
        const coords = existing.geometry.coordinates as [number, number];
        const prevLng = coords?.[0];
        const prevLat = coords?.[1];
        if (Number.isFinite(prevLng) && Number.isFinite(prevLat)) {
          fromById.set(snapshot.vehicleId, [prevLng, prevLat]);
        }
      }
      if (!fromById.has(snapshot.vehicleId)) {
        fromById.set(snapshot.vehicleId, [snapshot.lng, snapshot.lat]);
      }
      if (!existing) {
        featuresById.set(snapshot.vehicleId, {
          type: "Feature",
          geometry: { type: "Point", coordinates: [snapshot.lng, snapshot.lat] },
          properties: {
            vehicle_id: snapshot.vehicleId,
            service_id: snapshot.serviceId,
            line_id: snapshot.lineId,
            mode: snapshot.mode,
            mode_variant: snapshot.modeVariant,
            vehicle_capacity: snapshot.vehicleCapacity,
            passengers_on_board: snapshot.passengersOnBoard,
            display_color: snapshot.displayColor,
          },
        });
        topologyChanged = true;
      }
    }

    if (!previousKeyframe || durationMs <= 24 || targetById.size === 0) {
      for (const snapshot of targetById.values()) {
        const feature = featuresById.get(snapshot.vehicleId);
        if (!feature) continue;
        feature.geometry.coordinates = [snapshot.lng, snapshot.lat];
        feature.properties.service_id = snapshot.serviceId;
        feature.properties.line_id = snapshot.lineId;
        feature.properties.mode = snapshot.mode;
        feature.properties.mode_variant = snapshot.modeVariant;
        feature.properties.vehicle_capacity = snapshot.vehicleCapacity;
        feature.properties.passengers_on_board = snapshot.passengersOnBoard;
        feature.properties.display_color = snapshot.displayColor;
      }
      if (topologyChanged || targetById.size > 0) {
        runtimeVehicleGeoJsonRef.current = overlayCollection(Array.from(featuresById.values()));
        publishRuntimeVehicleOverlay(vehicleData.geojson);
      }
      return;
    }

    runtimeVehicleTransitionRef.current = {
      fromById,
      toById: targetById,
      durationMs,
      startedAtMs: nowMs,
    };

    const renderTransitionFrame = (): void => {
      const transition = runtimeVehicleTransitionRef.current;
      if (!transition) return;
      const elapsedMs = Math.max(performance.now() - transition.startedAtMs, 0);
      const alpha = Math.min(Math.max(elapsedMs / transition.durationMs, 0), 1);
      for (const [vehicleId, snapshot] of transition.toById.entries()) {
        const feature = featuresById.get(vehicleId);
        if (!feature) continue;
        const from = transition.fromById.get(vehicleId) ?? [snapshot.lng, snapshot.lat];
        const nextLng = from[0] + (snapshot.lng - from[0]) * alpha;
        const nextLat = from[1] + (snapshot.lat - from[1]) * alpha;
        feature.geometry.coordinates = [nextLng, nextLat];
        feature.properties.service_id = snapshot.serviceId;
        feature.properties.line_id = snapshot.lineId;
        feature.properties.mode = snapshot.mode;
        feature.properties.mode_variant = snapshot.modeVariant;
        feature.properties.vehicle_capacity = snapshot.vehicleCapacity;
        feature.properties.passengers_on_board = snapshot.passengersOnBoard;
        feature.properties.display_color = snapshot.displayColor;
      }
      runtimeVehicleGeoJsonRef.current = overlayCollection(Array.from(featuresById.values()));
      publishRuntimeVehicleOverlay(vehicleData.geojson);
      if (alpha < 1) {
        runtimeVehicleTransitionFrameRef.current = window.requestAnimationFrame(renderTransitionFrame);
      } else {
        runtimeVehicleTransitionRef.current = null;
        runtimeVehicleTransitionFrameRef.current = null;
      }
    };

    runtimeVehicleTransitionFrameRef.current = window.requestAnimationFrame(renderTransitionFrame);
  }, [
    cancelRuntimeVehicleTransition,
    gameMode,
    props.clock?.speed,
    props.clock?.tick_seconds,
    props.trainsAuthoritative,
    publishRuntimeVehicleOverlay,
    vehicleData,
  ]);

  useEffect(
    () => () => {
      cancelRuntimeVehicleTransition();
    },
    [cancelRuntimeVehicleTransition]
  );

  useEffect(() => {
    vehicleByIdRef.current = vehicleData.byId;
    if (selectedVehicleId && !vehicleData.byId.has(selectedVehicleId)) {
      clearVehicleSelection();
    }
  }, [clearVehicleSelection, selectedVehicleId, vehicleData.byId]);

  const selectedVehicle = useMemo(
    () => (selectedVehicleId ? vehicleData.byId.get(selectedVehicleId) ?? null : null),
    [selectedVehicleId, vehicleData.byId]
  );
  const selectedDemandOverlayLabel = useMemo(
    () => demandOverlayLabel(props.demandOverlayType),
    [props.demandOverlayType]
  );

  const loadBasemapCollection = useCallback(async (url: string): Promise<GeoCollection> => {
    const cached = assetCollectionCacheRef.current.get(url);
    if (cached) {
      return cached instanceof Promise ? cached : Promise.resolve(cached);
    }
    const pending = fetchFeatureCollection(url)
      .then((collection) => {
        assetCollectionCacheRef.current.set(url, collection);
        return collection;
      })
      .catch((error) => {
        assetCollectionCacheRef.current.delete(url);
        throw error;
      });
    assetCollectionCacheRef.current.set(url, pending);
    return pending;
  }, []);

  const refreshBuildPreview = useCallback(() => {
    buildPerfMeasure(
      "map.build_preview_source_update",
      () => {
        const map = mapRef.current;
        if (!map || !loadedRef.current || !map.getSource(SRC_BUILD_PREVIEW)) return;
        const current = propsRef.current;
        const preview = buildBuildPreviewOverlay({
          interactionMode: current.interactionMode,
          buildAction: current.buildAction,
          previewAnchorPoint: current.previewAnchorPoint,
          previewColor: current.previewColor,
          hoverPoint: hoverPointRef.current,
          crsType: current.scenario?.meta?.crs?.type,
        });
        setData(map, SRC_BUILD_PREVIEW, preview.geojson);
        setVisibility(
          map,
          "build-preview-line",
          preview.showPreviewLine && preview.geojson.features.length > 0
        );
        setVisibility(
          map,
          "build-preview-point",
          preview.showPreviewPoint && preview.geojson.features.length > 0
        );
      },
      {
        interactionMode: propsRef.current.interactionMode,
        buildAction: propsRef.current.buildAction,
      },
      { minDurationMs: 0, throttleMs: 120 }
    );
  }, []);

  const applyInteractionFilters = useCallback(() => {
    buildPerfMeasure(
      "map.selected_line_refresh",
      () => {
        const map = mapRef.current;
        if (!map || !loadedRef.current) return;
        const current = propsRef.current;
        const selectedLineId = current.selectedLineId?.trim() ? current.selectedLineId : "__none__";
        const selectedStopId = current.selectedStopId?.trim() ? current.selectedStopId : "__none__";
        const activeLineId = current.activeLineId?.trim() ? current.activeLineId : "__none__";
        const hoverStopId = hoverStopIdRef.current?.trim() ? hoverStopIdRef.current : "__none__";
        const selectedVehicleId = selectedVehicleIdRef.current?.trim() ? selectedVehicleIdRef.current : "__none__";
        const selectedDemandCellId =
          selectedDemandCellIdRef.current?.trim() ? selectedDemandCellIdRef.current : "__none__";
        const selectedLineStops =
          selectedLineId === "__none__"
            ? []
            : current.scenario?.world.services
                .filter((service) => serviceLineId(service) === selectedLineId)
                .flatMap((service) => service.stop_sequence) ?? [];

        const selectedLineFilter = ["==", ["get", "line_id"], selectedLineId];
        const selectedStopFilter = ["==", ["get", "id"], selectedStopId];
        const hoverStopFilter = ["==", ["get", "id"], hoverStopId];
        const activeLineFilter = ["==", ["get", "line_id"], activeLineId];
        const selectedVehicleFilter = ["==", ["get", "vehicle_id"], selectedVehicleId];
        const dimFilter =
          selectedLineId === "__none__"
            ? EMPTY_LINE_FILTER
            : (["!=", ["get", "line_id"], selectedLineId] as const);
        const selectedLineStopFilter =
          selectedLineStops.length > 0
            ? (["in", ["get", "id"], ["literal", selectedLineStops]] as const)
            : EMPTY_STOP_FILTER;

        if (map.getLayer("links-selected-glow")) map.setFilter("links-selected-glow", selectedLineFilter as never);
        if (map.getLayer("links-selected-casing")) map.setFilter("links-selected-casing", selectedLineFilter as never);
        if (map.getLayer("links-selected")) map.setFilter("links-selected", selectedLineFilter as never);
        if (map.getLayer("links-selection-dim")) map.setFilter("links-selection-dim", dimFilter as never);
        if (map.getLayer("links-active")) map.setFilter("links-active", activeLineFilter as never);
        if (map.getLayer("stops-selected-halo")) map.setFilter("stops-selected-halo", selectedStopFilter as never);
        if (map.getLayer("stops-selected")) map.setFilter("stops-selected", selectedStopFilter as never);
        if (map.getLayer("stops-build-hover-ring")) map.setFilter("stops-build-hover-ring", hoverStopFilter as never);
        if (map.getLayer("stops-selected-line-ring")) {
          map.setFilter("stops-selected-line-ring", selectedLineStopFilter as never);
        }
        if (map.getLayer("vehicles-selected-halo")) {
          map.setFilter("vehicles-selected-halo", selectedVehicleFilter as never);
        }
        if (map.getLayer("demand-cell-selected-outline")) {
          map.setFilter(
            "demand-cell-selected-outline",
            ["==", ["get", "cell_id"], selectedDemandCellId] as never
          );
        }
      },
      {
        interactionMode: propsRef.current.interactionMode,
        buildAction: propsRef.current.buildAction,
      },
      { minDurationMs: 0, throttleMs: 120 }
    );
  }, []);

  const readHexInspectDatum = useCallback(
    (feature: { properties?: Record<string, unknown> } | undefined): HexInspectDatum | null => {
      const hexIdRaw = feature?.properties?.hex_id;
      const regionIdRaw = feature?.properties?.region_id;
      if (typeof hexIdRaw !== "string" || !hexIdRaw.trim()) return null;
      if (typeof regionIdRaw !== "string" || !regionIdRaw.trim()) return null;
      const regionNameRaw = feature?.properties?.name;
      const assignmentStateRaw = feature?.properties?.hex_assignment_state;
      const assignmentState =
        typeof assignmentStateRaw === "string" && assignmentStateRaw.trim().length > 0
          ? assignmentStateRaw.trim()
          : "other_assigned";
      // Prefer the backend-authoritative canonical hex number over the
      // frontend-synthesized hex_num to ensure parity with manual_regions.json.
      const canonicalRaw = Number(feature?.properties?.hex_canonical_number ?? NaN);
      const hexNumRaw = Number(feature?.properties?.hex_num ?? NaN);
      const hexNumberRaw = Number.isFinite(canonicalRaw) && canonicalRaw > 0
        ? canonicalRaw
        : hexNumRaw;
      return {
        hexNumber: Number.isFinite(hexNumberRaw) ? Math.trunc(hexNumberRaw) : null,
        hexId: hexIdRaw.trim(),
        regionId: regionIdRaw.trim(),
        regionName:
          typeof regionNameRaw === "string" && regionNameRaw.trim().length > 0
            ? regionNameRaw.trim()
            : regionIdRaw.trim(),
        assignmentState,
        unassigned: Number(feature?.properties?.hex_unassigned ?? 0) === 1,
        unlocked: Number(feature?.properties?.unlocked ?? 0) === 1,
        resolved: Number(feature?.properties?.hex_resolved ?? 0) === 1,
      };
    },
    []
  );

  const readDemandCellInspectDatum = useCallback(
    (feature: { properties?: Record<string, unknown> } | undefined): DemandCellInspectDatum | null => {
      const cellIdRaw = feature?.properties?.cell_id;
      if (typeof cellIdRaw !== "string" || !cellIdRaw.trim()) return null;
      const readNumber = (field: string) => {
        const value = Number(feature?.properties?.[field] ?? NaN);
        return Number.isFinite(value) ? value : 0;
      };
      const planningRegionRaw = feature?.properties?.planning_region_id;
      const fallbackReasonRaw = feature?.properties?.fallback_reason;
      return {
        cellId: cellIdRaw.trim(),
        planningRegionId:
          typeof planningRegionRaw === "string" && planningRegionRaw.trim().length > 0
            ? planningRegionRaw.trim()
            : null,
        areaM2: readNumber("area_m2"),
        residentsNight: readNumber("residents_night"),
        jobsDay: readNumber("jobs_day"),
        centralityScore: readNumber("centrality_score"),
        dataQualityScore: readNumber("data_quality_score"),
        activityMixResidential: readNumber("activity_mix_residential"),
        activityMixOffice: readNumber("activity_mix_office"),
        activityMixRetail: readNumber("activity_mix_retail"),
        activityMixRecreation: readNumber("activity_mix_recreation"),
        activityMixIndustrial: readNumber("activity_mix_industrial"),
        activityMixEducation: readNumber("activity_mix_education"),
        activityMixHealth: readNumber("activity_mix_health"),
        rawWeightResidential: readNumber("raw_weight_residential"),
        rawWeightEmployment: readNumber("raw_weight_employment"),
        allocatedResidentialMass: readNumber("allocated_residential_mass"),
        allocatedEmploymentMass: readNumber("allocated_employment_mass"),
        totalAllocation: readNumber("total_allocation"),
        fallbackReason:
          typeof fallbackReasonRaw === "string" && fallbackReasonRaw.trim().length > 0
            ? fallbackReasonRaw.trim()
            : null,
      };
    },
    []
  );

  useEffect(() => {
    if (!containerRef.current || mapRef.current) return;
    emitBootProgress({
      stage: "map_style",
      progress: 0.52,
      message: "Initializing map style...",
    });
    const map = new maplibregl.Map({
      container: containerRef.current,
      style:
        vectorStyleUrl ??
        ({
          version: 8,
          glyphs: "https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf",
          sources: {},
          layers: [{ id: "background", type: "background", paint: { "background-color": "#b7d9f6" } }],
        } as never),
      center: props.startCenter ?? [-2.6, 54.4],
      zoom: props.startCenter ? 8.7 : 4.8,
      minZoom: 2,
      maxZoom: 17,
      maxPitch: 0,
      renderWorldCopies: false,
      attributionControl: false,
    });
    mapRef.current = map;
    const vectorStyleLoadGuard = window.setTimeout(() => {
      if (loadedRef.current || forceCompatibilityBasemap || !propsRef.current.mapRuntimeConfig?.style_url) {
        return;
      }
      setHint("Local map tiles are still loading.");
      emitBootProgress({
        stage: "error",
        progress: 0.58,
        message: "Map style is taking longer than expected.",
        error: "Vector style still loading.",
      });
    }, 30000);

    map.on("error", (event) => {
      const raw = String((event as { error?: unknown }).error ?? "Map rendering error");
      emitBootProgress({
        stage: "error",
        progress: 0.56,
        message: "Map reported an error.",
        error: raw,
      });
    });
    map.on("load", () => {
      window.clearTimeout(vectorStyleLoadGuard);
      loadedRef.current = true;
      setStyleReady(true);
      emitBootProgress({
        stage: "map_style",
        progress: 0.66,
        message: "Map style ready.",
      });
      ensureMapLayers(map);
      const setDefaultCursor = () => {
        const current = propsRef.current;
        map.getCanvas().style.cursor =
          current.interactionMode === "build" &&
          current.buildAction !== "select" &&
          current.buildAction !== "delete"
            ? "crosshair"
            : "";
      };
      const setCursorHint = (text: string | null, point?: { x: number; y: number }) => {
        const element = cursorHintRef.current;
        if (!element || !text || !point) {
          if (element) element.style.display = "none";
          return;
        }
        const container = containerRef.current;
        const maxX = Math.max((container?.clientWidth ?? 0) - 16, 0);
        const maxY = Math.max((container?.clientHeight ?? 0) - 16, 0);
        const x = Math.min(point.x + 16, maxX);
        const y = Math.min(point.y + 16, maxY);
        element.textContent = text;
        element.style.display = "block";
        element.style.transform = `translate(${x}px, ${y}px)`;
      };
      const modeConstraintHintAt = (lng: number, lat: number): string | null => {
        const mode = (propsRef.current.buildConstraintMode ?? "").trim().toLowerCase();
        if (mode !== "bus" && mode !== "ferry") return null;
        if (usesVectorBasemap) return null;
        if (map.getZoom() < 6.5) return null;
        const pixel = map.project([lng, lat]);
        const queryBox: [PointLike, PointLike] = [
          [pixel.x - 10, pixel.y - 10],
          [pixel.x + 10, pixel.y + 10],
        ];
        if (mode === "bus") {
          const nearbyRoad = map.queryRenderedFeatures(queryBox, {
            layers: ["gb-major-roads", "links-major", "links-trunk"],
          });
          if (nearbyRoad.length === 0) return "Bus stops and links must follow roads.";
          return null;
        }
        // County-sliced water overlays are no longer part of the active render path.
        // Keep ferry placement permissive in compatibility mode.
        return null;
      };
      const emitPointAction = (lng: number, lat: number, screenPoint?: { x: number; y: number }) => {
        buildPerfEvent("map.build_point_emit.start", {
          interactionMode: propsRef.current.interactionMode,
          buildAction: propsRef.current.buildAction,
          hasScreenPoint: Boolean(screenPoint),
        });
        const current = propsRef.current;
        const buildAction = current.buildAction ?? "select";
        const buildingPoint =
          current.interactionMode === "build" &&
          (buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line");
        if (buildingPoint) {
          const blockedRegion = lockedRegionLookupRef.current({ lng, lat });
          if (blockedRegion) {
            buildPerfEvent("map.build_point_emit.blocked_locked_region", {
              blockedRegion,
              buildAction,
            });
            if (screenPoint) {
              showPlacementHint(`Unlock ${blockedRegion} before building here.`, screenPoint, 2000);
            }
            return;
          }
          const modeHint = modeConstraintHintAt(lng, lat);
          if (modeHint) {
            if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
            setHint(modeHint);
            hintTimerRef.current = window.setTimeout(() => setHint(null), 1800);
          }
        }
        const converted = lngLatToXY(lng, lat, propsRef.current.scenario?.meta?.crs?.type);
        const actionPoint = converted
          ? {
              lng,
              lat,
              x: converted.x,
              y: converted.y,
            }
          : null;
        if (actionPoint) propsRef.current.onMapPointAction?.(actionPoint);
        buildPerfEvent("map.build_point_emit.done", {
          emitted: Boolean(actionPoint),
          buildAction,
        });
      };
      const emitStopAction = (stopId: string, lng: number, lat: number, screenPoint?: { x: number; y: number }) => {
        buildPerfEvent("map.build_stop_emit.start", {
          stopId,
          interactionMode: propsRef.current.interactionMode,
          buildAction: propsRef.current.buildAction,
        });
        const mappedStop = stopPointByIdRef.current.get(stopId);
        const targetLng = mappedStop?.lng ?? lng;
        const targetLat = mappedStop?.lat ?? lat;
        const current = propsRef.current;
        const buildAction = current.buildAction ?? "select";
        const buildingPoint =
          current.interactionMode === "build" &&
          (buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line");
        if (buildingPoint) {
          const blockedRegion = lockedRegionLookupRef.current({ lng: targetLng, lat: targetLat });
          if (blockedRegion) {
            buildPerfEvent("map.build_stop_emit.blocked_locked_region", {
              stopId,
              blockedRegion,
              buildAction,
            });
            if (screenPoint) {
              showPlacementHint(`Unlock ${blockedRegion} before building here.`, screenPoint, 2000);
            }
            return;
          }
          const modeHint = modeConstraintHintAt(targetLng, targetLat);
          if (modeHint) {
            if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
            setHint(modeHint);
            hintTimerRef.current = window.setTimeout(() => setHint(null), 1800);
          }
        }
        if (mappedStop) {
          propsRef.current.onStopAction?.({
            stopId,
            point: {
              lng: mappedStop.lng,
              lat: mappedStop.lat,
              x: mappedStop.x,
              y: mappedStop.y,
            },
          });
          buildPerfEvent("map.build_stop_emit.done", { stopId, snapped: true, buildAction });
          return;
        }
        const converted = lngLatToXY(lng, lat, propsRef.current.scenario?.meta?.crs?.type);
        if (!converted) return;
        propsRef.current.onStopAction?.({
          stopId,
          point: {
            lng,
            lat,
            x: converted.x,
            y: converted.y,
          },
        });
        buildPerfEvent("map.build_stop_emit.done", { stopId, snapped: false, buildAction });
      };
      const interactiveNetworkLayers = ["stops-hit", "links-hit", "vehicles-hit", "demand-cell-hit"];
      const setHoveredHexIfChanged = (next: HexInspectDatum | null) => {
        const nextKey = next ? `${next.hexId}::${next.hexNumber ?? "none"}` : null;
        if (hoveredHexIdRef.current === nextKey) return;
        hoveredHexIdRef.current = nextKey;
        setHoveredHex(next);
      };
      const readHexDatumAtPoint = (x: number, y: number): HexInspectDatum | null => {
        const queryBox: [PointLike, PointLike] = [
          [x - 5, y - 5],
          [x + 5, y + 5],
        ];
        for (const layer of [
          "planning-hex-hit",
          "planning-hex-gap-hit",
          "planning-hex-fill",
          "planning-hex-gap-fill",
          "planning-hex-outline",
          "planning-hex-gap-outline",
        ]) {
          const feature = map.queryRenderedFeatures(queryBox, { layers: [layer] })[0] as
            | { properties?: Record<string, unknown> }
            | undefined;
          const datum = readHexInspectDatum(feature);
          if (datum) return datum;
        }
        return null;
      };
      map.on("click", "region-fill", (event) => {
        if (authoringModeRef.current) return;
        if (propsRef.current.interactionMode === "build") return;
        if (
          map.queryRenderedFeatures(
            [
              [event.point.x - 8, event.point.y - 8],
              [event.point.x + 8, event.point.y + 8],
            ],
            { layers: interactiveNetworkLayers }
          ).length > 0
        ) {
          return;
        }
        suppressNextMapClickRef.current = true;
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const regionId = feature?.properties?.region_id;
        const unlocked = Number(feature?.properties?.unlocked ?? 0) === 1;
        if (unlocked) return;
        if (typeof regionId === "string" && regionId.length > 0) propsRef.current.onSelectCounty(regionId);
      });
      map.on("click", "world-country-fill", (event) => {
        if (authoringModeRef.current) return;
        if (propsRef.current.interactionMode === "build") return;
        if (
          map.queryRenderedFeatures(
            [
              [event.point.x - 8, event.point.y - 8],
              [event.point.x + 8, event.point.y + 8],
            ],
            { layers: interactiveNetworkLayers }
          ).length > 0
        ) {
          return;
        }
        suppressNextMapClickRef.current = true;
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const comingSoon = Number(feature?.properties?.coming_soon ?? 1) === 1;
        if (!comingSoon) return;
        const name = typeof feature?.properties?.name === "string" ? feature.properties.name : "This country";
        if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
        setHint(`${name} is coming soon.`);
        hintTimerRef.current = window.setTimeout(() => setHint(null), 2400);
      });
      const stopClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
        if (authoringModeRef.current) return;
        suppressNextMapClickRef.current = true;
        setSelectedDemandCell(null);
        clearVehicleSelection();
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const stopId = feature?.properties?.id;
        if (typeof stopId !== "string" || stopId.length === 0) return;
        emitStopAction(stopId, event.lngLat.lng, event.lngLat.lat, { x: event.point.x, y: event.point.y });
      };
      const lineClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
        if (authoringModeRef.current) return;
        if (
          map.queryRenderedFeatures(
            [
              [event.point.x - 8, event.point.y - 8],
              [event.point.x + 8, event.point.y + 8],
            ],
            { layers: ["stops-hit", "vehicles-hit"] }
          ).length > 0
        ) {
          return;
        }
        const current = propsRef.current;
        setSelectedDemandCell(null);
        clearVehicleSelection();
        const buildAction = current.buildAction ?? "select";
        if (
          current.interactionMode === "build" &&
          (buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line")
        ) {
          return;
        }
        suppressNextMapClickRef.current = true;
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const lineId = feature?.properties?.line_id;
        if (typeof lineId !== "string" || lineId.length === 0) return;
        current.onLineAction?.({ lineId });
      };
      const vehicleClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
        if (authoringModeRef.current) return;
        suppressNextMapClickRef.current = true;
        const stopFeature = map.queryRenderedFeatures(
          [
            [event.point.x - 8, event.point.y - 8],
            [event.point.x + 8, event.point.y + 8],
          ],
          { layers: ["stops-hit"] }
        )[0] as { properties?: Record<string, unknown> } | undefined;
        const stopId = stopFeature?.properties?.id;
        if (typeof stopId === "string" && stopId.length > 0) {
          clearVehicleSelection();
          emitStopAction(stopId, event.lngLat.lng, event.lngLat.lat, { x: event.point.x, y: event.point.y });
          applyInteractionFilters();
          return;
        }
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const vehicleId = feature?.properties?.vehicle_id;
        if (typeof vehicleId !== "string" || vehicleId.length === 0) return;
        const snapshot = vehicleByIdRef.current.get(vehicleId);
        if (!snapshot) return;
        setSelectedDemandCell(null);
        propsRef.current.onClearSelection?.();
        selectVehicleSnapshot(snapshot);
        map.easeTo({
          center: [snapshot.lng, snapshot.lat],
          duration: 220,
          essential: true,
        });
        applyInteractionFilters();
      };
      const demandCellClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
        if (authoringModeRef.current) return;
        const current = propsRef.current;
        if (current.interactionMode === "build") return;
        suppressNextMapClickRef.current = true;
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const datum = readDemandCellInspectDatum(feature);
        if (!datum) return;
        setSelectedDemandCell(datum);
        clearVehicleSelection();
        current.onClearSelection?.();
        applyInteractionFilters();
      };
      for (const layerId of ["stops-hit"]) {
        map.on("click", layerId, stopClickHandler);
      }
      for (const layerId of ["links-hit"]) {
        map.on("click", layerId, lineClickHandler);
      }
      for (const layerId of ["vehicles-hit"]) {
        map.on("click", layerId, vehicleClickHandler);
      }
      for (const layerId of ["demand-cell-hit"]) {
        map.on("click", layerId, demandCellClickHandler);
      }
      map.on("click", (event) => {
        if (suppressNextMapClickRef.current) {
          suppressNextMapClickRef.current = false;
          return;
        }
        const current = propsRef.current;
        // Authoring hard override: hex click/clear is authoritative when mode is active.
        if (authoringModeRef.current) {
          const hexDatum = readHexDatumAtPoint(event.point.x, event.point.y);
          if (hexDatum) {
            setSelectedHex(hexDatum);
            setHoveredHexIfChanged(hexDatum);
            setHint(`Pinned Hex #${hexDatum.hexNumber ?? "—"} · ${hexDatum.hexId}`);
            if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
            hintTimerRef.current = window.setTimeout(() => setHint(null), 1500);
          } else {
            setSelectedHex(null);
          }
          clearVehicleSelection();
          return;
        }
        const buildAction = current.buildAction ?? "select";
        if (current.interactionMode === "build") {
          if (
            buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line"
          ) {
            const hoveredStopId = hoverStopIdRef.current;
            const hoveredStop = hoveredStopId ? stopPointByIdRef.current.get(hoveredStopId) ?? null : null;
            if (hoveredStop && current.onStopAction) {
              emitStopAction(hoveredStopId!, hoveredStop.lng, hoveredStop.lat, {
                x: event.point.x,
                y: event.point.y,
              });
              return;
            }
            const forgivingHit = map.queryRenderedFeatures(
              [
                [event.point.x - 10, event.point.y - 10],
                [event.point.x + 10, event.point.y + 10],
              ],
              { layers: ["stops-hit"] }
            );
            const forgivingStopId = forgivingHit[0]?.properties?.id;
            if (typeof forgivingStopId === "string" && forgivingStopId.length > 0 && current.onStopAction) {
              const snapped = stopPointByIdRef.current.get(forgivingStopId);
              if (snapped) {
                emitStopAction(forgivingStopId, snapped.lng, snapped.lat, {
                  x: event.point.x,
                  y: event.point.y,
                });
                return;
              }
            }
            emitPointAction(event.lngLat.lng, event.lngLat.lat, { x: event.point.x, y: event.point.y });
            return;
          }
          if (buildAction === "select") {
            current.onClearSelection?.();
          }
          return;
        }
        clearVehicleSelection();
        setSelectedDemandCell(null);
        current.onClearSelection?.();
      });
      map.on("mousemove", (event) => {
        const authoringDatum = authoringModeRef.current
          ? readHexDatumAtPoint(event.point.x, event.point.y)
          : null;
        if (authoringModeRef.current) {
          setHoveredHexIfChanged(authoringDatum);
          if (authoringDatum) {
            const numberText =
              authoringDatum.hexNumber !== null
                ? `Hex #${authoringDatum.hexNumber.toLocaleString()}`
                : "Hex";
            setCursorHint(`${numberText} · ${authoringDatum.hexId}`, {
              x: event.point.x,
              y: event.point.y,
            });
            map.getCanvas().style.cursor = "crosshair";
          }
        } else {
          setHoveredHexIfChanged(null);
        }
        const current = propsRef.current;
        const buildAction = current.buildAction ?? "select";
        if (
          current.interactionMode === "build" &&
          (buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line")
        ) {
          const stopFeature = map.queryRenderedFeatures(
            [
              [event.point.x - 8, event.point.y - 8],
              [event.point.x + 8, event.point.y + 8],
            ],
            { layers: ["stops-hit"] }
          )[0] as { properties?: Record<string, unknown> } | undefined;
          const stopId = typeof stopFeature?.properties?.id === "string" ? stopFeature.properties.id : null;
          const snapped = stopId ? stopPointByIdRef.current.get(stopId) ?? null : null;
          let cursorHintText: string | null = null;
          if (snapped) {
            hoverStopIdRef.current = stopId;
            hoverPointRef.current = {
              lng: snapped.lng,
              lat: snapped.lat,
              x: snapped.x,
              y: snapped.y,
            };
            if (buildAction === "place_station") {
              cursorHintText = `Use ${snapped.name}`;
            }
          } else {
            const converted = lngLatToXY(event.lngLat.lng, event.lngLat.lat, current.scenario?.meta?.crs?.type);
            hoverStopIdRef.current = null;
            hoverPointRef.current = converted
              ? {
                  lng: event.lngLat.lng,
                  lat: event.lngLat.lat,
                  x: converted.x,
                  y: converted.y,
                }
              : null;
          }
          if ((buildAction === "start_line" || buildAction === "add_station_to_line") && hoverPointRef.current) {
            const anchor = current.previewAnchorPoint;
            if (anchor) {
              const dx = hoverPointRef.current.x - anchor.x;
              const dy = hoverPointRef.current.y - anchor.y;
              const distanceLabel = formatDistanceKm(Math.sqrt(dx * dx + dy * dy));
              if (snapped) {
                cursorHintText = `Connect ${snapped.name} · ${distanceLabel}`;
              } else {
                cursorHintText = distanceLabel;
              }
            } else {
              cursorHintText =
                buildAction === "add_station_to_line"
                  ? "Select terminus to begin extension"
                  : "Click a station to lock line start";
            }
          }
          const blockedRegion = hoverPointRef.current
            ? lockedRegionLookupRef.current({ lng: hoverPointRef.current.lng, lat: hoverPointRef.current.lat })
            : null;
          if (blockedRegion) {
            map.getCanvas().style.cursor = "not-allowed";
            setCursorHint(`Locked region: ${blockedRegion}`, { x: event.point.x, y: event.point.y });
            refreshBuildPreview();
            applyInteractionFilters();
            return;
          }
          const modeHint = hoverPointRef.current
            ? modeConstraintHintAt(hoverPointRef.current.lng, hoverPointRef.current.lat)
            : null;
          if (modeHint) {
            cursorHintText = cursorHintText ? `${cursorHintText} · ${modeHint}` : modeHint;
          }
          map.getCanvas().style.cursor = snapped ? "pointer" : "crosshair";
          setCursorHint(cursorHintText, { x: event.point.x, y: event.point.y });
          refreshBuildPreview();
          applyInteractionFilters();
          return;
        }
        if (!authoringModeRef.current || !authoringDatum) {
          setCursorHint(null);
        }
        if (hoverStopIdRef.current) {
          hoverStopIdRef.current = null;
        }
        if (hoverPointRef.current) {
          hoverPointRef.current = null;
          refreshBuildPreview();
        }
        applyInteractionFilters();
      });
      map.on("mouseout", () => {
        if (authoringModeRef.current) {
          setHoveredHexIfChanged(null);
        }
        if (hoverStopIdRef.current) {
          hoverStopIdRef.current = null;
        }
        if (hoverPointRef.current) {
          hoverPointRef.current = null;
          refreshBuildPreview();
        }
        if (propsRef.current.interactionMode === "build") {
          setHint(null);
        }
        setCursorHint(null);
        setDefaultCursor();
        applyInteractionFilters();
      });
      for (const layerId of ["region-fill", "world-country-fill"]) {
        map.on("mouseenter", layerId, () => {
          if (propsRef.current.interactionMode === "build") {
            setDefaultCursor();
            return;
          }
          map.getCanvas().style.cursor = "pointer";
        });
        map.on("mouseleave", layerId, () => {
          setDefaultCursor();
        });
      }
      map.on("mouseenter", "planning-hex-hit", () => {
        if (authoringModeRef.current) {
          map.getCanvas().style.cursor = "crosshair";
        }
      });
      map.on("mouseleave", "planning-hex-hit", () => {
        setDefaultCursor();
      });
      map.on("mouseenter", "planning-hex-gap-hit", () => {
        if (authoringModeRef.current) {
          map.getCanvas().style.cursor = "crosshair";
        }
      });
      map.on("mouseleave", "planning-hex-gap-hit", () => {
        setDefaultCursor();
      });
      for (const layerId of [
        "stops-hit",
        "links-hit",
        "vehicles-hit",
        "demand-cell-hit",
      ]) {
        map.on("mouseenter", layerId, () => {
          if (
            propsRef.current.interactionMode === "build" &&
            propsRef.current.buildAction !== "select" &&
            propsRef.current.buildAction !== "delete"
          ) {
            setDefaultCursor();
            return;
          }
          map.getCanvas().style.cursor = "pointer";
        });
        map.on("mouseleave", layerId, () => {
          setDefaultCursor();
        });
      }
      setDefaultCursor();
      setCursorHint(null);
      refreshBuildPreview();
      applyInteractionFilters();
    });
    return () => {
      window.clearTimeout(vectorStyleLoadGuard);
      loadedRef.current = false;
      setStyleReady(false);
      if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
      if (placementHintTimerRef.current) window.clearTimeout(placementHintTimerRef.current);
      for (const item of labelMarkersRef.current) item.marker.remove();
      labelMarkersRef.current = [];
      map.remove();
      mapRef.current = null;
    };
  }, [
    applyInteractionFilters,
    clearVehicleSelection,
    emitBootProgress,
    forceCompatibilityBasemap,
    props.mapRuntimeConfig?.style_url,
    props.startCenter,
    refreshBuildPreview,
    selectVehicleSnapshot,
    vectorStyleUrl,
    showPlacementHint,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !props.startCenter || initialCenteredRef.current) return;
    map.jumpTo({ center: props.startCenter, zoom: 8.7 });
    initialCenteredRef.current = true;
  }, [props.startCenter]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !props.focusStopId) return;
    const token = props.focusStopToken ?? 0;
    if (lastFocusedStopTokenRef.current === token) return;
    const stopPoint = stopPointByIdRef.current.get(props.focusStopId);
    if (!stopPoint) return;
    lastFocusedStopTokenRef.current = token;
    map.easeTo({
      center: [stopPoint.lng, stopPoint.lat],
      duration: 350,
      essential: true,
    });
  }, [props.focusStopId, props.focusStopToken]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !props.focusVehicleId) return;
    const token = props.focusVehicleToken ?? 0;
    if (lastFocusedVehicleTokenRef.current === token && selectedVehicleIdRef.current === props.focusVehicleId) return;
    const vehicle = vehicleByIdRef.current.get(props.focusVehicleId);
    if (!vehicle) return;
    lastFocusedVehicleTokenRef.current = token;
    setSelectedDemandCell(null);
    selectVehicleSnapshot(vehicle);
    map.easeTo({
      center: [vehicle.lng, vehicle.lat],
      duration: 300,
      essential: true,
    });
    applyInteractionFilters();
  }, [applyInteractionFilters, props.focusVehicleId, props.focusVehicleToken, selectVehicleSnapshot, vehicleData.byId]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !selectedVehicle) return;
    map.easeTo({
      center: [selectedVehicle.lng, selectedVehicle.lat],
      duration: 220,
      essential: true,
    });
  }, [selectedVehicle?.lat, selectedVehicle?.lng]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current) return;
    map.getCanvas().style.cursor =
      props.interactionMode === "build" &&
      props.buildAction !== "select" &&
      props.buildAction !== "delete"
        ? "crosshair"
        : "";
  }, [props.buildAction, props.interactionMode]);

  useEffect(() => {
    const drawingMode =
      props.interactionMode === "build" &&
      props.buildAction !== "select" &&
      props.buildAction !== "delete";
    if (drawingMode) return;
    hoverStopIdRef.current = null;
    if (hoverPointRef.current) {
      hoverPointRef.current = null;
    }
    if (cursorHintRef.current) {
      cursorHintRef.current.style.display = "none";
    }
    setHint(null);
    setPlacementHint(null);
    refreshBuildPreview();
    applyInteractionFilters();
  }, [applyInteractionFilters, props.buildAction, props.interactionMode, refreshBuildPreview]);

  useEffect(() => {
    refreshBuildPreview();
  }, [
    props.buildAction,
    props.interactionMode,
    props.previewAnchorPoint,
    props.previewColor,
    props.scenario?.meta?.crs?.type,
    refreshBuildPreview,
  ]);

  useEffect(() => {
    applyInteractionFilters();
  }, [
    applyInteractionFilters,
    props.activeLineId,
    props.selectedLineId,
    props.selectedStopId,
    selectedVehicleId,
    props.scenario?.world.services,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !props.startCenter || !props.mapRuntimeConfig?.default_bounds || initialCenteredRef.current) return;
    map.fitBounds(props.mapRuntimeConfig.default_bounds as LngLatBoundsLike, { padding: 44, duration: 0, maxZoom: 5.2 });
    initialCenteredRef.current = true;
  }, [props.mapRuntimeConfig?.default_bounds, props.startCenter]);

  useEffect(() => {
    let cancelled = false;
    assetCollectionCacheRef.current.clear();
    setWorldContextData(fc());
    setMajorRoadData(fc());
    emitBootProgress({
      stage: "map_context",
      progress: 0.72,
      message: "Loading world map context...",
    });
    const loadFromRuntimeUrls = async (): Promise<boolean> => {
      const cfg = props.mapRuntimeConfig;
      if (!cfg) return false;
      let loadedWorld = false;
      if (cfg.world_context_url?.trim()) {
        try {
          const collection = await loadBasemapCollection(cfg.world_context_url);
          if (!cancelled) setWorldContextData(collection);
          loadedWorld = collection.features.length > 0;
        } catch {
          if (!cancelled) setWorldContextData(fc());
        }
      }
      if (cfg.major_roads_url?.trim()) {
        try {
          const collection = await loadBasemapCollection(cfg.major_roads_url);
          if (!cancelled) setMajorRoadData(collection);
        } catch {
          if (!cancelled) setMajorRoadData(fc());
        }
      }
      return loadedWorld;
    };

    const loadFromContextFallback = async (): Promise<boolean> => {
      const projectPath = props.projectPath?.trim();
      if (!projectPath) return false;
      try {
        const context = (await loadCountryMapContext(projectPath)) as CountryMapContext;
        if (cancelled) return false;
        const world = normalizeBasemapFeatureCollection(context.world_context);
        const roads = normalizeBasemapFeatureCollection(context.major_roads);
        setWorldContextData(world);
        if (roads.features.length > 0) {
          setMajorRoadData(roads);
        }
        return world.features.length > 0;
      } catch {
        return false;
      }
    };

    void (async () => {
      const loadedWorldFromUrls = await loadFromRuntimeUrls();
      if (!loadedWorldFromUrls) {
        const loadedFallback = await loadFromContextFallback();
        if (!loadedFallback && !cancelled) {
          setWorldContextData(fc());
          emitBootProgress({
            stage: "error",
            progress: 0.82,
            message: "Map context did not load.",
            error: "No world context data was available for this session.",
          });
        } else if (!cancelled) {
          emitBootProgress({
            stage: "map_context",
            progress: 0.84,
            message: "Fallback map context loaded.",
          });
        }
      } else if (!cancelled) {
        emitBootProgress({
          stage: "map_context",
          progress: 0.84,
          message: "Map context ready.",
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    emitBootProgress,
    loadBasemapCollection,
    props.mapRuntimeConfig,
    props.projectPath,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !focusRegion || !loadedRef.current) return;
    if (previousFocusRef.current === focusRegion.region_id) return;
    previousFocusRef.current = focusRegion.region_id;
    if (!initialCenteredRef.current) return;
    const bounds = boundsFromGeometry(resolveRegionGeometry(focusRegion));
    if (bounds) {
      map.fitBounds(bounds, { padding: 64, duration: 700, maxZoom: 10.2 });
    }
  }, [focusRegion, resolveRegionGeometry]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !styleReady) return;
    const shouldLogBuildPerf = props.interactionMode === "build";
    const applySourcesAndVisibility = () => {
      ensureMapLayers(map);
      const setDataTimed = (
        sourceId: string,
        label: string,
        data: GeoCollection | GeoJsonFeatureCollection
      ) => {
        const started = performance.now();
        setData(map, sourceId, data);
        const elapsedMs = performance.now() - started;
        const featureCount =
          data && typeof data === "object" && Array.isArray((data as { features?: unknown[] }).features)
            ? (data as { features: unknown[] }).features.length
            : null;
        if (shouldLogBuildPerf && elapsedMs >= 2) {
          buildPerfEvent(`map.${label}`, {
            sourceId,
            featureCount,
            elapsedMs: Number(elapsedMs.toFixed(2)),
          });
        }
      };
      setDataTimed(SRC_WORLD, "world_source_update", worldCountryData);
      setDataTimed(SRC_WORLD_LABELS, "world_label_source_update", worldCountryLabelData);
      setDataTimed(SRC_REGIONS, "region_source_update", regionDisplayFeatures);
      setDataTimed(SRC_REGION_HEXES, "region_hex_source_update", planningHexData);
      setDataTimed(SRC_HEX_COVERAGE_GAPS, "hex_gap_source_update", hexCoverageGapData);
      setDataTimed(SRC_MAJOR_ROADS, "major_roads_source_update", majorRoadData);
      setDataTimed(SRC_LINKS, "draft_line_source_update", networkData.links);
      setDataTimed(SRC_TRANSFERS, "transfer_source_update", networkData.transfers);
      setDataTimed(SRC_STOPS, "station_overlay_refresh", networkData.stops);
      setDataTimed(SRC_ZONES, "zone_source_update", networkData.zones);
      setVisibility(map, "world-ocean-fill", true);
      setVisibility(map, "links-major", props.showLinks);
      setVisibility(map, "links-trunk", props.showLinks);
      setVisibility(map, "links-focus", props.showLinks);
      setVisibility(map, "transfers-focus", props.showLinks);
      setVisibility(map, "transfers-focus-label", props.showLinks);
      setVisibility(map, "links-selected-glow", props.showLinks);
      setVisibility(map, "links-selected-casing", props.showLinks);
      setVisibility(map, "links-selected", props.showLinks);
      setVisibility(map, "links-selection-dim", props.showLinks);
      setVisibility(map, "links-active", props.showLinks);
      setVisibility(map, "links-hit", props.showLinks);
      setVisibility(map, "stops-major", props.showStations);
      setVisibility(map, "stops-focus", props.showStations);
      setVisibility(map, "stops-focus-symbol", props.showStations);
      setVisibility(map, "stops-focus-badge", props.showStations);
      setVisibility(map, "stops-build-hover-ring", props.showStations);
      setVisibility(map, "stops-selected-halo", props.showStations);
      setVisibility(map, "stops-selected", props.showStations);
      setVisibility(map, "stops-selected-line-ring", props.showStations);
      setVisibility(map, "stops-hit", props.showStations);
      setVisibility(map, "stops-shape", props.showShapeStops);
      setVisibility(map, "zone-centroids", props.showZoneCentroids);
      refreshBuildPreview();
      applyInteractionFilters();
      if (!bootReadyEmittedRef.current) {
        bootReadyEmittedRef.current = true;
        emitBootProgress({
          stage: "ready",
          progress: 1,
          message: "Map and transit layers ready.",
        });
      }
    };
    if (shouldLogBuildPerf) {
      buildPerfEvent("map.build_mode_sources_refresh.requested", {
        buildAction: props.buildAction,
        lineFeatureCount: networkData.links.features.length,
        stationFeatureCount: networkData.stops.features.length,
        transferFeatureCount: networkData.transfers.features.length,
      });
      buildPerfMeasure(
        "map.build_mode_sources_refresh",
        applySourcesAndVisibility,
        {
          buildAction: props.buildAction,
          showLinks: props.showLinks,
          showStations: props.showStations,
        },
        { minDurationMs: 0, throttleMs: 100 }
      );
    } else {
      applySourcesAndVisibility();
    }
  }, [
    applyInteractionFilters,
    emitBootProgress,
    props.buildAction,
    props.interactionMode,
    styleReady,
    regionDisplayFeatures,
    planningHexData,
    hexCoverageGapData,
    majorRoadData,
    networkData,
    props.showLinks,
    props.showStations,
    props.showShapeStops,
    props.showZoneCentroids,
    refreshBuildPreview,
    usesVectorBasemap,
    worldCountryData,
    worldCountryLabelData,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !styleReady) return;
    ensureMapLayers(map);
    setData(map, SRC_DEMAND_CELLS, demandOverlayData.cells);
    setVisibility(map, "demand-cell-fill", showDemandCells);
    setVisibility(map, "demand-cell-outline", showDemandCells);
    setVisibility(map, "demand-cell-selected-outline", showDemandCells);
    setVisibility(map, "demand-cell-hit", showDemandCells);
    console.info(
      `[demand-overlay-map] features_set=${demandOverlayFeatureCount} source=${SRC_DEMAND_CELLS} show=${showDemandCells}`
    );
    applyInteractionFilters();
  }, [
    applyInteractionFilters,
    demandOverlayData,
    demandOverlayFeatureCount,
    showDemandCells,
    styleReady,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !styleReady) return;
    const authoringEnabled = allowHexAuthoring && authoringMode;
    const hexLayers = [
      "planning-hex-fill",
      "planning-hex-outline",
      "planning-hex-number",
      "planning-hex-selected-outline",
      "planning-hex-hover-outline",
      "planning-hex-gap-fill",
      "planning-hex-gap-hit",
      "planning-hex-gap-outline",
      "planning-hex-hit",
    ];
    for (const layerId of hexLayers) {
      setVisibility(map, layerId, authoringEnabled);
    }
    if (map.getLayer("planning-hex-selected-outline")) {
      map.setFilter(
        "planning-hex-selected-outline",
        ["==", ["get", "hex_id"], selectedHex?.hexId ?? "__none__"] as never
      );
    }
    if (map.getLayer("planning-hex-hover-outline")) {
      map.setFilter(
        "planning-hex-hover-outline",
        ["==", ["get", "hex_id"], hoveredHex?.hexId ?? "__none__"] as never
      );
    }
    if (map.getLayer("region-fill")) {
      map.setPaintProperty(
        "region-fill",
        "fill-color",
        (demandOverlayAnyVisible
          ? [
              "case",
              ["==", ["get", "unlocked"], 1],
              "#e3edf8",
              "#4a5561",
            ]
          : [
              "case",
              ["==", ["get", "unlocked"], 1],
              "#e8f1fb",
              "#4f5966",
            ]) as never
      );
      const normalFillOpacity = [
        "interpolate",
        ["linear"],
        ["zoom"],
        4,
        ["case", ["==", ["get", "unlocked"], 1], 0.05, 0.36],
        8,
        ["case", ["==", ["get", "unlocked"], 1], 0.038, 0.31],
        11.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.026, 0.26],
        14,
        ["case", ["==", ["get", "unlocked"], 1], 0.02, 0.22],
      ];
      const demandFillOpacity = [
        "interpolate",
        ["linear"],
        ["zoom"],
        4,
        ["case", ["==", ["get", "unlocked"], 1], 0.03, 0.29],
        8,
        ["case", ["==", ["get", "unlocked"], 1], 0.024, 0.25],
        11.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.02, 0.21],
        14,
        ["case", ["==", ["get", "unlocked"], 1], 0.016, 0.18],
      ];
      const authoringFillOpacity = [
        "interpolate",
        ["linear"],
        ["zoom"],
        4,
        ["case", ["==", ["get", "unlocked"], 1], 0.025, 0.08],
        8,
        ["case", ["==", ["get", "unlocked"], 1], 0.018, 0.06],
        11.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.012, 0.042],
        14,
        ["case", ["==", ["get", "unlocked"], 1], 0.008, 0.03],
      ];
      map.setPaintProperty(
        "region-fill",
        "fill-opacity",
        (authoringEnabled
          ? authoringFillOpacity
          : demandOverlayAnyVisible
            ? demandFillOpacity
            : normalFillOpacity) as never
      );
    }
    if (map.getLayer("region-outline")) {
      const defaultOutlineOpacity = [
        "interpolate",
        ["linear"],
        ["zoom"],
        4,
        ["case", ["==", ["get", "unlocked"], 1], 0.3, 0.8],
        8.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.4, 0.88],
        11.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.5, 0.96],
      ];
      const demandOutlineOpacity = [
        "interpolate",
        ["linear"],
        ["zoom"],
        4,
        ["case", ["==", ["get", "unlocked"], 1], 0.56, 0.84],
        8.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.72, 0.9],
        11.5,
        ["case", ["==", ["get", "unlocked"], 1], 0.86, 0.97],
      ];
      map.setPaintProperty(
        "region-outline",
        "line-color",
        (demandOverlayAnyVisible
          ? [
              "case",
              ["==", ["get", "unlocked"], 1],
              "#586f84",
              "#2a3642",
            ]
          : [
              "case",
              ["==", ["get", "unlocked"], 1],
              "#6e849a",
              "#2f3945",
            ]) as never
      );
      map.setPaintProperty(
        "region-outline",
        "line-width",
        (demandOverlayAnyVisible
          ? ["interpolate", ["linear"], ["zoom"], 4, 1.0, 8.5, 1.35, 11.5, 2.05]
          : ["interpolate", ["linear"], ["zoom"], 4, 0.9, 8.5, 1.2, 11.5, 1.9]) as never
      );
      map.setPaintProperty(
        "region-outline",
        "line-opacity",
        (demandOverlayAnyVisible ? demandOutlineOpacity : defaultOutlineOpacity) as never
      );
    }
    setVisibility(map, "region-unlocked-outline", demandOverlayAnyVisible);
    setVisibility(map, "region-focus-outline", false);

    if (!authoringEnabled) {
      hoveredHexIdRef.current = null;
      setHoveredHex(null);
      setVisibleHexCount(0);
      return;
    }

    const refreshVisibleHexCount = () => {
      const seen = new Set<string>();
      for (const feature of map.queryRenderedFeatures({
        layers: ["planning-hex-hit", "planning-hex-gap-hit"],
      })) {
        const hexId = feature.properties?.hex_id;
        if (typeof hexId === "string" && hexId.trim().length > 0) seen.add(hexId.trim());
      }
      setVisibleHexCount(seen.size);
    };
    refreshVisibleHexCount();
    map.on("moveend", refreshVisibleHexCount);
    map.on("zoomend", refreshVisibleHexCount);
    return () => {
      map.off("moveend", refreshVisibleHexCount);
      map.off("zoomend", refreshVisibleHexCount);
    };
  }, [
    allowHexAuthoring,
    authoringMode,
    demandOverlayAnyVisible,
    hoveredHex?.hexId,
    selectedHex?.hexId,
    styleReady,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !styleReady) return;
    ensureMapLayers(map);
    const activeVehicleData = activeVehicleOverlayCollection({
      gameMode,
      trainsAuthoritative: props.trainsAuthoritative,
      runtimeVehicleGeoJson: runtimeVehicleGeoJsonRef.current,
      vehicleGeoJson: vehicleData.geojson,
    });
    setData(map, SRC_VEHICLES, activeVehicleData);
    setVisibility(map, "vehicles-selected-halo", props.showLinks);
    setVisibility(map, "vehicles-point", props.showLinks);
    setVisibility(map, "vehicles-hit", props.showLinks);
    applyInteractionFilters();
  }, [
    applyInteractionFilters,
    gameMode,
    props.showLinks,
    props.trainsAuthoritative,
    runtimeVehicleGeoJsonVersion,
    styleReady,
    vehicleData.geojson,
  ]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current) return;
    for (const item of labelMarkersRef.current) item.marker.remove();
    labelMarkersRef.current = [];
    if (usesVectorBasemap) return;
    const visibleLabels = regionLabelData.filter((label) => unlockedRegionIds.has(label.regionId));
    labelMarkersRef.current = visibleLabels.map((label) => {
      const element = document.createElement("div");
      element.className = "region-label-marker";
      element.textContent = label.name;
      element.style.color = "#5b6e84";
      element.style.fontSize = "12px";
      element.style.fontWeight = "600";
      element.style.textShadow = "0 0 3px rgba(255,255,255,0.95), 0 0 8px rgba(255,255,255,0.95)";
      element.style.pointerEvents = "none";
      element.style.whiteSpace = "nowrap";
      const marker = new maplibregl.Marker({ element, anchor: "center" })
        .setLngLat(label.point)
        .addTo(map);
      return { marker, element };
    });

    const syncVisibility = () => {
      const visible = map.getZoom() >= 7.9;
      for (const item of labelMarkersRef.current) {
        item.element.style.display = visible ? "block" : "none";
      }
    };
    syncVisibility();
    map.on("zoom", syncVisibility);
    return () => {
      map.off("zoom", syncVisibility);
      for (const item of labelMarkersRef.current) item.marker.remove();
      labelMarkersRef.current = [];
    };
  }, [
    regionLabelData,
    unlockedRegionIds,
    usesVectorBasemap,
  ]);

  const pinnedHex = selectedHex;
  const activeHex = pinnedHex ?? hoveredHex;

  return (
    <div style={{ width: "100%", height: "100%", position: "relative" }}>
      <div ref={containerRef} style={{ width: "100%", height: "100%" }} />
      <div ref={cursorHintRef} className="map-cursor-hint" style={{ display: "none" }} />
      {allowHexAuthoring ? (
        <div
          style={{
            position: "absolute",
            right: 18,
            top: 132,
            display: "flex",
            flexDirection: "column",
            gap: 8,
            alignItems: "flex-end",
            zIndex: 9000,
            pointerEvents: "auto",
          }}
        >
          <button
            type="button"
            onClick={() =>
              setAuthoringMode((value) => {
                const next = !value;
                authoringModeRef.current = next;
                if (!next) {
                  hoveredHexIdRef.current = null;
                  setHoveredHex(null);
                  setSelectedHex(null);
                  setVisibleHexCount(0);
                }
                return next;
              })
            }
            style={{
              appearance: "none",
              border: authoringMode ? "1px solid #1d5ea8" : "1px solid #8aa7c5",
              background: authoringMode ? "rgba(210,231,255,0.98)" : "rgba(244,250,255,0.98)",
              color: authoringMode ? "#123e70" : "#2f4f6f",
              borderRadius: 12,
              fontSize: 12,
              fontWeight: 800,
              letterSpacing: 0.24,
              padding: "9px 12px",
              cursor: "pointer",
              boxShadow: "0 12px 30px rgba(17,45,78,0.24)",
            }}
          >
            {authoringMode ? "Hex Authoring: ON (Alt+H)" : "Hex Authoring: OFF (Alt+H)"}
          </button>
          {authoringMode ? (
            <div
              style={{
                minWidth: 260,
                maxWidth: 340,
                padding: "10px 12px",
                borderRadius: 12,
                border: "1px solid #c7d4e1",
                background: "rgba(250,253,255,0.96)",
                color: "#31485f",
                boxShadow: "0 14px 30px rgba(20,44,71,0.12)",
                fontSize: 12,
                lineHeight: 1.4,
              }}
            >
            <div style={{ fontWeight: 700, marginBottom: 4 }}>Hex Reference</div>
            <div
              style={{
                marginBottom: 6,
                padding: "5px 7px",
                borderRadius: 8,
                border: "1px solid #a9c4e2",
                background: "rgba(217,233,251,0.9)",
                color: "#1f4f82",
                fontSize: 11,
                fontWeight: 800,
                letterSpacing: 0.25,
              }}
            >
              AUTHORING MODE ACTIVE · HEX OVERRIDE
            </div>
            <div>Visible hexes: {visibleHexCount.toLocaleString()}</div>
            <div>Manual-assigned: {planningHexSummary.manualAssigned.toLocaleString()}</div>
            <div style={{ color: planningHexSummary.unassigned > 0 ? "#8d1f1f" : "#2f5a37", fontWeight: 700 }}>
              Unassigned: {planningHexSummary.unassigned.toLocaleString()}
            </div>
            <div>
              Hex validity: {planningHexSummary.resolved.toLocaleString()} resolved /{" "}
              {planningHexSummary.total.toLocaleString()} total
            </div>
            {planningHexSummary.unresolved > 0 ? (
              <div style={{ color: "#80593c" }}>
                Unresolved IDs: {planningHexSummary.unresolved.toLocaleString()}
              </div>
            ) : null}
            {ukHexCoverageDiagnostics.error ? (
              <div style={{ color: "#8a2c2c" }}>
                Coverage audit error: {ukHexCoverageDiagnostics.error}
              </div>
            ) : ukHexCoverageDiagnostics.expected_land_hexes > 0 ? (
              <>
                <div>
                  UK land coverage: {ukHexCoverageDiagnostics.covered_hexes.toLocaleString()} /{" "}
                  {ukHexCoverageDiagnostics.expected_land_hexes.toLocaleString()} (
                  {(ukHexCoverageDiagnostics.coverage_ratio * 100).toFixed(1)}%)
                </div>
                {ukHexCoverageDiagnostics.missing_land_hexes > 0 ? (
                  <div style={{ color: "#8d1f1f", fontWeight: 700 }}>
                    Missing UK land hexes: {ukHexCoverageDiagnostics.missing_land_hexes.toLocaleString()}
                  </div>
                ) : (
                  <div style={{ color: "#2f5a37", fontWeight: 700 }}>No UK land hex gaps detected.</div>
                )}
              </>
            ) : (
              <div style={{ color: "#6a7f93" }}>UK coverage audit unavailable for current map context.</div>
            )}
            <div style={{ marginTop: 8, color: "#48627c" }}>
              {activeHex
                ? "Hover any hex for live ID. Click to pin it."
                : "Move over UK hexes. No hex at cursor means a real coverage gap or non-hex area."}
            </div>
            {ukHexCoverageDiagnostics.missing_land_hexes > 0 ? (
              <div
                style={{
                  marginTop: 8,
                  padding: "8px 9px",
                  borderRadius: 9,
                  border: "1px solid #e3b3b3",
                  background: "rgba(255,239,239,0.9)",
                  color: "#7d1e1e",
                }}
              >
                Missing sample:{" "}
                {ukHexCoverageDiagnostics.missing_hex_ids.slice(0, 6).join(", ")}
              </div>
            ) : null}
            <div
              style={{
                marginTop: 8,
                padding: "8px 9px",
                borderRadius: 9,
                border: "1px solid #d8e2ee",
                background: "rgba(243,248,255,0.88)",
              }}
            >
              <div style={{ fontWeight: 700, marginBottom: 2 }}>Hover</div>
              <div style={{ color: "#3d5671" }}>
                {hoveredHex
                  ? `Hex #${hoveredHex.hexNumber ?? "—"} · ${hoveredHex.hexId}`
                  : "No hex under cursor"}
              </div>
            </div>
            <div
              style={{
                marginTop: 8,
                padding: "8px 9px",
                borderRadius: 9,
                border: "1px solid #ccd8e6",
                background: "rgba(236,244,254,0.92)",
              }}
            >
              <div style={{ fontWeight: 700, marginBottom: 2 }}>Pinned (click)</div>
              <div style={{ color: "#304b66" }}>
                {pinnedHex
                  ? `Hex #${pinnedHex.hexNumber ?? "—"} · ${pinnedHex.hexId}`
                  : "Click a visible hex to pin"}
              </div>
              {pinnedHex ? (
                <button
                  type="button"
                  onClick={() => setSelectedHex(null)}
                  style={{
                    marginTop: 6,
                    appearance: "none",
                    border: "1px solid #b9c9da",
                    background: "#f7fbff",
                    color: "#365678",
                    borderRadius: 8,
                    padding: "4px 8px",
                    fontSize: 11,
                    fontWeight: 700,
                    cursor: "pointer",
                  }}
                >
                  Clear Pin
                </button>
              ) : null}
            </div>
            {activeHex ? (
              <div
                style={{
                  marginTop: 8,
                  paddingTop: 8,
                  borderTop: "1px solid #d9e3ee",
                  display: "grid",
                  gap: 4,
                }}
              >
                <div>
                  <strong>Hex #:</strong>{" "}
                  {activeHex.hexNumber !== null ? activeHex.hexNumber.toLocaleString() : "—"}
                </div>
                <div>
                  <strong>Hex ID:</strong> {activeHex.hexId}
                </div>
                <div>
                  <strong>Region:</strong> {activeHex.regionName}
                </div>
                <div>
                  <strong>Region ID:</strong> {activeHex.regionId}
                </div>
                <div>
                  <strong>Status:</strong> {activeHex.unlocked ? "Unlocked" : "Locked"}
                  {!activeHex.resolved ? " (fallback id)" : ""}
                </div>
                <div>
                  <strong>Assignment:</strong>{" "}
                  {activeHex.unassigned ? "Unassigned authoring hex" : activeHex.assignmentState}
                </div>
                <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                  <button
                    type="button"
                    onClick={() => {
                      if (activeHex.hexNumber === null) return;
                      void navigator.clipboard?.writeText(String(activeHex.hexNumber));
                      setHint(`Copied hex number ${activeHex.hexNumber}`);
                      if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
                      hintTimerRef.current = window.setTimeout(() => setHint(null), 1800);
                    }}
                    style={{
                      marginTop: 2,
                      appearance: "none",
                      border: "1px solid #b9c9da",
                      background: "#f3f8fe",
                      color: "#2e4f76",
                      borderRadius: 8,
                      padding: "5px 8px",
                      fontSize: 11,
                      fontWeight: 700,
                      cursor: "pointer",
                    }}
                  >
                    Copy Hex #
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      const text = activeHex.hexNumber !== null
                        ? `${activeHex.hexNumber}\t${activeHex.hexId}`
                        : activeHex.hexId;
                      void navigator.clipboard?.writeText(text);
                      setHint("Copied hex reference line");
                      if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
                      hintTimerRef.current = window.setTimeout(() => setHint(null), 1800);
                    }}
                    style={{
                      marginTop: 2,
                      appearance: "none",
                      border: "1px solid #b9c9da",
                      background: "#eef6ff",
                      color: "#2e4f76",
                      borderRadius: 8,
                      padding: "5px 8px",
                      fontSize: 11,
                      fontWeight: 700,
                      cursor: "pointer",
                    }}
                  >
                    Copy Ref Line
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      void navigator.clipboard?.writeText(activeHex.hexId);
                      setHint(`Copied hex ID ${activeHex.hexId}`);
                      if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
                      hintTimerRef.current = window.setTimeout(() => setHint(null), 1800);
                    }}
                    style={{
                      marginTop: 2,
                      appearance: "none",
                      border: "1px solid #b9c9da",
                      background: "#eef4fb",
                      color: "#2e4f76",
                      borderRadius: 8,
                      padding: "5px 8px",
                      fontSize: 11,
                      fontWeight: 700,
                      cursor: "pointer",
                    }}
                  >
                    Copy Hex ID
                  </button>
                </div>
              </div>
            ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
      {hint && (
        <div
          style={{
            position: "absolute",
            left: 20,
            bottom: 20,
            padding: "10px 14px",
            borderRadius: 14,
            background: "rgba(255,255,255,0.94)",
            border: "1px solid #c6d0da",
            color: "#32475f",
            fontSize: 14,
            fontWeight: 600,
            pointerEvents: "none",
            boxShadow: "0 12px 32px rgba(31,52,79,0.08)",
          }}
        >
          {hint}
        </div>
      )}
      {placementHint ? (
        <div
          style={{
            position: "absolute",
            left: placementHint.x + 14,
            top: placementHint.y + 16,
            padding: "7px 10px",
            borderRadius: 10,
            background: "rgba(15, 25, 38, 0.94)",
            border: "1px solid rgba(138, 163, 196, 0.72)",
            color: "#f1f6ff",
            fontSize: 12,
            fontWeight: 700,
            pointerEvents: "none",
            boxShadow: "0 14px 30px rgba(8, 15, 26, 0.38)",
            maxWidth: 260,
            zIndex: 85,
          }}
        >
          {placementHint.message}
        </div>
      ) : null}
      {selectedDemandCell ? (
        <aside className="editor-drawer-sheet demand-cell-inspector-sheet">
          <div className="editor-drawer-head">
            <div>
              <p>Demand Cell</p>
              <h4>{selectedDemandOverlayLabel}</h4>
              <span className="vehicle-direction-badge">{selectedDemandCell.cellId}</span>
            </div>
            <button
              onClick={() => {
                setSelectedDemandCell(null);
                applyInteractionFilters();
              }}
            >
              Close
            </button>
          </div>
          <div className="vehicle-inspector-line">
            Planning Region: {selectedDemandCell.planningRegionId ?? "unmapped"}
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Allocated Residential</small>
              <strong>{formatDemandMetric(selectedDemandCell.allocatedResidentialMass)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Allocated Employment</small>
              <strong>{formatDemandMetric(selectedDemandCell.allocatedEmploymentMass)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Total Allocation</small>
              <strong>{formatDemandMetric(selectedDemandCell.totalAllocation)}</strong>
            </div>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Raw Residential Weight</small>
              <strong>{formatDemandMetric(selectedDemandCell.rawWeightResidential, 4)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Raw Employment Weight</small>
              <strong>{formatDemandMetric(selectedDemandCell.rawWeightEmployment, 4)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Fallback</small>
              <strong>{selectedDemandCell.fallbackReason ?? "none"}</strong>
            </div>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Area (m²)</small>
              <strong>{formatDemandMetric(selectedDemandCell.areaM2, 0)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Residents Signal</small>
              <strong>{formatDemandMetric(selectedDemandCell.residentsNight)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Jobs Signal</small>
              <strong>{formatDemandMetric(selectedDemandCell.jobsDay)}</strong>
            </div>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Quality</small>
              <strong>{formatDemandMetric(selectedDemandCell.dataQualityScore, 3)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Centrality</small>
              <strong>{formatDemandMetric(selectedDemandCell.centralityScore, 3)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Mix (R/O/Rt/Le/Ind/Edu/Health)</small>
              <strong>
                {[
                  selectedDemandCell.activityMixResidential,
                  selectedDemandCell.activityMixOffice,
                  selectedDemandCell.activityMixRetail,
                  selectedDemandCell.activityMixRecreation,
                  selectedDemandCell.activityMixIndustrial,
                  selectedDemandCell.activityMixEducation,
                  selectedDemandCell.activityMixHealth,
                ]
                  .map((value) => formatDemandMetric(value, 2))
                  .join(" / ")}
              </strong>
            </div>
          </div>
        </aside>
      ) : null}
    </div>
  );
}
