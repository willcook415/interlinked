import type {
  LineOpsRuntimeView,
  SimulationAdvanceEconomy,
  SimulationClock,
  StationRuntimeView,
  TrainRuntimeView,
} from "../types";

export type ClockFreshness = {
  tickSeconds: number;
  tickIndex: number;
  clockRevision: number;
  capturedAtEpochMs: number;
};

const CLOCK_FRESHNESS_EPSILON_S = 1e-6;

function finiteNumber(value: number | null | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function sameClock(a: SimulationClock | null, b: SimulationClock | null): boolean {
  if (!a && !b) return true;
  if (!a || !b) return false;
  return (
    a.running === b.running &&
    a.speed === b.speed &&
    Math.abs(a.tick_seconds - b.tick_seconds) < 1e-9
  );
}

export function clockFreshnessFromSnapshot(
  clock: SimulationClock | null | undefined,
  tickIndex: number | null | undefined,
  clockRevision: number | null | undefined,
  capturedAtEpochMs: number | null | undefined
): ClockFreshness | null {
  if (!clock || !Number.isFinite(clock.tick_seconds)) return null;
  return {
    tickSeconds: clock.tick_seconds,
    tickIndex: finiteNumber(tickIndex, 0),
    clockRevision: finiteNumber(clockRevision, 0),
    capturedAtEpochMs: finiteNumber(capturedAtEpochMs, 0),
  };
}

export function isNonDecreasingClockFreshness(
  next: ClockFreshness,
  previous: ClockFreshness
): boolean {
  if (next.tickSeconds > previous.tickSeconds + CLOCK_FRESHNESS_EPSILON_S) {
    return true;
  }
  if (next.tickSeconds + CLOCK_FRESHNESS_EPSILON_S < previous.tickSeconds) {
    return false;
  }
  if (next.tickIndex > previous.tickIndex) return true;
  if (next.tickIndex < previous.tickIndex) return false;
  if (next.clockRevision > previous.clockRevision) return true;
  if (next.clockRevision < previous.clockRevision) return false;
  return next.capturedAtEpochMs >= previous.capturedAtEpochMs;
}

export function sameEconomy(
  a: SimulationAdvanceEconomy | null,
  b: SimulationAdvanceEconomy | null
): boolean {
  if (!a && !b) return true;
  if (!a || !b) return false;
  return (
    Math.abs(a.current_balance_base - b.current_balance_base) < 1e-6 &&
    Math.abs(a.cumulative_revenue_base - b.cumulative_revenue_base) < 1e-6 &&
    Math.abs(a.cumulative_opex_base - b.cumulative_opex_base) < 1e-6 &&
    Math.abs(a.budget_display - b.budget_display) < 1e-6
  );
}

export function sameServiceLoads(a: Record<string, number>, b: Record<string, number>): boolean {
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  for (const key of aKeys) {
    if (!(key in b)) return false;
    if (Math.abs((a[key] ?? 0) - (b[key] ?? 0)) > 1e-6) return false;
  }
  return true;
}

export function sameRuntimeTrains(a: TrainRuntimeView[], b: TrainRuntimeView[]): boolean {
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index += 1) {
    const left = a[index];
    const right = b[index];
    if (left.train_id !== right.train_id) return false;
    if (left.service_id !== right.service_id) return false;
    if (left.line_id !== right.line_id) return false;
    if (left.destination_label !== right.destination_label) return false;
    if (left.vehicle_ordinal !== right.vehicle_ordinal) return false;
    if ((left.passenger_counter_provenance ?? left.provenance) !== (right.passenger_counter_provenance ?? right.provenance))
      return false;
    if (Math.abs((left.x ?? 0) - (right.x ?? 0)) > 1e-6) return false;
    if (Math.abs((left.y ?? 0) - (right.y ?? 0)) > 1e-6) return false;
    if (Math.abs((left.onboard_pax ?? 0) - (right.onboard_pax ?? 0)) > 1e-6) return false;
    if (left.in_motion !== right.in_motion) return false;
  }
  return true;
}

export function sameRuntimeStations(a: StationRuntimeView[], b: StationRuntimeView[]): boolean {
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index += 1) {
    const left = a[index];
    const right = b[index];
    if (left.stop_id !== right.stop_id) return false;
    if ((left.passenger_counter_provenance ?? left.provenance) !== (right.passenger_counter_provenance ?? right.provenance))
      return false;
    if (Math.abs((left.current_inside_pax ?? 0) - (right.current_inside_pax ?? 0)) > 1e-6)
      return false;
    if (Math.abs((left.declined_last_hour ?? 0) - (right.declined_last_hour ?? 0)) > 1e-6)
      return false;
    if (Math.abs((left.avg_wait_to_board_s ?? 0) - (right.avg_wait_to_board_s ?? 0)) > 1e-6)
      return false;
  }
  return true;
}

export function sameRuntimeLineOps(a: LineOpsRuntimeView[], b: LineOpsRuntimeView[]): boolean {
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index += 1) {
    const left = a[index];
    const right = b[index];
    if (left.line_id !== right.line_id) return false;
    if ((left.passenger_counter_provenance ?? left.provenance) !== (right.passenger_counter_provenance ?? right.provenance))
      return false;
    if ((left.active_trains ?? 0) !== (right.active_trains ?? 0)) return false;
    if (Math.abs((left.boarded_per_hour ?? 0) - (right.boarded_per_hour ?? 0)) > 1e-6)
      return false;
    if (Math.abs((left.denied_boardings_per_hour ?? 0) - (right.denied_boardings_per_hour ?? 0)) > 1e-6)
      return false;
    if (Math.abs((left.mean_wait_s ?? 0) - (right.mean_wait_s ?? 0)) > 1e-6) return false;
  }
  return true;
}
