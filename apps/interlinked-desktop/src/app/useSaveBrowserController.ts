import { useCallback, useEffect, useMemo, useState } from "react";

import type {
  DeletedSaveBrowserEntry,
  SaveBrowserClassification,
  SaveBrowserDifficultyFilter,
  SaveBrowserEntry,
  SaveBrowserSortKey,
  SaveBrowserViewGroup,
  SaveLibrarySnapshot,
  SessionKind,
} from "../types";

const EMPTY_LIBRARY: SaveLibrarySnapshot = {
  games: [],
  scenarios: [],
  deleted: [],
};

type SaveBrowserViewState = {
  query: string;
  sortKey: SaveBrowserSortKey;
  group: SaveBrowserViewGroup;
  classificationFilter: SaveBrowserClassification | "all";
  difficultyFilter: SaveBrowserDifficultyFilter;
  selectedProjectId: string | null;
};

export type SaveBrowserViewModel = {
  kind: SessionKind;
  query: string;
  sortKey: SaveBrowserSortKey;
  group: SaveBrowserViewGroup;
  classificationFilter: SaveBrowserClassification | "all";
  difficultyFilter: SaveBrowserDifficultyFilter;
  groupedEntries: Record<SaveBrowserViewGroup, SaveBrowserEntry[]>;
  entries: SaveBrowserEntry[];
  deletedEntries: DeletedSaveBrowserEntry[];
  selectedProjectId: string | null;
  selectedEntry: SaveBrowserEntry | null;
};

export type SaveBrowserControllerPort = {
  library: SaveLibrarySnapshot;
  setLibrary: (next: SaveLibrarySnapshot) => void;
  allEntries: SaveBrowserEntry[];
  recentEntries: SaveBrowserEntry[];
  recentGameEntries: SaveBrowserEntry[];
  continueTarget: SaveBrowserEntry | null;
  canContinue: boolean;
  gameView: SaveBrowserViewModel;
  scenarioView: SaveBrowserViewModel;
  setQuery: (kind: SessionKind, query: string) => void;
  setSortKey: (kind: SessionKind, sortKey: SaveBrowserSortKey) => void;
  setGroup: (kind: SessionKind, group: SaveBrowserViewGroup) => void;
  setClassificationFilter: (
    kind: SessionKind,
    filter: SaveBrowserClassification | "all"
  ) => void;
  setDifficultyFilter: (kind: SessionKind, filter: SaveBrowserDifficultyFilter) => void;
  selectProject: (kind: SessionKind, projectId: string | null) => void;
};

const BASE_VIEW_STATE: Record<SessionKind, SaveBrowserViewState> = {
  game: {
    query: "",
    sortKey: "last_played_desc",
    group: "recent",
    classificationFilter: "all",
    difficultyFilter: "all",
    selectedProjectId: null,
  },
  scenario: {
    query: "",
    sortKey: "last_played_desc",
    group: "recent",
    classificationFilter: "all",
    difficultyFilter: "all",
    selectedProjectId: null,
  },
};

function normalizedText(value: string | null | undefined): string {
  return (value ?? "").trim().toLowerCase();
}

function parseDateLike(value: string | null | undefined): Date | null {
  const raw = (value ?? "").trim();
  if (!raw) return null;

  const numeric = Number(raw);
  if (Number.isFinite(numeric)) {
    const asMs = numeric >= 1_000_000_000_000 ? numeric : numeric * 1000;
    const parsedNumeric = new Date(asMs);
    if (!Number.isNaN(parsedNumeric.getTime())) {
      return parsedNumeric;
    }
  }

  const parsed = new Date(raw);
  if (!Number.isNaN(parsed.getTime())) return parsed;
  return null;
}

function canonicalDateString(value: string | null | undefined): string {
  const parsed = parseDateLike(value);
  if (parsed) return parsed.toISOString();
  return (value ?? "").trim();
}

function compareDateLike(a: string, b: string): number {
  const left = parseDateLike(a)?.getTime() ?? Number.NEGATIVE_INFINITY;
  const right = parseDateLike(b)?.getTime() ?? Number.NEGATIVE_INFINITY;
  if (left !== right) return left - right;
  return a.localeCompare(b);
}

function resolveSimDate(baseIso: string | null | undefined, tickSeconds: number | null | undefined): string {
  const parsedBase = parseDateLike(baseIso);
  if (!parsedBase) return (baseIso ?? "").trim();
  const tick = typeof tickSeconds === "number" && Number.isFinite(tickSeconds)
    ? Math.max(tickSeconds, 0)
    : 0;
  const resolved = new Date(parsedBase.getTime() + tick * 1000);
  if (!Number.isNaN(resolved.getTime())) return resolved.toISOString();
  return parsedBase.toISOString();
}

function normalizeGameEntry(game: SaveLibrarySnapshot["games"][number]): SaveBrowserEntry {
  const peakRidershipPph =
    game.peak_ridership_pph ?? game.progress_metrics?.ridership ?? null;
  const networkSize = Number(game.network_stops || 0);
  return {
    project_id: game.project_id,
    project_path: game.project_path,
    session_kind: "game",
    classification: "sandbox",
    name: game.name,
    last_played_at: canonicalDateString(game.last_opened_at),
    start_country: game.start_country ?? null,
    start_city: game.start_city ?? null,
    in_game_date: canonicalDateString(resolveSimDate(game.sim_datetime_utc, game.sim_tick_seconds)),
    difficulty: null,
    playtime_total_minutes: null,
    network_size: networkSize,
    passenger_activity: peakRidershipPph,
    progress_value: game.progress_metrics?.coverage ?? null,
    health_indicators: {
      coverage: game.progress_metrics?.coverage ?? null,
      ridership: peakRidershipPph,
      share_trips_served: null,
      denied_boardings: null,
    },
  };
}

function normalizeScenarioEntry(
  scenario: SaveLibrarySnapshot["scenarios"][number]
): SaveBrowserEntry {
  return {
    project_id: scenario.project_id,
    project_path: scenario.project_path,
    session_kind: "scenario",
    classification: "scenario",
    name: scenario.name,
    last_played_at: canonicalDateString(scenario.last_opened_at),
    start_country: scenario.start_country ?? null,
    start_city: scenario.start_city ?? null,
    in_game_date: null,
    difficulty: null,
    playtime_total_minutes: null,
    network_size: null,
    passenger_activity: scenario.latest_share_trips_served ?? null,
    progress_value: scenario.latest_share_trips_served ?? null,
    health_indicators: {
      coverage: null,
      ridership: null,
      share_trips_served: scenario.latest_share_trips_served ?? null,
      denied_boardings: scenario.latest_total_boardings_denied ?? null,
    },
  };
}

function normalizeDeletedEntry(
  deleted: SaveLibrarySnapshot["deleted"][number]
): DeletedSaveBrowserEntry {
  return {
    deleted_id: deleted.deleted_id,
    project_id: deleted.project_id,
    session_kind: deleted.session_kind,
    name: deleted.name,
    deleted_at: canonicalDateString(deleted.deleted_at),
  };
}

function sortEntries(entries: SaveBrowserEntry[], sortKey: SaveBrowserSortKey): SaveBrowserEntry[] {
  const sorted = [...entries];
  sorted.sort((left, right) => {
    if (sortKey === "last_played_desc") {
      return compareDateLike(right.last_played_at, left.last_played_at);
    }
    if (sortKey === "last_played_asc") {
      return compareDateLike(left.last_played_at, right.last_played_at);
    }
    if (sortKey === "name_asc") {
      return left.name.localeCompare(right.name);
    }
    if (sortKey === "network_size_desc") {
      const leftSize = left.network_size ?? -1;
      const rightSize = right.network_size ?? -1;
      if (rightSize !== leftSize) return rightSize - leftSize;
      return compareDateLike(right.last_played_at, left.last_played_at);
    }
    const leftProgress = left.progress_value ?? -1;
    const rightProgress = right.progress_value ?? -1;
    if (rightProgress !== leftProgress) return rightProgress - leftProgress;
    return compareDateLike(right.last_played_at, left.last_played_at);
  });
  return sorted;
}

function matchesQuery(entry: SaveBrowserEntry, query: string): boolean {
  const q = normalizedText(query);
  if (!q) return true;
  return (
    normalizedText(entry.name).includes(q) ||
    normalizedText(entry.start_city).includes(q) ||
    normalizedText(entry.start_country).includes(q)
  );
}

function matchesClassification(
  entry: SaveBrowserEntry,
  classification: SaveBrowserClassification | "all"
): boolean {
  if (classification === "all") return true;
  return entry.classification === classification;
}

function matchesDifficulty(entry: SaveBrowserEntry, difficulty: SaveBrowserDifficultyFilter): boolean {
  if (difficulty === "all") return true;
  return entry.difficulty === difficulty;
}

function scopedEntries(
  entries: SaveBrowserEntry[],
  kind: SessionKind,
  state: SaveBrowserViewState
): Record<SaveBrowserViewGroup, SaveBrowserEntry[]> {
  const filtered = entries.filter(
    (entry) =>
      entry.session_kind === kind &&
      matchesQuery(entry, state.query) &&
      matchesClassification(entry, state.classificationFilter) &&
      matchesDifficulty(entry, state.difficultyFilter)
  );
  const sorted = sortEntries(filtered, state.sortKey);
  return {
    recent: sorted.slice(0, 12),
    all: sorted,
  };
}

function initialSelectionId(entries: SaveBrowserEntry[]): string | null {
  return entries[0]?.project_id ?? null;
}

export function useSaveBrowserController(): SaveBrowserControllerPort {
  const [library, setLibraryState] = useState<SaveLibrarySnapshot>(EMPTY_LIBRARY);
  const [views, setViews] = useState<Record<SessionKind, SaveBrowserViewState>>(BASE_VIEW_STATE);

  const allEntries = useMemo(() => {
    const games = library.games.map(normalizeGameEntry);
    const scenarios = library.scenarios.map(normalizeScenarioEntry);
    return [...games, ...scenarios];
  }, [library.games, library.scenarios]);

  const deletedEntries = useMemo(
    () => library.deleted.map(normalizeDeletedEntry),
    [library.deleted]
  );

  const gamesByRecent = useMemo(
    () => sortEntries(allEntries.filter((entry) => entry.session_kind === "game"), "last_played_desc"),
    [allEntries]
  );
  const scenariosByRecent = useMemo(
    () =>
      sortEntries(
        allEntries.filter((entry) => entry.session_kind === "scenario"),
        "last_played_desc"
      ),
    [allEntries]
  );

  useEffect(() => {
    setViews((previous) => {
      const next = { ...previous };
      let changed = false;
      const scopedByKind: Record<SessionKind, SaveBrowserEntry[]> = {
        game: gamesByRecent,
        scenario: scenariosByRecent,
      };
      for (const kind of ["game", "scenario"] as const) {
        const current = previous[kind];
        const selectedId = current.selectedProjectId;
        const scope = scopedByKind[kind];
        const exists = selectedId ? scope.some((entry) => entry.project_id === selectedId) : false;
        if (!exists) {
          const fallback = initialSelectionId(scope);
          if (fallback !== selectedId) {
            next[kind] = {
              ...current,
              selectedProjectId: fallback,
            };
            changed = true;
          }
        }
      }
      return changed ? next : previous;
    });
  }, [gamesByRecent, scenariosByRecent]);

  const setLibrary = useCallback((next: SaveLibrarySnapshot) => {
    setLibraryState({
      games: [...next.games],
      scenarios: [...next.scenarios],
      deleted: [...next.deleted],
    });
  }, []);

  const setQuery = useCallback((kind: SessionKind, query: string) => {
    setViews((previous) => ({
      ...previous,
      [kind]: {
        ...previous[kind],
        query,
      },
    }));
  }, []);

  const setSortKey = useCallback((kind: SessionKind, sortKey: SaveBrowserSortKey) => {
    setViews((previous) => ({
      ...previous,
      [kind]: {
        ...previous[kind],
        sortKey,
      },
    }));
  }, []);

  const setGroup = useCallback((kind: SessionKind, group: SaveBrowserViewGroup) => {
    setViews((previous) => ({
      ...previous,
      [kind]: {
        ...previous[kind],
        group,
      },
    }));
  }, []);

  const setClassificationFilter = useCallback(
    (kind: SessionKind, classificationFilter: SaveBrowserClassification | "all") => {
      setViews((previous) => ({
        ...previous,
        [kind]: {
          ...previous[kind],
          classificationFilter,
        },
      }));
    },
    []
  );

  const setDifficultyFilter = useCallback((kind: SessionKind, difficultyFilter: SaveBrowserDifficultyFilter) => {
    setViews((previous) => ({
      ...previous,
      [kind]: {
        ...previous[kind],
        difficultyFilter,
      },
    }));
  }, []);

  const selectProject = useCallback((kind: SessionKind, projectId: string | null) => {
    setViews((previous) => ({
      ...previous,
      [kind]: {
        ...previous[kind],
        selectedProjectId: projectId,
      },
    }));
  }, []);

  const gameGrouped = useMemo(
    () => scopedEntries(allEntries, "game", views.game),
    [allEntries, views.game]
  );
  const scenarioGrouped = useMemo(
    () => scopedEntries(allEntries, "scenario", views.scenario),
    [allEntries, views.scenario]
  );

  const deletedByKind = useMemo(
    () => ({
      game: deletedEntries.filter((entry) => entry.session_kind === "game"),
      scenario: deletedEntries.filter((entry) => entry.session_kind === "scenario"),
    }),
    [deletedEntries]
  );

  const gameSelected = useMemo(() => {
    const selectedId = views.game.selectedProjectId;
    if (!selectedId) return null;
    const fromScope = gameGrouped.all.find((entry) => entry.project_id === selectedId);
    return fromScope ?? null;
  }, [gameGrouped.all, views.game.selectedProjectId]);

  const scenarioSelected = useMemo(() => {
    const selectedId = views.scenario.selectedProjectId;
    if (!selectedId) return null;
    const fromScope = scenarioGrouped.all.find((entry) => entry.project_id === selectedId);
    return fromScope ?? null;
  }, [scenarioGrouped.all, views.scenario.selectedProjectId]);

  const continueTarget = gamesByRecent[0] ?? null;
  const recentEntries = useMemo(
    () => sortEntries(allEntries, "last_played_desc").slice(0, 12),
    [allEntries]
  );

  return {
    library,
    setLibrary,
    allEntries,
    recentEntries,
    recentGameEntries: gamesByRecent.slice(0, 12),
    continueTarget,
    canContinue: Boolean(continueTarget),
    gameView: {
      kind: "game",
      query: views.game.query,
      sortKey: views.game.sortKey,
      group: views.game.group,
      classificationFilter: views.game.classificationFilter,
      difficultyFilter: views.game.difficultyFilter,
      groupedEntries: gameGrouped,
      entries: gameGrouped[views.game.group],
      deletedEntries: deletedByKind.game,
      selectedProjectId: views.game.selectedProjectId,
      selectedEntry: gameSelected,
    },
    scenarioView: {
      kind: "scenario",
      query: views.scenario.query,
      sortKey: views.scenario.sortKey,
      group: views.scenario.group,
      classificationFilter: views.scenario.classificationFilter,
      difficultyFilter: views.scenario.difficultyFilter,
      groupedEntries: scenarioGrouped,
      entries: scenarioGrouped[views.scenario.group],
      deletedEntries: deletedByKind.scenario,
      selectedProjectId: views.scenario.selectedProjectId,
      selectedEntry: scenarioSelected,
    },
    setQuery,
    setSortKey,
    setGroup,
    setClassificationFilter,
    setDifficultyFilter,
    selectProject,
  };
}
