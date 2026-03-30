import type { DeletedSaveMeta, GameSaveMeta } from "../types";

export default function LoadGameScreen(props: {
  saves: GameSaveMeta[];
  deleted: DeletedSaveMeta[];
  onBack: () => void;
  onOpen: (id: string) => void;
  onDelete: (id: string, name: string) => void;
  onRestore: (deletedId: string) => void;
  onPurge: (deletedId: string) => void;
}) {
  return (
    <div className="form-screen">
      <header>
        <h2>Load Game</h2>
        <p>Select a save slot</p>
      </header>
      <div className="slot-list">
        {props.saves.length === 0 && <p>No game saves found.</p>}
        {props.saves.map((s) => (
          <article key={s.project_id} className="slot-card">
            <div>
              <strong>{s.name}</strong>
              <p>
                {s.start_city ?? "Unknown city"}, {s.start_country ?? "Unknown country"} | In-game{" "}
                {s.sim_datetime_utc}
              </p>
              <p>
                Budget {s.progress_metrics.currency} {Math.round(s.progress_metrics.budget).toLocaleString()} |{" "}
                Coverage {(s.progress_metrics.coverage * 100).toFixed(1)}% | Ridership{" "}
                {Math.round(s.progress_metrics.ridership).toLocaleString()}
              </p>
              <p>
                Countries {s.unlocked_countries} | Stops {s.network_stops.toLocaleString()} | Links{" "}
                {s.network_links.toLocaleString()} | Services {s.network_services.toLocaleString()} |{" "}
                {s.total_link_km.toFixed(1)} km
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
      <div className="form-actions">
        <button onClick={props.onBack}>Back to Menu</button>
      </div>
    </div>
  );
}
