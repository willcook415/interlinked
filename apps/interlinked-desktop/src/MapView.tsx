import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import maplibregl, {
  type GeoJSONSource,
  type LngLatBoundsLike,
  type Map as MapLibreMap,
  type PointLike,
} from "maplibre-gl";
import type { LinkModeFilter } from "./ui/MapFiltersPanel";
import { EMPTY_LINE_FILTER, EMPTY_STOP_FILTER, SRC_BUILD_PREVIEW, SRC_COUNTIES, SRC_COUNTY_BASEMAP, SRC_LINKS, SRC_MAJOR_ROADS, SRC_STOPS, SRC_TRANSFERS, SRC_VEHICLES, SRC_WORLD, SRC_WORLD_LABELS, SRC_ZONES, ensureMapLayers } from "./map/style/ensureMapLayers";
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
  vehicleTypeLabel,
} from "./map/runtimeVehicleOverlay";
import type {
  CountryMapContext,
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
  boundsIntersect,
  flattenBounds,
  padBounds,
  pointInGeometry,
} from "./map/geo/geometry";
import {
  basemapTierForZoom,
  buildCountyBoundsData,
  buildCountyFeatures,
  buildCountyLabelData,
  buildWorldCountryData,
  buildWorldCountryLabelData,
  fetchFeatureCollection,
  makeUrlFromTemplate,
  mergeFeatureCollections,
  normalizeBasemapFeatureCollection,
  parseCountyGeometry,
} from "./map/data/worldContext";
import {
  buildNetworkGeojsonData,
  buildStopPointById,
  formatDistanceKm,
  modeColor,
} from "./map/data/networkGeojson";
import {
  activeVehicleOverlayCollection,
  buildBuildPreviewOverlay,
} from "./map/data/runtimeOverlays";

type LabelMarker = {
  marker: maplibregl.Marker;
  element: HTMLDivElement;
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

function setData(map: MapLibreMap, sourceId: string, data: GeoCollection | GeoJsonFeatureCollection): void {
  (map.getSource(sourceId) as GeoJSONSource | undefined)?.setData(data as never);
}

function setVisibility(map: MapLibreMap, layerId: string, visible: boolean): void {
  if (!map.getLayer(layerId)) return;
  map.setLayoutProperty(layerId, "visibility", visible ? "visible" : "none");
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
  onMapPointAction?: (payload: MapWorldPoint) => void;
  onClearSelection?: () => void;
  onScrapVehicle?: (vehicleId: string) => void;
}) {
  const [forceCompatibilityBasemap, setForceCompatibilityBasemap] = useState(false);
  const vectorStyleUrl =
    forceCompatibilityBasemap || !props.mapRuntimeConfig?.map_ready
      ? null
      : props.mapRuntimeConfig?.style_url ?? null;
  const usesVectorBasemap = Boolean(vectorStyleUrl);
  const gameMode = props.sessionKind === "game";
  const containerRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const propsRef = useRef(props);
  const stopPointByIdRef = useRef<Map<string, MapWorldPoint & { name: string }>>(new Map());
  const loadedRef = useRef(false);
  const initialCenteredRef = useRef(false);
  const previousFocusRef = useRef<string | null>(null);
  const labelMarkersRef = useRef<LabelMarker[]>([]);
  const countyBasemapCacheRef = useRef<Map<string, Promise<GeoCollection> | GeoCollection>>(new Map());
  const lastViewportKeyRef = useRef<string>("");
  const suppressNextMapClickRef = useRef(false);
  const hoverPointRef = useRef<MapWorldPoint | null>(null);
  const hoverStopIdRef = useRef<string | null>(null);
  const selectedVehicleIdRef = useRef<string | null>(null);
  const vehicleByIdRef = useRef<Map<string, VehicleSnapshot>>(new Map());
  const runtimeVehicleFeatureByIdRef = useRef<Map<string, OverlayGeoFeature>>(new Map());
  const runtimeVehicleGeoJsonRef = useRef<OverlayGeoCollection>(overlayCollection());
  const runtimeVehicleKeyframeRef = useRef<RuntimeVehicleKeyframe | null>(null);
  const runtimeVehicleTransportIntervalMsRef = useRef(120);
  const runtimeVehicleTransitionRef = useRef<RuntimeVehicleTransition | null>(null);
  const runtimeVehicleTransitionFrameRef = useRef<number | null>(null);
  const lastFocusedStopTokenRef = useRef<number | null>(null);
  const lastFocusedVehicleTokenRef = useRef<number | null>(null);
  const cursorHintRef = useRef<HTMLDivElement | null>(null);
  const lockedCountyLookupRef = useRef<(point: XY) => string | null>(() => null);
  const [styleReady, setStyleReady] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [runtimeVehicleGeoJsonVersion, setRuntimeVehicleGeoJsonVersion] = useState(0);
  const [selectedVehicleId, setSelectedVehicleId] = useState<string | null>(null);
  const [worldContextData, setWorldContextData] = useState<GeoCollection>(fc());
  const [countyGeometryData, setCountyGeometryData] = useState<GeoCollection>(fc());
  const [majorRoadData, setMajorRoadData] = useState<GeoCollection>(fc());
  const [countyBasemapData, setCountyBasemapData] = useState<GeoCollection>(fc());
  const hintTimerRef = useRef<number | null>(null);
  const bootReadyEmittedRef = useRef(false);

  useEffect(() => {
    setForceCompatibilityBasemap(false);
    bootReadyEmittedRef.current = false;
  }, [props.mapRuntimeConfig?.style_url, props.projectPath]);

  useEffect(() => {
    propsRef.current = props;
  }, [props]);

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

  useEffect(() => {
    selectedVehicleIdRef.current = selectedVehicleId;
  }, [selectedVehicleId]);

  const countyGeometryByRegionId = useMemo(() => {
    const out = new Map<string, GeoJsonGeometry>();
    for (const feature of countyGeometryData.features) {
      const regionId = feature.properties?.region_id;
      if (typeof regionId === "string" && feature.geometry.type !== "Point" && feature.geometry.type !== "LineString") {
        out.set(regionId, feature.geometry as GeoJsonGeometry);
      }
    }
    return out;
  }, [countyGeometryData]);

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
      return countyGeometryByRegionId.get(region.region_id) ?? parseCountyGeometry(region);
    },
    [countyGeometryByRegionId]
  );

  const countyFeatures = useMemo<GeoCollection>(
    () =>
      buildCountyFeatures({
        regions: props.regions,
        focusRegionId: props.focusRegionId,
        selectedRegionId: props.selectedRegionId,
        resolveRegionGeometry,
      }),
    [props.regions, props.focusRegionId, props.selectedRegionId, resolveRegionGeometry]
  );

  const lockedCountyNameAtPoint = useCallback(
    (point: XY): string | null => {
      for (const feature of countyFeatures.features) {
        if (feature.geometry.type === "Point" || feature.geometry.type === "LineString") continue;
        if (Number(feature.properties?.unlocked ?? 0) === 1) continue;
        if (!pointInGeometry(point, feature.geometry as GeoJsonGeometry)) continue;
        const name = feature.properties?.name;
        return typeof name === "string" && name.trim().length > 0 ? name : "this county";
      }
      return null;
    },
    [countyFeatures]
  );

  useEffect(() => {
    lockedCountyLookupRef.current = lockedCountyNameAtPoint;
  }, [lockedCountyNameAtPoint]);

  const countyLabelData = useMemo(
    () =>
      buildCountyLabelData({
        regions: props.regions,
        focusRegionId: props.focusRegionId,
        resolveRegionGeometry,
      }),
    [props.regions, props.focusRegionId, resolveRegionGeometry]
  );

  const countyBoundsData = useMemo(
    () =>
      buildCountyBoundsData({
        regions: props.regions,
        resolveRegionGeometry,
      }),
    [props.regions, resolveRegionGeometry]
  );

  const networkData = useMemo(
    () =>
      buildNetworkGeojsonData({
        scenario: props.scenario,
        linkMode: props.linkMode,
        visibleCountryIso2: props.visibleCountryIso2,
        focusRegion,
        resolveRegionGeometry,
      }),
    [
      props.linkMode,
      props.scenario,
      props.visibleCountryIso2,
      focusRegion,
      resolveRegionGeometry,
    ]
  );

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
      setSelectedVehicleId(null);
    }
  }, [selectedVehicleId, vehicleData.byId]);

  const selectedVehicle = useMemo(
    () => (selectedVehicleId ? vehicleData.byId.get(selectedVehicleId) ?? null : null),
    [selectedVehicleId, vehicleData.byId]
  );

  const loadBasemapCollection = useCallback(async (url: string): Promise<GeoCollection> => {
    const cached = countyBasemapCacheRef.current.get(url);
    if (cached) {
      return cached instanceof Promise ? cached : Promise.resolve(cached);
    }
    const pending = fetchFeatureCollection(url)
      .then((collection) => {
        countyBasemapCacheRef.current.set(url, collection);
        return collection;
      })
      .catch((error) => {
        countyBasemapCacheRef.current.delete(url);
        throw error;
      });
    countyBasemapCacheRef.current.set(url, pending);
    return pending;
  }, []);

  const refreshBuildPreview = useCallback(() => {
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
  }, []);

  const applyInteractionFilters = useCallback(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current) return;
    const current = propsRef.current;
    const selectedLineId = current.selectedLineId?.trim() ? current.selectedLineId : "__none__";
    const selectedStopId = current.selectedStopId?.trim() ? current.selectedStopId : "__none__";
    const activeLineId = current.activeLineId?.trim() ? current.activeLineId : "__none__";
    const hoverStopId = hoverStopIdRef.current?.trim() ? hoverStopIdRef.current : "__none__";
    const selectedVehicleId = selectedVehicleIdRef.current?.trim() ? selectedVehicleIdRef.current : "__none__";
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
  }, []);

  const refreshViewportBasemap = useCallback(() => {
    const map = mapRef.current;
    if (!map || usesVectorBasemap || !props.mapRuntimeConfig?.map_ready) {
      setCountyBasemapData(fc());
      return;
    }
    const tier = basemapTierForZoom(map.getZoom());
    if (tier === "none") {
      lastViewportKeyRef.current = "none";
      setCountyBasemapData(fc());
      return;
    }
    const bounds = map.getBounds();
    const viewportBounds = padBounds(
      [bounds.getWest(), bounds.getSouth(), bounds.getEast(), bounds.getNorth()],
      tier === "full" ? 0.12 : 0.4
    );
    const center = map.getCenter();
    const visible = countyBoundsData
      .filter((entry) => boundsIntersect(entry.bounds, viewportBounds))
      .sort((a, b) => {
        const aCenterLng = (a.bounds[0] + a.bounds[2]) * 0.5;
        const aCenterLat = (a.bounds[1] + a.bounds[3]) * 0.5;
        const bCenterLng = (b.bounds[0] + b.bounds[2]) * 0.5;
        const bCenterLat = (b.bounds[1] + b.bounds[3]) * 0.5;
        const aD2 = (aCenterLng - center.lng) ** 2 + (aCenterLat - center.lat) ** 2;
        const bD2 = (bCenterLng - center.lng) ** 2 + (bCenterLat - center.lat) ** 2;
        return aD2 - bD2;
      })
      .slice(0, map.getZoom() >= 12.5 ? 3 : 1);
    const template =
      tier === "full"
        ? props.mapRuntimeConfig.county_basemap_full_url_template ?? props.mapRuntimeConfig.county_basemap_mid_url_template
        : props.mapRuntimeConfig.county_basemap_mid_url_template ?? props.mapRuntimeConfig.county_basemap_full_url_template;
    if (!template || visible.length === 0) {
      lastViewportKeyRef.current = `${tier}:empty`;
      setCountyBasemapData(fc());
      return;
    }
    const urls = visible
      .map((entry) => makeUrlFromTemplate(template, entry.countyId))
      .filter((url): url is string => Boolean(url))
      .sort();
    const viewportKey = `${tier}:${urls.join("|")}`;
    if (viewportKey === lastViewportKeyRef.current) return;
    lastViewportKeyRef.current = viewportKey;

    void Promise.all(urls.map((url) => loadBasemapCollection(url).catch(() => fc()))).then((collections) => {
      if (lastViewportKeyRef.current !== viewportKey) return;
      setCountyBasemapData(mergeFeatureCollections(collections));
    });
  }, [countyBoundsData, loadBasemapCollection, props.mapRuntimeConfig, usesVectorBasemap]);

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
          layers: [{ id: "background", type: "background", paint: { "background-color": "#d9e7f6" } }],
        } as never),
      center: props.startCenter ?? [-2.6, 54.4],
      zoom: props.startCenter ? 8.7 : 4.8,
      minZoom: 2,
      maxZoom: 17,
      maxPitch: 0,
      renderWorldCopies: false,
    });
    mapRef.current = map;
    const vectorStyleLoadGuard = window.setTimeout(() => {
      if (loadedRef.current || forceCompatibilityBasemap || !propsRef.current.mapRuntimeConfig?.style_url) {
        return;
      }
      setForceCompatibilityBasemap(true);
      setHint("Map style failed to load; retrying map rendering.");
      emitBootProgress({
        stage: "error",
        progress: 0.58,
        message: "Map style load timed out.",
        error: "Style timeout. Retry map load.",
      });
    }, 4500);
    map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "bottom-left");
    map.on("error", (event) => {
      const raw = String((event as { error?: unknown }).error ?? "Map rendering error");
      if (propsRef.current.mapRuntimeConfig?.style_url && !forceCompatibilityBasemap) {
        setForceCompatibilityBasemap(true);
      }
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
            layers: ["county-basemap-roads", "county-basemap-road-casing", "gb-major-roads"],
          });
          if (nearbyRoad.length === 0) return "Bus stops and links must follow roads.";
          return null;
        }
        const nearbyWater = map.queryRenderedFeatures(queryBox, {
          layers: ["county-basemap-water"],
        });
        if (nearbyWater.length === 0) {
          return "Ferry stops and links must stay on water or shoreline.";
        }
        return null;
      };
      const emitPointAction = (lng: number, lat: number) => {
        const current = propsRef.current;
        const buildAction = current.buildAction ?? "select";
        const buildingPoint =
          current.interactionMode === "build" &&
          (buildAction === "place_station" ||
            buildAction === "start_line" ||
            buildAction === "add_station_to_line");
        if (buildingPoint) {
          const blockedCounty = lockedCountyLookupRef.current({ lng, lat });
          if (blockedCounty) {
            if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
            setHint(`Unlock ${blockedCounty} before building here.`);
            hintTimerRef.current = window.setTimeout(() => setHint(null), 2200);
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
        const point = converted
          ? {
              lng,
              lat,
              x: converted.x,
              y: converted.y,
            }
          : null;
        if (point) propsRef.current.onMapPointAction?.(point);
      };
      const emitStopAction = (stopId: string, lng: number, lat: number) => {
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
          const blockedCounty = lockedCountyLookupRef.current({ lng: targetLng, lat: targetLat });
          if (blockedCounty) {
            if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
            setHint(`Unlock ${blockedCounty} before building here.`);
            hintTimerRef.current = window.setTimeout(() => setHint(null), 2200);
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
      };
      const interactiveNetworkLayers = ["stops-hit", "links-hit", "vehicles-hit"];
      map.on("click", "county-fill", (event) => {
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
        if (typeof regionId === "string" && regionId.length > 0) propsRef.current.onSelectCounty(regionId);
      });
      map.on("click", "world-country-fill", (event) => {
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
        const unlockedNow = Number(feature?.properties?.unlocked_now ?? 0) === 1;
        if (unlockedNow) return;
        const name = typeof feature?.properties?.name === "string" ? feature.properties.name : "This country";
        if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
        setHint(`${name} is coming soon.`);
        hintTimerRef.current = window.setTimeout(() => setHint(null), 2400);
      });
      const stopClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
        suppressNextMapClickRef.current = true;
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const stopId = feature?.properties?.id;
        if (typeof stopId !== "string" || stopId.length === 0) return;
        emitStopAction(stopId, event.lngLat.lng, event.lngLat.lat);
      };
      const lineClickHandler = (event: maplibregl.MapLayerMouseEvent) => {
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
          setSelectedVehicleId(null);
          emitStopAction(stopId, event.lngLat.lng, event.lngLat.lat);
          applyInteractionFilters();
          return;
        }
        const feature = event.features?.[0] as { properties?: Record<string, unknown> } | undefined;
        const vehicleId = feature?.properties?.vehicle_id;
        if (typeof vehicleId !== "string" || vehicleId.length === 0) return;
        setSelectedVehicleId(vehicleId);
        propsRef.current.onClearSelection?.();
        const snapshot = vehicleByIdRef.current.get(vehicleId);
        if (snapshot) {
          map.easeTo({
            center: [snapshot.lng, snapshot.lat],
            duration: 220,
            essential: true,
          });
        }
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
      map.on("click", (event) => {
        if (suppressNextMapClickRef.current) {
          suppressNextMapClickRef.current = false;
          return;
        }
        const current = propsRef.current;
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
              current.onStopAction({
                stopId: hoveredStopId!,
                point: {
                  lng: hoveredStop.lng,
                  lat: hoveredStop.lat,
                  x: hoveredStop.x,
                  y: hoveredStop.y,
                },
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
                current.onStopAction({
                  stopId: forgivingStopId,
                  point: {
                    lng: snapped.lng,
                    lat: snapped.lat,
                    x: snapped.x,
                    y: snapped.y,
                  },
                });
                return;
              }
            }
            emitPointAction(event.lngLat.lng, event.lngLat.lat);
            return;
          }
          if (buildAction === "select") {
            current.onClearSelection?.();
          }
          return;
        }
        setSelectedVehicleId(null);
        current.onClearSelection?.();
      });
      map.on("mousemove", (event) => {
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
              cursorHintText = "Click a station to lock line start";
            }
          }
          const modeHint = hoverPointRef.current
            ? modeConstraintHintAt(hoverPointRef.current.lng, hoverPointRef.current.lat)
            : null;
          setHint(modeHint);
          setCursorHint(cursorHintText, { x: event.point.x, y: event.point.y });
          refreshBuildPreview();
          applyInteractionFilters();
          return;
        }
        setCursorHint(null);
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
      for (const layerId of ["county-fill", "world-country-fill"]) {
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
      for (const layerId of [
        "stops-hit",
        "links-hit",
        "vehicles-hit",
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
      for (const item of labelMarkersRef.current) item.marker.remove();
      labelMarkersRef.current = [];
      map.remove();
      mapRef.current = null;
    };
  }, [
    applyInteractionFilters,
    emitBootProgress,
    forceCompatibilityBasemap,
    props.mapRuntimeConfig?.style_url,
    props.startCenter,
    refreshBuildPreview,
    vectorStyleUrl,
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
    setSelectedVehicleId(props.focusVehicleId);
    map.easeTo({
      center: [vehicle.lng, vehicle.lat],
      duration: 300,
      essential: true,
    });
    applyInteractionFilters();
  }, [applyInteractionFilters, props.focusVehicleId, props.focusVehicleToken, vehicleData.byId]);

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
    countyBasemapCacheRef.current.clear();
    lastViewportKeyRef.current = "";
    setWorldContextData(fc());
    setCountyGeometryData(fc());
    setMajorRoadData(fc());
    setCountyBasemapData(fc());
    emitBootProgress({
      stage: "map_context",
      progress: 0.72,
      message: "Loading world and county context...",
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
      if (cfg.counties_url?.trim()) {
        try {
          const collection = await loadBasemapCollection(cfg.counties_url);
          if (!cancelled) setCountyGeometryData(collection);
        } catch {
          if (!cancelled) setCountyGeometryData(fc());
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
          if (props.mapRuntimeConfig?.style_url && !forceCompatibilityBasemap) {
            setForceCompatibilityBasemap(true);
          }
          emitBootProgress({
            stage: "error",
            progress: 0.82,
            message: "Map context did not load.",
            error: "No world context data was available for this session.",
          });
        } else if (!cancelled) {
          if (props.mapRuntimeConfig?.style_url && !forceCompatibilityBasemap) {
            setForceCompatibilityBasemap(true);
          }
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
    forceCompatibilityBasemap,
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
    if (!map || !loadedRef.current || usesVectorBasemap) return;
    const rerender = () => {
      refreshViewportBasemap();
    };
    rerender();
    map.on("moveend", rerender);
    map.on("zoomend", rerender);
    return () => {
      map.off("moveend", rerender);
      map.off("zoomend", rerender);
    };
  }, [refreshViewportBasemap, usesVectorBasemap]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loadedRef.current || !styleReady) return;
    ensureMapLayers(map);
    setData(map, SRC_WORLD, worldCountryData);
    setData(map, SRC_WORLD_LABELS, worldCountryLabelData);
    setData(map, SRC_COUNTIES, countyFeatures);
    setData(map, SRC_MAJOR_ROADS, majorRoadData);
    setData(map, SRC_COUNTY_BASEMAP, countyBasemapData);
    setData(map, SRC_LINKS, networkData.links);
    setData(map, SRC_TRANSFERS, networkData.transfers);
    setData(map, SRC_STOPS, networkData.stops);
    setData(map, SRC_ZONES, networkData.zones);
    setVisibility(map, "world-ocean-fill", !usesVectorBasemap);
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
  }, [
    applyInteractionFilters,
    emitBootProgress,
    styleReady,
    countyFeatures,
    countyBasemapData,
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
    labelMarkersRef.current = countyLabelData.map((label) => {
      const element = document.createElement("div");
      element.className = "county-label-marker";
      element.textContent = label.name;
      element.style.color = label.focus ? "#315f8d" : "#5b6e84";
      element.style.fontSize = label.focus ? "15px" : "12px";
      element.style.fontWeight = label.focus ? "700" : "600";
      element.style.textShadow = "0 0 3px rgba(255,255,255,0.95), 0 0 8px rgba(255,255,255,0.95)";
      element.style.pointerEvents = "none";
      element.style.whiteSpace = "nowrap";
      const marker = new maplibregl.Marker({ element, anchor: "center" })
        .setLngLat(label.point)
        .addTo(map);
      return { marker, element };
    });

    const syncVisibility = () => {
      const visible = map.getZoom() >= 5.6;
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
  }, [countyLabelData, usesVectorBasemap]);

  return (
    <div style={{ width: "100%", height: "100%", position: "relative" }}>
      <div ref={containerRef} style={{ width: "100%", height: "100%" }} />
      <div ref={cursorHintRef} className="map-cursor-hint" style={{ display: "none" }} />
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
      {selectedVehicle ? (
        <aside className="editor-drawer-sheet vehicle-inspector-sheet">
          <div className="editor-drawer-head">
            <div>
              <p>Vehicle</p>
              <h4>
                {vehicleTypeLabel(selectedVehicle.mode)} #{selectedVehicle.vehicleOrdinal}
              </h4>
              <span className="vehicle-direction-badge">{selectedVehicle.destinationLabel}</span>
            </div>
            <button
              onClick={() => {
                setSelectedVehicleId(null);
                applyInteractionFilters();
              }}
            >
              Close
            </button>
          </div>
          <div className="vehicle-inspector-line">{selectedVehicle.lineName}</div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Vehicle Capacity</small>
              <strong>{Math.round(selectedVehicle.vehicleCapacity).toLocaleString()} pax</strong>
            </div>
            <div className="inspector-stat">
              <small>On Board</small>
              <strong>
                {selectedVehicle.passengersOnBoard >= 1
                  ? Math.round(selectedVehicle.passengersOnBoard).toLocaleString()
                  : selectedVehicle.passengersOnBoard > 0
                    ? "<1"
                    : "0"}{" "}
                pax
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Rolling Stock</small>
              <strong>{selectedVehicle.stockTierId ?? "standard"}</strong>
            </div>
          </div>
          {props.onScrapVehicle ? (
            <div className="editor-drawer-footer">
              <button
                className="danger-button"
                onClick={() => {
                  props.onScrapVehicle?.(selectedVehicle.vehicleId);
                  setSelectedVehicleId(null);
                  applyInteractionFilters();
                }}
              >
                Scrap Vehicle
              </button>
            </div>
          ) : null}
        </aside>
      ) : null}
    </div>
  );
}
