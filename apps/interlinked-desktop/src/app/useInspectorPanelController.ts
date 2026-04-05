import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import {
  computeLocalLineDetail,
  computeLocalStationLines,
  getBuildPreset,
  serviceLineId,
  stopDisplayName,
} from "../build/helpers";
import type { PurchaseOrderLite, ScenarioLite, TrainRuntimeView } from "../types";

export type FocusVehicleRequest = {
  vehicleId: string;
  token: number;
} | null;

type BuildControllerState = ReturnType<typeof import("../build/useBuildController").useBuildController>;

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

type FleetEditorState = {
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
};

const EMPTY_STATION_INTERCHANGE_CONTEXT: StationInterchangeContext = {
  members: [],
  suggestions: [],
  transfers: [],
};

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

export function useInspectorPanelController(args: {
  build: BuildControllerState;
  activeScenario: ScenarioLite | null;
  scenario: ScenarioLite | null;
  runtimeTrains: TrainRuntimeView[];
  setFocusVehicleRequest: Dispatch<SetStateAction<FocusVehicleRequest>>;
}) {
  const { build, activeScenario, scenario, runtimeTrains, setFocusVehicleRequest } = args;

  const [rollingStockEditorOpen, setRollingStockEditorOpen] = useState(false);
  const [scheduleEditorOpen, setScheduleEditorOpen] = useState(false);
  const [lineDeleteDialogOpen, setLineDeleteDialogOpen] = useState(false);

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

  const selectedStationInterchangeContext = useMemo<StationInterchangeContext>(() => {
    if (!activeScenario || build.selection?.kind !== "stop") {
      return EMPTY_STATION_INTERCHANGE_CONTEXT;
    }
    const stops = activeScenario.world.stops.filter(
      (stop) => !String(stop.stop_type ?? "").toLowerCase().includes("shape")
    );
    const selectedStopId = build.selection.kind === "stop" ? build.selection.stopId : null;
    const selectedStop = stops.find((stop) => stop.id === selectedStopId) ?? null;
    if (!selectedStop) {
      return EMPTY_STATION_INTERCHANGE_CONTEXT;
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
        const penaltyS = Math.round(
          Math.min(row.penaltyOutS ?? Number.POSITIVE_INFINITY, row.penaltyInS ?? Number.POSITIVE_INFINITY)
        );
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

  const selectedLineFleetEditorState = useMemo<FleetEditorState | null>(() => {
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
      unitsCommitted: fleetState?.units_committed ?? selectedLineDetail?.stockUnitsCommitted ?? unitsOwned,
      unitsPending: fleetState?.units_pending ?? selectedLineDetail?.stockUnitsPending ?? 0,
      unitsAssigned,
      carsPerUnit: fleetState?.cars_per_unit ?? selectedLineDetail?.carsPerUnit ?? 1,
      speedLevel: fleetState?.speed_level ?? selectedLineDetail?.speedLevel ?? "balanced",
      comfortLevel: fleetState?.comfort_level ?? selectedLineDetail?.comfortLevel ?? "standard",
      requiredUnitsNow: fleetState?.units_required_now ?? selectedLineDetail?.requiredUnits ?? 0,
      pendingOrders: fleetState?.pending_orders ?? selectedLineDetail?.pendingOrders ?? [],
    };
  }, [build.lineInspection?.fleet_state, selectedLineDetail]);

  const selectedLineUnitLabel = useMemo(
    () => unitLabelForMode(selectedLineBuildPreset?.engine_mode ?? selectedLineDetail?.mode ?? null),
    [selectedLineBuildPreset?.engine_mode, selectedLineDetail?.mode]
  );

  const selectedLineActiveVehicles = useMemo(() => {
    if (!selectedLineDetail)
      return [] as Array<{
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
    if (!lineDeleteDialogOpen) return;
    if (build.selection?.kind === "line" && selectedLineDetail) return;
    setLineDeleteDialogOpen(false);
  }, [build.selection, lineDeleteDialogOpen, selectedLineDetail]);

  const closeLineEditors = useCallback(() => {
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }, []);

  const focusVehicleFromFleet = useCallback(
    (vehicleId: string) => {
      setFocusVehicleRequest((previous) => ({
        vehicleId,
        token: (previous?.token ?? 0) + 1,
      }));
    },
    [setFocusVehicleRequest]
  );

  const handleScrapVehicleFromMap = useCallback(
    (vehicleId: string) => {
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
        setFocusVehicleRequest((previous) => (previous?.vehicleId === vehicleId ? null : previous));
      }
    },
    [build, runtimeTrains, selectedLineBuildPreset?.engine_mode, setFocusVehicleRequest]
  );

  const openRollingStockEditorFromLineInspector = useCallback(() => {
    if (!selectedLineBuildPreset) {
      build.setBuilderError("Rolling stock data is unavailable for this line preset.");
      return;
    }
    setRollingStockEditorOpen(true);
    setScheduleEditorOpen(false);
  }, [build, selectedLineBuildPreset]);

  const openScheduleEditorFromLineInspector = useCallback(() => {
    setScheduleEditorOpen(true);
    setRollingStockEditorOpen(false);
  }, []);

  const openRollingStockEditorFromSchedule = useCallback(() => {
    setScheduleEditorOpen(false);
    setRollingStockEditorOpen(true);
  }, []);

  const requestDeleteSelectedLine = useCallback(() => {
    if (!selectedLineDetail) return;
    setLineDeleteDialogOpen(true);
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }, [selectedLineDetail]);

  const cancelDeleteSelectedLine = useCallback(() => {
    setLineDeleteDialogOpen(false);
  }, []);

  const deleteSelectedLineWithScrap = useCallback(() => {
    const ok = build.deleteSelectedLineWithDisposition("scrap");
    if (!ok) return;
    setLineDeleteDialogOpen(false);
    setRollingStockEditorOpen(false);
    setScheduleEditorOpen(false);
  }, [build]);

  const deleteSelectedLineWithTransfer = useCallback(
    (targetLineId: string) => {
      const ok = build.deleteSelectedLineWithDisposition("transfer", targetLineId);
      if (!ok) return;
      setLineDeleteDialogOpen(false);
      setRollingStockEditorOpen(false);
      setScheduleEditorOpen(false);
    },
    [build]
  );

  const stationInspectorOpen =
    build.selection?.kind === "stop" &&
    !(
      build.workspaceMode === "build" &&
      (build.buildAction === "start_line" || build.buildAction === "add_station_to_line")
    );

  const lineInspectorOpen = build.selection?.kind === "line";

  const lineDeleteDialogEnabled =
    lineDeleteDialogOpen && build.selection?.kind === "line" && Boolean(selectedLineDetail);

  const rollingStockEditorEnabled =
    rollingStockEditorOpen &&
    build.selection?.kind === "line" &&
    Boolean(selectedLineDetail) &&
    Boolean(selectedLineFleetEditorState) &&
    Boolean(selectedLineBuildPreset);

  const scheduleEditorEnabled =
    scheduleEditorOpen &&
    build.selection?.kind === "line" &&
    Boolean(selectedLineDetail) &&
    Boolean(selectedLineScheduleState);

  return {
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
  };
}
