import { useCallback, type Dispatch, type SetStateAction } from "react";
import type { BuildAction, BuilderSelection, MapWorldPoint } from "../build/types";

export type FocusStopRequest = {
  stopId: string;
  token: number;
} | null;

type BuildInteractionPort = {
  workspaceMode: "view" | "build";
  buildAction: BuildAction;
  selection: BuilderSelection;
  selectedStop: { name?: string | null } | null;
  handleBuildPoint: (point: MapWorldPoint, snappedStopId?: string) => void;
  selectStop: (stopId: string) => void;
  selectLine: (lineId: string) => void;
  setSelection: (selection: null) => void;
  updateSelectedStationInterchange: (interchangeId: string) => void;
};

export function useMapBuildInteractions(args: {
  build: BuildInteractionPort;
  setFocusStopRequest: Dispatch<SetStateAction<FocusStopRequest>>;
}) {
  const { build, setFocusStopRequest } = args;

  const handleStopAction = useCallback(
    (payload: { stopId: string; point: MapWorldPoint }) => {
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
    },
    [build]
  );

  const handleLineAction = useCallback(
    (payload: { lineId: string }) => {
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
    },
    [build]
  );

  const handleMapPointAction = useCallback(
    (point: MapWorldPoint) => {
      if (build.workspaceMode !== "build") return;
      build.handleBuildPoint(point);
    },
    [build]
  );

  const handleMapClearSelection = useCallback(() => {
    if (build.workspaceMode === "build" && build.buildAction !== "select") return;
    build.setSelection(null);
  }, [build]);

  const focusStationById = useCallback(
    (stopId: string) => {
      build.selectStop(stopId);
      setFocusStopRequest((previous) => ({
        stopId,
        token: (previous?.token ?? 0) + 1,
      }));
    },
    [build, setFocusStopRequest]
  );

  const createInterchangeGroupForSelectedStation = useCallback(() => {
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
  }, [build]);

  const clearSelectedStationInterchange = useCallback(() => {
    if (build.selection?.kind !== "stop") return;
    build.updateSelectedStationInterchange("");
  }, [build]);

  const applySuggestedInterchange = useCallback(
    (interchangeId: string) => {
      if (build.selection?.kind !== "stop") return;
      build.updateSelectedStationInterchange(interchangeId);
    },
    [build]
  );

  return {
    handleStopAction,
    handleLineAction,
    handleMapPointAction,
    handleMapClearSelection,
    focusStationById,
    createInterchangeGroupForSelectedStation,
    clearSelectedStationInterchange,
    applySuggestedInterchange,
  };
}
