import type { LngLatBoundsLike } from "maplibre-gl";

import type { GeoJsonGeometry } from "../../types";
import type { XY } from "./coords";

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

export function boundsFromGeometry(
  geometry: GeoJsonGeometry | null
): LngLatBoundsLike | null {
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

export function labelPointFromGeometry(
  geometry: GeoJsonGeometry | null
): [number, number] | null {
  const bounds = boundsFromGeometry(geometry);
  if (!bounds) return null;
  const [[minLng, minLat], [maxLng, maxLat]] = bounds as [
    [number, number],
    [number, number],
  ];
  return [(minLng + maxLng) * 0.5, (minLat + maxLat) * 0.5];
}

function normalizedLon360(lng: number): number {
  if (!Number.isFinite(lng)) return 0;
  const wrapped = lng % 360;
  return wrapped < 0 ? wrapped + 360 : wrapped;
}

function ringCenterAndScore(
  ring: number[][]
): { point: [number, number]; score: number } | null {
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

function polygonLabelPointAndScore(
  polygon: number[][][]
): { point: [number, number]; score: number } | null {
  if (!Array.isArray(polygon) || polygon.length === 0) return null;
  return ringCenterAndScore(polygon[0]);
}

export function countryLabelPointAndArea(
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

export function flattenBounds(
  bounds: LngLatBoundsLike | null
): [number, number, number, number] | null {
  if (!bounds) return null;
  const [[minLng, minLat], [maxLng, maxLat]] = bounds as [
    [number, number],
    [number, number],
  ];
  if (![minLng, minLat, maxLng, maxLat].every((value) => Number.isFinite(value))) {
    return null;
  }
  return [minLng, minLat, maxLng, maxLat];
}

export function boundsIntersect(
  a: [number, number, number, number],
  b: [number, number, number, number]
): boolean {
  return a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1];
}

export function padBounds(
  bounds: [number, number, number, number],
  pad: number
): [number, number, number, number] {
  return [bounds[0] - pad, bounds[1] - pad, bounds[2] + pad, bounds[3] + pad];
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

export function pointInGeometry(point: XY, geometry: GeoJsonGeometry | null): boolean {
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
