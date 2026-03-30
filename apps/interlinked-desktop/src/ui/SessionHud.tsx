import { useEffect, useState } from "react";
import type { AlertItem, CurrencyCode, SessionKind, SimulationClock, SimulationSpeed } from "../types";

function fmtDateTime(baseIso: string, tickSeconds: number): string {
  const base = new Date(baseIso).getTime();
  const dt = new Date(base + Math.round(tickSeconds) * 1000);
  const dd = String(dt.getUTCDate()).padStart(2, "0");
  const mm = String(dt.getUTCMonth() + 1).padStart(2, "0");
  const yyyy = dt.getUTCFullYear();
  const hh = String(dt.getUTCHours()).padStart(2, "0");
  const min = String(dt.getUTCMinutes()).padStart(2, "0");
  const ss = String(dt.getUTCSeconds()).padStart(2, "0");
  return `${dd}/${mm}/${yyyy} ${hh}:${min}:${ss}`;
}

function formatMoneyCompact(value: number | null, currency: CurrencyCode): string {
  if (value === null || !Number.isFinite(value)) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

function formatMoneyExact(value: number | null, currency: CurrencyCode): string {
  if (value === null || !Number.isFinite(value)) return "-";
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

type FleetDeliveryItem = {
  id: string;
  orderId: string;
  label: string;
  lineId: string;
  lineName: string;
  status: string;
  etaAtTickS: number | null;
  focusVehicleId?: string | null;
};

export default function SessionHud(props: {
  sessionKind: SessionKind;
  projectName: string;
  clock: SimulationClock;
  budget: number | null;
  budgetCurrency: CurrencyCode;
  alerts: AlertItem[];
  alertsOpen: boolean;
  buildModeActive: boolean;
  buildTransportLabel?: string | null;
  menuOpen: boolean;
  fleetDeliveries: FleetDeliveryItem[];
  onMenuToggle: () => void;
  onOpenFinancialDashboard: () => void;
  onFocusLineFromFleet: (lineId: string) => void;
  onFocusVehicleFromFleet: (vehicleId: string) => void;
  onExpediteFleetDelivery: (delivery: FleetDeliveryItem) => Promise<void> | void;
  onSave: () => void;
  onSaveQuit: () => void;
  onOpenSettings: () => void;
  onOpenCommandPalette: () => void;
  onAlertsToggle: () => void;
  onToggleRunning: (running: boolean) => void;
  onSpeedChange: (speed: SimulationSpeed) => void;
}) {
  const [displayBudget, setDisplayBudget] = useState<number | null>(props.budget);
  const [displayTickS, setDisplayTickS] = useState(0);
  const [fleetMenuOpen, setFleetMenuOpen] = useState(false);
  const [expeditingDeliveryId, setExpeditingDeliveryId] = useState<string | null>(null);

  useEffect(() => {
    if (props.budget === null) {
      setDisplayBudget(null);
      return;
    }
    const nextBudget = props.budget;
    setDisplayBudget((prev) => {
      if (prev === null || !Number.isFinite(prev)) return nextBudget;
      const jitterFloor = Math.max(Math.abs(prev) * 0.00002, 500);
      return Math.abs(nextBudget - prev) < jitterFloor ? prev : nextBudget;
    });
  }, [props.budget]);

  useEffect(() => {
    setDisplayTickS(props.clock.tick_seconds);
  }, [props.clock.tick_seconds]);

  useEffect(() => {
    if (!props.clock.running) return;
    const step = Math.max(props.clock.speed, 1);
    const timer = window.setInterval(() => {
      setDisplayTickS((value) => value + step);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [props.clock.running, props.clock.speed]);

  const budgetLabel = formatMoneyCompact(displayBudget, props.budgetCurrency);
  const budgetExactLabel = formatMoneyExact(displayBudget, props.budgetCurrency);
  const running = props.clock.running;
  const runPauseLabel = running ? "Pause simulation" : "Start simulation";
  const criticalAlerts = props.alerts.filter((alert) => alert.severity === "critical").length;
  const pendingDeliveries = props.fleetDeliveries.filter((row) => {
    if (row.status.toLowerCase() !== "pending") return false;
    if (row.etaAtTickS === null) return true;
    return row.etaAtTickS > displayTickS + 0.5;
  });

  async function handleExpediteDelivery(delivery: FleetDeliveryItem) {
    if (!delivery.orderId || expeditingDeliveryId === delivery.id) return;
    setExpeditingDeliveryId(delivery.id);
    try {
      await props.onExpediteFleetDelivery(delivery);
    } finally {
      setExpeditingDeliveryId((current) => (current === delivery.id ? null : current));
    }
  }

  return (
    <>
      <header
        className={`session-hud ${props.buildModeActive ? "is-build" : ""} ${running ? "is-running" : "is-paused"}`}
      >
        <div className="hud-left">
          <p>{props.sessionKind === "game" ? "Open World Sandbox" : "Scenario Studio"}</p>
          <strong>{props.projectName}</strong>
          {props.buildModeActive ? (
            <span className="hud-build-tag">
              Build Mode{props.buildTransportLabel ? ` | ${props.buildTransportLabel}` : ""}
            </span>
          ) : null}
        </div>
        <div className="hud-center">
          <span className={`hud-state ${running ? "running" : "paused"}`}>{running ? "Running" : "Paused"}</span>
          <span className="hud-time">{fmtDateTime(props.clock.sim_datetime_utc, props.clock.tick_seconds)}</span>
          <button
            className="hud-play-pause-btn"
            disabled={props.buildModeActive}
            title={runPauseLabel}
            aria-label={runPauseLabel}
            onClick={() => props.onToggleRunning(!running)}
          >
            {running ? "⏸" : "▶"}
          </button>
          {[1, 2, 4].map((s) => (
            <button
              key={s}
              className={props.clock.speed === s ? "active" : ""}
              disabled={props.buildModeActive}
              onClick={() => props.onSpeedChange(s as SimulationSpeed)}
            >
              {s}x
            </button>
          ))}
        </div>
        <div className="hud-right">
          <button
            className="pill budget-pill"
            title={`Exact balance: ${budgetExactLabel}`}
            onClick={props.onOpenFinancialDashboard}
          >
            Budget: {budgetLabel}
          </button>
          <button
            className={`pill ${fleetMenuOpen ? "is-active" : ""}`}
            onClick={() => setFleetMenuOpen((value) => !value)}
            title="View active rolling stock deliveries"
          >
            Fleet: {pendingDeliveries.length}
          </button>
          <button className={`pill ${props.alertsOpen ? "is-active" : ""}`} onClick={props.onAlertsToggle}>
            Alerts: {props.alerts.length}
            {criticalAlerts > 0 ? ` (${criticalAlerts} critical)` : ""}
          </button>
          <button onClick={props.onMenuToggle}>Menu</button>
        </div>
      </header>
      {fleetMenuOpen && (
        <div className="fleet-menu">
          <strong>Delivery Queue</strong>
          {pendingDeliveries.length === 0 ? (
            <p>No pending deliveries.</p>
          ) : (
            pendingDeliveries.slice(0, 12).map((delivery) => {
              const remainingS =
                delivery.etaAtTickS === null ? null : Math.max(delivery.etaAtTickS - displayTickS, 0);
              const isExpediting = expeditingDeliveryId === delivery.id;
              return (
                <div key={delivery.id} className="fleet-menu-row">
                  <div>
                    <span>{delivery.label}</span>
                    <small>{delivery.lineName}</small>
                  </div>
                  <div className="fleet-menu-actions">
                    <small>{remainingS === null ? "ETA pending" : formatCountdown(remainingS)}</small>
                    <button disabled={isExpediting} onClick={() => void handleExpediteDelivery(delivery)}>
                      {isExpediting ? "Expediting..." : "Expedite"}
                    </button>
                    {delivery.focusVehicleId ? (
                      <button onClick={() => props.onFocusVehicleFromFleet(delivery.focusVehicleId!)}>Vehicle</button>
                    ) : null}
                    <button onClick={() => props.onFocusLineFromFleet(delivery.lineId)}>Line</button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      )}
      {props.menuOpen && (
        <div className="hud-menu">
          <button onClick={props.onSave}>Save</button>
          <button onClick={props.onSaveQuit}>Save &amp; Quit</button>
          <button onClick={props.onAlertsToggle}>Alerts Center</button>
          <button onClick={props.onOpenCommandPalette}>Command Palette</button>
          <button onClick={props.onOpenSettings}>Settings</button>
          <button onClick={props.onMenuToggle}>Back</button>
        </div>
      )}
    </>
  );
}
