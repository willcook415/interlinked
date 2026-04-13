import {
  canonicalModeClass as canonicalModeClassToken,
  isMajorMode,
  modeClassFromStopType as modeClassFromStopTypeToken,
} from "../../modes";
import type { LinkModeFilter } from "../../ui/MapFiltersPanel";
import type { GeoJsonGeometry, RegionStatus, ScenarioLite } from "../../types";
import { type XY, safeRes6Token, xyToLngLat } from "../geo/coords";
import { pointInGeometry } from "../geo/geometry";
import { serviceLineId } from "../runtimeVehicleOverlay";
import { fc, type GeoCollection, type GeoFeature } from "./contracts";
import { regionH3CellId } from "./worldContext";

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

export function modeClass(
  mode: string | null | undefined,
  modeVariant: string | null | undefined
): string {
  return canonicalModeClassToken(mode, modeVariant);
}

export function modeColor(
  mode: string | null | undefined,
  modeVariant: string | null | undefined
): string {
  const key = modeClass(mode, modeVariant);
  return MODE_COLOR_BY_CLASS[key] ?? MODE_COLOR_BY_CLASS.unknown;
}

export function formatDistanceKm(distanceM: number): string {
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

function modeMatches(mode: string, filter: LinkModeFilter): boolean {
  return filter === "all" || mode.toLowerCase() === filter;
}

export type StopPointWithName = {
  lng: number;
  lat: number;
  x: number;
  y: number;
  name: string;
};

export function buildStopPointById(
  scenario: ScenarioLite | null
): Map<string, StopPointWithName> {
  const out = new Map<string, StopPointWithName>();
  const crs = scenario?.meta?.crs?.type;
  for (const stop of scenario?.world.stops ?? []) {
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
}

export type NetworkGeojsonData = {
  links: GeoCollection;
  transfers: GeoCollection;
  stops: GeoCollection;
  zones: GeoCollection;
};

export function buildNetworkGeojsonData(args: {
  scenario: ScenarioLite | null;
  linkMode: LinkModeFilter;
  visibleCountryIso2: string[] | null;
  focusRegion: RegionStatus | null;
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): NetworkGeojsonData {
  const { scenario, linkMode, visibleCountryIso2, focusRegion, resolveRegionGeometry } =
    args;
  if (!scenario) return { links: fc(), transfers: fc(), stops: fc(), zones: fc() };
  const allowedCountries = visibleCountryIso2
    ? new Set(visibleCountryIso2.map((code) => code.trim().toUpperCase()))
    : null;
  const crs = scenario.meta?.crs?.type;
  const stopCoords = new Map<string, XY>();
  const stopInFocus = new Map<string, boolean>();
  const stopDegree = new Map<string, number>();
  const stopColors = new Map<string, string>();
  const stopModeClass = new Map<string, string>();
  const lineColors = new Map<string, string>();
  const lineModes = new Map<string, { mode: string; modeVariant: string | null }>();
  const lineByPair = new Map<string, string>();

  for (const link of scenario.world.links) {
    stopDegree.set(link.from_stop, (stopDegree.get(link.from_stop) ?? 0) + 1);
    stopDegree.set(link.to_stop, (stopDegree.get(link.to_stop) ?? 0) + 1);
  }

  for (const service of scenario.world.services) {
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

  for (const stop of scenario.world.stops) {
    const coord = xyToLngLat(stop.x, stop.y, crs);
    if (!coord) continue;
    const iso = stop.country_iso2?.trim().toUpperCase() ?? null;
    if (allowedCountries && iso && !allowedCountries.has(iso)) continue;
    stopCoords.set(stop.id, coord);
    const inFocusByGeometry = pointInGeometry(coord, resolveRegionGeometry(focusRegion));
    const inFocusByH3Region =
      !inFocusByGeometry &&
      Boolean(focusRegion && regionH3CellId(focusRegion) === safeRes6Token(coord.lat, coord.lng));
    stopInFocus.set(stop.id, inFocusByGeometry || inFocusByH3Region);
  }

  const linkFeatures: GeoFeature[] = [];
  for (const link of scenario.world.links) {
    if (!modeMatches(link.mode, linkMode)) continue;
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
      modeColor(
        lineMode?.mode ?? link.mode,
        lineMode?.modeVariant ?? link.mode_variant ?? null
      );
    linkFeatures.push({
      type: "Feature",
      geometry: { type: "LineString", coordinates },
      properties: {
        id: link.id,
        line_id: lineId,
        display_color: displayColor,
        mode_class: modeClass(
          lineMode?.mode ?? link.mode,
          lineMode?.modeVariant ?? link.mode_variant ?? null
        ),
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
  for (const transfer of scenario.world.transfers) {
    const from = stopCoords.get(transfer.from_stop);
    const to = stopCoords.get(transfer.to_stop);
    if (!from || !to) continue;
    const fromFocus = stopInFocus.get(transfer.from_stop) ?? false;
    const toFocus = stopInFocus.get(transfer.to_stop) ?? false;
    const pairKey = [transfer.from_stop, transfer.to_stop].sort().join("::");
    if (seenTransferPairs.has(pairKey)) continue;
    seenTransferPairs.add(pairKey);
    const transferTimeS = Number.isFinite(transfer.time_s)
      ? Math.max(transfer.time_s, 0)
      : 0;
    const penaltyS = Number.isFinite(transfer.penalty_s ?? 0)
      ? Math.max(transfer.penalty_s ?? 0, 0)
      : 0;
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
  for (const stop of scenario.world.stops) {
    const coord = stopCoords.get(stop.id);
    if (!coord) continue;
    const resolvedModeClass =
      stopModeClass.get(stop.id) ?? modeClassFromStopType(stop.stop_type);
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
        major_interchange:
          stop.interchange_id || (stopDegree.get(stop.id) ?? 0) >= 5 ? 1 : 0,
        shape_stop: (stop.stop_type ?? "").toLowerCase().includes("shape") ? 1 : 0,
      },
    });
  }

  const zoneFeatures: GeoFeature[] = [];
  for (const zone of scenario.world.zones) {
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
}
