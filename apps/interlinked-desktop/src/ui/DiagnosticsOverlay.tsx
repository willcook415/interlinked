import type { RuntimePerfTelemetry, RuntimeTemporalDiagnostics, SimulationClock } from "../types";

function fmt(value: number | null | undefined, suffix: string): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "-";
  return `${value.toFixed(1)}${suffix}`;
}

export default function DiagnosticsOverlay(props: {
  open: boolean;
  fps: number | null;
  frameMs: number | null;
  telemetry: RuntimePerfTelemetry | null;
  snapshotLatencyMs: number | null;
  clock: SimulationClock;
  temporalDiagnostics: RuntimeTemporalDiagnostics;
  mapComplexityScore: number;
}) {
  if (!props.open) return null;

  return (
    <aside className="diagnostics-overlay">
      <div className="diagnostics-head">
        <strong>Diagnostics</strong>
      </div>
      <div className="diagnostics-grid">
        <span>FPS</span>
        <span>{props.fps === null ? "-" : Math.round(props.fps).toString()}</span>
        <span>Frame</span>
        <span>{fmt(props.frameMs, " ms")}</span>
        <span>Tick Total</span>
        <span>{fmt(props.telemetry?.tick_total_ms ?? null, " ms")}</span>
        <span>Tick Index</span>
        <span>{props.telemetry?.tick_index?.toLocaleString() ?? "-"}</span>
        <span>Fixed Step</span>
        <span>{fmt(props.telemetry?.fixed_step_s ?? null, " s")}</span>
        <span>Clock Tick</span>
        <span>{fmt(props.clock.tick_seconds, " s")}</span>
        <span>Sim UTC</span>
        <span>{props.clock.sim_datetime_utc || "-"}</span>
        <span>Speed</span>
        <span>{props.clock.speed}x</span>
        <span>Clock Revision</span>
        <span>{props.temporalDiagnostics.latest_fast_clock_revision.toLocaleString()}</span>
        <span>Fast Interval</span>
        <span>{fmt(props.temporalDiagnostics.last_fast_snapshot_interval_ms, " ms")}</span>
        <span>Stale Rejects</span>
        <span>{props.temporalDiagnostics.stale_fast_snapshots_rejected.toLocaleString()}</span>
        <span>Snapshot Age</span>
        <span>{fmt(props.snapshotLatencyMs, " ms")}</span>
        <span>Backlog Steps</span>
        <span>{props.telemetry?.backlog_steps?.toLocaleString() ?? "-"}</span>
        <span>Backlog</span>
        <span>{fmt(props.telemetry?.backlog_s ?? null, " s")}</span>
        <span>Queue Depth</span>
        <span>{props.telemetry?.queue_depth?.toLocaleString() ?? "-"}</span>
        <span>Map Cost</span>
        <span>{props.mapComplexityScore.toLocaleString()}</span>
      </div>
    </aside>
  );
}
