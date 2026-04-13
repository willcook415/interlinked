import type { RegionStatus } from "../../types";

function looksLikeHexToken(value: string): boolean {
  return /^[0-9a-f]{10,}$/i.test(value.trim());
}

function isSubstrateName(name: string, regionId: string): boolean {
  const trimmed = name.trim();
  if (!trimmed) return true;
  if (/^[A-Z]{2}\s+[0-9a-f]{10,}$/i.test(trimmed)) return true;
  if (/^r\d+:[A-Z]{2}:[0-9a-f]{10,}$/i.test(regionId.trim())) return true;
  if (/^county:[A-Z]{2}:[0-9a-z_-]+$/i.test(regionId.trim()) && looksLikeHexToken(trimmed)) {
    return true;
  }
  return false;
}

export function buildRegionDisplayNames(regions: RegionStatus[]): Map<string, string> {
  const map = new Map<string, string>();
  const sorted = [...regions].sort((a, b) =>
    a.region_id.localeCompare(b.region_id)
  );
  let syntheticIndex = 1;
  for (const region of sorted) {
    const explicitName = region.name.trim();
    const sourceCode = (region.source_code ?? "").trim().toLowerCase();
    if (sourceCode.startsWith("manual_region_") || /^hex\s+#\d+$/i.test(explicitName)) {
      map.set(region.region_id, explicitName || region.region_id);
      continue;
    }
    if (!isSubstrateName(region.name, region.region_id)) {
      map.set(region.region_id, region.name.trim());
      continue;
    }
    map.set(
      region.region_id,
      `Planning Region ${String(syntheticIndex).padStart(2, "0")}`
    );
    syntheticIndex += 1;
  }
  return map;
}
