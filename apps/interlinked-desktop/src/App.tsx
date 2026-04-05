import { useMemo, useRef, useState } from "react";
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
  DeletedSaveMeta,
  Difficulty,
  FarePolicyManifest,
  GameSaveMeta,
  Mission,
  OpenSessionResult,
  MapRuntimeConfig,
  RegionStatus,
  ScenarioLite,
  ScenarioSaveMeta,
  SimulationAdvanceEconomy,
  SimulationClock,
  RuntimePerfTelemetry,
  LineOpsRuntimeView,
  StationRuntimeView,
  TrainRuntimeView,
  DifficultyProfile,
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
import {
  DEFAULT_UI_SETTINGS,
  useShellStatusOrchestration,
} from "./app/useShellStatusOrchestration";
import { useNewGameFlowController } from "./app/useNewGameFlowController";
import {
  type SessionBootState,
  useSessionController,
} from "./app/useSessionController";
import type { LinkModeFilter } from "./ui/MapFiltersPanel";
import AppRouteScreens from "./ui/AppRouteScreens";
import AppSessionShell from "./ui/AppSessionShell";

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

  const [showShapeStops, setShowShapeStops] = useState(false);
  const [showZoneCentroids, setShowZoneCentroids] = useState(false);
  const [showStations, setShowStations] = useState(true);
  const [showLinks, setShowLinks] = useState(true);
  const [linkMode, setLinkMode] = useState<LinkModeFilter>("all");
  const [financialRequest, setFinancialRequest] = useState<FinancialDashboardRequest>({
    granularity: "month",
    periods: 12,
  });
  const [financialData, setFinancialData] = useState<FinancialDashboardResponse | null>(null);
  const [financialBusy, setFinancialBusy] = useState(false);
  const [financialError, setFinancialError] = useState<string | null>(null);
  const [focusStopRequest, setFocusStopRequest] = useState<FocusStopRequest>(null);
  const [focusVehicleRequest, setFocusVehicleRequest] = useState<FocusVehicleRequest>(null);
  const [runtimeTelemetry, setRuntimeTelemetry] = useState<RuntimePerfTelemetry | null>(null);
  const [snapshotLatencyMs, setSnapshotLatencyMs] = useState<number | null>(null);
  const [runtimeStations, setRuntimeStations] = useState<StationRuntimeView[]>([]);
  const [runtimeLineOps, setRuntimeLineOps] = useState<LineOpsRuntimeView[]>([]);
  const [mapInstanceToken, setMapInstanceToken] = useState(0);
  const [sessionBootState, setSessionBootState] = useState<SessionBootState>({
    stage: "idle",
    progress: 0,
    message: "",
    error: null,
  });

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
    sessionKind,
  });
  const scenarioSidebar = useScenarioSidebarController();
  const newGame = useNewGameFlowController({
    defaultBudgetFor,
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
    sessionBootState,
    bundleProjectId: bundle?.manifest.project_id,
    clock,
    workspaceMode: build.workspaceMode,
    activeScenario,
    lineSummaries: build.lineSummaries,
    runtimeLineOps,
    runtimeStations,
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

  const {
    refreshFinancialDashboard,
    onCountryChanged,
    installCountryPack,
    uninstallCountryPack,
    continueLatestGame,
    loadGameSave,
    loadScenarioSave,
    selectCounty,
    focusSelectedCounty,
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
    sessionBootState,
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
    setGameSaves,
    setScenarioSaves,
    setDeletedSaves,
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
    setSessionBootState,
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
    if (build.workspaceMode !== "build") return;
    if (build.isDirty) {
      const discard = window.confirm("Discard the current build draft and return to view mode?");
      if (!discard) return;
    }
    build.cancelBuildMode();
  }

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
  });

  if (route !== "session_game" && route !== "session_scenario") {
    return (
      <AppRouteScreens
        route={route}
        canContinue={canContinue}
        latestGameSave={latestGameSave}
        gameSaves={gameSaves}
        scenarioSaves={scenarioSaves}
        deletedSaves={deletedSaves}
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
        onContinueLatestGame={continueLatestGame}
        onLoadGameSave={loadGameSave}
        onLoadScenarioSave={loadScenarioSave}
        onDeleteSave={deleteSave}
        onRestoreDeletedSave={restoreDeletedSave}
        onPurgeDeletedSave={purgeDeletedSave}
        onCreateGame={createGame}
        onCreateScenario={createScenario}
        onImportScenarioFromPicker={importScenarioFromPicker}
        onCountryChanged={onCountryChanged}
        onInstallCountryPack={installCountryPack}
        onUninstallCountryPack={uninstallCountryPack}
        onCitySearchChange={setCitySearch}
        onCitySelected={setSelectedCityId}
      />
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
      demandWarning={demandWarning}
      error={error}
      setError={setError}
      sessionBootState={sessionBootState}
      mapInstanceToken={mapInstanceToken}
      mapRuntimeConfig={mapRuntimeConfig}
      liveEconomy={liveEconomy}
      farePolicy={farePolicy}
      serviceLoadByServiceId={serviceLoadByServiceId}
      runtimeTrains={runtimeTrains}
      trainsAuthoritative={trainsAuthoritative}
      runtimeTelemetry={runtimeTelemetry}
      snapshotLatencyMs={snapshotLatencyMs}
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
      setFocusVehicleRequest={setFocusVehicleRequest}
      previewAnchorPoint={previewAnchorPoint}
      previewColor={previewColor}
      buildConstraintMode={buildConstraintMode}
      stationCostBase={stationCostBase}
      lineCostPerKmBase={lineCostPerKmBase}
      extensionAddedStations={extensionAddedStations}
      extensionAddedLengthM={extensionAddedLengthM}
      extensionConstructionCostBase={extensionConstructionCostBase}
      lineDraftMode={lineDraftMode}
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
      onFocusVehicleFromFleet={focusVehicleFromFleet}
      onOpenRollingStockEditorFromSchedule={openRollingStockEditorFromSchedule}
      onCreateInterchangeGroupForSelectedStation={createInterchangeGroupForSelectedStation}
      onClearSelectedStationInterchange={clearSelectedStationInterchange}
      onApplySuggestedInterchange={applySuggestedInterchange}
      onLeaveBuildMode={leaveBuildMode}
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
      onFocusSelectedCounty={focusSelectedCounty}
      onUnlockAndFocusSelectedCounty={unlockAndFocusSelectedCounty}
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
