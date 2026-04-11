import type { SaveBrowserSortKey, SaveBrowserViewGroup } from "../types";
import type { SaveBrowserViewModel } from "../app/useSaveBrowserController";

const SCENARIO_SORT_OPTIONS: Array<{ value: SaveBrowserSortKey; label: string }> = [
  { value: "last_played_desc", label: "Most Recent" },
  { value: "last_played_asc", label: "Oldest" },
  { value: "progress_desc", label: "Best Outcome" },
  { value: "name_asc", label: "Name (A-Z)" },
];

const GROUP_OPTIONS: Array<{ value: SaveBrowserViewGroup; label: string }> = [
  { value: "recent", label: "Recent" },
  { value: "all", label: "All" },
];

export default function LoadScenarioScreen(props: {
  view: SaveBrowserViewModel;
  busy: boolean;
  onBack: () => void;
  onQueryChange: (query: string) => void;
  onSortChange: (sortKey: SaveBrowserSortKey) => void;
  onGroupChange: (group: SaveBrowserViewGroup) => void;
  onSelect: (projectId: string | null) => void;
  onOpen: (id: string) => void;
  onImport: () => void;
  onDelete: (id: string, name: string) => void;
  onRestore: (deletedId: string) => void;
  onPurge: (deletedId: string) => void;
}) {
  return (
    <div className="form-screen">
      <header>
        <h2>Load Scenario</h2>
        <p>Scenario library and import</p>
      </header>
      <div className="form-actions">
        <button onClick={props.onBack}>Back to Menu</button>
        <button onClick={props.onImport} disabled={props.busy}>
          Import Scenario
        </button>
      </div>

      <div className="form-actions">
        <input
          type="search"
          placeholder="Search by name, city, or country"
          value={props.view.query}
          onChange={(event) => props.onQueryChange(event.target.value)}
        />
        <select
          value={props.view.group}
          onChange={(event) => props.onGroupChange(event.target.value as SaveBrowserViewGroup)}
        >
          {GROUP_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <select
          value={props.view.sortKey}
          onChange={(event) => props.onSortChange(event.target.value as SaveBrowserSortKey)}
        >
          {SCENARIO_SORT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      <div className="slot-list">
        {props.view.entries.length === 0 && (
          <p>No scenario saves found for the current browser filters.</p>
        )}
        {props.view.entries.map((s) => {
          const selected = props.view.selectedProjectId === s.project_id;
          const served =
            s.health_indicators.share_trips_served === null ||
            s.health_indicators.share_trips_served === undefined
              ? "-"
              : `${(s.health_indicators.share_trips_served * 100).toFixed(2)}%`;
          const denied =
            s.health_indicators.denied_boardings === null ||
            s.health_indicators.denied_boardings === undefined
              ? "-"
              : s.health_indicators.denied_boardings.toFixed(1);
          return (
            <article
              key={s.project_id}
              className={`slot-card${selected ? " is-selected" : ""}`}
              onClick={() => props.onSelect(s.project_id)}
            >
              <div>
                <strong>{s.name}</strong>
                <p>
                  {s.start_city ?? "Unknown city"}, {s.start_country ?? "Unknown country"} | Last played{" "}
                  {new Date(s.last_played_at).toLocaleString()}
                </p>
                <p>Trips served {served} | Denied boardings {denied}</p>
              </div>
              <div className="slot-actions">
                <button
                  onClick={() => {
                    props.onSelect(s.project_id);
                    props.onOpen(s.project_id);
                  }}
                >
                  Open
                </button>
                <button
                  className="danger-button"
                  onClick={() => {
                    props.onSelect(s.project_id);
                    props.onDelete(s.project_id, s.name);
                  }}
                >
                  Delete
                </button>
              </div>
            </article>
          );
        })}
      </div>

      {props.view.deletedEntries.length > 0 && (
        <section className="deleted-section">
          <h3>Recently Deleted</h3>
          <div className="slot-list">
            {props.view.deletedEntries.map((d) => (
              <article key={d.deleted_id} className="slot-card">
                <div>
                  <strong>{d.name}</strong>
                  <p>Deleted {new Date(d.deleted_at).toLocaleString()}</p>
                </div>
                <div className="slot-actions">
                  <button onClick={() => props.onRestore(d.deleted_id)}>Restore</button>
                  <button className="danger-button" onClick={() => props.onPurge(d.deleted_id)}>
                    Purge
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
