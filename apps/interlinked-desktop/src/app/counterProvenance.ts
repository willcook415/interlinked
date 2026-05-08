import type { CounterProvenance } from "../types";

const KNOWN_PROVENANCE = new Set<string>([
  "authoritative_sim",
  "strategic_estimate",
  "runtime_projection",
  "animation_only",
  "debug_legacy",
]);

export function normalizeCounterProvenance(
  value: CounterProvenance | string | null | undefined,
  fallback: CounterProvenance = "debug_legacy"
): CounterProvenance {
  const normalized = typeof value === "string" ? value.trim().toLowerCase() : "";
  return KNOWN_PROVENANCE.has(normalized) ? (normalized as CounterProvenance) : fallback;
}

export function formatCounterProvenance(
  value: CounterProvenance | string | null | undefined
): string {
  switch (normalizeCounterProvenance(value)) {
    case "authoritative_sim":
      return "Sim truth";
    case "strategic_estimate":
      return "Sim estimate";
    case "runtime_projection":
      return "Live projection";
    case "animation_only":
      return "Animation";
    case "debug_legacy":
      return "Legacy diagnostic";
    default:
      return "Legacy diagnostic";
  }
}

export function isPlayerMeaningfulCounter(
  value: CounterProvenance | string | null | undefined
): boolean {
  const provenance = normalizeCounterProvenance(value);
  return provenance === "authoritative_sim" || provenance === "strategic_estimate";
}

export function shouldShowDebugCounter(
  value: CounterProvenance | string | null | undefined
): boolean {
  return normalizeCounterProvenance(value) !== "debug_legacy";
}
