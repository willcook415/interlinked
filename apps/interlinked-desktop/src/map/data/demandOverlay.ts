import { cellToBoundary, isValidCell } from "h3-js";

import type {
  DemandOverlayCellDatum,
  DemandOverlayPayload,
  DemandOverlayType,
} from "../../types";
import { fc, type GeoCollection, type GeoFeature } from "./contracts";

type DemandOverlayGeojson = {
  cells: GeoCollection;
};

type CellFeatureSeed = {
  row: DemandOverlayCellDatum;
  geometry: GeoFeature["geometry"];
  value: number;
  fallbackFlag: number;
};

type CellFeatureSeedBuildResult = {
  seeds: CellFeatureSeed[];
  droppedInvalidGeometry: number;
  droppedInvalidGeometrySample: string[];
};

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(Math.max(value, 0), 1);
}

function percentile(values: number[], q: number): number {
  if (values.length === 0) return 0;
  const sorted = values
    .filter((value) => Number.isFinite(value))
    .sort((left, right) => left - right);
  if (sorted.length === 0) return 0;
  const clamped = clamp01(q);
  const index = Math.min(sorted.length - 1, Math.floor(clamped * (sorted.length - 1)));
  return sorted[index] ?? 0;
}

function normalizeScale(values: number[]): number {
  const p95 = percentile(values, 0.95);
  if (!Number.isFinite(p95) || p95 <= 0) {
    const max = Math.max(0, ...values);
    return max > 0 ? max : 1;
  }
  return p95;
}

function quantileThresholds(values: number[]): [number, number, number, number] {
  const cleaned = values
    .filter((value) => Number.isFinite(value) && value > 0)
    .sort((left, right) => left - right);
  if (cleaned.length === 0) return [0, 0, 0, 0];
  if (cleaned.length === 1) {
    const only = cleaned[0] ?? 0;
    return [only * 0.25, only * 0.5, only * 0.75, only];
  }
  return [
    percentile(cleaned, 0.2),
    percentile(cleaned, 0.45),
    percentile(cleaned, 0.7),
    percentile(cleaned, 0.88),
  ];
}

function bandForValue(
  value: number,
  thresholds: [number, number, number, number]
): 0 | 1 | 2 | 3 | 4 {
  if (!Number.isFinite(value) || value <= 0) return 0;
  if (value < thresholds[0]) return 0;
  if (value < thresholds[1]) return 1;
  if (value < thresholds[2]) return 2;
  if (value < thresholds[3]) return 3;
  return 4;
}

function boostedNorm(value: number, scale: number): number {
  if (!Number.isFinite(value) || value <= 0 || !Number.isFinite(scale) || scale <= 0) return 0;
  return clamp01(Math.pow(value / scale, 0.62));
}

function toNonNegative(value: number | null | undefined): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(Number(value), 0);
}

function normalizeH3CellId(cellId: string): string | null {
  const trimmed = cellId.trim().toLowerCase();
  if (!trimmed) return null;
  if (isValidCell(trimmed)) return trimmed;
  const suffix = trimmed.split(":").pop() ?? "";
  return suffix && isValidCell(suffix) ? suffix : null;
}

function buildCellGeometry(cellId: string): GeoFeature["geometry"] | null {
  const h3CellId = normalizeH3CellId(cellId);
  if (!h3CellId) return null;
  const ring = cellToBoundary(h3CellId, true) as [number, number][];
  if (!Array.isArray(ring) || ring.length < 3) return null;
  const first = ring[0];
  const closedRing =
    ring[ring.length - 1][0] === first[0] && ring[ring.length - 1][1] === first[1]
      ? ring
      : [...ring, [first[0], first[1]] as [number, number]];
  return {
    type: "Polygon",
    coordinates: [closedRing],
  };
}

function overlayValueFor(row: DemandOverlayCellDatum, overlayType: DemandOverlayType): number {
  const residential = toNonNegative(row.allocated_residential_mass);
  const employment = toNonNegative(row.allocated_employment_mass);
  switch (overlayType) {
    case "residential_allocation":
      return residential;
    case "employment_allocation":
      return employment;
    case "total_allocation":
      return residential + employment;
    case "raw_residential_weight":
      return toNonNegative(row.raw_weight_residential);
    case "raw_employment_weight":
      return toNonNegative(row.raw_weight_employment);
    case "fallback_cells":
      return row.fallback_reason?.trim() ? 1 : 0;
    default:
      return residential + employment;
  }
}

function buildCellFeatureSeeds(
  payload: DemandOverlayPayload,
  overlayType: DemandOverlayType
): CellFeatureSeedBuildResult {
  const seeds: CellFeatureSeed[] = [];
  let droppedInvalidGeometry = 0;
  const droppedInvalidGeometrySample: string[] = [];
  for (const row of payload.cell_data ?? []) {
    const geometry = buildCellGeometry(row.cell_id);
    if (!geometry) {
      droppedInvalidGeometry += 1;
      if (droppedInvalidGeometrySample.length < 5) {
        droppedInvalidGeometrySample.push(row.cell_id);
      }
      continue;
    }
    seeds.push({
      row,
      geometry,
      value: overlayValueFor(row, overlayType),
      fallbackFlag: row.fallback_reason?.trim() ? 1 : 0,
    });
  }
  return {
    seeds,
    droppedInvalidGeometry,
    droppedInvalidGeometrySample,
  };
}

export function buildDemandOverlayGeojson(
  payload: DemandOverlayPayload | null,
  overlayType: DemandOverlayType
): DemandOverlayGeojson {
  if (!payload?.available || !payload.cell_data?.length) {
    return {
      cells: fc(),
    };
  }

  const { seeds, droppedInvalidGeometry, droppedInvalidGeometrySample } = buildCellFeatureSeeds(
    payload,
    overlayType
  );
  console.info(
    `[demand-overlay-geojson] overlay=${overlayType} payload_rows=${payload.cell_data?.length ?? 0} payload_total=${payload.cell_data_total ?? 0} mappable=${payload.cell_data_mappable ?? 0} dropped_invalid_geometry=${droppedInvalidGeometry} dropped_sample=${droppedInvalidGeometrySample.join("|")} seeds=${seeds.length}`
  );
  if (seeds.length === 0) {
    return {
      cells: fc(),
    };
  }

  const metricValues = seeds.map((seed) => toNonNegative(seed.value));
  const scale = overlayType === "fallback_cells" ? 1 : normalizeScale(metricValues);
  const thresholds =
    overlayType === "fallback_cells"
      ? ([0.2, 0.4, 0.6, 0.8] as [number, number, number, number])
      : quantileThresholds(metricValues);
  const fallbackCount = seeds.reduce((count, seed) => count + seed.fallbackFlag, 0);
  console.info(
    `[demand-overlay-scale] overlay=${overlayType} rows=${seeds.length} scale=${scale.toFixed(3)} thresholds=${thresholds.map((value) => value.toFixed(3)).join("|")} fallback_cells=${fallbackCount}`
  );

  const features: GeoFeature[] = seeds.map((seed) => {
    const row = seed.row;
    const allocatedResidentialMass = toNonNegative(row.allocated_residential_mass);
    const allocatedEmploymentMass = toNonNegative(row.allocated_employment_mass);
    const totalAllocation = allocatedResidentialMass + allocatedEmploymentMass;
    const rawResidentialWeight = toNonNegative(row.raw_weight_residential);
    const rawEmploymentWeight = toNonNegative(row.raw_weight_employment);
    const overlayValue = toNonNegative(seed.value);
    const overlayNorm =
      overlayType === "fallback_cells"
        ? (seed.fallbackFlag === 1 ? 1 : 0)
        : boostedNorm(overlayValue, scale);

    return {
      type: "Feature",
      geometry: seed.geometry,
      properties: {
        cell_id: row.cell_id,
        planning_region_id: row.planning_region_id?.trim() || null,
        lon: Number.isFinite(row.lon) ? row.lon : 0,
        lat: Number.isFinite(row.lat) ? row.lat : 0,
        area_m2: toNonNegative(row.area_m2),
        residents_night: toNonNegative(row.residents_night),
        jobs_day: toNonNegative(row.jobs_day),
        centrality_score: toNonNegative(row.centrality_score),
        data_quality_score: toNonNegative(row.data_quality_score),
        activity_mix_residential: toNonNegative(row.activity_mix_residential),
        activity_mix_office: toNonNegative(row.activity_mix_office),
        activity_mix_retail: toNonNegative(row.activity_mix_retail),
        activity_mix_recreation: toNonNegative(row.activity_mix_recreation),
        activity_mix_industrial: toNonNegative(row.activity_mix_industrial),
        activity_mix_education: toNonNegative(row.activity_mix_education),
        activity_mix_health: toNonNegative(row.activity_mix_health),
        raw_weight_residential: rawResidentialWeight,
        raw_weight_employment: rawEmploymentWeight,
        allocated_residential_mass: allocatedResidentialMass,
        allocated_employment_mass: allocatedEmploymentMass,
        total_allocation: totalAllocation,
        overlay_value: overlayValue,
        overlay_norm: overlayNorm,
        overlay_band: bandForValue(overlayValue, thresholds),
        fallback_flag: seed.fallbackFlag,
        fallback_reason: row.fallback_reason?.trim() || null,
      },
    };
  });

  return {
    cells: fc(features),
  };
}
