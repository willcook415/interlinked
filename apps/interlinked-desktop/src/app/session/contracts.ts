import type { Dispatch, MutableRefObject, SetStateAction } from "react";

import type {
  AppRoute,
  CityOption,
  CompareResult,
  CountryOption,
  CountryPackStatus,
  CurrencyCode,
  DemandCoverageMeta,
  Difficulty,
  FarePolicyManifest,
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  LineOpsRuntimeView,
  MapRuntimeConfig,
  OpenSessionResult,
  PlanningRunConfig,
  RegionStatus,
  RuntimePerfTelemetry,
  SaveLibrarySnapshot,
  SessionKind,
  SimulationAdvanceEconomy,
  SimulationClock,
  SimulationSpeed,
  StationRuntimeView,
  TrainRuntimeView,
} from "../../types";

export type SessionBootState = {
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

export type SessionLifecycleState =
  | "app_home"
  | "project_selected"
  | "session_booting"
  | "session_loading_map"
  | "session_loading_runtime"
  | "session_ready"
  | "session_error"
  | "session_recovering";

export type SessionLifecycleErrorSource =
  | "project_open"
  | "map_runtime_config"
  | "map"
  | "runtime_control"
  | "runtime_snapshot"
  | "unknown";

export type SessionLifecycleError = {
  source: SessionLifecycleErrorSource;
  message: string;
  recoverable: boolean;
  occurredAtEpochMs: number;
};

export type SessionLifecycleCheckpoints = {
  projectAccepted: boolean;
  mapContextReady: boolean;
  runtimeControlReady: boolean;
  firstFastSnapshotReady: boolean;
};

export type SessionLifecycleSnapshot = {
  state: SessionLifecycleState;
  sessionKind: SessionKind | null;
  checkpoints: SessionLifecycleCheckpoints;
  lastError: SessionLifecycleError | null;
  bootState: SessionBootState;
};

export type MapBootProgressPayload = {
  stage: "map_style" | "map_context" | "ready" | "error";
  progress: number;
  message: string;
  error?: string | null;
};

export type SessionLifecycleControllerPort = {
  snapshot: SessionLifecycleSnapshot;
  beginProjectSelection: (message?: string) => void;
  acceptOpenedSession: (sessionKind: SessionKind) => void;
  markMapRuntimeConfigStarted: () => void;
  markMapRuntimeConfigReady: () => void;
  markMapRuntimeConfigFailed: (message: string) => void;
  publishMapBootProgress: (payload: MapBootProgressPayload) => void;
  markRuntimeControlReady: () => void;
  markFirstFastSnapshotReady: () => void;
  reportBlockingError: (
    source: SessionLifecycleErrorSource,
    message: string,
    recoverable?: boolean
  ) => void;
  beginRecovery: (
    source: SessionLifecycleErrorSource,
    message?: string
  ) => void;
  resetToAppHome: () => void;
};

export type BuildSessionPort = {
  workspaceMode: "view" | "build";
  isDirty: boolean;
  setBuilderError: (value: string | null) => void;
};

export type UseSessionControllerParams = {
  route: AppRoute;
  setRoute: Dispatch<SetStateAction<AppRoute>>;
  bundle: OpenSessionResult | null;
  setBundle: Dispatch<SetStateAction<OpenSessionResult | null>>;
  sessionKind: SessionKind | null;
  clock: SimulationClock | null;
  setClock: Dispatch<SetStateAction<SimulationClock | null>>;
  build: BuildSessionPort;
  selectedCountryIso2: string;
  setSelectedCountryIso2: Dispatch<SetStateAction<string>>;
  selectedCountry: CountryOption | null;
  selectedCountryPack: CountryPackStatus | null;
  selectedCity: CityOption | null;
  setSelectedCityId: Dispatch<SetStateAction<number | null>>;
  setCitySearch: Dispatch<SetStateAction<string>>;
  selectedRegionId: string | null;
  regions: RegionStatus[];
  newGameName: string;
  newGameDifficulty: Difficulty;
  newGameCurrency: CurrencyCode;
  newGameBudget: string;
  scenarioName: string;
  runConfig: PlanningRunConfig;
  selectedBaseRun: string;
  selectedCandidateRun: string;
  setSelectedBaseRun: Dispatch<SetStateAction<string>>;
  financialRequest: FinancialDashboardRequest;
  showFinancialDashboard: boolean;
  lifecycle: SessionLifecycleControllerPort;
  defaultBudgetFor: (difficulty: Difficulty, currency: CurrencyCode) => number;
  formatBackendError: (error: unknown) => string;
  playUiCue: (kind: "confirm" | "error" | "toggle" | "alert") => void;
  latestClockTickRef: MutableRefObject<number>;
  latestSnapshotTickRef: MutableRefObject<number>;
  latestSnapshotCapturedRef: MutableRefObject<number>;
  latestStrategicSnapshotTickRef: MutableRefObject<number>;
  latestStrategicSnapshotCapturedRef: MutableRefObject<number>;
  runtimeControlQueueRef: MutableRefObject<Promise<void>>;
  setBusy: Dispatch<SetStateAction<boolean>>;
  setError: Dispatch<SetStateAction<string | null>>;
  setDemandWarning: Dispatch<SetStateAction<string | null>>;
  setSaveStatus: Dispatch<SetStateAction<string>>;
  setSaveLibrary: (next: SaveLibrarySnapshot) => void;
  setCountries: Dispatch<SetStateAction<CountryOption[]>>;
  setCountryPacks: Dispatch<SetStateAction<CountryPackStatus[]>>;
  setCities: Dispatch<SetStateAction<CityOption[]>>;
  setDemandCoverage: Dispatch<SetStateAction<DemandCoverageMeta[]>>;
  setRegions: Dispatch<SetStateAction<RegionStatus[]>>;
  setFocusRegionId: Dispatch<SetStateAction<string | null>>;
  setSelectedRegionId: Dispatch<SetStateAction<string | null>>;
  setShowCountryInfo: Dispatch<SetStateAction<boolean>>;
  setShowFares: Dispatch<SetStateAction<boolean>>;
  setMapRuntimeConfig: Dispatch<SetStateAction<MapRuntimeConfig | null>>;
  setFarePolicy: Dispatch<SetStateAction<FarePolicyManifest | null>>;
  setLiveEconomy: Dispatch<SetStateAction<SimulationAdvanceEconomy | null>>;
  setServiceLoadByServiceId: Dispatch<SetStateAction<Record<string, number>>>;
  setRuntimeTrains: Dispatch<SetStateAction<TrainRuntimeView[]>>;
  setRuntimeStations: Dispatch<SetStateAction<StationRuntimeView[]>>;
  setRuntimeLineOps: Dispatch<SetStateAction<LineOpsRuntimeView[]>>;
  setTrainsAuthoritative: Dispatch<SetStateAction<boolean>>;
  setRuntimeTelemetry: Dispatch<SetStateAction<RuntimePerfTelemetry | null>>;
  setSnapshotLatencyMs: Dispatch<SetStateAction<number | null>>;
  setMapInstanceToken: Dispatch<SetStateAction<number>>;
  setFinancialBusy: Dispatch<SetStateAction<boolean>>;
  setFinancialError: Dispatch<SetStateAction<string | null>>;
  setFinancialData: Dispatch<SetStateAction<FinancialDashboardResponse | null>>;
  setCompareResult: Dispatch<SetStateAction<CompareResult | null>>;
  setShowMenu: Dispatch<SetStateAction<boolean>>;
};

export type SessionControllerResult = {
  refreshLibraries: () => Promise<void>;
  refreshFinancialDashboard: () => Promise<void>;
  onCountryChanged: (iso2: string) => Promise<void>;
  installCountryPack: (iso2: string) => Promise<void>;
  uninstallCountryPack: (iso2: string) => Promise<void>;
  continueLatestGame: () => Promise<void>;
  loadGameSave: (saveId: string) => Promise<void>;
  loadScenarioSave: (saveId: string) => Promise<void>;
  selectCounty: (regionId: string) => void;
  focusSelectedCounty: () => Promise<void>;
  unlockAndFocusSelectedCounty: () => Promise<void>;
  deleteSave: (saveId: string, name: string) => Promise<void>;
  restoreDeletedSave: (deletedId: string) => Promise<void>;
  purgeDeletedSave: (deletedId: string) => Promise<void>;
  createGame: () => Promise<void>;
  createScenario: () => Promise<void>;
  importScenarioFromPicker: () => Promise<void>;
  saveSession: () => Promise<void>;
  saveQuit: () => Promise<void>;
  setRunning: (running: boolean) => Promise<void>;
  setSpeed: (speed: SimulationSpeed) => Promise<void>;
  runPlanning: () => Promise<void>;
  exportRunCsv: (runId: string) => Promise<void>;
  exportRunJson: (runId: string) => Promise<void>;
  compareRuns: () => Promise<void>;
  rebuildDemandForUnlocked: () => Promise<void>;
  handleMapBootProgress: (payload: MapBootProgressPayload) => void;
  retryMapLoad: () => void;
  updateFarePolicy: (patch: Partial<FarePolicyManifest>) => void;
  expediteFleetDelivery: (delivery: {
    id: string;
    orderId: string;
    label: string;
    lineId: string;
    lineName: string;
  }) => Promise<void>;
};

export type WithBusy = <T>(fn: () => Promise<T>) => Promise<T | null>;
