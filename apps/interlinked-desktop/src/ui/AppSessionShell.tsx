import type { FocusStopRequest } from "../app/useMapBuildInteractions";
import type { FocusVehicleRequest } from "../app/useInspectorPanelController";
import type { SessionBootState } from "../app/useSessionController";
import type { ShellCommandAction } from "../app/useShellPanelsController";
import type { MapStopAction, MapWorldPoint } from "../MapView";
import type {
  AlertItem,
  CurrencyCode,
  FarePolicyManifest,
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  LineOpsRuntimeView,
  MapRuntimeConfig,
  Mission,
  OpenSessionResult,
  RegionStatus,
  RuntimePerfTelemetry,
  ScenarioLite,
  SessionKind,
  SimulationAdvanceEconomy,
  SimulationClock,
  StationRuntimeView,
  TrainRuntimeView,
} from "../types";
import BuildPalette from "./BuildPalette";
import CommandPalette from "./CommandPalette";
import CountryInfoDrawer from "./CountryInfoDrawer";
import DiagnosticsOverlay from "./DiagnosticsOverlay";
import FarePolicyPanel from "./FarePolicyPanel";
import FinancialDashboardModal from "./FinancialDashboardModal";
import LineDeleteDialog from "./LineDeleteDialog";
import LineInspectorSheet from "./LineInspectorSheet";
import MapCanvas from "./MapCanvas";
import MapFiltersPanel, { type LinkModeFilter } from "./MapFiltersPanel";
import MissionsDrawer from "./MissionsDrawer";
import RollingStockEditorSheet from "./RollingStockEditorSheet";
import ScenarioControlsSidebar from "./ScenarioControlsSidebar";
import ScheduleEditorSheet from "./ScheduleEditorSheet";
import SessionHud from "./SessionHud";
import SessionSideHud from "./SessionSideHud";
import SettingsPanel from "./SettingsPanel";
import ShellStatusOverlays from "./ShellStatusOverlays";
import StationInspectorModal from "./StationInspectorModal";
import AlertsCenter from "./AlertsCenter";

type BuildControllerState = ReturnType<typeof import("../build/useBuildController").useBuildController>;
type ShellPanelsState = ReturnType<typeof import("../app/useShellPanelsController").useShellPanelsController>;
type ScenarioSidebarState = ReturnType<typeof import("../app/useScenarioSidebarController").useScenarioSidebarController>;
type ShellStatusState = ReturnType<typeof import("../app/useShellStatusOrchestration").useShellStatusOrchestration>;

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

type StationInterchangeContext = {
  members: Array<{ stopId: string; name: string; distanceM: number }>;
  suggestions: Array<{ interchangeId: string; memberCount: number; nearestDistanceM: number }>;
  transfers: Array<{
    stopId: string;
    name: string;
    distanceM: number;
    transferTimeS: number;
    penaltyS: number;
    direction: "to" | "from" | "both";
  }>;
};

type DraftPreview = {
  lineId: string;
  lineName: string;
  modeLabel: string;
  displayColor: string;
  stationNames: string[];
  stationIds: string[];
} | null;

export default function AppSessionShell(props: {
  bundle: OpenSessionResult;
  sessionKind: SessionKind;
  clock: SimulationClock;
  build: BuildControllerState;
  shellPanels: ShellPanelsState;
  shellStatus: ShellStatusState;
  scenarioSidebar: ScenarioSidebarState;
  busy: boolean;
  saveStatus: string;
  setSaveStatus: (value: string) => void;
  demandWarning: string | null;
  error: string | null;
  setError: (value: string | null) => void;
  sessionBootState: SessionBootState;
  mapInstanceToken: number;
  mapRuntimeConfig: MapRuntimeConfig | null;
  liveEconomy: SimulationAdvanceEconomy | null;
  farePolicy: FarePolicyManifest | null;
  serviceLoadByServiceId: Record<string, number>;
  runtimeTrains: TrainRuntimeView[];
  trainsAuthoritative: boolean;
  runtimeTelemetry: RuntimePerfTelemetry | null;
  snapshotLatencyMs: number | null;
  runtimeStations: StationRuntimeView[];
  runtimeLineOps: LineOpsRuntimeView[];
  showShapeStops: boolean;
  setShowShapeStops: (value: boolean) => void;
  showZoneCentroids: boolean;
  setShowZoneCentroids: (value: boolean) => void;
  showStations: boolean;
  setShowStations: (value: boolean) => void;
  showLinks: boolean;
  setShowLinks: (value: boolean) => void;
  linkMode: LinkModeFilter;
  setLinkMode: (value: LinkModeFilter) => void;
  currentBuildPreset: { label?: string | null } | null;
  fleetDeliveries: FleetDeliveryItem[];
  activeScenario: ScenarioLite | null;
  startCenter: [number, number] | null;
  visibleCountryIso2: string[] | null;
  regions: RegionStatus[];
  focusRegionId: string | null;
  activeRegionIds: string[];
  selectedRegionId: string | null;
  budgetCurrency: CurrencyCode;
  currentBalanceBase: number | null;
  mapComplexityScore: number;
  missions: Mission[];
  defaultUiSettings: ReturnType<typeof import("../app/useShellStatusOrchestration").useShellStatusOrchestration>["uiSettings"];
  hasShapeNodeData: boolean;
  hasZoneCentroidData: boolean;
  focusStopRequest: FocusStopRequest;
  focusVehicleRequest: FocusVehicleRequest;
  setFocusVehicleRequest: (value: FocusVehicleRequest) => void;
  previewAnchorPoint: { x: number; y: number } | null;
  previewColor: string;
  buildConstraintMode: string | null;
  stationCostBase: number | null;
  lineCostPerKmBase: number | null;
  extensionAddedStations: number;
  extensionAddedLengthM: number;
  extensionConstructionCostBase: number | null;
  lineDraftMode: boolean;
  activeLineDraftPreview: DraftPreview;
  selectedLinePresetId: string | null;
  selectedLineDetail: ReturnType<typeof import("../build/helpers").computeLocalLineDetail> | null;
  selectedLineBuildPreset: ReturnType<typeof import("../build/helpers").getBuildPreset> | null;
  selectedLineStationDecorations: Record<
    string,
    {
      interchange: boolean;
      connectedLines: Array<{ lineId: string; lineName: string; displayColor?: string | null }>;
    }
  >;
  selectedLineEstimatedCapexBase: number | null;
  selectedLineScheduleState: {
    peak_start_minute: number;
    peak_end_minute: number;
    overnight_start_minute: number;
    overnight_end_minute: number;
    tph_peak: number;
    tph_off_peak: number;
    tph_overnight: number;
  } | null;
  selectedLineFleetEditorState: {
    packageId: string;
    unitsOwned: number;
    unitsCommitted: number;
    unitsPending: number;
    unitsAssigned: number;
    carsPerUnit: number;
    speedLevel: string;
    comfortLevel: string;
    requiredUnitsNow: number;
    pendingOrders: Array<{
      order_id: string;
      units: number;
      label?: string | null;
      status?: string | null;
      unit_cost_base?: number | null;
      total_cost_base?: number | null;
      placed_at_tick_s?: number | null;
      eta_at_tick_s?: number | null;
    }>;
  } | null;
  selectedLineUnitLabel: string;
  selectedLineActiveVehicles: Array<{
    vehicleId: string;
    label: string;
    destinationLabel: string;
    onBoard: number;
    capacity: number;
  }>;
  selectedLineTransferTargets: Array<{ lineId: string; lineName: string }>;
  selectedLineScrapEstimateBase: number | null;
  selectedStationLines: ReturnType<typeof import("../build/helpers").computeLocalStationLines>;
  selectedStationInterchangeContext: StationInterchangeContext;
  lineInspectorOpen: boolean;
  stationInspectorOpen: boolean;
  lineDeleteDialogEnabled: boolean;
  rollingStockEditorEnabled: boolean;
  scheduleEditorEnabled: boolean;
  rollingStockEditorOpen: boolean;
  scheduleEditorOpen: boolean;
  lineDeleteDialogOpen: boolean;
  commandActions: ShellCommandAction[];
  runPaletteCommand: (commandId: string) => void;
  financialBusy: boolean;
  financialError: string | null;
  financialRequest: FinancialDashboardRequest;
  setFinancialRequest: (updater: (previous: FinancialDashboardRequest) => FinancialDashboardRequest) => void;
  financialData: FinancialDashboardResponse | null;
  financialLineOptions: Array<{ lineId: string; name: string }>;
  onRefreshFinancialDashboard: () => Promise<void>;
  onHandleMapBootProgress: (payload: {
    stage: "map_style" | "map_context" | "ready" | "error";
    progress: number;
    message: string;
    error?: string | null;
  }) => void;
  onSelectCounty: (regionId: string) => void;
  onHandleStopAction: (payload: MapStopAction) => void;
  onHandleLineAction: (payload: { lineId: string }) => void;
  onHandleMapPointAction: (point: MapWorldPoint) => void;
  onHandleMapClearSelection: () => void;
  onHandleScrapVehicleFromMap: (trainId: string) => void;
  onRunPlanning: () => Promise<void>;
  onRebuildDemandForUnlocked: () => Promise<void>;
  onExportRunCsv: (runId: string) => Promise<void>;
  onExportRunJson: (runId: string) => Promise<void>;
  onCompareRuns: () => Promise<void>;
  onRequestDeleteSelectedLine: () => void;
  onFocusStationById: (stopId: string) => void;
  onOpenRollingStockEditorFromLineInspector: () => void;
  onOpenScheduleEditorFromLineInspector: () => void;
  onCancelDeleteSelectedLine: () => void;
  onDeleteSelectedLineWithScrap: () => void;
  onDeleteSelectedLineWithTransfer: (lineId: string) => void;
  onFocusVehicleFromFleet: (vehicleId: string) => void;
  onOpenRollingStockEditorFromSchedule: () => void;
  onCreateInterchangeGroupForSelectedStation: () => void;
  onClearSelectedStationInterchange: () => void;
  onApplySuggestedInterchange: (interchangeId: string) => void;
  onLeaveBuildMode: () => void;
  onNavigateFromAlert: (alert: AlertItem) => void;
  onRefreshDashboardFromHud: () => void;
  onExpediteFleetDelivery: (delivery: {
    id: string;
    orderId: string;
    label: string;
    lineId: string;
    lineName: string;
  }) => Promise<void>;
  onSaveSession: () => Promise<void>;
  onSaveQuit: () => Promise<void>;
  onSetRunning: (running: boolean) => Promise<void>;
  onSetSpeed: (speed: 1 | 2 | 4) => Promise<void>;
  onFocusSelectedCounty: () => Promise<void>;
  onUnlockAndFocusSelectedCounty: () => Promise<void>;
  onUpdateFarePolicy: (patch: Partial<FarePolicyManifest>) => void;
  onRetryMapLoad: () => void;
  closeLineEditors: () => void;
  setRollingStockEditorOpen: (value: boolean) => void;
  setScheduleEditorOpen: (value: boolean) => void;
}) {
  const isGame = props.sessionKind === "game";

  return (
    <div className={`session-shell ${props.build.workspaceMode === "build" ? "is-build-workspace" : ""}`}>
      <SessionHud
        sessionKind={props.sessionKind}
        projectName={props.bundle.manifest.name}
        clock={props.clock}
        budget={props.liveEconomy?.budget_display ?? props.bundle.manifest.progress_metrics?.budget ?? null}
        budgetCurrency={props.bundle.manifest.progress_metrics?.currency ?? "GBP"}
        alerts={props.shellStatus.visibleAlerts}
        alertsOpen={props.shellPanels.showAlerts}
        buildModeActive={props.build.workspaceMode === "build"}
        buildTransportLabel={props.currentBuildPreset?.label ?? null}
        menuOpen={props.shellPanels.showMenu}
        fleetDeliveries={props.fleetDeliveries}
        onMenuToggle={props.shellPanels.toggleMenuPanel}
        onOpenFinancialDashboard={props.onRefreshDashboardFromHud}
        onFocusLineFromFleet={(lineId) => {
          props.build.selectLine(lineId);
          props.closeLineEditors();
        }}
        onFocusVehicleFromFleet={(vehicleId) => {
          props.setFocusVehicleRequest({ vehicleId, token: Date.now() });
        }}
        onExpediteFleetDelivery={props.onExpediteFleetDelivery}
        onSave={() => {
          void props.onSaveSession();
        }}
        onSaveQuit={() => {
          void props.onSaveQuit();
        }}
        onOpenSettings={props.shellPanels.openSettingsPanel}
        onOpenCommandPalette={props.shellPanels.openCommandPaletteFromHud}
        onAlertsToggle={props.shellPanels.toggleAlertsPanel}
        onToggleRunning={(running) => {
          void props.onSetRunning(running);
        }}
        onSpeedChange={(speed) => {
          void props.onSetSpeed(speed);
        }}
      />
      {props.build.workspaceMode === "view" ? (
        <SessionSideHud
          sessionKind={props.sessionKind}
          buildModeActive={false}
          filtersOpen={props.shellPanels.showFilters}
          missionsOpen={props.shellPanels.showMissions}
          countryInfoOpen={props.shellPanels.showCountryInfo}
          faresOpen={props.shellPanels.showFares}
          onEnterBuildMode={props.build.enterBuildMode}
          onExitBuildMode={props.onLeaveBuildMode}
          onOpenFilters={props.shellPanels.toggleFiltersPanel}
          onOpenMissions={props.shellPanels.toggleMissionsPanel}
          onOpenCountryInfo={props.shellPanels.toggleCountryInfoPanel}
          onOpenFares={props.shellPanels.toggleFarePanel}
        />
      ) : null}

      <main className="session-main">
        <MapCanvas
          instanceToken={props.mapInstanceToken}
          scenario={props.activeScenario}
          projectPath={props.bundle.project_path}
          mapRuntimeConfig={props.mapRuntimeConfig}
          clock={props.clock}
          showShapeStops={props.showShapeStops}
          showZoneCentroids={props.showZoneCentroids}
          showStations={props.showStations}
          showLinks={props.showLinks}
          linkMode={props.linkMode}
          startCenter={props.startCenter}
          serviceLoadByServiceId={props.serviceLoadByServiceId}
          runtimeTrains={props.runtimeTrains}
          trainsAuthoritative={props.trainsAuthoritative}
          sessionKind={props.sessionKind}
          visibleCountryIso2={props.visibleCountryIso2}
          regions={props.regions}
          focusRegionId={props.focusRegionId}
          activeRegionIds={props.activeRegionIds}
          selectedRegionId={props.selectedRegionId}
          interactionMode={props.build.workspaceMode}
          buildAction={props.build.buildAction}
          buildConstraintMode={props.buildConstraintMode}
          selectedStopId={props.build.selection?.kind === "stop" ? props.build.selection.stopId : null}
          selectedLineId={props.build.selection?.kind === "line" ? props.build.selection.lineId : null}
          activeLineId={props.build.activeLine?.lineId ?? null}
          focusStopId={props.focusStopRequest?.stopId ?? null}
          focusStopToken={props.focusStopRequest?.token ?? 0}
          focusVehicleId={props.focusVehicleRequest?.vehicleId ?? null}
          focusVehicleToken={props.focusVehicleRequest?.token ?? 0}
          previewAnchorPoint={props.previewAnchorPoint}
          previewColor={props.previewColor}
          onBootProgress={props.onHandleMapBootProgress}
          onSelectCounty={props.onSelectCounty}
          onStopAction={props.onHandleStopAction}
          onLineAction={props.onHandleLineAction}
          onMapPointAction={props.onHandleMapPointAction}
          onClearSelection={props.onHandleMapClearSelection}
          onScrapVehicle={props.build.workspaceMode === "build" ? props.onHandleScrapVehicleFromMap : undefined}
        />

        <ScenarioControlsSidebar
          open={!isGame && props.build.workspaceMode === "view"}
          busy={props.busy}
          runs={props.bundle.runs}
          controller={props.scenarioSidebar}
          onRunPlanning={props.onRunPlanning}
          onRebuildDemand={props.onRebuildDemandForUnlocked}
          onExportRunCsv={props.onExportRunCsv}
          onExportRunJson={props.onExportRunJson}
          onCompareRuns={props.onCompareRuns}
        />

        <LineInspectorSheet
          open={props.lineInspectorOpen || props.lineDraftMode}
          inspection={props.build.lineInspection}
          lineDetail={props.selectedLineDetail}
          draftPreview={
            props.lineDraftMode
              ? props.activeLineDraftPreview
              : props.selectedLineDetail
                ? null
                : props.activeLineDraftPreview
          }
          forceDraftMode={props.lineDraftMode}
          editable={props.build.workspaceMode === "build" && Boolean(props.selectedLineDetail)}
          stationDecorations={props.selectedLineStationDecorations}
          presets={props.build.buildDefaults?.presets ?? []}
          selectedPresetId={props.selectedLinePresetId}
          budgetCurrency={props.budgetCurrency}
          estimatedCapexBase={props.selectedLineEstimatedCapexBase}
          stationCapexBase={props.build.buildDefaults?.station_capex_base ?? null}
          addingStationMode={props.build.buildAction === "add_station_to_line"}
          onClose={() => props.build.setSelection(null)}
          onAddStationToLine={() => props.build.armLineExtension()}
          onDelete={props.onRequestDeleteSelectedLine}
          onNameChange={(value) => props.build.updateSelectedLine({ name: value })}
          onColorChange={(value) => props.build.updateSelectedLine({ display_color: value })}
          onPresetChange={props.build.updateSelectedLinePreset}
          onStationClick={(stopId) => {
            props.onFocusStationById(stopId);
          }}
          onOpenRollingStockEditor={props.onOpenRollingStockEditorFromLineInspector}
          onOpenScheduleEditor={props.onOpenScheduleEditorFromLineInspector}
          onRemoveDraftStation={(stopId) => {
            props.build.removeStationFromActiveDraft(stopId);
          }}
        />

        <LineDeleteDialog
          open={props.lineDeleteDialogEnabled}
          lineName={props.selectedLineDetail?.name?.trim() ? props.selectedLineDetail.name : "Untitled Line"}
          unitLabel={props.selectedLineUnitLabel}
          unitsOwned={props.selectedLineFleetEditorState?.unitsOwned ?? 0}
          unitsPending={props.selectedLineFleetEditorState?.unitsPending ?? 0}
          budgetCurrency={props.budgetCurrency}
          estimatedScrapValueBase={props.selectedLineScrapEstimateBase}
          transferTargets={props.selectedLineTransferTargets}
          onCancel={props.onCancelDeleteSelectedLine}
          onConfirmScrap={props.onDeleteSelectedLineWithScrap}
          onConfirmTransfer={props.onDeleteSelectedLineWithTransfer}
        />

        <RollingStockEditorSheet
          open={props.rollingStockEditorEnabled}
          editable={props.build.workspaceMode === "build" && !props.lineDraftMode}
          lineName={props.selectedLineDetail?.name?.trim() ? props.selectedLineDetail.name : "Untitled Line"}
          budgetCurrency={props.budgetCurrency}
          modeId={props.selectedLineBuildPreset?.engine_mode ?? props.selectedLineDetail?.mode ?? null}
          preset={props.selectedLineBuildPreset}
          packageId={props.selectedLineFleetEditorState?.packageId ?? "standard"}
          unitsOwned={props.selectedLineFleetEditorState?.unitsOwned ?? 0}
          unitsCommitted={
            props.selectedLineFleetEditorState?.unitsCommitted ?? props.selectedLineFleetEditorState?.unitsOwned ?? 0
          }
          unitsPending={props.selectedLineFleetEditorState?.unitsPending ?? 0}
          unitsAssigned={props.selectedLineFleetEditorState?.unitsAssigned ?? 0}
          carsPerUnit={props.selectedLineFleetEditorState?.carsPerUnit ?? 1}
          speedLevel={props.selectedLineFleetEditorState?.speedLevel ?? "balanced"}
          comfortLevel={props.selectedLineFleetEditorState?.comfortLevel ?? "standard"}
          requiredUnitsNow={props.selectedLineFleetEditorState?.requiredUnitsNow ?? 0}
          pendingOrders={props.selectedLineFleetEditorState?.pendingOrders ?? []}
          activeVehicles={props.selectedLineActiveVehicles}
          currentTickS={props.clock?.tick_seconds ?? 0}
          clockRunning={props.clock?.running ?? false}
          clockSpeed={props.clock?.speed ?? 1}
          onClose={() => props.setRollingStockEditorOpen(false)}
          onSave={(patch) => props.build.updateSelectedLineOperations({ fleet: patch })}
          onFocusVehicle={props.onFocusVehicleFromFleet}
        />

        <ScheduleEditorSheet
          open={props.scheduleEditorEnabled}
          editable={props.build.workspaceMode === "build" && !props.lineDraftMode}
          lineName={props.selectedLineDetail?.name?.trim() ? props.selectedLineDetail.name : "Untitled Line"}
          budgetCurrency={props.budgetCurrency}
          preset={props.selectedLineBuildPreset}
          unitsOwned={props.selectedLineFleetEditorState?.unitsOwned ?? 0}
          roundTripS={props.selectedLineDetail?.roundTripS ?? 0}
          schedule={
            props.selectedLineScheduleState ?? {
              peak_start_minute: 420,
              peak_end_minute: 570,
              overnight_start_minute: 0,
              overnight_end_minute: 300,
              tph_peak: 0,
              tph_off_peak: 0,
              tph_overnight: 0,
            }
          }
          onClose={() => props.setScheduleEditorOpen(false)}
          onOpenRollingStockEditor={props.onOpenRollingStockEditorFromSchedule}
          onSave={(patch) => props.build.updateSelectedLineOperations({ schedule: patch })}
        />

        <StationInspectorModal
          open={props.stationInspectorOpen}
          stop={props.build.selectedStop}
          inspection={props.build.stationInspection}
          localLines={props.selectedStationLines}
          interchangeMembers={props.selectedStationInterchangeContext.members}
          suggestedInterchanges={props.selectedStationInterchangeContext.suggestions}
          transferLinks={props.selectedStationInterchangeContext.transfers}
          editable={props.build.workspaceMode === "build"}
          onClose={() => props.build.setSelection(null)}
          onNameChange={props.build.renameSelectedStation}
          onInterchangeChange={props.build.updateSelectedStationInterchange}
          onCreateInterchangeGroup={props.onCreateInterchangeGroupForSelectedStation}
          onClearInterchangeGroup={props.onClearSelectedStationInterchange}
          onApplySuggestedInterchange={props.onApplySuggestedInterchange}
          onSelectLinkedStop={props.onFocusStationById}
          onDelete={props.build.deleteSelectedStation}
        />
      </main>

      {props.build.workspaceMode === "build" && (
        <BuildPalette
          presets={props.build.buildDefaults?.presets ?? []}
          lines={props.build.lineSummaries.map((line) => ({
            lineId: line.lineId,
            name: line.name,
            mode: line.mode,
            modeVariant: line.modeVariant ?? null,
            displayColor: line.displayColor ?? null,
          }))}
          transportPresetId={props.build.transportPresetId}
          buildAction={props.build.buildAction}
          hasSelectedLine={props.build.selection?.kind === "line"}
          hasSelectedStop={props.build.selection?.kind === "stop"}
          selectedLineId={props.build.selection?.kind === "line" ? props.build.selection.lineId : null}
          selectedLineName={props.selectedLineDetail?.name ?? props.build.lineInspection?.name ?? null}
          selectedStopName={props.build.selectedStop?.name ?? null}
          selectedLineConstructionCostBase={props.selectedLineEstimatedCapexBase}
          mutationPreview={props.build.mutationPreview}
          isDirty={props.build.isDirty}
          builderBusy={props.build.builderBusy}
          builderError={props.build.builderError}
          budgetCurrency={props.budgetCurrency}
          stationCostBase={props.stationCostBase}
          lineCostPerKmBase={props.lineCostPerKmBase}
          extensionAddedStations={props.extensionAddedStations}
          extensionAddedLengthM={props.extensionAddedLengthM}
          extensionConstructionCostBase={props.extensionConstructionCostBase}
          activeLineStopCount={props.build.activeLine?.stationIds.length ?? 0}
          canUndoDraftPlacement={(props.build.activeLine?.stationIds.length ?? 0) > 1}
          onExitBuildMode={props.onLeaveBuildMode}
          onSelectBuildAction={props.build.selectBuildAction}
          onArmLineExtension={() => props.build.armLineExtension()}
          onTransportPresetChange={props.build.setTransportPresetId}
          onSelectLine={props.build.selectLine}
          onApplyDraft={() => props.build.applyDraft()}
          onFinishLine={props.build.finishLineDraw}
          onUndoLinePlacement={props.build.undoActiveLinePlacement}
        />
      )}
      {isGame && props.build.workspaceMode === "view" && (
        <CountryInfoDrawer
          open={props.shellPanels.showCountryInfo}
          busy={props.busy}
          regions={props.regions}
          selectedRegionId={props.selectedRegionId}
          focusRegionId={props.focusRegionId}
          currentBalanceBase={props.currentBalanceBase}
          onClose={() => props.shellPanels.setShowCountryInfo(false)}
          onSelectRegion={props.onSelectCounty}
          onFocusRegion={() => {
            void props.onFocusSelectedCounty();
          }}
          onUnlockRegion={() => {
            void props.onUnlockAndFocusSelectedCounty();
          }}
        />
      )}
      {isGame && props.build.workspaceMode === "view" && (
        <FarePolicyPanel
          open={props.shellPanels.showFares}
          busy={props.busy}
          policy={props.farePolicy}
          onClose={() => props.shellPanels.setShowFares(false)}
          onChange={props.onUpdateFarePolicy}
        />
      )}

      {props.build.workspaceMode === "view" && (
        <MapFiltersPanel
          open={props.shellPanels.showFilters}
          onClose={() => props.shellPanels.setShowFilters(false)}
          showStations={props.showStations}
          showLinks={props.showLinks}
          showZoneCentroids={props.showZoneCentroids}
          showShapeStops={props.showShapeStops}
          hasZoneCentroidData={props.hasZoneCentroidData}
          hasShapeNodeData={props.hasShapeNodeData}
          linkMode={props.linkMode}
          onShowStationsChange={props.setShowStations}
          onShowLinksChange={props.setShowLinks}
          onShowZoneCentroidsChange={props.setShowZoneCentroids}
          onShowShapeStopsChange={props.setShowShapeStops}
          onLinkModeChange={props.setLinkMode}
        />
      )}
      {props.build.workspaceMode === "view" && (
        <MissionsDrawer
          open={props.shellPanels.showMissions}
          missions={props.missions}
          onClose={() => props.shellPanels.setShowMissions(false)}
        />
      )}

      <AlertsCenter
        open={props.shellPanels.showAlerts}
        alerts={props.shellStatus.visibleAlerts}
        onClose={() => props.shellPanels.setShowAlerts(false)}
        onNavigate={props.onNavigateFromAlert}
        onDismiss={props.shellStatus.dismissAlert}
      />

      <SettingsPanel
        open={props.shellPanels.showSettings}
        settings={props.shellStatus.uiSettings}
        onClose={() => props.shellPanels.setShowSettings(false)}
        onChange={props.shellStatus.setUiSettings}
        onReset={() => props.shellStatus.setUiSettings(props.defaultUiSettings)}
      />

      <CommandPalette
        open={props.shellPanels.commandPaletteOpen}
        query={props.shellPanels.commandPaletteQuery}
        commands={props.commandActions.map((command) => ({
          id: command.id,
          label: command.label,
          detail: command.detail,
          shortcut: command.shortcut,
          disabled: command.disabled,
        }))}
        onQueryChange={props.shellPanels.setCommandPaletteQuery}
        onRun={props.runPaletteCommand}
        onClose={props.shellPanels.closeCommandPalette}
      />

      <DiagnosticsOverlay
        open={props.shellStatus.uiSettings.showDiagnostics}
        fps={props.shellStatus.fps}
        frameMs={props.shellStatus.frameMs}
        telemetry={props.runtimeTelemetry}
        snapshotLatencyMs={props.snapshotLatencyMs}
        mapComplexityScore={props.mapComplexityScore}
      />
      <ShellStatusOverlays
        busy={props.busy}
        showSessionBootOverlay={props.shellStatus.showSessionBootOverlay}
        sessionBootState={props.sessionBootState}
        onRetryMapLoad={props.onRetryMapLoad}
        isOffline={props.shellStatus.isOffline}
        showPausedBanner={!props.clock.running && props.build.workspaceMode !== "build"}
        saveStatus={props.saveStatus}
        onDismissSaveStatus={() => props.setSaveStatus("")}
        onboardingActive={props.shellStatus.onboardingActive && props.sessionKind === "game"}
        onboardingStep={props.shellStatus.onboardingStep}
        onboardingStepCount={props.shellStatus.onboardingStepCount}
        onboardingTitle={props.shellStatus.onboardingStepInfo.title}
        onboardingDescription={props.shellStatus.onboardingStepInfo.description}
        onSkipOnboarding={props.shellStatus.skipOnboarding}
        onAdvanceOnboarding={props.shellStatus.advanceOnboardingStep}
      />
      {props.demandWarning && <div className="global-error">{props.demandWarning}</div>}
      <FinancialDashboardModal
        open={props.shellPanels.showFinancialDashboard}
        busy={props.financialBusy}
        error={props.financialError}
        currency={props.budgetCurrency}
        request={props.financialRequest}
        data={props.financialData}
        regions={props.regions}
        lineOptions={props.financialLineOptions}
        onRequestChange={(patch) =>
          props.setFinancialRequest((previous) => ({
            ...previous,
            ...patch,
          }))
        }
        onRefresh={props.onRefreshFinancialDashboard}
        onClose={() => props.shellPanels.setShowFinancialDashboard(false)}
      />
      {props.error ? (
        <div className="global-error global-error-floating">
          <span>{props.error}</span>
          <button onClick={() => props.setError(null)}>Dismiss</button>
        </div>
      ) : null}
    </div>
  );
}
