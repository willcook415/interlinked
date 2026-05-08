import type { ReactNode } from "react";

export default function WorkspacePanel(props: {
  mode: "view" | "build";
  eyebrow: string;
  title: string;
  subtitle: string;
  status?: string | null;
  onEnterBuildMode: () => void;
  onExitBuildMode: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  const buildActive = props.mode === "build";

  return (
    <aside className={`workspace-panel is-${props.mode}-workspace`}>
      <div className="workspace-panel-header">
        <div className="workspace-panel-heading">
          <p>{props.eyebrow}</p>
          <h3>{props.title}</h3>
          <span>{props.subtitle}</span>
        </div>
        {props.status ? <span className="workspace-status-pill">{props.status}</span> : null}
      </div>

      <div className="workspace-mode-switch" role="group" aria-label="Workspace mode">
        <button
          className={!buildActive ? "active" : ""}
          disabled={!buildActive}
          onClick={buildActive ? props.onExitBuildMode : undefined}
        >
          View
        </button>
        <button
          className={buildActive ? "active" : ""}
          disabled={buildActive}
          onClick={!buildActive ? props.onEnterBuildMode : undefined}
        >
          Build
        </button>
      </div>

      <div className="workspace-panel-body">{props.children}</div>
      {props.footer ? <div className="workspace-panel-footer">{props.footer}</div> : null}
    </aside>
  );
}
