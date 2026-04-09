import { useCallback } from "react";

import {
  compareRuns as compareRunsRequest,
  openProject,
  rebuildDemandForUnlocked as rebuildDemandForUnlockedRequest,
  runPlanning as runPlanningRequest,
} from "../../api/desktopApi";
import type { CompareResult, OpenSessionResult } from "../../types";
import type { UseSessionControllerParams, WithBusy } from "./contracts";

type UsePlanningRefreshParams = {
  params: UseSessionControllerParams;
  withBusy: WithBusy;
  applyOpenedSession: (opened: OpenSessionResult) => void;
};

type UsePlanningRefreshResult = {
  runPlanning: () => Promise<void>;
  compareRuns: () => Promise<void>;
  rebuildDemandForUnlocked: () => Promise<void>;
};

export function usePlanningRefresh({
  params,
  withBusy,
  applyOpenedSession,
}: UsePlanningRefreshParams): UsePlanningRefreshResult {
  const runPlanning = useCallback(async () => {
    if (!params.bundle) return;
    const run = await withBusy(async () =>
      runPlanningRequest(params.bundle!.project_path, params.runConfig)
    );
    if (!run) return;
    params.setBundle((previous) => {
      if (!previous) return previous;
      const deduped = previous.runs.filter((entry) => entry.run_id !== run.run_id);
      return { ...previous, runs: [run, ...deduped] };
    });
    params.setSelectedBaseRun(run.run_id);
  }, [params, withBusy]);

  const compareRuns = useCallback(async () => {
    if (!params.bundle || !params.selectedBaseRun || !params.selectedCandidateRun) return;
    const result = await withBusy(async () =>
      compareRunsRequest(
        params.bundle!.project_path,
        params.selectedBaseRun,
        params.selectedCandidateRun
      )
    );
    if (result) params.setCompareResult(result as CompareResult);
  }, [params, withBusy]);

  const rebuildDemandForUnlocked = useCallback(async () => {
    if (!params.bundle) return;
    const rebuilt = await withBusy(async () =>
      rebuildDemandForUnlockedRequest(params.bundle!.project_path)
    );
    if (!rebuilt) return;
    const reopened = await withBusy(async () => openProject(params.bundle!.project_path));
    if (!reopened) return;
    applyOpenedSession(reopened);
    params.setSaveStatus(
      `Demand rebuilt: loaded ${rebuilt.loaded_countries.length}, missing ${rebuilt.missing_countries.length}`
    );
  }, [applyOpenedSession, params, withBusy]);

  return {
    runPlanning,
    compareRuns,
    rebuildDemandForUnlocked,
  };
}
