import type { RuntimePerfTelemetry } from "../types";

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
        <span>Snapshot Age</span>
        <span>{fmt(props.snapshotLatencyMs, " ms")}</span>
        <span>Queue Depth</span>
        <span>{props.telemetry?.queue_depth?.toLocaleString() ?? "-"}</span>
        <span>Map Cost</span>
        <span>{props.mapComplexityScore.toLocaleString()}</span>
      </div>
    </aside>
  );
}
