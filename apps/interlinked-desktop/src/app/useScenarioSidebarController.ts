import { useCallback, useMemo, useState } from "react";
import type { CompareResult, PlanningRunConfig } from "../types";

const DEFAULT_RUN_CONFIG: PlanningRunConfig = {
  deterministic_seed: 42,
  horizon_s: 3600,
  time_bin_s: 300,
  time_of_day_s: 28800,
};

function parseNullableNumber(value: string): number | null {
  return value ? Number(value) : null;
}

export function useScenarioSidebarController() {
  const [runConfig, setRunConfig] = useState<PlanningRunConfig>(DEFAULT_RUN_CONFIG);
  const [compareResult, setCompareResult] = useState<CompareResult | null>(null);
  const [selectedBaseRun, setSelectedBaseRun] = useState("");
  const [selectedCandidateRun, setSelectedCandidateRun] = useState("");

  const onSeedChanged = useCallback((value: string) => {
    setRunConfig((previous) => ({
      ...previous,
      deterministic_seed: parseNullableNumber(value),
    }));
  }, []);

  const onHorizonChanged = useCallback((value: string) => {
    setRunConfig((previous) => ({
      ...previous,
      horizon_s: parseNullableNumber(value),
    }));
  }, []);

  const onTimeBinChanged = useCallback((value: string) => {
    setRunConfig((previous) => ({
      ...previous,
      time_bin_s: parseNullableNumber(value),
    }));
  }, []);

  const compareSummary = useMemo(() => {
    if (!compareResult) return null;
    return `Delta trips: ${compareResult.delta.kpis.total_trips.toFixed(2)} | Delta GC: ${compareResult.delta.kpis.mean_generalized_cost_s.toFixed(2)}`;
  }, [compareResult]);

  return {
    runConfig,
    compareResult,
    compareSummary,
    selectedBaseRun,
    selectedCandidateRun,
    setCompareResult,
    setSelectedBaseRun,
    onSeedChanged,
    onHorizonChanged,
    onTimeBinChanged,
    onSelectedBaseRunChanged: setSelectedBaseRun,
    onSelectedCandidateRunChanged: setSelectedCandidateRun,
  };
}

export type ScenarioSidebarController = ReturnType<typeof useScenarioSidebarController>;
