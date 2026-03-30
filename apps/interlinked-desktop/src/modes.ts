export type ModeClass =
  | "bus"
  | "tram"
  | "metro"
  | "ferry"
  | "commuter_rail"
  | "high_speed_rail"
  | "rail"
  | "unknown";

export function normalizeModeToken(mode: string | null | undefined): string {
  return (mode ?? "").trim().toLowerCase();
}

export function normalizeModeVariantToken(modeVariant: string | null | undefined): string | null {
  const token = (modeVariant ?? "").trim().toLowerCase();
  return token.length > 0 ? token : null;
}

export function modeIdentityKey(mode: string | null | undefined, modeVariant: string | null | undefined): string {
  return `${normalizeModeToken(mode)}:${normalizeModeVariantToken(modeVariant) ?? ""}`;
}

export function canonicalModeClass(mode: string | null | undefined, modeVariant: string | null | undefined): ModeClass {
  const base = normalizeModeToken(mode);
  const variant = normalizeModeVariantToken(modeVariant) ?? "";
  if (base.includes("bus")) return "bus";
  if (base.includes("tram")) return "tram";
  if (base.includes("metro") || base.includes("subway") || base.includes("underground")) return "metro";
  if (base.includes("ferry") || base.includes("boat")) return "ferry";
  if (base.includes("rail") || base.includes("train")) {
    if (variant.includes("high_speed") || variant.includes("highspeed")) return "high_speed_rail";
    if (variant.includes("commuter") || variant.includes("suburban")) return "commuter_rail";
    return "rail";
  }
  if (variant.includes("high_speed") || variant.includes("highspeed")) return "high_speed_rail";
  if (variant.includes("commuter") || variant.includes("suburban")) return "commuter_rail";
  if (variant.includes("bus")) return "bus";
  if (variant.includes("tram")) return "tram";
  if (variant.includes("metro")) return "metro";
  if (variant.includes("ferry")) return "ferry";
  if (variant.includes("rail") || variant.includes("train")) return "rail";
  return "unknown";
}

export function modeClassFromStopType(stopType: string | null | undefined): ModeClass {
  return canonicalModeClass(stopType, null);
}

export function isBusMode(mode: string | null | undefined, modeVariant: string | null | undefined): boolean {
  return canonicalModeClass(mode, modeVariant) === "bus";
}

export function isMajorMode(mode: string | null | undefined, modeVariant: string | null | undefined): boolean {
  const modeClass = canonicalModeClass(mode, modeVariant);
  return (
    modeClass === "metro" ||
    modeClass === "commuter_rail" ||
    modeClass === "high_speed_rail" ||
    modeClass === "rail"
  );
}
