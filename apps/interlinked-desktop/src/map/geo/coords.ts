import { latLngToCell } from "h3-js";

export type XY = { lng: number; lat: number };

const WORLD_HALF_M = 20037508.342789244;

function inverseWebMercator(x: number, y: number): XY | null {
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  const lng = (x / WORLD_HALF_M) * 180.0;
  const ydeg = (y / WORLD_HALF_M) * 180.0;
  const lat =
    (180.0 / Math.PI) *
    (2.0 * Math.atan(Math.exp((ydeg * Math.PI) / 180.0)) - Math.PI / 2.0);
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  return Math.abs(lng) <= 180 && Math.abs(lat) <= 90 ? { lng, lat } : null;
}

function forwardWebMercator(lng: number, lat: number): { x: number; y: number } | null {
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  if (Math.abs(lng) > 180 || Math.abs(lat) > 90) return null;
  const x = (lng * WORLD_HALF_M) / 180.0;
  const clampedLat = Math.max(Math.min(lat, 85.05112878), -85.05112878);
  const y =
    Math.log(Math.tan(((90.0 + clampedLat) * Math.PI) / 360.0)) *
    (WORLD_HALF_M / Math.PI);
  return Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null;
}

export function xyToLngLat(x: number, y: number, crsType?: string): XY | null {
  const hint = crsType?.toLowerCase() ?? "";
  const alreadyLngLat = Math.abs(x) <= 180 && Math.abs(y) <= 90;
  if (hint.includes("4326") || hint.includes("wgs84")) {
    return alreadyLngLat ? { lng: x, lat: y } : null;
  }
  if (hint.includes("3857") || hint.includes("mercator")) {
    return inverseWebMercator(x, y);
  }
  return alreadyLngLat ? { lng: x, lat: y } : inverseWebMercator(x, y);
}

export function lngLatToXY(
  lng: number,
  lat: number,
  crsType?: string
): { x: number; y: number } | null {
  const hint = crsType?.toLowerCase() ?? "";
  if (hint.includes("4326") || hint.includes("wgs84")) {
    return { x: lng, y: lat };
  }
  if (hint.includes("3857") || hint.includes("mercator")) {
    return forwardWebMercator(lng, lat);
  }
  return forwardWebMercator(lng, lat) ?? { x: lng, y: lat };
}

export function safeRes6Token(lat: number, lng: number): string | null {
  try {
    return latLngToCell(lat, lng, 6).toLowerCase();
  } catch {
    return null;
  }
}
