import { useMemo, type CSSProperties } from "react";
import type { CurrencyCode, ModeBuildPreset, NetworkMutationSummary } from "../types";
import type { BuildAction } from "../build/types";

type LineRow = {
  lineId: string;
  name: string;
  mode: string;
  modeVariant?: string | null;
  displayColor?: string | null;
};

function formatMoney(value: number | null | undefined, currency: CurrencyCode): string {
  if (value === null || value === undefined) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
}

function modeMatchesPreset(line: LineRow, preset: ModeBuildPreset | null): boolean {
  if (!preset) return false;
  const sameMode = line.mode === preset.engine_mode;
  const lineVariant = line.modeVariant ?? null;
  const presetVariant = preset.mode_variant ?? null;
  if (!sameMode) return false;
  if (presetVariant === null) return true;
  return lineVariant === presetVariant;
}

function modeIconKind(presetId: string): "train" | "bus" | "ferry" {
  if (presetId === "bus") return "bus";
  if (presetId === "ferry") return "ferry";
  return "train";
}

function ModeIcon(props: { kind: "train" | "bus" | "ferry" }) {
  if (props.kind === "bus") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="3" y="5" width="18" height="12" rx="3" />
        <line x1="6" y1="17" x2="6" y2="20" />
        <line x1="18" y1="17" x2="18" y2="20" />
        <line x1="7" y1="9" x2="17" y2="9" />
      </svg>
    );
  }
  if (props.kind === "ferry") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M5 14h14l-2 4H7z" />
        <line x1="8" y1="14" x2="8" y2="8" />
        <line x1="16" y1="14" x2="16" y2="8" />
        <line x1="4" y1="20" x2="20" y2="20" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="5" y="4" width="14" height="13" rx="4" />
      <line x1="9" y1="8" x2="11" y2="8" />
      <line x1="13" y1="8" x2="15" y2="8" />
      <line x1="9" y1="17" x2="8" y2="20" />
      <line x1="15" y1="17" x2="16" y2="20" />
    </svg>
  );
}

export default function BuildPalette(props: {
  presets: ModeBuildPreset[];
  lines: LineRow[];
  transportPresetId: string;
  buildAction: BuildAction;
  hasSelectedLine?: boolean;
  hasSelectedStop?: boolean;
  selectedLineId?: string | null;
  selectedLineName?: string | null;
  selectedStopName?: string | null;
  selectedLineConstructionCostBase?: number | null;
  mutationPreview: NetworkMutationSummary | null;
  isDirty: boolean;
  builderBusy: boolean;
  builderError?: string | null;
  budgetCurrency: CurrencyCode;
  stationCostBase?: number | null;
  lineCostPerKmBase?: number | null;
  extensionAddedStations?: number;
  extensionAddedLengthM?: number;
  extensionConstructionCostBase?: number | null;
  activeLineStopCount?: number;
  canUndoDraftPlacement?: boolean;
  onExitBuildMode: () => void;
  onSelectBuildAction: (action: BuildAction) => void;
  onArmLineExtension?: () => void;
  onTransportPresetChange: (presetId: string) => void;
  onSelectLine: (lineId: string) => void;
  onApplyDraft: () => void;
  onFinishLine: () => void;
  onUndoLinePlacement?: () => void;
}) {
  const selectedPreset =
    props.presets.find((preset) => preset.id === props.transportPresetId) ?? props.presets[0] ?? null;
  const accent = selectedPreset?.default_color ?? "#104894";
  const summary = props.mutationPreview;
  const lineActive = (props.activeLineStopCount ?? 0) > 0;
  const stationCostBase = Math.max(props.stationCostBase ?? 0, 0);
  const perKmLineCostBase = Math.max(props.lineCostPerKmBase ?? selectedPreset?.capex_per_km_base ?? 0, 0);
  const extensionAddedStations = Math.max(props.extensionAddedStations ?? 0, 0);
  const activeLineLengthKm = Math.max(props.extensionAddedLengthM ?? 0, 0) / 1000;
  const stationComponentCostBase = stationCostBase * extensionAddedStations;
  const trackComponentCostBase = activeLineLengthKm * perKmLineCostBase;
  const extensionEstimateBase = props.extensionConstructionCostBase ?? stationComponentCostBase + trackComponentCostBase;
  const filteredLines = useMemo(
    () => props.lines.filter((line) => modeMatchesPreset(line, selectedPreset)),
    [props.lines, selectedPreset]
  );
  const selectedLineLabel = props.selectedLineName?.trim()
    ? props.selectedLineName
    : props.hasSelectedLine
      ? "Selected Line"
      : "New Line";
  const selectedLineCostBase =
    props.selectedLineConstructionCostBase ?? summary?.construction_cost_delta_base ?? null;
  const showExtensionCost = props.buildAction === "add_station_to_line";
  const oneTimeConstruction = summary?.construction_cost_delta_base ?? 0;
  const oneTimeFleetPurchase = summary?.fleet_purchase_delta_base ?? 0;
  const oneTimeFleetConfig = summary?.fleet_configuration_delta_base ?? 0;
  const oneTimeTotal = summary?.apply_total_delta_base ?? 0;
  const recurringOpex = summary?.projected_opex_per_hour_base ?? 0;
  const recurringStaff = summary?.projected_staff_opex_per_hour_base ?? 0;
  const showFleetPurchaseCard = Number.isFinite(oneTimeFleetPurchase) && Math.abs(oneTimeFleetPurchase) > 1;
  const showFleetConfigCard = Number.isFinite(oneTimeFleetConfig) && Math.abs(oneTimeFleetConfig) > 1;
  const showRecurringStaffCard = Number.isFinite(recurringStaff) && Math.abs(recurringStaff) > 1;
  const activeToolLabel = useMemo(() => {
    if (props.buildAction === "start_line") return `New ${selectedPreset?.label ?? "Line"}`;
    if (props.buildAction === "add_station_to_line") return "Extend Selected Line";
    if (props.buildAction === "place_station") return `New ${selectedPreset?.label ?? "Station"}`;
    if (props.buildAction === "delete") return "Remove Draft Stops";
    return "Inspect Existing Objects";
  }, [props.buildAction, selectedPreset?.label]);
  const selectedObjectLabel = useMemo(() => {
    if (props.hasSelectedLine) {
      if (props.selectedLineName?.trim()) return `Line: ${props.selectedLineName}`;
      return "Line: Untitled Line";
    }
    if (props.hasSelectedStop) {
      if (props.selectedStopName?.trim()) return `Station: ${props.selectedStopName}`;
      return "Station selected";
    }
    return "No object selected";
  }, [props.hasSelectedLine, props.hasSelectedStop, props.selectedLineName, props.selectedStopName]);
  const toolHint = useMemo(() => {
    if (props.buildAction === "start_line") {
      if (lineActive) return "Click map or stations to add stops. Finish when the route is complete.";
      return "Click the map to place the first stop and begin a new line.";
    }
    if (props.buildAction === "add_station_to_line") {
      if (lineActive) return "Select a terminus then click map or stations to continue the extension.";
      if (!props.hasSelectedLine) return "Select a line on the map first, then extend from a terminus.";
      return "Click a terminus station to lock extension, then keep adding stops.";
    }
    if (props.buildAction === "place_station") {
      return "Click the map to place a standalone station.";
    }
    if (props.buildAction === "delete") {
      if (!lineActive) return "Start or extend a line to remove draft stations.";
      return "Click a station in the current draft route to remove it.";
    }
    return "Click any line or station on the map to inspect and edit it.";
  }, [lineActive, props.buildAction, props.hasSelectedLine]);
  const canExtendSelected = props.hasSelectedLine || lineActive;
  const canUseDeleteTool = lineActive;

  return (
    <section className="build-workspace" style={{ "--build-accent": accent } as CSSProperties}>
      <div className="build-workspace-head">
        <p>Build Workspace</p>
        <strong>{selectedPreset ? `${selectedPreset.label} Tools` : "Build Tools"}</strong>
        <span>{activeToolLabel}</span>
      </div>

      <div className="build-workspace-scroll">
        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Transport Mode</h4>
          </div>
          <div className="build-mode-grid">
            {props.presets.map((preset) => (
              <button
                key={preset.id}
                className={`build-mode-tile ${preset.id === props.transportPresetId ? "active" : ""}`}
                onClick={() => {
                  props.onTransportPresetChange(preset.id);
                  props.onSelectBuildAction("select");
                }}
              >
                <span className="build-mode-icon">
                  <ModeIcon kind={modeIconKind(preset.id)} />
                </span>
                <span>{preset.label}</span>
              </button>
            ))}
          </div>
        </div>

        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Tools</h4>
          </div>
          <div className="build-action-stack">
            <button
              className={props.buildAction === "select" ? "active" : ""}
              onClick={() => props.onSelectBuildAction("select")}
            >
              Edit Existing
            </button>
            <button
              className={props.buildAction === "start_line" ? "active" : ""}
              onClick={() => props.onSelectBuildAction("start_line")}
            >
              New Line
            </button>
            <button
              className={props.buildAction === "place_station" ? "active" : ""}
              onClick={() => props.onSelectBuildAction("place_station")}
            >
              New Station
            </button>
            <button
              className={props.buildAction === "add_station_to_line" ? "active" : ""}
              disabled={!canExtendSelected}
              onClick={() => {
                if (props.onArmLineExtension && props.hasSelectedLine && !lineActive) {
                  props.onArmLineExtension();
                  return;
                }
                props.onSelectBuildAction("add_station_to_line");
              }}
            >
              Extend Selected
            </button>
            <button
              className={props.buildAction === "delete" ? "active" : ""}
              disabled={!canUseDeleteTool}
              onClick={() =>
                props.onSelectBuildAction(
                  props.buildAction === "delete"
                    ? props.hasSelectedLine
                      ? "add_station_to_line"
                      : "start_line"
                    : "delete"
                )
              }
            >
              {props.buildAction === "delete" ? "Done Removing" : "Remove Draft Stop"}
            </button>
          </div>
        </div>

        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Context</h4>
          </div>
          <div className="build-line-focus">
            <strong>{selectedObjectLabel}</strong>
            <span>{toolHint}</span>
          </div>
          {(props.buildAction === "add_station_to_line" || props.buildAction === "start_line") ? (
            <div className={`line-draw-status ${lineActive ? "is-active" : ""}`}>
              <span className="line-draw-dot" />
              {lineActive
                ? `Draft route has ${props.activeLineStopCount} station${props.activeLineStopCount === 1 ? "" : "s"}`
                : "No draft route yet"}
            </div>
          ) : null}
          {lineActive ? (
            <div className="build-line-actions">
              <button className="build-finish-button" onClick={props.onFinishLine}>
                Finish Route ({props.activeLineStopCount} stops)
              </button>
              <button onClick={() => props.onUndoLinePlacement?.()} disabled={!props.canUndoDraftPlacement}>
                Undo Last
              </button>
              <button onClick={() => props.onSelectBuildAction("select")}>Cancel Tool</button>
            </div>
          ) : (
            <div className="build-line-actions build-line-actions-single">
              <button onClick={() => props.onSelectBuildAction("select")}>Cancel Tool</button>
            </div>
          )}
        </div>

        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>{selectedPreset?.label ?? "Selected"} Lines</h4>
          </div>
          <div className="build-line-list">
            {filteredLines.length === 0 ? (
              <p className="hint-line">No {selectedPreset?.label ?? "selected"} lines yet.</p>
            ) : (
              filteredLines.map((line) => (
                <button
                  key={line.lineId}
                  className={`build-line-row ${props.selectedLineId === line.lineId ? "active" : ""}`}
                  onClick={() => {
                    props.onSelectLine(line.lineId);
                    props.onSelectBuildAction("select");
                  }}
                >
                  <span className="build-line-chip" style={{ backgroundColor: line.displayColor ?? accent }} />
                  <span>{line.name.trim() ? line.name : "Untitled Line"}</span>
                </button>
              ))
            )}
          </div>
        </div>

        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Draft Impact</h4>
          </div>
          <div className="build-line-focus">
            <strong>{selectedLineLabel}</strong>
            <span>Construction value: {formatMoney(selectedLineCostBase, props.budgetCurrency)}</span>
          </div>
          {summary ? (
            <div className="build-draft-grid">
              <div>
                <small>One-Time Construction</small>
                <strong>{formatMoney(oneTimeConstruction, props.budgetCurrency)}</strong>
              </div>
              {showFleetPurchaseCard ? (
                <div>
                  <small>One-Time Fleet Purchase</small>
                  <strong>{formatMoney(oneTimeFleetPurchase, props.budgetCurrency)}</strong>
                </div>
              ) : null}
              {showFleetConfigCard ? (
                <div>
                  <small>One-Time Fleet Config</small>
                  <strong>{formatMoney(oneTimeFleetConfig, props.budgetCurrency)}</strong>
                </div>
              ) : null}
              <div>
                <small>Apply Total</small>
                <strong>{formatMoney(oneTimeTotal, props.budgetCurrency)}</strong>
              </div>
              <div>
                <small>Recurring Opex / hr</small>
                <strong>{formatMoney(recurringOpex, props.budgetCurrency)}</strong>
              </div>
              {showRecurringStaffCard ? (
                <div>
                  <small>Staff Cost / hr</small>
                  <strong>{formatMoney(recurringStaff, props.budgetCurrency)}</strong>
                </div>
              ) : null}
            </div>
          ) : (
            <p className="hint-line">Start editing on the map to generate a live draft impact preview.</p>
          )}
          {selectedPreset?.id === "bus" ? (
            <p className="hint-line">
              Bus routes use road network infrastructure, so per-km construction is set to zero. Costs come from stops,
              fleet, and operations.
            </p>
          ) : null}
          {showExtensionCost ? (
            <>
              <div className="build-draft-grid">
                <div>
                  <small>New Stations</small>
                  <strong>{extensionAddedStations}</strong>
                </div>
                <div>
                  <small>Added Track</small>
                  <strong>{activeLineLengthKm.toFixed(2)} km</strong>
                </div>
                <div>
                  <small>Station Build Cost</small>
                  <strong>{formatMoney(stationComponentCostBase, props.budgetCurrency)}</strong>
                </div>
                <div>
                  <small>Track Build Cost</small>
                  <strong>{formatMoney(trackComponentCostBase, props.budgetCurrency)}</strong>
                </div>
              </div>
              <div className="build-line-focus">
                <strong>Estimated Extension Cost: {formatMoney(extensionEstimateBase, props.budgetCurrency)}</strong>
                <span>
                  {formatMoney(stationCostBase, props.budgetCurrency)} x {extensionAddedStations} stations +{" "}
                  {formatMoney(perKmLineCostBase, props.budgetCurrency)} x {activeLineLengthKm.toFixed(2)} km
                </span>
              </div>
            </>
          ) : null}
        </div>
      </div>

      <div className="build-workspace-footer">
        <button onClick={props.onExitBuildMode}>Exit Build Mode</button>
        <button disabled={!props.isDirty || props.builderBusy} onClick={props.onApplyDraft}>
          {props.builderBusy ? "Applying..." : "Apply Changes"}
        </button>
      </div>

      {props.builderError ? <div className="build-inline-error">{props.builderError}</div> : null}
    </section>
  );
}
