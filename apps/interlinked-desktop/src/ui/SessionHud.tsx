import { useEffect, useRef, useState } from "react";
import type { CurrencyCode, SessionKind, SimulationClock, SimulationSpeed } from "../types";

function simulationDate(baseIso: string, tickSeconds: number): Date {
  const base = new Date(baseIso).getTime();
  const wholeSeconds = Math.floor(Math.max(Number.isFinite(tickSeconds) ? tickSeconds : 0, 0) + 1e-6);
  return new Date(base + wholeSeconds * 1000);
}

function fmtDate(value: Date): string {
  const dd = String(value.getUTCDate()).padStart(2, "0");
  const month = value.toLocaleString(undefined, { month: "short", timeZone: "UTC" });
  const yyyy = value.getUTCFullYear();
  return `${dd} ${month} ${yyyy}`;
}

function fmtTime(value: Date): string {
  const hh = String(value.getUTCHours()).padStart(2, "0");
  const min = String(value.getUTCMinutes()).padStart(2, "0");
  const ss = String(value.getUTCSeconds()).padStart(2, "0");
  return `${hh}:${min}:${ss}`;
}

function formatMoneyExact(value: number | null, currency: CurrencyCode): string {
  if (value === null || !Number.isFinite(value)) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
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
  const [displayTickSeconds, setDisplayTickSeconds] = useState<number>(
    Math.max(Number.isFinite(props.clock.tick_seconds) ? props.clock.tick_seconds : 0, 0)
  );
  const displayTickRef = useRef(displayTickSeconds);
  const clockFrameRef = useRef<number | null>(null);
  const lastClockKeyframeRef = useRef<{ tickSeconds: number; receivedAtMs: number } | null>(null);
  const clockTransportIntervalMsRef = useRef(120);

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
    displayTickRef.current = displayTickSeconds;
  }, [displayTickSeconds]);

  useEffect(() => {
    const nowMs = performance.now();
    const rawTick = Number.isFinite(props.clock.tick_seconds) ? props.clock.tick_seconds : 0;
    const incomingTick = Math.max(rawTick, 0);
    const previousKeyframe = lastClockKeyframeRef.current;
    const monotonicTick = previousKeyframe
      ? Math.max(previousKeyframe.tickSeconds, incomingTick)
      : incomingTick;

    if (previousKeyframe && incomingTick + 1e-6 < previousKeyframe.tickSeconds) {
      console.warn("[runtime-temporal] backward hud clock keyframe rejected", {
        previousTickSeconds: previousKeyframe.tickSeconds,
        incomingTickSeconds: incomingTick,
      });
    }

    let durationMs = clockTransportIntervalMsRef.current;
    if (previousKeyframe) {
      const simDeltaSeconds = Math.max(monotonicTick - previousKeyframe.tickSeconds, 0);
      const speed = Math.max(props.clock.speed ?? 1, 1);
      const fromSimMs = simDeltaSeconds > 0 ? (simDeltaSeconds / speed) * 1000 : 0;
      const arrivalMs = Math.max(nowMs - previousKeyframe.receivedAtMs, 0);
      if (arrivalMs > 1) {
        clockTransportIntervalMsRef.current =
          clockTransportIntervalMsRef.current * 0.7 + arrivalMs * 0.3;
      }
      const transportMs = Math.max(arrivalMs, clockTransportIntervalMsRef.current);
      const baselineMs = Math.max(fromSimMs, transportMs);
      durationMs = Math.min(Math.max(baselineMs || 120, 24), 900);
    }

    lastClockKeyframeRef.current = {
      tickSeconds: monotonicTick,
      receivedAtMs: nowMs,
    };

    if (clockFrameRef.current !== null) {
      window.cancelAnimationFrame(clockFrameRef.current);
      clockFrameRef.current = null;
    }

    const startTick = displayTickRef.current;
    if (monotonicTick <= startTick + 1e-6 || durationMs <= 24) {
      setDisplayTickSeconds(monotonicTick);
      return;
    }

    const animate = (): void => {
      const elapsedMs = Math.max(performance.now() - nowMs, 0);
      const alpha = Math.min(Math.max(elapsedMs / durationMs, 0), 1);
      const nextTick = startTick + (monotonicTick - startTick) * alpha;
      setDisplayTickSeconds(nextTick);
      if (alpha < 1) {
        clockFrameRef.current = window.requestAnimationFrame(animate);
      } else {
        clockFrameRef.current = null;
      }
    };

    clockFrameRef.current = window.requestAnimationFrame(animate);
  }, [props.clock.speed, props.clock.tick_seconds]);

  useEffect(
    () => () => {
      if (clockFrameRef.current !== null) {
        window.cancelAnimationFrame(clockFrameRef.current);
        clockFrameRef.current = null;
      }
    },
    []
  );

  const budgetLabel = formatMoneyExact(displayBudget, props.budgetCurrency);
  const budgetExactLabel = formatMoneyExact(displayBudget, props.budgetCurrency);
  const running = props.clock.running;
  const timelineTickSeconds = displayTickSeconds;
  const displayDate = simulationDate(props.clock.sim_datetime_utc, timelineTickSeconds);
  const speedOptions: SimulationSpeed[] = [1, 2, 4];

  return (
    <>
      <header
        className={`session-hud ${props.buildModeActive ? "is-build" : ""} ${running ? "is-running" : "is-paused"}`}
      >
        <div className="hud-cluster hud-left" aria-label="Current save">
          <strong className="hud-save-name" title={props.projectName}>
            {props.projectName}
          </strong>
        </div>
        <div className="hud-cluster hud-center" aria-label="Simulation command controls">
          <div className="hud-clock" aria-label="Simulation date and time">
            <span className="hud-date">{fmtDate(displayDate)}</span>
            <span className="hud-time">{fmtTime(displayDate)}</span>
          </div>
          <div className="hud-transport-controls" role="group" aria-label="Simulation transport">
            <button
              className={`hud-transport-btn ${!running ? "is-active" : ""}`}
              disabled={props.buildModeActive}
              title="Pause simulation"
              aria-label="Pause simulation"
              aria-pressed={!running}
              onClick={() => props.onToggleRunning(false)}
            >
              <span className="hud-icon hud-icon-pause" aria-hidden="true" />
            </button>
            <button
              className={`hud-transport-btn ${running ? "is-active" : ""}`}
              disabled={props.buildModeActive}
              title="Start simulation"
              aria-label="Start simulation"
              aria-pressed={running}
              onClick={() => props.onToggleRunning(true)}
            >
              <span className="hud-icon hud-icon-play" aria-hidden="true" />
            </button>
          </div>
          <div className="hud-speed-controls" role="group" aria-label="Simulation speed">
            {speedOptions.map((speed) => (
              <button
                key={speed}
                className={props.clock.speed === speed ? "is-active" : ""}
                disabled={props.buildModeActive}
                aria-pressed={props.clock.speed === speed}
                onClick={() => props.onSpeedChange(speed)}
              >
                {speed}x
              </button>
            ))}
          </div>
        </div>
        <div className="hud-cluster hud-right" aria-label="Network status">
          <button
            className="hud-status-cell hud-budget"
            title={`Exact balance: ${budgetExactLabel}`}
            onClick={props.onOpenFinancialDashboard}
          >
            <span>Budget</span>
            <strong>{budgetLabel}</strong>
          </button>
          <button className="hud-menu-button" onClick={props.onMenuToggle} aria-expanded={props.menuOpen}>
            Menu
          </button>
        </div>
      </header>
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
