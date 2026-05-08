import type { DemandOverlayType } from "../types";

export type LinkModeFilter = "all" | "rail" | "metro" | "tram" | "bus" | "ferry";

export type MapFiltersContentProps = {
  showStations: boolean;
  showLinks: boolean;
  showZoneCentroids: boolean;
  showShapeStops: boolean;
  showDemandOverlay: boolean;
  demandOverlayType: DemandOverlayType;
  demandOverlayLoading: boolean;
  demandOverlayAvailable: boolean;
  demandOverlayStatusMessage: string | null;
  hasZoneCentroidData: boolean;
  hasShapeNodeData: boolean;
  linkMode: LinkModeFilter;
  onShowStationsChange: (v: boolean) => void;
  onShowLinksChange: (v: boolean) => void;
  onShowZoneCentroidsChange: (v: boolean) => void;
  onShowShapeStopsChange: (v: boolean) => void;
  onShowDemandOverlayChange: (v: boolean) => void;
  onDemandOverlayTypeChange: (v: DemandOverlayType) => void;
  onLinkModeChange: (v: LinkModeFilter) => void;
};

export function MapFiltersContent(props: MapFiltersContentProps) {
  return (
    <div className="map-filters-content">
      <label>
        <input
          type="checkbox"
          checked={props.showLinks}
          onChange={(e) => props.onShowLinksChange(e.target.checked)}
        />
        Transport lines
      </label>
      <label>
        Mode
        <select value={props.linkMode} onChange={(e) => props.onLinkModeChange(e.target.value as LinkModeFilter)}>
          <option value="all">Show all</option>
          <option value="rail">Rail</option>
          <option value="metro">Metro</option>
          <option value="tram">Tram</option>
          <option value="bus">Bus</option>
          <option value="ferry">Ferry</option>
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={props.showStations}
          onChange={(e) => props.onShowStationsChange(e.target.checked)}
        />
        Stations
      </label>
      <details className="debug-group" open>
        <summary>Demand Analysis</summary>
        <label>
          <input
            type="checkbox"
            checked={props.showDemandOverlay}
            onChange={(e) => props.onShowDemandOverlayChange(e.target.checked)}
          />
          Demand overlay
        </label>
        <label className={!props.showDemandOverlay ? "disabled" : ""}>
          Overlay
          <select
            value={props.demandOverlayType}
            disabled={!props.showDemandOverlay}
            onChange={(e) => props.onDemandOverlayTypeChange(e.target.value as DemandOverlayType)}
          >
            <option value="residential_allocation">Residential Allocation</option>
            <option value="employment_allocation">Employment Allocation</option>
            <option value="total_allocation">Total Allocation</option>
            <option value="raw_residential_weight">Raw Residential Weight</option>
            <option value="raw_employment_weight">Raw Employment Weight</option>
            <option value="fallback_cells">Fallback Cells</option>
          </select>
        </label>
        {props.showDemandOverlay && props.demandOverlayLoading ? (
          <div style={{ fontSize: 12, color: "rgba(255,255,255,0.75)" }}>Loading demand cells...</div>
        ) : null}
        {props.showDemandOverlay &&
        !props.demandOverlayLoading &&
        !props.demandOverlayAvailable &&
        props.demandOverlayStatusMessage ? (
          <div style={{ fontSize: 12, color: "rgba(255,210,175,0.95)" }}>
            {props.demandOverlayStatusMessage}
          </div>
        ) : null}
      </details>
    </div>
  );
}

export default function MapFiltersPanel(props: MapFiltersContentProps & {
  open: boolean;
  onClose: () => void;
}) {
  if (!props.open) return null;
  return (
    <aside className="filters-panel">
      <div className="filters-head">
        <h4>Map Filters</h4>
        <button onClick={props.onClose}>Close</button>
      </div>
      <MapFiltersContent {...props} />
    </aside>
  );
}
