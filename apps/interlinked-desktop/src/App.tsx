import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AlertItem,
  AppRoute,
  CityOption,
  CompareResult,
  CountryPackStatus,
  CountryOption,
  CurrencyCode,
  DemandCoverageMeta,
  DemandRebuildResult,
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  FleetDeliveryExpediteResult,
  DeletedSaveMeta,
  DeleteSaveResult,
  Difficulty,
  FarePolicyManifest,
  FocusResult,
  GameCreatePayload,
  GameSaveMeta,
  InstallResult,
  Mission,
  OpenSessionResult,
  PlanningRunConfig,
  MapRuntimeConfig,
  RegionStatus,
  RunMeta,
  PurgeSaveResult,
  RestoreSaveResult,
  SaveResult,
  ScenarioLite,
  ScenarioSaveMeta,
  SimulationAdvanceEconomy,
  SimulationClock,
  SimulationSpeed,
  RuntimeSnapshot,
  RuntimePerfTelemetry,
  LineOpsRuntimeView,
  StationRuntimeView,
  TrainRuntimeView,
  UnlockFocusResult,
  DifficultyProfile,
  UninstallResult,
} from "./types";
import {
  computeLocalLineDetail,
  computeLocalStationLines,
  getBuildPreset,
  serviceLineId,
  stopDisplayName,
} from "./build/helpers";
import { useBuildController } from "./build/useBuildController";
import { clockFreshnessFromSnapshot, sameClock, sameEconomy, sameRuntimeTrains, sameServiceLoads } from "./app/runtimeFreshness";
import { useRuntimePolling } from "./app/useRuntimePolling";
import HomeScreen from "./ui/HomeScreen";
import NewGameScreen from "./ui/NewGameScreen";
import LoadGameScreen from "./ui/LoadGameScreen";
import NewScenarioScreen from "./ui/NewScenarioScreen";
import LoadScenarioScreen from "./ui/LoadScenarioScreen";
import SessionHud from "./ui/SessionHud";
import SessionSideHud from "./ui/SessionSideHud";
import MapCanvas from "./ui/MapCanvas";
import BuildPalette from "./ui/BuildPalette";
import MapFiltersPanel, { type LinkModeFilter } from "./ui/MapFiltersPanel";
import MissionsDrawer from "./ui/MissionsDrawer";
import CountryInfoDrawer from "./ui/CountryInfoDrawer";
import LineInspectorSheet from "./ui/LineInspectorSheet";
import StationInspectorModal from "./ui/StationInspectorModal";
import RollingStockEditorSheet from "./ui/RollingStockEditorSheet";
import ScheduleEditorSheet from "./ui/ScheduleEditorSheet";
import FarePolicyPanel from "./ui/FarePolicyPanel";
import LineDeleteDialog from "./ui/LineDeleteDialog";
import SettingsPanel from "./ui/SettingsPanel";
import CommandPalette from "./ui/CommandPalette";
import DiagnosticsOverlay from "./ui/DiagnosticsOverlay";
import AlertsCenter from "./ui/AlertsCenter";
import FinancialDashboardModal from "./ui/FinancialDashboardModal";

const MISSIONS: Mission[] = [
  {
    id: "m1",
    title: "Starter Spine",
    description: "Build one frequent corridor connecting the main CBD to two outer districts.",
    status: "active",
  },
  {
    id: "m2",
    title: "Basic Reliability",
    description: "Maintain queue peak below 500 for 20 in-game minutes.",
    status: "active",
  },
  {
    id: "m3",
    title: "Cost Discipline",
    description: "Keep operating balance non-negative for one in-game hour.",
    status: "blocked",
  },
];

const BASE_ALERTS: AlertItem[] = [{ id: "a1", title: "No active disruptions", severity: "info" }];

type UiSettings = {
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

type CommandAction = {
  id: string;
  label: string;
  detail?: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
};

const UI_SETTINGS_STORAGE_KEY = "interlinked.desktop.ui_settings.v1";
const ONBOARDING_STORAGE_KEY = "interlinked.desktop.onboarding.completed.v1";
const WINDOW_STATE_STORAGE_KEY = "interlinked.desktop.window_state.v1";
const WINDOW_MIN_WIDTH = 1280;
const WINDOW_MIN_HEIGHT = 720;
const DEFAULT_UI_SETTINGS: UiSettings = {
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

type SessionBootState = {
  stage:
    | "idle"
    | "session_open"
    | "map_runtime_config"
    | "map_style"
    | "map_context"
    | "snapshot"
    | "ready"
    | "error";
  progress: number;
  message: string;
  error: string | null;
};

type MapBootProgressPayload = {
  stage: "map_style" | "map_context" | "ready" | "error";
  progress: number;
  message: string;
  error?: string | null;
};

const SESSION_BOOT_STAGE_RANK: Record<SessionBootState["stage"], number> = {
  idle: 0,
  session_open: 1,
  map_runtime_config: 2,
  map_style: 3,
  map_context: 4,
  snapshot: 5,
  ready: 6,
  error: 7,
};

const DIFFICULTY_PROFILES: Record<Difficulty, DifficultyProfile> = {
  easy: {
    profile_id: "easy",
    demand_mult: 0.85,
    capex_mult: 0.9,
    opex_mult: 0.92,
    maintenance_mult: 0.85,
    penalty_mult: 0.85,
    ancillary_revenue_mult: 1.08,
    unlock_cost_mult: 0.9,
  },
  standard: {
    profile_id: "standard",
    demand_mult: 1.0,
    capex_mult: 1.0,
    opex_mult: 1.0,
    maintenance_mult: 1.0,
    penalty_mult: 1.0,
    ancillary_revenue_mult: 1.0,
    unlock_cost_mult: 1.0,
  },
  hard: {
    profile_id: "hard",
    demand_mult: 1.2,
    capex_mult: 1.15,
    opex_mult: 1.18,
    maintenance_mult: 1.25,
    penalty_mult: 1.25,
    ancillary_revenue_mult: 0.92,
    unlock_cost_mult: 1.15,
  },
};

type WindowStateSnapshot = {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
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

function isTextInputLike(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  return (
    tag === "input" ||
    tag === "textarea" ||
    tag === "select" ||
    target.isContentEditable
  );
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
    if (
      !Number.isFinite(width) ||
      !Number.isFinite(height) ||
      !Number.isFinite(x) ||
      !Number.isFinite(y)
    ) {
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

function defaultBudgetFor(difficulty: Difficulty, currency: CurrencyCode): number {
  const baseGbp =
    difficulty === "easy" ? 3_000_000_000 : difficulty === "hard" ? 750_000_000 : 1_500_000_000;
  const fxFromGbp: Record<CurrencyCode, number> = {
    GBP: 1,
    USD: 1 / 0.79,
    EUR: 1 / 0.86,
  };
  return Math.round(baseGbp * fxFromGbp[currency]);
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

function distanceMetersXY(
  a: { x: number; y: number } | null | undefined,
  b: { x: number; y: number } | null | undefined
): number {
  if (!a || !b) return 0;
  const dx = a.x - b.x;
  const dy = a.y - b.y;
  return Math.sqrt(dx * dx + dy * dy);
}

function formatBackendError(error: unknown): string {
  const raw = String(error ?? "Unknown error");
  const message = raw.replace(/^Error:\s*/i, "");
  if (message.includes("RegionNotAdjacent")) {
    return "Unlock failed: the selected county must border one of your unlocked counties.";
  }
  if (message.includes("InsufficientFunds")) {
    return "Unlock failed: insufficient funds for this county.";
  }
  if (message.includes("WrongCountryScope")) {
    return "Unlock failed: this county is outside your current country scope.";
  }
  if (message.includes("CountryPackMissing")) {
    return "Unlock failed: required country data is not installed.";
  }
  return message;
}

export default function App() {
  const [route, setRoute] = useState<AppRoute>("home");
  const [bundle, setBundle] = useState<OpenSessionResult | null>(null);
  const [clock, setClock] = useState<SimulationClock | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [demandWarning, setDemandWarning] = useState<string | null>(null);
  const [saveStatus, setSaveStatus] = useState("");
  const [gameSaves, setGameSaves] = useState<GameSaveMeta[]>([]);
  const [scenarioSaves, setScenarioSaves] = useState<ScenarioSaveMeta[]>([]);
  const [deletedSaves, setDeletedSaves] = useState<DeletedSaveMeta[]>([]);
  const [countries, setCountries] = useState<CountryOption[]>([]);
  const [countryPacks, setCountryPacks] = useState<CountryPackStatus[]>([]);
  const [cities, setCities] = useState<CityOption[]>([]);
  const [demandCoverage, setDemandCoverage] = useState<DemandCoverageMeta[]>([]);
  const [regions, setRegions] = useState<RegionStatus[]>([]);
  const [focusRegionId, setFocusRegionId] = useState<string | null>(null);
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(null);
  const [showCountryInfo, setShowCountryInfo] = useState(false);
  const [showFares, setShowFares] = useState(false);
  const [mapRuntimeConfig, setMapRuntimeConfig] = useState<MapRuntimeConfig | null>(null);
  const [farePolicy, setFarePolicy] = useState<FarePolicyManifest | null>(null);
  const [liveEconomy, setLiveEconomy] = useState<SimulationAdvanceEconomy | null>(null);
  const [serviceLoadByServiceId, setServiceLoadByServiceId] = useState<Record<string, number>>({});
  const [runtimeTrains, setRuntimeTrains] = useState<TrainRuntimeView[]>([]);
  const [trainsAuthoritative, setTrainsAuthoritative] = useState(false);
  const latestClockTickRef = useRef(0);
  const latestSnapshotTickRef = useRef(0);
  const latestSnapshotCapturedRef = useRef(0);
  const latestStrategicSnapshotTickRef = useRef(0);
  const latestStrategicSnapshotCapturedRef = useRef(0);
  const runtimeControlQueueRef = useRef<Promise<void>>(Promise.resolve());
  const audioContextRef = useRef<AudioContext | null>(null);
  const previousCriticalAlertCountRef = useRef(0);

  const [newGameStep, setNewGameStep] = useState<1 | 2 | 3 | 4>(1);
  const [newGameName, setNewGameName] = useState("Interlinked World");
  const [newGameIntent, setNewGameIntent] = useState("balanced");
  const [newGameDifficulty, setNewGameDifficulty] = useState<Difficulty>("standard");
  const [newGameCurrency, setNewGameCurrency] = useState<CurrencyCode>("GBP");
  const [newGameBudget, setNewGameBudget] = useState("1500000000");
  const [selectedCountryIso2, setSelectedCountryIso2] = useState("");
  const [citySearch, setCitySearch] = useState("");
  const [selectedCityId, setSelectedCityId] = useState<number | null>(null);
  const [scenarioName, setScenarioName] = useState("Interlinked Scenario");

  const [showShapeStops, setShowShapeStops] = useState(false);
  const [showZoneCentroids, setShowZoneCentroids] = useState(false);
  const [showStations, setShowStations] = useState(true);
  const [showLinks, setShowLinks] = useState(true);
  const [linkMode, setLinkMode] = useState<LinkModeFilter>("all");
  const [showFilters, setShowFilters] = useState(false);
  const [showMissions, setShowMissions] = useState(false);
  const [showAlerts, setShowAlerts] = useState(false);
  const [showMenu, setShowMenu] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showFinancialDashboard, setShowFinancialDashboard] = useState(false);
  const [financialRequest, setFinancialRequest] = useState<FinancialDashboardRequest>({
    granularity: "month",
    periods: 12,
  });
  const [financialData, setFinancialData] = useState<FinancialDashboardResponse | null>(null);
  const [financialBusy, setFinancialBusy] = useState(false);
  const [financialError, setFinancialError] = useState<string | null>(null);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [commandPaletteQuery, setCommandPaletteQuery] = useState("");
  const [uiSettings, setUiSettings] = useState<UiSettings>(() => readUiSettings());
  const [budgetEdited, setBudgetEdited] = useState(false);
  const [rollingStockEditorOpen, setRollingStockEditorOpen] = useState(false);
  const [scheduleEditorOpen, setScheduleEditorOpen] = useState(false);
  const [focusStopRequest, setFocusStopRequest] = useState<{ stopId: string; token: number } | null>(null);
  const [focusVehicleRequest, setFocusVehicleRequest] = useState<{ vehicleId: string; token: number } | null>(null);
  const [lineDeleteDialogOpen, setLineDeleteDialogOpen] = useState(false);
  const [runtimeTelemetry, setRuntimeTelemetry] = useState<RuntimePerfTelemetry | null>(null);
  const [snapshotLatencyMs, setSnapshotLatencyMs] = useState<number | null>(null);
  const [runtimeStations, setRuntimeStations] = useState<StationRuntimeView[]>([]);
  const [runtimeLineOps, setRuntimeLineOps] = useState<LineOpsRuntimeView[]>([]);
  const [dismissedAlertIds, setDismissedAlertIds] = useState<string[]>([]);
  const [fps, setFps] = useState<number | null>(null);
  const [frameMs, setFrameMs] = useState<number | null>(null);
  const [isOffline, setIsOffline] = useState<boolean>(() =>
    typeof navigator !== "undefined" ? !navigator.onLine : false
  );
  const [onboardingActive, setOnboardingActive] = useState(false);
  const [onboardingStep, setOnboardingStep] = useState(0);
  const [mapInstanceToken, setMapInstanceToken] = useState(0);
  const [sessionBootState, setSessionBootState] = useState<SessionBootState>({
    stage: "idle",
    progress: 0,
    message: "",
    error: null,
  });

  const [runConfig, setRunConfig] = useState<PlanningRunConfig>({
    deterministic_seed: 42,
    horizon_s: 3600,
    time_bin_s: 300,
    time_of_day_s: 28800,
  });
  const [compareResult, setCompareResult] = useState<CompareResult | null>(null);
  const [selectedBaseRun, setSelectedBaseRun] = useState("");
  const [selectedCandidateRun, setSelectedCandidateRun] = useState("");
  const scenario = (bundle?.scenario?.scenario ?? null) as ScenarioLite | null;
  const sessionKind = bundle?.manifest.session_kind ?? null;
  const build = useBuildController({
    bundle,
    clock,
    sessionKind,
    setBundle,
    setClock,
    setSaveStatus,
  });
  const activeScenario = build.workingScenario ?? scenario;
  const canContinue = gameSaves.length > 0;
  const latestGameSave = gameSaves[0] ?? null;
  const budgetCurrency =
    bundle?.manifest.progress_metrics?.currency ??
    bundle?.manifest.economy?.currency ??
    "GBP";
  const selectedCity = useMemo(
    () => cities.find((c) => c.geonameid === selectedCityId) ?? null,
    [cities, selectedCityId]
  );
  const selectedCountry = useMemo(
    () => countries.find((c) => c.iso2 === selectedCountryIso2) ?? null,
    [countries, selectedCountryIso2]
  );
  const selectedCountryPack = useMemo(
    () => countryPacks.find((p) => p.country_iso2 === selectedCountryIso2) ?? null,
    [countryPacks, selectedCountryIso2]
  );
  const selectedDifficultyProfile = useMemo(
    () => DIFFICULTY_PROFILES[newGameDifficulty],
    [newGameDifficulty]
  );
  const activeRegionIds = useMemo(
    () => bundle?.manifest.region_state?.active_region_ids ?? [],
    [bundle?.manifest.region_state?.active_region_ids]
  );
  const currentBalanceBase =
    liveEconomy?.current_balance_base ?? bundle?.manifest.economy?.current_balance_base ?? null;
  const rawAlerts = useMemo(() => {
    const alerts: AlertItem[] = [];
    const inSession = route === "session_game" || route === "session_scenario";
    if (error?.trim()) {
      alerts.push({
        id: "runtime-error",
        title: "Runtime error",
        detail: error,
        severity: "critical",
      });
    }
    if (build.builderError?.trim()) {
      alerts.push({
        id: "build-warning",
        title: "Build validation warning",
        detail: build.builderError,
        severity: "warn",
      });
    }
    if (demandWarning?.trim()) {
      alerts.push({
        id: "demand-warning",
        title: "Demand data coverage warning",
        detail: demandWarning,
        severity: "warn",
      });
    }
    if (saveStatus.trim()) {
      alerts.push({
        id: "save-status",
        title: "Latest action",
        detail: saveStatus,
        severity: "info",
      });
    }
    if (inSession && clock && !clock.running) {
      alerts.push({
        id: "clock-paused",
        title: "Simulation paused",
        detail: "No services progress while paused.",
        severity: "info",
      });
    }
    if (currentBalanceBase !== null && currentBalanceBase < 0) {
      alerts.push({
        id: "budget-negative",
        title: "Budget is negative",
        detail: "Cut operating costs or raise fares to return to a positive balance.",
        severity: "critical",
      });
    }

    const lineNameById = new Map(
      build.lineSummaries.map((line) => [line.lineId, line.name.trim() ? line.name : "Untitled Line"])
    );
    const stressedLines = [...runtimeLineOps]
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

    if (activeScenario) {
      const stopById = new Map(activeScenario.world.stops.map((stop) => [stop.id, stop]));
      const hotStations = [...runtimeStations]
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
    activeScenario,
    build.builderError,
    build.lineSummaries,
    clock,
    currentBalanceBase,
    demandWarning,
    error,
    route,
    runtimeLineOps,
    runtimeStations,
    saveStatus,
  ]);
  const visibleAlerts = useMemo(() => {
    const filteredByDismiss = rawAlerts.filter((item) => !dismissedAlertIds.includes(item.id));
    if (!filteredByDismiss.length) return BASE_ALERTS;
    return uiSettings.quietAlerts
      ? filteredByDismiss.filter((item) => item.severity !== "info")
      : filteredByDismiss;
  }, [dismissedAlertIds, rawAlerts, uiSettings.quietAlerts]);
  useEffect(() => {
    const criticalCount = visibleAlerts.filter((alert) => alert.severity === "critical").length;
    if (criticalCount > previousCriticalAlertCountRef.current) {
      playUiCue("alert");
    }
    previousCriticalAlertCountRef.current = criticalCount;
  }, [visibleAlerts]);
  const visibleCountryIso2 = useMemo(() => {
    if (!bundle || sessionKind !== "game") return null;
    const loaded = demandCoverage
      .filter((c) => c.loaded_in_scenario)
      .map((c) => c.country_iso2.trim().toUpperCase())
      .filter((c) => c.length === 2);
    if (loaded.length > 0) {
      return Array.from(new Set(loaded));
    }
    const merged = new Set<string>();
    for (const c of bundle.manifest.economy?.unlocked_countries ?? []) {
      const code = c.trim().toUpperCase();
      if (code.length === 2) merged.add(code);
    }
    const startCode = bundle.manifest.start_location?.country_iso2?.trim().toUpperCase();
    if (startCode && startCode.length === 2) merged.add(startCode);
    return merged.size ? Array.from(merged) : null;
  }, [bundle, demandCoverage, sessionKind]);
  const filteredCities = useMemo(() => {
    const q = citySearch.trim().toLowerCase();
    if (!q) return cities;
    return cities.filter((c) => c.name.toLowerCase().includes(q));
  }, [cities, citySearch]);
  const selectedLineServices = useMemo(() => {
    if (!activeScenario || build.selection?.kind !== "line") return [];
    const lineId = build.selection.lineId;
    return activeScenario.world.services.filter((service) => serviceLineId(service) === lineId);
  }, [activeScenario, build.selection]);
  const selectedLinePresetId = useMemo(() => {
    if (!build.buildDefaults || selectedLineServices.length === 0) return null;
    const sample = selectedLineServices[0];
    const exact = build.buildDefaults.presets.find(
      (preset) =>
        preset.engine_mode === sample.mode &&
        (preset.mode_variant ?? null) === (sample.mode_variant ?? null)
    );
    if (exact) return exact.id;
    return build.buildDefaults.presets.find((preset) => preset.engine_mode === sample.mode)?.id ?? null;
  }, [build.buildDefaults, selectedLineServices]);
  const selectedLineDetail = useMemo(() => {
    if (!activeScenario || build.selection?.kind !== "line") return null;
    return computeLocalLineDetail(activeScenario, build.selection.lineId);
  }, [activeScenario, build.selection]);
  const selectedBaseLineDetail = useMemo(() => {
    if (!scenario || build.selection?.kind !== "line") return null;
    return computeLocalLineDetail(scenario, build.selection.lineId);
  }, [scenario, build.selection]);
  const selectedLineBuildPreset = useMemo(
    () => getBuildPreset(build.buildDefaults, selectedLinePresetId),
    [build.buildDefaults, selectedLinePresetId]
  );
  const stationLineIndex = useMemo(() => {
    const index = new Map<string, Array<{ lineId: string; lineName: string; displayColor?: string | null }>>();
    for (const line of build.lineSummaries) {
      for (const stopId of line.stationIds) {
        const bucket = index.get(stopId);
        const item = {
          lineId: line.lineId,
          lineName: line.name.trim() ? line.name : "Untitled Line",
          displayColor: line.displayColor ?? null,
        };
        if (bucket) bucket.push(item);
        else index.set(stopId, [item]);
      }
    }
    return index;
  }, [build.lineSummaries]);
  const selectedStationLines = useMemo(() => {
    if (!activeScenario || build.selection?.kind !== "stop") return [];
    return computeLocalStationLines(activeScenario, build.selection.stopId);
  }, [activeScenario, build.selection]);
  const selectedStationInterchangeContext = useMemo(() => {
    if (!activeScenario || build.selection?.kind !== "stop") {
      return {
        members: [] as Array<{ stopId: string; name: string; distanceM: number }>,
        suggestions: [] as Array<{ interchangeId: string; memberCount: number; nearestDistanceM: number }>,
        transfers: [] as Array<{
          stopId: string;
          name: string;
          distanceM: number;
          transferTimeS: number;
          penaltyS: number;
          direction: "to" | "from" | "both";
        }>,
      };
    }
    const stops = activeScenario.world.stops.filter(
      (stop) => !String(stop.stop_type ?? "").toLowerCase().includes("shape")
    );
    const selectedStopId = build.selection.kind === "stop" ? build.selection.stopId : null;
    const selectedStop = stops.find((stop) => stop.id === selectedStopId) ?? null;
    if (!selectedStop) {
      return {
        members: [] as Array<{ stopId: string; name: string; distanceM: number }>,
        suggestions: [] as Array<{ interchangeId: string; memberCount: number; nearestDistanceM: number }>,
        transfers: [] as Array<{
          stopId: string;
          name: string;
          distanceM: number;
          transferTimeS: number;
          penaltyS: number;
          direction: "to" | "from" | "both";
        }>,
      };
    }
    const selectedGroup = selectedStop.interchange_id?.trim() || "";
    const members = selectedGroup
      ? stops
          .filter((stop) => stop.id !== selectedStop.id && stop.interchange_id?.trim() === selectedGroup)
          .map((stop) => ({
            stopId: stop.id,
            name: stopDisplayName(stop),
            distanceM: distanceMetersXY(stop, selectedStop),
          }))
          .sort((left, right) => left.distanceM - right.distanceM)
      : [];

    const suggestionMap = new Map<string, { interchangeId: string; memberCount: number; nearestDistanceM: number }>();
    for (const stop of stops) {
      if (stop.id === selectedStop.id) continue;
      const interchangeId = stop.interchange_id?.trim();
      if (!interchangeId || interchangeId === selectedGroup) continue;
      const distanceM = distanceMetersXY(stop, selectedStop);
      if (!(distanceM <= 420)) continue;
      const current = suggestionMap.get(interchangeId);
      if (!current) {
        suggestionMap.set(interchangeId, { interchangeId, memberCount: 1, nearestDistanceM: distanceM });
        continue;
      }
      current.memberCount += 1;
      current.nearestDistanceM = Math.min(current.nearestDistanceM, distanceM);
    }
    const suggestions = [...suggestionMap.values()]
      .sort((left, right) => left.nearestDistanceM - right.nearestDistanceM)
      .slice(0, 3);

    const stopById = new Map(stops.map((stop) => [stop.id, stop]));
    const transferMap = new Map<
      string,
      {
        stopId: string;
        transferOutS: number | null;
        transferInS: number | null;
        penaltyOutS: number | null;
        penaltyInS: number | null;
      }
    >();
    for (const transfer of activeScenario.world.transfers) {
      const isOut = transfer.from_stop === selectedStop.id;
      const isIn = transfer.to_stop === selectedStop.id;
      if (!isOut && !isIn) continue;
      const otherStopId = isOut ? transfer.to_stop : transfer.from_stop;
      const otherStop = stopById.get(otherStopId);
      if (!otherStop) continue;
      const row = transferMap.get(otherStopId) ?? {
        stopId: otherStopId,
        transferOutS: null,
        transferInS: null,
        penaltyOutS: null,
        penaltyInS: null,
      };
      const timeS = Number.isFinite(transfer.time_s) ? Math.max(transfer.time_s, 0) : 0;
      const penaltyS = Number.isFinite(transfer.penalty_s ?? 0) ? Math.max(transfer.penalty_s ?? 0, 0) : 0;
      if (isOut) {
        row.transferOutS = row.transferOutS === null ? timeS : Math.min(row.transferOutS, timeS);
        row.penaltyOutS = row.penaltyOutS === null ? penaltyS : Math.min(row.penaltyOutS, penaltyS);
      }
      if (isIn) {
        row.transferInS = row.transferInS === null ? timeS : Math.min(row.transferInS, timeS);
        row.penaltyInS = row.penaltyInS === null ? penaltyS : Math.min(row.penaltyInS, penaltyS);
      }
      transferMap.set(otherStopId, row);
    }
    const transfers = [...transferMap.values()]
      .map((row) => {
        const otherStop = stopById.get(row.stopId);
        if (!otherStop) return null;
        const toS = row.transferOutS;
        const fromS = row.transferInS;
        let direction: "to" | "from" | "both" = "to";
        if (toS !== null && fromS !== null) direction = "both";
        else if (toS === null && fromS !== null) direction = "from";
        const transferTimeS = Math.round(Math.min(toS ?? Number.POSITIVE_INFINITY, fromS ?? Number.POSITIVE_INFINITY));
        const penaltyS = Math.round(Math.min(row.penaltyOutS ?? Number.POSITIVE_INFINITY, row.penaltyInS ?? Number.POSITIVE_INFINITY));
        return {
          stopId: row.stopId,
          name: stopDisplayName(otherStop),
          distanceM: distanceMetersXY(otherStop, selectedStop),
          transferTimeS: Number.isFinite(transferTimeS) ? transferTimeS : 0,
          penaltyS: Number.isFinite(penaltyS) ? penaltyS : 0,
          direction,
        };
      })
      .filter(
        (
          value
        ): value is {
          stopId: string;
          name: string;
          distanceM: number;
          transferTimeS: number;
          penaltyS: number;
          direction: "to" | "from" | "both";
        } => Boolean(value)
      )
      .sort((left, right) => left.transferTimeS - right.transferTimeS)
      .slice(0, 8);

    return { members, suggestions, transfers };
  }, [activeScenario, build.selection]);
  const selectedLineEstimatedCapexBase = useMemo(() => {
    if (!selectedLineDetail || !build.buildDefaults) return null;
    const preset = build.buildDefaults.presets.find((candidate) => candidate.id === selectedLinePresetId);
    if (!preset) return null;
    return (
      selectedLineDetail.stationIds.length * build.buildDefaults.station_capex_base +
      (selectedLineDetail.lengthM / 1000) * preset.capex_per_km_base
    );
  }, [build.buildDefaults, selectedLineDetail, selectedLinePresetId]);
  const selectedLineStationDecorations = useMemo(() => {
    if (!activeScenario || !selectedLineDetail) return {};
    const stopById = new Map(activeScenario.world.stops.map((stop) => [stop.id, stop]));
    const decorations: Record<
      string,
      {
        interchange: boolean;
        connectedLines: Array<{ lineId: string; lineName: string; displayColor?: string | null }>;
      }
    > = {};
    for (const station of selectedLineDetail.stations) {
      const servedLines = stationLineIndex.get(station.stop_id) ?? [];
      const connectedLines = servedLines
        .filter((line) => line.lineId !== selectedLineDetail.lineId)
        .map((line) => ({
          lineId: line.lineId,
          lineName: line.lineName,
          displayColor: line.displayColor ?? null,
        }));
      const stop = stopById.get(station.stop_id);
      decorations[station.stop_id] = {
        interchange: Boolean(stop?.interchange_id?.trim()) || connectedLines.length > 0,
        connectedLines: connectedLines.slice(0, 4),
      };
    }
    return decorations;
  }, [activeScenario, selectedLineDetail, stationLineIndex]);
  const activeLineMetrics = useMemo(() => {
    if (!build.activeLine || !activeScenario) {
      return { totalLengthM: 0, extensionLengthM: 0 };
    }
    const byId = new Map(activeScenario.world.stops.map((stop) => [stop.id, stop]));
    let total = 0;
    let baseline = 0;
    const startCount = Math.max(build.activeLine.extensionStartStationCount ?? 1, 1);
    for (let index = 1; index < build.activeLine.stationIds.length; index += 1) {
      const from = byId.get(build.activeLine.stationIds[index - 1]);
      const to = byId.get(build.activeLine.stationIds[index]);
      if (!from || !to) continue;
      const dx = from.x - to.x;
      const dy = from.y - to.y;
      const segment = Math.sqrt(dx * dx + dy * dy);
      total += segment;
      if (index < startCount) {
        baseline += segment;
      }
    }
    return {
      totalLengthM: total,
      extensionLengthM: Math.max(total - baseline, 0),
    };
  }, [activeScenario, build.activeLine]);
  const selectedLineScheduleState = useMemo(() => {
    if (!selectedLineDetail && !build.lineInspection?.schedule_state) return null;
    const schedule =
      selectedLineDetail?.scheduleProfile
        ? {
            peak_start_minute: selectedLineDetail.scheduleProfile.peak_start_minute,
            peak_end_minute: selectedLineDetail.scheduleProfile.peak_end_minute,
            overnight_start_minute: selectedLineDetail.scheduleProfile.overnight_start_minute,
            overnight_end_minute: selectedLineDetail.scheduleProfile.overnight_end_minute,
            tph_peak: selectedLineDetail.scheduleProfile.tph_peak,
            tph_off_peak: selectedLineDetail.scheduleProfile.tph_off_peak,
            tph_overnight: selectedLineDetail.scheduleProfile.tph_overnight,
          }
        : build.lineInspection?.schedule_state;
    return {
      peak_start_minute: schedule?.peak_start_minute ?? 420,
      peak_end_minute: schedule?.peak_end_minute ?? 570,
      overnight_start_minute: schedule?.overnight_start_minute ?? 0,
      overnight_end_minute: schedule?.overnight_end_minute ?? 300,
      tph_peak: schedule?.tph_peak ?? 0,
      tph_off_peak: schedule?.tph_off_peak ?? 0,
      tph_overnight: schedule?.tph_overnight ?? 0,
    };
  }, [build.lineInspection?.schedule_state, selectedLineDetail]);
  const selectedLineFleetEditorState = useMemo(() => {
    const fleetState =
      selectedLineDetail
        ? {
            package_id: selectedLineDetail.packageId,
            units_owned: selectedLineDetail.stockUnitsOwned,
            units_committed: selectedLineDetail.stockUnitsCommitted,
            units_pending: selectedLineDetail.stockUnitsPending,
            units_assigned: selectedLineDetail.stockUnitsAssigned,
            cars_per_unit: selectedLineDetail.carsPerUnit,
            speed_level: selectedLineDetail.speedLevel,
            comfort_level: selectedLineDetail.comfortLevel,
            units_required_now: selectedLineDetail.requiredUnits,
            pending_orders: selectedLineDetail.pendingOrders,
          }
        : build.lineInspection?.fleet_state;
    if (!selectedLineDetail && !fleetState) return null;
    const packageId = fleetState?.package_id ?? selectedLineDetail?.packageId ?? "standard";
    const unitsOwned = fleetState?.units_owned ?? selectedLineDetail?.stockUnitsOwned ?? 0;
    const unitsAssigned = fleetState?.units_assigned ?? selectedLineDetail?.stockUnitsAssigned ?? unitsOwned;
    return {
      packageId,
      unitsOwned,
      unitsCommitted:
        fleetState?.units_committed ??
        selectedLineDetail?.stockUnitsCommitted ??
        unitsOwned,
      unitsPending:
        fleetState?.units_pending ??
        selectedLineDetail?.stockUnitsPending ??
        0,
      unitsAssigned,
      carsPerUnit: fleetState?.cars_per_unit ?? selectedLineDetail?.carsPerUnit ?? 1,
      speedLevel: fleetState?.speed_level ?? selectedLineDetail?.speedLevel ?? "balanced",
      comfortLevel: fleetState?.comfort_level ?? selectedLineDetail?.comfortLevel ?? "standard",
      requiredUnitsNow: fleetState?.units_required_now ?? selectedLineDetail?.requiredUnits ?? 0,
      pendingOrders:
        fleetState?.pending_orders ??
        selectedLineDetail?.pendingOrders ??
        [],
    };
  }, [build.lineInspection?.fleet_state, selectedLineDetail]);
  const selectedLineUnitLabel = useMemo(
    () => unitLabelForMode(selectedLineBuildPreset?.engine_mode ?? selectedLineDetail?.mode ?? null),
    [selectedLineBuildPreset?.engine_mode, selectedLineDetail?.mode]
  );
  const selectedLineActiveVehicles = useMemo(() => {
    if (!selectedLineDetail) return [] as Array<{
      vehicleId: string;
      label: string;
      destinationLabel: string;
      onBoard: number;
      capacity: number;
    }>;
    return runtimeTrains
      .filter((train) => train.line_id === selectedLineDetail.lineId)
      .sort((left, right) => {
        const leftOrdinal = Math.max(Math.round(left.vehicle_ordinal ?? 0), 0);
        const rightOrdinal = Math.max(Math.round(right.vehicle_ordinal ?? 0), 0);
        if (leftOrdinal !== rightOrdinal) return leftOrdinal - rightOrdinal;
        return left.train_id.localeCompare(right.train_id);
      })
      .map((train) => ({
        vehicleId: train.train_id,
        label: `${selectedLineUnitLabel} #${Math.max(Math.round(train.vehicle_ordinal || 0), 1)}`,
        destinationLabel: train.destination_label || train.direction_label || "Outbound",
        onBoard: Math.max(train.onboard_pax ?? 0, 0),
        capacity: Math.max(train.vehicle_capacity ?? 0, 0),
      }));
  }, [runtimeTrains, selectedLineDetail, selectedLineUnitLabel]);
  const selectedLineTransferTargets = useMemo(() => {
    if (!selectedLineDetail) return [] as Array<{ lineId: string; lineName: string }>;
    return build.lineSummaries
      .filter((line) => line.lineId !== selectedLineDetail.lineId)
      .filter((line) => line.mode === selectedLineDetail.mode)
      .filter((line) => (line.modeVariant ?? null) === (selectedLineDetail.modeVariant ?? null))
      .map((line) => ({
        lineId: line.lineId,
        lineName: line.name.trim() ? line.name : "Untitled Line",
      }));
  }, [build.lineSummaries, selectedLineDetail]);
  const selectedLineUnitCostBase = useMemo(() => {
    if (!selectedLineBuildPreset || !selectedLineFleetEditorState) return 0;
    const packageOptions = selectedLineBuildPreset.package_options.length
      ? selectedLineBuildPreset.package_options
      : selectedLineBuildPreset.tiers;
    const packageChoice =
      packageOptions.find(
        (item) => item.id.toLowerCase() === selectedLineFleetEditorState.packageId.toLowerCase()
      ) ??
      packageOptions[0] ??
      null;
    const speedChoice =
      selectedLineBuildPreset.speed_levels.find(
        (item) => item.id.toLowerCase() === selectedLineFleetEditorState.speedLevel.toLowerCase()
      ) ??
      selectedLineBuildPreset.speed_levels[0] ??
      null;
    const comfortChoice =
      selectedLineBuildPreset.comfort_levels.find(
        (item) => item.id.toLowerCase() === selectedLineFleetEditorState.comfortLevel.toLowerCase()
      ) ??
      selectedLineBuildPreset.comfort_levels[0] ??
      null;
    const carsPerUnit = selectedLineBuildPreset.supports_carriages
      ? Math.min(
          Math.max(selectedLineFleetEditorState.carsPerUnit, selectedLineBuildPreset.cars_min),
          selectedLineBuildPreset.cars_max
        )
      : 1;
    const carsMultiplier = selectedLineBuildPreset.supports_carriages
      ? Math.max(carsPerUnit / Math.max(selectedLineBuildPreset.cars_default, 1), 0.5)
      : 1;
    return (
      selectedLineBuildPreset.base_unit_purchase_cost_base *
      (packageChoice?.purchase_cost_multiplier ?? 1) *
      (speedChoice?.cost_multiplier ?? 1) *
      (comfortChoice?.cost_multiplier ?? 1) *
      carsMultiplier
    );
  }, [selectedLineBuildPreset, selectedLineFleetEditorState]);
  const selectedLineScrapEstimateBase = useMemo(() => {
    if (!selectedLineBuildPreset || !selectedLineFleetEditorState) return 0;
    return (
      selectedLineFleetEditorState.unitsOwned *
      selectedLineUnitCostBase *
      Math.max(selectedLineBuildPreset.salvage_rate, 0)
    );
  }, [selectedLineBuildPreset, selectedLineFleetEditorState, selectedLineUnitCostBase]);
  const fleetDeliveries = useMemo(
    () =>
      build.lineSummaries
        .flatMap((line) => {
          const lineName = line.name.trim() ? line.name : "Untitled Line";
          return (line.pendingOrders ?? []).flatMap((order, index) => {
            const units = Math.max(Math.round(order.units ?? 0), 0);
            const rows = [];
            for (let ordinal = 0; ordinal < units; ordinal += 1) {
              const rowLabel =
                (ordinal === 0 ? order.label?.trim() : "") ||
                `${unitLabelForMode(line.mode)} #${Math.max(line.stockUnitsOwned + ordinal + 1, 1)}`;
              const activeVehicle =
                runtimeTrains.find((train) => train.line_id === line.lineId) ?? null;
              rows.push({
                id: `${order.order_id}:${index}:${ordinal}`,
                orderId: order.order_id,
                label: rowLabel,
                lineId: line.lineId,
                lineName,
                status: order.status ?? "pending",
                etaAtTickS:
                  typeof order.eta_at_tick_s === "number" && Number.isFinite(order.eta_at_tick_s)
                    ? order.eta_at_tick_s
                    : null,
                focusVehicleId: activeVehicle?.train_id ?? null,
              });
            }
            return rows;
          });
        })
        .sort((left, right) => {
          const leftEta = left.etaAtTickS ?? Number.POSITIVE_INFINITY;
          const rightEta = right.etaAtTickS ?? Number.POSITIVE_INFINITY;
          return leftEta - rightEta;
        }),
    [build.lineSummaries, runtimeTrains]
  );
  const financialLineOptions = useMemo(
    () =>
      build.lineSummaries.map((line) => ({
        lineId: line.lineId,
        name: line.name.trim() ? line.name : "Untitled Line",
      })),
    [build.lineSummaries]
  );
  const showSessionBootOverlay =
    (route === "session_game" || route === "session_scenario") &&
    sessionBootState.stage !== "idle" &&
    sessionBootState.stage !== "ready";
  const currentBuildPreset = useMemo(
    () => getBuildPreset(build.buildDefaults, build.transportPresetId),
    [build.buildDefaults, build.transportPresetId]
  );
  const previewAnchorPoint = useMemo(() => {
    if (!build.activeLine || !activeScenario) return null;
    const lastStopId = build.activeLine.stationIds[build.activeLine.stationIds.length - 1];
    const lastStop = activeScenario.world.stops.find((stop) => stop.id === lastStopId);
    if (!lastStop) return null;
    return { x: lastStop.x, y: lastStop.y };
  }, [activeScenario, build.activeLine]);
  const previewColor = useMemo(() => {
    if (build.buildAction === "add_station_to_line") {
      return selectedLineDetail?.displayColor ?? currentBuildPreset?.default_color ?? "#104894";
    }
    return currentBuildPreset?.default_color ?? "#104894";
  }, [build.buildAction, currentBuildPreset?.default_color, selectedLineDetail?.displayColor]);
  const activeLineDraftPreview = useMemo(() => {
    if (!build.activeLine || !activeScenario) return null;
    const preset =
      getBuildPreset(build.buildDefaults, build.activeLine.presetId) ??
      getBuildPreset(build.buildDefaults, build.transportPresetId);
    const stationNames = build.activeLine.stationIds.map((stopId) => {
      const stop = activeScenario.world.stops.find((candidate) => candidate.id === stopId);
      return stop ? stopDisplayName(stop) : stopId;
    });
    return {
      lineId: build.activeLine.lineId,
      lineName: `New ${preset?.label ?? "Line"}`,
      modeLabel: preset?.label ?? "Transit",
      displayColor: preset?.default_color ?? previewColor,
      stationNames,
      stationIds: [...build.activeLine.stationIds],
    };
  }, [
    activeScenario,
    build.activeLine,
    build.buildDefaults,
    build.transportPresetId,
    previewColor,
  ]);
  const stationCostBase = useMemo(() => build.buildDefaults?.station_capex_base ?? null, [build.buildDefaults]);
  const lineCostPerKmBase = useMemo(
    () => selectedLineBuildPreset?.capex_per_km_base ?? currentBuildPreset?.capex_per_km_base ?? null,
    [currentBuildPreset, selectedLineBuildPreset]
  );
  const extensionAddedStations = useMemo(() => {
    if (selectedLineDetail) {
      const baseCount = selectedBaseLineDetail?.stationIds.length ?? 0;
      return Math.max(selectedLineDetail.stationIds.length - baseCount, 0);
    }
    if (!build.activeLine) return 0;
    const startCount = Math.max(build.activeLine.extensionStartStationCount ?? 1, 1);
    return Math.max(build.activeLine.stationIds.length - startCount, 0);
  }, [build.activeLine, selectedBaseLineDetail, selectedLineDetail]);
  const extensionAddedLengthM = useMemo(
    () =>
      selectedLineDetail
        ? Math.max(selectedLineDetail.lengthM - (selectedBaseLineDetail?.lengthM ?? 0), 0)
        : activeLineMetrics.extensionLengthM,
    [activeLineMetrics.extensionLengthM, selectedBaseLineDetail?.lengthM, selectedLineDetail]
  );
  const extensionConstructionCostBase = useMemo(() => {
    if (stationCostBase === null || lineCostPerKmBase === null) return null;
    const stationComponent = extensionAddedStations * stationCostBase;
    const trackComponent = (Math.max(extensionAddedLengthM, 0) / 1000) * lineCostPerKmBase;
    return stationComponent + trackComponent;
  }, [extensionAddedLengthM, extensionAddedStations, lineCostPerKmBase, stationCostBase]);
  const lineDraftMode =
    build.workspaceMode === "build" &&
    (build.buildAction === "start_line" || build.buildAction === "add_station_to_line") &&
    Boolean(build.activeLine);
  const buildConstraintMode = useMemo(() => {
    if (build.workspaceMode !== "build") return null;
    if (build.buildAction === "add_station_to_line") {
      return selectedLineBuildPreset?.engine_mode ?? currentBuildPreset?.engine_mode ?? null;
    }
    if (build.buildAction === "start_line" || build.buildAction === "place_station") {
      return currentBuildPreset?.engine_mode ?? null;
    }
    return selectedLineBuildPreset?.engine_mode ?? null;
  }, [
    build.buildAction,
    build.workspaceMode,
    currentBuildPreset?.engine_mode,
    selectedLineBuildPreset?.engine_mode,
  ]);

  useEffect(() => {
    void refreshLibraries();
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
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
    if (budgetEdited) return;
    setNewGameBudget(String(defaultBudgetFor(newGameDifficulty, newGameCurrency)));
  }, [newGameDifficulty, newGameCurrency, budgetEdited]);

  useEffect(() => {
    if (!saveStatus.trim()) return;
    const timer = window.setTimeout(() => {
      setSaveStatus("");
    }, 5500);
    return () => window.clearTimeout(timer);
  }, [saveStatus]);

  useEffect(() => {
    const activeIds = new Set(rawAlerts.map((alert) => alert.id));
    setDismissedAlertIds((previous) => previous.filter((id) => activeIds.has(id)));
  }, [rawAlerts]);

  useEffect(() => {
    if (!lineDeleteDialogOpen) return;
    if (build.selection?.kind === "line" && selectedLineDetail) return;
    setLineDeleteDialogOpen(false);
  }, [build.selection, lineDeleteDialogOpen, selectedLineDetail]);

  useEffect(() => {
    let cancelled = false;
    if (!bundle || sessionKind !== "game") {
      setMapRuntimeConfig(null);
      setSessionBootState((prev) =>
        prev.stage === "idle"
          ? prev
          : {
              stage: "idle",
              progress: 0,
              message: "",
              error: null,
            }
      );
      return;
    }
    setSessionBootState((prev) => ({
      stage: "map_runtime_config",
      progress: Math.max(prev.progress, 0.28),
      message: "Loading map runtime config...",
      error: null,
    }));
    void invoke("load_map_runtime_config", {
      projectPath: bundle.project_path,
    })
      .then((res) => {
        if (!cancelled) {
          setMapRuntimeConfig(res as MapRuntimeConfig);
          setSessionBootState((prev) => ({
            stage: "map_runtime_config",
            progress: Math.max(prev.progress, 0.46),
            message: "Map runtime config ready.",
            error: null,
          }));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setMapRuntimeConfig(null);
          setSessionBootState({
            stage: "error",
            progress: 0.46,
            message: "Map config failed to load.",
            error: "Unable to load map runtime config. You can retry map loading.",
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [bundle?.project_path, sessionKind]);

  useEffect(() => {
    let cancelled = false;
    if (!bundle || sessionKind !== "game") {
      setFarePolicy(null);
      return;
    }
    void invoke("get_fare_policy", {
      projectPath: bundle.project_path,
    })
      .then((res) => {
        if (!cancelled) setFarePolicy(res as FarePolicyManifest);
      })
      .catch(() => {
        if (!cancelled) setFarePolicy(null);
      });
    return () => {
      cancelled = true;
    };
  }, [bundle?.project_path, sessionKind]);

  useRuntimePolling({
    bundle,
    sessionKind,
    latestClockTickRef,
    latestSnapshotTickRef,
    latestSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
    latestStrategicSnapshotCapturedRef,
    runtimeControlQueueRef,
    setClock,
    setLiveEconomy,
    setServiceLoadByServiceId,
    setRuntimeTrains,
    setRuntimeStations,
    setRuntimeLineOps,
    setTrainsAuthoritative,
    setRuntimeTelemetry,
    setSnapshotLatencyMs,
    setError,
  });

  useEffect(() => {
    if (build.workspaceMode !== "build") return;
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [build.workspaceMode]);

  useEffect(() => {
    if (build.selection?.kind !== "line") {
      setRollingStockEditorOpen(false);
      setScheduleEditorOpen(false);
    }
  }, [build.selection]);

  useEffect(() => {
    if (build.workspaceMode !== "build") {
      setRollingStockEditorOpen(false);
      setScheduleEditorOpen(false);
    }
  }, [build.workspaceMode]);

  useEffect(() => {
    const inSession = route === "session_game" || route === "session_scenario";
    if (inSession) return;
    setShowAlerts(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
    setShowMenu(false);
  }, [route]);

  useEffect(() => {
    if (!showFinancialDashboard || !bundle || sessionKind !== "game") return;
    let cancelled = false;
    const poll = async () => {
      setFinancialBusy(true);
      setFinancialError(null);
      try {
        const response = (await invoke("get_financial_dashboard", {
          projectPath: bundle.project_path,
          request: financialRequest,
        })) as FinancialDashboardResponse;
        if (!cancelled) setFinancialData(response);
      } catch (err) {
        if (!cancelled) setFinancialError(formatBackendError(err));
      } finally {
        if (!cancelled) setFinancialBusy(false);
      }
    };
    void poll();
    const timer = window.setInterval(() => {
      void poll();
    }, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [bundle?.project_path, financialRequest, sessionKind, showFinancialDashboard]);

  useEffect(() => {
    if (route !== "session_game") {
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
  }, [route, bundle?.manifest.project_id]);

  useEffect(() => {
    if (!onboardingActive || route !== "session_game") return;
    const hasCommittedLine = (activeScenario?.world.services.length ?? 0) > 0;
    const hasOrderedUnits = build.lineSummaries.some((line) => {
      const unitsPending = line.stockUnitsPending ?? 0;
      const unitsOwned = line.stockUnitsOwned ?? 0;
      return unitsPending > 0 || unitsOwned > 0;
    });
    if (onboardingStep === 0 && build.workspaceMode === "build") {
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
    if (onboardingStep === 3 && clock?.running) {
      setOnboardingActive(false);
      if (typeof window !== "undefined") {
        window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "done");
      }
    }
  }, [
    activeScenario?.world.services.length,
    build.lineSummaries,
    build.workspaceMode,
    clock?.running,
    onboardingActive,
    onboardingStep,
    route,
  ]);

  function playUiCue(kind: "confirm" | "error" | "toggle" | "alert") {
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
  }

  async function withBusy<T>(fn: () => Promise<T>): Promise<T | null> {
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e: unknown) {
      setError(formatBackendError(e));
      playUiCue("error");
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function refreshLibraries() {
    const [games, scenarios, deleted, countryList, packList] = await Promise.all([
      invoke("list_game_saves").catch(() => [] as GameSaveMeta[]),
      invoke("list_scenario_saves").catch(() => [] as ScenarioSaveMeta[]),
      invoke("list_deleted_saves").catch(() => [] as DeletedSaveMeta[]),
      invoke("list_countries").catch(() => [] as CountryOption[]),
      invoke("list_country_pack_status").catch(() => [] as CountryPackStatus[]),
    ]);
    setGameSaves(games as GameSaveMeta[]);
    setScenarioSaves(scenarios as ScenarioSaveMeta[]);
    setDeletedSaves(deleted as DeletedSaveMeta[]);
    const countriesResolved = countryList as CountryOption[];
    const packsResolved = packList as CountryPackStatus[];
    setCountries(countriesResolved);
    setCountryPacks(packsResolved);
    if (countriesResolved.length > 0 && !selectedCountryIso2) {
      const eligibleIso = packsResolved.find((p) => p.eligible)?.country_iso2 ?? countriesResolved[0].iso2;
      void onCountryChanged(eligibleIso);
    }
  }

  async function refreshDemandCoverage(projectPath?: string) {
    const target = projectPath ?? bundle?.project_path;
    if (!target) {
      setDemandCoverage([]);
      return;
    }
    const coverage = await invoke("list_demand_coverage", { projectPath: target }).catch(
      () => [] as DemandCoverageMeta[]
    );
    const rows = coverage as DemandCoverageMeta[];
    setDemandCoverage(rows);
    const missing = rows.filter((c) => !c.installed).map((c) => c.country_iso2);
    if (missing.length > 0) {
      setDemandWarning(
        `Demand data not installed for ${missing.join(", ")}. Install demand surface packs to enable country coverage.`
      );
    } else {
      setDemandWarning(null);
    }
  }

  async function refreshRegions(projectPath?: string, preferredFocusId?: string | null) {
    const target = projectPath ?? bundle?.project_path;
    if (!target) {
      setRegions([]);
      setFocusRegionId(null);
      setSelectedRegionId(null);
      return;
    }
    const rows = (await invoke("list_regions", { projectPath: target }).catch(
      () => [] as RegionStatus[]
    )) as RegionStatus[];
    setRegions(rows);
    if (rows.length === 0) {
      setFocusRegionId(null);
      setSelectedRegionId(null);
      return;
    }
    const focusFromManifest = preferredFocusId ?? bundle?.manifest.region_state?.primary_focus_region_id ?? null;
    const resolvedFocus =
      rows.find((r) => r.region_id === focusFromManifest)?.region_id ??
      rows.find((r) => r.active)?.region_id ??
      rows.find((r) => r.unlocked)?.region_id ??
      rows[0].region_id;
    setFocusRegionId(resolvedFocus);
    setSelectedRegionId((prev) => {
      if (prev && rows.some((r) => r.region_id === prev)) return prev;
      return resolvedFocus;
    });
  }

  async function primeRuntimeSnapshot(projectPath: string) {
    setSessionBootState((prev) => ({
      stage: "snapshot",
      progress: Math.max(prev.progress, 0.78),
      message: "Hydrating runtime snapshot...",
      error: null,
    }));
    const res = await invoke("get_runtime_snapshot", {
      projectPath,
    }).catch(() => null);
    if (!res) {
      setSessionBootState((prev) => ({
        stage: "snapshot",
        progress: Math.max(prev.progress, 0.82),
        message: "Runtime snapshot unavailable, continuing with live polling.",
        error: null,
      }));
      return;
    }
    const snapshot = res as RuntimeSnapshot;
    const nextFreshness = clockFreshnessFromSnapshot(
      snapshot.clock,
      snapshot.telemetry?.tick_index,
      snapshot.captured_at_epoch_ms
    );
    if (nextFreshness) {
      latestClockTickRef.current = nextFreshness.tickSeconds;
      latestSnapshotTickRef.current = nextFreshness.tickIndex;
      latestSnapshotCapturedRef.current = nextFreshness.capturedAtEpochMs;
    }
    latestStrategicSnapshotTickRef.current = finiteNumber(snapshot.telemetry?.tick_index, 0);
    latestStrategicSnapshotCapturedRef.current = finiteNumber(snapshot.captured_at_epoch_ms, 0);
    setRuntimeTelemetry(snapshot.telemetry ?? null);
    const capturedAt = finiteNumber(snapshot.captured_at_epoch_ms, 0);
    setSnapshotLatencyMs(capturedAt > 0 ? Math.max(Date.now() - capturedAt, 0) : null);
    setTrainsAuthoritative(Boolean(snapshot.trains_authoritative));
    const nextRuntimeTrains = Array.isArray(snapshot.trains) ? snapshot.trains : [];
    setRuntimeTrains((prev) => (sameRuntimeTrains(prev, nextRuntimeTrains) ? prev : nextRuntimeTrains));
    setRuntimeStations(Array.isArray(snapshot.stations) ? snapshot.stations : []);
    setRuntimeLineOps(Array.isArray(snapshot.line_ops) ? snapshot.line_ops : []);
    if (snapshot.clock) {
      setClock((prev) => (sameClock(prev, snapshot.clock ?? null) ? prev : snapshot.clock ?? null));
    }
    if (snapshot.economy) {
      setLiveEconomy((prev) => (sameEconomy(prev, snapshot.economy ?? null) ? prev : snapshot.economy ?? null));
    }
    const nextServiceLoads: Record<string, number> = {};
    for (const row of snapshot.frame?.service_loads ?? []) {
      const serviceId = row.service_id?.trim();
      if (!serviceId) continue;
      const ratio = Number.isFinite(row.load_to_capacity)
        ? Math.max(row.load_to_capacity, 0)
        : 0;
      nextServiceLoads[serviceId] = Math.max(nextServiceLoads[serviceId] ?? 0, ratio);
    }
    setServiceLoadByServiceId((prev) => (sameServiceLoads(prev, nextServiceLoads) ? prev : nextServiceLoads));
    setSessionBootState((prev) => ({
      stage: "snapshot",
      progress: Math.max(prev.progress, 0.9),
      message: "Runtime snapshot hydrated.",
      error: null,
    }));
  }

  function handleMapBootProgress(payload: MapBootProgressPayload) {
    if (payload.stage === "ready") {
      setSessionBootState({
        stage: "ready",
        progress: 1,
        message: payload.message || "Session ready.",
        error: null,
      });
      return;
    }
    if (payload.stage === "error") {
      setSessionBootState((prev) => ({
        stage: "error",
        progress: Math.max(prev.progress, payload.progress || 0.5),
        message: payload.message || "Map failed to load.",
        error: payload.error ?? "Map failed to load. Retry map loading.",
      }));
      return;
    }
    setSessionBootState((prev) => {
      if (prev.stage === "ready") return prev;
      const prevRank = SESSION_BOOT_STAGE_RANK[prev.stage];
      const nextRank = SESSION_BOOT_STAGE_RANK[payload.stage];
      if (nextRank < prevRank) return prev;
      return {
        stage: payload.stage,
        progress: Math.max(prev.progress, payload.progress),
        message: payload.message,
        error: null,
      };
    });
  }

  function retryMapLoad() {
    setMapInstanceToken((value) => value + 1);
    setSessionBootState((prev) => ({
      stage: "map_style",
      progress: Math.max(prev.progress, 0.52),
      message: "Retrying map initialization...",
      error: null,
    }));
  }

  useEffect(() => {
    const inSession = route === "session_game" || route === "session_scenario";
    if (!inSession) return;
    if (sessionBootState.stage !== "map_context") return;
    if (sessionBootState.error) return;
    if (sessionBootState.progress < 0.84) return;
    const timer = window.setTimeout(() => {
      setSessionBootState((prev) => {
        if (prev.stage !== "map_context" || prev.error) return prev;
        return {
          stage: "ready",
          progress: 1,
          message: "Session ready.",
          error: null,
        };
      });
    }, 900);
    return () => window.clearTimeout(timer);
  }, [route, sessionBootState.error, sessionBootState.progress, sessionBootState.stage]);

  async function refreshFinancialDashboard() {
    if (!bundle || sessionKind !== "game") return;
    setFinancialBusy(true);
    setFinancialError(null);
    try {
      const response = (await invoke("get_financial_dashboard", {
        projectPath: bundle.project_path,
        request: financialRequest,
      })) as FinancialDashboardResponse;
      setFinancialData(response);
    } catch (err) {
      setFinancialError(formatBackendError(err));
    } finally {
      setFinancialBusy(false);
    }
  }

  async function onCountryChanged(iso2: string) {
    setSelectedCountryIso2(iso2);
    const cityList = await withBusy(async () =>
      (await invoke("list_cities", { countryIso2: iso2 })) as CityOption[]
    );
    if (!cityList) return;
    setCities(cityList);
    setSelectedCityId(cityList[0]?.geonameid ?? null);
    setCitySearch("");
  }

  async function installCountryPack(iso2: string) {
    const res = await withBusy(
      async () => (await invoke("install_country_pack", { countryIso2: iso2 })) as InstallResult
    );
    if (!res) return;
    setSaveStatus(res.message);
    await refreshLibraries();
    await onCountryChanged(iso2);
  }

  async function uninstallCountryPack(iso2: string) {
    const res = await withBusy(
      async () => (await invoke("uninstall_country_pack", { countryIso2: iso2 })) as UninstallResult
    );
    if (!res) return;
    setSaveStatus(res.message);
    await refreshLibraries();
    await onCountryChanged(iso2);
  }

  function applyOpenedSession(opened: OpenSessionResult) {
    setBundle(opened);
    setClock(opened.clock);
    setSaveStatus("");
    setRoute(opened.manifest.session_kind === "game" ? "session_game" : "session_scenario");
    setMapInstanceToken((value) => value + 1);
    setSessionBootState({
      stage: "session_open",
      progress: 0.12,
      message: "Opening session...",
      error: null,
    });
    void refreshDemandCoverage(opened.project_path);
    if (opened.manifest.session_kind === "game") {
      setShowCountryInfo(false);
      void refreshRegions(opened.project_path, opened.manifest.region_state?.primary_focus_region_id ?? null);
      void primeRuntimeSnapshot(opened.project_path);
    } else {
      setRegions([]);
      setFocusRegionId(null);
      setSelectedRegionId(null);
      setShowCountryInfo(false);
    }
  }

  async function continueLatestGame() {
    const opened = await withBusy(async () =>
      (await invoke("continue_latest_game")) as OpenSessionResult
    );
    if (opened) applyOpenedSession(opened);
  }

  async function loadGameSave(saveId: string) {
    const opened = await withBusy(async () =>
      (await invoke("load_game_save", { saveId })) as OpenSessionResult
    );
    if (opened) applyOpenedSession(opened);
  }

  async function loadScenarioSave(saveId: string) {
    const opened = await withBusy(async () =>
      (await invoke("load_scenario_save", { saveId })) as OpenSessionResult
    );
    if (opened) applyOpenedSession(opened);
  }

  function selectCounty(regionId: string) {
    setSelectedRegionId(regionId);
    const region = regions.find((row) => row.region_id === regionId) ?? null;
    setShowCountryInfo(Boolean(region && !region.unlocked));
  }

  async function focusSelectedCounty() {
    if (!bundle || !selectedRegionId || sessionKind !== "game") return;
    const reopened = await withBusy(async () => {
      await invoke("set_primary_focus_region", {
        projectPath: bundle.project_path,
        regionId: selectedRegionId,
      }) as FocusResult;
      return (await invoke("open_project", { projectPath: bundle.project_path })) as OpenSessionResult;
    });
    if (!reopened) return;
    applyOpenedSession(reopened);
    setSaveStatus(`Focused county ${selectedRegionId}`);
  }

  async function unlockAndFocusSelectedCounty() {
    if (!bundle || !selectedRegionId || sessionKind !== "game") return;
    let focusedRegionId = selectedRegionId;
    const reopened = await withBusy(async () => {
      const unlock = (await invoke("unlock_and_focus_region", {
        projectPath: bundle.project_path,
        regionId: selectedRegionId,
      })) as UnlockFocusResult;
      if (unlock.region_id?.trim()) focusedRegionId = unlock.region_id.trim();
      return (await invoke("open_project", { projectPath: bundle.project_path })) as OpenSessionResult;
    });
    if (!reopened) return;
    applyOpenedSession(reopened);
    setSaveStatus(`Unlocked and focused county ${focusedRegionId}`);
    await refreshLibraries();
  }

  async function deleteSave(saveId: string, name: string) {
    const ok = window.confirm(`Move "${name}" to Recently Deleted?`);
    if (!ok) return;
    await withBusy(async () => (await invoke("delete_save", { saveId })) as DeleteSaveResult);
    await refreshLibraries();
  }

  async function restoreDeletedSave(deletedId: string) {
    await withBusy(
      async () => (await invoke("restore_deleted_save", { deletedId })) as RestoreSaveResult
    );
    await refreshLibraries();
  }

  async function purgeDeletedSave(deletedId: string) {
    const ok = window.confirm("Permanently delete this save?");
    if (!ok) return;
    await withBusy(async () => (await invoke("purge_deleted_save", { deletedId })) as PurgeSaveResult);
    await refreshLibraries();
  }

  async function createGame() {
    if (!selectedCountry || !selectedCity) {
      setError("Select a country and city before creating a game.");
      return;
    }
    if (!selectedCountryPack?.eligible) {
      setError(selectedCountryPack?.reason ?? `Country ${selectedCountry.iso2} is not available yet.`);
      return;
    }
    const payload: GameCreatePayload = {
      name: newGameName.trim() || "Interlinked Game",
      country_iso2: selectedCountry.iso2,
      country_name: selectedCountry.name,
      city_id: selectedCity.geonameid,
      city_name: selectedCity.name,
      city_lon: selectedCity.lon,
      city_lat: selectedCity.lat,
      city_population: selectedCity.population,
      difficulty: newGameDifficulty,
      currency: newGameCurrency,
      starting_budget: Number(newGameBudget) || defaultBudgetFor(newGameDifficulty, newGameCurrency),
    };
    const opened = await withBusy(async () =>
      (await invoke("create_game", { payload })) as OpenSessionResult
    );
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
    }
  }

  async function createScenario() {
    const opened = await withBusy(async () =>
      (await invoke("create_scenario", {
        payload: { name: scenarioName.trim() || "Interlinked Scenario" },
      })) as OpenSessionResult
    );
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
    }
  }

  async function importScenarioFromPicker() {
    const picked = await withBusy(async () =>
      (await invoke("pick_scenario_file")) as string | null
    );
    if (!picked) return;
    const opened = await withBusy(async () =>
      (await invoke("import_scenario", {
        filePath: picked,
        name: null,
      })) as OpenSessionResult
    );
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
    }
  }

  async function saveSession() {
    if (!bundle) return;
    if (build.workspaceMode === "build" && build.isDirty) {
      build.setBuilderError("Apply or cancel the current build draft before saving the session.");
      return;
    }
    const res = await withBusy(async () =>
      (await invoke("save_session", {
        projectPath: bundle.project_path,
        payload: { scenario_document: bundle.scenario },
      })) as SaveResult
    );
    if (res) {
      setSaveStatus(`Saved ${res.updated_at}`);
      playUiCue("confirm");
    }
  }

  async function saveQuit() {
    if (!bundle) return;
    if (build.workspaceMode === "build" && build.isDirty) {
      build.setBuilderError("Apply or cancel the current build draft before saving and quitting.");
      return;
    }
    const res = await withBusy(async () =>
      (await invoke("save_and_quit", {
        projectPath: bundle.project_path,
        payload: { scenario_document: bundle.scenario },
      })) as SaveResult
    );
    if (!res) return;
    setBundle(null);
    setClock(null);
    setDemandCoverage([]);
    setDemandWarning(null);
    setRegions([]);
    setFocusRegionId(null);
    setSelectedRegionId(null);
    setShowCountryInfo(false);
    setRoute("home");
    setShowMenu(false);
    setSaveStatus(`Saved ${res.updated_at}`);
    playUiCue("confirm");
    await refreshLibraries();
  }

  async function setRunning(running: boolean) {
    if (!bundle) return;
    const projectPath = bundle.project_path;
    runtimeControlQueueRef.current = runtimeControlQueueRef.current
      .then(async () => {
        const res = (await invoke("set_simulation_running", {
          projectPath,
          running,
        })) as SimulationClock;
        setClock((prev) => (sameClock(prev, res) ? prev : res));
        playUiCue("toggle");
      })
      .catch((e) => {
        setError(String(e));
        playUiCue("error");
      });
    await runtimeControlQueueRef.current;
  }

  async function setSpeed(speed: SimulationSpeed) {
    if (!bundle) return;
    const projectPath = bundle.project_path;
    runtimeControlQueueRef.current = runtimeControlQueueRef.current
      .then(async () => {
        const res = (await invoke("set_simulation_speed", {
          projectPath,
          speed,
        })) as SimulationClock;
        setClock((prev) => (sameClock(prev, res) ? prev : res));
      })
      .catch((e) => {
        setError(String(e));
        playUiCue("error");
      });
    await runtimeControlQueueRef.current;
  }

  async function runPlanning() {
    if (!bundle) return;
    const run = await withBusy(async () =>
      (await invoke("run_planning", {
        projectPath: bundle.project_path,
        runConfig,
      })) as RunMeta
    );
    if (!run) return;
    setBundle((prev) => {
      if (!prev) return prev;
      const deduped = prev.runs.filter((r) => r.run_id !== run.run_id);
      return { ...prev, runs: [run, ...deduped] };
    });
    setSelectedBaseRun(run.run_id);
  }

  async function pickExportPath(kind: "csv" | "json"): Promise<string | null> {
    const picked = await withBusy(
      async () => (await invoke("pick_export_path", { fileKind: kind })) as string | null
    );
    return picked ?? null;
  }

  async function exportRunCsv(runId: string) {
    if (!bundle) return;
    const outPath = await pickExportPath("csv");
    if (!outPath) return;
    const res = await withBusy(async () =>
      (await invoke("export_scenario_report_csv", {
        projectPath: bundle.project_path,
        runId,
        filePath: outPath,
      })) as { out_path: string }
    );
    if (res) setSaveStatus(`CSV saved ${res.out_path}`);
  }

  async function exportRunJson(runId: string) {
    if (!bundle) return;
    const outPath = await pickExportPath("json");
    if (!outPath) return;
    const res = await withBusy(async () =>
      (await invoke("export_scenario_report_json", {
        projectPath: bundle.project_path,
        runId,
        filePath: outPath,
      })) as { out_path: string }
    );
    if (res) setSaveStatus(`JSON saved ${res.out_path}`);
  }

  async function compareRuns() {
    if (!bundle || !selectedBaseRun || !selectedCandidateRun) return;
    const res = await withBusy(async () =>
      (await invoke("compare_runs", {
        projectPath: bundle.project_path,
        baseRunId: selectedBaseRun,
        candidateRunId: selectedCandidateRun,
      })) as CompareResult
    );
    if (res) setCompareResult(res);
  }

  async function rebuildDemandForUnlocked() {
    if (!bundle) return;
    const rebuilt = await withBusy(
      async () =>
        (await invoke("rebuild_demand_for_unlocked", {
          projectPath: bundle.project_path,
        })) as DemandRebuildResult
    );
    if (!rebuilt) return;
    const reopened = await withBusy(async () =>
      (await invoke("open_project", { projectPath: bundle.project_path })) as OpenSessionResult
    );
    if (!reopened) return;
    applyOpenedSession(reopened);
    setSaveStatus(
      `Demand rebuilt: loaded ${rebuilt.loaded_countries.length}, missing ${rebuilt.missing_countries.length}`
    );
  }

  function nextGameStep() {
    setError(null);
    if (newGameStep === 1) {
      if (!newGameName.trim()) {
        setError("Enter a game name.");
        return;
      }
      setNewGameStep(2);
      return;
    }
    if (newGameStep === 2) {
      if (!selectedCountry || !selectedCity) {
        setError("Select country and city.");
        return;
      }
      if (!selectedCountryPack?.eligible) {
        setError(selectedCountryPack?.reason ?? `Country ${selectedCountry.iso2} is not available yet.`);
        return;
      }
      setNewGameStep(3);
      return;
    }
    if (newGameStep === 3) {
      const budget = Number(newGameBudget);
      if (!Number.isFinite(budget) || budget <= 0) {
        setError("Enter a valid starting budget.");
        return;
      }
      setNewGameStep(4);
    }
  }

  function leaveBuildMode() {
    if (build.workspaceMode !== "build") return;
    if (build.isDirty) {
      const discard = window.confirm("Discard the current build draft and return to view mode?");
      if (!discard) return;
    }
    build.cancelBuildMode();
  }

  function handleStopAction(payload: { stopId: string; point: { lng: number; lat: number; x: number; y: number } }) {
    const placingInBuildMode =
      build.workspaceMode === "build" &&
      (build.buildAction === "place_station" ||
        build.buildAction === "start_line" ||
        build.buildAction === "add_station_to_line" ||
        build.buildAction === "delete");
    if (placingInBuildMode) {
      build.handleBuildPoint(payload.point, payload.stopId);
      return;
    }
    build.selectStop(payload.stopId);
  }

  function handleLineAction(payload: { lineId: string }) {
    const drawingLineInBuildMode =
      build.workspaceMode === "build" &&
      (build.buildAction === "start_line" ||
        build.buildAction === "add_station_to_line" ||
        build.buildAction === "place_station" ||
        build.buildAction === "delete");
    if (drawingLineInBuildMode) {
      return;
    }
    build.selectLine(payload.lineId);
  }

  function handleMapPointAction(point: { lng: number; lat: number; x: number; y: number }) {
    if (build.workspaceMode !== "build") return;
    build.handleBuildPoint(point);
  }

  function handleMapClearSelection() {
    if (build.workspaceMode === "build" && build.buildAction !== "select") return;
    build.setSelection(null);
  }

  function focusStationById(stopId: string) {
    build.selectStop(stopId);
    setFocusStopRequest((prev) => ({
      stopId,
      token: (prev?.token ?? 0) + 1,
    }));
  }

  function createInterchangeGroupForSelectedStation() {
    if (build.selection?.kind !== "stop") return;
    const stop = build.selectedStop;
    const safeLabel = (stop?.name ?? "hub")
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 24);
    const suffix = Math.random().toString(36).slice(2, 7);
    const interchangeId = `interchange:${safeLabel || "hub"}:${suffix}`;
    build.updateSelectedStationInterchange(interchangeId);
  }

  function clearSelectedStationInterchange() {
    if (build.selection?.kind !== "stop") return;
    build.updateSelectedStationInterchange("");
  }

  function applySuggestedInterchange(interchangeId: string) {
    if (build.selection?.kind !== "stop") return;
    build.updateSelectedStationInterchange(interchangeId);
  }

  function focusVehicleFromFleet(vehicleId: string) {
    setFocusVehicleRequest((prev) => ({
      vehicleId,
      token: (prev?.token ?? 0) + 1,
    }));
  }

  async function expediteFleetDelivery(delivery: {
    id: string;
    orderId: string;
    label: string;
    lineId: string;
    lineName: string;
  }) {
    if (!bundle || !delivery.orderId.trim()) return;
    const confirmed = window.confirm(
      `Expedite ${delivery.label} on ${delivery.lineName}? This delivers immediately for a premium cost.`
    );
    if (!confirmed) return;
    const response = await withBusy(async () => {
      const result = (await invoke("expedite_fleet_delivery", {
        projectPath: bundle.project_path,
        lineId: delivery.lineId,
        orderId: delivery.orderId,
      })) as FleetDeliveryExpediteResult;
      const reopened = (await invoke("open_project", {
        projectPath: bundle.project_path,
      })) as OpenSessionResult;
      return { result, reopened };
    });
    if (!response) return;
    applyOpenedSession(response.reopened);
    const costBaseLabel = Math.round(Math.max(response.result.expedite_cost_base, 0)).toLocaleString();
    setSaveStatus(
      `Expedited ${delivery.label} on ${delivery.lineName} for ${costBaseLabel} base cost.`
    );
  }

  function handleScrapVehicleFromMap(vehicleId: string) {
    if (build.workspaceMode !== "build") return;
    const vehicle = runtimeTrains.find((item) => item.train_id === vehicleId) ?? null;
    const lineId = build.selection?.kind === "line" ? build.selection.lineId : vehicle?.line_id ?? null;
    if (!lineId) {
      build.setBuilderError("Select a line before scrapping vehicles.");
      return;
    }
    const unitLabel = unitLabelForMode(vehicle?.mode ?? selectedLineBuildPreset?.engine_mode ?? null);
    const confirmed = window.confirm(`Scrap ${unitLabel.toLowerCase()} from this line?`);
    if (!confirmed) return;
    const ok = build.scrapLineVehicle(lineId);
    if (ok) {
      setFocusVehicleRequest((prev) =>
        prev?.vehicleId === vehicleId ? null : prev
      );
    }
  }

  function requestDeleteSelectedLine() {
    if (!selectedLineDetail) return;
    setLineDeleteDialogOpen(true);
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }

  function cancelDeleteSelectedLine() {
    setLineDeleteDialogOpen(false);
  }

  function deleteSelectedLineWithScrap() {
    const ok = build.deleteSelectedLineWithDisposition("scrap");
    if (!ok) return;
    setLineDeleteDialogOpen(false);
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }

  function deleteSelectedLineWithTransfer(targetLineId: string) {
    const ok = build.deleteSelectedLineWithDisposition("transfer", targetLineId);
    if (!ok) return;
    setLineDeleteDialogOpen(false);
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }

  function openSettingsPanel() {
    if (build.workspaceMode === "build") return;
    setShowSettings(true);
    setShowAlerts(false);
    setShowMenu(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setCommandPaletteOpen(false);
  }

  function toggleFiltersPanel() {
    if (build.workspaceMode === "build") return;
    setShowFilters((prev) => !prev);
    setShowAlerts(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }

  function toggleMissionsPanel() {
    if (build.workspaceMode === "build" || sessionKind !== "game") return;
    setShowMissions((prev) => !prev);
    setShowAlerts(false);
    setShowFilters(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }

  function toggleCountryInfoPanel() {
    if (build.workspaceMode === "build" || sessionKind !== "game") return;
    setShowCountryInfo((prev) => !prev);
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }

  function toggleFarePanel() {
    if (build.workspaceMode === "build" || sessionKind !== "game") return;
    setShowFares((prev) => !prev);
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }

  function toggleAlertsPanel() {
    if (build.workspaceMode === "build") return;
    setShowAlerts((prev) => !prev);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }

  function dismissAlert(alertId: string) {
    setDismissedAlertIds((previous) => (previous.includes(alertId) ? previous : [...previous, alertId]));
    if (alertId === "runtime-error") {
      setError(null);
    }
    if (alertId === "save-status") {
      setSaveStatus("");
    }
  }

  function navigateFromAlert(alert: AlertItem) {
    setShowAlerts(false);
    const target = alert.target;
    if (!target?.id) return;
    if (target.kind === "line") {
      build.selectLine(target.id);
      return;
    }
    if (target.kind === "stop") {
      focusStationById(target.id);
      return;
    }
    if (target.kind === "region") {
      selectCounty(target.id);
      return;
    }
  }

  function updateFarePolicy(patch: Partial<FarePolicyManifest>) {
    if (!bundle || sessionKind !== "game") return;
    void invoke("set_fare_policy", {
      projectPath: bundle.project_path,
      policyPatch: patch,
    })
      .then((res) => {
        const updated = res as FarePolicyManifest;
        setFarePolicy(updated);
        setBundle((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            manifest: {
              ...prev.manifest,
              economy: prev.manifest.economy
                ? {
                    ...prev.manifest.economy,
                    fare_policy: updated,
                  }
                : prev.manifest.economy,
            },
          };
        });
      })
      .catch((e) => setError(String(e)));
  }

  const startCenter = useMemo<[number, number] | null>(() => {
    if (bundle?.start_location) {
      return [bundle.start_location.city_lon, bundle.start_location.city_lat];
    }
    if (bundle?.manifest.start_location) {
      return [bundle.manifest.start_location.city_lon, bundle.manifest.start_location.city_lat];
    }
    return null;
  }, [
    bundle?.start_location?.city_lon,
    bundle?.start_location?.city_lat,
    bundle?.manifest.start_location?.city_lon,
    bundle?.manifest.start_location?.city_lat,
  ]);
  const mapComplexityScore =
    (activeScenario?.world.links.length ?? 0) +
    (activeScenario?.world.transfers.length ?? 0) +
    (activeScenario?.world.stops.length ?? 0) +
    (activeScenario?.world.zones.length ?? 0);
  const inSession = route === "session_game" || route === "session_scenario";
  const inGame = route === "session_game";
  const inViewMode = build.workspaceMode === "view";
  const canRun = Boolean(bundle && inSession && clock);
  const onboardingStepInfo =
    onboardingStep >= 0 && onboardingStep < ONBOARDING_STEPS.length
      ? ONBOARDING_STEPS[onboardingStep]
      : ONBOARDING_STEPS[0];
  const commandActions: CommandAction[] = [
      {
        id: "save",
        label: "Quick Save",
        detail: "Save the active session",
        shortcut: "Ctrl/Cmd+S",
        disabled: !canRun,
        run: () => {
          void saveSession();
        },
      },
      {
        id: "save_quit",
        label: "Save and Quit",
        detail: "Return to main menu",
        shortcut: "Ctrl/Cmd+Shift+S",
        disabled: !canRun,
        run: () => {
          void saveQuit();
        },
      },
      {
        id: "toggle_running",
        label: clock?.running ? "Pause Simulation" : "Start Simulation",
        detail: "Toggle simulation runtime",
        shortcut: "Space",
        disabled: !canRun || build.workspaceMode === "build",
        run: () => {
          if (!clock) return;
          void setRunning(!clock.running);
        },
      },
      {
        id: "speed_1x",
        label: "Set Speed 1x",
        shortcut: "1",
        disabled: !canRun || build.workspaceMode === "build",
        run: () => {
          void setSpeed(1);
        },
      },
      {
        id: "speed_2x",
        label: "Set Speed 2x",
        shortcut: "2",
        disabled: !canRun || build.workspaceMode === "build",
        run: () => {
          void setSpeed(2);
        },
      },
      {
        id: "speed_4x",
        label: "Set Speed 4x",
        shortcut: "3",
        disabled: !canRun || build.workspaceMode === "build",
        run: () => {
          void setSpeed(4);
        },
      },
      {
        id: "enter_build",
        label: "Enter Build Mode",
        shortcut: "B",
        disabled: !inSession || build.workspaceMode === "build",
        run: () => {
          build.enterBuildMode();
        },
      },
      {
        id: "exit_build",
        label: "Exit Build Mode",
        shortcut: "V",
        disabled: !inSession || build.workspaceMode !== "build",
        run: () => {
          leaveBuildMode();
        },
      },
      {
        id: "open_filters",
        label: "Toggle Map Filters",
        detail: "Show or hide map layer controls",
        disabled: !inSession || !inViewMode,
        run: () => {
          toggleFiltersPanel();
        },
      },
      {
        id: "open_counties",
        label: "Toggle County Info",
        detail: "Open county progression panel",
        disabled: !inGame || !inViewMode,
        run: () => {
          toggleCountryInfoPanel();
        },
      },
      {
        id: "open_alerts",
        label: "Open Alerts Center",
        detail: "Review grouped alerts and jump to affected lines/stations",
        disabled: !inSession || build.workspaceMode === "build",
        run: () => {
          toggleAlertsPanel();
        },
      },
      {
        id: "open_settings",
        label: "Open Settings",
        detail: "Display, accessibility, and diagnostics",
        shortcut: "Ctrl/Cmd+,",
        disabled: !inSession || build.workspaceMode === "build",
        run: () => {
          openSettingsPanel();
        },
      },
    ];

  function runPaletteCommand(commandId: string) {
    const command = commandActions.find((item) => item.id === commandId);
    if (!command || command.disabled) return;
    command.run();
    setCommandPaletteOpen(false);
    setCommandPaletteQuery("");
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTextInputLike(event.target)) return;
      const inSession = route === "session_game" || route === "session_scenario";
      const isMeta = event.metaKey || event.ctrlKey;
      if (isMeta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (!inSession) return;
        setShowAlerts(false);
        setCommandPaletteOpen(true);
        setCommandPaletteQuery("");
        return;
      }
      if (isMeta && event.key === ",") {
        event.preventDefault();
        if (!inSession || build.workspaceMode === "build") return;
        openSettingsPanel();
        return;
      }
      if (isMeta && event.shiftKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (!inSession) return;
        void saveQuit();
        return;
      }
      if (isMeta && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (!inSession) return;
        void saveSession();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        if (commandPaletteOpen) {
          setCommandPaletteOpen(false);
          return;
        }
        if (showSettings) {
          setShowSettings(false);
          return;
        }
        if (showAlerts) {
          setShowAlerts(false);
          return;
        }
        if (showFinancialDashboard) {
          setShowFinancialDashboard(false);
          return;
        }
        if (rollingStockEditorOpen) {
          setRollingStockEditorOpen(false);
          return;
        }
        if (scheduleEditorOpen) {
          setScheduleEditorOpen(false);
          return;
        }
        if (lineDeleteDialogOpen) {
          setLineDeleteDialogOpen(false);
          return;
        }
        if (showMenu) {
          setShowMenu(false);
          return;
        }
        if (showFilters) {
          setShowFilters(false);
          return;
        }
        if (showMissions) {
          setShowMissions(false);
          return;
        }
        if (showCountryInfo) {
          setShowCountryInfo(false);
          return;
        }
        if (showFares) {
          setShowFares(false);
        }
        return;
      }
      if (!inSession) return;
      if (event.key === " " && build.workspaceMode !== "build" && clock) {
        event.preventDefault();
        void setRunning(!clock.running);
        return;
      }
      if (event.key === "1" && build.workspaceMode !== "build") {
        event.preventDefault();
        void setSpeed(1);
        return;
      }
      if (event.key === "2" && build.workspaceMode !== "build") {
        event.preventDefault();
        void setSpeed(2);
        return;
      }
      if (event.key === "3" && build.workspaceMode !== "build") {
        event.preventDefault();
        void setSpeed(4);
        return;
      }
      if (event.key.toLowerCase() === "b" && build.workspaceMode !== "build") {
        event.preventDefault();
        build.enterBuildMode();
        return;
      }
      if (event.key.toLowerCase() === "v" && build.workspaceMode === "build") {
        event.preventDefault();
        leaveBuildMode();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    build,
    clock,
    commandPaletteOpen,
    lineDeleteDialogOpen,
    rollingStockEditorOpen,
    route,
    scheduleEditorOpen,
    showCountryInfo,
    showFares,
    showFilters,
    showAlerts,
    showFinancialDashboard,
    showMenu,
    showMissions,
    showSettings,
  ]);

  if (route === "home") {
    return (
      <HomeScreen
        onContinueGame={continueLatestGame}
        onOpenRecentGame={loadGameSave}
        onLoadGame={() => setRoute("load_game")}
        onNewGame={() => {
          setNewGameStep(1);
          setNewGameIntent("balanced");
          setBudgetEdited(false);
          setNewGameBudget(String(defaultBudgetFor(newGameDifficulty, newGameCurrency)));
          setRoute("new_game");
        }}
        onNewScenario={() => setRoute("new_scenario")}
        onLoadScenario={() => setRoute("load_scenario")}
        canContinue={canContinue}
        latestGameName={latestGameSave?.name ?? null}
        latestGameOpenedAt={latestGameSave?.last_opened_at ?? null}
        recentGames={gameSaves}
      />
    );
  }

  if (route === "new_game") {
    return (
      <NewGameScreen
        step={newGameStep}
        gameName={newGameName}
        modeIntent={newGameIntent}
        difficulty={newGameDifficulty}
        difficultyProfile={selectedDifficultyProfile}
        currency={newGameCurrency}
        budget={newGameBudget}
        countries={countries}
        countryPacks={countryPacks}
        selectedCountryIso2={selectedCountryIso2}
        selectedCityId={selectedCityId}
        selectedCountryName={selectedCountry?.name ?? null}
        selectedCityName={selectedCity?.name ?? null}
        citySearch={citySearch}
        filteredCities={filteredCities}
        busy={busy}
        error={error}
        onBack={() => setRoute("home")}
        onNext={nextGameStep}
        onPrev={() =>
          setNewGameStep((s) => {
            if (s === 4) return 3;
            if (s === 3) return 2;
            return 1;
          })
        }
        onCreate={createGame}
        onNameChange={setNewGameName}
        onModeIntentChange={setNewGameIntent}
        onDifficultyChange={(v) => {
          setBudgetEdited(false);
          setNewGameDifficulty(v);
        }}
        onCurrencyChange={(v) => {
          setBudgetEdited(false);
          setNewGameCurrency(v);
        }}
        onBudgetChange={(v) => {
          setBudgetEdited(true);
          setNewGameBudget(v);
        }}
        onCountryChange={onCountryChanged}
        onInstallPack={installCountryPack}
        onUninstallPack={uninstallCountryPack}
        onCitySearchChange={setCitySearch}
        onCitySelect={setSelectedCityId}
      />
    );
  }

  if (route === "load_game") {
    return (
      <LoadGameScreen
        saves={gameSaves}
        deleted={deletedSaves.filter((d) => d.session_kind === "game")}
        onBack={() => setRoute("home")}
        onOpen={loadGameSave}
        onDelete={deleteSave}
        onRestore={restoreDeletedSave}
        onPurge={purgeDeletedSave}
      />
    );
  }

  if (route === "new_scenario") {
    return (
      <NewScenarioScreen
        scenarioName={scenarioName}
        busy={busy}
        onNameChange={setScenarioName}
        onCreate={createScenario}
        onBack={() => setRoute("home")}
      />
    );
  }

  if (route === "load_scenario") {
    return (
      <LoadScenarioScreen
        saves={scenarioSaves}
        deleted={deletedSaves.filter((d) => d.session_kind === "scenario")}
        busy={busy}
        onBack={() => setRoute("home")}
        onOpen={loadScenarioSave}
        onImport={importScenarioFromPicker}
        onDelete={deleteSave}
        onRestore={restoreDeletedSave}
        onPurge={purgeDeletedSave}
      />
    );
  }

  if (!bundle || !sessionKind || !clock) {
    return <div className="global-error">No active session.</div>;
  }

  const isGame = sessionKind === "game";
  const hasShapeNodeData = Boolean(activeScenario?.world.stops.some((s) => s.stop_type === "shape"));
  const hasZoneCentroidData = (activeScenario?.world.zones?.length ?? 0) > 0;

  return (
    <div className={`session-shell ${build.workspaceMode === "build" ? "is-build-workspace" : ""}`}>
      <SessionHud
        sessionKind={sessionKind}
        projectName={bundle.manifest.name}
        clock={clock}
        budget={liveEconomy?.budget_display ?? bundle.manifest.progress_metrics?.budget ?? null}
        budgetCurrency={bundle.manifest.progress_metrics?.currency ?? "GBP"}
        alerts={visibleAlerts}
        alertsOpen={showAlerts}
        buildModeActive={build.workspaceMode === "build"}
        buildTransportLabel={currentBuildPreset?.label ?? null}
        menuOpen={showMenu}
        fleetDeliveries={fleetDeliveries}
        onMenuToggle={() => {
          setShowMenu((v) => !v);
          setShowSettings(false);
          setShowAlerts(false);
          setCommandPaletteOpen(false);
        }}
        onOpenFinancialDashboard={() => {
          setShowFinancialDashboard(true);
          void refreshFinancialDashboard();
        }}
        onFocusLineFromFleet={(lineId) => {
          build.selectLine(lineId);
          setRollingStockEditorOpen(false);
          setScheduleEditorOpen(false);
        }}
        onFocusVehicleFromFleet={(vehicleId) => {
          setFocusVehicleRequest({ vehicleId, token: Date.now() });
        }}
        onExpediteFleetDelivery={expediteFleetDelivery}
        onSave={saveSession}
        onSaveQuit={saveQuit}
        onOpenSettings={openSettingsPanel}
        onOpenCommandPalette={() => {
          if (build.workspaceMode === "build") return;
          setShowMenu(false);
          setShowSettings(false);
          setShowAlerts(false);
          setCommandPaletteQuery("");
          setCommandPaletteOpen(true);
        }}
        onAlertsToggle={toggleAlertsPanel}
        onToggleRunning={setRunning}
        onSpeedChange={setSpeed}
      />
      {build.workspaceMode === "view" ? (
        <SessionSideHud
          sessionKind={sessionKind}
          buildModeActive={false}
          filtersOpen={showFilters}
          missionsOpen={showMissions}
          countryInfoOpen={showCountryInfo}
          faresOpen={showFares}
          onEnterBuildMode={build.enterBuildMode}
          onExitBuildMode={leaveBuildMode}
          onOpenFilters={toggleFiltersPanel}
          onOpenMissions={toggleMissionsPanel}
          onOpenCountryInfo={toggleCountryInfoPanel}
          onOpenFares={toggleFarePanel}
        />
      ) : null}

      <main className="session-main">
        <MapCanvas
          instanceToken={mapInstanceToken}
          scenario={activeScenario}
          projectPath={bundle.project_path}
          mapRuntimeConfig={mapRuntimeConfig}
          clock={clock}
          showShapeStops={showShapeStops}
          showZoneCentroids={showZoneCentroids}
          showStations={showStations}
          showLinks={showLinks}
          linkMode={linkMode}
          startCenter={startCenter}
          serviceLoadByServiceId={serviceLoadByServiceId}
          runtimeTrains={runtimeTrains}
          trainsAuthoritative={trainsAuthoritative}
          sessionKind={sessionKind}
          visibleCountryIso2={visibleCountryIso2}
          regions={regions}
          focusRegionId={focusRegionId}
          activeRegionIds={activeRegionIds}
          selectedRegionId={selectedRegionId}
          interactionMode={build.workspaceMode}
          buildAction={build.buildAction}
          buildConstraintMode={buildConstraintMode}
          selectedStopId={build.selection?.kind === "stop" ? build.selection.stopId : null}
          selectedLineId={build.selection?.kind === "line" ? build.selection.lineId : null}
          activeLineId={build.activeLine?.lineId ?? null}
          focusStopId={focusStopRequest?.stopId ?? null}
          focusStopToken={focusStopRequest?.token ?? 0}
          focusVehicleId={focusVehicleRequest?.vehicleId ?? null}
          focusVehicleToken={focusVehicleRequest?.token ?? 0}
          previewAnchorPoint={previewAnchorPoint}
          previewColor={previewColor}
          onBootProgress={handleMapBootProgress}
          onSelectCounty={selectCounty}
          onStopAction={handleStopAction}
          onLineAction={handleLineAction}
          onMapPointAction={handleMapPointAction}
          onClearSelection={handleMapClearSelection}
          onScrapVehicle={build.workspaceMode === "build" ? handleScrapVehicleFromMap : undefined}
        />

        {!isGame && build.workspaceMode === "view" && (
          <aside className="scenario-panel">
            <h4>Scenario Controls</h4>
            <label>Seed</label>
            <input
              value={runConfig.deterministic_seed ?? ""}
              onChange={(e) =>
                setRunConfig((prev) => ({
                  ...prev,
                  deterministic_seed: e.target.value ? Number(e.target.value) : null,
                }))
              }
            />
            <label>Horizon (s)</label>
            <input
              value={runConfig.horizon_s ?? ""}
              onChange={(e) =>
                setRunConfig((prev) => ({ ...prev, horizon_s: e.target.value ? Number(e.target.value) : null }))
              }
            />
            <label>Time Bin (s)</label>
            <input
              value={runConfig.time_bin_s ?? ""}
              onChange={(e) =>
                setRunConfig((prev) => ({ ...prev, time_bin_s: e.target.value ? Number(e.target.value) : null }))
              }
            />
            <div className="action-row">
              <button onClick={runPlanning} disabled={busy}>
                Run Planning
              </button>
              <button onClick={rebuildDemandForUnlocked} disabled={busy}>
                Rebuild Demand
              </button>
            </div>
            <div className="scenario-runs">
              {bundle.runs.map((r) => (
                <div key={r.run_id} className="run-chip">
                  <span>{r.run_id}</span>
                  <button onClick={() => exportRunCsv(r.run_id)}>CSV</button>
                  <button onClick={() => exportRunJson(r.run_id)}>JSON</button>
                </div>
              ))}
            </div>
            <label>Baseline</label>
            <select value={selectedBaseRun} onChange={(e) => setSelectedBaseRun(e.target.value)}>
              <option value="">Select run</option>
              {bundle.runs.map((r) => (
                <option key={r.run_id} value={r.run_id}>
                  {r.run_id}
                </option>
              ))}
            </select>
            <label>Candidate</label>
            <select value={selectedCandidateRun} onChange={(e) => setSelectedCandidateRun(e.target.value)}>
              <option value="">Select run</option>
              {bundle.runs.map((r) => (
                <option key={r.run_id} value={r.run_id}>
                  {r.run_id}
                </option>
              ))}
            </select>
            <button onClick={compareRuns}>Compare</button>
            {compareResult && (
              <p className="hint-line">
                Delta trips: {compareResult.delta.kpis.total_trips.toFixed(2)} | Delta GC:{" "}
                {compareResult.delta.kpis.mean_generalized_cost_s.toFixed(2)}
              </p>
            )}
          </aside>
        )}

        <LineInspectorSheet
          open={
            build.selection?.kind === "line" || lineDraftMode
          }
          inspection={build.lineInspection}
          lineDetail={selectedLineDetail}
          draftPreview={lineDraftMode ? activeLineDraftPreview : selectedLineDetail ? null : activeLineDraftPreview}
          forceDraftMode={lineDraftMode}
          editable={build.workspaceMode === "build" && Boolean(selectedLineDetail)}
          stationDecorations={selectedLineStationDecorations}
          presets={build.buildDefaults?.presets ?? []}
          selectedPresetId={selectedLinePresetId}
          budgetCurrency={budgetCurrency}
          estimatedCapexBase={selectedLineEstimatedCapexBase}
          stationCapexBase={build.buildDefaults?.station_capex_base ?? null}
          addingStationMode={build.buildAction === "add_station_to_line"}
          onClose={() => build.setSelection(null)}
          onAddStationToLine={() => build.armLineExtension()}
          onDelete={requestDeleteSelectedLine}
          onNameChange={(value) => build.updateSelectedLine({ name: value })}
          onColorChange={(value) => build.updateSelectedLine({ display_color: value })}
          onPresetChange={build.updateSelectedLinePreset}
          onStationClick={(stopId) => {
            focusStationById(stopId);
          }}
          onOpenRollingStockEditor={() => {
            if (!selectedLineBuildPreset) {
              build.setBuilderError("Rolling stock data is unavailable for this line preset.");
              return;
            }
            setRollingStockEditorOpen(true);
            setScheduleEditorOpen(false);
          }}
          onOpenScheduleEditor={() => {
            setScheduleEditorOpen(true);
            setRollingStockEditorOpen(false);
          }}
          onRemoveDraftStation={(stopId) => {
            build.removeStationFromActiveDraft(stopId);
          }}
        />

        <LineDeleteDialog
          open={
            lineDeleteDialogOpen &&
            build.selection?.kind === "line" &&
            Boolean(selectedLineDetail)
          }
          lineName={selectedLineDetail?.name?.trim() ? selectedLineDetail.name : "Untitled Line"}
          unitLabel={selectedLineUnitLabel}
          unitsOwned={selectedLineFleetEditorState?.unitsOwned ?? 0}
          unitsPending={selectedLineFleetEditorState?.unitsPending ?? 0}
          budgetCurrency={budgetCurrency}
          estimatedScrapValueBase={selectedLineScrapEstimateBase}
          transferTargets={selectedLineTransferTargets}
          onCancel={cancelDeleteSelectedLine}
          onConfirmScrap={deleteSelectedLineWithScrap}
          onConfirmTransfer={deleteSelectedLineWithTransfer}
        />

        <RollingStockEditorSheet
          open={
            rollingStockEditorOpen &&
            build.selection?.kind === "line" &&
            Boolean(selectedLineDetail) &&
            Boolean(selectedLineFleetEditorState) &&
            Boolean(selectedLineBuildPreset)
          }
          editable={build.workspaceMode === "build" && !lineDraftMode}
          lineName={selectedLineDetail?.name?.trim() ? selectedLineDetail.name : "Untitled Line"}
          budgetCurrency={budgetCurrency}
          modeId={selectedLineBuildPreset?.engine_mode ?? selectedLineDetail?.mode ?? null}
          preset={selectedLineBuildPreset}
          packageId={selectedLineFleetEditorState?.packageId ?? "standard"}
          unitsOwned={selectedLineFleetEditorState?.unitsOwned ?? 0}
          unitsCommitted={selectedLineFleetEditorState?.unitsCommitted ?? selectedLineFleetEditorState?.unitsOwned ?? 0}
          unitsPending={selectedLineFleetEditorState?.unitsPending ?? 0}
          unitsAssigned={selectedLineFleetEditorState?.unitsAssigned ?? 0}
          carsPerUnit={selectedLineFleetEditorState?.carsPerUnit ?? 1}
          speedLevel={selectedLineFleetEditorState?.speedLevel ?? "balanced"}
          comfortLevel={selectedLineFleetEditorState?.comfortLevel ?? "standard"}
          requiredUnitsNow={selectedLineFleetEditorState?.requiredUnitsNow ?? 0}
          pendingOrders={selectedLineFleetEditorState?.pendingOrders ?? []}
          activeVehicles={selectedLineActiveVehicles}
          currentTickS={clock?.tick_seconds ?? 0}
          clockRunning={clock?.running ?? false}
          clockSpeed={clock?.speed ?? 1}
          onClose={() => setRollingStockEditorOpen(false)}
          onSave={(patch) => build.updateSelectedLineOperations({ fleet: patch })}
          onFocusVehicle={focusVehicleFromFleet}
        />

        <ScheduleEditorSheet
          open={
            scheduleEditorOpen &&
            build.selection?.kind === "line" &&
            Boolean(selectedLineDetail) &&
            Boolean(selectedLineScheduleState)
          }
          editable={build.workspaceMode === "build" && !lineDraftMode}
          lineName={selectedLineDetail?.name?.trim() ? selectedLineDetail.name : "Untitled Line"}
          budgetCurrency={budgetCurrency}
          preset={selectedLineBuildPreset}
          unitsOwned={selectedLineFleetEditorState?.unitsOwned ?? 0}
          roundTripS={selectedLineDetail?.roundTripS ?? 0}
          schedule={
            selectedLineScheduleState ?? {
              peak_start_minute: 420,
              peak_end_minute: 570,
              overnight_start_minute: 0,
              overnight_end_minute: 300,
              tph_peak: 0,
              tph_off_peak: 0,
              tph_overnight: 0,
            }
          }
          onClose={() => setScheduleEditorOpen(false)}
          onOpenRollingStockEditor={() => {
            setScheduleEditorOpen(false);
            setRollingStockEditorOpen(true);
          }}
          onSave={(patch) => build.updateSelectedLineOperations({ schedule: patch })}
        />

        <StationInspectorModal
          open={
            build.selection?.kind === "stop" &&
            !(build.workspaceMode === "build" &&
              (build.buildAction === "start_line" || build.buildAction === "add_station_to_line"))
          }
          stop={build.selectedStop}
          inspection={build.stationInspection}
          localLines={selectedStationLines}
          interchangeMembers={selectedStationInterchangeContext.members}
          suggestedInterchanges={selectedStationInterchangeContext.suggestions}
          transferLinks={selectedStationInterchangeContext.transfers}
          editable={build.workspaceMode === "build"}
          onClose={() => build.setSelection(null)}
          onNameChange={build.renameSelectedStation}
          onInterchangeChange={build.updateSelectedStationInterchange}
          onCreateInterchangeGroup={createInterchangeGroupForSelectedStation}
          onClearInterchangeGroup={clearSelectedStationInterchange}
          onApplySuggestedInterchange={applySuggestedInterchange}
          onSelectLinkedStop={focusStationById}
          onDelete={build.deleteSelectedStation}
        />
      </main>

      {build.workspaceMode === "build" && (
        <BuildPalette
          presets={build.buildDefaults?.presets ?? []}
          lines={build.lineSummaries.map((line) => ({
            lineId: line.lineId,
            name: line.name,
            mode: line.mode,
            modeVariant: line.modeVariant ?? null,
            displayColor: line.displayColor ?? null,
          }))}
          transportPresetId={build.transportPresetId}
          buildAction={build.buildAction}
          hasSelectedLine={build.selection?.kind === "line"}
          hasSelectedStop={build.selection?.kind === "stop"}
          selectedLineId={build.selection?.kind === "line" ? build.selection.lineId : null}
          selectedLineName={selectedLineDetail?.name ?? build.lineInspection?.name ?? null}
          selectedStopName={build.selectedStop?.name ?? null}
          selectedLineConstructionCostBase={selectedLineEstimatedCapexBase}
          mutationPreview={build.mutationPreview}
          isDirty={build.isDirty}
          builderBusy={build.builderBusy}
          builderError={build.builderError}
          budgetCurrency={budgetCurrency}
          stationCostBase={stationCostBase}
          lineCostPerKmBase={lineCostPerKmBase}
          extensionAddedStations={extensionAddedStations}
          extensionAddedLengthM={extensionAddedLengthM}
          extensionConstructionCostBase={extensionConstructionCostBase}
          activeLineStopCount={build.activeLine?.stationIds.length ?? 0}
          canUndoDraftPlacement={(build.activeLine?.stationIds.length ?? 0) > 1}
          onExitBuildMode={leaveBuildMode}
          onSelectBuildAction={build.selectBuildAction}
          onArmLineExtension={() => build.armLineExtension()}
          onTransportPresetChange={build.setTransportPresetId}
          onSelectLine={build.selectLine}
          onApplyDraft={() => build.applyDraft()}
          onFinishLine={build.finishLineDraw}
          onUndoLinePlacement={build.undoActiveLinePlacement}
        />
      )}
      {isGame && build.workspaceMode === "view" && (
        <CountryInfoDrawer
          open={showCountryInfo}
          busy={busy}
          regions={regions}
          selectedRegionId={selectedRegionId}
          focusRegionId={focusRegionId}
          currentBalanceBase={currentBalanceBase}
          onClose={() => setShowCountryInfo(false)}
          onSelectRegion={selectCounty}
          onFocusRegion={focusSelectedCounty}
          onUnlockRegion={unlockAndFocusSelectedCounty}
        />
      )}
      {isGame && build.workspaceMode === "view" && (
        <FarePolicyPanel
          open={showFares}
          busy={busy}
          policy={farePolicy}
          onClose={() => setShowFares(false)}
          onChange={updateFarePolicy}
        />
      )}

      {build.workspaceMode === "view" && (
        <MapFiltersPanel
          open={showFilters}
          onClose={() => setShowFilters(false)}
          showStations={showStations}
          showLinks={showLinks}
          showZoneCentroids={showZoneCentroids}
          showShapeStops={showShapeStops}
          hasZoneCentroidData={hasZoneCentroidData}
          hasShapeNodeData={hasShapeNodeData}
          linkMode={linkMode}
          onShowStationsChange={setShowStations}
          onShowLinksChange={setShowLinks}
          onShowZoneCentroidsChange={setShowZoneCentroids}
          onShowShapeStopsChange={setShowShapeStops}
          onLinkModeChange={setLinkMode}
        />
      )}
      {build.workspaceMode === "view" && (
        <MissionsDrawer open={showMissions} missions={MISSIONS} onClose={() => setShowMissions(false)} />
      )}

      <AlertsCenter
        open={showAlerts}
        alerts={visibleAlerts}
        onClose={() => setShowAlerts(false)}
        onNavigate={navigateFromAlert}
        onDismiss={dismissAlert}
      />

      <SettingsPanel
        open={showSettings}
        settings={uiSettings}
        onClose={() => setShowSettings(false)}
        onChange={setUiSettings}
        onReset={() => setUiSettings(DEFAULT_UI_SETTINGS)}
      />

      <CommandPalette
        open={commandPaletteOpen}
        query={commandPaletteQuery}
        commands={commandActions.map((command) => ({
          id: command.id,
          label: command.label,
          detail: command.detail,
          shortcut: command.shortcut,
          disabled: command.disabled,
        }))}
        onQueryChange={setCommandPaletteQuery}
        onRun={runPaletteCommand}
        onClose={() => setCommandPaletteOpen(false)}
      />

      <DiagnosticsOverlay
        open={uiSettings.showDiagnostics && (route === "session_game" || route === "session_scenario")}
        fps={fps}
        frameMs={frameMs}
        telemetry={runtimeTelemetry}
        snapshotLatencyMs={snapshotLatencyMs}
        mapComplexityScore={mapComplexityScore}
      />

      {busy ? (
        <div className="app-status-overlay">
          <div className="app-status-card">
            <strong>Working...</strong>
            <span>Preparing data and applying your request.</span>
          </div>
        </div>
      ) : null}

      {showSessionBootOverlay ? (
        <div className="session-boot-overlay">
          <div className="session-boot-card">
            <strong>{sessionBootState.stage === "error" ? "Load issue" : "Loading session"}</strong>
            <span>{sessionBootState.message || "Preparing map and runtime state..."}</span>
            <div className="session-boot-progress">
              <div
                className="session-boot-progress-fill"
                style={{ width: `${Math.max(Math.min(sessionBootState.progress, 1), 0) * 100}%` }}
              />
            </div>
            {sessionBootState.error ? <p className="form-error">{sessionBootState.error}</p> : null}
            {sessionBootState.stage === "error" ? (
              <div className="session-boot-actions">
                <button onClick={retryMapLoad}>Retry Map Load</button>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {isOffline ? <div className="offline-banner">Offline: cloud-dependent features are temporarily unavailable.</div> : null}

      {!clock.running && build.workspaceMode !== "build" ? (
        <div className="paused-banner">Simulation Paused</div>
      ) : null}

      {saveStatus.trim() ? (
        <div className="status-toast">
          <span>{saveStatus}</span>
          <button onClick={() => setSaveStatus("")}>Dismiss</button>
        </div>
      ) : null}

      {onboardingActive && route === "session_game" ? (
        <aside className="onboarding-card">
          <p>Quick Start Guide</p>
          <strong>
            Step {Math.min(onboardingStep + 1, ONBOARDING_STEPS.length)} / {ONBOARDING_STEPS.length}:{" "}
            {onboardingStepInfo.title}
          </strong>
          <span>{onboardingStepInfo.description}</span>
          <div className="onboarding-actions">
            <button
              onClick={() => {
                setOnboardingActive(false);
                if (typeof window !== "undefined") {
                  window.localStorage.setItem(ONBOARDING_STORAGE_KEY, "done");
                }
              }}
            >
              Skip Guide
            </button>
            <button
              className="primary"
              onClick={() => setOnboardingStep((prev) => Math.min(prev + 1, ONBOARDING_STEPS.length - 1))}
            >
              Next Tip
            </button>
          </div>
        </aside>
      ) : null}

      {demandWarning && <div className="global-error">{demandWarning}</div>}
      <FinancialDashboardModal
        open={showFinancialDashboard}
        busy={financialBusy}
        error={financialError}
        currency={budgetCurrency}
        request={financialRequest}
        data={financialData}
        regions={regions}
        lineOptions={financialLineOptions}
        onRequestChange={(patch) =>
          setFinancialRequest((prev) => ({
            ...prev,
            ...patch,
          }))
        }
        onRefresh={refreshFinancialDashboard}
        onClose={() => setShowFinancialDashboard(false)}
      />
      {error ? (
        <div className="global-error global-error-floating">
          <span>{error}</span>
          <button onClick={() => setError(null)}>Dismiss</button>
        </div>
      ) : null}
    </div>
  );
}
