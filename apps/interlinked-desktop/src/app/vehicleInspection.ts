import type { CounterProvenance, TrainRuntimeView } from "../types";

export type VehicleInspection = {
  vehicleId: string;
  vehicleOrdinal: number;
  serviceId: string;
  lineId: string;
  lineName: string;
  destinationLabel: string;
  mode: string;
  modeVariant: string | null;
  stockTierId: string | null;
  vehicleCapacity: number;
  passengersOnBoard: number;
  headwayS: number | null;
  lng: number | null;
  lat: number | null;
  displayColor: string | null;
  atStopId?: string | null;
  inMotion?: boolean | null;
  provenance?: CounterProvenance | string | null;
  passengerCounterProvenance?: CounterProvenance | string | null;
};

export function vehicleInspectionFromRuntimeTrain(train: TrainRuntimeView): VehicleInspection {
  return {
    vehicleId: train.train_id,
    vehicleOrdinal: Math.max(Math.round(train.vehicle_ordinal || 0), 1),
    serviceId: train.service_id,
    lineId: train.line_id,
    lineName: train.line_name,
    destinationLabel: train.destination_label || train.direction_label || "Outbound",
    mode: train.mode,
    modeVariant: train.mode_variant ?? null,
    stockTierId: train.stock_tier_id ?? null,
    vehicleCapacity: Math.max(train.vehicle_capacity ?? 0, 0),
    passengersOnBoard: Math.max(train.onboard_pax ?? 0, 0),
    headwayS: null,
    lng: null,
    lat: null,
    displayColor: null,
    atStopId: train.at_stop_id ?? null,
    inMotion: train.in_motion,
    provenance: train.provenance,
    passengerCounterProvenance: train.passenger_counter_provenance ?? train.provenance,
  };
}

export function mergeRuntimeVehicleInspection(
  selected: VehicleInspection | null,
  runtimeTrains: TrainRuntimeView[]
): VehicleInspection | null {
  if (!selected) return null;
  const runtime = runtimeTrains.find((train) => train.train_id === selected.vehicleId) ?? null;
  if (!runtime) return selected;
  return {
    ...selected,
    ...vehicleInspectionFromRuntimeTrain(runtime),
    lng: selected.lng,
    lat: selected.lat,
    displayColor: selected.displayColor,
    headwayS: selected.headwayS,
  };
}
