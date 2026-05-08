import type { Mission } from "../types";

export function MissionsContent(props: {
  missions: Mission[];
}) {
  return (
    <div className="missions-content">
      <ul>
        {props.missions.map((m) => (
          <li key={m.id}>
            <strong>{m.title}</strong>
            <p>{m.description}</p>
            <span className={`status ${m.status}`}>{m.status}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export default function MissionsDrawer(props: {
  open: boolean;
  missions: Mission[];
  onClose: () => void;
}) {
  if (!props.open) return null;
  return (
    <aside className="missions-drawer">
      <div className="filters-head">
        <h4>Missions</h4>
        <button onClick={props.onClose}>Close</button>
      </div>
      <MissionsContent missions={props.missions} />
    </aside>
  );
}
