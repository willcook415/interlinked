import { useEffect, useMemo, useState } from "react";
import type { CurrencyCode, ModeBuildPreset, PurchaseOrderLite, SimulationSpeed } from "../types";
import type { LineFleetPatch } from "../build/helpers";

function formatMoney(value: number | null | undefined, currency: CurrencyCode): string {
  if (value === null || value === undefined) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
}

function formatCountdown(seconds: number): string {
  const safeSeconds = Math.max(Math.round(seconds), 0);
  const hours = Math.floor(safeSeconds / 3600);
  const minutes = Math.floor((safeSeconds % 3600) / 60);
  const secs = safeSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes.toString().padStart(2, "0")}m ${secs.toString().padStart(2, "0")}s`;
  if (minutes > 0) return `${minutes}m ${secs.toString().padStart(2, "0")}s`;
  return `${secs}s`;
}

function unitLabelForMode(modeId: string | null | undefined): string {
  const normalized = modeId?.toLowerCase() ?? "";
  if (normalized === "bus") return "Bus";
  if (normalized === "tram") return "Tram";
  if (normalized === "metro") return "Train";
  if (normalized === "ferry") return "Ferry";
  if (normalized === "rail") return "Train";
  return "Vehicle";
}

type QueueVehicle = {
  id: string;
  label: string;
  etaAtTickS: number | null;
  status: string;
};

export default function RollingStockEditorSheet(props: {
  open: boolean;
  editable: boolean;
  lineName: string;
  budgetCurrency: CurrencyCode;
  modeId?: string | null;
  preset: ModeBuildPreset | null;
  packageId: string;
  unitsOwned: number;
  unitsCommitted: number;
  unitsPending: number;
  unitsAssigned: number;
  carsPerUnit: number;
  speedLevel: string;
  comfortLevel: string;
  requiredUnitsNow: number;
  pendingOrders: PurchaseOrderLite[];
  activeVehicles: Array<{
    vehicleId: string;
    label: string;
    destinationLabel: string;
    onBoard: number;
    capacity: number;
  }>;
  currentTickS: number;
  clockRunning: boolean;
  clockSpeed: SimulationSpeed;
  onClose: () => void;
  onSave: (patch: LineFleetPatch) => void;
  onFocusVehicle: (vehicleId: string) => void;
}) {
  const [carsPerUnitDraft, setCarsPerUnitDraft] = useState(props.carsPerUnit);
  const [speedLevelDraft, setSpeedLevelDraft] = useState(props.speedLevel);
  const [comfortLevelDraft, setComfortLevelDraft] = useState(props.comfortLevel);
  const [orderMenuOpen, setOrderMenuOpen] = useState(false);
  const [orderNameDraft, setOrderNameDraft] = useState("");
  const [orderSpeedLevelDraft, setOrderSpeedLevelDraft] = useState(props.speedLevel);
  const [orderComfortLevelDraft, setOrderComfortLevelDraft] = useState(props.comfortLevel);
  const [orderCarsPerUnitDraft, setOrderCarsPerUnitDraft] = useState(props.carsPerUnit);
  const [displayTickS, setDisplayTickS] = useState(0);
  const preset = props.preset;
  const speedLevels = Array.isArray(preset?.speed_levels) ? preset.speed_levels : [];
  const comfortLevels = Array.isArray(preset?.comfort_levels) ? preset.comfort_levels : [];
  const packageOptions = preset
    ? (Array.isArray(preset.package_options) && preset.package_options.length
        ? preset.package_options
        : Array.isArray(preset.tiers)
          ? preset.tiers
          : [])
    : [];
  const leadTimeLabel =
    preset?.engine_mode.toLowerCase() === "bus"
      ? "4h"
      : preset?.engine_mode.toLowerCase() === "tram"
        ? "8h"
        : preset?.engine_mode.toLowerCase() === "metro"
          ? "12h"
          : preset?.engine_mode.toLowerCase() === "ferry"
            ? "14h"
            : "18h";

  useEffect(() => {
    if (!props.open) return;
    setCarsPerUnitDraft(props.carsPerUnit);
    setSpeedLevelDraft(props.speedLevel);
    setComfortLevelDraft(props.comfortLevel);
    setOrderSpeedLevelDraft(props.speedLevel);
    setOrderComfortLevelDraft(props.comfortLevel);
    setOrderCarsPerUnitDraft(props.carsPerUnit);
    setDisplayTickS(props.currentTickS);
  }, [
    props.carsPerUnit,
    props.comfortLevel,
    props.open,
    props.speedLevel,
  ]);

  const selectedPackage =
    packageOptions.find((item) => item.id.toLowerCase() === props.packageId.toLowerCase()) ??
    packageOptions[0] ??
    null;
  const selectedSpeed =
    speedLevels.find((item) => item.id.toLowerCase() === speedLevelDraft.toLowerCase()) ??
    speedLevels[0] ??
    null;
  const selectedComfort =
    comfortLevels.find((item) => item.id.toLowerCase() === comfortLevelDraft.toLowerCase()) ??
    comfortLevels[0] ??
    null;
  const selectedOrderSpeed =
    speedLevels.find((item) => item.id.toLowerCase() === orderSpeedLevelDraft.toLowerCase()) ??
    speedLevels[0] ??
    null;
  const selectedOrderComfort =
    comfortLevels.find((item) => item.id.toLowerCase() === orderComfortLevelDraft.toLowerCase()) ??
    comfortLevels[0] ??
    null;

  const normalizedCarsPerUnit = preset?.supports_carriages
    ? Math.min(Math.max(carsPerUnitDraft, preset.cars_min), preset.cars_max)
    : 1;
  const normalizedOrderCarsPerUnit = preset?.supports_carriages
    ? Math.min(Math.max(orderCarsPerUnitDraft, preset.cars_min), preset.cars_max)
    : 1;

  const computeUnitCostBase = (
    speedItem: { cost_multiplier: number } | null,
    comfortItem: { cost_multiplier: number } | null,
    carsPerUnit: number
  ): number => {
    if (!preset) return 0;
    const carsMultiplier = preset.supports_carriages
      ? Math.max(carsPerUnit / Math.max(preset.cars_default, 1), 0.5)
      : 1;
    return (
      preset.base_unit_purchase_cost_base *
      (selectedPackage?.purchase_cost_multiplier ?? 1) *
      (speedItem?.cost_multiplier ?? 1) *
      (comfortItem?.cost_multiplier ?? 1) *
      carsMultiplier
    );
  };

  const unitPurchaseCostBase = computeUnitCostBase(selectedSpeed, selectedComfort, normalizedCarsPerUnit);
  const orderUnitPurchaseCostBase = computeUnitCostBase(
    selectedOrderSpeed,
    selectedOrderComfort,
    normalizedOrderCarsPerUnit
  );
  const unitSalvageBase = unitPurchaseCostBase * (preset?.salvage_rate ?? 0);
  const fleetValueBase = props.unitsOwned * unitPurchaseCostBase;
  const speedText = selectedSpeed
    ? `Travel speed: x${selectedSpeed.speed_multiplier.toFixed(2)} | Operating cost: x${selectedSpeed.cost_multiplier.toFixed(2)}`
    : "Adjusts travel speed and operating cost";
  const comfortText = selectedComfort
    ? `Passenger appeal: x${selectedComfort.demand_multiplier.toFixed(2)} | Operating cost: x${selectedComfort.cost_multiplier.toFixed(2)}`
    : "Adjusts passenger appeal and operating cost";
  const canDecrementCars = preset ? normalizedCarsPerUnit > preset.cars_min : false;
  const canIncrementCars = preset ? normalizedCarsPerUnit < preset.cars_max : false;
  const canDecrementOrderCars = preset ? normalizedOrderCarsPerUnit > preset.cars_min : false;
  const canIncrementOrderCars = preset ? normalizedOrderCarsPerUnit < preset.cars_max : false;
  const projectedReadyUnits = props.unitsOwned;
  const projectedPendingUnits = props.unitsPending;
  const activeVehicleCount = props.activeVehicles.length > 0 ? props.activeVehicles.length : props.unitsAssigned;
  const unitLabel = unitLabelForMode(props.modeId ?? preset?.engine_mode);
  const nextUnitOrdinal = props.unitsCommitted + 1;
  const defaultOrderName = `${unitLabel} #${Math.max(nextUnitOrdinal, 1)}`;
  const dirty =
    normalizedCarsPerUnit !== props.carsPerUnit ||
    speedLevelDraft.toLowerCase() !== props.speedLevel.toLowerCase() ||
    comfortLevelDraft.toLowerCase() !== props.comfortLevel.toLowerCase();

  const queueVehicles = useMemo<QueueVehicle[]>(() => {
    const rows: QueueVehicle[] = [];
    let unitOrdinal = props.unitsOwned;
    const sortedOrders = [...(Array.isArray(props.pendingOrders) ? props.pendingOrders : [])].sort((left, right) => {
      const leftEta = typeof left.eta_at_tick_s === "number" ? left.eta_at_tick_s : Number.POSITIVE_INFINITY;
      const rightEta = typeof right.eta_at_tick_s === "number" ? right.eta_at_tick_s : Number.POSITIVE_INFINITY;
      return leftEta - rightEta;
    });
    for (const order of sortedOrders) {
      const units = Math.max(Math.round(order.units ?? 0), 0);
      if (units <= 0) continue;
      const status = typeof order.status === "string" ? order.status : "pending";
      for (let index = 0; index < units; index += 1) {
        unitOrdinal += 1;
        const explicitLabel = index === 0 ? order.label?.trim() ?? "" : "";
        rows.push({
          id: `${order.order_id}:${index}`,
          label: explicitLabel || `${unitLabel} #${unitOrdinal}`,
          etaAtTickS:
            typeof order.eta_at_tick_s === "number" && Number.isFinite(order.eta_at_tick_s)
              ? order.eta_at_tick_s
              : null,
          status,
        });
      }
    }
    return rows;
  }, [props.pendingOrders, props.unitsOwned, unitLabel]);

  useEffect(() => {
    if (!props.open) return;
    setDisplayTickS(props.currentTickS);
  }, [props.currentTickS, props.open]);

  useEffect(() => {
    if (!props.open || !props.clockRunning) return;
    const speed = Math.max(props.clockSpeed ?? 1, 1);
    const timer = window.setInterval(() => {
      setDisplayTickS((value) => value + speed);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [props.clockRunning, props.clockSpeed, props.open]);

  const handleBack = () => {
    if (props.editable && dirty) {
      const discard = window.confirm("You have unsaved rolling stock changes. Discard them?");
      if (!discard) return;
    }
    props.onClose();
  };

  const handleSaveSetupAndBack = (): void => {
    if (!dirty) {
      props.onClose();
      return;
    }
    props.onSave({
      cars_per_unit: normalizedCarsPerUnit,
      speed_level: selectedSpeed?.id ?? speedLevelDraft,
      comfort_level: selectedComfort?.id ?? comfortLevelDraft,
    });
    props.onClose();
  };

  const openOrderMenu = () => {
    setOrderMenuOpen(true);
    setOrderNameDraft(defaultOrderName);
    setOrderSpeedLevelDraft(selectedSpeed?.id ?? props.speedLevel);
    setOrderComfortLevelDraft(selectedComfort?.id ?? props.comfortLevel);
    setOrderCarsPerUnitDraft(normalizedCarsPerUnit);
  };

  const placeOrder = () => {
    const requiresProcurementConfirm = orderUnitPurchaseCostBase >= 2_000_000;
    const orderName = orderNameDraft.trim() || defaultOrderName;
    if (requiresProcurementConfirm) {
      const confirmed = window.confirm(
        `Confirm order: ${orderName} for ${formatMoney(orderUnitPurchaseCostBase, props.budgetCurrency)}.\nFunds are committed immediately.`
      );
      if (!confirmed) return;
    }
    props.onSave({
      units_owned: props.unitsCommitted + 1,
      cars_per_unit: normalizedOrderCarsPerUnit,
      speed_level: selectedOrderSpeed?.id ?? orderSpeedLevelDraft,
      comfort_level: selectedOrderComfort?.id ?? orderComfortLevelDraft,
      order_label: orderName,
    });
    setOrderMenuOpen(false);
    setOrderNameDraft("");
  };

  if (!props.open || !preset) return null;

  return (
    <aside className="editor-drawer-sheet">
      <div className="editor-drawer-head">
        <div>
          <p>Rolling Stock Editor</p>
          <h4>{props.lineName}</h4>
        </div>
        <button onClick={handleBack}>Back To Line</button>
      </div>

      <p className="hint-line">Order units individually, then manage defaults and delivery readiness from this panel.</p>

      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Procurement</h5>
          <span>{queueVehicles.length.toLocaleString()} in delivery queue</span>
        </div>
        {orderMenuOpen ? (
          <div className="rolling-order-card">
            <label>
              Unit Name
              <input
                disabled={!props.editable}
                value={orderNameDraft}
                placeholder={defaultOrderName}
                onChange={(event) => setOrderNameDraft(event.target.value)}
              />
            </label>
            <label>
              Speed Setup
              <select
                disabled={!props.editable}
                value={selectedOrderSpeed?.id ?? ""}
                onChange={(event) => setOrderSpeedLevelDraft(event.target.value)}
              >
                {speedLevels.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Comfort Setup
              <select
                disabled={!props.editable}
                value={selectedOrderComfort?.id ?? ""}
                onChange={(event) => setOrderComfortLevelDraft(event.target.value)}
              >
                {comfortLevels.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
            </label>
            {preset.supports_carriages ? (
              <label>
                Cars Per Train
                <div className="stepper-field">
                  <button
                    type="button"
                    className="stepper-btn"
                    disabled={!props.editable || !canDecrementOrderCars}
                    onClick={() => setOrderCarsPerUnitDraft((prev) => Math.max(prev - 1, preset.cars_min))}
                  >
                    -
                  </button>
                  <strong className="stepper-value">{normalizedOrderCarsPerUnit.toLocaleString()}</strong>
                  <button
                    type="button"
                    className="stepper-btn"
                    disabled={!props.editable || !canIncrementOrderCars}
                    onClick={() => setOrderCarsPerUnitDraft((prev) => Math.min(prev + 1, preset.cars_max))}
                  >
                    +
                  </button>
                </div>
              </label>
            ) : null}
            <div className="inspector-stat-row">
              <div className="inspector-stat">
                <small>Purchase Cost</small>
                <strong>{formatMoney(orderUnitPurchaseCostBase, props.budgetCurrency)}</strong>
              </div>
              <div className="inspector-stat">
                <small>Sell Value</small>
                <strong>{formatMoney(orderUnitPurchaseCostBase * preset.salvage_rate, props.budgetCurrency)}</strong>
              </div>
              <div className="inspector-stat">
                <small>Lead Time</small>
                <strong>{leadTimeLabel}</strong>
              </div>
            </div>
            <div className="rolling-order-actions">
              <button onClick={() => setOrderMenuOpen(false)}>Cancel</button>
              <button className="primary" disabled={!props.editable} onClick={placeOrder}>
                Put In Order
              </button>
            </div>
          </div>
        ) : (
          <button className="primary" disabled={!props.editable} onClick={openOrderMenu}>
            Order New {unitLabel}
          </button>
        )}
      </section>

      <div className="inspector-stat-row">
        <div className="inspector-stat">
          <small>Operating On Line</small>
          <strong>{activeVehicleCount.toLocaleString()}</strong>
        </div>
        <div className="inspector-stat">
          <small>Ready For Service</small>
          <strong>{projectedReadyUnits.toLocaleString()}</strong>
        </div>
        <div className="inspector-stat">
          <small>On Order</small>
          <strong>{projectedPendingUnits.toLocaleString()}</strong>
        </div>
        <div className="inspector-stat">
          <small>Fleet Value</small>
          <strong>{formatMoney(fleetValueBase, props.budgetCurrency)}</strong>
        </div>
        <div className="inspector-stat">
          <small>Sell Value / Unit</small>
          <strong>{formatMoney(unitSalvageBase, props.budgetCurrency)}</strong>
        </div>
        <div className="inspector-stat">
          <small>Status</small>
          <strong>
            {projectedReadyUnits >= props.requiredUnitsNow
              ? "Fleet covers current timetable"
              : `${(props.requiredUnitsNow - projectedReadyUnits).toLocaleString()} more needed`}
          </strong>
        </div>
      </div>

      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Line Vehicles</h5>
          <span>{props.activeVehicles.length.toLocaleString()} active now</span>
        </div>
        {props.activeVehicles.length ? (
          <div className="rolling-vehicle-list">
            {props.activeVehicles.map((vehicle) => (
              <button
                key={vehicle.vehicleId}
                className="rolling-vehicle-row"
                onClick={() => props.onFocusVehicle(vehicle.vehicleId)}
              >
                <div>
                  <strong>{vehicle.label}</strong>
                  <small>{vehicle.destinationLabel}</small>
                </div>
                <span>
                  {Math.round(vehicle.onBoard).toLocaleString()} / {Math.round(vehicle.capacity).toLocaleString()} pax
                </span>
              </button>
            ))}
          </div>
        ) : (
          <p className="hint-line">No active vehicles on the line right now. Start time or buy stock to begin service.</p>
        )}
      </section>

      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Delivery Queue</h5>
          <span>{queueVehicles.length.toLocaleString()} scheduled</span>
        </div>
        {queueVehicles.length ? (
          <div className="rolling-vehicle-list">
            {queueVehicles.map((vehicle) => {
              const remainingS =
                vehicle.etaAtTickS === null ? null : Math.max(vehicle.etaAtTickS - displayTickS, 0);
              return (
                <div key={vehicle.id} className="rolling-vehicle-row is-passive">
                  <div>
                    <strong>{vehicle.label}</strong>
                    <small>{vehicle.status}</small>
                  </div>
                  <span>{remainingS === null ? "ETA pending" : formatCountdown(remainingS)}</span>
                </div>
              );
            })}
          </div>
        ) : (
          <p className="hint-line">No deliveries queued.</p>
        )}
      </section>

      <section className="inspector-section">
        <div className="inspector-section-head">
          <h5>Default Setup</h5>
          <span>Applies to new and existing line stock</span>
        </div>
        <div className="inspector-grid">
          {preset.supports_carriages ? (
            <label>
              Cars Per Train
              <div className="stepper-field">
                <button
                  type="button"
                  className="stepper-btn"
                  disabled={!props.editable || !canDecrementCars}
                  onClick={() =>
                    setCarsPerUnitDraft((prev) =>
                      Math.max(prev - 1, preset.cars_min)
                    )
                  }
                >
                  -
                </button>
                <strong className="stepper-value">{normalizedCarsPerUnit.toLocaleString()}</strong>
                <button
                  type="button"
                  className="stepper-btn"
                  disabled={!props.editable || !canIncrementCars}
                  onClick={() =>
                    setCarsPerUnitDraft((prev) =>
                      Math.min(prev + 1, preset.cars_max)
                    )
                  }
                >
                  +
                </button>
              </div>
            </label>
          ) : null}
          <label>
            Speed Setup
            <select
              disabled={!props.editable}
              value={selectedSpeed?.id ?? ""}
              onChange={(event) => setSpeedLevelDraft(event.target.value)}
            >
              {speedLevels.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
            <span className="field-caption">{speedText}</span>
          </label>
          <label>
            Comfort Setup
            <select
              disabled={!props.editable}
              value={selectedComfort?.id ?? ""}
              onChange={(event) => setComfortLevelDraft(event.target.value)}
            >
              {comfortLevels.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.label}
                </option>
              ))}
            </select>
            <span className="field-caption">{comfortText}</span>
          </label>
        </div>
      </section>

      {props.editable ? (
        <div className="editor-drawer-footer">
          <button onClick={handleBack}>Back</button>
          <button className="primary" disabled={!dirty} onClick={handleSaveSetupAndBack}>
            Save Setup
          </button>
        </div>
      ) : null}
    </aside>
  );
}
