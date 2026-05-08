import { useEffect } from "react";
import type { SaveBrowserSortKey } from "../types";
import type { SaveBrowserViewModel } from "../app/useSaveBrowserController";
import { InterlinkedButton, InterlinkedPageShell } from "./primitives";

const GAME_SORT_OPTIONS: Array<{ value: SaveBrowserSortKey; label: string }> = [
  { value: "last_played_desc", label: "Most Recent" },
  { value: "last_played_asc", label: "Oldest" },
  { value: "progress_desc", label: "Most Progressed" },
  { value: "network_size_desc", label: "Largest Network" },
  { value: "name_asc", label: "Name (A-Z)" },
];

function formatDateLabel(value: string | null | undefined): string {
  const raw = (value ?? "").trim();
  if (!raw) return "-";
  const numeric = Number(raw);
  const parsed = Number.isFinite(numeric)
    ? new Date(numeric >= 1_000_000_000_000 ? numeric : numeric * 1000)
    : new Date(raw);
  if (Number.isNaN(parsed.getTime())) return "-";
  return parsed.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function locationLabel(city: string | null | undefined, country: string | null | undefined): string {
  const parts = [city?.trim(), country?.trim()].filter((value): value is string => Boolean(value));
  return parts.length > 0 ? parts.join(", ") : "Location unavailable";
}

export default function LoadGameScreen(props: {
  busy: boolean;
  view: SaveBrowserViewModel;
  onBack: () => void;
  onQueryChange: (query: string) => void;
  onSortChange: (sortKey: SaveBrowserSortKey) => void;
  onSelect: (projectId: string | null) => void;
  onOpen: (id: string) => void;
  onDelete: (id: string, name: string) => void;
  onRestore: (deletedId: string) => void;
  onPurge: (deletedId: string) => void;
}) {
  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    const previousRootOverflow = root.style.overflow;
    const previousRootOverscroll = root.style.overscrollBehavior;
    const previousBodyOverflow = body.style.overflow;
    const previousBodyOverscroll = body.style.overscrollBehavior;
    root.classList.add("il-load-route");
    body.classList.add("il-load-route");
    root.style.overflow = "hidden";
    root.style.overscrollBehavior = "none";
    body.style.overflow = "hidden";
    body.style.overscrollBehavior = "none";
    return () => {
      root.classList.remove("il-load-route");
      body.classList.remove("il-load-route");
      root.style.overflow = previousRootOverflow;
      root.style.overscrollBehavior = previousRootOverscroll;
      body.style.overflow = previousBodyOverflow;
      body.style.overscrollBehavior = previousBodyOverscroll;
    };
  }, []);

  return (
    <InterlinkedPageShell className="il-load-game" centered>
      <div className="il-load-atmosphere" aria-hidden="true">
        <span className="il-load-trace is-a" />
        <span className="il-load-trace is-b" />
        <span className="il-load-node is-a" />
        <span className="il-load-node is-b" />
      </div>

      <section className="il-load-stage">
        <header className="il-load-topbar">
          <InterlinkedButton size="sm" tone="ghost" className="il-load-back" onClick={props.onBack}>
            Back to Menu
          </InterlinkedButton>
          <div className="il-load-title-wrap">
            <h1 className="il-load-title">LOAD GAME</h1>
            <p className="il-load-subtitle">Select a save project</p>
          </div>
          <span aria-hidden="true" className="il-load-topbar-spacer" />
        </header>

        <section className="il-load-toolbar" aria-label="Save Browser Controls">
          <label className="il-load-control is-search">
            <span>Search</span>
            <input
              type="search"
              placeholder="Name, city, or country"
              value={props.view.query}
              onChange={(event) => props.onQueryChange(event.target.value)}
            />
          </label>
          <label className="il-load-control">
            <span>Sort</span>
            <select
              value={props.view.sortKey}
              onChange={(event) => props.onSortChange(event.target.value as SaveBrowserSortKey)}
            >
              {GAME_SORT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
        </section>

        <section className="il-load-library" aria-label="Game Archive">
          <header className="il-load-library-head">
            <h2>Game Archive</h2>
            <p>{props.view.entries.length.toLocaleString()} save projects</p>
          </header>

          <div className="il-load-library-scroll">
            {props.view.entries.length === 0 ? (
              <p className="il-load-empty">No game saves found for the current browser filters.</p>
            ) : null}

            {props.view.entries.map((s) => {
              const selected = props.view.selectedProjectId === s.project_id;
              const ridershipPph =
                s.health_indicators.ridership === null || s.health_indicators.ridership === undefined
                  ? "-"
                  : Math.round(s.health_indicators.ridership).toLocaleString();
              const networkStops =
                s.network_size === null || s.network_size === undefined
                  ? "-"
                  : Math.round(s.network_size).toLocaleString();

              return (
                <article
                  key={s.project_id}
                  className={`il-load-entry ${selected ? "is-selected" : ""}`}
                  onClick={() => props.onSelect(s.project_id)}
                >
                  <div className="il-load-entry-main">
                    <h3>{s.name}</h3>
                    <p className="il-load-entry-location">{locationLabel(s.start_city, s.start_country)}</p>
                    <p className="il-load-entry-dates">
                      <span>Simulation Date: {formatDateLabel(s.in_game_date)}</span>
                      <span>Last Played: {formatDateLabel(s.last_played_at)}</span>
                    </p>
                    <p className="il-load-entry-stats">
                      <span>Peak PPH {ridershipPph}</span>
                      <span>Stops {networkStops}</span>
                    </p>
                  </div>
                  <div className="il-load-entry-actions">
                    <InterlinkedButton
                      size="sm"
                      tone="secondary"
                      disabled={props.busy}
                      onClick={(event) => {
                        event.stopPropagation();
                        if (props.busy) return;
                        props.onOpen(s.project_id);
                      }}
                    >
                      Open
                    </InterlinkedButton>
                    <InterlinkedButton
                      size="sm"
                      tone="ghost"
                      className="il-load-delete"
                      disabled={props.busy}
                      onClick={(event) => {
                        event.stopPropagation();
                        if (props.busy) return;
                        props.onSelect(s.project_id);
                        props.onDelete(s.project_id, s.name);
                      }}
                    >
                      Delete
                    </InterlinkedButton>
                  </div>
                </article>
              );
            })}

            {props.view.deletedEntries.length > 0 ? (
              <section className="il-load-deleted">
                <h3>Recently Deleted</h3>
                {props.view.deletedEntries.map((d) => (
                  <article key={d.deleted_id} className="il-load-entry is-deleted">
                    <div className="il-load-entry-main">
                      <h3>{d.name}</h3>
                      <p className="il-load-entry-dates">
                        <span>Deleted: {formatDateLabel(d.deleted_at)}</span>
                      </p>
                    </div>
                    <div className="il-load-entry-actions">
                      <InterlinkedButton size="sm" tone="secondary" onClick={() => props.onRestore(d.deleted_id)}>
                        Restore
                      </InterlinkedButton>
                      <InterlinkedButton size="sm" tone="ghost" className="il-load-delete" onClick={() => props.onPurge(d.deleted_id)}>
                        Purge
                      </InterlinkedButton>
                    </div>
                  </article>
                ))}
              </section>
            ) : null}
          </div>
        </section>
      </section>
    </InterlinkedPageShell>
  );
}
