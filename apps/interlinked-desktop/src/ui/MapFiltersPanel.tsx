export type LinkModeFilter = "all" | "rail" | "metro" | "tram" | "bus" | "ferry";

export default function MapFiltersPanel(props: {
  open: boolean;
  onClose: () => void;
  showStations: boolean;
  showLinks: boolean;
  showZoneCentroids: boolean;
  showShapeStops: boolean;
  hasZoneCentroidData: boolean;
  hasShapeNodeData: boolean;
  linkMode: LinkModeFilter;
  onShowStationsChange: (v: boolean) => void;
  onShowLinksChange: (v: boolean) => void;
  onShowZoneCentroidsChange: (v: boolean) => void;
  onShowShapeStopsChange: (v: boolean) => void;
  onLinkModeChange: (v: LinkModeFilter) => void;
}) {
  if (!props.open) return null;
  return (
    <aside className="filters-panel">
      <div className="filters-head">
        <h4>Map Filters</h4>
        <button onClick={props.onClose}>Close</button>
      </div>
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
      <details className="debug-group">
        <summary>Advanced Debug</summary>
        <label className={!props.hasZoneCentroidData ? "disabled" : ""}>
          <input
            type="checkbox"
            checked={props.showZoneCentroids}
            disabled={!props.hasZoneCentroidData}
            onChange={(e) => props.onShowZoneCentroidsChange(e.target.checked)}
          />
          LSOA centroids {!props.hasZoneCentroidData ? "(No data in scenario)" : ""}
        </label>
        <label className={!props.hasShapeNodeData ? "disabled" : ""}>
          <input
            type="checkbox"
            checked={props.showShapeStops}
            disabled={!props.hasShapeNodeData}
            onChange={(e) => props.onShowShapeStopsChange(e.target.checked)}
          />
          Corridor shape nodes {!props.hasShapeNodeData ? "(No data in scenario)" : ""}
        </label>
      </details>
    </aside>
  );
}
