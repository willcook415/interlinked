import { useCallback } from "react";

import { setSimulationRunning, setSimulationSpeed } from "../../api/desktopApi";
import { sameClock } from "../runtimeFreshness";
import type { SimulationSpeed } from "../../types";
import type { UseSessionControllerParams } from "./contracts";

type UseRuntimeControllerResult = {
  setRunning: (running: boolean) => Promise<void>;
  setSpeed: (speed: SimulationSpeed) => Promise<void>;
};

export function useRuntimeController(params: UseSessionControllerParams): UseRuntimeControllerResult {
  const setRunning = useCallback(
    async (running: boolean) => {
      if (!params.bundle) return;
      const projectPath = params.bundle.project_path;
      params.runtimeControlQueueRef.current = params.runtimeControlQueueRef.current
        .then(async () => {
          const result = await setSimulationRunning(projectPath, running);
          params.setClock((previous) => (sameClock(previous, result) ? previous : result));
          params.playUiCue("toggle");
        })
        .catch((error) => {
          params.setError(String(error));
          params.playUiCue("error");
        });
      await params.runtimeControlQueueRef.current;
    },
    [params]
  );

  const setSpeed = useCallback(
    async (speed: SimulationSpeed) => {
      if (!params.bundle) return;
      const projectPath = params.bundle.project_path;
      params.runtimeControlQueueRef.current = params.runtimeControlQueueRef.current
        .then(async () => {
          const result = await setSimulationSpeed(projectPath, speed);
          params.setClock((previous) => (sameClock(previous, result) ? previous : result));
        })
        .catch((error) => {
          params.setError(String(error));
          params.playUiCue("error");
        });
      await params.runtimeControlQueueRef.current;
    },
    [params]
  );

  return {
    setRunning,
    setSpeed,
  };
}
