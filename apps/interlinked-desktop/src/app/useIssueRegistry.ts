import { useCallback, useEffect, useMemo, useState } from "react";

import { stopDisplayName } from "../build/helpers";
import type {
  AlertItem,
  AppRoute,
  LineOpsRuntimeView,
  ScenarioLite,
  StationRuntimeView,
} from "../types";
import type { SessionLifecycleError } from "./session/contracts";

export type IssueSeverity = "critical" | "blocking" | "warning" | "advisory" | "info";
export type IssueState = "detected" | "active" | "acknowledged" | "resolved" | "archived";
export type IssueSource = "runtime" | "build" | "economy" | "demand" | "session" | "map";
export type IssuePersistence = "ephemeral" | "session" | "project";
export type IssueActionIntent = "fix" | "inspect" | "defer";

export type IssueAction = {
  id: string;
  label: string;
  intent: IssueActionIntent;
};

export type IssueTarget = {
  kind: "line" | "stop" | "region" | "session";
  id: string;
};

export type IssueRecord = {
  id: string;
  source: IssueSource;
  severity: IssueSeverity;
  state: IssueState;
  title: string;
  detail: string;
  target?: IssueTarget | null;
  actions: IssueAction[];
  persistence: IssuePersistence;
  detectedAtEpochMs: number;
  updatedAtEpochMs: number;
  resolvedAtEpochMs?: number | null;
};

type IssueDetectionSignal = {
  id: string;
  source: IssueSource;
  severity: IssueSeverity;
  title: string;
  detail: string;
  target?: IssueTarget | null;
  actions?: IssueAction[];
  persistence: IssuePersistence;
};

type LineSummaryLike = {
  lineId: string;
  name: string;
};

type UseIssueRegistryArgs = {
  route: AppRoute;
  activeScenario: ScenarioLite | null;
  lineSummaries: LineSummaryLike[];
  runtimeLineOps: LineOpsRuntimeView[];
  runtimeStations: StationRuntimeView[];
  currentBalanceBase: number | null;
  builderError: string | null;
  demandWarning: string | null;
  runtimeError: string | null;
  lifecycleError: SessionLifecycleError | null;
};

function issueSeverityRank(value: IssueSeverity): number {
  if (value === "critical") return 0;
  if (value === "blocking") return 1;
  if (value === "warning") return 2;
  if (value === "advisory") return 3;
  return 4;
}

function issueStateRank(value: IssueState): number {
  if (value === "active") return 0;
  if (value === "detected") return 1;
  if (value === "acknowledged") return 2;
  if (value === "resolved") return 3;
  return 4;
}

function alertSeverityForIssue(value: IssueSeverity): AlertItem["severity"] {
  if (value === "critical" || value === "blocking") return "critical";
  if (value === "warning") return "warn";
  return "info";
}

function buildAlertItemFromIssue(issue: IssueRecord): AlertItem {
  const inspectAction =
    issue.actions.find((action) => action.intent === "fix") ??
    issue.actions.find((action) => action.intent === "inspect") ??
    issue.actions[0] ??
    null;
  const target = issue.target
    ? {
        kind: issue.target.kind,
        id: issue.target.id,
      }
    : null;
  return {
    id: issue.id,
    title: issue.title,
    detail: issue.detail,
    severity: alertSeverityForIssue(issue.severity),
    action_label: inspectAction?.label ?? null,
    target,
  };
}

export function useIssueRegistry(args: UseIssueRegistryArgs) {
  const [issuesById, setIssuesById] = useState<Record<string, IssueRecord>>({});

  const detectedSignals = useMemo<IssueDetectionSignal[]>(() => {
    const signals: IssueDetectionSignal[] = [];
    const inSession = args.route === "session_game" || args.route === "session_scenario";
    if (!inSession) return signals;

    if (args.lifecycleError) {
      const sourceLabel =
        args.lifecycleError.source === "map" || args.lifecycleError.source === "map_runtime_config"
          ? "map"
          : args.lifecycleError.source === "runtime_control" || args.lifecycleError.source === "runtime_snapshot"
            ? "runtime"
            : "session";
      signals.push({
        id: `session-lifecycle:${args.lifecycleError.source}`,
        source:
          sourceLabel === "map"
            ? "map"
            : sourceLabel === "runtime"
              ? "runtime"
              : "session",
        severity: args.lifecycleError.recoverable ? "blocking" : "critical",
        title: "Session load issue",
        detail: args.lifecycleError.message,
        target: { kind: "session", id: args.lifecycleError.source },
        actions: [
          {
            id: `session-recover:${args.lifecycleError.source}`,
            label: args.lifecycleError.recoverable ? "Retry" : "Inspect",
            intent: args.lifecycleError.recoverable ? "fix" : "inspect",
          },
        ],
        persistence: "session",
      });
    }

    if (args.runtimeError?.trim()) {
      signals.push({
        id: "runtime-error",
        source: "runtime",
        severity: "critical",
        title: "Runtime error",
        detail: args.runtimeError,
        actions: [{ id: "runtime-error-inspect", label: "Inspect", intent: "inspect" }],
        persistence: "session",
      });
    }

    if (args.builderError?.trim()) {
      signals.push({
        id: "build-warning",
        source: "build",
        severity: "warning",
        title: "Build validation warning",
        detail: args.builderError,
        actions: [{ id: "build-warning-inspect", label: "Inspect", intent: "inspect" }],
        persistence: "session",
      });
    }

    if (args.demandWarning?.trim()) {
      signals.push({
        id: "demand-warning",
        source: "demand",
        severity: "warning",
        title: "Demand data coverage warning",
        detail: args.demandWarning,
        actions: [{ id: "demand-warning-inspect", label: "Inspect", intent: "inspect" }],
        persistence: "project",
      });
    }

    if (args.currentBalanceBase !== null && args.currentBalanceBase < 0) {
      signals.push({
        id: "budget-negative",
        source: "economy",
        severity: "critical",
        title: "Budget is negative",
        detail: "Cut operating costs or raise fares to return to a positive balance.",
        actions: [{ id: "budget-negative-fix", label: "Inspect", intent: "fix" }],
        persistence: "session",
      });
    }

    const lineNameById = new Map(
      args.lineSummaries.map((line) => [line.lineId, line.name.trim() ? line.name : "Untitled Line"])
    );
    const stressedLines = [...args.runtimeLineOps]
      .filter((line) => (line.denied_boardings_per_hour ?? 0) >= 30)
      .sort((left, right) => (right.denied_boardings_per_hour ?? 0) - (left.denied_boardings_per_hour ?? 0))
      .slice(0, 3);
    for (const line of stressedLines) {
      const denied = Math.round(line.denied_boardings_per_hour ?? 0);
      const lineName = lineNameById.get(line.line_id) ?? line.line_id;
      signals.push({
        id: `line-denied:${line.line_id}`,
        source: "runtime",
        severity: denied >= 120 ? "critical" : "warning",
        title: `${lineName} is denying boardings`,
        detail: `${denied.toLocaleString()} denied boardings/hr. Increase service or capacity.`,
        target: { kind: "line", id: line.line_id },
        actions: [{ id: `line-denied-open:${line.line_id}`, label: "Open line", intent: "inspect" }],
        persistence: "session",
      });
    }

    if (args.activeScenario) {
      const stopById = new Map(args.activeScenario.world.stops.map((stop) => [stop.id, stop]));
      const hotStations = [...args.runtimeStations]
        .map((station) => {
          const capacity = Math.max(station.capacity_pax ?? 0, 0);
          const ratio = capacity > 0 ? Math.max(station.current_inside_pax ?? 0, 0) / capacity : 0;
          return { station, ratio };
        })
        .filter((entry) => entry.ratio >= 0.9)
        .sort((left, right) => right.ratio - left.ratio)
        .slice(0, 2);
      for (const entry of hotStations) {
        const stop = stopById.get(entry.station.stop_id);
        const stopName = stopDisplayName(stop ?? { id: entry.station.stop_id, x: 0, y: 0 });
        signals.push({
          id: `station-capacity:${entry.station.stop_id}`,
          source: "runtime",
          severity: entry.ratio >= 1 ? "critical" : "warning",
          title: `${stopName} nearing capacity`,
          detail: `${Math.round(entry.ratio * 100)}% full. Consider more service or station expansion.`,
          target: { kind: "stop", id: entry.station.stop_id },
          actions: [
            {
              id: `station-capacity-open:${entry.station.stop_id}`,
              label: "Open station",
              intent: "inspect",
            },
          ],
          persistence: "session",
        });
      }
    }

    return signals;
  }, [
    args.activeScenario,
    args.builderError,
    args.currentBalanceBase,
    args.demandWarning,
    args.lifecycleError,
    args.lineSummaries,
    args.route,
    args.runtimeError,
    args.runtimeLineOps,
    args.runtimeStations,
  ]);

  useEffect(() => {
    const now = Date.now();
    setIssuesById((previous) => {
      const next: Record<string, IssueRecord> = { ...previous };
      const detectedIds = new Set<string>();

      for (const signal of detectedSignals) {
        detectedIds.add(signal.id);
        const existing = next[signal.id];
        if (!existing) {
          next[signal.id] = {
            id: signal.id,
            source: signal.source,
            severity: signal.severity,
            state: "active",
            title: signal.title,
            detail: signal.detail,
            target: signal.target ?? null,
            actions: signal.actions ?? [],
            persistence: signal.persistence,
            detectedAtEpochMs: now,
            updatedAtEpochMs: now,
            resolvedAtEpochMs: null,
          };
          continue;
        }
        const nextState =
          existing.state === "acknowledged"
            ? "acknowledged"
            : existing.state === "archived"
              ? "active"
              : existing.state === "resolved"
                ? "active"
                : "active";
        next[signal.id] = {
          ...existing,
          source: signal.source,
          severity: signal.severity,
          state: nextState,
          title: signal.title,
          detail: signal.detail,
          target: signal.target ?? null,
          actions: signal.actions ?? [],
          persistence: signal.persistence,
          updatedAtEpochMs: now,
          resolvedAtEpochMs: nextState === "active" || nextState === "acknowledged" ? null : existing.resolvedAtEpochMs ?? null,
        };
      }

      for (const issueId of Object.keys(next)) {
        if (detectedIds.has(issueId)) continue;
        const issue = next[issueId];
        if (issue.state === "active" || issue.state === "acknowledged" || issue.state === "detected") {
          next[issueId] = {
            ...issue,
            state: "resolved",
            updatedAtEpochMs: now,
            resolvedAtEpochMs: now,
          };
        }
      }

      return next;
    });
  }, [detectedSignals]);

  const acknowledgeIssue = useCallback((issueId: string) => {
    setIssuesById((previous) => {
      const issue = previous[issueId];
      if (!issue || issue.state === "archived" || issue.state === "resolved") return previous;
      return {
        ...previous,
        [issueId]: {
          ...issue,
          state: "acknowledged",
          updatedAtEpochMs: Date.now(),
        },
      };
    });
  }, []);

  const resolveIssue = useCallback((issueId: string) => {
    setIssuesById((previous) => {
      const issue = previous[issueId];
      if (!issue || issue.state === "resolved" || issue.state === "archived") return previous;
      const now = Date.now();
      return {
        ...previous,
        [issueId]: {
          ...issue,
          state: "resolved",
          updatedAtEpochMs: now,
          resolvedAtEpochMs: now,
        },
      };
    });
  }, []);

  const archiveIssue = useCallback((issueId: string) => {
    setIssuesById((previous) => {
      const issue = previous[issueId];
      if (!issue || issue.state === "archived") return previous;
      return {
        ...previous,
        [issueId]: {
          ...issue,
          state: "archived",
          updatedAtEpochMs: Date.now(),
        },
      };
    });
  }, []);

  const issues = useMemo(() => {
    return Object.values(issuesById).sort((left, right) => {
      const stateDelta = issueStateRank(left.state) - issueStateRank(right.state);
      if (stateDelta !== 0) return stateDelta;
      const severityDelta = issueSeverityRank(left.severity) - issueSeverityRank(right.severity);
      if (severityDelta !== 0) return severityDelta;
      return right.updatedAtEpochMs - left.updatedAtEpochMs;
    });
  }, [issuesById]);

  const activeIssues = useMemo(
    () => issues.filter((issue) => issue.state === "active"),
    [issues]
  );

  const activeBlockingIssues = useMemo(
    () =>
      activeIssues.filter(
        (issue) => issue.severity === "critical" || issue.severity === "blocking"
      ),
    [activeIssues]
  );

  const actionableWarnings = useMemo(
    () =>
      activeIssues.filter(
        (issue) => issue.severity === "warning" || issue.severity === "advisory"
      ),
    [activeIssues]
  );

  const backlogIssues = useMemo(
    () =>
      issues.filter(
        (issue) =>
          issue.state !== "archived" &&
          issue.persistence !== "ephemeral" &&
          (issue.state === "active" || issue.state === "acknowledged" || issue.state === "resolved")
      ),
    [issues]
  );

  const alertItems = useMemo<AlertItem[]>(
    () =>
      activeIssues
        .filter((issue) => issue.severity !== "info")
        .map((issue) => buildAlertItemFromIssue(issue)),
    [activeIssues]
  );

  return {
    issues,
    activeIssues,
    activeBlockingIssues,
    actionableWarnings,
    backlogIssues,
    alertItems,
    acknowledgeIssue,
    resolveIssue,
    archiveIssue,
  };
}
