import type {
  AlertItem,
  LineOpsRuntimeView,
  StationRuntimeView,
  TrainRuntimeView,
} from "../types";
import { formatCounterProvenance } from "../app/counterProvenance";

function formatCompactCount(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "-";
  const rounded = Math.max(Math.round(value), 0);
  if (rounded < 1000) return String(rounded);
  return new Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: rounded < 10000 ? 1 : 0,
  }).format(rounded);
}

function formatDurationSeconds(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "-";
  const seconds = Math.max(Math.round(value), 0);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder > 0 ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function average(values: number[]): number | null {
  const finite = values.filter((value) => Number.isFinite(value) && value >= 0);
  if (!finite.length) return null;
  return finite.reduce((total, value) => total + value, 0) / finite.length;
}

export default function ViewSummaryBottomPanel(props: {
  currentRiders: number;
  runtimeTrains: TrainRuntimeView[];
  runtimeStations: StationRuntimeView[];
  runtimeLineOps: LineOpsRuntimeView[];
  alerts: AlertItem[];
  onOpenAlerts: () => void;
}) {
  const activeVehicles = props.runtimeTrains.length;
  const stationOccupancy = props.runtimeStations.reduce(
    (total, station) => total + Math.max(Math.round(station.current_inside_pax ?? 0), 0),
    0
  );
  const deniedBoardings = props.runtimeLineOps.reduce(
    (total, line) => total + Math.max(Math.round(line.denied_boardings_per_hour ?? 0), 0),
    0
  );
  const meanWait = average([
    ...props.runtimeLineOps.map((line) => line.mean_wait_s),
    ...props.runtimeStations.map((station) => station.avg_wait_to_board_s),
  ]);
  const criticalAlerts = props.alerts.filter((alert) => alert.severity === "critical").length;
  const firstAlerts = props.alerts.slice(0, 2);
  const runtimeProjectionLabel = formatCounterProvenance(
    props.runtimeStations[0]?.passenger_counter_provenance ??
      props.runtimeLineOps[0]?.passenger_counter_provenance ??
      "runtime_projection"
  );

  return (
    <section className="session-bottom-panel view-summary-bottom">
      <div className="bottom-panel-head">
        <div>
          <p>View Mode</p>
          <h3>Network Performance</h3>
        </div>
        <button className="bottom-panel-alert-button" onClick={props.onOpenAlerts}>
          Alerts {props.alerts.length}
        </button>
      </div>

      <div className="network-summary-grid">
        <span>
          <small>Riders now - {runtimeProjectionLabel}</small>
          <strong>{formatCompactCount(props.currentRiders)}</strong>
        </span>
        <span>
          <small>Vehicles active</small>
          <strong>{formatCompactCount(activeVehicles)}</strong>
        </span>
        <span>
          <small>Station load - {runtimeProjectionLabel}</small>
          <strong>{formatCompactCount(stationOccupancy)}</strong>
        </span>
        <span>
          <small>Mean wait - {runtimeProjectionLabel}</small>
          <strong>{formatDurationSeconds(meanWait)}</strong>
        </span>
        <span>
          <small>Denied / hr - {runtimeProjectionLabel}</small>
          <strong>{formatCompactCount(deniedBoardings)}</strong>
        </span>
        <span>
          <small>On-time</small>
          <strong>No signal</strong>
        </span>
        <span>
          <small>Satisfaction</small>
          <strong>No signal</strong>
        </span>
      </div>

      <div className="bottom-alert-strip">
        <div className="bottom-alert-summary">
          <small>Alert state</small>
          <strong>{criticalAlerts > 0 ? `${criticalAlerts} critical` : `${props.alerts.length} active`}</strong>
        </div>
        <div className="bottom-alert-list">
          {firstAlerts.length ? (
            firstAlerts.map((alert) => (
              <button key={alert.id} onClick={props.onOpenAlerts} className={`bottom-alert-item severity-${alert.severity}`}>
                <strong>{alert.title}</strong>
                {alert.detail ? <span>{alert.detail}</span> : null}
              </button>
            ))
          ) : (
            <span className="bottom-alert-empty">No active alerts.</span>
          )}
        </div>
      </div>
    </section>
  );
}
