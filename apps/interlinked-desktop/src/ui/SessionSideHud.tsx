import type { SessionKind } from "../types";

export default function SessionSideHud(props: {
  sessionKind: SessionKind;
  buildModeActive: boolean;
  filtersOpen: boolean;
  missionsOpen: boolean;
  countryInfoOpen: boolean;
  faresOpen: boolean;
  onEnterBuildMode: () => void;
  onExitBuildMode: () => void;
  onOpenFilters: () => void;
  onOpenMissions: () => void;
  onOpenCountryInfo: () => void;
  onOpenFares: () => void;
}) {
  const showGameControls = props.sessionKind === "game";

  return (
    <aside className={`side-hud ${props.buildModeActive ? "is-build" : ""}`}>
      <div className="side-hud-section">
        <p>Workspace</p>
        <div className="side-hud-mode">
          <button
            className={!props.buildModeActive ? "active" : ""}
            disabled={!props.buildModeActive}
            onClick={props.buildModeActive ? props.onExitBuildMode : undefined}
          >
            View Mode
          </button>
          <button
            className={props.buildModeActive ? "active" : ""}
            disabled={props.buildModeActive}
            onClick={!props.buildModeActive ? props.onEnterBuildMode : undefined}
          >
            Build Mode
          </button>
        </div>
      </div>

      <div className="side-hud-section">
        <p>Panels</p>
        <div className="side-hud-stack">
          <button
            className={props.filtersOpen ? "active" : ""}
            disabled={props.buildModeActive}
            onClick={props.onOpenFilters}
          >
            Map Filters
          </button>
          {showGameControls ? (
            <button
              className={props.faresOpen ? "active" : ""}
              disabled={props.buildModeActive}
              onClick={props.onOpenFares}
            >
              Fares
            </button>
          ) : null}
          {showGameControls ? (
            <button
              className={props.countryInfoOpen ? "active" : ""}
              disabled={props.buildModeActive}
              onClick={props.onOpenCountryInfo}
            >
              Country Info
            </button>
          ) : null}
          {showGameControls ? (
            <button
              className={props.missionsOpen ? "active" : ""}
              disabled={props.buildModeActive}
              onClick={props.onOpenMissions}
            >
              Missions
            </button>
          ) : null}
        </div>
      </div>
    </aside>
  );
}
