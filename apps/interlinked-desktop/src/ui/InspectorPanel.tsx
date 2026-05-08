import type { ReactNode } from "react";

export default function InspectorPanel(props: {
  eyebrow?: string;
  title: string;
  status?: string | null;
  variant?: "empty" | "context" | "line" | "station" | "vehicle";
  className?: string;
  onClose?: () => void;
  children: ReactNode;
}) {
  const variant = props.variant ?? "empty";
  const className = `inspector-panel is-${variant}-inspector ${props.className ?? ""}`.trim();

  return (
    <aside className={className}>
      <div className="inspector-panel-head">
        <div className="inspector-panel-heading">
          {props.eyebrow?.trim() ? <p>{props.eyebrow}</p> : null}
          <h4>{props.title}</h4>
          {props.status ? <span className="inspector-panel-state">{props.status}</span> : null}
        </div>
        {props.onClose ? <button onClick={props.onClose}>Clear</button> : null}
      </div>
      <div className="inspector-panel-body">{props.children}</div>
    </aside>
  );
}
