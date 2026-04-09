import { useCallback } from "react";

import {
  exportScenarioReportCsv,
  exportScenarioReportJson,
  importScenario,
  pickExportPath,
  pickScenarioFile,
} from "../../api/desktopApi";
import type { OpenSessionResult } from "../../types";
import type {
  UseSessionControllerParams,
  WithBusy,
} from "./contracts";

type UseExportControllerParams = {
  params: UseSessionControllerParams;
  withBusy: WithBusy;
  applyOpenedSession: (opened: OpenSessionResult) => void;
  refreshLibraries: () => Promise<void>;
};

type UseExportControllerResult = {
  importScenarioFromPicker: () => Promise<void>;
  exportRunCsv: (runId: string) => Promise<void>;
  exportRunJson: (runId: string) => Promise<void>;
};

export function useExportController({
  params,
  withBusy,
  applyOpenedSession,
  refreshLibraries,
}: UseExportControllerParams): UseExportControllerResult {
  const importScenarioFromPicker = useCallback(async () => {
    const picked = await withBusy(async () => pickScenarioFile());
    if (!picked) return;
    const opened = await withBusy(async () => importScenario(picked, null));
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
    }
  }, [applyOpenedSession, refreshLibraries, withBusy]);

  const pickOutputPath = useCallback(
    async (kind: "csv" | "json"): Promise<string | null> => {
      const picked = await withBusy(async () => pickExportPath(kind));
      return picked ?? null;
    },
    [withBusy]
  );

  const exportRunCsv = useCallback(
    async (runId: string) => {
      if (!params.bundle) return;
      const outPath = await pickOutputPath("csv");
      if (!outPath) return;
      const result = await withBusy(async () =>
        exportScenarioReportCsv(params.bundle!.project_path, runId, outPath)
      );
      if (result) params.setSaveStatus(`CSV saved ${result.out_path}`);
    },
    [params.bundle, params.setSaveStatus, pickOutputPath, withBusy]
  );

  const exportRunJson = useCallback(
    async (runId: string) => {
      if (!params.bundle) return;
      const outPath = await pickOutputPath("json");
      if (!outPath) return;
      const result = await withBusy(async () =>
        exportScenarioReportJson(params.bundle!.project_path, runId, outPath)
      );
      if (result) params.setSaveStatus(`JSON saved ${result.out_path}`);
    },
    [params.bundle, params.setSaveStatus, pickOutputPath, withBusy]
  );

  return {
    importScenarioFromPicker,
    exportRunCsv,
    exportRunJson,
  };
}
