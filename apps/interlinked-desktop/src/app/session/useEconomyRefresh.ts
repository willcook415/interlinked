import { useCallback, useEffect } from "react";

import {
  getFarePolicy,
  getFinancialDashboard,
  setFarePolicy as setFarePolicyRequest,
} from "../../api/desktopApi";
import type { FarePolicyManifest } from "../../types";
import type { UseSessionControllerParams } from "./contracts";

type UseEconomyRefreshResult = {
  refreshFinancialDashboard: () => Promise<void>;
};

type UseFarePolicyControllerResult = {
  updateFarePolicy: (patch: Partial<FarePolicyManifest>) => void;
};

export function useFarePolicyController(
  params: UseSessionControllerParams
): UseFarePolicyControllerResult {
  const updateFarePolicy = useCallback(
    (patch: Partial<FarePolicyManifest>) => {
      if (!params.bundle || params.sessionKind !== "game") return;
      void setFarePolicyRequest(params.bundle.project_path, patch)
        .then((updated) => {
          params.setFarePolicy(updated);
          params.setBundle((previous) => {
            if (!previous) return previous;
            return {
              ...previous,
              manifest: {
                ...previous.manifest,
                economy: previous.manifest.economy
                  ? {
                      ...previous.manifest.economy,
                      fare_policy: updated,
                    }
                  : previous.manifest.economy,
              },
            };
          });
        })
        .catch((error) => params.setError(String(error)));
    },
    [params]
  );

  useEffect(() => {
    let cancelled = false;
    if (!params.bundle || params.sessionKind !== "game") {
      params.setFarePolicy(null);
      return;
    }
    void getFarePolicy(params.bundle.project_path)
      .then((policy) => {
        if (!cancelled) params.setFarePolicy(policy);
      })
      .catch(() => {
        if (!cancelled) params.setFarePolicy(null);
      });
    return () => {
      cancelled = true;
    };
  }, [params.bundle?.project_path, params.sessionKind, params.setFarePolicy]);

  return {
    updateFarePolicy,
  };
}

export function useEconomyRefresh(
  params: UseSessionControllerParams
): UseEconomyRefreshResult {
  const refreshFinancialDashboard = useCallback(async () => {
    if (!params.bundle || params.sessionKind !== "game") return;
    params.setFinancialBusy(true);
    params.setFinancialError(null);
    try {
      const response = await getFinancialDashboard(params.bundle.project_path, params.financialRequest);
      params.setFinancialData(response);
    } catch (error) {
      params.setFinancialError(params.formatBackendError(error));
    } finally {
      params.setFinancialBusy(false);
    }
  }, [params]);

  useEffect(() => {
    if (!params.showFinancialDashboard || !params.bundle || params.sessionKind !== "game") return;
    let cancelled = false;
    const poll = async () => {
      params.setFinancialBusy(true);
      params.setFinancialError(null);
      try {
        const response = await getFinancialDashboard(
          params.bundle!.project_path,
          params.financialRequest
        );
        if (!cancelled) params.setFinancialData(response);
      } catch (error) {
        if (!cancelled) params.setFinancialError(params.formatBackendError(error));
      } finally {
        if (!cancelled) params.setFinancialBusy(false);
      }
    };
    void poll();
    const timer = window.setInterval(() => {
      void poll();
    }, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [
    params.bundle?.project_path,
    params.financialRequest,
    params.formatBackendError,
    params.sessionKind,
    params.setFinancialBusy,
    params.setFinancialData,
    params.setFinancialError,
    params.showFinancialDashboard,
  ]);

  return {
    refreshFinancialDashboard,
  };
}
