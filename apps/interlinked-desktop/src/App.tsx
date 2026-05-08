import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  AlertItem,
  AppRoute,
  CityOption,
  CountryPackStatus,
  CountryOption,
  CurrencyCode,
  DemandCoverageMeta,
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  Difficulty,
  FarePolicyManifest,
  Mission,
  OpenSessionResult,
  MapRuntimeConfig,
  RegionStatus,
  ScenarioLite,
  SimulationAdvanceEconomy,
  SimulationClock,
  RuntimePerfTelemetry,
  RuntimeTemporalDiagnostics,
  LineOpsRuntimeView,
  StationRuntimeView,
  TrainRuntimeView,
  DifficultyProfile,
  DemandOverlayPayload,
  DemandOverlayType,
} from "./types";
import {
  getBuildPreset,
  stopDisplayName,
} from "./build/helpers";
import { type FocusStopRequest, useMapBuildInteractions } from "./app/useMapBuildInteractions";
import {
  useInspectorPanelController,
  type FocusVehicleRequest,
} from "./app/useInspectorPanelController";
import { useBuildController } from "./build/useBuildController";
import { useRuntimePolling } from "./app/useRuntimePolling";
import {
  useShellCommandOrchestration,
  useShellPanelsController,
} from "./app/useShellPanelsController";
import { useScenarioSidebarController } from "./app/useScenarioSidebarController";
import { useSaveBrowserController } from "./app/useSaveBrowserController";
import {
  DEFAULT_UI_SETTINGS,
  useShellStatusOrchestration,
} from "./app/useShellStatusOrchestration";
import { useNewGameFlowController } from "./app/useNewGameFlowController";
import {
  useSessionController,
} from "./app/useSessionController";
import { useSessionLifecycleController } from "./app/session/useSessionLifecycleController";
import type { LinkModeFilter } from "./ui/MapFiltersPanel";
import {
  mergeRuntimeVehicleInspection,
  vehicleInspectionFromRuntimeTrain,
  type VehicleInspection,
} from "./app/vehicleInspection";
import { getDemandOverlayPayload } from "./api/desktopApi";
import AppRouteScreens from "./ui/AppRouteScreens";
import AppSessionShell from "./ui/AppSessionShell";
import SettingsPanel from "./ui/SettingsPanel";
import { buildPerfEvent } from "./perf/buildPerf";

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

function formatBackendError(error: unknown): string {
  const raw = String(error ?? "Unknown error");
  const message = raw.replace(/^Error:\s*/i, "");
  if (message.includes("RegionNotAdjacent")) {
    return "Unlock failed: the selected region must border one of your unlocked regions.";
  }
  if (message.includes("InsufficientFunds")) {
    return "Unlock failed: insufficient funds for this region.";
  }
  if (message.includes("WrongCountryScope")) {
    return "Unlock failed: this region is outside your current country scope.";
  }
  if (message.includes("CountryPackMissing")) {
    return "Unlock failed: required country data is not installed.";
  }
  return message;
}

const GAME_SURFACE_ROUTES: ReadonlySet<AppRoute> = new Set([
  "session_game",
  "session_scenario",
]);

function isEditableSurfaceTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest(
      'input, textarea, select, [contenteditable="true"], [contenteditable="plaintext-only"]'
    )
  );
}

function allowsScrollOverflow(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  return normalized === "auto" || normalized === "scroll" || normalized === "overlay";
}

function canConsumeScrollDelta(target: EventTarget | null, deltaX: number, deltaY: number): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest(".maplibregl-canvas-container, .maplibregl-canvas")) return true;

  let node: Element | null = target;
  while (node && node !== document.body) {
    if (node instanceof HTMLElement) {
      const style = window.getComputedStyle(node);
      const canScrollY =
        allowsScrollOverflow(style.overflowY) && node.scrollHeight > node.clientHeight + 1;
      if (canScrollY) {
        const maxY = node.scrollHeight - node.clientHeight;
        if ((deltaY < 0 && node.scrollTop > 0) || (deltaY > 0 && node.scrollTop < maxY)) {
          return true;
        }
      }
      const canScrollX =
        allowsScrollOverflow(style.overflowX) && node.scrollWidth > node.clientWidth + 1;
      if (canScrollX) {
        const maxX = node.scrollWidth - node.clientWidth;
        if ((deltaX < 0 && node.scrollLeft > 0) || (deltaX > 0 && node.scrollLeft < maxX)) {
          return true;
        }
      }
    }
    node = node.parentElement;
  }
  return false;
}

export default function App() {
  const [route, setRoute] = useState<AppRoute>("home");
  const [bundle, setBundle] = useState<OpenSessionResult | null>(null);
  const [clock, setClock] = useState<SimulationClock | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [demandWarning, setDemandWarning] = useState<string | null>(null);
  const [saveStatus, setSaveStatus] = useState("");
  const saveBrowser = useSaveBrowserController();
  const [countries, setCountries] = useState<CountryOption[]>([]);
  const [countryPacks, setCountryPacks] = useState<CountryPackStatus[]>([]);
  const [cities, setCities] = useState<CityOption[]>([]);
  const [demandCoverage, setDemandCoverage] = useState<DemandCoverageMeta[]>([]);
  const [regions, setRegions] = useState<RegionStatus[]>([]);
  const [focusRegionId, setFocusRegionId] = useState<string | null>(null);
  const [selectedRegionId, setSelectedRegionId] = useState<string | null>(null);
  const [buildExitConfirmOpen, setBuildExitConfirmOpen] = useState(false);
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

  const [selectedCountryIso2, setSelectedCountryIso2] = useState("");
  const [citySearch, setCitySearch] = useState("");
  const [selectedCityId, setSelectedCityId] = useState<number | null>(null);
  const [scenarioName, setScenarioName] = useState("Interlinked Scenario");
  const [homeSettingsOpen, setHomeSettingsOpen] = useState(false);

  const [showShapeStops, setShowShapeStops] = useState(false);
  const [showZoneCentroids, setShowZoneCentroids] = useState(false);
  const [showStations, setShowStations] = useState(true);
  const [showLinks, setShowLinks] = useState(true);
  const [linkMode, setLinkMode] = useState<LinkModeFilter>("all");
  const [showDemandOverlay, setShowDemandOverlay] = useState(false);
  const [demandOverlayType, setDemandOverlayType] = useState<DemandOverlayType>(
    "total_allocation"
  );
  const [demandOverlayPayload, setDemandOverlayPayload] = useState<DemandOverlayPayload | null>(
    null
  );
  const [demandOverlayLoading, setDemandOverlayLoading] = useState(false);
  const [demandOverlayStatusMessage, setDemandOverlayStatusMessage] = useState<string | null>(
    null
  );
  const [financialRequest, setFinancialRequest] = useState<FinancialDashboardRequest>({
    granularity: "month",
    periods: 12,
  });
  const [financialData, setFinancialData] = useState<FinancialDashboardResponse | null>(null);
  const [financialBusy, setFinancialBusy] = useState(false);
  const [financialError, setFinancialError] = useState<string | null>(null);
  const [focusStopRequest, setFocusStopRequest] = useState<FocusStopRequest>(null);
  const [focusVehicleRequest, setFocusVehicleRequest] = useState<FocusVehicleRequest>(null);
  const [selectedVehicleSnapshot, setSelectedVehicleSnapshot] = useState<VehicleInspection | null>(null);
  const [runtimeTelemetry, setRuntimeTelemetry] = useState<RuntimePerfTelemetry | null>(null);
  const [snapshotLatencyMs, setSnapshotLatencyMs] = useState<number | null>(null);
  const [temporalDiagnostics, setTemporalDiagnostics] = useState<RuntimeTemporalDiagnostics>({
    last_fast_snapshot_interval_ms: null,
    stale_fast_snapshots_rejected: 0,
    latest_fast_clock_revision: 0,
    latest_fast_tick_index: 0,
  });
  const [runtimeStations, setRuntimeStations] = useState<StationRuntimeView[]>([]);
  const [runtimeLineOps, setRuntimeLineOps] = useState<LineOpsRuntimeView[]>([]);
  const [mapInstanceToken, setMapInstanceToken] = useState(0);
  const lifecycle = useSessionLifecycleController({ route });

  useEffect(() => {
    const body = document.body;
    const applyDesktopSurfacePolicy = GAME_SURFACE_ROUTES.has(route);

    if (!applyDesktopSurfacePolicy) {
      body.classList.remove("il-noneditable-surface");
      return;
    }

    body.classList.add("il-noneditable-surface");
    const onContextMenu = (event: MouseEvent) => {
      if (isEditableSurfaceTarget(event.target)) return;
      event.preventDefault();
    };
    const onWheel = (event: WheelEvent) => {
      if (isEditableSurfaceTarget(event.target)) return;
      if (canConsumeScrollDelta(event.target, event.deltaX, event.deltaY)) return;
      event.preventDefault();
    };

    document.addEventListener("contextmenu", onContextMenu, true);
    document.addEventListener("wheel", onWheel, { capture: true, passive: false });
    return () => {
      body.classList.remove("il-noneditable-surface");
      document.removeEventListener("contextmenu", onContextMenu, true);
      document.removeEventListener("wheel", onWheel, true);
    };
  }, [route]);

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
  const shellPanels = useShellPanelsController({
    route,
    workspaceMode: build.workspaceMode,
    buildAction: build.buildAction,
    sessionKind,
  });
  const scenarioSidebar = useScenarioSidebarController();
  const newGame = useNewGameFlowController({
    defaultBudgetFor,
  });
  const activeScenario = build.workingScenario ?? scenario;
  const canContinue = saveBrowser.canContinue;
  const latestGameSave = saveBrowser.continueTarget;
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
    () => DIFFICULTY_PROFILES[newGame.difficulty],
    [newGame.difficulty]
  );
  const activeRegionIds = useMemo(
    () => bundle?.manifest.region_state?.active_region_ids ?? [],
    [bundle?.manifest.region_state?.active_region_ids]
  );
  const currentBalanceBase =
    liveEconomy?.current_balance_base ?? bundle?.manifest.economy?.current_balance_base ?? null;
  const shellStatus = useShellStatusOrchestration({
    route,
    sessionBootState: lifecycle.snapshot.bootState,
    lifecycleError: lifecycle.snapshot.lastError,
    bundleProjectId: bundle?.manifest.project_id,
    clock,
    workspaceMode: build.workspaceMode,
    activeScenario,
    lineSummaries: build.lineSummaries,
    runtimeLineOps,
    runtimeStations,
    runtimeTelemetry,
    currentBalanceBase,
    builderError: build.builderError,
    demandWarning,
    error,
    saveStatus,
    setSaveStatus,
    setError,
  });
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
  const {
    selectedLinePresetId,
    selectedLineDetail,
    selectedBaseLineDetail,
    selectedLineBuildPreset,
    selectedStationLines,
    selectedStationInterchangeContext,
    selectedLineEstimatedCapexBase,
    selectedLineStationDecorations,
    selectedLineScheduleState,
    selectedLineFleetEditorState,
    selectedLineUnitLabel,
    selectedLineActiveVehicles,
    selectedLineTransferTargets,
    selectedLineScrapEstimateBase,
    rollingStockEditorOpen,
    scheduleEditorOpen,
    lineDeleteDialogOpen,
    lineInspectorOpen,
    stationInspectorOpen,
    lineDeleteDialogEnabled,
    rollingStockEditorEnabled,
    scheduleEditorEnabled,
    focusVehicleFromFleet,
    handleScrapVehicleFromMap,
    openRollingStockEditorFromLineInspector,
    openScheduleEditorFromLineInspector,
    openRollingStockEditorFromSchedule,
    closeLineEditors,
    setRollingStockEditorOpen,
    setScheduleEditorOpen,
    requestDeleteSelectedLine,
    cancelDeleteSelectedLine,
    deleteSelectedLineWithScrap,
    deleteSelectedLineWithTransfer,
  } = useInspectorPanelController({
    build,
    activeScenario,
    scenario,
    runtimeTrains,
    setFocusVehicleRequest,
  });

  const selectedVehicleInspection = useMemo(
    () => mergeRuntimeVehicleInspection(selectedVehicleSnapshot, runtimeTrains),
    [runtimeTrains, selectedVehicleSnapshot]
  );

  const clearVehicleInspection = useCallback(() => {
    setSelectedVehicleSnapshot(null);
  }, []);

  const handleVehicleInspection = useCallback(
    (vehicle: VehicleInspection) => {
      build.setSelection(null);
      setSelectedVehicleSnapshot(vehicle);
    },
    [build]
  );

  const focusVehicleInspectionFromFleet = useCallback(
    (vehicleId: string) => {
      const runtimeVehicle = runtimeTrains.find((train) => train.train_id === vehicleId) ?? null;
      if (runtimeVehicle) {
        setSelectedVehicleSnapshot(vehicleInspectionFromRuntimeTrain(runtimeVehicle));
      }
      focusVehicleFromFleet(vehicleId);
    },
    [focusVehicleFromFleet, runtimeTrains]
  );

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
  const currentBuildPreset = useMemo(
    () => getBuildPreset(build.buildDefaults, build.transportPresetId),
    [build.buildDefaults, build.transportPresetId]
  );
  const previewAnchorPoint = useMemo(() => {
    if (!build.activeLine || !activeScenario) return null;
    if (build.buildAction === "add_station_to_line") {
      const anchorStopId = build.activeLine.extensionAnchorStopId ?? null;
      if (!anchorStopId) return null;
      const anchorStop = activeScenario.world.stops.find((stop) => stop.id === anchorStopId);
      if (!anchorStop) return null;
      return { x: anchorStop.x, y: anchorStop.y };
    }
    const lastStopId = build.activeLine.stationIds[build.activeLine.stationIds.length - 1];
    const lastStop = activeScenario.world.stops.find((stop) => stop.id === lastStopId);
    if (!lastStop) return null;
    return { x: lastStop.x, y: lastStop.y };
  }, [activeScenario, build.activeLine, build.buildAction]);
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
  const lineDraftMode =
    build.workspaceMode === "build" &&
    (build.buildAction === "start_line" || build.buildAction === "add_station_to_line") &&
    Boolean(build.activeLine);
  const lineDraftAwaitingTerminus =
    build.workspaceMode === "build" &&
    build.buildAction === "add_station_to_line" &&
    Boolean(build.activeLine) &&
    !build.activeLine?.extensionAnchorStopId;
  const lineDraftAnchorStopName = useMemo(() => {
    const anchorStopId = build.activeLine?.extensionAnchorStopId ?? null;
    if (!anchorStopId || !activeScenario) return null;
    const stop = activeScenario.world.stops.find((candidate) => candidate.id === anchorStopId);
    if (!stop) return null;
    return stopDisplayName(stop);
  }, [activeScenario, build.activeLine?.extensionAnchorStopId]);
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

  useRuntimePolling({
    bundle,
    sessionKind,
    lifecycle,
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
    setTemporalDiagnostics,
    setError,
  });

  const {
    refreshFinancialDashboard,
    onCountryChanged,
    installCountryPack,
    continueLatestGame,
    loadGameSave,
    loadScenarioSave,
    selectCounty,
    unlockAndFocusSelectedCounty,
    deleteSave,
    restoreDeletedSave,
    purgeDeletedSave,
    createGame,
    createScenario,
    importScenarioFromPicker,
    saveSession,
    saveQuit,
    setRunning,
    setSpeed,
    runPlanning,
    exportRunCsv,
    exportRunJson,
    compareRuns,
    rebuildDemandForUnlocked,
    handleMapBootProgress,
    retryMapLoad,
    updateFarePolicy,
    expediteFleetDelivery,
  } = useSessionController({
    route,
    setRoute,
    bundle,
    setBundle,
    sessionKind,
    clock,
    setClock,
    build: {
      workspaceMode: build.workspaceMode,
      isDirty: build.isDirty,
      setBuilderError: build.setBuilderError,
    },
    selectedCountryIso2,
    setSelectedCountryIso2,
    selectedCountry,
    selectedCountryPack,
    selectedCity,
    setSelectedCityId,
    setCitySearch,
    selectedRegionId,
    regions,
    newGameName: newGame.name,
    newGameDifficulty: newGame.difficulty,
    newGameCurrency: newGame.currency,
    newGameBudget: newGame.budget,
    scenarioName,
    runConfig: scenarioSidebar.runConfig,
    selectedBaseRun: scenarioSidebar.selectedBaseRun,
    selectedCandidateRun: scenarioSidebar.selectedCandidateRun,
    setSelectedBaseRun: scenarioSidebar.setSelectedBaseRun,
    financialRequest,
    showFinancialDashboard: shellPanels.showFinancialDashboard,
    lifecycle,
    defaultBudgetFor,
    formatBackendError,
    playUiCue: shellStatus.playUiCue,
    latestClockTickRef,
    latestSnapshotTickRef,
    latestSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
    latestStrategicSnapshotCapturedRef,
    runtimeControlQueueRef,
    setBusy,
    setError,
    setDemandWarning,
    setSaveStatus,
    setSaveLibrary: saveBrowser.setLibrary,
    setCountries,
    setCountryPacks,
    setCities,
    setDemandCoverage,
    setRegions,
    setFocusRegionId,
    setSelectedRegionId,
    setShowCountryInfo: shellPanels.setShowCountryInfo,
    setShowFares: shellPanels.setShowFares,
    setMapRuntimeConfig,
    setFarePolicy,
    setLiveEconomy,
    setServiceLoadByServiceId,
    setRuntimeTrains,
    setRuntimeStations,
    setRuntimeLineOps,
    setTrainsAuthoritative,
    setRuntimeTelemetry,
    setSnapshotLatencyMs,
    setMapInstanceToken,
    setFinancialBusy,
    setFinancialError,
    setFinancialData,
    setCompareResult: scenarioSidebar.setCompareResult,
    setShowMenu: shellPanels.setShowMenu,
  });

  function nextNewGameStep() {
    newGame.nextStep({
      selectedCountry,
      selectedCity,
      selectedCountryPack,
      setError,
    });
  }

  function leaveBuildMode() {
    buildPerfEvent("build.ui.exit_button_pressed", {
      workspaceMode: build.workspaceMode,
      isDirty: build.isDirty,
    });
    if (build.workspaceMode !== "build") return;
    if (build.isDirty) {
      setBuildExitConfirmOpen(true);
      buildPerfEvent("build.ui.exit_confirm_opened");
      return;
    }
    setBuildExitConfirmOpen(false);
    build.cancelBuildMode();
  }

  function cancelLeaveBuildModeConfirm() {
    buildPerfEvent("build.ui.exit_confirm_cancelled");
    setBuildExitConfirmOpen(false);
  }

  function confirmLeaveBuildModeDiscard() {
    buildPerfEvent("build.ui.exit_confirm_discard");
    setBuildExitConfirmOpen(false);
    if (build.workspaceMode !== "build") return;
    build.cancelBuildMode();
  }

  useEffect(() => {
    buildPerfEvent("build.state.workspace_mode_changed", {
      workspaceMode: build.workspaceMode,
      buildAction: build.buildAction,
      isDirty: build.isDirty,
    });
  }, [build.buildAction, build.isDirty, build.workspaceMode]);

  useEffect(() => {
    if (build.workspaceMode !== "build" || !build.isDirty) {
      setBuildExitConfirmOpen(false);
    }
  }, [build.isDirty, build.workspaceMode]);

  const {
    handleStopAction,
    handleLineAction,
    handleMapPointAction,
    handleMapClearSelection,
    focusStationById,
    createInterchangeGroupForSelectedStation,
    clearSelectedStationInterchange,
    applySuggestedInterchange,
  } = useMapBuildInteractions({
    build,
    setFocusStopRequest,
  });

  function navigateFromAlert(alert: AlertItem) {
    shellPanels.setShowAlerts(false);
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
    if (target.kind === "session") {
      if (target.id.includes("map")) {
        retryMapLoad();
      }
      return;
    }
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
  const unlockedRegionSignature = useMemo(
    () =>
      regions
        .filter((region) => region.unlocked)
        .map((region) => region.region_id)
        .sort()
        .join("|"),
    [regions]
  );
  const demandOverlaySelection = useMemo(() => {
    if (demandOverlayStatusMessage?.trim()) {
      return {
        available: false,
        reason: demandOverlayStatusMessage.trim(),
      };
    }
    if (!demandOverlayPayload) {
      return {
        available: false,
        reason: "Demand overlays are unavailable.",
      };
    }
    const available = demandOverlayPayload.available ?? false;
    return {
      available,
      reason: available
        ? null
        : demandOverlayPayload.reason?.trim() ??
          "Demand overlay is unavailable.",
    };
  }, [demandOverlayPayload, demandOverlayStatusMessage]);
  const demandOverlayAvailable = demandOverlaySelection.available;
  const demandOverlaySelectedMessage = demandOverlaySelection.reason;

  useEffect(() => {
    setDemandOverlayPayload(null);
    setDemandOverlayStatusMessage(null);
    setDemandOverlayLoading(false);
  }, [bundle?.project_path]);

  useEffect(() => {
    const projectPath = bundle?.project_path?.trim();
    if (!showDemandOverlay || !projectPath) {
      setDemandOverlayLoading(false);
      return;
    }
    let cancelled = false;
    const refreshPayload = async () => {
      setDemandOverlayLoading(true);
      try {
        const payload = await getDemandOverlayPayload(projectPath, demandOverlayType);
        if (cancelled) return;
        setDemandOverlayPayload(payload);
        setDemandOverlayStatusMessage(null);
      } catch (error) {
        if (cancelled) return;
        const detail =
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : "Unknown backend error";
        console.error("[demand-overlay] payload load failed", error);
        setDemandOverlayPayload(null);
        setDemandOverlayStatusMessage(
          `Demand overlays could not be loaded. ${detail}`
        );
      } finally {
        if (!cancelled) {
          setDemandOverlayLoading(false);
        }
      }
    };
    void refreshPayload();
    return () => {
      cancelled = true;
    };
  }, [
    bundle?.project_path,
    demandOverlayType,
    showDemandOverlay,
    unlockedRegionSignature,
  ]);

  const { commandActions, runPaletteCommand } = useShellCommandOrchestration({
    route,
    workspaceMode: build.workspaceMode,
    clock,
    hasActiveSessionBundle: Boolean(bundle),
    panels: shellPanels,
    onSaveSession: saveSession,
    onSaveQuit: saveQuit,
    onSetRunning: setRunning,
    onSetSpeed: setSpeed,
    onEnterBuildMode: build.enterBuildMode,
    onLeaveBuildMode: leaveBuildMode,
    rollingStockEditorOpen,
    onCloseRollingStockEditor: () => setRollingStockEditorOpen(false),
    scheduleEditorOpen,
    onCloseScheduleEditor: () => setScheduleEditorOpen(false),
    lineDeleteDialogOpen,
    onCancelDeleteSelectedLine: cancelDeleteSelectedLine,
    blockingOverlayActive: busy || shellStatus.showSessionBootOverlay,
    canCancelWorkspaceToolStep:
      build.workspaceMode === "build" && build.buildAction !== "select",
    onCancelWorkspaceToolStep: () => build.selectBuildAction("select"),
    canClearWorkspaceSelection:
      Boolean(build.selection) &&
      (build.workspaceMode !== "build" || build.buildAction === "select"),
    onClearWorkspaceSelection: () => build.clearSelection(),
  });

  useEffect(() => {
    if (route !== "home") {
      setHomeSettingsOpen(false);
    }
  }, [route]);

  if (route !== "session_game" && route !== "session_scenario") {
    return (
      <>
        <AppRouteScreens
          route={route}
          canContinue={canContinue}
          latestGameSave={latestGameSave}
          gameBrowserView={saveBrowser.gameView}
          scenarioBrowserView={saveBrowser.scenarioView}
          countries={countries}
          countryPacks={countryPacks}
          selectedCountryIso2={selectedCountryIso2}
          selectedCountryName={selectedCountry?.name ?? null}
          selectedCityId={selectedCityId}
          selectedCityName={selectedCity?.name ?? null}
          citySearch={citySearch}
          filteredCities={filteredCities}
          busy={busy}
          error={error}
          scenarioName={scenarioName}
          setScenarioName={setScenarioName}
          selectedDifficultyProfile={selectedDifficultyProfile}
          newGame={newGame}
          onNextNewGameStep={nextNewGameStep}
          onRouteHome={() => setRoute("home")}
          onRouteLoadGame={() => setRoute("load_game")}
          onRouteNewGame={() => {
            newGame.beginFlow();
            setRoute("new_game");
          }}
          onRouteNewScenario={() => setRoute("new_scenario")}
          onRouteLoadScenario={() => setRoute("load_scenario")}
          onOpenSettings={() => setHomeSettingsOpen(true)}
          onContinueLatestGame={continueLatestGame}
          onLoadGameSave={loadGameSave}
          onLoadScenarioSave={loadScenarioSave}
          onSaveBrowserQueryChange={saveBrowser.setQuery}
          onSaveBrowserSortChange={saveBrowser.setSortKey}
          onSaveBrowserGroupChange={saveBrowser.setGroup}
          onSaveBrowserSelectProject={saveBrowser.selectProject}
          onDeleteSave={deleteSave}
          onRestoreDeletedSave={restoreDeletedSave}
          onPurgeDeletedSave={purgeDeletedSave}
          onCreateGame={createGame}
          onCreateScenario={createScenario}
          onImportScenarioFromPicker={importScenarioFromPicker}
          onCountryChanged={onCountryChanged}
          onInstallCountryPack={installCountryPack}
          onCitySearchChange={setCitySearch}
          onCitySelected={setSelectedCityId}
        />
        <SettingsPanel
          open={homeSettingsOpen}
          settings={shellStatus.uiSettings}
          onChange={shellStatus.setUiSettings}
          onClose={() => setHomeSettingsOpen(false)}
          onReset={() => shellStatus.setUiSettings(DEFAULT_UI_SETTINGS)}
        />
      </>
    );
  }

  if (!bundle || !sessionKind || !clock) {
    return <div className="global-error">No active session.</div>;
  }

  const hasShapeNodeData = Boolean(activeScenario?.world.stops.some((stop) => stop.stop_type === "shape"));
  const hasZoneCentroidData = (activeScenario?.world.zones?.length ?? 0) > 0;

  return (
    <AppSessionShell
      bundle={bundle}
      sessionKind={sessionKind}
      clock={clock}
      build={build}
      shellPanels={shellPanels}
      shellStatus={shellStatus}
      scenarioSidebar={scenarioSidebar}
      busy={busy}
      saveStatus={saveStatus}
      setSaveStatus={setSaveStatus}
      demandWarning={shellStatus.primaryDemandIssue?.detail ?? null}
      error={shellStatus.primaryBlockingIssue?.detail ?? null}
      setError={(value) => {
        if (value === null) {
          const issueId = shellStatus.primaryBlockingIssue?.id;
          if (issueId) {
            shellStatus.dismissAlert(issueId);
          } else {
            setError(null);
          }
          return;
        }
        setError(value);
      }}
      sessionBootState={lifecycle.snapshot.bootState}
      mapInstanceToken={mapInstanceToken}
      mapRuntimeConfig={mapRuntimeConfig}
      liveEconomy={liveEconomy}
      farePolicy={farePolicy}
      serviceLoadByServiceId={serviceLoadByServiceId}
      runtimeTrains={runtimeTrains}
      trainsAuthoritative={trainsAuthoritative}
      runtimeTelemetry={runtimeTelemetry}
      snapshotLatencyMs={snapshotLatencyMs}
      temporalDiagnostics={temporalDiagnostics}
      runtimeStations={runtimeStations}
      runtimeLineOps={runtimeLineOps}
      showShapeStops={showShapeStops}
      setShowShapeStops={setShowShapeStops}
      showZoneCentroids={showZoneCentroids}
      setShowZoneCentroids={setShowZoneCentroids}
      showStations={showStations}
      setShowStations={setShowStations}
      showLinks={showLinks}
      setShowLinks={setShowLinks}
      linkMode={linkMode}
      setLinkMode={setLinkMode}
      showDemandOverlay={showDemandOverlay}
      setShowDemandOverlay={setShowDemandOverlay}
      demandOverlayType={demandOverlayType}
      setDemandOverlayType={setDemandOverlayType}
      demandOverlayLoading={demandOverlayLoading}
      demandOverlayAvailable={demandOverlayAvailable}
      demandOverlayStatusMessage={demandOverlaySelectedMessage}
      demandOverlayPayload={demandOverlayPayload}
      currentBuildPreset={currentBuildPreset}
      fleetDeliveries={fleetDeliveries}
      activeScenario={activeScenario}
      startCenter={startCenter}
      visibleCountryIso2={visibleCountryIso2}
      regions={regions}
      focusRegionId={focusRegionId}
      activeRegionIds={activeRegionIds}
      selectedRegionId={selectedRegionId}
      budgetCurrency={budgetCurrency}
      currentBalanceBase={currentBalanceBase}
      mapComplexityScore={mapComplexityScore}
      hasShapeNodeData={hasShapeNodeData}
      hasZoneCentroidData={hasZoneCentroidData}
      focusStopRequest={focusStopRequest}
      focusVehicleRequest={focusVehicleRequest}
      selectedVehicleInspection={selectedVehicleInspection}
      previewAnchorPoint={previewAnchorPoint}
      previewColor={previewColor}
      buildConstraintMode={buildConstraintMode}
      extensionAddedStations={extensionAddedStations}
      extensionAddedLengthM={extensionAddedLengthM}
      lineDraftMode={lineDraftMode}
      lineDraftAwaitingTerminus={lineDraftAwaitingTerminus}
      lineDraftAnchorStopName={lineDraftAnchorStopName}
      activeLineDraftPreview={activeLineDraftPreview}
      selectedLinePresetId={selectedLinePresetId}
      selectedLineDetail={selectedLineDetail}
      selectedLineBuildPreset={selectedLineBuildPreset}
      selectedLineStationDecorations={selectedLineStationDecorations}
      selectedLineEstimatedCapexBase={selectedLineEstimatedCapexBase}
      selectedLineScheduleState={selectedLineScheduleState}
      selectedLineFleetEditorState={selectedLineFleetEditorState}
      selectedLineUnitLabel={selectedLineUnitLabel}
      selectedLineActiveVehicles={selectedLineActiveVehicles}
      selectedLineTransferTargets={selectedLineTransferTargets}
      selectedLineScrapEstimateBase={selectedLineScrapEstimateBase}
      selectedStationLines={selectedStationLines}
      selectedStationInterchangeContext={selectedStationInterchangeContext}
      lineInspectorOpen={lineInspectorOpen}
      stationInspectorOpen={stationInspectorOpen}
      lineDeleteDialogEnabled={lineDeleteDialogEnabled}
      rollingStockEditorEnabled={rollingStockEditorEnabled}
      scheduleEditorEnabled={scheduleEditorEnabled}
      rollingStockEditorOpen={rollingStockEditorOpen}
      scheduleEditorOpen={scheduleEditorOpen}
      lineDeleteDialogOpen={lineDeleteDialogOpen}
      commandActions={commandActions}
      runPaletteCommand={runPaletteCommand}
      financialBusy={financialBusy}
      financialError={financialError}
      financialRequest={financialRequest}
      setFinancialRequest={setFinancialRequest}
      financialData={financialData}
      financialLineOptions={financialLineOptions}
      onRefreshFinancialDashboard={refreshFinancialDashboard}
      onHandleMapBootProgress={handleMapBootProgress}
      onSelectCounty={selectCounty}
      onHandleStopAction={handleStopAction}
      onHandleLineAction={handleLineAction}
      onHandleVehicleAction={handleVehicleInspection}
      onClearVehicleInspection={clearVehicleInspection}
      onHandleMapPointAction={handleMapPointAction}
      onHandleMapClearSelection={handleMapClearSelection}
      onHandleScrapVehicleFromMap={handleScrapVehicleFromMap}
      onRunPlanning={runPlanning}
      onRebuildDemandForUnlocked={rebuildDemandForUnlocked}
      onExportRunCsv={exportRunCsv}
      onExportRunJson={exportRunJson}
      onCompareRuns={compareRuns}
      onRequestDeleteSelectedLine={requestDeleteSelectedLine}
      onFocusStationById={focusStationById}
      onOpenRollingStockEditorFromLineInspector={openRollingStockEditorFromLineInspector}
      onOpenScheduleEditorFromLineInspector={openScheduleEditorFromLineInspector}
      onCancelDeleteSelectedLine={cancelDeleteSelectedLine}
      onDeleteSelectedLineWithScrap={deleteSelectedLineWithScrap}
      onDeleteSelectedLineWithTransfer={deleteSelectedLineWithTransfer}
      onFocusVehicleFromFleet={focusVehicleInspectionFromFleet}
      onOpenRollingStockEditorFromSchedule={openRollingStockEditorFromSchedule}
      onCreateInterchangeGroupForSelectedStation={createInterchangeGroupForSelectedStation}
      onClearSelectedStationInterchange={clearSelectedStationInterchange}
      onApplySuggestedInterchange={applySuggestedInterchange}
      onLeaveBuildMode={leaveBuildMode}
      buildExitConfirmOpen={buildExitConfirmOpen}
      onCancelExitBuildModeConfirm={cancelLeaveBuildModeConfirm}
      onConfirmExitBuildModeDiscard={confirmLeaveBuildModeDiscard}
      onNavigateFromAlert={navigateFromAlert}
      onRefreshDashboardFromHud={async () => {
        shellPanels.setShowFinancialDashboard(true);
        await refreshFinancialDashboard();
      }}
      onExpediteFleetDelivery={expediteFleetDelivery}
      onSaveSession={saveSession}
      onSaveQuit={saveQuit}
      onSetRunning={setRunning}
      onSetSpeed={setSpeed}
      onUnlockSelectedCounty={unlockAndFocusSelectedCounty}
      onUpdateFarePolicy={updateFarePolicy}
      onRetryMapLoad={retryMapLoad}
      closeLineEditors={closeLineEditors}
      setRollingStockEditorOpen={setRollingStockEditorOpen}
      setScheduleEditorOpen={setScheduleEditorOpen}
      missions={MISSIONS}
      defaultUiSettings={DEFAULT_UI_SETTINGS}
    />
  );
}
