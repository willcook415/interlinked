import type { SessionKind } from "../types";

export type WorkspaceContextId = "layers" | "missions" | "network";

export default function WorkspaceNav(props: {
  workspaceMode: "view" | "build";
  activeContext: WorkspaceContextId;
  operationsOpen: boolean;
  sessionKind: SessionKind;
  missionCount: number;
  onEnterBuildMode: () => void;
  onExitBuildMode: () => void;
  onSelectLayers: () => void;
  onSelectNetwork: () => void;
  onSelectMissions: () => void;
  onOpenOperations: () => void;
}) {
  const buildActive = props.workspaceMode === "build";
  const gameSession = props.sessionKind === "game";

  return (
    <aside className={`workspace-nav is-${props.workspaceMode}-workspace`}>
      <div className="workspace-nav-mode" role="group" aria-label="Workspace mode">
        <button
          className={!buildActive ? "active" : ""}
          onClick={buildActive ? props.onExitBuildMode : undefined}
          aria-pressed={!buildActive}
        >
          View
        </button>
        <button
          className={buildActive ? "active" : ""}
          onClick={!buildActive ? props.onEnterBuildMode : undefined}
          aria-pressed={buildActive}
        >
          Build
        </button>
      </div>

      <nav className="workspace-nav-list" aria-label="Workspace navigation">
        <button
          className={`workspace-nav-item ${props.activeContext === "network" && !props.operationsOpen ? "active" : ""}`}
          onClick={props.onSelectNetwork}
        >
          <strong>Network</strong>
        </button>
        <button
          className={`workspace-nav-item ${props.activeContext === "layers" && !props.operationsOpen ? "active" : ""}`}
          onClick={props.onSelectLayers}
        >
          <strong>Layers</strong>
        </button>
        {gameSession ? (
          <button
            className={`workspace-nav-item ${props.activeContext === "missions" && !props.operationsOpen ? "active" : ""}`}
            onClick={props.onSelectMissions}
          >
            <strong>Missions</strong>
            <span>{props.missionCount.toLocaleString()}</span>
          </button>
        ) : null}
        {gameSession ? (
          <button
            className={`workspace-nav-item ${props.operationsOpen ? "active" : ""}`}
            onClick={props.onOpenOperations}
          >
            <strong>Operations</strong>
          </button>
        ) : null}
      </nav>
    </aside>
  );
}
