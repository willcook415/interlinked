import { useCallback, useEffect, useState } from "react";

import type { AppRoute, SessionKind } from "../../types";
import type {
  MapBootProgressPayload,
  SessionBootState,
  SessionLifecycleCheckpoints,
  SessionLifecycleControllerPort,
  SessionLifecycleError,
  SessionLifecycleErrorSource,
  SessionLifecycleSnapshot,
} from "./contracts";

const BASE_BOOT_STATE: SessionBootState = {
  stage: "idle",
  progress: 0,
  message: "",
  error: null,
};

const BASE_CHECKPOINTS: SessionLifecycleCheckpoints = {
  projectAccepted: false,
  mapContextReady: false,
  runtimeControlReady: false,
  firstFastSnapshotReady: false,
};

const BOOT_STAGE_RANK: Record<SessionBootState["stage"], number> = {
  idle: 0,
  session_open: 1,
  map_runtime_config: 2,
  map_style: 3,
  map_context: 4,
  snapshot: 5,
  ready: 6,
  error: 7,
};

// Cold-start session opens can take significantly longer due to first-time surface
// wire parsing (which is cached on subsequent opens). The timeout must be generous
// enough to avoid false "runtime did not become ready" errors on legitimate first boot.
const RUNTIME_CHECKPOINT_TIMEOUT_MS = 90_000;

function inSessionRoute(route: AppRoute): boolean {
  return route === "session_game" || route === "session_scenario";
}

function clampProgress(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(Math.max(value, 0), 1);
}

function createLifecycleError(
  source: SessionLifecycleErrorSource,
  message: string,
  recoverable: boolean
): SessionLifecycleError {
  return {
    source,
    message: message.trim() || "Session load failed.",
    recoverable,
    occurredAtEpochMs: Date.now(),
  };
}

function isRuntimeCheckpointTimeoutError(error: SessionLifecycleError | null): boolean {
  if (!error) return false;
  if (error.source === "runtime_snapshot") return true;
  if (error.source !== "runtime_control") return false;
  return (
    error.message ===
      "Runtime control and telemetry did not become ready. Retry loading the session." ||
    error.message ===
      "Runtime control did not become ready. Retry loading the session."
  );
}

function shouldClearRuntimeCheckpointTimeoutError(
  snapshot: SessionLifecycleSnapshot,
  nextCheckpoints: SessionLifecycleCheckpoints
): boolean {
  if (snapshot.sessionKind !== "game") return false;
  if (!nextCheckpoints.runtimeControlReady || !nextCheckpoints.firstFastSnapshotReady) {
    return false;
  }
  return isRuntimeCheckpointTimeoutError(snapshot.lastError);
}

function nextBootState(
  previous: SessionBootState,
  next: Partial<SessionBootState> & Pick<SessionBootState, "stage">
): SessionBootState {
  if (next.stage === "error") {
    return {
      stage: "error",
      progress: clampProgress(Math.max(previous.progress, next.progress ?? 0.5)),
      message: next.message?.trim() || previous.message || "Session load failed.",
      error: next.error?.trim() || previous.error || "Session load failed.",
    };
  }
  if (next.stage === "ready") {
    return {
      stage: "ready",
      progress: 1,
      message: next.message?.trim() || "Session ready.",
      error: null,
    };
  }
  if (previous.stage === "ready") return previous;
  const previousRank = BOOT_STAGE_RANK[previous.stage];
  const nextRank = BOOT_STAGE_RANK[next.stage];
  if (nextRank < previousRank) return previous;
  return {
    stage: next.stage,
    progress: clampProgress(Math.max(previous.progress, next.progress ?? previous.progress)),
    message: next.message ?? previous.message,
    error: null,
  };
}

function reconcile(snapshot: SessionLifecycleSnapshot): SessionLifecycleSnapshot {
  if (snapshot.state === "app_home" || snapshot.state === "project_selected") {
    return snapshot;
  }

  if (snapshot.lastError) {
    return {
      ...snapshot,
      state: "session_error",
      bootState: nextBootState(snapshot.bootState, {
        stage: "error",
        progress: 0.5,
        message: snapshot.lastError.message,
        error: snapshot.lastError.message,
      }),
    };
  }

  if (!snapshot.checkpoints.projectAccepted) {
    return snapshot;
  }

  if (!snapshot.checkpoints.mapContextReady) {
    if (snapshot.state === "session_recovering") return snapshot;
    return {
      ...snapshot,
      state: "session_loading_map",
    };
  }

  if (
    snapshot.sessionKind === "game" &&
    (!snapshot.checkpoints.runtimeControlReady || !snapshot.checkpoints.firstFastSnapshotReady)
  ) {
    return {
      ...snapshot,
      state: "session_loading_runtime",
      bootState: nextBootState(snapshot.bootState, {
        stage: "snapshot",
        progress: 0.9,
        message: "Waiting for runtime telemetry...",
      }),
    };
  }

  return {
    ...snapshot,
    state: "session_ready",
    bootState: {
      stage: "ready",
      progress: 1,
      message: "Session ready.",
      error: null,
    },
  };
}

function appHomeSnapshot(): SessionLifecycleSnapshot {
  return {
    state: "app_home",
    sessionKind: null,
    checkpoints: BASE_CHECKPOINTS,
    lastError: null,
    bootState: BASE_BOOT_STATE,
  };
}

export function useSessionLifecycleController(args: {
  route: AppRoute;
}): SessionLifecycleControllerPort {
  const [snapshot, setSnapshot] = useState<SessionLifecycleSnapshot>(() => appHomeSnapshot());

  const beginProjectSelection = useCallback((message?: string) => {
    setSnapshot({
      state: "project_selected",
      sessionKind: null,
      checkpoints: BASE_CHECKPOINTS,
      lastError: null,
      bootState: {
        stage: "idle",
        progress: 0,
        message: message?.trim() || "Selecting project...",
        error: null,
      },
    });
  }, []);

  const acceptOpenedSession = useCallback((sessionKind: SessionKind) => {
    setSnapshot(
      reconcile({
        state: "session_booting",
        sessionKind,
        checkpoints: {
          ...BASE_CHECKPOINTS,
          projectAccepted: true,
        },
        lastError: null,
        bootState: {
          stage: "session_open",
          progress: 0.12,
          message: "Opening session...",
          error: null,
        },
      })
    );
  }, []);

  const markMapRuntimeConfigStarted = useCallback(() => {
    setSnapshot((previous) =>
      reconcile({
        ...previous,
        state: previous.state === "session_recovering" ? "session_recovering" : "session_loading_map",
        bootState: nextBootState(previous.bootState, {
          stage: "map_runtime_config",
          progress: 0.28,
          message: "Loading map runtime config...",
        }),
      })
    );
  }, []);

  const markMapRuntimeConfigReady = useCallback(() => {
    setSnapshot((previous) =>
      reconcile({
        ...previous,
        bootState: nextBootState(previous.bootState, {
          stage: "map_runtime_config",
          progress: 0.46,
          message: "Map runtime config ready.",
        }),
      })
    );
  }, []);

  const reportBlockingError = useCallback(
    (source: SessionLifecycleErrorSource, message: string, recoverable = true) => {
      setSnapshot((previous) =>
        reconcile({
          ...previous,
          lastError: createLifecycleError(source, message, recoverable),
        })
      );
    },
    []
  );

  const markMapRuntimeConfigFailed = useCallback(
    (message: string) => {
      reportBlockingError("map_runtime_config", message, true);
    },
    [reportBlockingError]
  );

  const publishMapBootProgress = useCallback(
    (payload: MapBootProgressPayload) => {
      if (payload.stage === "error") {
        reportBlockingError("map", payload.error ?? payload.message ?? "Map failed to load.", true);
        return;
      }
      setSnapshot((previous) => {
        const next: SessionLifecycleSnapshot = {
          ...previous,
          state: previous.state === "session_recovering" ? "session_recovering" : previous.state,
          checkpoints: { ...previous.checkpoints },
          bootState: nextBootState(previous.bootState, {
            stage: payload.stage === "ready" ? "map_context" : payload.stage,
            progress: payload.progress,
            message: payload.message,
          }),
        };
        if (payload.stage === "map_context" && payload.progress >= 0.84) {
          next.checkpoints.mapContextReady = true;
        }
        if (payload.stage === "ready") {
          next.checkpoints.mapContextReady = true;
        }
        return reconcile(next);
      });
    },
    [reportBlockingError]
  );

  const markRuntimeControlReady = useCallback(() => {
    setSnapshot((previous) => {
      if (previous.checkpoints.runtimeControlReady) return previous;
      const nextCheckpoints: SessionLifecycleCheckpoints = {
        ...previous.checkpoints,
        runtimeControlReady: true,
      };
      const clearRuntimeTimeoutError = shouldClearRuntimeCheckpointTimeoutError(
        previous,
        nextCheckpoints
      );
      return reconcile({
        ...previous,
        checkpoints: nextCheckpoints,
        lastError: clearRuntimeTimeoutError ? null : previous.lastError,
      });
    });
  }, []);

  const markFirstFastSnapshotReady = useCallback(() => {
    setSnapshot((previous) => {
      if (previous.checkpoints.firstFastSnapshotReady) return previous;
      const nextCheckpoints: SessionLifecycleCheckpoints = {
        ...previous.checkpoints,
        firstFastSnapshotReady: true,
      };
      const clearRuntimeTimeoutError = shouldClearRuntimeCheckpointTimeoutError(
        previous,
        nextCheckpoints
      );
      return reconcile({
        ...previous,
        checkpoints: nextCheckpoints,
        lastError: clearRuntimeTimeoutError ? null : previous.lastError,
      });
    });
  }, []);

  useEffect(() => {
    if (snapshot.state !== "session_loading_runtime" || snapshot.sessionKind !== "game" || snapshot.lastError) {
      return;
    }
    const runtimeControlMissing = !snapshot.checkpoints.runtimeControlReady;
    const runtimeSnapshotMissing = !snapshot.checkpoints.firstFastSnapshotReady;
    if (!runtimeControlMissing && !runtimeSnapshotMissing) {
      return;
    }
    const timeout = window.setTimeout(() => {
      setSnapshot((previous) => {
        if (
          previous.state !== "session_loading_runtime" ||
          previous.sessionKind !== "game" ||
          previous.lastError
        ) {
          return previous;
        }
        const missingRuntimeControl = !previous.checkpoints.runtimeControlReady;
        const missingRuntimeSnapshot = !previous.checkpoints.firstFastSnapshotReady;
        if (!missingRuntimeControl && !missingRuntimeSnapshot) {
          return previous;
        }
        const source: SessionLifecycleErrorSource = missingRuntimeControl
          ? "runtime_control"
          : "runtime_snapshot";
        const message =
          missingRuntimeControl && missingRuntimeSnapshot
            ? "Runtime control and telemetry did not become ready. Retry loading the session."
            : missingRuntimeControl
              ? "Runtime control did not become ready. Retry loading the session."
              : "Runtime telemetry did not arrive. Retry loading the session.";
        // Intentional visibility for lifecycle deadlock diagnosis.
        console.warn("[session-lifecycle] runtime checkpoint timeout", {
          checkpoints: previous.checkpoints,
          state: previous.state,
          sessionKind: previous.sessionKind,
        });
        return reconcile({
          ...previous,
          lastError: createLifecycleError(source, message, true),
        });
      });
    }, RUNTIME_CHECKPOINT_TIMEOUT_MS);
    return () => {
      window.clearTimeout(timeout);
    };
  }, [
    snapshot.checkpoints.firstFastSnapshotReady,
    snapshot.checkpoints.runtimeControlReady,
    snapshot.lastError,
    snapshot.sessionKind,
    snapshot.state,
  ]);

  const beginRecovery = useCallback(
    (source: SessionLifecycleErrorSource, message?: string) => {
      setSnapshot((previous) => {
        const next: SessionLifecycleSnapshot = {
          ...previous,
          state: "session_recovering",
          lastError: null,
          checkpoints: {
            ...previous.checkpoints,
            mapContextReady: false,
          },
          bootState: {
            stage: "map_style",
            progress: 0.52,
            message: message?.trim() || "Retrying session load...",
            error: null,
          },
        };
        if (!next.checkpoints.projectAccepted) {
          next.lastError = createLifecycleError(
            source,
            "Cannot recover before a project is selected.",
            false
          );
        }
        return reconcile(next);
      });
    },
    []
  );

  const resetToAppHome = useCallback(() => {
    setSnapshot(appHomeSnapshot());
  }, []);

  useEffect(() => {
    if (inSessionRoute(args.route)) return;
    setSnapshot((previous) => {
      if (previous.state === "project_selected") return previous;
      return appHomeSnapshot();
    });
  }, [args.route]);

  return {
    snapshot,
    beginProjectSelection,
    acceptOpenedSession,
    markMapRuntimeConfigStarted,
    markMapRuntimeConfigReady,
    markMapRuntimeConfigFailed,
    publishMapBootProgress,
    markRuntimeControlReady,
    markFirstFastSnapshotReady,
    reportBlockingError,
    beginRecovery,
    resetToAppHome,
  };
}
