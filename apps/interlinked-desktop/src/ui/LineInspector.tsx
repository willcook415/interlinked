import { useEffect, useMemo, useState, type CSSProperties } from "react";
import type {
  CurrencyCode,
  LineActivationReason,
  LineInspection,
  ModeBuildPreset,
} from "../types";
import { formatCounterProvenance } from "../app/counterProvenance";
import type { LocalLineDetail } from "../build/helpers";
import type { BuildAction } from "../build/types";
import { buildPerfEvent, buildPerfMeasure } from "../perf/buildPerf";
import InspectorPanel from "./InspectorPanel";

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

function formatServiceBandLabel(value: string | null | undefined): string {
  const normalized = (value ?? "").trim().toLowerCase();
  if (!normalized) return "-";
  if (normalized === "off_peak") return "Off-Peak";
  if (normalized === "peak") return "Peak";
  if (normalized === "overnight") return "Overnight";
  return normalized
    .split("_")
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(" ");
}

function formatActivationReasonLabel(reason: LineActivationReason | null | undefined): string | null {
  switch (reason) {
    case "no_target_tph_in_active_band":
      return "0 TPH in active band";
    case "no_assigned_units":
      return "no units assigned";
    case "no_owned_units":
      return "no owned stock";
    case "fleet_insufficient_for_round_trip":
      return "insufficient fleet for round trip";
    case "invalid_headway_or_disabled":
      return "invalid headway or disabled";
    case "no_required_units":
      return "no required units";
    default:
      return null;
  }
}

export default function LineInspector(props: {
  inspection: LineInspection | null;
  lineDetail: LocalLineDetail | null;
  draftPreview?: DraftLinePreview | null;
  forceDraftMode?: boolean;
  editable?: boolean;
  stationDecorations?: Record<string, LineStationDecoration>;
  presets: ModeBuildPreset[];
  selectedPresetId: string | null;
  budgetCurrency: CurrencyCode;
  draftToolMode?: BuildAction;
  estimatedCapexBase: number | null;
  stationCapexBase: number | null;
  extensionAddedStations?: number;
  extensionAddedLengthM?: number;
  hasPendingBuildChanges?: boolean;
  awaitingExtensionTerminus?: boolean;
  extensionAnchorStopName?: string | null;
  addingStationMode?: boolean;
  canUndoDraftPlacement?: boolean;
  onClose: () => void;
  onAddStationToLine: () => void;
  onFinishDraftRoute?: () => void;
  onUndoDraftPlacement?: () => void;
  onDelete: () => void;
  onNameChange: (value: string) => void;
  onColorChange: (value: string) => void;
  onStationClick?: (stopId: string) => void;
  onOpenRollingStockEditor: () => void;
  onOpenScheduleEditor: () => void;
  onRemoveDraftStation?: (stopId: string) => void;
}) {
  if (!props.lineDetail && !props.draftPreview && !props.forceDraftMode) return null;

  const live = props.inspection;
  const hasDraftContext = Boolean(props.draftPreview) || Boolean(props.forceDraftMode);
  const isDraftOnly = (props.forceDraftMode ?? false) || (!props.lineDetail && hasDraftContext);
  const canEdit = (props.editable ?? true) && Boolean(props.lineDetail);
  const lineName = props.lineDetail?.name ?? props.draftPreview?.lineName ?? "New Metro Line";
  const lineNameDisplay = lineName.trim() ? lineName : "Untitled Line";
  const inspectorStateLabel = isDraftOnly
    ? "New Metro Line"
    : canEdit
      ? "Editing Metro Line"
      : "Viewing Metro Line";
  const displayColor =
    props.lineDetail?.displayColor ?? live?.display_color ?? props.draftPreview?.displayColor ?? "#1f3e63";
  const selectedPreset = props.presets.find((preset) => preset.id === props.selectedPresetId) ?? null;
  const stationCount = props.lineDetail?.stationIds.length ?? props.draftPreview?.stationNames.length ?? 0;
  const lengthM = props.lineDetail?.lengthM ?? live?.length_m ?? null;
  const capexBase =
    live?.estimated_capex_base ??
    props.estimatedCapexBase ??
    ((props.stationCapexBase ?? 0) * stationCount + ((lengthM ?? 0) / 1000) * 0);
  const activation = live?.activation;
  const operationsNow = live?.operations_now;
  const fleetState = live?.fleet_state;
  const activeBand = activation?.active_band ?? operationsNow?.active_band ?? "off_peak";
  const activeBandLabel = formatServiceBandLabel(activeBand);
  const activeTph =
    activation?.effective_tph ??
    operationsNow?.live_tph ??
    props.lineDetail?.effectiveTph ??
    live?.effective_tph ??
    0;
  const targetTphNow = activation?.target_tph ?? live?.target_tph ?? 0;
  const activationReason = activation?.reason ?? null;
  const activationOwnedUnits = activation?.units_owned ?? (fleetState?.units_owned ?? props.lineDetail?.stockUnitsOwned ?? live?.owned_units ?? 0);
  const activationAssignedUnits =
    activation?.units_assigned ??
    (fleetState?.units_assigned ?? props.lineDetail?.stockUnitsAssigned ?? live?.assigned_units ?? 0);
  const activationRequiredUnits =
    activation?.required_units ??
    (fleetState?.units_required_now ?? props.lineDetail?.requiredUnits ?? live?.required_units ?? 0);
  const activationEnabled = activation?.enabled ?? activeTph > 0;
  const activationReasonLabel = formatActivationReasonLabel(activationReason);
  const serviceStatusText = activationEnabled
    ? `${activeTph.toFixed(1)} TPH`
    : `Not Running${activationReasonLabel ? ` - ${activationReasonLabel}` : ""}`;
  const activeWaitS = operationsNow?.avg_wait_s ?? props.lineDetail?.averageWaitS ?? live?.avg_wait_s ?? null;
  const activeCapacity = operationsNow?.capacity_per_hour ?? props.lineDetail?.lineCapacityPerHour ?? 0;
  const unitsOwned = fleetState?.units_owned ?? props.lineDetail?.stockUnitsOwned ?? live?.owned_units ?? 0;
  const unitsAssigned = fleetState?.units_assigned ?? props.lineDetail?.stockUnitsAssigned ?? live?.assigned_units ?? 0;
  const unitsRequired = fleetState?.units_required_now ?? props.lineDetail?.requiredUnits ?? live?.required_units ?? 0;
  const fleetReadyForTimetable = unitsOwned > 0 || unitsAssigned > 0;
  const performanceReady = !isDraftOnly && !props.hasPendingBuildChanges;
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
  const passengerProvenanceLabel = formatCounterProvenance(
    live?.passenger_counter_provenance ?? "strategic_estimate"
  );
  const addedTrackKm = Math.max(props.extensionAddedLengthM ?? 0, 0) / 1000;
  const awaitingTerminusSelection =
    Boolean(props.awaitingExtensionTerminus) &&
    (props.draftToolMode === "add_station_to_line" || props.addingStationMode);
  const draftModeStateLabel =
    props.draftToolMode === "start_line"
      ? "Editing on Map"
      : awaitingTerminusSelection
        ? "Select Terminus on Map"
        : props.draftToolMode === "add_station_to_line"
          ? props.extensionAnchorStopName
            ? `Extending from ${props.extensionAnchorStopName}`
            : "Editing on Map"
          : "Editing on Map";
  const draftStatusText =
    props.draftToolMode === "start_line"
      ? stationCount > 0
        ? "Route draft active. Keep plotting stations on the map."
        : "Click the map to place the first station."
      : awaitingTerminusSelection
        ? "Select one terminus station on this line to begin extension."
        : props.draftToolMode === "add_station_to_line"
          ? "Extension active. Place the next station on the map."
          : "Select a line and continue drafting on the map.";
  const diagramStops = useMemo(
    () =>
      buildPerfMeasure(
        "build.ui.derive.line_inspector.diagram_stops",
        () =>
          props.lineDetail
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
                connectedLines: [] as Array<{
                  lineId: string;
                  lineName: string;
                  displayColor?: string | null;
                }>,
              })),
        {
          stationCount:
            props.lineDetail?.stations.length ?? props.draftPreview?.stationNames.length ?? 0,
          draftOnly: isDraftOnly,
        },
        { minDurationMs: 1, throttleMs: 150 }
      ),
    [props.draftPreview?.stationIds, props.draftPreview?.stationNames, props.lineDetail, props.stationDecorations]
  );
  const [hexInput, setHexInput] = useState(normalizeHexColor(displayColor) ?? "#1f3e63");
  const [activeTab, setActiveTab] = useState<LineInspectorTab>(isDraftOnly ? "route" : "overview");
  const tubeColor = normalizeHexColor(displayColor) ?? "#4f76a3";
  const lineSessionKey = `${props.lineDetail?.lineId ?? props.draftPreview?.lineId ?? "draft"}:${isDraftOnly ? "draft" : "live"}`;
  const tabSpec = useMemo(
    () =>
      buildPerfMeasure(
        "build.ui.derive.line_inspector.tab_spec",
        () => [
          { id: "overview" as const, label: "Overview", disabled: isDraftOnly },
          { id: "route" as const, label: "Route", disabled: false },
          { id: "fleet" as const, label: "Fleet", disabled: isDraftOnly },
          {
            id: "timetable" as const,
            label: "Timetable",
            disabled: isDraftOnly || !fleetReadyForTimetable,
          },
          { id: "performance" as const, label: "Performance", disabled: !performanceReady },
        ],
        { isDraftOnly, fleetReadyForTimetable, performanceReady },
        { minDurationMs: 1, throttleMs: 250 }
      ),
    [fleetReadyForTimetable, isDraftOnly, performanceReady]
  );

  useEffect(() => {
    setHexInput(normalizeHexColor(displayColor) ?? "#1f3e63");
  }, [displayColor]);

  useEffect(() => {
    setActiveTab(isDraftOnly ? "route" : "overview");
  }, [isDraftOnly, lineSessionKey]);

  useEffect(() => {
    buildPerfEvent("build.ui.line_inspector.open", {
      lineId: props.lineDetail?.lineId ?? props.draftPreview?.lineId ?? null,
      draftOnly: isDraftOnly,
      initialTab: isDraftOnly ? "route" : "overview",
    });
    return () => {
      buildPerfEvent("build.ui.line_inspector.close", {
        lineId: props.lineDetail?.lineId ?? props.draftPreview?.lineId ?? null,
      });
    };
  }, [isDraftOnly, props.draftPreview?.lineId, props.lineDetail?.lineId]);

  useEffect(() => {
    buildPerfEvent("build.ui.line_inspector.tab_selected", {
      tab: activeTab,
      lineId: props.lineDetail?.lineId ?? props.draftPreview?.lineId ?? null,
      draftOnly: isDraftOnly,
    });
  }, [activeTab, isDraftOnly, props.draftPreview?.lineId, props.lineDetail?.lineId]);

  const commitHexColor = () => {
    const normalized = normalizeHexColor(hexInput);
    if (normalized) {
      props.onColorChange(normalized);
      return;
    }
    setHexInput(normalizeHexColor(displayColor) ?? "#1f3e63");
  };

  return (
    <InspectorPanel
      variant="line"
      eyebrow={isDraftOnly || canEdit ? "Line Builder" : "Line Inspector"}
      title={lineNameDisplay}
      status={inspectorStateLabel}
      className={isDraftOnly ? "is-draft-mode" : ""}
      onClose={props.onClose}
    >
      <div className="inspector-tab-row" role="tablist" aria-label="Line inspector sections">
        {tabSpec.map((tab) => (
          <button
            key={tab.id}
            className={`${activeTab === tab.id ? "active" : ""} ${tab.disabled ? "is-disabled" : ""}`.trim()}
            onClick={() => {
              if (tab.disabled) return;
              setActiveTab(tab.id);
            }}
            disabled={tab.disabled}
          >
            {tab.label}
          </button>
        ))}
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
                <div className="inspector-read-field static-meta-field">
                  <small>Transport Mode</small>
                  <strong>Metro</strong>
                </div>
                <div className="inspector-read-field static-meta-field">
                  <small>Current Band</small>
                  <strong>{activeBandLabel}</strong>
                </div>
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
                  <strong>Metro</strong>
                </div>
                <div className="inspector-read-field static-meta-field">
                  <small>Current Service Band</small>
                  <strong>{activeBandLabel}</strong>
                </div>
              </div>
            )
          ) : (
            <p className="hint-line">Line start locked. Keep clicking stations to continue drawing this route.</p>
          )}
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Service</small>
              <strong>{serviceStatusText}</strong>
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
          <p className="hint-line">
            Activation: {activeBandLabel} | Target {targetTphNow.toFixed(1)} TPH | Fleet {activationAssignedUnits}/
            {activationOwnedUnits} assigned/owned | Required {activationRequiredUnits} | Effective {activeTph.toFixed(1)} TPH
          </p>
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
            <h5>Route Schematic</h5>
            <span>
              {stationCount} stops | {formatDistance(lengthM)}
            </span>
          </div>
          {!isDraftOnly ? (
            <p className="inspector-route-intro">
              Continue plotting on the map. This route list mirrors stop order for quick review and edits.
            </p>
          ) : null}
          {isDraftOnly ? (
            <div className="draft-route-control-card">
              <div className="draft-route-status-row">
                <strong>Draft Status</strong>
                <span>{props.draftToolMode === "add_station_to_line" ? "Extension" : "New Route"}</span>
              </div>
              <p>{draftStatusText}</p>
              <div className="draft-route-metrics">
                <span>
                  <small>Stations</small>
                  <strong>{stationCount}</strong>
                </span>
                <span>
                  <small>Added Track</small>
                  <strong>{addedTrackKm.toFixed(2)} km</strong>
                </span>
              </div>
              <div className="draft-route-actions">
                <div className="draft-route-mode-indicator">{draftModeStateLabel}</div>
                <button
                  onClick={() => {
                    buildPerfEvent("build.ui.finish_route_click", { stationCount });
                    props.onFinishDraftRoute?.();
                  }}
                  disabled={stationCount < 2}
                >
                  Finish Route
                </button>
                <button
                  onClick={() => {
                    buildPerfEvent("build.ui.undo_last_click", { stationCount });
                    props.onUndoDraftPlacement?.();
                  }}
                  disabled={!props.canUndoDraftPlacement}
                >
                  Undo Last
                </button>
              </div>
            </div>
          ) : null}
          {diagramStops.length === 0 ? (
            <p className="hint-line">Click the map to place the first stop.</p>
          ) : (
            <div
              className="tube-diagram"
              style={{ "--tube-color": tubeColor } as CSSProperties}
            >
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
                    <div className="tube-stop-head">
                      {station.stopId && props.onStationClick ? (
                        <button
                          className="tube-label-button"
                          onClick={() => {
                            buildPerfEvent("build.ui.route_station_click", {
                              stopId: station.stopId,
                            });
                            props.onStationClick?.(station.stopId!);
                          }}
                          title="Open station details"
                        >
                          <strong>{station.name}</strong>
                        </button>
                      ) : (
                        <strong>{station.name}</strong>
                      )}
                      <span className="tube-stop-role">
                        {index === 0 ? "Origin" : index === diagramStops.length - 1 ? "Terminus" : `Stop ${index + 1}`}
                      </span>
                      {isDraftOnly && station.stopId && props.onRemoveDraftStation ? (
                        <button
                          className="tube-stop-remove-inline"
                          disabled={diagramStops.length <= 1}
                          onClick={() => {
                            buildPerfEvent("build.ui.remove_draft_station_click", {
                              stopId: station.stopId,
                              stationCount: diagramStops.length,
                            });
                            props.onRemoveDraftStation?.(station.stopId!);
                          }}
                        >
                          Remove
                        </button>
                      ) : null}
                    </div>
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
          {isDraftOnly ? null : (
            <div className="inspector-actions">
              {canEdit ? (
                <button
                  className="primary"
                  onClick={() => {
                    buildPerfEvent("build.ui.edit_route_click", {
                      lineId: props.lineDetail?.lineId ?? null,
                    });
                    props.onAddStationToLine();
                  }}
                >
                  Edit Route
                </button>
              ) : null}
              {canEdit ? (
                <button
                  className="danger-button"
                  onClick={() => {
                    buildPerfEvent("build.ui.delete_line_click", {
                      lineId: props.lineDetail?.lineId ?? null,
                    });
                    props.onDelete();
                  }}
                >
                  Delete Line
                </button>
              ) : null}
            </div>
          )}
          {!isDraftOnly ? (
            <p className="hint-line">Route editing is map-first. Use this tab to guide and review your route structure.</p>
          ) : null}
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
            <span>{activeBandLabel}</span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Service</small>
              <strong>{serviceStatusText}</strong>
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
              <small>Boarding Attempts - {passengerProvenanceLabel}</small>
              <strong>{live ? Math.round(live.boardings_attempted).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Boarded - {passengerProvenanceLabel}</small>
              <strong>{live ? Math.round(live.boardings_served).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Denied - {passengerProvenanceLabel}</small>
              <strong>{live ? Math.round(live.denied_boardings).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Alighted - {passengerProvenanceLabel}</small>
              <strong>{live ? Math.round(live.alightings_served).toLocaleString() : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Queue End - {passengerProvenanceLabel}</small>
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
    </InspectorPanel>
  );
}
