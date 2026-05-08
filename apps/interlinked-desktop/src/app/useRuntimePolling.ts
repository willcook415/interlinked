import { useEffect, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import {
  clockFreshnessFromSnapshot,
  isNonDecreasingClockFreshness,
  sameClock,
  sameEconomy,
  sameRuntimeLineOps,
  sameRuntimeStations,
  sameRuntimeTrains,
  sameServiceLoads,
  type ClockFreshness,
} from "./runtimeFreshness";
import type {
  LineOpsRuntimeView,
  OpenSessionResult,
  RuntimeFastSnapshot,
  RuntimePerfTelemetry,
  RuntimeTemporalDiagnostics,
  RuntimeStrategicSnapshot,
  SessionKind,
  SimulationAdvanceEconomy,
  SimulationClock,
  StationRuntimeView,
  TrainRuntimeView,
} from "../types";
import {
  getRuntimeFastSnapshot,
  getRuntimeStrategicSnapshot,
  startRuntimeLoop,
  stopRuntimeLoop,
} from "../api/desktopApi";
import type { SessionLifecycleControllerPort } from "./session/contracts";

type UseRuntimePollingParams = {
  bundle: OpenSessionResult | null;
  sessionKind: SessionKind | null;
  lifecycle: SessionLifecycleControllerPort;
  latestClockTickRef: MutableRefObject<number>;
  latestSnapshotTickRef: MutableRefObject<number>;
  latestSnapshotCapturedRef: MutableRefObject<number>;
  latestStrategicSnapshotTickRef: MutableRefObject<number>;
  latestStrategicSnapshotCapturedRef: MutableRefObject<number>;
  runtimeControlQueueRef: MutableRefObject<Promise<void>>;
  setClock: Dispatch<SetStateAction<SimulationClock | null>>;
  setLiveEconomy: Dispatch<SetStateAction<SimulationAdvanceEconomy | null>>;
  setServiceLoadByServiceId: Dispatch<SetStateAction<Record<string, number>>>;
  setRuntimeTrains: Dispatch<SetStateAction<TrainRuntimeView[]>>;
  setRuntimeStations: Dispatch<SetStateAction<StationRuntimeView[]>>;
  setRuntimeLineOps: Dispatch<SetStateAction<LineOpsRuntimeView[]>>;
  setTrainsAuthoritative: Dispatch<SetStateAction<boolean>>;
  setRuntimeTelemetry: Dispatch<SetStateAction<RuntimePerfTelemetry | null>>;
  setSnapshotLatencyMs: Dispatch<SetStateAction<number | null>>;
  setTemporalDiagnostics: Dispatch<SetStateAction<RuntimeTemporalDiagnostics>>;
  setError: Dispatch<SetStateAction<string | null>>;
};

const FAST_POLL_INTERVAL_MS = 50;
const STRATEGIC_POLL_INTERVAL_MS = 1400;

function finiteNumber(value: number | null | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function useRuntimePolling({
  bundle,
  sessionKind,
  lifecycle,
  latestClockTickRef,
  latestSnapshotTickRef,
  latestSnapshotCapturedRef,
  latestStrategicSnapshotTickRef,
  latestStrategicSnapshotCapturedRef,
  runtimeControlQueueRef,
  setClock,
  setLiveEconomy,
  setServiceLoadByServiceId,
  setRuntimeTrains,
  setRuntimeStations,
  setRuntimeLineOps,
  setTrainsAuthoritative,
  setRuntimeTelemetry,
  setSnapshotLatencyMs,
  setTemporalDiagnostics,
  setError,
}: UseRuntimePollingParams): void {
  const markRuntimeControlReady = lifecycle.markRuntimeControlReady;
  const markFirstFastSnapshotReady = lifecycle.markFirstFastSnapshotReady;
  const reportLifecycleBlockingError = lifecycle.reportBlockingError;

  useEffect(() => {
    if (!bundle || sessionKind !== "game") {
      setLiveEconomy(null);
      setServiceLoadByServiceId({});
      setRuntimeTrains([]);
      setRuntimeStations([]);
      setRuntimeLineOps([]);
      setTrainsAuthoritative(false);
      setRuntimeTelemetry(null);
      setSnapshotLatencyMs(null);
      setTemporalDiagnostics({
        last_fast_snapshot_interval_ms: null,
        stale_fast_snapshots_rejected: 0,
        latest_fast_clock_revision: 0,
        latest_fast_tick_index: 0,
      });
      latestClockTickRef.current = 0;
      latestSnapshotTickRef.current = 0;
      latestSnapshotCapturedRef.current = 0;
      latestStrategicSnapshotTickRef.current = 0;
      latestStrategicSnapshotCapturedRef.current = 0;
      return;
    }
    setTrainsAuthoritative(true);
    setLiveEconomy({
      current_balance_base: bundle.manifest.economy?.current_balance_base ?? 0,
      cumulative_revenue_base: bundle.manifest.economy?.cumulative_revenue_base ?? 0,
      cumulative_opex_base: bundle.manifest.economy?.cumulative_opex_base ?? 0,
      budget_display:
        bundle.manifest.progress_metrics?.budget ??
        bundle.manifest.economy?.current_balance_base ??
        0,
    });
  }, [
    bundle?.project_path,
    bundle?.manifest.economy,
    bundle?.manifest.progress_metrics,
    latestClockTickRef,
    latestSnapshotCapturedRef,
    latestSnapshotTickRef,
    latestStrategicSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
    sessionKind,
    setLiveEconomy,
    setRuntimeLineOps,
    setRuntimeStations,
    setRuntimeTelemetry,
    setRuntimeTrains,
    setServiceLoadByServiceId,
    setSnapshotLatencyMs,
    setTemporalDiagnostics,
    setTrainsAuthoritative,
  ]);

  useEffect(() => {
    latestClockTickRef.current = 0;
    latestSnapshotTickRef.current = 0;
    latestSnapshotCapturedRef.current = 0;
    latestStrategicSnapshotTickRef.current = 0;
    latestStrategicSnapshotCapturedRef.current = 0;
    runtimeControlQueueRef.current = Promise.resolve();
    setRuntimeTrains([]);
    setRuntimeStations([]);
    setRuntimeLineOps([]);
    setTrainsAuthoritative(sessionKind === "game");
    setRuntimeTelemetry(null);
    setSnapshotLatencyMs(null);
    setTemporalDiagnostics({
      last_fast_snapshot_interval_ms: null,
      stale_fast_snapshots_rejected: 0,
      latest_fast_clock_revision: 0,
      latest_fast_tick_index: 0,
    });
  }, [
    bundle?.project_path,
    latestClockTickRef,
    latestSnapshotCapturedRef,
    latestSnapshotTickRef,
    latestStrategicSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
    runtimeControlQueueRef,
    sessionKind,
    setRuntimeLineOps,
    setRuntimeStations,
    setRuntimeTelemetry,
    setRuntimeTrains,
    setSnapshotLatencyMs,
    setTemporalDiagnostics,
    setTrainsAuthoritative,
  ]);

  useEffect(() => {
    if (!bundle || sessionKind !== "game") return;
    let cancelled = false;
    let fastTimer: number | null = null;
    let strategicTimer: number | null = null;
    let firstFastSnapshotReady = false;
    let staleFastSnapshotsRejected = 0;
    let latestFastClockRevision = 0;
    let lastFastSnapshotIntervalMs: number | null = null;
    let lastReceivedLog: { bucket: number; running: boolean; speed: number } | null = null;
    let lastAcceptedLog: { bucket: number; running: boolean; speed: number } | null = null;

    const publishTemporalDiagnostics = (next: {
      intervalMs: number | null;
      staleRejected: number;
      latestClockRevision: number;
      latestTickIndex: number;
    }) => {
      setTemporalDiagnostics({
        last_fast_snapshot_interval_ms: next.intervalMs,
        stale_fast_snapshots_rejected: next.staleRejected,
        latest_fast_clock_revision: next.latestClockRevision,
        latest_fast_tick_index: next.latestTickIndex,
      });
    };

    const scheduleFastPoll = (delayMs: number) => {
      if (cancelled) return;
      fastTimer = window.setTimeout(() => {
        void runFastPoll();
      }, Math.max(delayMs, 0));
    };

    const runFastPoll = async (): Promise<void> => {
      const cycleStartedAtMs = performance.now();
      try {
        const res = await getRuntimeFastSnapshot(bundle.project_path);
        if (cancelled || !res) return;
        const snapshot = res as RuntimeFastSnapshot;
        const nextFreshness = clockFreshnessFromSnapshot(
          snapshot.clock,
          snapshot.telemetry?.tick_index,
          snapshot.clock_revision,
          snapshot.captured_at_epoch_ms
        );
        if (!nextFreshness) return;
        const receivedRunning = Boolean(snapshot.clock?.running);
        const receivedSpeed = Number.isFinite(snapshot.clock?.speed)
          ? (snapshot.clock?.speed ?? 1)
          : 1;
        const receivedBucket = Math.floor(Math.max(nextFreshness.tickSeconds, 0));
        if (
          !lastReceivedLog ||
          lastReceivedLog.bucket !== receivedBucket ||
          lastReceivedLog.running !== receivedRunning ||
          lastReceivedLog.speed !== receivedSpeed
        ) {
          console.info("[time-debug] fast_snapshot_received", {
            tickSeconds: nextFreshness.tickSeconds,
            tickIndex: nextFreshness.tickIndex,
            clockRevision: nextFreshness.clockRevision,
            running: receivedRunning,
            speed: receivedSpeed,
            capturedAtEpochMs: nextFreshness.capturedAtEpochMs,
          });
          lastReceivedLog = {
            bucket: receivedBucket,
            running: receivedRunning,
            speed: receivedSpeed,
          };
        }
        const previousFreshness: ClockFreshness = {
          tickSeconds: latestClockTickRef.current,
          tickIndex: latestSnapshotTickRef.current,
          clockRevision: latestFastClockRevision,
          capturedAtEpochMs: latestSnapshotCapturedRef.current,
        };
        const snapshotIntervalMs =
          previousFreshness.capturedAtEpochMs > 0
            ? nextFreshness.capturedAtEpochMs - previousFreshness.capturedAtEpochMs
            : 0;
        if (snapshotIntervalMs > 0) {
          lastFastSnapshotIntervalMs = snapshotIntervalMs;
        }
        if (snapshotIntervalMs > 450) {
          console.warn("[runtime-transport] fast snapshot interval spike", {
            intervalMs: snapshotIntervalMs,
            previous: previousFreshness,
            incoming: nextFreshness,
          });
        }
        if (!isNonDecreasingClockFreshness(nextFreshness, previousFreshness)) {
          if (previousFreshness.capturedAtEpochMs > 0) {
            console.warn("[runtime-clock] stale fast snapshot rejected", {
              previous: previousFreshness,
              incoming: nextFreshness,
              clockRevision: snapshot.clock_revision,
            });
          }
          staleFastSnapshotsRejected += 1;
          if (
            staleFastSnapshotsRejected <= 3 ||
            staleFastSnapshotsRejected % 20 === 0
          ) {
            console.warn("[time-debug] fast_snapshot_rejected", {
              reason: "stale_freshness",
              staleFastSnapshotsRejected,
              previous: previousFreshness,
              incoming: nextFreshness,
              incomingRunning: receivedRunning,
              incomingSpeed: receivedSpeed,
            });
          }
          publishTemporalDiagnostics({
            intervalMs: lastFastSnapshotIntervalMs,
            staleRejected: staleFastSnapshotsRejected,
            latestClockRevision: latestFastClockRevision,
            latestTickIndex: latestSnapshotTickRef.current,
          });
          return;
        }
        const tickDeltaSeconds = nextFreshness.tickSeconds - previousFreshness.tickSeconds;
        if (previousFreshness.capturedAtEpochMs > 0 && tickDeltaSeconds > 8) {
          console.warn("[runtime-clock] large fast snapshot clock jump", {
            tickDeltaSeconds,
            previous: previousFreshness,
            incoming: nextFreshness,
            executedStepsThisCycle: snapshot.telemetry?.executed_steps_this_cycle ?? null,
            backlogSteps: snapshot.telemetry?.backlog_steps ?? null,
          });
        }

        latestClockTickRef.current = nextFreshness.tickSeconds;
        latestSnapshotTickRef.current = nextFreshness.tickIndex;
        latestSnapshotCapturedRef.current = nextFreshness.capturedAtEpochMs;
        latestFastClockRevision = nextFreshness.clockRevision;

        if (!firstFastSnapshotReady) {
          firstFastSnapshotReady = true;
          markFirstFastSnapshotReady();
        }

        setRuntimeTelemetry(snapshot.telemetry ?? null);
        setSnapshotLatencyMs(
          nextFreshness.capturedAtEpochMs > 0
            ? Math.max(Date.now() - nextFreshness.capturedAtEpochMs, 0)
            : null
        );
        setTrainsAuthoritative(Boolean(snapshot.trains_authoritative));
        const nextRuntimeTrains = Array.isArray(snapshot.trains) ? snapshot.trains : [];
        setRuntimeTrains((prev) =>
          sameRuntimeTrains(prev, nextRuntimeTrains) ? prev : nextRuntimeTrains
        );
        const nextStations = Array.isArray(snapshot.stations) ? snapshot.stations : [];
        const nextLineOps = Array.isArray(snapshot.line_ops) ? snapshot.line_ops : [];
        setRuntimeStations((prev) =>
          sameRuntimeStations(prev, nextStations) ? prev : nextStations
        );
        setRuntimeLineOps((prev) =>
          sameRuntimeLineOps(prev, nextLineOps) ? prev : nextLineOps
        );
        setClock((prev) => (sameClock(prev, snapshot.clock ?? null) ? prev : snapshot.clock ?? null));
        const acceptedBucket = Math.floor(Math.max(nextFreshness.tickSeconds, 0));
        if (
          !lastAcceptedLog ||
          lastAcceptedLog.bucket !== acceptedBucket ||
          lastAcceptedLog.running !== receivedRunning ||
          lastAcceptedLog.speed !== receivedSpeed
        ) {
          console.info("[time-debug] fast_snapshot_accepted", {
            tickSeconds: nextFreshness.tickSeconds,
            tickIndex: nextFreshness.tickIndex,
            clockRevision: nextFreshness.clockRevision,
            running: receivedRunning,
            speed: receivedSpeed,
          });
          lastAcceptedLog = {
            bucket: acceptedBucket,
            running: receivedRunning,
            speed: receivedSpeed,
          };
        }
        publishTemporalDiagnostics({
          intervalMs: lastFastSnapshotIntervalMs,
          staleRejected: staleFastSnapshotsRejected,
          latestClockRevision: latestFastClockRevision,
          latestTickIndex: nextFreshness.tickIndex,
        });
      } catch (error) {
        if (!cancelled) setError(String(error));
      } finally {
        if (!cancelled) {
          const elapsedMs = performance.now() - cycleStartedAtMs;
          scheduleFastPoll(Math.max(FAST_POLL_INTERVAL_MS - elapsedMs, 0));
        }
      }
    };

    const scheduleStrategicPoll = (delayMs: number) => {
      if (cancelled) return;
      strategicTimer = window.setTimeout(() => {
        void runStrategicPoll();
      }, Math.max(delayMs, 0));
    };

    const runStrategicPoll = async (): Promise<void> => {
      const cycleStartedAtMs = performance.now();
      try {
        const res = await getRuntimeStrategicSnapshot(bundle.project_path);
        if (cancelled || !res) return;
        const snapshot = res as RuntimeStrategicSnapshot;
        const strategicTick = finiteNumber(snapshot.telemetry?.tick_index, 0);
        const strategicCapturedAt = finiteNumber(snapshot.captured_at_epoch_ms, 0);
        const strategicIsFresh =
          strategicTick > latestStrategicSnapshotTickRef.current ||
          (strategicTick === latestStrategicSnapshotTickRef.current &&
            strategicCapturedAt > latestStrategicSnapshotCapturedRef.current);
        if (!strategicIsFresh) return;
        latestStrategicSnapshotTickRef.current = strategicTick;
        latestStrategicSnapshotCapturedRef.current = strategicCapturedAt;
        if (snapshot.economy) {
          setLiveEconomy((prev) =>
            sameEconomy(prev, snapshot.economy ?? null) ? prev : snapshot.economy ?? null
          );
        }
        const nextServiceLoads: Record<string, number> = {};
        for (const row of snapshot.frame?.service_loads ?? []) {
          const serviceId = row.service_id?.trim();
          if (!serviceId) continue;
          const ratio = Number.isFinite(row.load_to_capacity)
            ? Math.max(row.load_to_capacity, 0)
            : 0;
          nextServiceLoads[serviceId] = Math.max(nextServiceLoads[serviceId] ?? 0, ratio);
        }
        setServiceLoadByServiceId((prev) =>
          sameServiceLoads(prev, nextServiceLoads) ? prev : nextServiceLoads
        );
      } catch (error) {
        if (!cancelled) setError(String(error));
      } finally {
        if (!cancelled) {
          const elapsedMs = performance.now() - cycleStartedAtMs;
          scheduleStrategicPoll(Math.max(STRATEGIC_POLL_INTERVAL_MS - elapsedMs, 0));
        }
      }
    };

    void startRuntimeLoop(bundle.project_path)
      .then(() => {
        if (cancelled) return;
        markRuntimeControlReady();
      })
      .catch((e) => {
        if (cancelled) return;
        const message = String(e);
        setError(message);
        reportLifecycleBlockingError("runtime_control", message, true);
      });

    scheduleFastPoll(0);
    scheduleStrategicPoll(STRATEGIC_POLL_INTERVAL_MS);

    return () => {
      cancelled = true;
      if (fastTimer !== null) window.clearTimeout(fastTimer);
      if (strategicTimer !== null) window.clearTimeout(strategicTimer);
      void stopRuntimeLoop(bundle.project_path).catch(() => undefined);
    };
  }, [
    bundle?.project_path,
    latestClockTickRef,
    latestSnapshotCapturedRef,
    latestSnapshotTickRef,
    markFirstFastSnapshotReady,
    markRuntimeControlReady,
    latestStrategicSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
    reportLifecycleBlockingError,
    sessionKind,
    setClock,
    setError,
    setLiveEconomy,
    setRuntimeLineOps,
    setRuntimeStations,
    setRuntimeTelemetry,
    setRuntimeTrains,
    setServiceLoadByServiceId,
    setSnapshotLatencyMs,
    setTemporalDiagnostics,
    setTrainsAuthoritative,
  ]);
}
