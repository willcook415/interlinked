import { useCallback } from "react";

import {
  listDemandCoverage,
  listRegions,
  setPrimaryFocusRegion,
  unlockAndFocusRegion,
} from "../../api/desktopApi";
import type {
  DemandCoverageMeta,
  FocusResult,
  RegionStatus,
  UnlockFocusResult,
} from "../../types";
import type {
  UseSessionControllerParams,
  WithBusy,
} from "./contracts";

type UseRegionScopeControllerParams = {
  params: UseSessionControllerParams;
  withBusy: WithBusy;
};

type UseRegionScopeControllerResult = {
  selectCounty: (regionId: string) => void;
  focusSelectedCounty: () => Promise<void>;
  unlockAndFocusSelectedCounty: () => Promise<void>;
};

type RegionScopeMutation = FocusResult | UnlockFocusResult;

function pickFocusRegionId(rows: RegionStatus[], preferredId: string | null): string | null {
  if (rows.length === 0) return null;
  if (preferredId && rows.some((row) => row.region_id === preferredId)) return preferredId;
  return (
    rows.find((row) => row.active)?.region_id ??
    rows.find((row) => row.unlocked)?.region_id ??
    rows[0].region_id
  );
}

function demandWarningMessage(rows: DemandCoverageMeta[]): string | null {
  const missing = rows.filter((row) => !row.installed).map((row) => row.country_iso2);
  if (missing.length === 0) return null;
  return `Demand data not installed for ${missing.join(", ")}. Install demand surface packs to enable country coverage.`;
}

export function useRegionScopeController({
  params,
  withBusy,
}: UseRegionScopeControllerParams): UseRegionScopeControllerResult {
  const selectCounty = useCallback(
    (regionId: string) => {
      params.setSelectedRegionId(regionId);
      const region = params.regions.find((row) => row.region_id === regionId) ?? null;
      const sourceCode = (region?.source_code ?? "").trim().toLowerCase();
      const isManualUnassignedHex = sourceCode === "manual_region_unassigned_hex";
      params.setShowCountryInfo(Boolean(region && (!region.unlocked || isManualUnassignedHex)));
    },
    [params]
  );

  const applyRegionMutation = useCallback(
    (mutation: RegionScopeMutation) => {
      params.setBundle((previous) => {
        if (!previous) return previous;
        const nextEconomy = previous.manifest.economy
          ? {
              ...previous.manifest.economy,
              current_balance_base: mutation.current_balance_base,
              unlocked_countries: mutation.unlocked_countries,
            }
          : previous.manifest.economy;
        return {
          ...previous,
          manifest: {
            ...previous.manifest,
            region_state: {
              primary_focus_region_id: mutation.primary_focus_region_id,
              active_region_ids: mutation.active_region_ids,
              unlocked_region_ids: mutation.unlocked_region_ids,
            },
            economy: nextEconomy,
          },
          scenario: mutation.scenario,
        };
      });
      params.setLiveEconomy((previous) => {
        if (!previous) {
          return {
            current_balance_base: mutation.current_balance_base,
            cumulative_revenue_base:
              params.bundle?.manifest.economy?.cumulative_revenue_base ?? 0,
            cumulative_opex_base:
              params.bundle?.manifest.economy?.cumulative_opex_base ?? 0,
            budget_display:
              params.bundle?.manifest.progress_metrics?.budget ??
              mutation.current_balance_base,
          };
        }
        return {
          ...previous,
          current_balance_base: mutation.current_balance_base,
        };
      });
    },
    [params]
  );

  const applyRegionRows = useCallback(
    (rows: RegionStatus[], preferredFocusId: string | null) => {
      params.setRegions(rows);
      if (rows.length === 0) {
        params.setFocusRegionId(null);
        params.setSelectedRegionId(null);
        return;
      }
      const resolvedFocus = pickFocusRegionId(rows, preferredFocusId);
      params.setFocusRegionId(resolvedFocus);
      params.setSelectedRegionId((previous) => {
        if (previous && rows.some((row) => row.region_id === previous)) {
          return previous;
        }
        return resolvedFocus;
      });
    },
    [params]
  );

  const applyDemandCoverageRows = useCallback(
    (rows: DemandCoverageMeta[]) => {
      params.setDemandCoverage(rows);
      params.setDemandWarning(demandWarningMessage(rows));
    },
    [params]
  );

  const focusSelectedCounty = useCallback(async () => {
    if (!params.bundle || !params.selectedRegionId || params.sessionKind !== "game") return;
    const selectedRegionId = params.selectedRegionId;
    const projectPath = params.bundle.project_path;
    const outcome = await withBusy(async () => {
      const focus = (await setPrimaryFocusRegion(projectPath, selectedRegionId)) as FocusResult;
      const [regions, demandCoverage] = await Promise.all([
        listRegions(projectPath).catch(() => [] as RegionStatus[]),
        listDemandCoverage(projectPath).catch(() => [] as DemandCoverageMeta[]),
      ]);
      return { focus, regions, demandCoverage };
    });
    if (!outcome) return;

    applyRegionMutation(outcome.focus);
    applyRegionRows(outcome.regions, outcome.focus.primary_focus_region_id);
    applyDemandCoverageRows(outcome.demandCoverage);
    params.setShowCountryInfo(false);
    params.setSaveStatus("Focused selected region");
  }, [
    applyDemandCoverageRows,
    applyRegionMutation,
    applyRegionRows,
    params,
    withBusy,
  ]);

  const unlockAndFocusSelectedCounty = useCallback(async () => {
    if (!params.bundle || !params.selectedRegionId || params.sessionKind !== "game") return;
    const selectedRegionId = params.selectedRegionId;
    const projectPath = params.bundle.project_path;
    const outcome = await withBusy(async () => {
      const unlock = (await unlockAndFocusRegion(
        projectPath,
        selectedRegionId
      )) as UnlockFocusResult;
      const [regions, demandCoverage] = await Promise.all([
        listRegions(projectPath).catch(() => [] as RegionStatus[]),
        listDemandCoverage(projectPath).catch(() => [] as DemandCoverageMeta[]),
      ]);
      return { unlock, regions, demandCoverage };
    });
    if (!outcome) return;

    applyRegionMutation(outcome.unlock);
    applyRegionRows(outcome.regions, outcome.unlock.primary_focus_region_id);
    applyDemandCoverageRows(outcome.demandCoverage);
    params.setShowCountryInfo(false);
    params.setSaveStatus("Unlocked and focused selected region");
  }, [
    applyDemandCoverageRows,
    applyRegionMutation,
    applyRegionRows,
    params,
    withBusy,
  ]);

  return {
    selectCounty,
    focusSelectedCounty,
    unlockAndFocusSelectedCounty,
  };
}
