import { useEffect, useState } from "react";
import type { CurrencyCode, LineInspection, ModeBuildPreset } from "../types";
import type { LocalLineDetail } from "../build/helpers";

type DraftLinePreview = {
  lineId: string;
  lineName: string;
  modeLabel: string;
  displayColor: string;
  stationNames: string[];
  stationIds?: string[];
};

type LineStationDecoration = {
  interchange: boolean;
  connectedLines: Array<{ lineId: string; lineName: string; displayColor?: string | null }>;
};

type LineInspectorTab = "overview" | "route" | "fleet" | "timetable" | "performance";

function formatMoney(value: number | null | undefined, currency: CurrencyCode, compact = false): string {
  if (value === null || value === undefined) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: compact ? 1 : 0,
    notation: compact ? "compact" : "standard",
  }).format(value);
}

function formatSeconds(value: number | null | undefined): string {
  if (value === null || value === undefined) return "-";
  if (value >= 3600) return `${(value / 3600).toFixed(1)}h`;
  if (value >= 60) return `${Math.round(value / 60)} min`;
  return `${Math.round(value)}s`;
}

function formatDistance(value: number | null | undefined): string {
  if (value === null || value === undefined) return "-";
  return `${(value / 1000).toFixed(1)} km`;
}

function normalizeHexColor(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const normalized = trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
  if (/^#[0-9a-fA-F]{6}$/.test(normalized)) return normalized.toLowerCase();
  if (/^#[0-9a-fA-F]{3}$/.test(normalized)) {
    return `#${normalized[1]}${normalized[1]}${normalized[2]}${normalized[2]}${normalized[3]}${normalized[3]}`.toLowerCase();
  }
  return null;
}

function minuteToClock(minute: number): string {
  const normalized = ((Math.round(minute) % 1440) + 1440) % 1440;
  const hh = Math.floor(normalized / 60)
    .toString()
    .padStart(2, "0");
  const mm = (normalized % 60).toString().padStart(2, "0");
  return `${hh}:${mm}`;
}

export default function LineInspectorSheet(props: {
  open: boolean;
  inspection: LineInspection | null;
  lineDetail: LocalLineDetail | null;
  draftPreview?: DraftLinePreview | null;
  forceDraftMode?: boolean;
  editable?: boolean;
  stationDecorations?: Record<string, LineStationDecoration>;
  presets: ModeBuildPreset[];
  selectedPresetId: string | null;
  budgetCurrency: CurrencyCode;
  estimatedCapexBase: number | null;
  stationCapexBase: number | null;
  addingStationMode?: boolean;
  onClose: () => void;
  onAddStationToLine: () => void;
  onDelete: () => void;
  onNameChange: (value: string) => void;
  onColorChange: (value: string) => void;
  onPresetChange: (value: string) => void;
  onStationClick?: (stopId: string) => void;
  onOpenRollingStockEditor: () => void;
  onOpenScheduleEditor: () => void;
  onRemoveDraftStation?: (stopId: string) => void;
}) {
  if (!props.open) return null;
  if (!props.lineDetail && !props.draftPreview) return null;

  const live = props.inspection;
  const isDraftOnly = (props.forceDraftMode ?? false) || (!props.lineDetail && Boolean(props.draftPreview));
  const canEdit = (props.editable ?? true) && Boolean(props.lineDetail);
  const lineName = props.lineDetail?.name ?? props.draftPreview?.lineName ?? "New Line";
  const lineNameDisplay = lineName.trim() ? lineName : "Untitled Line";
  const displayColor =
    props.lineDetail?.displayColor ?? live?.display_color ?? props.draftPreview?.displayColor ?? "#1f3e63";
  const mode = props.lineDetail?.mode ?? props.draftPreview?.modeLabel ?? "Draft";
  const modeVariant = props.lineDetail?.modeVariant ?? live?.mode_variant ?? null;
  const selectedPreset = props.presets.find((preset) => preset.id === props.selectedPresetId) ?? null;
  const stationCount = props.lineDetail?.stationIds.length ?? props.draftPreview?.stationNames.length ?? 0;
  const lengthM = props.lineDetail?.lengthM ?? live?.length_m ?? null;
  const capexBase =
    live?.estimated_capex_base ??
    props.estimatedCapexBase ??
    ((props.stationCapexBase ?? 0) * stationCount + ((lengthM ?? 0) / 1000) * 0);
  const operationsNow = live?.operations_now;
  const activeBand = operationsNow?.active_band ?? "off_peak";
  const activeTph = operationsNow?.live_tph ?? props.lineDetail?.effectiveTph ?? live?.effective_tph ?? 0;
  const activeWaitS = operationsNow?.avg_wait_s ?? props.lineDetail?.averageWaitS ?? live?.avg_wait_s ?? null;
  const activeCapacity = operationsNow?.capacity_per_hour ?? props.lineDetail?.lineCapacityPerHour ?? 0;
  const fleetState = live?.fleet_state;
  const unitsOwned = fleetState?.units_owned ?? props.lineDetail?.stockUnitsOwned ?? live?.owned_units ?? 0;
  const unitsAssigned = fleetState?.units_assigned ?? props.lineDetail?.stockUnitsAssigned ?? live?.assigned_units ?? 0;
  const unitsRequired = fleetState?.units_required_now ?? props.lineDetail?.requiredUnits ?? live?.required_units ?? 0;
  const vehicleCapacity = fleetState?.vehicle_capacity_effective ?? live?.vehicle_capacity_effective ?? null;
  const schedule = live?.schedule_state ?? {
    peak_start_minute: props.lineDetail?.scheduleProfile.peak_start_minute ?? 420,
    peak_end_minute: props.lineDetail?.scheduleProfile.peak_end_minute ?? 570,
    overnight_start_minute: props.lineDetail?.scheduleProfile.overnight_start_minute ?? 0,
    overnight_end_minute: props.lineDetail?.scheduleProfile.overnight_end_minute ?? 300,
    tph_peak: props.lineDetail?.scheduleProfile.tph_peak ?? 0,
    tph_off_peak: props.lineDetail?.scheduleProfile.tph_off_peak ?? 0,
    tph_overnight: props.lineDetail?.scheduleProfile.tph_overnight ?? 0,
  };
  const costStory = live?.cost_story;
  const diagramStops = props.lineDetail
      ? props.lineDetail.stations.map((station) => {
        const decoration = props.stationDecorations?.[station.stop_id];
        return {
          key: station.stop_id,
          stopId: station.stop_id,
          name: station.name,
          interchange: decoration?.interchange ?? false,
          connectedLines: decoration?.connectedLines ?? [],
        };
      })
    : (props.draftPreview?.stationNames ?? []).map((stationName, index) => ({
        key: `draft:${index}:${stationName}`,
        stopId: props.draftPreview?.stationIds?.[index] ?? null,
        name: stationName,
        interchange: false,
        connectedLines: [] as Array<{ lineId: string; lineName: string; displayColor?: string | null }>,
      }));
  const [hexInput, setHexInput] = useState(normalizeHexColor(displayColor) ?? "#1f3e63");
  const [activeTab, setActiveTab] = useState<LineInspectorTab>(isDraftOnly ? "route" : "overview");
  const tubeColor = normalizeHexColor(displayColor) ?? "#4f76a3";
  const lineSessionKey = `${props.lineDetail?.lineId ?? props.draftPreview?.lineId ?? "draft"}:${isDraftOnly ? "draft" : "live"}`;

  useEffect(() => {
    setHexInput(normalizeHexColor(displayColor) ?? "#1f3e63");
  }, [displayColor]);

  useEffect(() => {
    setActiveTab(isDraftOnly ? "route" : "overview");
  }, [isDraftOnly, lineSessionKey]);

  const commitHexColor = () => {
    const normalized = normalizeHexColor(hexInput);
    if (normalized) {
      props.onColorChange(normalized);
      return;
    }
    setHexInput(normalizeHexColor(displayColor) ?? "#1f3e63");
  };

  return (
    <aside className="line-inspector-sheet">
      <div className="inspector-head">
        <div>
          <p>{isDraftOnly ? "Line Builder" : "Line Inspector"}</p>
          <h4>{lineNameDisplay}</h4>
        </div>
        <button onClick={props.onClose}>Close</button>
      </div>

      <div className="inspector-tab-row" role="tablist" aria-label="Line inspector sections">
        <button className={activeTab === "overview" ? "active" : ""} onClick={() => setActiveTab("overview")}>
          Overview
        </button>
        <button className={activeTab === "route" ? "active" : ""} onClick={() => setActiveTab("route")}>
          Route
        </button>
        <button className={activeTab === "fleet" ? "active" : ""} onClick={() => setActiveTab("fleet")}>
          Fleet
        </button>
        <button className={activeTab === "timetable" ? "active" : ""} onClick={() => setActiveTab("timetable")}>
          Timetable
        </button>
        <button className={activeTab === "performance" ? "active" : ""} onClick={() => setActiveTab("performance")}>
          Performance
        </button>
      </div>

      {activeTab === "overview" ? (
        <section className="inspector-section">
          {props.lineDetail ? (
            canEdit ? (
              <div className="inspector-grid">
                <label>
                  Line Name
                  <input
                    value={lineName}
                    placeholder="Untitled Line"
                    onChange={(event) => props.onNameChange(event.target.value)}
                  />
                </label>
                <label>
                  Line Color
                  <div className="line-color-field">
                    <input
                      type="color"
                      value={normalizeHexColor(displayColor) ?? "#1f3e63"}
                      onChange={(event) => {
                        const next = normalizeHexColor(event.target.value) ?? "#1f3e63";
                        setHexInput(next);
                        props.onColorChange(next);
                      }}
                    />
                    <input
                      className="line-color-hex"
                      value={hexInput}
                      onChange={(event) => setHexInput(event.target.value)}
                      onBlur={commitHexColor}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") commitHexColor();
                      }}
                    />
                  </div>
                </label>
                <label>
                  Transport Mode
                  <select value={props.selectedPresetId ?? ""} onChange={(event) => props.onPresetChange(event.target.value)}>
                    <option value="" disabled>
                      Select preset
                    </option>
                    {props.presets.map((preset) => (
                      <option key={preset.id} value={preset.id}>
                        {preset.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Current Band
                  <input value={activeBand.replace("_", " ")} readOnly />
                </label>
              </div>
            ) : (
              <div className="inspector-read-grid">
                <div className="inspector-read-field">
                  <small>Line Name</small>
                  <strong>{lineNameDisplay}</strong>
                </div>
                <div className="inspector-read-field">
                  <small>Line Color</small>
                  <strong className="line-color-read">
                    <span className="station-line-swatch" style={{ backgroundColor: tubeColor }} />
                    {tubeColor}
                  </strong>
                </div>
                <div className="inspector-read-field">
                  <small>Transport Mode</small>
                  <strong>{modeVariant ? `${mode} / ${modeVariant}` : mode}</strong>
                </div>
                <div className="inspector-read-field">
                  <small>Current Service Band</small>
                  <strong>{activeBand.replace("_", " ")}</strong>
                </div>
              </div>
            )
          ) : (
            <p className="hint-line">Line start locked. Keep clicking stations to continue drawing this route.</p>
          )}
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Service</small>
              <strong>{activeTph > 0 ? `${activeTph.toFixed(1)} TPH` : "Not Running"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Route</small>
              <strong>
                {stationCount} stops | {formatDistance(lengthM)}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Construction Value</small>
              <strong title={formatMoney(capexBase, props.budgetCurrency, false)}>
                {formatMoney(capexBase, props.budgetCurrency, true)}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Operating / hr</small>
              <strong title={formatMoney(live?.estimated_opex_per_hour_base ?? null, props.budgetCurrency, false)}>
                {formatMoney(live?.estimated_opex_per_hour_base ?? null, props.budgetCurrency, true)}
              </strong>
            </div>
          </div>
          {selectedPreset ? (
            <p className="hint-line">
              {selectedPreset.label} lines need vehicles before they run. Configure fleet and timetable to start service.
            </p>
          ) : null}
        </section>
      ) : null}

      {activeTab === "route" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Route</h5>
            <span>
              {stationCount} stops | {formatDistance(lengthM)}
            </span>
          </div>
          {diagramStops.length === 0 ? (
            <p className="hint-line">Click the map to place the first stop.</p>
          ) : (
            <div className="tube-diagram">
              {diagramStops.map((station, index) => (
                <div key={station.key} className={`tube-stop ${station.interchange ? "is-interchange" : ""}`}>
                  <div className="tube-track-col">
                    <span
                      className={`tube-track-segment ${index === 0 ? "is-hidden" : ""}`}
                      style={{ backgroundColor: tubeColor }}
                    />
                    <span
                      className={`tube-node ${station.interchange ? "is-interchange" : ""}`}
                      style={{ borderColor: tubeColor }}
                    />
                    <span className="tube-track-stub" style={{ backgroundColor: tubeColor }} />
                    <span
                      className={`tube-track-segment ${index === diagramStops.length - 1 ? "is-hidden" : ""}`}
                      style={{ backgroundColor: tubeColor }}
                    />
                  </div>
                  <div className="tube-label">
                    {station.stopId && props.onStationClick ? (
                      <button
                        className="tube-label-button"
                        onClick={() => props.onStationClick?.(station.stopId!)}
                        title="Open station details"
                      >
                        <strong>{station.name}</strong>
                        <p>
                          {index === 0
                            ? "Origin"
                            : index === diagramStops.length - 1
                            ? "Terminus"
                            : `Stop ${index + 1}`}
                        </p>
                      </button>
                    ) : (
                      <>
                        <strong>{station.name}</strong>
                        <p>
                          {index === 0
                            ? "Origin"
                            : index === diagramStops.length - 1
                            ? "Terminus"
                            : `Stop ${index + 1}`}
                        </p>
                      </>
                    )}
                    {isDraftOnly && station.stopId && props.onRemoveDraftStation && diagramStops.length > 1 ? (
                      <button
                        className="tube-remove-stop"
                        onClick={() => props.onRemoveDraftStation?.(station.stopId!)}
                      >
                        Remove stop
                      </button>
                    ) : null}
                    {station.interchange && station.connectedLines.length > 0 ? (
                      <div className="tube-connection-list">
                        {station.connectedLines.map((connected) => (
                          <span key={`${station.key}:${connected.lineId}`} className="tube-connection-chip">
                            <span
                              className="tube-connection-swatch"
                              style={{ backgroundColor: normalizeHexColor(connected.displayColor ?? "") ?? "#5f85b0" }}
                            />
                            {connected.lineName.trim() ? connected.lineName : "Untitled Line"}
                          </span>
                        ))}
                      </div>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>
          )}
          <div className="inspector-actions">
            {canEdit ? (
              <button onClick={props.onAddStationToLine}>
                {props.addingStationMode ? "Adding Stations..." : "Extend Line On Map"}
              </button>
            ) : null}
            {canEdit && !isDraftOnly ? (
              <button className="danger-button" onClick={props.onDelete}>
                Delete Line
              </button>
            ) : null}
          </div>
          <p className="hint-line">Route editing is map-first. Use this tab to guide and review your route structure.</p>
        </section>
      ) : null}

      {activeTab === "fleet" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Fleet</h5>
            <span>{unitsOwned.toLocaleString()} vehicles</span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Vehicles Owned</small>
              <strong>{unitsOwned.toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Assigned</small>
              <strong>{unitsAssigned.toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Needed Now</small>
              <strong>{unitsRequired.toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Seats / Vehicle</small>
              <strong>{vehicleCapacity === null ? "-" : Math.round(vehicleCapacity).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Fleet Value</small>
              <strong title={formatMoney(costStory?.fleet_value_base ?? null, props.budgetCurrency, false)}>
                {formatMoney(costStory?.fleet_value_base ?? null, props.budgetCurrency, true)}
              </strong>
            </div>
          </div>
          {unitsOwned <= 0 ? (
            <p className="hint-line">No active vehicles. Procure rolling stock to unlock service.</p>
          ) : null}
          {canEdit ? (
            <button onClick={props.onOpenRollingStockEditor}>Manage Fleet And Procurement</button>
          ) : (
            <p className="hint-line">Switch to build mode to edit fleet assignments and orders.</p>
          )}
        </section>
      ) : null}

      {activeTab === "timetable" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Timetable</h5>
            <span>
              Peak {schedule.tph_peak.toFixed(1)} | Off-Peak {schedule.tph_off_peak.toFixed(1)} | Night{" "}
              {schedule.tph_overnight.toFixed(1)}
            </span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Peak Window</small>
              <strong>{minuteToClock(schedule.peak_start_minute)} - {minuteToClock(schedule.peak_end_minute)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Night Window</small>
              <strong>
                {minuteToClock(schedule.overnight_start_minute)} - {minuteToClock(schedule.overnight_end_minute)}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Average Wait</small>
              <strong>{formatSeconds(activeWaitS)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Operating / hr</small>
              <strong title={formatMoney(live?.estimated_opex_per_hour_base ?? null, props.budgetCurrency, false)}>
                {formatMoney(live?.estimated_opex_per_hour_base ?? null, props.budgetCurrency, true)}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Staff / hr</small>
              <strong title={formatMoney(costStory?.staff_opex_per_hour_base ?? null, props.budgetCurrency, false)}>
                {formatMoney(costStory?.staff_opex_per_hour_base ?? null, props.budgetCurrency, true)}
              </strong>
            </div>
          </div>
          {unitsOwned <= 0 ? <p className="hint-line">Buy vehicles first, then set frequency targets.</p> : null}
          {canEdit ? (
            <button onClick={props.onOpenScheduleEditor}>Edit Timetable</button>
          ) : (
            <p className="hint-line">Switch to build mode to edit timetable settings.</p>
          )}
        </section>
      ) : null}

      {activeTab === "performance" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Performance</h5>
            <span>{activeBand.replace("_", " ")}</span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Service</small>
              <strong>{activeTph > 0 ? `${activeTph.toFixed(1)} TPH` : "Not Running"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Capacity / Hour</small>
              <strong>{Math.round(activeCapacity).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Average Wait</small>
              <strong>{formatSeconds(activeWaitS)}</strong>
            </div>
            <div className="inspector-stat">
              <small>Boarding Attempts</small>
              <strong>{live ? Math.round(live.boardings_attempted).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Boarded</small>
              <strong>{live ? Math.round(live.boardings_served).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Denied</small>
              <strong>{live ? Math.round(live.denied_boardings).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Alighted</small>
              <strong>{live ? Math.round(live.alightings_served).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Queue End</small>
              <strong>{live ? Math.round(live.queue_end).toLocaleString() : "-"}</strong>
            </div>
          </div>
          {!live ? (
            <p className="hint-line">
              Performance data appears once simulation snapshots are available. Route, fleet, and timetable updates will
              feed this view automatically.
            </p>
          ) : null}
        </section>
      ) : null}
    </aside>
  );
}
