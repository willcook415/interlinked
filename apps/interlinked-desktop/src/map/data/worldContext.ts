import {
  POLYGON_TO_CELLS_FLAGS,
  cellToBoundary,
  cellsToMultiPolygon,
  isValidCell,
  latLngToCell,
  polygonToCells,
  polygonToCellsExperimental,
} from "h3-js";

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

export function regionH3CellId(
  region: Pick<RegionStatus, "h3_cell_id" | "region_id"> | null
): string | null {
  if (!region) return null;
  const explicitToken = region.h3_cell_id?.trim().toLowerCase();
  if (explicitToken && isValidCell(explicitToken)) return explicitToken;
  return parseLegacyH3Region(region.region_id);
}

export function parseRegionGeometry(
  region: RegionStatus | null,
  fallbackGeometry: GeoJsonGeometry | null = null
): GeoJsonGeometry | null {
  // Runtime region geometry from RegionStatus is authoritative.
  // Optional fallback is compatibility-only.
  return region?.geometry ?? fallbackGeometry ?? null;
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
  if (zoom >= 11.4) return "full";
  if (zoom >= 7.2) return "mid";
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
    const sourcePlayable =
      feature.properties?.playable_now === true ||
      Number(feature.properties?.playable_now ?? 0) === 1;
    const unlockedNow = sourcePlayable && unlockedSet.has(iso) ? 1 : 0;
    return {
      ...feature,
      properties: {
        ...feature.properties,
        unlocked_now: unlockedNow,
        playable_now: sourcePlayable ? 1 : 0,
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
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): GeoCollection {
  const { regions, resolveRegionGeometry } = args;
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
        },
      });
      continue;
    }
    const token = regionH3CellId(region);
    if (!token || !isValidCell(token)) continue;
    // h3-js with geoJson=true already returns [lng, lat] coordinate order.
    const lngLatBoundary = cellToBoundary(token, true) as [number, number][];
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
      },
    });
  }
  return fc(features);
}

function polygonOuterRing(geometry: GeoJsonGeometry): [number, number][] | null {
  if (geometry.type === "Polygon") {
    const ring = geometry.coordinates[0];
    if (!Array.isArray(ring) || ring.length < 3) return null;
    return ring.map((point) => [Number(point[0]), Number(point[1])] as [number, number]);
  }
  const first = geometry.coordinates[0]?.[0];
  if (!Array.isArray(first) || first.length < 3) return null;
  return first.map((point) => [Number(point[0]), Number(point[1])] as [number, number]);
}

function centroidFromRing(ring: [number, number][]): [number, number] | null {
  if (ring.length < 3) return null;
  let sx = 0;
  let sy = 0;
  let n = 0;
  for (const [lng, lat] of ring) {
    if (!Number.isFinite(lng) || !Number.isFinite(lat)) continue;
    sx += lng;
    sy += lat;
    n += 1;
  }
  if (n === 0) return null;
  return [sx / n, sy / n];
}

function tryLatLngToRes6Cell(lat: number, lng: number): string | null {
  try {
    return latLngToCell(lat, lng, 6);
  } catch {
    return null;
  }
}

function regionById(regions: RegionStatus[]): Map<string, RegionStatus> {
  const out = new Map<string, RegionStatus>();
  for (const region of regions) out.set(region.region_id, region);
  return out;
}

export function buildPlanningHexFeatures(args: {
  regions: RegionStatus[];
  regionFeatures: GeoCollection;
}): GeoCollection {
  const { regions, regionFeatures } = args;
  const regionsById = regionById(regions);
  const features: GeoFeature[] = [];
  let fallbackCounter = 0;

  for (const feature of regionFeatures.features) {
    const regionId = String(feature.properties?.region_id ?? "").trim();
    if (!regionId) continue;
    const region = regionsById.get(regionId) ?? null;
    const regionName =
      typeof feature.properties?.name === "string"
        ? feature.properties.name
        : region?.name ?? regionId;
    const sourceCode = (region?.source_code ?? "").trim().toLowerCase();
    const assignmentState =
      sourceCode === "manual_region_unassigned_hex"
        ? "unassigned"
        : sourceCode.startsWith("manual_region_definition")
        ? "manual_assigned"
        : "other_assigned";
    const isUnassigned = assignmentState === "unassigned" ? 1 : 0;
    const isManualAssigned = assignmentState === "manual_assigned" ? 1 : 0;
    const unlocked = Number(feature.properties?.unlocked ?? 0) === 1 ? 1 : 0;

    const polygons =
      feature.geometry.type === "Polygon"
        ? [feature.geometry.coordinates as number[][][]]
        : feature.geometry.type === "MultiPolygon"
        ? (feature.geometry.coordinates as number[][][][])
        : ([] as number[][][][]);
    if (polygons.length === 0) continue;

    const regionCell =
      typeof region?.h3_cell_id === "string" && isValidCell(region.h3_cell_id)
        ? region.h3_cell_id.toLowerCase()
        : null;

    for (let i = 0; i < polygons.length; i++) {
      const polygon = polygons[i];
      const geometry: GeoJsonGeometry = {
        type: "Polygon",
        coordinates: polygon as number[][][],
      };
      const ring = polygonOuterRing(geometry);
      const centroid = ring ? centroidFromRing(ring) : null;
      const derivedCell =
        centroid && Number.isFinite(centroid[0]) && Number.isFinite(centroid[1])
          ? tryLatLngToRes6Cell(centroid[1], centroid[0])
          : null;
      const cellId =
        regionCell && polygons.length === 1
          ? regionCell
          : derivedCell && isValidCell(derivedCell)
          ? derivedCell.toLowerCase()
          : `poly:${regionId}:${String(++fallbackCounter).padStart(4, "0")}`;
      const hexResolved = isValidCell(cellId) ? 1 : 0;

      let polygonCanonical: number | null = null;
      if (
        Array.isArray(region?.constituent_hex_numbers) &&
        typeof region.constituent_hex_numbers[i] === "number" &&
        Number.isFinite(region.constituent_hex_numbers[i]) &&
        region.constituent_hex_numbers[i] > 0
      ) {
        polygonCanonical = region.constituent_hex_numbers[i];
      } else if (
        typeof region?.canonical_hex_number === "number" &&
        Number.isFinite(region.canonical_hex_number) &&
        region.canonical_hex_number > 0
      ) {
        polygonCanonical = region.canonical_hex_number;
      }

      features.push({
        type: "Feature",
        geometry,
        properties: {
          region_id: regionId,
          name: regionName,
          unlocked,
          region_source_code: sourceCode,
          hex_assignment_state: assignmentState,
          hex_unassigned: isUnassigned,
          hex_manual_assigned: isManualAssigned,
          hex_id: cellId,
          hex_resolved: hexResolved,
          hex_canonical_number: polygonCanonical,
        },
      });
    }
  }
  return fc(features);
}

export type UkHexCoverageDiagnostics = {
  expected_land_hexes: number;
  covered_hexes: number;
  missing_land_hexes: number;
  extra_non_land_hexes: number;
  missing_hex_ids: string[];
  coverage_ratio: number;
  error: string | null;
};

function isUkCountryCode(value: unknown): boolean {
  const iso = String(value ?? "")
    .trim()
    .toUpperCase();
  return iso === "UK" || iso === "GB";
}

function addPolygonCoverageHexes(target: Set<string>, coordinates: number[][][]): void {
  // Coastline-aware expected coverage:
  // prefer overlap mode so coastal slivers/islands are not dropped by centroid-only rules.
  let ids: string[] = [];
  try {
    ids = polygonToCellsExperimental(
      coordinates,
      6,
      POLYGON_TO_CELLS_FLAGS.containmentOverlapping,
      true
    );
  } catch {
    ids = polygonToCells(coordinates, 6, true);
  }
  for (const id of ids.map((id) => String(id).trim().toLowerCase()).filter((id) => id.length > 0)) {
    target.add(id);
  }
}

export function buildUkHexCoverageDiagnostics(
  worldCountryData: GeoCollection,
  planningHexData: GeoCollection
): UkHexCoverageDiagnostics {
  try {
    const expected = new Set<string>();
    for (const feature of worldCountryData.features) {
      if (!isUkCountryCode(feature.properties?.country_iso2)) continue;
      if (feature.geometry.type === "Polygon") {
        addPolygonCoverageHexes(expected, feature.geometry.coordinates as number[][][]);
      } else if (feature.geometry.type === "MultiPolygon") {
        for (const polygon of feature.geometry.coordinates as number[][][][]) {
          addPolygonCoverageHexes(expected, polygon);
        }
      }
    }

    const covered = new Set<string>();
    for (const feature of planningHexData.features) {
      const hexId = String(feature.properties?.hex_id ?? "")
        .trim()
        .toLowerCase();
      if (hexId.length === 0 || !isValidCell(hexId)) continue;
      covered.add(hexId);
    }

    const missing = Array.from(expected).filter((id) => !covered.has(id));
    missing.sort();
    const extraCount = Array.from(covered).filter((id) => !expected.has(id)).length;
    const expectedCount = expected.size;
    const coveredCount = expectedCount - missing.length;
    const coverageRatio = expectedCount > 0 ? coveredCount / expectedCount : 1;
    return {
      expected_land_hexes: expectedCount,
      covered_hexes: coveredCount,
      missing_land_hexes: missing.length,
      extra_non_land_hexes: extraCount,
      missing_hex_ids: missing,
      coverage_ratio: coverageRatio,
      error: null,
    };
  } catch (error) {
    return {
      expected_land_hexes: 0,
      covered_hexes: 0,
      missing_land_hexes: 0,
      extra_non_land_hexes: 0,
      missing_hex_ids: [],
      coverage_ratio: 0,
      error: String(error ?? "Failed to compute UK hex coverage diagnostics"),
    };
  }
}

export function buildHexCoverageGapFeatures(hexIds: string[], startNumber = 1): GeoCollection {
  const features: GeoFeature[] = [];
  let index = 0;
  for (const rawId of hexIds) {
    const hexId = String(rawId).trim().toLowerCase();
    if (!hexId || !isValidCell(hexId)) continue;
    const boundary = cellToBoundary(hexId, true) as [number, number][];
    if (!Array.isArray(boundary) || boundary.length < 3) continue;
    const first = boundary[0];
    const ring =
      boundary[boundary.length - 1][0] === first[0] &&
      boundary[boundary.length - 1][1] === first[1]
        ? boundary
        : [...boundary, [first[0], first[1]]];
    features.push({
      type: "Feature",
      geometry: { type: "Polygon", coordinates: [ring] },
      properties: {
        hex_id: hexId,
        gap: 1,
        hex_num: startNumber + index,
        region_id: `r6:UK:${hexId}`,
        name: `Hex #${startNumber + index}`,
        hex_assignment_state: "coverage_backfill_gap",
        hex_unassigned: 1,
        hex_manual_assigned: 0,
        hex_resolved: 1,
        unlocked: 0,
      },
    });
    index += 1;
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
        const token = regionH3CellId(region);
        if (token && isValidCell(token)) {
          const boundary = cellToBoundary(token, true) as [number, number][];
          let minLng = Infinity;
          let minLat = Infinity;
          let maxLng = -Infinity;
          let maxLat = -Infinity;
          for (const [lng, lat] of boundary) {
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
  countyGeometryData: GeoCollection;
  regions: RegionStatus[];
  resolveRegionGeometry: (region: RegionStatus | null) => GeoJsonGeometry | null;
}): CountyBoundsDatum[] {
  const { countyGeometryData, regions, resolveRegionGeometry } = args;
  const mergedByCounty = new Map<string, CountyBoundsDatum>();

  // Legacy compatibility helper for county-sliced basemap chunking.
  // Active runtime region rendering does not require this path.
  for (const feature of countyGeometryData.features) {
    if (feature.geometry.type === "Point" || feature.geometry.type === "LineString") continue;
    const bounds = flattenBounds(boundsFromGeometry(feature.geometry as GeoJsonGeometry));
    if (!bounds) continue;
    const countyIdRaw = feature.properties?.county_id;
    const countyId =
      typeof countyIdRaw === "string" && countyIdRaw.trim().length > 0
        ? countyIdRaw.trim()
        : countyIdFromRegionId(String(feature.properties?.region_id ?? ""));
    if (!countyId) continue;
    const regionIdRaw = feature.properties?.region_id;
    const regionId = typeof regionIdRaw === "string" ? regionIdRaw : `county:${countyId}`;
    const existing = mergedByCounty.get(countyId);
    if (!existing) {
      mergedByCounty.set(countyId, { regionId, countyId, bounds });
      continue;
    }
    existing.bounds = [
      Math.min(existing.bounds[0], bounds[0]),
      Math.min(existing.bounds[1], bounds[1]),
      Math.max(existing.bounds[2], bounds[2]),
      Math.max(existing.bounds[3], bounds[3]),
    ];
  }

  if (mergedByCounty.size > 0) {
    return Array.from(mergedByCounty.values());
  }

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

export function buildRegionDisplayFeatures(
  regionFeatures: GeoCollection,
  planningHexData: GeoCollection,
  regions: RegionStatus[]
): GeoCollection {
  const regionsById = regionById(regions);
  const hexesByRegion = new Map<string, string[]>();

  for (const feature of planningHexData.features) {
    const regionId = String(feature.properties?.region_id ?? "").trim();
    const hexId = String(feature.properties?.hex_id ?? "").trim();
    if (regionId && hexId && hexId !== "__none__" && isValidCell(hexId)) {
      let list = hexesByRegion.get(regionId);
      if (!list) {
        list = [];
        hexesByRegion.set(regionId, list);
      }
      list.push(hexId);
    }
  }

  const outputFeatures: GeoFeature[] = [];

  for (const feature of regionFeatures.features) {
    const regionId = String(feature.properties?.region_id ?? "").trim();
    const region = regionsById.get(regionId);
    if (!region) {
      outputFeatures.push(feature);
      continue;
    }

    const sourceCode = (region.source_code ?? "").trim().toLowerCase();
    const isManual =
      sourceCode.startsWith("manual_region_definition") ||
      sourceCode === "manual_region_unassigned_hex" ||
      sourceCode.startsWith("manual_region_");

    if (isManual) {
      const cells = hexesByRegion.get(regionId);
      if (cells && cells.length > 0) {
        try {
          const multiPoly = cellsToMultiPolygon(cells, true);
          if (multiPoly && multiPoly.length > 0) {
            outputFeatures.push({
              ...feature,
              geometry: {
                type: "MultiPolygon",
                coordinates: multiPoly as number[][][][],
              },
            });
            continue;
          }
        } catch (err) {
          console.warn(`Failed to dissolve manual region ${regionId}`, err);
        }
      }
    }

    // fallback mapping
    outputFeatures.push(feature);
  }

  return fc(outputFeatures);
}
