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

type UseRuntimePollingParams = {
  bundle: OpenSessionResult | null;
  sessionKind: SessionKind | null;
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
  setError: Dispatch<SetStateAction<string | null>>;
};

function finiteNumber(value: number | null | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function useRuntimePolling({
  bundle,
  sessionKind,
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
  setError,
}: UseRuntimePollingParams): void {
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
    setTrainsAuthoritative,
  ]);

  useEffect(() => {
    if (!bundle || sessionKind !== "game") return;
    let cancelled = false;
    void startRuntimeLoop(bundle.project_path).catch((e) => {
      if (!cancelled) setError(String(e));
    });
    const fastTimer = window.setInterval(() => {
      void getRuntimeFastSnapshot(bundle.project_path)
        .then((res) => {
          if (cancelled || !res) return;
          const snapshot = res as RuntimeFastSnapshot;
          // Fast snapshots are the authoritative owner for clock publication.
          // Strategic snapshots may lag by design and therefore must never drive clock writes.
          const nextFreshness = clockFreshnessFromSnapshot(
            snapshot.clock,
            snapshot.telemetry?.tick_index,
            snapshot.captured_at_epoch_ms
          );
          if (!nextFreshness) return;
          const previousFreshness: ClockFreshness = {
            tickSeconds: latestClockTickRef.current,
            tickIndex: latestSnapshotTickRef.current,
            capturedAtEpochMs: latestSnapshotCapturedRef.current,
          };
          if (!isNonDecreasingClockFreshness(nextFreshness, previousFreshness)) {
            // Guard against stale/late fast snapshots regressing displayed time.
            return;
          }
          latestClockTickRef.current = nextFreshness.tickSeconds;
          latestSnapshotTickRef.current = nextFreshness.tickIndex;
          latestSnapshotCapturedRef.current = nextFreshness.capturedAtEpochMs;
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
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        });
    }, 120);

    const strategicTimer = window.setInterval(() => {
      void getRuntimeStrategicSnapshot(bundle.project_path)
        .then((res) => {
          if (cancelled || !res) return;
          const snapshot = res as RuntimeStrategicSnapshot;
          // Strategic polling updates strategic-owned payloads only (economy/service overlays).
          // Clock ownership intentionally stays with fast snapshots to guarantee monotonic time.
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
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        });
    }, 1400);
    return () => {
      cancelled = true;
      window.clearInterval(fastTimer);
      window.clearInterval(strategicTimer);
      void stopRuntimeLoop(bundle.project_path).catch(() => undefined);
    };
  }, [
    bundle?.project_path,
    latestClockTickRef,
    latestSnapshotCapturedRef,
    latestSnapshotTickRef,
    latestStrategicSnapshotCapturedRef,
    latestStrategicSnapshotTickRef,
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
    setTrainsAuthoritative,
  ]);
}
