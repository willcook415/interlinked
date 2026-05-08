import type { VehicleInspection } from "../app/vehicleInspection";
import { formatCounterProvenance } from "../app/counterProvenance";
import { vehicleTypeLabel } from "../map/runtimeVehicleOverlay";
import InspectorPanel from "./InspectorPanel";

function formatPassengerCount(value: number): string {
  if (value >= 1) return Math.round(value).toLocaleString();
  if (value > 0) return "<1";
  return "0";
}

function formatHeadway(value: number | null): string {
  if (value === null || !Number.isFinite(value) || value <= 0) return "-";
  if (value >= 3600) return `${(value / 3600).toFixed(1)}h`;
  if (value >= 60) return `${Math.round(value / 60)} min`;
  return `${Math.round(value)}s`;
}

export default function TrainInspector(props: {
  vehicle: VehicleInspection;
  editable: boolean;
  onClose: () => void;
  onScrapVehicle: (vehicleId: string) => void;
}) {
  const vehicleLabel = `${vehicleTypeLabel(props.vehicle.mode)} #${props.vehicle.vehicleOrdinal}`;
  const loadPct =
    props.vehicle.vehicleCapacity > 0
      ? Math.max(0, props.vehicle.passengersOnBoard / props.vehicle.vehicleCapacity) * 100
      : null;
  const passengerProvenanceLabel = formatCounterProvenance(
    props.vehicle.passengerCounterProvenance ?? props.vehicle.provenance
  );

  return (
    <InspectorPanel
      variant="vehicle"
      eyebrow="Vehicle Inspector"
      title={vehicleLabel}
      status={props.vehicle.destinationLabel}
      onClose={props.onClose}
    >
      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Current Service</h5>
          <span>{props.vehicle.inMotion === false ? "At stop" : "In motion"}</span>
        </div>
        <div className="vehicle-inspector-line">{props.vehicle.lineName}</div>
        <div className="inspector-stat-row">
          <div className="inspector-stat">
            <small>On Board - {passengerProvenanceLabel}</small>
            <strong>{formatPassengerCount(props.vehicle.passengersOnBoard)} pax</strong>
          </div>
          <div className="inspector-stat">
            <small>Capacity</small>
            <strong>{Math.round(props.vehicle.vehicleCapacity).toLocaleString()} pax</strong>
          </div>
          <div className="inspector-stat">
            <small>Load</small>
            <strong>{loadPct === null ? "-" : `${loadPct.toFixed(0)}%`}</strong>
          </div>
          <div className="inspector-stat">
            <small>Headway</small>
            <strong>{formatHeadway(props.vehicle.headwayS)}</strong>
          </div>
        </div>
      </section>

      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Identity</h5>
          <span>{props.vehicle.stockTierId ?? "standard"}</span>
        </div>
        <div className="inspector-read-grid">
          <div className="inspector-read-field">
            <small>Vehicle ID</small>
            <strong>{props.vehicle.vehicleId}</strong>
          </div>
          <div className="inspector-read-field">
            <small>Service ID</small>
            <strong>{props.vehicle.serviceId}</strong>
          </div>
          <div className="inspector-read-field">
            <small>Line ID</small>
            <strong>{props.vehicle.lineId}</strong>
          </div>
          <div className="inspector-read-field">
            <small>Position</small>
            <strong>
              {props.vehicle.lng !== null && props.vehicle.lat !== null
                ? `${props.vehicle.lng.toFixed(5)}, ${props.vehicle.lat.toFixed(5)}`
                : props.vehicle.atStopId
                  ? `At ${props.vehicle.atStopId}`
                  : "-"}
            </strong>
          </div>
        </div>
      </section>

      {props.editable ? (
        <div className="inspector-actions">
          <button className="danger-button" onClick={() => props.onScrapVehicle(props.vehicle.vehicleId)}>
            Scrap Vehicle
          </button>
        </div>
      ) : null}
    </InspectorPanel>
  );
}
