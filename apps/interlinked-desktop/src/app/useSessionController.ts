import { useCallback } from "react";

import { useEconomyRefresh, useFarePolicyController } from "./session/useEconomyRefresh";
import { useExportController } from "./session/useExportController";
import { usePlanningRefresh } from "./session/usePlanningRefresh";
import { useRegionScopeController } from "./session/useRegionScopeController";
import { useRuntimeController } from "./session/useRuntimeController";
import { useSessionLifecycle, useSessionMapReadyBridge } from "./session/useSessionLifecycle";
import type {
  SessionControllerResult,
  UseSessionControllerParams,
  WithBusy,
} from "./session/contracts";

export type { SessionBootState, MapBootProgressPayload } from "./session/contracts";

export function useSessionController(params: UseSessionControllerParams): SessionControllerResult {
  const withBusy = useCallback(
    async <T,>(fn: () => Promise<T>): Promise<T | null> => {
      params.setBusy(true);
      params.setError(null);
      try {
        return await fn();
      } catch (error) {
        params.setError(params.formatBackendError(error));
        params.playUiCue("error");
        return null;
      } finally {
        params.setBusy(false);
      }
    },
    [params]
  ) as WithBusy;

  const lifecycle = useSessionLifecycle({
    params,
    withBusy,
  });

  const farePolicyController = useFarePolicyController(params);
  useSessionMapReadyBridge(params);

  const regionScope = useRegionScopeController({
    params,
    withBusy,
    applyOpenedSession: lifecycle.applyOpenedSession,
    refreshLibraries: lifecycle.refreshLibraries,
  });

  const planning = usePlanningRefresh({
    params,
    withBusy,
    applyOpenedSession: lifecycle.applyOpenedSession,
  });

  const exportController = useExportController({
    params,
    withBusy,
    applyOpenedSession: lifecycle.applyOpenedSession,
    refreshLibraries: lifecycle.refreshLibraries,
  });

  const economy = useEconomyRefresh(params);
  const runtimeController = useRuntimeController(params);

  return {
    refreshLibraries: lifecycle.refreshLibraries,
    refreshFinancialDashboard: economy.refreshFinancialDashboard,
    onCountryChanged: lifecycle.onCountryChanged,
    installCountryPack: lifecycle.installCountryPack,
    uninstallCountryPack: lifecycle.uninstallCountryPack,
    continueLatestGame: lifecycle.continueLatestGame,
    loadGameSave: lifecycle.loadGameSave,
    loadScenarioSave: lifecycle.loadScenarioSave,
    selectCounty: regionScope.selectCounty,
    focusSelectedCounty: regionScope.focusSelectedCounty,
    unlockAndFocusSelectedCounty: regionScope.unlockAndFocusSelectedCounty,
    deleteSave: lifecycle.deleteSave,
    restoreDeletedSave: lifecycle.restoreDeletedSave,
    purgeDeletedSave: lifecycle.purgeDeletedSave,
    createGame: lifecycle.createGame,
    createScenario: lifecycle.createScenario,
    importScenarioFromPicker: exportController.importScenarioFromPicker,
    saveSession: lifecycle.saveSession,
    saveQuit: lifecycle.saveQuit,
    setRunning: runtimeController.setRunning,
    setSpeed: runtimeController.setSpeed,
    runPlanning: planning.runPlanning,
    exportRunCsv: exportController.exportRunCsv,
    exportRunJson: exportController.exportRunJson,
    compareRuns: planning.compareRuns,
    rebuildDemandForUnlocked: planning.rebuildDemandForUnlocked,
    handleMapBootProgress: lifecycle.handleMapBootProgress,
    retryMapLoad: lifecycle.retryMapLoad,
    updateFarePolicy: farePolicyController.updateFarePolicy,
    expediteFleetDelivery: lifecycle.expediteFleetDelivery,
  };
}
