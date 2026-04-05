import { cellToBoundary, isValidCell } from "h3-js";

import type { GeoJsonAnyGeometry, GeoJsonGeometry, RegionStatus } from "../../types";
import {
  boundsFromGeometry,
  countryLabelPointAndArea,
  flattenBounds,
  labelPointFromGeometry,
} from "../geo/geometry";
import { fc, type GeoCollection, type GeoFeature } from "./contracts";

export function parseLegacyH3Region(
  regionId: string | null | undefined
): string | null {
  if (!regionId) return null;
  const match = /^r6:[A-Za-z]{2}:([0-9a-f]+)$/i.exec(regionId.trim());
  return match ? match[1].toLowerCase() : null;
}

export function parseCountyGeometry(region: RegionStatus | null): GeoJsonGeometry | null {
  return region?.geometry ?? null;
}

export function countyIdFromRegionId(regionId: string): string | null {
  const parts = regionId.trim().split(":");
  if (parts.length >= 3 && parts[0] === "county") return parts[2];
  return null;
}

export function makeUrlFromTemplate(
  template: string | null | undefined,
  countyId: string
): string | null {
  if (!template) return null;
  return template.replace("{county_id}", encodeURIComponent(countyId));
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function normalizeBasemapFeatureCollection(input: unknown): GeoCollection {
  const features = asArray((input as { features?: unknown[] } | null)?.features)
    .map((feature): GeoFeature | null => {
      if (!feature || typeof feature !== "object") return null;
      const geometry = (feature as { geometry?: GeoJsonAnyGeometry | null }).geometry;
      if (!geometry || typeof geometry !== "object") return null;
      const properties = {
        ...(((feature as { properties?: Record<string, unknown> | null }).properties ?? {}) as Record<
          string,
          unknown
        >),
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

export async function fetchFeatureCollection(url: string): Promise<GeoCollection> {
  const response = await fetch(url, { cache: "force-cache" });
  if (!response.ok) {
    throw new Error(`failed to load ${url}: ${response.status}`);
  }
  const payload = (await response.json()) as unknown;
  return normalizeBasemapFeatureCollection(payload);
}

export function mergeFeatureCollections(collections: GeoCollection[]): GeoCollection {
  const features = collections.flatMap((collection) => collection.features);
  return fc(features);
}

export function basemapTierForZoom(zoom: number): "none" | "mid" | "full" {
  if (zoom >= 10.0) return "full";
  return "none";
}

export function buildWorldCountryData(
  worldContextData: GeoCollection,
  visibleCountryIso2: string[] | null
): GeoCollection {
  const unlockedSet = new Set(
    (visibleCountryIso2 ?? [])
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
}

export function buildWorldCountryLabelData(
  worldCountryData: GeoCollection
): GeoCollection {
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
}

export function buildCountyFeatures(args: {
  regions: RegionStatus[];
  focusRegionId: string | null;
  selectedRegionId: string | null;
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): GeoCollection {
  const { regions, focusRegionId, selectedRegionId, resolveRegionGeometry } = args;
  const features: GeoFeature[] = [];
  for (const region of regions) {
    const geometry = resolveRegionGeometry(region);
    if (geometry) {
      features.push({
        type: "Feature",
        geometry,
        properties: {
          region_id: region.region_id,
          name: region.name,
          unlocked: region.unlocked ? 1 : 0,
          focus: region.region_id === focusRegionId ? 1 : 0,
          selected: region.region_id === selectedRegionId ? 1 : 0,
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
        focus: region.region_id === focusRegionId ? 1 : 0,
        selected: region.region_id === selectedRegionId ? 1 : 0,
      },
    });
  }
  return fc(features);
}

export type CountyLabelDatum = {
  regionId: string;
  name: string;
  point: [number, number];
  focus: boolean;
};

export function buildCountyLabelData(args: {
  regions: RegionStatus[];
  focusRegionId: string | null;
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): CountyLabelDatum[] {
  const { regions, focusRegionId, resolveRegionGeometry } = args;
  return regions
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
          if (
            Number.isFinite(minLng) &&
            Number.isFinite(minLat) &&
            Number.isFinite(maxLng) &&
            Number.isFinite(maxLat)
          ) {
            point = [(minLng + maxLng) * 0.5, (minLat + maxLat) * 0.5];
          }
        }
      }
      return point
        ? {
            regionId: region.region_id,
            name: region.name,
            point,
            focus: region.region_id === focusRegionId,
          }
        : null;
    })
    .filter((value): value is CountyLabelDatum => Boolean(value));
}

export type CountyBoundsDatum = {
  regionId: string;
  countyId: string;
  bounds: [number, number, number, number];
};

export function buildCountyBoundsData(args: {
  regions: RegionStatus[];
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): CountyBoundsDatum[] {
  const { regions, resolveRegionGeometry } = args;
  return regions
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
    .filter((value): value is CountyBoundsDatum => Boolean(value));
}
