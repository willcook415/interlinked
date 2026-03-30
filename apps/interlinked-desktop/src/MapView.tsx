import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import maplibregl, {
  type GeoJSONSource,
  type LngLatBoundsLike,
  type Map as MapLibreMap,
  type PointLike,
} from "maplibre-gl";
import { cellToBoundary, isValidCell, latLngToCell } from "h3-js";
import { invoke } from "@tauri-apps/api/core";
import {
  canonicalModeClass as canonicalModeClassToken,
  isMajorMode,
  modeClassFromStopType as modeClassFromStopTypeToken,
} from "./modes";
import type { LinkModeFilter } from "./ui/MapFiltersPanel";
import { EMPTY_LINE_FILTER, EMPTY_STOP_FILTER, SRC_BUILD_PREVIEW, SRC_COUNTIES, SRC_COUNTY_BASEMAP, SRC_LINKS, SRC_MAJOR_ROADS, SRC_STOPS, SRC_TRANSFERS, SRC_VEHICLES, SRC_WORLD, SRC_WORLD_LABELS, SRC_ZONES, ensureMapLayers } from "./map/style/ensureMapLayers";
import type {
  CountryMapContext,
  GeoJsonAnyGeometry,
  GeoJsonFeatureCollection,
  GeoJsonGeometry,
  MapRuntimeConfig,
  RegionStatus,
  ScenarioLite,
  SessionKind,
  ServiceLite,
  SimulationClock,
  TrainRuntimeView,
} from "./types";

type XY = { lng: number; lat: number };
type GeoFeature = {
  type: "Feature";
  geometry: { type: "Point" | "LineString" | "Polygon" | "MultiPolygon"; coordinates: unknown };
  properties: Record<string, string | number | boolean | null>;
};
type GeoCollection = { type: "FeatureCollection"; features: GeoFeature[] };
type LabelMarker = {
  marker: maplibregl.Marker;
  element: HTMLDivElement;
};

type VehicleSnapshot = {
  vehicleId: string;
  vehicleOrdinal: number;
  serviceId: string;
  lineId: string;
  lineName: string;
  destinationLabel: string;
  mode: string;
  modeVariant: string | null;
  stockTierId: string | null;
  vehicleCapacity: number;
  passengersOnBoard: number;
  headwayS: number;
  lng: number;
  lat: number;
  displayColor: string;
};

type VehicleRouteSeed = {
  serviceId: string;
  lineId: string;
  lineName: string;
  destinationLabel: string;
  mode: string;
  modeVariant: string | null;
  stockTierId: string | null;
  vehicleCapacity: number;
  headwayS: number;
  cycleS: number;
  speedMps: number;
  dwellS: number;
  routeLengthM: number;
  stopDistancesM: number[];
  vehiclesOnService: number;
  coords: [number, number][];
  displayColor: string;
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

const WORLD_HALF_M = 20037508.342789244;
const MODE_COLOR_BY_CLASS: Record<string, string> = {
  metro: "#0f5ca8",
  tram: "#e65a2b",
  bus: "#146c58",
  ferry: "#2969b2",
  commuter_rail: "#6c3bcf",
  high_speed_rail: "#b11f3a",
  rail: "#3a5f8f",
  unknown: "#5f7796",
};

function fc(features: GeoFeature[] = []): GeoCollection {
  return { type: "FeatureCollection", features };
}


function parseLegacyH3Region(regionId: string | null | undefined): string | null {
  if (!regionId) return null;
  const match = /^r6:[A-Za-z]{2}:([0-9a-f]+)$/i.exec(regionId.trim());
  return match ? match[1].toLowerCase() : null;
}

function parseCountyGeometry(region: RegionStatus | null): GeoJsonGeometry | null {
  return region?.geometry ?? null;
}

function inverseWebMercator(x: number, y: number): XY | null {
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  const lng = (x / WORLD_HALF_M) * 180.0;
  const ydeg = (y / WORLD_HALF_M) * 180.0;
  const lat = (180.0 / Math.PI) * (2.0 * Math.atan(Math.exp((ydeg * Math.PI) / 180.0)) - Math.PI / 2.0);
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  return Math.abs(lng) <= 180 && Math.abs(lat) <= 90 ? { lng, lat } : null;
}

function forwardWebMercator(lng: number, lat: number): { x: number; y: number } | null {
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  if (Math.abs(lng) > 180 || Math.abs(lat) > 90) return null;
  const x = (lng * WORLD_HALF_M) / 180.0;
  const clampedLat = Math.max(Math.min(lat, 85.05112878), -85.05112878);
  const y =
    Math.log(Math.tan(((90.0 + clampedLat) * Math.PI) / 360.0)) * (WORLD_HALF_M / Math.PI);
  return Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null;
}

function xyToLngLat(x: number, y: number, crsType?: string): XY | null {
  const hint = crsType?.toLowerCase() ?? "";
  const alreadyLngLat = Math.abs(x) <= 180 && Math.abs(y) <= 90;
  if (hint.includes("4326") || hint.includes("wgs84")) return alreadyLngLat ? { lng: x, lat: y } : null;
  if (hint.includes("3857") || hint.includes("mercator")) return inverseWebMercator(x, y);
  return alreadyLngLat ? { lng: x, lat: y } : inverseWebMercator(x, y);
}

function lngLatToXY(lng: number, lat: number, crsType?: string): { x: number; y: number } | null {
  const hint = crsType?.toLowerCase() ?? "";
  if (hint.includes("4326") || hint.includes("wgs84")) {
    return { x: lng, y: lat };
  }
  if (hint.includes("3857") || hint.includes("mercator")) {
    return forwardWebMercator(lng, lat);
  }
  return forwardWebMercator(lng, lat) ?? { x: lng, y: lat };
}

function safeRes6Token(lat: number, lng: number): string | null {
  try {
    return latLngToCell(lat, lng, 6).toLowerCase();
  } catch {
    return null;
  }
}

function ringBounds(ring: number[][]): [number, number, number, number] {
  let minLng = Infinity;
  let minLat = Infinity;
  let maxLng = -Infinity;
  let maxLat = -Infinity;
  for (const [lng, lat] of ring) {
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) continue;
    minLng = Math.min(minLng, lng);
    minLat = Math.min(minLat, lat);
    maxLng = Math.max(maxLng, lng);
    maxLat = Math.max(maxLat, lat);
  }
  return [minLng, minLat, maxLng, maxLat];
}

function boundsFromGeometry(geometry: GeoJsonGeometry | null): LngLatBoundsLike | null {
  if (!geometry) return null;
  if (geometry.type === "Polygon" && geometry.coordinates.length > 0) {
    const [minLng, minLat, maxLng, maxLat] = ringBounds(geometry.coordinates[0]);
    return [
      [minLng, minLat],
      [maxLng, maxLat],
    ];
  }
  if (geometry.type === "MultiPolygon" && geometry.coordinates.length > 0) {
    let minLng = Infinity;
    let minLat = Infinity;
    let maxLng = -Infinity;
    let maxLat = -Infinity;
    for (const polygon of geometry.coordinates) {
      if (polygon.length === 0) continue;
      const [a, b, c, d] = ringBounds(polygon[0]);
      minLng = Math.min(minLng, a);
      minLat = Math.min(minLat, b);
      maxLng = Math.max(maxLng, c);
      maxLat = Math.max(maxLat, d);
    }
    return [
      [minLng, minLat],
      [maxLng, maxLat],
    ];
  }
  return null;
}

function labelPointFromGeometry(geometry: GeoJsonGeometry | null): [number, number] | null {
  const bounds = boundsFromGeometry(geometry);
  if (!bounds) return null;
  const [[minLng, minLat], [maxLng, maxLat]] = bounds as [[number, number], [number, number]];
  return [(minLng + maxLng) * 0.5, (minLat + maxLat) * 0.5];
}

function normalizedLon360(lng: number): number {
  if (!Number.isFinite(lng)) return 0;
  const wrapped = lng % 360;
  return wrapped < 0 ? wrapped + 360 : wrapped;
}

function ringCenterAndScore(ring: number[][]): { point: [number, number]; score: number } | null {
  if (!Array.isArray(ring) || ring.length < 3) return null;
  const lons: number[] = [];
  const lats: number[] = [];
  for (const point of ring) {
    const lng = Number(point?.[0]);
    const lat = Number(point?.[1]);
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) continue;
    lons.push(lng);
    lats.push(lat);
  }
  if (lons.length < 3 || lats.length < 3) return null;
  const minLat = Math.min(...lats);
  const maxLat = Math.max(...lats);
  if (!Number.isFinite(minLat) || !Number.isFinite(maxLat)) return null;
  const latHeight = Math.max(0, maxLat - minLat);

  const rawMinLon = Math.min(...lons);
  const rawMaxLon = Math.max(...lons);
  const rawSpan = Math.max(0, rawMaxLon - rawMinLon);

  let centerLon: number;
  let lonWidth: number;
  if (rawSpan <= 180) {
    centerLon = (rawMinLon + rawMaxLon) * 0.5;
    lonWidth = rawSpan;
  } else {
    const lons360 = lons.map(normalizedLon360);
    const min360 = Math.min(...lons360);
    const max360 = Math.max(...lons360);
    lonWidth = Math.max(0, max360 - min360);
    let center360 = (min360 + max360) * 0.5;
    if (center360 > 180) center360 -= 360;
    centerLon = center360;
  }
  const centerLat = (minLat + maxLat) * 0.5;
  const score = lonWidth * latHeight * Math.max(Math.cos((centerLat * Math.PI) / 180), 0.2);
  return {
    point: [centerLon, centerLat],
    score: Number.isFinite(score) ? score : 0,
  };
}

function polygonLabelPointAndScore(polygon: number[][][]): { point: [number, number]; score: number } | null {
  if (!Array.isArray(polygon) || polygon.length === 0) return null;
  return ringCenterAndScore(polygon[0]);
}

function countryLabelPointAndArea(
  geometry: GeoJsonGeometry | null
): { point: [number, number]; area: number } | null {
  if (!geometry) return null;
  if (geometry.type === "Polygon") {
    const placement = polygonLabelPointAndScore(geometry.coordinates);
    if (placement) return { point: placement.point, area: placement.score };
    const fallback = labelPointFromGeometry(geometry);
    return fallback ? { point: fallback, area: 0 } : null;
  }
  if (geometry.type === "MultiPolygon") {
    let bestArea = 0;
    let bestPoint: [number, number] | null = null;
    for (const polygon of geometry.coordinates) {
      const placement = polygonLabelPointAndScore(polygon);
      if (!placement) continue;
      const area = placement.score;
      if (area <= bestArea) continue;
      bestArea = area;
      bestPoint = placement.point;
    }
    if (bestPoint) return { point: bestPoint, area: bestArea };
    const fallback = labelPointFromGeometry(geometry);
    return fallback ? { point: fallback, area: 0 } : null;
  }
  return null;
}

function flattenBounds(bounds: LngLatBoundsLike | null): [number, number, number, number] | null {
  if (!bounds) return null;
  const [[minLng, minLat], [maxLng, maxLat]] = bounds as [[number, number], [number, number]];
  if (![minLng, minLat, maxLng, maxLat].every((value) => Number.isFinite(value))) return null;
  return [minLng, minLat, maxLng, maxLat];
}

function boundsIntersect(a: [number, number, number, number], b: [number, number, number, number]): boolean {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

function padBounds(bounds: [number, number, number, number], pad: number): [number, number, number, number] {
  return [bounds[0] - pad, bounds[1] - pad, bounds[2] + pad, bounds[3] + pad];
}

function countyIdFromRegionId(regionId: string): string | null {
  const parts = regionId.trim().split(":");
  if (parts.length >= 3 && parts[0] === "county") return parts[2];
  return null;
}

function makeUrlFromTemplate(template: string | null | undefined, countyId: string): string | null {
  if (!template) return null;
  return template.replace("{county_id}", encodeURIComponent(countyId));
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function normalizeBasemapFeatureCollection(input: unknown): GeoCollection {
  const features = asArray((input as { features?: unknown[] } | null)?.features)
    .map((feature): GeoFeature | null => {
      if (!feature || typeof feature !== "object") return null;
      const geometry = (feature as { geometry?: GeoJsonAnyGeometry | null }).geometry;
      if (!geometry || typeof geometry !== "object") return null;
      const properties = {
        ...(((feature as { properties?: Record<string, unknown> | null }).properties ?? {}) as Record<string, unknown>),
      };
      if (!properties.feature_layer) {
        if (typeof properties.road_class === "string") properties.feature_layer = "road";
        else if (typeof properties.rail_class === "string") properties.feature_layer = "rail";
      }
      return {
        type: "Feature",
        geometry: geometry as GeoFeature["geometry"],
        properties: properties as GeoFeature["properties"],
      };
    })
    .filter((feature): feature is GeoFeature => Boolean(feature));
  return fc(features);
}

async function fetchFeatureCollection(url: string): Promise<GeoCollection> {
  const response = await fetch(url, { cache: "force-cache" });
  if (!response.ok) {
    throw new Error(`failed to load ${url}: ${response.status}`);
  }
  const payload = (await response.json()) as unknown;
  return normalizeBasemapFeatureCollection(payload);
}

function mergeFeatureCollections(collections: GeoCollection[]): GeoCollection {
  const features = collections.flatMap((collection) => collection.features);
  return fc(features);
}

function modeClass(mode: string | null | undefined, modeVariant: string | null | undefined): string {
  return canonicalModeClassToken(mode, modeVariant);
}

function modeColor(mode: string | null | undefined, modeVariant: string | null | undefined): string {
  const key = modeClass(mode, modeVariant);
  return MODE_COLOR_BY_CLASS[key] ?? MODE_COLOR_BY_CLASS.unknown;
}

function formatDistanceKm(distanceM: number): string {
  if (!Number.isFinite(distanceM) || distanceM < 0) return "0.00 km";
  const km = distanceM / 1000;
  if (km >= 10) return `${km.toFixed(1)} km`;
  return `${km.toFixed(2)} km`;
}

function stopTypeColor(stopType: string | null | undefined): string {
  const modeClassValue = modeClassFromStopTypeToken(stopType);
  return MODE_COLOR_BY_CLASS[modeClassValue] ?? MODE_COLOR_BY_CLASS.unknown;
}

function modeClassFromStopType(stopType: string | null | undefined): string {
  return modeClassFromStopTypeToken(stopType);
}

function modeBadgeForStop(modeClassValue: string): string {
  switch (modeClassValue) {
    case "bus":
      return "B";
    case "tram":
      return "T";
    case "metro":
      return "M";
    case "ferry":
      return "F";
    case "commuter_rail":
      return "CR";
    case "high_speed_rail":
      return "HS";
    case "rail":
      return "R";
    default:
      return "S";
  }
}

function modeSymbolForStop(modeClassValue: string): string {
  switch (modeClassValue) {
    case "bus":
      return "■";
    case "tram":
      return "◆";
    case "metro":
      return "●";
    case "ferry":
      return "▲";
    case "commuter_rail":
      return "⬢";
    case "high_speed_rail":
      return "⬣";
    case "rail":
      return "⬟";
    default:
      return "●";
  }
}

function basemapTierForZoom(zoom: number): "none" | "mid" | "full" {
  if (zoom >= 10.0) return "full";
  return "none";
}

function pointInRing(point: XY, ring: number[][]): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    const crosses =
      yi > point.lat !== yj > point.lat &&
      point.lng < ((xj - xi) * (point.lat - yi)) / ((yj - yi) || 1e-12) + xi;
    if (crosses) inside = !inside;
  }
  return inside;
}

function pointInGeometry(point: XY, geometry: GeoJsonGeometry | null): boolean {
  if (!geometry) return false;
  if (geometry.type === "Polygon") {
    if (geometry.coordinates.length === 0) return false;
    if (!pointInRing(point, geometry.coordinates[0])) return false;
    for (let i = 1; i < geometry.coordinates.length; i += 1) {
      if (pointInRing(point, geometry.coordinates[i])) return false;
    }
    return true;
  }
  for (const polygon of geometry.coordinates) {
    if (polygon.length === 0) continue;
    if (!pointInRing(point, polygon[0])) continue;
    let inHole = false;
    for (let i = 1; i < polygon.length; i += 1) {
      if (pointInRing(point, polygon[i])) {
        inHole = true;
        break;
      }
    }
    if (!inHole) return true;
  }
  return false;
}

function modeMatches(mode: string, filter: LinkModeFilter): boolean {
  return filter === "all" || mode.toLowerCase() === filter;
}

function serviceLineId(service: ServiceLite): string {
  const lineId = service.line_id?.trim();
  return lineId && lineId.length > 0 ? lineId : service.id;
}

function serviceLineName(service: ServiceLite): string {
  const name = service.name?.trim();
  return name && name.length > 0 ? name : serviceLineId(service);
}

function approxDistanceM(a: XY, b: XY): number {
  const avgLatRad = ((a.lat + b.lat) * 0.5 * Math.PI) / 180;
  const dx = (b.lng - a.lng) * 111_320 * Math.cos(avgLatRad);
  const dy = (b.lat - a.lat) * 110_540;
  return Math.sqrt(dx * dx + dy * dy);
}

function pushCoords(target: [number, number][], incoming: [number, number][]): void {
  for (const point of incoming) {
    const prev = target[target.length - 1];
    if (!prev || prev[0] !== point[0] || prev[1] !== point[1]) {
      target.push(point);
    }
  }
}

function pointAlongPolyline(coords: [number, number][], distanceM: number): [number, number] | null {
  if (coords.length < 2) return coords[0] ?? null;
  let remaining = Math.max(0, distanceM);
  for (let index = 1; index < coords.length; index += 1) {
    const a = { lng: coords[index - 1][0], lat: coords[index - 1][1] };
    const b = { lng: coords[index][0], lat: coords[index][1] };
    const segLen = approxDistanceM(a, b);
    if (segLen <= 0) continue;
    if (remaining <= segLen) {
      const t = remaining / segLen;
      return [a.lng + (b.lng - a.lng) * t, a.lat + (b.lat - a.lat) * t];
    }
    remaining -= segLen;
  }
  return coords[coords.length - 1];
}

type VehiclePhaseSample = {
  distanceM: number;
  stopIndex: number;
};

function sampleVehiclePhase(seed: VehicleRouteSeed, phaseS: number): VehiclePhaseSample {
  if (seed.stopDistancesM.length < 2 || !(seed.routeLengthM > 0)) {
    return { distanceM: 0, stopIndex: 0 };
  }
  let remaining = ((phaseS % seed.cycleS) + seed.cycleS) % seed.cycleS;
  const stops = seed.stopDistancesM;
  const lastStopIndex = Math.max(stops.length - 1, 0);
  for (let index = 0; index < stops.length - 1; index += 1) {
    const stopDistance = stops[index];
    if (remaining <= seed.dwellS) {
      return { distanceM: stopDistance, stopIndex: index };
    }
    remaining -= seed.dwellS;
    const nextDistance = stops[index + 1];
    const segmentDistance = Math.max(nextDistance - stopDistance, 0);
    if (!(segmentDistance > 0)) continue;
    const travelS = segmentDistance / Math.max(seed.speedMps, 1);
    if (!(travelS > 0)) continue;
    if (remaining <= travelS) {
      const t = remaining / travelS;
      return {
        distanceM: stopDistance + segmentDistance * t,
        stopIndex: index,
      };
    }
    remaining -= travelS;
  }
  const terminal = stops[stops.length - 1];
  if (remaining <= seed.dwellS) {
    return { distanceM: terminal, stopIndex: lastStopIndex };
  }
  return {
    distanceM: seed.routeLengthM,
    stopIndex: Math.max(lastStopIndex - 1, 0),
  };
}

function stopOccupancyRatio(stopIndex: number, stopCount: number, baseLoadRatio: number): number {
  if (stopCount <= 1) return 0;
  const terminalIndex = stopCount - 1;
  const clampedIndex = Math.min(Math.max(Math.floor(stopIndex), 0), terminalIndex);
  if (clampedIndex >= terminalIndex) return 0;
  // Keep occupancy constant on a segment and update only on stop transitions.
  return Math.max(baseLoadRatio, 0);
}

function vehicleTypeLabel(mode: string): string {
  const modeClassValue = canonicalModeClassToken(mode, null);
  switch (modeClassValue) {
    case "metro":
      return "Metro Train";
    case "tram":
      return "Tram";
    case "bus":
      return "Bus";
    case "ferry":
      return "Ferry";
    case "commuter_rail":
    case "high_speed_rail":
    case "rail":
      return "Rail Train";
    default:
      return "Vehicle";
  }
}

function resolveDirectionBadge(
  directionName: string | null | undefined,
  direction: string | null | undefined
): string {
  const directionNameToken = directionName?.trim().toLowerCase() ?? "";
  if (directionNameToken.includes("outbound")) return "Outbound";
  if (directionNameToken.includes("inbound")) return "Inbound";
  if (directionNameToken.includes("clockwise")) return "Outbound";
  if (directionNameToken.includes("counterclockwise")) return "Inbound";
  const directionToken = direction?.trim().toLowerCase() ?? "";
  if (directionToken.includes("forward") || directionToken.includes("outbound")) return "Outbound";
  if (directionToken.includes("reverse") || directionToken.includes("inbound")) return "Inbound";
  return "Outbound";
}

function setData(map: MapLibreMap, sourceId: string, data: GeoCollection | GeoJsonFeatureCollection): void {
  (map.getSource(sourceId) as GeoJSONSource | undefined)?.setData(data as never);
}

function setVisibility(map: MapLibreMap, layerId: string, visible: boolean): void {
  if (!map.getLayer(layerId)) return;
  map.setLayoutProperty(layerId, "visibility", visible ? "visible" : "none");
}

function normalizeMinute(value: number | null | undefined, fallback: number): number {
  const minute = typeof value === "number" && Number.isFinite(value) ? Math.round(value) : fallback;
  return ((minute % 1440) + 1440) % 1440;
}

function minuteOfDayFromClock(clock: SimulationClock | null | undefined): number | null {
  const raw = clock?.sim_datetime_utc;
  if (!raw) return null;
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed.getUTCHours() * 60 + parsed.getUTCMinutes();
}

function inTimeWindow(minuteOfDay: number, startMinute: number, endMinute: number): boolean {
  if (startMinute === endMinute) return false;
  if (startMinute < endMinute) return minuteOfDay >= startMinute && minuteOfDay < endMinute;
  return minuteOfDay >= startMinute || minuteOfDay < endMinute;
}

function activeScheduledTph(service: ServiceLite, minuteOfDay: number | null): number | null {
  const profile = service.schedule_profile;
  if (!profile || minuteOfDay === null) return null;
  const peakStart = normalizeMinute(profile.peak_start_minute, 420);
  const peakEnd = normalizeMinute(profile.peak_end_minute, 570);
  const overnightStart = normalizeMinute(profile.overnight_start_minute, 0);
  const overnightEnd = normalizeMinute(profile.overnight_end_minute, 300);
  if (inTimeWindow(minuteOfDay, peakStart, peakEnd)) {
    return Math.max(profile.tph_peak ?? 0, 0);
  }
  if (inTimeWindow(minuteOfDay, overnightStart, overnightEnd)) {
    return Math.max(profile.tph_overnight ?? 0, 0);
  }
  return Math.max(profile.tph_off_peak ?? 0, 0);
}

function inferServiceRunningTph(service: ServiceLite, minuteOfDay: number | null): number {
  const scheduledTph = activeScheduledTph(service, minuteOfDay);
  let fallbackTph = 0;
  if (typeof service.operating_tph === "number" && Number.isFinite(service.operating_tph)) {
    fallbackTph = Math.max(service.operating_tph, 0);
  } else if (
    typeof service.headway_s === "number" &&
    Number.isFinite(service.headway_s) &&
    service.headway_s > 0 &&
    service.headway_s < 86_399
  ) {
    fallbackTph = 3600 / service.headway_s;
  }
  if (scheduledTph !== null) return Math.max(scheduledTph, fallbackTph);
  return fallbackTph;
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
  const runtimeVehicleFeatureByIdRef = useRef<Map<string, GeoFeature>>(new Map());
  const runtimeVehicleGeoJsonRef = useRef<GeoCollection>(fc());
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

  const worldCountryData = useMemo<GeoCollection>(() => {
    const unlockedSet = new Set(
      (props.visibleCountryIso2 ?? [])
        .map((code) => code.trim().toUpperCase())
        .filter((code) => code.length === 2)
    );
    const features = worldContextData.features.map((feature) => {
      const iso = String(feature.properties?.country_iso2 ?? "")
        .trim()
        .toUpperCase();
      const unlockedNow = unlockedSet.has(iso) ? 1 : 0;
      return {
        ...feature,
        properties: {
          ...feature.properties,
          unlocked_now: unlockedNow,
          playable_now: unlockedNow,
          coming_soon: unlockedNow ? 0 : 1,
        },
      };
    });
    return fc(features);
  }, [props.visibleCountryIso2, worldContextData]);

  const worldCountryLabelData = useMemo<GeoCollection>(() => {
    type CountryLabelAgg = {
      name: string;
      point: [number, number];
      score: number;
      unlockedNow: number;
    };
    const grouped = new Map<string, CountryLabelAgg>();
    for (const feature of worldCountryData.features) {
      const iso = String(feature.properties?.country_iso2 ?? "")
        .trim()
        .toUpperCase();
      if (iso.length !== 2) continue;
      const placement = countryLabelPointAndArea(feature.geometry as GeoJsonGeometry);
      if (!placement) continue;
      const nameValue = feature.properties?.name;
      const name = typeof nameValue === "string" ? nameValue.trim() : "";
      const unlockedNow = Number(feature.properties?.unlocked_now ?? 0) === 1 ? 1 : 0;
      const existing = grouped.get(iso);
      if (!existing) {
        grouped.set(iso, {
          name,
          point: placement.point,
          score: placement.area,
          unlockedNow,
        });
        continue;
      }
      if (placement.area > existing.score) {
        existing.point = placement.point;
        existing.score = placement.area;
        if (name) existing.name = name;
      }
      if (!existing.name && name) existing.name = name;
      existing.unlockedNow = Math.max(existing.unlockedNow, unlockedNow);
    }
    const features: GeoFeature[] = [];
    for (const [iso, agg] of grouped) {
      features.push({
        type: "Feature",
        geometry: {
          type: "Point",
          coordinates: [agg.point[0], agg.point[1]],
        },
        properties: {
          country_iso2: iso,
          name: agg.name || iso,
          unlocked_now: agg.unlockedNow,
        },
      });
    }
    return fc(features);
  }, [worldCountryData]);

  const stopPointById = useMemo(() => {
    const out = new Map<string, MapWorldPoint & { name: string }>();
    const crs = props.scenario?.meta?.crs?.type;
    for (const stop of props.scenario?.world.stops ?? []) {
      const ll = xyToLngLat(stop.x, stop.y, crs);
      if (!ll) continue;
      out.set(stop.id, {
        lng: ll.lng,
        lat: ll.lat,
        x: stop.x,
        y: stop.y,
        name: stop.name?.trim() ? stop.name : stop.id,
      });
    }
    return out;
  }, [props.scenario?.meta?.crs?.type, props.scenario?.world.stops]);

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

  const countyFeatures = useMemo<GeoCollection>(() => {
    const features: GeoFeature[] = [];
    for (const region of props.regions) {
      const geometry = resolveRegionGeometry(region);
      if (geometry) {
        features.push({
          type: "Feature",
          geometry,
          properties: {
            region_id: region.region_id,
            name: region.name,
            unlocked: region.unlocked ? 1 : 0,
            focus: region.region_id === props.focusRegionId ? 1 : 0,
            selected: region.region_id === props.selectedRegionId ? 1 : 0,
          },
        });
        continue;
      }
      const token = parseLegacyH3Region(region.region_id);
      if (!token || !isValidCell(token)) continue;
      const boundary = cellToBoundary(token, true) as [number, number][];
      const lngLatBoundary = boundary.map(([lat, lng]) => [lng, lat] as [number, number]);
      const first = lngLatBoundary[0];
      const ring =
        lngLatBoundary[lngLatBoundary.length - 1][0] === first[0] &&
        lngLatBoundary[lngLatBoundary.length - 1][1] === first[1]
          ? lngLatBoundary
          : [...lngLatBoundary, [first[0], first[1]]];
      features.push({
        type: "Feature",
        geometry: { type: "Polygon", coordinates: [ring] },
        properties: {
          region_id: region.region_id,
          name: region.name,
          unlocked: region.unlocked ? 1 : 0,
          focus: region.region_id === props.focusRegionId ? 1 : 0,
          selected: region.region_id === props.selectedRegionId ? 1 : 0,
        },
      });
    }
    return fc(features);
  }, [props.regions, props.focusRegionId, props.selectedRegionId, resolveRegionGeometry]);

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

  const countyLabelData = useMemo(() => {
    return props.regions
      .map((region) => {
        const geometry = resolveRegionGeometry(region);
        let point = labelPointFromGeometry(geometry);
        if (!point) {
          const token = parseLegacyH3Region(region.region_id);
          if (token && isValidCell(token)) {
            const boundary = cellToBoundary(token, true) as [number, number][];
            let minLng = Infinity;
            let minLat = Infinity;
            let maxLng = -Infinity;
            let maxLat = -Infinity;
            for (const [lat, lng] of boundary) {
              minLng = Math.min(minLng, lng);
              minLat = Math.min(minLat, lat);
              maxLng = Math.max(maxLng, lng);
              maxLat = Math.max(maxLat, lat);
            }
            if (Number.isFinite(minLng) && Number.isFinite(minLat) && Number.isFinite(maxLng) && Number.isFinite(maxLat)) {
              point = [(minLng + maxLng) * 0.5, (minLat + maxLat) * 0.5];
            }
          }
        }
        return point
          ? {
              regionId: region.region_id,
              name: region.name,
              point,
              focus: region.region_id === props.focusRegionId,
            }
          : null;
      })
      .filter((value): value is { regionId: string; name: string; point: [number, number]; focus: boolean } => Boolean(value));
  }, [props.regions, props.focusRegionId, resolveRegionGeometry]);

  const countyBoundsData = useMemo(() => {
    return props.regions
      .map((region) => {
        const geometry = resolveRegionGeometry(region);
        const bounds = flattenBounds(boundsFromGeometry(geometry));
        const countyId = countyIdFromRegionId(region.region_id);
        if (!bounds || !countyId) return null;
        return {
          regionId: region.region_id,
          countyId,
          bounds,
        };
      })
      .filter((value): value is { regionId: string; countyId: string; bounds: [number, number, number, number] } => Boolean(value));
  }, [props.regions, resolveRegionGeometry]);

  const networkData = useMemo(() => {
    if (!props.scenario) return { links: fc(), transfers: fc(), stops: fc(), zones: fc() };
    const allowedCountries = props.visibleCountryIso2
      ? new Set(props.visibleCountryIso2.map((code) => code.trim().toUpperCase()))
      : null;
    const crs = props.scenario.meta?.crs?.type;
    const stopCoords = new Map<string, XY>();
    const stopInFocus = new Map<string, boolean>();
    const stopDegree = new Map<string, number>();
    const stopColors = new Map<string, string>();
    const stopModeClass = new Map<string, string>();
    const lineColors = new Map<string, string>();
    const lineModes = new Map<string, { mode: string; modeVariant: string | null }>();
    const lineByPair = new Map<string, string>();

    for (const link of props.scenario.world.links) {
      stopDegree.set(link.from_stop, (stopDegree.get(link.from_stop) ?? 0) + 1);
      stopDegree.set(link.to_stop, (stopDegree.get(link.to_stop) ?? 0) + 1);
    }

    for (const service of props.scenario.world.services) {
      const lineId = serviceLineId(service);
      if (!lineModes.has(lineId)) {
        lineModes.set(lineId, { mode: service.mode, modeVariant: service.mode_variant ?? null });
      }
      if (service.display_color && !lineColors.has(lineId)) {
        lineColors.set(lineId, service.display_color);
      }
      for (const stopId of service.stop_sequence) {
        if (service.display_color && !stopColors.has(stopId)) {
          stopColors.set(stopId, service.display_color);
        }
        if (!stopModeClass.has(stopId)) {
          stopModeClass.set(stopId, modeClass(service.mode, service.mode_variant));
        }
      }
      for (let index = 1; index < service.stop_sequence.length; index += 1) {
        const fromStop = service.stop_sequence[index - 1];
        const toStop = service.stop_sequence[index];
        lineByPair.set(`${fromStop}->${toStop}::${service.mode}`, lineId);
      }
    }

    for (const stop of props.scenario.world.stops) {
      const coord = xyToLngLat(stop.x, stop.y, crs);
      if (!coord) continue;
      const iso = stop.country_iso2?.trim().toUpperCase() ?? null;
      if (allowedCountries && iso && !allowedCountries.has(iso)) continue;
      stopCoords.set(stop.id, coord);
      const inFocusByGeometry = pointInGeometry(coord, resolveRegionGeometry(focusRegion));
      const inFocusByLegacyH3 =
        !inFocusByGeometry &&
        Boolean(focusRegion && parseLegacyH3Region(focusRegion.region_id) === safeRes6Token(coord.lat, coord.lng));
      stopInFocus.set(stop.id, inFocusByGeometry || inFocusByLegacyH3);
    }

    const linkFeatures: GeoFeature[] = [];
    for (const link of props.scenario.world.links) {
      if (!modeMatches(link.mode, props.linkMode)) continue;
      const from = stopCoords.get(link.from_stop);
      const to = stopCoords.get(link.to_stop);
      if (!from || !to) continue;

      let coordinates: [number, number][] = [
        [from.lng, from.lat],
        [to.lng, to.lat],
      ];
      if (Array.isArray(link.geometry) && link.geometry.length >= 2) {
        const line: [number, number][] = [];
        for (const [x, y] of link.geometry) {
          const coord = xyToLngLat(x, y, crs);
          if (coord) line.push([coord.lng, coord.lat]);
        }
        if (line.length >= 2) coordinates = line;
      }
      const fromFocus = stopInFocus.get(link.from_stop) ?? false;
      const toFocus = stopInFocus.get(link.to_stop) ?? false;
      const lineId =
        link.line_id?.trim() ||
        lineByPair.get(`${link.from_stop}->${link.to_stop}::${link.mode}`) ||
        link.id;
      const lineMode = lineModes.get(lineId);
      const displayColor =
        lineColors.get(lineId) ??
        modeColor(lineMode?.mode ?? link.mode, lineMode?.modeVariant ?? link.mode_variant ?? null);
      linkFeatures.push({
        type: "Feature",
        geometry: { type: "LineString", coordinates },
          properties: {
            id: link.id,
            line_id: lineId,
            display_color: displayColor,
            mode_class: modeClass(lineMode?.mode ?? link.mode, lineMode?.modeVariant ?? link.mode_variant ?? null),
            in_focus: fromFocus || toFocus ? 1 : 0,
            focus_connector: fromFocus !== toFocus ? 1 : 0,
            major_mode:
              isMajorMode(
                lineMode?.mode ?? link.mode,
                lineMode?.modeVariant ?? link.mode_variant ?? null
              ) || link.speed_mps >= 42
                ? 1
                : 0,
          },
        });
    }

    const transferFeatures: GeoFeature[] = [];
    const seenTransferPairs = new Set<string>();
    for (const transfer of props.scenario.world.transfers) {
      const from = stopCoords.get(transfer.from_stop);
      const to = stopCoords.get(transfer.to_stop);
      if (!from || !to) continue;
      const fromFocus = stopInFocus.get(transfer.from_stop) ?? false;
      const toFocus = stopInFocus.get(transfer.to_stop) ?? false;
      const pairKey = [transfer.from_stop, transfer.to_stop].sort().join("::");
      if (seenTransferPairs.has(pairKey)) continue;
      seenTransferPairs.add(pairKey);
      const transferTimeS = Number.isFinite(transfer.time_s) ? Math.max(transfer.time_s, 0) : 0;
      const penaltyS = Number.isFinite(transfer.penalty_s ?? 0) ? Math.max(transfer.penalty_s ?? 0, 0) : 0;
      const transferMinutes = Math.max(Math.round(transferTimeS / 60), 1);
      transferFeatures.push({
        type: "Feature",
        geometry: {
          type: "LineString",
          coordinates: [
            [from.lng, from.lat],
            [to.lng, to.lat],
          ],
        },
        properties: {
          id: `transfer:${pairKey}`,
          in_focus: fromFocus || toFocus ? 1 : 0,
          transfer_s: transferTimeS,
          penalty_s: penaltyS,
          transfer_label: `${transferMinutes}m walk`,
        },
      });
    }

    const stopFeatures: GeoFeature[] = [];
    for (const stop of props.scenario.world.stops) {
      const coord = stopCoords.get(stop.id);
      if (!coord) continue;
      const resolvedModeClass = stopModeClass.get(stop.id) ?? modeClassFromStopType(stop.stop_type);
      const resolvedDisplayColor =
        stopColors.get(stop.id) ??
        MODE_COLOR_BY_CLASS[resolvedModeClass] ??
        stopTypeColor(stop.stop_type);
      stopFeatures.push({
        type: "Feature",
        geometry: { type: "Point", coordinates: [coord.lng, coord.lat] },
          properties: {
            id: stop.id,
            display_color: resolvedDisplayColor,
            mode_class: resolvedModeClass,
            stop_badge: modeBadgeForStop(resolvedModeClass),
            stop_symbol: modeSymbolForStop(resolvedModeClass),
            in_focus: stopInFocus.get(stop.id) ? 1 : 0,
            major_interchange: stop.interchange_id || (stopDegree.get(stop.id) ?? 0) >= 5 ? 1 : 0,
            shape_stop: (stop.stop_type ?? "").toLowerCase().includes("shape") ? 1 : 0,
          },
        });
    }

    const zoneFeatures: GeoFeature[] = [];
    for (const zone of props.scenario.world.zones) {
      const coord = xyToLngLat(zone.x, zone.y, crs);
      if (!coord) continue;
      zoneFeatures.push({
        type: "Feature",
        geometry: { type: "Point", coordinates: [coord.lng, coord.lat] },
        properties: { id: zone.id },
      });
    }

    return {
      links: fc(linkFeatures),
      transfers: fc(transferFeatures),
      stops: fc(stopFeatures),
      zones: fc(zoneFeatures),
    };
  }, [
    props.linkMode,
    props.scenario,
    props.visibleCountryIso2,
    focusRegion,
    resolveRegionGeometry,
  ]);

  const vehicleRouteSeeds = useMemo(() => {
    if (gameMode || props.trainsAuthoritative) return [] as VehicleRouteSeed[];
    if (!props.scenario) return [] as VehicleRouteSeed[];
    const crs = props.scenario.meta?.crs?.type;
    const linkByPair = new Map<string, { geometry: [number, number][] | null | undefined; speedMps: number }>();
    const linkByPairNoMode = new Map<string, { geometry: [number, number][] | null | undefined; speedMps: number }>();
    for (const link of props.scenario.world.links) {
      const keyWithMode = `${link.from_stop}->${link.to_stop}::${link.mode.toLowerCase()}`;
      const keyNoMode = `${link.from_stop}->${link.to_stop}`;
      const payload = { geometry: link.geometry, speedMps: Math.max(link.speed_mps || 0, 0) };
      if (!linkByPair.has(keyWithMode)) linkByPair.set(keyWithMode, payload);
      if (!linkByPairNoMode.has(keyNoMode)) linkByPairNoMode.set(keyNoMode, payload);
    }

    const out: VehicleRouteSeed[] = [];
    for (const service of props.scenario.world.services) {
      if (service.stop_sequence.length < 2) continue;
      const rawAssignedUnits =
        service.stock_units_assigned ??
        service.stock_units_owned ??
        service.rolling_stock_profile?.units_owned ??
        0;
      const assignedUnits = typeof rawAssignedUnits === "number" && Number.isFinite(rawAssignedUnits)
        ? Math.max(Math.round(rawAssignedUnits), 0)
        : 0;
      if (assignedUnits <= 0) continue;
      const runningTph = inferServiceRunningTph(service, clockMinuteOfDay);
      if (!(runningTph > 0)) continue;
      const coords: [number, number][] = [];
      let routeLengthM = 0;
      let speedSum = 0;
      let speedCount = 0;
      const stopDistancesM: number[] = [0];

      for (let index = 1; index < service.stop_sequence.length; index += 1) {
        const fromStopId = service.stop_sequence[index - 1];
        const toStopId = service.stop_sequence[index];
        const fromPoint = stopPointById.get(fromStopId);
        const toPoint = stopPointById.get(toStopId);
        if (!fromPoint || !toPoint) continue;

        const pairWithMode = `${fromStopId}->${toStopId}::${service.mode.toLowerCase()}`;
        const pairNoMode = `${fromStopId}->${toStopId}`;
        const link = linkByPair.get(pairWithMode) ?? linkByPairNoMode.get(pairNoMode);
        if (link?.speedMps && link.speedMps > 0) {
          speedSum += link.speedMps;
          speedCount += 1;
        }

        let segmentCoords: [number, number][] = [
          [fromPoint.lng, fromPoint.lat],
          [toPoint.lng, toPoint.lat],
        ];
        if (Array.isArray(link?.geometry) && link.geometry.length >= 2) {
          const converted: [number, number][] = [];
          for (const [x, y] of link.geometry) {
            const ll = xyToLngLat(x, y, crs);
            if (ll) converted.push([ll.lng, ll.lat]);
          }
          if (converted.length >= 2) {
            segmentCoords = converted;
          }
        }
        const first = segmentCoords[0];
        const last = segmentCoords[segmentCoords.length - 1];
        if (first[0] !== fromPoint.lng || first[1] !== fromPoint.lat) {
          segmentCoords.unshift([fromPoint.lng, fromPoint.lat]);
        }
        if (last[0] !== toPoint.lng || last[1] !== toPoint.lat) {
          segmentCoords.push([toPoint.lng, toPoint.lat]);
        }
        pushCoords(coords, segmentCoords);
        let segmentLengthM = 0;
        for (let pointIndex = 1; pointIndex < segmentCoords.length; pointIndex += 1) {
          segmentLengthM += approxDistanceM(
            { lng: segmentCoords[pointIndex - 1][0], lat: segmentCoords[pointIndex - 1][1] },
            { lng: segmentCoords[pointIndex][0], lat: segmentCoords[pointIndex][1] }
          );
        }
        if (segmentLengthM > 0) {
          routeLengthM += segmentLengthM;
          stopDistancesM.push(routeLengthM);
        }
      }

      if (coords.length < 2 || stopDistancesM.length < 2 || !(routeLengthM > 10)) continue;

      const speedMps = speedCount > 0 ? speedSum / speedCount : Math.max(8, routeLengthM / 180);
      const dwellS = Math.max(service.dwell_s, 12);
      const travelS = routeLengthM / Math.max(speedMps, 2);
      const cycleS = Math.max(travelS + dwellS * Math.max(stopDistancesM.length, 1), 90);
      const headwayS = Math.max(3600 / runningTph, 45);
      const vehiclesFromCadence = Math.min(Math.max(Math.round(cycleS / headwayS), 1), 24);
      const vehiclesOnService = Math.min(vehiclesFromCadence, assignedUnits);
      if (vehiclesOnService <= 0) continue;
      const destinationStopId = service.stop_sequence[service.stop_sequence.length - 1];
      const destinationName = destinationStopId ? stopPointById.get(destinationStopId)?.name ?? destinationStopId : null;
      const directionBadge = resolveDirectionBadge(service.direction_name, service.direction);
      out.push({
        serviceId: service.id,
        lineId: serviceLineId(service),
        lineName: serviceLineName(service),
        destinationLabel: destinationName ? `To ${destinationName}` : directionBadge,
        mode: service.mode,
        modeVariant: service.mode_variant ?? null,
        stockTierId: service.rolling_stock_profile?.package_id ?? service.stock_tier_id ?? null,
        vehicleCapacity: Math.max(service.vehicle_capacity || 0, 0),
        headwayS,
        cycleS,
        speedMps,
        dwellS,
        routeLengthM,
        stopDistancesM,
        vehiclesOnService,
        coords,
        displayColor: service.display_color ?? modeColor(service.mode, service.mode_variant ?? null),
      });
    }
    out.sort((a, b) => a.lineId.localeCompare(b.lineId) || a.serviceId.localeCompare(b.serviceId));
    return out;
  }, [clockMinuteOfDay, gameMode, props.scenario, props.trainsAuthoritative, stopPointById]);

  const serviceById = useMemo(() => {
    const byId = new Map<string, ServiceLite>();
    const scenario = props.scenario;
    if (!scenario) return byId;
    for (const service of scenario.world.services) {
      byId.set(service.id, service);
    }
    return byId;
  }, [props.scenario]);

  const vehicleData = useMemo(() => {
    const byId = new Map<string, VehicleSnapshot>();
    const scenario = props.scenario;
    const crs = scenario?.meta?.crs?.type;

    if (gameMode || props.trainsAuthoritative) {
      const trains = Array.isArray(props.runtimeTrains) ? props.runtimeTrains : [];
      for (const train of trains) {
        const coord = xyToLngLat(train.x, train.y, crs);
        if (!coord) continue;
        const service = serviceById.get(train.service_id);
        const displayColor =
          service?.display_color ?? modeColor(train.mode, train.mode_variant ?? null);
        const headwayS = service?.headway_s ?? 0;
        const snapshot: VehicleSnapshot = {
          vehicleId: train.train_id,
          vehicleOrdinal: Math.max(Math.round(train.vehicle_ordinal || 0), 1),
          serviceId: train.service_id,
          lineId: train.line_id,
          lineName: train.line_name,
          destinationLabel: train.destination_label || train.direction_label || "Outbound",
          mode: train.mode,
          modeVariant: train.mode_variant ?? null,
          stockTierId: train.stock_tier_id ?? null,
          vehicleCapacity: Math.max(train.vehicle_capacity ?? 0, 0),
          passengersOnBoard: Math.max(train.onboard_pax ?? 0, 0),
          headwayS,
          lng: coord.lng,
          lat: coord.lat,
          displayColor,
        };
        byId.set(snapshot.vehicleId, snapshot);
      }
      return { geojson: runtimeVehicleGeoJsonRef.current, byId };
    }

    const features: GeoFeature[] = [];
    const tickSeconds = props.clock?.tick_seconds ?? 0;
    const lineOrdinalByLine = new Map<string, number>();
    for (const seed of vehicleRouteSeeds) {
      const serviceLoadRatio = Math.max(props.serviceLoadByServiceId?.[seed.serviceId] ?? 0, 0);
      const effectiveLoadRatio = Math.min(serviceLoadRatio, 1);
      for (let vehicleIndex = 0; vehicleIndex < seed.vehiclesOnService; vehicleIndex += 1) {
        const phaseS = ((tickSeconds + vehicleIndex * seed.headwayS) % seed.cycleS + seed.cycleS) % seed.cycleS;
        const phase = sampleVehiclePhase(seed, phaseS);
        const point = pointAlongPolyline(seed.coords, phase.distanceM);
        if (!point) continue;
        const stopCount = seed.stopDistancesM.length;
        const occupancyRatio = stopOccupancyRatio(phase.stopIndex, stopCount, effectiveLoadRatio);
        const vehicleOrdinal = (lineOrdinalByLine.get(seed.lineId) ?? 0) + 1;
        lineOrdinalByLine.set(seed.lineId, vehicleOrdinal);
        const vehicleId = `${seed.serviceId}#${vehicleIndex}`;
        const passengersOnBoard = Math.round(
          Math.min(seed.vehicleCapacity, Math.max(seed.vehicleCapacity * occupancyRatio, 0))
        );
        const snapshot: VehicleSnapshot = {
          vehicleId,
          vehicleOrdinal,
          serviceId: seed.serviceId,
          lineId: seed.lineId,
          lineName: seed.lineName,
          destinationLabel: seed.destinationLabel,
          mode: seed.mode,
          modeVariant: seed.modeVariant,
          stockTierId: seed.stockTierId,
          vehicleCapacity: seed.vehicleCapacity,
          passengersOnBoard,
          headwayS: seed.headwayS,
          lng: point[0],
          lat: point[1],
          displayColor: seed.displayColor,
        };
        byId.set(vehicleId, snapshot);
        features.push({
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
      }
    }
    return { geojson: fc(features), byId };
  }, [
    props.clock?.tick_seconds,
    gameMode,
    props.runtimeTrains,
    props.scenario,
    props.serviceLoadByServiceId,
    serviceById,
    props.trainsAuthoritative,
    vehicleRouteSeeds,
  ]);

  useEffect(() => {
    if (!(gameMode || props.trainsAuthoritative)) {
      runtimeVehicleFeatureByIdRef.current.clear();
      runtimeVehicleGeoJsonRef.current = vehicleData.geojson;
      setRuntimeVehicleGeoJsonVersion((prev) => prev + 1);
      return;
    }

    const featureById = runtimeVehicleFeatureByIdRef.current;
    const nextSeen = new Set<string>();
    let changed = false;
    for (const snapshot of vehicleData.byId.values()) {
      nextSeen.add(snapshot.vehicleId);
      const existing = featureById.get(snapshot.vehicleId);
      if (!existing) {
        featureById.set(snapshot.vehicleId, {
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
        changed = true;
        continue;
      }
      const coords = existing.geometry.coordinates as [number, number];
      const prevLng = coords?.[0] ?? 0;
      const prevLat = coords?.[1] ?? 0;
      if (Math.abs(prevLng - snapshot.lng) > 1e-6 || Math.abs(prevLat - snapshot.lat) > 1e-6) {
        existing.geometry.coordinates = [snapshot.lng, snapshot.lat];
        changed = true;
      }
      if ((existing.properties.passengers_on_board as number | undefined) !== snapshot.passengersOnBoard) {
        existing.properties.passengers_on_board = snapshot.passengersOnBoard;
        changed = true;
      }
      if ((existing.properties.display_color as string | undefined) !== snapshot.displayColor) {
        existing.properties.display_color = snapshot.displayColor;
        changed = true;
      }
      if ((existing.properties.line_id as string | undefined) !== snapshot.lineId) {
        existing.properties.line_id = snapshot.lineId;
        changed = true;
      }
      if ((existing.properties.service_id as string | undefined) !== snapshot.serviceId) {
        existing.properties.service_id = snapshot.serviceId;
        changed = true;
      }
    }

    for (const vehicleId of Array.from(featureById.keys())) {
      if (!nextSeen.has(vehicleId)) {
        featureById.delete(vehicleId);
        changed = true;
      }
    }

    if (changed) {
      runtimeVehicleGeoJsonRef.current = fc(Array.from(featureById.values()));
      setRuntimeVehicleGeoJsonVersion((prev) => prev + 1);
    }
  }, [
    gameMode,
    props.trainsAuthoritative,
    vehicleData.byId,
    vehicleData.geojson,
  ]);

  useEffect(() => {
    vehicleByIdRef.current = vehicleData.byId;
    if (selectedVehicleId && !vehicleData.byId.has(selectedVehicleId)) {
      setSelectedVehicleId(null);
    }
  }, [selectedVehicleId, vehicleData]);

  const selectedVehicle = useMemo(
    () => (selectedVehicleId ? vehicleData.byId.get(selectedVehicleId) ?? null : null),
    [selectedVehicleId, vehicleData]
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
    const buildAction = current.buildAction ?? "select";
    const isBuildMode = current.interactionMode === "build";
    const showPreviewPoint =
      isBuildMode &&
      (buildAction === "place_station" || buildAction === "start_line" || buildAction === "add_station_to_line");
    const showPreviewLine =
      isBuildMode &&
      (buildAction === "start_line" || buildAction === "add_station_to_line") &&
      Boolean(current.previewAnchorPoint) &&
      Boolean(hoverPointRef.current);

    const previewFeatures: GeoFeature[] = [];
    const hoverPoint = hoverPointRef.current;
    const crs = current.scenario?.meta?.crs?.type;

    if (showPreviewLine && current.previewAnchorPoint && hoverPoint) {
      const from = xyToLngLat(current.previewAnchorPoint.x, current.previewAnchorPoint.y, crs);
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
          properties: { kind: "line", display_color: current.previewColor ?? "#104894" },
        });
      }
    }

    if (showPreviewPoint && hoverPoint) {
      previewFeatures.push({
        type: "Feature",
        geometry: { type: "Point", coordinates: [hoverPoint.lng, hoverPoint.lat] },
        properties: { kind: "point", display_color: current.previewColor ?? "#104894" },
      });
    }

    setData(map, SRC_BUILD_PREVIEW, fc(previewFeatures));
    setVisibility(map, "build-preview-line", showPreviewLine && previewFeatures.length > 0);
    setVisibility(map, "build-preview-point", showPreviewPoint && previewFeatures.length > 0);
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
        const context = (await invoke("load_country_map_context", {
          projectPath,
        })) as CountryMapContext;
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
    const activeVehicleData =
      gameMode || props.trainsAuthoritative
        ? runtimeVehicleGeoJsonRef.current
        : vehicleData.geojson;
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
