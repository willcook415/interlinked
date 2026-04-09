import { useCallback } from "react";

import {
  openProject,
  setPrimaryFocusRegion,
  unlockAndFocusRegion,
} from "../../api/desktopApi";
import type {
  FocusResult,
  OpenSessionResult,
  UnlockFocusResult,
} from "../../types";
import type {
  UseSessionControllerParams,
  WithBusy,
} from "./contracts";

type UseRegionScopeControllerParams = {
  params: UseSessionControllerParams;
  withBusy: WithBusy;
  applyOpenedSession: (opened: OpenSessionResult) => void;
  refreshLibraries: () => Promise<void>;
};

type UseRegionScopeControllerResult = {
  selectCounty: (regionId: string) => void;
  focusSelectedCounty: () => Promise<void>;
  unlockAndFocusSelectedCounty: () => Promise<void>;
};

export function useRegionScopeController({
  params,
  withBusy,
  applyOpenedSession,
  refreshLibraries,
}: UseRegionScopeControllerParams): UseRegionScopeControllerResult {
  const selectCounty = useCallback(
    (regionId: string) => {
      params.setSelectedRegionId(regionId);
      const region = params.regions.find((row) => row.region_id === regionId) ?? null;
      params.setShowCountryInfo(Boolean(region && !region.unlocked));
    },
    [params]
  );

  const focusSelectedCounty = useCallback(async () => {
    if (!params.bundle || !params.selectedRegionId || params.sessionKind !== "game") return;
    const reopened = await withBusy(async () => {
      await setPrimaryFocusRegion(params.bundle!.project_path, params.selectedRegionId!) as FocusResult;
      return openProject(params.bundle!.project_path);
    });
    if (!reopened) return;
    applyOpenedSession(reopened);
    params.setSaveStatus(`Focused county ${params.selectedRegionId}`);
  }, [applyOpenedSession, params, withBusy]);

  const unlockAndFocusSelectedCounty = useCallback(async () => {
    if (!params.bundle || !params.selectedRegionId || params.sessionKind !== "game") return;
    let focusedRegionId = params.selectedRegionId;
    const reopened = await withBusy(async () => {
      const unlock = (await unlockAndFocusRegion(
        params.bundle!.project_path,
        params.selectedRegionId!
      )) as UnlockFocusResult;
      if (unlock.region_id?.trim()) focusedRegionId = unlock.region_id.trim();
      return openProject(params.bundle!.project_path);
    });
    if (!reopened) return;
    applyOpenedSession(reopened);
    params.setSaveStatus(`Unlocked and focused county ${focusedRegionId}`);
    await refreshLibraries();
  }, [applyOpenedSession, params, refreshLibraries, withBusy]);

  return {
    selectCounty,
    focusSelectedCounty,
    unlockAndFocusSelectedCounty,
  };
}
