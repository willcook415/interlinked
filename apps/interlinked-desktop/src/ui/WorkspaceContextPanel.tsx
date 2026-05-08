import type { DemandOverlayPayload, DemandOverlayType, Mission } from "../types";
import { canonicalModeClass } from "../modes";
import InspectorPanel from "./InspectorPanel";
import { MapFiltersContent, type LinkModeFilter } from "./MapFiltersPanel";
import { MissionsContent } from "./MissionsDrawer";
import type { WorkspaceContextId } from "./WorkspaceNav";

type NetworkLineRow = {
  lineId: string;
  name: string;
  mode: string;
  modeVariant?: string | null;
  displayColor?: string | null;
};

export default function WorkspaceContextPanel(props: {
  context: WorkspaceContextId;
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
  missions: Mission[];
  lines: NetworkLineRow[];
  selectedLineId?: string | null;
  onSelectLine: (lineId: string) => void;
  onShowStationsChange: (v: boolean) => void;
  onShowLinksChange: (v: boolean) => void;
  onShowZoneCentroidsChange: (v: boolean) => void;
  onShowShapeStopsChange: (v: boolean) => void;
  onShowDemandOverlayChange: (v: boolean) => void;
  onDemandOverlayTypeChange: (v: DemandOverlayType) => void;
  onLinkModeChange: (v: LinkModeFilter) => void;
}) {
  if (props.context === "missions") {
    return (
      <InspectorPanel
        variant="context"
        className="workspace-context-panel is-missions-context"
        title="Missions"
        status={`${props.missions.length.toLocaleString()} Tracked`}
      >
        <MissionsContent missions={props.missions} />
      </InspectorPanel>
    );
  }

  if (props.context === "network") {
    const metroLines = props.lines.filter(
      (line) => canonicalModeClass(line.mode, line.modeVariant ?? null) === "metro"
    );
    const selectedLine = props.lines.find((line) => line.lineId === props.selectedLineId) ?? null;

    return (
      <InspectorPanel
        variant="context"
        className="workspace-context-panel is-network-context"
        title="Network"
        status={`${metroLines.length.toLocaleString()} Metro Lines`}
      >
        <div className="workspace-context-summary">
          <span>
            <small>Total lines</small>
            <strong>{props.lines.length.toLocaleString()}</strong>
          </span>
          <span>
            <small>Metro</small>
            <strong>{metroLines.length.toLocaleString()}</strong>
          </span>
          <span>
            <small>Selected</small>
            <strong>{selectedLine ? selectedLine.name || "Untitled Line" : "-"}</strong>
          </span>
          <span>
            <small>Context</small>
            <strong>Line list</strong>
          </span>
        </div>
        <div className="network-context-list" aria-label="Metro lines">
          {metroLines.length ? (
            metroLines.map((line) => (
              <button
                key={line.lineId}
                className={`network-context-line ${props.selectedLineId === line.lineId ? "active" : ""}`}
                onClick={() => props.onSelectLine(line.lineId)}
              >
                <span
                  className="network-context-line-chip"
                  style={{ backgroundColor: line.displayColor ?? "#2f7de1" }}
                />
                <strong>{line.name.trim() ? line.name : "Untitled Line"}</strong>
                <small>Metro</small>
              </button>
            ))
          ) : (
            <span className="network-context-empty">No metro lines yet.</span>
          )}
        </div>
      </InspectorPanel>
    );
  }

  const demandStatus = props.showDemandOverlay
    ? props.demandOverlayLoading
      ? "Demand Loading"
      : props.demandOverlayAvailable
        ? "Demand Overlay"
        : "Demand Unavailable"
    : "Standard Display";
  const demandRegionCount = props.demandOverlayPayload?.region_data.length ?? null;

  return (
    <InspectorPanel
      variant="context"
      className="workspace-context-panel is-layers-context"
      title="Layers"
      status={demandStatus}
    >
      <div className="workspace-context-summary">
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
          <small>Demand regions</small>
          <strong>{demandRegionCount !== null ? demandRegionCount.toLocaleString() : "-"}</strong>
        </span>
      </div>
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
    </InspectorPanel>
  );
}
