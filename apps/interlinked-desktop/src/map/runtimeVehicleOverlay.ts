import type {
  ScenarioLite,
  ServiceLite,
  SimulationClock,
  TrainRuntimeView,
} from "../types";
import { canonicalModeClass as canonicalModeClassToken } from "../modes";

export type LngLatPoint = { lng: number; lat: number };

export type OverlayGeoFeature = {
  type: "Feature";
  geometry: { type: "Point" | "LineString" | "Polygon" | "MultiPolygon"; coordinates: unknown };
  properties: Record<string, string | number | boolean | null>;
};

export type OverlayGeoCollection = { type: "FeatureCollection"; features: OverlayGeoFeature[] };

export type StopPoint = {
  lng: number;
  lat: number;
  x: number;
  y: number;
  name: string;
};

export type VehicleSnapshot = {
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

export type VehicleRouteSeed = {
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

export type VehicleData = {
  geojson: OverlayGeoCollection;
  byId: Map<string, VehicleSnapshot>;
};

type VehiclePhaseSample = {
  distanceM: number;
  stopIndex: number;
};

type XyToLngLat = (x: number, y: number, crsType?: string) => LngLatPoint | null;
type ModeColor = (
  mode: string | null | undefined,
  modeVariant: string | null | undefined
) => string;

export function overlayCollection(features: OverlayGeoFeature[] = []): OverlayGeoCollection {
  return { type: "FeatureCollection", features };
}

export function serviceLineId(service: ServiceLite): string {
  const lineId = service.line_id?.trim();
  return lineId && lineId.length > 0 ? lineId : service.id;
}

function serviceLineName(service: ServiceLite): string {
  const name = service.name?.trim();
  return name && name.length > 0 ? name : serviceLineId(service);
}

function approxDistanceM(a: LngLatPoint, b: LngLatPoint): number {
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
  return Math.max(baseLoadRatio, 0);
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

function normalizeMinute(value: number | null | undefined, fallback: number): number {
  const minute = typeof value === "number" && Number.isFinite(value) ? Math.round(value) : fallback;
  return ((minute % 1440) + 1440) % 1440;
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

export function minuteOfDayFromClock(clock: SimulationClock | null | undefined): number | null {
  const raw = clock?.sim_datetime_utc;
  if (!raw) return null;
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed.getUTCHours() * 60 + parsed.getUTCMinutes();
}

export function buildServiceById(scenario: ScenarioLite | null | undefined): Map<string, ServiceLite> {
  const byId = new Map<string, ServiceLite>();
  if (!scenario) return byId;
  for (const service of scenario.world.services) {
    byId.set(service.id, service);
  }
  return byId;
}

export function buildVehicleRouteSeeds(args: {
  gameMode: boolean;
  trainsAuthoritative?: boolean;
  scenario: ScenarioLite | null;
  stopPointById: Map<string, StopPoint>;
  clockMinuteOfDay: number | null;
  xyToLngLat: XyToLngLat;
  modeColor: ModeColor;
}): VehicleRouteSeed[] {
  const {
    gameMode,
    trainsAuthoritative,
    scenario,
    stopPointById,
    clockMinuteOfDay,
    xyToLngLat,
    modeColor,
  } = args;

  if (gameMode || trainsAuthoritative) return [];
  if (!scenario) return [];

  const crs = scenario.meta?.crs?.type;
  const linkByPair = new Map<string, { geometry: [number, number][] | null | undefined; speedMps: number }>();
  const linkByPairNoMode = new Map<string, { geometry: [number, number][] | null | undefined; speedMps: number }>();
  for (const link of scenario.world.links) {
    const keyWithMode = `${link.from_stop}->${link.to_stop}::${link.mode.toLowerCase()}`;
    const keyNoMode = `${link.from_stop}->${link.to_stop}`;
    const payload = { geometry: link.geometry, speedMps: Math.max(link.speed_mps || 0, 0) };
    if (!linkByPair.has(keyWithMode)) linkByPair.set(keyWithMode, payload);
    if (!linkByPairNoMode.has(keyNoMode)) linkByPairNoMode.set(keyNoMode, payload);
  }

  const out: VehicleRouteSeed[] = [];
  for (const service of scenario.world.services) {
    if (service.stop_sequence.length < 2) continue;
    const rawAssignedUnits =
      service.stock_units_assigned ??
      service.stock_units_owned ??
      service.rolling_stock_profile?.units_owned ??
      0;
    const assignedUnits =
      typeof rawAssignedUnits === "number" && Number.isFinite(rawAssignedUnits)
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
    const destinationName = destinationStopId
      ? stopPointById.get(destinationStopId)?.name ?? destinationStopId
      : null;
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
}

export function buildVehicleData(args: {
  gameMode: boolean;
  trainsAuthoritative?: boolean;
  scenario: ScenarioLite | null;
  runtimeTrains?: TrainRuntimeView[];
  serviceById: Map<string, ServiceLite>;
  vehicleRouteSeeds: VehicleRouteSeed[];
  clockTickSeconds: number;
  serviceLoadByServiceId?: Record<string, number>;
  xyToLngLat: XyToLngLat;
  modeColor: ModeColor;
  runtimeVehicleGeoJson: OverlayGeoCollection;
}): VehicleData {
  const {
    gameMode,
    trainsAuthoritative,
    scenario,
    runtimeTrains,
    serviceById,
    vehicleRouteSeeds,
    clockTickSeconds,
    serviceLoadByServiceId,
    xyToLngLat,
    modeColor,
    runtimeVehicleGeoJson,
  } = args;

  const byId = new Map<string, VehicleSnapshot>();
  const crs = scenario?.meta?.crs?.type;

  if (gameMode || trainsAuthoritative) {
    const trains = Array.isArray(runtimeTrains) ? runtimeTrains : [];
    for (const train of trains) {
      const coord = xyToLngLat(train.x, train.y, crs);
      if (!coord) continue;
      const service = serviceById.get(train.service_id);
      const displayColor = service?.display_color ?? modeColor(train.mode, train.mode_variant ?? null);
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
    return { geojson: runtimeVehicleGeoJson, byId };
  }

  const features: OverlayGeoFeature[] = [];
  const lineOrdinalByLine = new Map<string, number>();

  for (const seed of vehicleRouteSeeds) {
    const serviceLoadRatio = Math.max(serviceLoadByServiceId?.[seed.serviceId] ?? 0, 0);
    const effectiveLoadRatio = Math.min(serviceLoadRatio, 1);
    for (let vehicleIndex = 0; vehicleIndex < seed.vehiclesOnService; vehicleIndex += 1) {
      const phaseS =
        ((clockTickSeconds + vehicleIndex * seed.headwayS) % seed.cycleS + seed.cycleS) %
        seed.cycleS;
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

  return { geojson: overlayCollection(features), byId };
}

export function reconcileRuntimeVehicleGeoJson(args: {
  gameMode: boolean;
  trainsAuthoritative?: boolean;
  vehicleData: VehicleData;
  runtimeVehicleFeatureById: Map<string, OverlayGeoFeature>;
}): { changed: boolean; nextGeojson: OverlayGeoCollection | null } {
  const { gameMode, trainsAuthoritative, vehicleData, runtimeVehicleFeatureById } = args;

  if (!(gameMode || trainsAuthoritative)) {
    runtimeVehicleFeatureById.clear();
    return { changed: true, nextGeojson: vehicleData.geojson };
  }

  const nextSeen = new Set<string>();
  let changed = false;

  for (const snapshot of vehicleData.byId.values()) {
    nextSeen.add(snapshot.vehicleId);
    const existing = runtimeVehicleFeatureById.get(snapshot.vehicleId);
    if (!existing) {
      runtimeVehicleFeatureById.set(snapshot.vehicleId, {
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

  for (const vehicleId of Array.from(runtimeVehicleFeatureById.keys())) {
    if (!nextSeen.has(vehicleId)) {
      runtimeVehicleFeatureById.delete(vehicleId);
      changed = true;
    }
  }

  if (!changed) return { changed: false, nextGeojson: null };
  return {
    changed: true,
    nextGeojson: overlayCollection(Array.from(runtimeVehicleFeatureById.values())),
  };
}

export function vehicleTypeLabel(mode: string): string {
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
