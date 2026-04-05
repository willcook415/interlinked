import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { stopDisplayName } from "../build/helpers";
import type {
  AppRoute,
  AlertItem,
  LineOpsRuntimeView,
  ScenarioLite,
  SimulationClock,
  StationRuntimeView,
} from "../types";
import type { SessionBootState } from "./useSessionController";

const UI_SETTINGS_STORAGE_KEY = "interlinked.desktop.ui_settings.v1";
const ONBOARDING_STORAGE_KEY = "interlinked.desktop.onboarding.completed.v1";
const WINDOW_STATE_STORAGE_KEY = "interlinked.desktop.window_state.v1";
const WINDOW_MIN_WIDTH = 1280;
const WINDOW_MIN_HEIGHT = 720;

const BASE_ALERTS: AlertItem[] = [{ id: "a1", title: "No active disruptions", severity: "info" }];

const ONBOARDING_STEPS = [
  {
    id: "build_mode",
    title: "Enter Build Mode",
    description: "Open Build Mode from the left workspace panel to start planning your first route.",
  },
  {
    id: "first_line",
    title: "Create A Route",
    description: "Place stations and finish your first line, then apply changes to commit it.",
  },
  {
    id: "rolling_stock",
    title: "Order Rolling Stock",
    description: "Open Rolling Stock Editor from line inspector and place at least one vehicle order.",
  },
  {
    id: "start_service",
    title: "Start Service",
    description: "Press play to start simulation time and put your line into live operation.",
  },
] as const;

export type UiSettings = {
  uiScale: number;
  textScale: number;
  highContrast: boolean;
  reducedMotion: boolean;
  quietAlerts: boolean;
  showDiagnostics: boolean;
  masterVolume: number;
  uiVolume: number;
  gameplayVolume: number;
};

export const DEFAULT_UI_SETTINGS: UiSettings = {
  uiScale: 1,
  textScale: 1,
  highContrast: false,
  reducedMotion: false,
  quietAlerts: false,
  showDiagnostics: false,
  masterVolume: 0.75,
  uiVolume: 0.7,
  gameplayVolume: 0.7,
};

type WindowStateSnapshot = {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
};

type LineSummaryLike = {
  lineId: string;
  name: string;
  stockUnitsPending?: number | null;
  stockUnitsOwned?: number | null;
};

type UseShellStatusOrchestrationArgs = {
  route: AppRoute;
  sessionBootState: SessionBootState;
  bundleProjectId: string | null | undefined;
  clock: SimulationClock | null;
  workspaceMode: "view" | "build";
  activeScenario: ScenarioLite | null;
  lineSummaries: LineSummaryLike[];
  runtimeLineOps: LineOpsRuntimeView[];
  runtimeStations: StationRuntimeView[];
  currentBalanceBase: number | null;
  builderError: string | null;
  demandWarning: string | null;
  error: string | null;
  saveStatus: string;
  setSaveStatus: Dispatch<SetStateAction<string>>;
  setError: Dispatch<SetStateAction<string | null>>;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function finiteNumber(value: number | null | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function readUiSettings(): UiSettings {
  if (typeof window === "undefined") return DEFAULT_UI_SETTINGS;
  try {
    const raw = window.localStorage.getItem(UI_SETTINGS_STORAGE_KEY);
    if (!raw) return DEFAULT_UI_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<UiSettings> | null;
    if (!parsed) return DEFAULT_UI_SETTINGS;
    return {
      uiScale: clamp(Number(parsed.uiScale ?? DEFAULT_UI_SETTINGS.uiScale), 0.8, 1.25),
      textScale: clamp(Number(parsed.textScale ?? DEFAULT_UI_SETTINGS.textScale), 0.85, 1.3),
      highContrast: Boolean(parsed.highContrast),
      reducedMotion: Boolean(parsed.reducedMotion),
      quietAlerts: Boolean(parsed.quietAlerts),
      showDiagnostics: Boolean(parsed.showDiagnostics),
      masterVolume: clamp(Number(parsed.masterVolume ?? DEFAULT_UI_SETTINGS.masterVolume), 0, 1),
      uiVolume: clamp(Number(parsed.uiVolume ?? DEFAULT_UI_SETTINGS.uiVolume), 0, 1),
      gameplayVolume: clamp(Number(parsed.gameplayVolume ?? DEFAULT_UI_SETTINGS.gameplayVolume), 0, 1),
    };
  } catch {
    return DEFAULT_UI_SETTINGS;
  }
}

function readWindowState(): WindowStateSnapshot | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(WINDOW_STATE_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<WindowStateSnapshot> | null;
    if (!parsed) return null;
    const width = finiteNumber(parsed.width, NaN);
    const height = finiteNumber(parsed.height, NaN);
    const x = finiteNumber(parsed.x, NaN);
    const y = finiteNumber(parsed.y, NaN);
    if (!Number.isFinite(width) || !Number.isFinite(height) || !Number.isFinite(x) || !Number.isFinite(y)) {
      return null;
    }
    return {
      width: Math.max(Math.round(width), WINDOW_MIN_WIDTH),
      height: Math.max(Math.round(height), WINDOW_MIN_HEIGHT),
      x: Math.round(x),
      y: Math.round(y),
      maximized: Boolean(parsed.maximized),
    };
  } catch {
    return null;
  }
}

export function useShellStatusOrchestration(args: UseShellStatusOrchestrationArgs) {
  const [uiSettings, setUiSettings] = useState<UiSettings>(() => readUiSettings());
  const [dismissedAlertIds, setDismissedAlertIds] = useState<string[]>([]);
  const [fps, setFps] = useState<number | null>(null);
  const [frameMs, setFrameMs] = useState<number | null>(null);
  const [isOffline, setIsOffline] = useState<boolean>(() =>
    typeof navigator !== "undefined" ? !navigator.onLine : false
  );
  const [onboardingActive, setOnboardingActive] = useState(false);
  const [onboardingStep, setOnboardingStep] = useState(0);
  const audioContextRef = useRef<AudioContext | null>(null);
  const previousCriticalAlertCountRef = useRef(0);

  const playUiCue = useCallback(
    (kind: "confirm" | "error" | "toggle" | "alert") => {
      if (typeof window === "undefined") return;
      const volume = clamp(uiSettings.masterVolume * uiSettings.uiVolume, 0, 1);
      if (volume <= 0) return;
      const audioWindow = window as Window &
        typeof globalThis & {
          webkitAudioContext?: typeof AudioContext;
        };
      const Ctx = audioWindow.AudioContext ?? audioWindow.webkitAudioContext;
      if (!Ctx) return;
      const AudioCtor = Ctx as { new (): AudioContext };
      try {
        if (!audioContextRef.current) {
          audioContextRef.current = new AudioCtor();
        }
        const ctx = audioContextRef.current;
        if (!ctx) return;
        const now = ctx.currentTime;
        const oscillator = ctx.createOscillator();
        const gainNode = ctx.createGain();
        oscillator.connect(gainNode);
        gainNode.connect(ctx.destination);
        const config =
          kind === "error"
            ? { frequency: 180, type: "sawtooth" as OscillatorType, duration: 0.16, gain: 0.12 }
            : kind === "toggle"
              ? { frequency: 360, type: "triangle" as OscillatorType, duration: 0.09, gain: 0.08 }
              : kind === "alert"
                ? { frequency: 520, type: "sine" as OscillatorType, duration: 0.12, gain: 0.09 }
                : { frequency: 440, type: "sine" as OscillatorType, duration: 0.08, gain: 0.08 };
        oscillator.type = config.type;
        oscillator.frequency.setValueAtTime(config.frequency, now);
        gainNode.gain.setValueAtTime(0.0001, now);
        gainNode.gain.exponentialRampToValueAtTime(Math.max(volume * config.gain, 0.0001), now + 0.012);
        gainNode.gain.exponentialRampToValueAtTime(0.0001, now + config.duration);
        oscillator.start(now);
        oscillator.stop(now + config.duration + 0.02);
      } catch {
        // Audio feedback is optional and must never interrupt gameplay.
      }
    },
    [uiSettings.masterVolume, uiSettings.uiVolume]
  );

  const rawAlerts = useMemo(() => {
    const alerts: AlertItem[] = [];
    const inSession = args.route === "session_game" || args.route === "session_scenario";
    if (args.error?.trim()) {
      alerts.push({
        id: "runtime-error",
        title: "Runtime error",
        detail: args.error,
        severity: "critical",
      });
    }
    if (args.builderError?.trim()) {
      alerts.push({
        id: "build-warning",
        title: "Build validation warning",
        detail: args.builderError,
        severity: "warn",
      });
    }
    if (args.demandWarning?.trim()) {
      alerts.push({
        id: "demand-warning",
        title: "Demand data coverage warning",
        detail: args.demandWarning,
        severity: "warn",
      });
    }
    if (args.saveStatus.trim()) {
      alerts.push({
        id: "save-status",
        title: "Latest action",
        detail: args.saveStatus,
        severity: "info",
      });
    }
    if (inSession && args.clock && !args.clock.running) {
      alerts.push({
        id: "clock-paused",
        title: "Simulation paused",
        detail: "No services progress while paused.",
        severity: "info",
      });
    }
    if (args.currentBalanceBase !== null && args.currentBalanceBase < 0) {
      alerts.push({
        id: "budget-negative",
        title: "Budget is negative",
        detail: "Cut operating costs or raise fares to return to a positive balance.",
        severity: "critical",
      });
    }

    const lineNameById = new Map(
      args.lineSummaries.map((line) => [line.lineId, line.name.trim() ? line.name : "Untitled Line"])
    );
    const stressedLines = [...args.runtimeLineOps]
      .filter((line) => (line.denied_boardings_per_hour ?? 0) >= 30)
      .sort((left, right) => (right.denied_boardings_per_hour ?? 0) - (left.denied_boardings_per_hour ?? 0))
      .slice(0, 3);
    for (const line of stressedLines) {
      const denied = Math.round(line.denied_boardings_per_hour ?? 0);
      const lineName = lineNameById.get(line.line_id) ?? line.line_id;
      alerts.push({
        id: `line-denied:${line.line_id}`,
        title: `${lineName} is denying boardings`,
        detail: `${denied.toLocaleString()} denied boardings/hr. Increase service or capacity.`,
        severity: denied >= 120 ? "critical" : "warn",
        action_label: "Open line",
        target: { kind: "line", id: line.line_id },
      });
    }

    if (args.activeScenario) {
      const stopById = new Map(args.activeScenario.world.stops.map((stop) => [stop.id, stop]));
      const hotStations = [...args.runtimeStations]
        .map((station) => {
          const capacity = Math.max(station.capacity_pax ?? 0, 0);
          const ratio = capacity > 0 ? Math.max(station.current_inside_pax ?? 0, 0) / capacity : 0;
          return { station, ratio };
        })
        .filter((entry) => entry.ratio >= 0.9)
        .sort((left, right) => right.ratio - left.ratio)
        .slice(0, 2);
      for (const entry of hotStations) {
        const stop = stopById.get(entry.station.stop_id);
        const stopName = stopDisplayName(stop ?? { id: entry.station.stop_id, x: 0, y: 0 });
        alerts.push({
          id: `station-capacity:${entry.station.stop_id}`,
          title: `${stopName} nearing capacity`,
          detail: `${Math.round(entry.ratio * 100)}% full. Consider more service or station expansion.`,
          severity: entry.ratio >= 1 ? "critical" : "warn",
          action_label: "Open station",
          target: { kind: "stop", id: entry.station.stop_id },
        });
      }
    }

    if (alerts.length === 0) return BASE_ALERTS;
    return alerts.slice(0, 24);
  }, [
    args.activeScenario,
    args.builderError,
    args.clock,
    args.currentBalanceBase,
    args.demandWarning,
    args.error,
    args.lineSummaries,
    args.route,
    args.runtimeLineOps,
    args.runtimeStations,
    args.saveStatus,
  ]);

  const visibleAlerts = useMemo(() => {
    const filteredByDismiss = rawAlerts.filter((item) => !dismissedAlertIds.includes(item.id));
    if (!filteredByDismiss.length) return BASE_ALERTS;
    return uiSettings.quietAlerts
      ? filteredByDismiss.filter((item) => item.severity !== "info")
      : filteredByDismiss;
  }, [dismissedAlertIds, rawAlerts, uiSettings.quietAlerts]);

  const showSessionBootOverlay = useMemo(
    () =>
      (args.route === "session_game" || args.route === "session_scenario") &&
      args.sessionBootState.stage !== "idle" &&
      args.sessionBootState.stage !== "ready",
    [args.route, args.sessionBootState.stage]
  );

  const onboardingStepInfo =
    onboardingStep >= 0 && onboardingStep < ONBOARDING_STEPS.length
      ? ONBOARDING_STEPS[onboardingStep]
      : ONBOARDING_STEPS[0];

  const dismissAlert = useCallback(
    (alertId: string) => {
      setDismissedAlertIds((previous) => (previous.includes(alertId) ? previous : [...previous, alertId]));
      if (alertId === "runtime-error") {
        args.setError(null);
      }
      if (alertId === "save-status") {
        args.setSaveStatus("");
      }
    },
    [args]
  );

  const skipOnboarding = useCallback(() => {
    setOnboardingActive(false);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "done");
    }
  }, []);

  const advanceOnboardingStep = useCallback(() => {
    setOnboardingStep((previous) => Math.min(previous + 1, ONBOARDING_STEPS.length - 1));
  }, []);

  useEffect(() => {
    window.localStorage.setItem(UI_SETTINGS_STORAGE_KEY, JSON.stringify(uiSettings));
    const root = document.documentElement;
    root.style.setProperty("--ui-scale", uiSettings.uiScale.toFixed(2));
    root.style.setProperty("--text-scale", uiSettings.textScale.toFixed(2));
    root.classList.toggle("pref-high-contrast", uiSettings.highContrast);
    root.classList.toggle("pref-reduced-motion", uiSettings.reducedMotion);
  }, [uiSettings]);

  useEffect(() => {
    const markOnline = () => setIsOffline(false);
    const markOffline = () => setIsOffline(true);
    window.addEventListener("online", markOnline);
    window.addEventListener("offline", markOffline);
    return () => {
      window.removeEventListener("online", markOnline);
      window.removeEventListener("offline", markOffline);
    };
  }, []);

  useEffect(() => {
    const tauriRuntime = (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    if (!tauriRuntime) return;
    let disposed = false;
    let unlistenResize: (() => void) | null = null;
    let unlistenMove: (() => void) | null = null;
    let unlistenClose: (() => void) | null = null;

    const setup = async () => {
      try {
        const windowApi = await import("@tauri-apps/api/window");
        if (disposed) return;
        const currentWindow = windowApi.getCurrentWindow();
        await currentWindow.setMinSize(new windowApi.LogicalSize(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT));

        const restore = readWindowState();
        if (restore && !restore.maximized) {
          await currentWindow.setSize(new windowApi.LogicalSize(restore.width, restore.height));
          await currentWindow.setPosition(new windowApi.LogicalPosition(restore.x, restore.y));
        } else if (restore?.maximized) {
          await currentWindow.maximize();
        }

        const persistState = async () => {
          const [size, pos, maximized] = await Promise.all([
            currentWindow.innerSize(),
            currentWindow.outerPosition(),
            currentWindow.isMaximized(),
          ]);
          const snapshot: WindowStateSnapshot = {
            width: Math.max(Math.round(size.width), WINDOW_MIN_WIDTH),
            height: Math.max(Math.round(size.height), WINDOW_MIN_HEIGHT),
            x: Math.round(pos.x),
            y: Math.round(pos.y),
            maximized,
          };
          window.localStorage.setItem(WINDOW_STATE_STORAGE_KEY, JSON.stringify(snapshot));
        };

        unlistenResize = await currentWindow.onResized(() => {
          void persistState();
        });
        unlistenMove = await currentWindow.onMoved(() => {
          void persistState();
        });
        unlistenClose = await currentWindow.onCloseRequested(() => {
          void persistState();
        });
      } catch {
        // Ignore non-fatal desktop shell API issues.
      }
    };

    void setup();
    return () => {
      disposed = true;
      unlistenResize?.();
      unlistenMove?.();
      unlistenClose?.();
    };
  }, []);

  useEffect(() => {
    if (!uiSettings.showDiagnostics) {
      setFps(null);
      setFrameMs(null);
      return;
    }
    let raf = 0;
    let frameCount = 0;
    let lastFrame = performance.now();
    let lastSample = lastFrame;
    const tick = (now: number) => {
      frameCount += 1;
      const delta = now - lastFrame;
      lastFrame = now;
      if (now - lastSample >= 500) {
        const elapsed = Math.max(now - lastSample, 1);
        setFps((frameCount * 1000) / elapsed);
        setFrameMs(delta);
        frameCount = 0;
        lastSample = now;
      }
      raf = window.requestAnimationFrame(tick);
    };
    raf = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(raf);
  }, [uiSettings.showDiagnostics]);

  useEffect(() => {
    if (!args.saveStatus.trim()) return;
    const timer = window.setTimeout(() => {
      args.setSaveStatus("");
    }, 5500);
    return () => window.clearTimeout(timer);
  }, [args.saveStatus, args.setSaveStatus]);

  useEffect(() => {
    const activeIds = new Set(rawAlerts.map((alert) => alert.id));
    setDismissedAlertIds((previous) => previous.filter((id) => activeIds.has(id)));
  }, [rawAlerts]);

  useEffect(() => {
    const criticalCount = visibleAlerts.filter((alert) => alert.severity === "critical").length;
    if (criticalCount > previousCriticalAlertCountRef.current) {
      playUiCue("alert");
    }
    previousCriticalAlertCountRef.current = criticalCount;
  }, [playUiCue, visibleAlerts]);

  useEffect(() => {
    if (args.route !== "session_game") {
      setOnboardingActive(false);
      setOnboardingStep(0);
      return;
    }
    const completed =
      typeof window !== "undefined" &&
      window.localStorage.getItem(ONBOARDING_STORAGE_KEY) === "done";
    if (!completed) {
      setOnboardingActive(true);
      setOnboardingStep(0);
    }
  }, [args.bundleProjectId, args.route]);

  useEffect(() => {
    if (!onboardingActive || args.route !== "session_game") return;
    const hasCommittedLine = (args.activeScenario?.world.services.length ?? 0) > 0;
    const hasOrderedUnits = args.lineSummaries.some((line) => {
      const unitsPending = line.stockUnitsPending ?? 0;
      const unitsOwned = line.stockUnitsOwned ?? 0;
      return unitsPending > 0 || unitsOwned > 0;
    });
    if (onboardingStep === 0 && args.workspaceMode === "build") {
      setOnboardingStep(1);
      return;
    }
    if (onboardingStep === 1 && hasCommittedLine) {
      setOnboardingStep(2);
      return;
    }
    if (onboardingStep === 2 && hasOrderedUnits) {
      setOnboardingStep(3);
      return;
    }
    if (onboardingStep === 3 && args.clock?.running) {
      setOnboardingActive(false);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "done");
      }
    }
  }, [
    args.activeScenario?.world.services.length,
    args.clock?.running,
    args.lineSummaries,
    args.route,
    args.workspaceMode,
    onboardingActive,
    onboardingStep,
  ]);

  return {
    uiSettings,
    setUiSettings,
    fps,
    frameMs,
    isOffline,
    visibleAlerts,
    dismissAlert,
    playUiCue,
    showSessionBootOverlay,
    onboardingActive,
    onboardingStep,
    onboardingStepInfo,
    onboardingStepCount: ONBOARDING_STEPS.length,
    skipOnboarding,
    advanceOnboardingStep,
  };
}
