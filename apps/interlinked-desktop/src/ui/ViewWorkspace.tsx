import type { ReactNode } from "react";
import type {
  DemandOverlayPayload,
  DemandOverlayType,
  FarePolicyManifest,
  Mission,
  RegionStatus,
  SessionKind,
} from "../types";
import { CountryInfoContent } from "./CountryInfoDrawer";
import { FarePolicyContent } from "./FarePolicyPanel";
import { MapFiltersContent, type LinkModeFilter } from "./MapFiltersPanel";
import { MissionsContent } from "./MissionsDrawer";
import WorkspacePanel from "./WorkspacePanel";

type ViewWorkspacePanelId = "none" | "filters" | "fares" | "country_info" | "missions";

function normalizePanel(value: string): ViewWorkspacePanelId {
  if (
    value === "filters" ||
    value === "fares" ||
    value === "country_info" ||
    value === "missions"
  ) {
    return value;
  }
  return "none";
}

function ViewWorkspaceSection(props: {
  title: string;
  kicker?: string;
  children: ReactNode;
}) {
  return (
    <section className="workspace-section">
      <div className="workspace-section-head">
        <h4>{props.title}</h4>
        {props.kicker ? <span>{props.kicker}</span> : null}
      </div>
      {props.children}
    </section>
  );
}

export default function ViewWorkspace(props: {
  sessionKind: SessionKind;
  activePanel: string;
  showStations: boolean;
  showLinks: boolean;
  showZoneCentroids: boolean;
  showShapeStops: boolean;
  showDemandOverlay: boolean;
  demandOverlayType: DemandOverlayType;
  demandOverlayLoading: boolean;
  demandOverlayAvailable: boolean;
  demandOverlayStatusMessage: string | null;
  demandOverlayPayload: DemandOverlayPayload | null;
  hasZoneCentroidData: boolean;
  hasShapeNodeData: boolean;
  linkMode: LinkModeFilter;
  farePolicy: FarePolicyManifest | null;
  busy: boolean;
  regions: RegionStatus[];
  selectedRegionId: string | null;
  currentBalanceBase: number | null;
  missions: Mission[];
  onEnterBuildMode: () => void;
  onCloseActivePanel: () => boolean;
  onOpenFilters: () => void;
  onOpenMissions: () => void;
  onOpenCountryInfo: () => void;
  onOpenFares: () => void;
  onSelectRegion: (regionId: string) => void;
  onUnlockRegion: () => void;
  onUpdateFarePolicy: (patch: Partial<FarePolicyManifest>) => void;
  onShowStationsChange: (v: boolean) => void;
  onShowLinksChange: (v: boolean) => void;
  onShowZoneCentroidsChange: (v: boolean) => void;
  onShowShapeStopsChange: (v: boolean) => void;
  onShowDemandOverlayChange: (v: boolean) => void;
  onDemandOverlayTypeChange: (v: DemandOverlayType) => void;
  onLinkModeChange: (v: LinkModeFilter) => void;
}) {
  const activePanel = normalizePanel(props.activePanel);
  const showGameControls = props.sessionKind === "game";
  const activeLabel =
    activePanel === "filters"
      ? "Map Layers"
      : activePanel === "fares"
        ? "Fare Policy"
        : activePanel === "country_info"
          ? "Regions"
          : activePanel === "missions"
            ? "Missions"
            : "Overview";

  const toolButton = (
    id: ViewWorkspacePanelId,
    label: string,
    detail: string,
    onClick: () => void
  ) => (
      <button
        className={`workspace-tool-button ${activePanel === id ? "active" : ""}`}
        onClick={() => {
          if (activePanel === id) {
            props.onCloseActivePanel();
            return;
          }
          onClick();
        }}
      >
      <strong>{label}</strong>
      <span>{detail}</span>
    </button>
  );

  const renderOverview = () => (
    <div className="workspace-overview-stack">
      <ViewWorkspaceSection title="Network View" kicker="Map canvas">
        <div className="workspace-signal-grid">
          <span>
            <small>Lines</small>
            <strong>{props.showLinks ? "Visible" : "Hidden"}</strong>
          </span>
          <span>
            <small>Stations</small>
            <strong>{props.showStations ? "Visible" : "Hidden"}</strong>
          </span>
          <span>
            <small>Mode</small>
            <strong>{props.linkMode === "all" ? "All" : props.linkMode}</strong>
          </span>
          <span>
            <small>Demand</small>
            <strong>{props.showDemandOverlay ? "Overlay" : "Off"}</strong>
          </span>
        </div>
        <p className="hint-line">
          Keep the map readable while switching analysis layers and workspace tools from this column.
        </p>
      </ViewWorkspaceSection>

      <ViewWorkspaceSection title="Workspace Tools" kicker={showGameControls ? "Game controls" : "Scenario controls"}>
        <div className="workspace-tool-list">
          {toolButton("filters", "Map Layers", "Visibility, modes, demand overlays", props.onOpenFilters)}
          {showGameControls
            ? toolButton("fares", "Fare Policy", "Price and transfer settings", props.onOpenFares)
            : null}
          {showGameControls
            ? toolButton("country_info", "Regions", "Unlock scope and county progression", props.onOpenCountryInfo)
            : null}
          {showGameControls
            ? toolButton("missions", "Missions", "Current objectives and blockers", props.onOpenMissions)
            : null}
        </div>
      </ViewWorkspaceSection>

      <ViewWorkspaceSection title="Construction" kicker="Mode switch">
        <button className="workspace-primary-action" onClick={props.onEnterBuildMode}>
          Enter Build Workspace
        </button>
        <p className="hint-line">
          Build mode keeps this shell in place and swaps the left column into construction tools.
        </p>
      </ViewWorkspaceSection>
    </div>
  );

  const renderActiveContent = () => {
    if (activePanel === "filters") {
      return (
        <ViewWorkspaceSection title="Map Layers" kicker="Live view">
          <MapFiltersContent
            showStations={props.showStations}
            showLinks={props.showLinks}
            showZoneCentroids={props.showZoneCentroids}
            showShapeStops={props.showShapeStops}
            showDemandOverlay={props.showDemandOverlay}
            demandOverlayType={props.demandOverlayType}
            demandOverlayLoading={props.demandOverlayLoading}
            demandOverlayAvailable={props.demandOverlayAvailable}
            demandOverlayStatusMessage={props.demandOverlayStatusMessage}
            hasZoneCentroidData={props.hasZoneCentroidData}
            hasShapeNodeData={props.hasShapeNodeData}
            linkMode={props.linkMode}
            onShowStationsChange={props.onShowStationsChange}
            onShowLinksChange={props.onShowLinksChange}
            onShowZoneCentroidsChange={props.onShowZoneCentroidsChange}
            onShowShapeStopsChange={props.onShowShapeStopsChange}
            onShowDemandOverlayChange={props.onShowDemandOverlayChange}
            onDemandOverlayTypeChange={props.onDemandOverlayTypeChange}
            onLinkModeChange={props.onLinkModeChange}
          />
        </ViewWorkspaceSection>
      );
    }

    if (activePanel === "fares" && showGameControls) {
      return (
        <ViewWorkspaceSection title="Fare Policy" kicker="Operations">
          <FarePolicyContent
            busy={props.busy}
            policy={props.farePolicy}
            onChange={props.onUpdateFarePolicy}
          />
        </ViewWorkspaceSection>
      );
    }

    if (activePanel === "country_info" && showGameControls) {
      return (
        <CountryInfoContent
          compact
          busy={props.busy}
          regions={props.regions}
          selectedRegionId={props.selectedRegionId}
          currentBalanceBase={props.currentBalanceBase}
          onSelectRegion={props.onSelectRegion}
          onUnlockRegion={props.onUnlockRegion}
        />
      );
    }

    if (activePanel === "missions" && showGameControls) {
      return (
        <ViewWorkspaceSection title="Missions" kicker={`${props.missions.length} tracked`}>
          <MissionsContent missions={props.missions} />
        </ViewWorkspaceSection>
      );
    }

    return renderOverview();
  };

  return (
    <WorkspacePanel
      mode="view"
      eyebrow="Operations"
      title="View Workspace"
      subtitle="Map layers, policy, regions, and objectives stay in one command surface."
      status={activeLabel}
      onEnterBuildMode={props.onEnterBuildMode}
      onExitBuildMode={() => undefined}
    >
      <div className="workspace-tab-row" role="tablist" aria-label="View workspace tools">
        <button
          className={activePanel === "none" ? "active" : ""}
          onClick={() => {
            props.onCloseActivePanel();
          }}
        >
          Overview
        </button>
        <button className={activePanel === "filters" ? "active" : ""} onClick={props.onOpenFilters}>
          Layers
        </button>
        {showGameControls ? (
          <button className={activePanel === "fares" ? "active" : ""} onClick={props.onOpenFares}>
            Fares
          </button>
        ) : null}
        {showGameControls ? (
          <button
            className={activePanel === "country_info" ? "active" : ""}
            onClick={props.onOpenCountryInfo}
          >
            Regions
          </button>
        ) : null}
        {showGameControls ? (
          <button className={activePanel === "missions" ? "active" : ""} onClick={props.onOpenMissions}>
            Missions
          </button>
        ) : null}
      </div>
      <div className="workspace-content-scroll">{renderActiveContent()}</div>
    </WorkspacePanel>
  );
}
