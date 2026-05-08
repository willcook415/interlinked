import type { ReactNode } from "react";

export default function AppSessionFrame(props: {
  workspaceMode: "view" | "build";
  topBar: ReactNode;
  leftWorkspace: ReactNode;
  mapViewport: ReactNode;
  rightInspector: ReactNode;
  bottomPanel: ReactNode;
}) {
  return (
    <div className={`app-session-frame is-${props.workspaceMode}-workspace`}>
      <div className="app-session-topbar">{props.topBar}</div>
      <div className="app-session-body">
        <section className="app-session-workspace" aria-label="Workspace">
          {props.leftWorkspace}
        </section>
        <section className="app-session-map-region" aria-label="Map viewport">
          {props.mapViewport}
        </section>
        <section className="app-session-inspector" aria-label="Context inspector">
          {props.rightInspector}
        </section>
      </div>
      <section className="app-session-bottom" aria-label="Mode summary and actions">
        {props.bottomPanel}
      </section>
    </div>
  );
}
