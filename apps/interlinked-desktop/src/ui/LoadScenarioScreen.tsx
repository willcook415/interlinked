import type { DeletedSaveMeta, ScenarioSaveMeta } from "../types";

export default function LoadScenarioScreen(props: {
  saves: ScenarioSaveMeta[];
  deleted: DeletedSaveMeta[];
  busy: boolean;
  onBack: () => void;
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
      <div className="slot-list">
        {props.saves.length === 0 && <p>No scenario saves found.</p>}
        {props.saves.map((s) => (
          <article key={s.project_id} className="slot-card">
            <div>
              <strong>{s.name}</strong>
              <p>
                {s.start_city ?? "Unknown city"}, {s.start_country ?? "Unknown country"} | Last opened{" "}
                {s.last_opened_at}
              </p>
              <p>
                Latest run: {s.latest_run_id ?? "-"} {s.latest_run_created_at ? `(${s.latest_run_created_at})` : ""}
              </p>
              <p>
                Served{" "}
                {s.latest_share_trips_served === null || s.latest_share_trips_served === undefined
                  ? "-"
                  : `${(s.latest_share_trips_served * 100).toFixed(2)}%`}{" "}
                | Mean GC{" "}
                {s.latest_mean_generalized_cost_s === null || s.latest_mean_generalized_cost_s === undefined
                  ? "-"
                  : `${s.latest_mean_generalized_cost_s.toFixed(1)}s`}{" "}
                | Denied{" "}
                {s.latest_total_boardings_denied === null || s.latest_total_boardings_denied === undefined
                  ? "-"
                  : s.latest_total_boardings_denied.toFixed(1)}
              </p>
            </div>
            <div className="slot-actions">
              <button onClick={() => props.onOpen(s.project_id)}>Open</button>
              <button className="danger-button" onClick={() => props.onDelete(s.project_id, s.name)}>
                Delete
              </button>
            </div>
          </article>
        ))}
      </div>
      {props.deleted.length > 0 && (
        <section className="deleted-section">
          <h3>Recently Deleted</h3>
          <div className="slot-list">
            {props.deleted.map((d) => (
              <article key={d.deleted_id} className="slot-card">
                <div>
                  <strong>{d.name}</strong>
                  <p>Deleted {d.deleted_at}</p>
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
