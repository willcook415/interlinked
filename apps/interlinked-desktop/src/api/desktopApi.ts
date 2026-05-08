import { invokeTyped } from "./tauriClient";
import { buildPerfMeasureAsync } from "../perf/buildPerf";
import type {
  BuildDefaults,
  CityOption,
  CompareResult,
  CountryMapContext,
  CountryOption,
  CountryPackStatus,
  DeleteSaveResult,
  DeletedSaveMeta,
  DemandCoverageMeta,
  DemandOverlayPayload,
  DemandRebuildResult,
  FarePolicyManifest,
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  FleetDeliveryExpediteResult,
  FocusResult,
  GameCreatePayload,
  GameSaveMeta,
  InstallResult,
  LineInspection,
  MapRuntimeConfig,
  NetworkMutationPreviewResult,
  NetworkMutationResult,
  OpenSessionResult,
  PlanningRunConfig,
  PurgeSaveResult,
  RegionStatus,
  RestoreSaveResult,
  RunMeta,
  RuntimeFastSnapshot,
  RuntimeSnapshot,
  RuntimeStrategicSnapshot,
  SaveResult,
  ScenarioDocumentLite,
  ScenarioSaveMeta,
  SimulationClock,
  SimulationSpeed,
  StationInspection,
  UnlockFocusResult,
  UninstallResult,
} from "../types";

type SaveSessionPayload = {
  scenario_document: ScenarioDocumentLite;
};

export function listGameSaves(): Promise<GameSaveMeta[]> {
  return invokeTyped<GameSaveMeta[]>("list_game_saves");
}

export function listScenarioSaves(): Promise<ScenarioSaveMeta[]> {
  return invokeTyped<ScenarioSaveMeta[]>("list_scenario_saves");
}

export function listDeletedSaves(): Promise<DeletedSaveMeta[]> {
  return invokeTyped<DeletedSaveMeta[]>("list_deleted_saves");
}

export function listCountries(): Promise<CountryOption[]> {
  return invokeTyped<CountryOption[]>("list_countries");
}

export function listCountryPackStatus(): Promise<CountryPackStatus[]> {
  return invokeTyped<CountryPackStatus[]>("list_country_pack_status");
}

export function listCities(countryIso2: string): Promise<CityOption[]> {
  return invokeTyped<CityOption[]>("list_cities", { countryIso2 });
}

export function installCountryPack(countryIso2: string): Promise<InstallResult> {
  return invokeTyped<InstallResult>("install_country_pack", { countryIso2 });
}

export function uninstallCountryPack(countryIso2: string): Promise<UninstallResult> {
  return invokeTyped<UninstallResult>("uninstall_country_pack", { countryIso2 });
}

export function continueLatestGame(): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("continue_latest_game");
}

export function loadGameSave(saveId: string): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("load_game_save", { saveId });
}

export function loadScenarioSave(saveId: string): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("load_scenario_save", { saveId });
}

export function openProject(projectPath: string): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("open_project", { projectPath });
}

export function createGame(payload: GameCreatePayload): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("create_game", { payload });
}

export function createScenario(name: string): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("create_scenario", {
    payload: { name },
  });
}

export function pickScenarioFile(): Promise<string | null> {
  return invokeTyped<string | null>("pick_scenario_file");
}

export function importScenario(filePath: string, name: string | null): Promise<OpenSessionResult> {
  return invokeTyped<OpenSessionResult>("import_scenario", {
    filePath,
    name,
  });
}

export function deleteSave(saveId: string): Promise<DeleteSaveResult> {
  return invokeTyped<DeleteSaveResult>("delete_save", { saveId });
}

export function restoreDeletedSave(deletedId: string): Promise<RestoreSaveResult> {
  return invokeTyped<RestoreSaveResult>("restore_deleted_save", { deletedId });
}

export function purgeDeletedSave(deletedId: string): Promise<PurgeSaveResult> {
  return invokeTyped<PurgeSaveResult>("purge_deleted_save", { deletedId });
}

export function saveSession(projectPath: string, payload: SaveSessionPayload): Promise<SaveResult> {
  return invokeTyped<SaveResult>("save_session", { projectPath, payload });
}

export function saveAndQuit(projectPath: string, payload: SaveSessionPayload): Promise<SaveResult> {
  return invokeTyped<SaveResult>("save_and_quit", { projectPath, payload });
}

export function listDemandCoverage(projectPath: string): Promise<DemandCoverageMeta[]> {
  return invokeTyped<DemandCoverageMeta[]>("list_demand_coverage", { projectPath });
}

export function getDemandOverlayPayload(
  projectPath: string,
  overlayType?:
    | "residential_allocation"
    | "employment_allocation"
    | "total_allocation"
    | "raw_residential_weight"
    | "raw_employment_weight"
    | "fallback_cells"
): Promise<DemandOverlayPayload> {
  return invokeTyped<DemandOverlayPayload>("get_demand_overlay_payload", {
    projectPath,
    overlayType: overlayType ?? null,
  });
}

export function listRegions(projectPath: string): Promise<RegionStatus[]> {
  return invokeTyped<RegionStatus[]>("list_regions", { projectPath });
}

export function setPrimaryFocusRegion(projectPath: string, regionId: string): Promise<FocusResult> {
  return invokeTyped<FocusResult>("set_primary_focus_region", { projectPath, regionId });
}

export function unlockAndFocusRegion(projectPath: string, regionId: string): Promise<UnlockFocusResult> {
  return invokeTyped<UnlockFocusResult>("unlock_and_focus_region", { projectPath, regionId });
}

export function rebuildDemandForUnlocked(projectPath: string): Promise<DemandRebuildResult> {
  return invokeTyped<DemandRebuildResult>("rebuild_demand_for_unlocked", { projectPath });
}

export function loadMapRuntimeConfig(projectPath: string): Promise<MapRuntimeConfig> {
  return invokeTyped<MapRuntimeConfig>("load_map_runtime_config", { projectPath });
}

export function loadCountryMapContext(projectPath: string): Promise<CountryMapContext> {
  return invokeTyped<CountryMapContext>("load_country_map_context", { projectPath });
}

export function getRuntimeSnapshot(projectPath: string): Promise<RuntimeSnapshot | null> {
  return invokeTyped<RuntimeSnapshot | null>("get_runtime_snapshot", { projectPath });
}

export function startRuntimeLoop(projectPath: string): Promise<void> {
  return invokeTyped<void>("start_runtime_loop", { projectPath });
}

export function stopRuntimeLoop(projectPath: string): Promise<void> {
  return invokeTyped<void>("stop_runtime_loop", { projectPath });
}

export function getRuntimeFastSnapshot(projectPath: string): Promise<RuntimeFastSnapshot | null> {
  return invokeTyped<RuntimeFastSnapshot | null>("get_runtime_fast_snapshot", { projectPath });
}

export function getRuntimeStrategicSnapshot(
  projectPath: string
): Promise<RuntimeStrategicSnapshot | null> {
  return invokeTyped<RuntimeStrategicSnapshot | null>("get_runtime_strategic_snapshot", {
    projectPath,
  });
}

export function setSimulationRunning(
  projectPath: string,
  running: boolean
): Promise<SimulationClock> {
  return buildPerfMeasureAsync(
    "frontend.command.set_simulation_running.roundtrip",
    () => invokeTyped<SimulationClock>("set_simulation_running", { projectPath, running }),
    { projectPath, running },
    { minDurationMs: 0 }
  );
}

export function setSimulationSpeed(
  projectPath: string,
  speed: SimulationSpeed
): Promise<SimulationClock> {
  return invokeTyped<SimulationClock>("set_simulation_speed", { projectPath, speed });
}

export function getFarePolicy(projectPath: string): Promise<FarePolicyManifest> {
  return invokeTyped<FarePolicyManifest>("get_fare_policy", { projectPath });
}

export function setFarePolicy(
  projectPath: string,
  policyPatch: Partial<FarePolicyManifest>
): Promise<FarePolicyManifest> {
  return invokeTyped<FarePolicyManifest>("set_fare_policy", { projectPath, policyPatch });
}

export function runPlanning(projectPath: string, runConfig: PlanningRunConfig): Promise<RunMeta> {
  return invokeTyped<RunMeta>("run_planning", { projectPath, runConfig });
}

export function pickExportPath(fileKind: "csv" | "json"): Promise<string | null> {
  return invokeTyped<string | null>("pick_export_path", { fileKind });
}

export function exportScenarioReportCsv(
  projectPath: string,
  runId: string,
  filePath: string
): Promise<{ out_path: string }> {
  return invokeTyped<{ out_path: string }>("export_scenario_report_csv", {
    projectPath,
    runId,
    filePath,
  });
}

export function exportScenarioReportJson(
  projectPath: string,
  runId: string,
  filePath: string
): Promise<{ out_path: string }> {
  return invokeTyped<{ out_path: string }>("export_scenario_report_json", {
    projectPath,
    runId,
    filePath,
  });
}

export function compareRuns(
  projectPath: string,
  baseRunId: string,
  candidateRunId: string
): Promise<CompareResult> {
  return invokeTyped<CompareResult>("compare_runs", {
    projectPath,
    baseRunId,
    candidateRunId,
  });
}

export function getFinancialDashboard(
  projectPath: string,
  request: FinancialDashboardRequest
): Promise<FinancialDashboardResponse> {
  return invokeTyped<FinancialDashboardResponse>("get_financial_dashboard", {
    projectPath,
    request,
  });
}

export function expediteFleetDelivery(
  projectPath: string,
  lineId: string,
  orderId: string
): Promise<FleetDeliveryExpediteResult> {
  return invokeTyped<FleetDeliveryExpediteResult>("expedite_fleet_delivery", {
    projectPath,
    lineId,
    orderId,
  });
}

export function loadBuildDefaults(): Promise<BuildDefaults> {
  return buildPerfMeasureAsync(
    "frontend.command.load_build_defaults.roundtrip",
    () => invokeTyped<BuildDefaults>("load_build_defaults"),
    undefined,
    { minDurationMs: 0, throttleMs: 1000 }
  );
}

export function inspectStation(projectPath: string, stopId: string): Promise<StationInspection> {
  return buildPerfMeasureAsync(
    "frontend.command.inspect_station.roundtrip",
    () => invokeTyped<StationInspection>("inspect_station", { projectPath, stopId }),
    { projectPath, stopId },
    { minDurationMs: 0 }
  );
}

export function inspectLine(projectPath: string, lineId: string): Promise<LineInspection> {
  return buildPerfMeasureAsync(
    "frontend.command.inspect_line.roundtrip",
    () => invokeTyped<LineInspection>("inspect_line", { projectPath, lineId }),
    { projectPath, lineId },
    { minDurationMs: 0 }
  );
}

export function previewNetworkMutation(
  projectPath: string,
  scenarioDocument: ScenarioDocumentLite
): Promise<NetworkMutationPreviewResult> {
  return buildPerfMeasureAsync(
    "frontend.command.preview_network_mutation.roundtrip",
    () =>
      invokeTyped<NetworkMutationPreviewResult>("preview_network_mutation", {
        projectPath,
        scenarioDocument,
      }),
    { projectPath },
    { minDurationMs: 0, throttleMs: 250 }
  );
}

export function applyNetworkMutation(args: {
  projectPath: string;
  scenarioDocument: ScenarioDocumentLite;
  capexOverrideBase: number | null;
}): Promise<NetworkMutationResult> {
  return buildPerfMeasureAsync(
    "frontend.command.apply_network_mutation.roundtrip",
    () => invokeTyped<NetworkMutationResult>("apply_network_mutation", args),
    { projectPath: args.projectPath, capexOverrideBase: args.capexOverrideBase },
    { minDurationMs: 0 }
  );
}
