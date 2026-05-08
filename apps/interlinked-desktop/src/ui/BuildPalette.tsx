import { useEffect, useMemo, type CSSProperties } from "react";
import type { CurrencyCode, ModeBuildPreset, NetworkMutationSummary } from "../types";
import type { BuildAction } from "../build/types";
import type { DraftImpact } from "../build/types";
import { canonicalModeClass, type ModeClass } from "../modes";
import { buildPerfEvent, buildPerfMeasure } from "../perf/buildPerf";

type LineRow = {
  lineId: string;
  name: string;
  mode: string;
  modeVariant?: string | null;
  displayColor?: string | null;
};

type TransportOption = {
  modeClass: Exclude<ModeClass, "rail" | "unknown">;
  label: string;
  icon: "train" | "bus" | "ferry";
};

const TRANSPORT_OPTIONS: TransportOption[] = [
  { modeClass: "metro", label: "Metro", icon: "train" },
  { modeClass: "tram", label: "Tram", icon: "train" },
  { modeClass: "bus", label: "Bus", icon: "bus" },
  { modeClass: "ferry", label: "Ferry", icon: "ferry" },
  { modeClass: "commuter_rail", label: "Commuter Rail", icon: "train" },
  { modeClass: "high_speed_rail", label: "High Speed Rail", icon: "train" },
];

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
  selectedLineId?: string | null;
  budgetCurrency: CurrencyCode;
  mutationPreview?: NetworkMutationSummary | null;
  estimatedCapexBase?: number | null;
  draftImpact?: DraftImpact | null;
  stationCapexBase?: number | null;
  isDirty: boolean;
  builderBusy: boolean;
  builderError?: string | null;
  activeLineStopCount?: number;
  onExitBuildMode: () => void;
  onSelectBuildAction: (action: BuildAction) => void;
  onTransportPresetChange: (presetId: string) => void;
  onSelectLine: (lineId: string) => void;
  onApplyDraft: () => void;
}) {
  const formatMoney = (value: number | null | undefined): string => {
    if (value === null || value === undefined) return "-";
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: props.budgetCurrency,
      maximumFractionDigits: 0,
    }).format(value);
  };

  const presetByModeClass = useMemo(() => {
    return buildPerfMeasure(
      "build.ui.derive.transport_mode_map",
      () => {
        const map = new Map<TransportOption["modeClass"], ModeBuildPreset>();
        for (const preset of props.presets) {
          const modeClass = canonicalModeClass(preset.engine_mode, preset.mode_variant ?? null);
          if (
            modeClass === "metro" ||
            modeClass === "tram" ||
            modeClass === "bus" ||
            modeClass === "ferry" ||
            modeClass === "commuter_rail" ||
            modeClass === "high_speed_rail"
          ) {
            if (!map.has(modeClass)) map.set(modeClass, preset);
          }
        }
        return map;
      },
      { presetCount: props.presets.length },
      { minDurationMs: 1, throttleMs: 200 }
    );
  }, [props.presets]);

  const metroPreset = presetByModeClass.get("metro") ?? props.presets[0] ?? null;
  const accent = metroPreset?.default_color ?? "#104894";
  const lineActive = (props.activeLineStopCount ?? 0) > 0;
  const draftToolActive =
    props.buildAction === "start_line" || props.buildAction === "add_station_to_line";
  const draftInProgress = draftToolActive || lineActive;
  const showCostSummary = draftInProgress || props.isDirty;
  const draftStateLabel =
    props.buildAction === "start_line" ? "New Metro Line" : "Editing Metro Line";
  const pendingStateLabel = draftInProgress ? draftStateLabel : "Pending Build Changes";
  const summary = props.mutationPreview ?? null;
  const addedStations = Math.max(props.draftImpact?.addedStations ?? 0, 0);
  const stationUnitCostBase =
    typeof props.stationCapexBase === "number" && Number.isFinite(props.stationCapexBase)
      ? Math.max(props.stationCapexBase, 0)
      : null;
  const stationSubtotalBase =
    stationUnitCostBase !== null ? Math.max(addedStations, 0) * stationUnitCostBase : null;
  const constructionCostBase =
    summary?.construction_cost_delta_base ?? props.estimatedCapexBase ?? null;
  const infrastructureResidualBase =
    constructionCostBase !== null && stationSubtotalBase !== null
      ? Math.max(constructionCostBase - stationSubtotalBase, 0)
      : null;
  const fleetPurchaseBase = summary?.fleet_purchase_delta_base ?? null;
  const fleetConfigurationBase = summary?.fleet_configuration_delta_base ?? null;
  const capitalCostBase =
    summary
      ? (summary.construction_cost_delta_base ?? 0) +
        (summary.fleet_purchase_delta_base ?? 0) +
        (summary.fleet_configuration_delta_base ?? 0)
      : props.estimatedCapexBase ?? null;
  const operatingCostPerHourBase = summary?.projected_opex_per_hour_base ?? null;
  const commitTotalBase = summary?.apply_total_delta_base ?? capitalCostBase;

  useEffect(() => {
    if (!metroPreset) return;
    if (props.transportPresetId !== metroPreset.id) {
      props.onTransportPresetChange(metroPreset.id);
    }
  }, [metroPreset, props.onTransportPresetChange, props.transportPresetId]);

  const metroLines = useMemo(
    () =>
      buildPerfMeasure(
        "build.ui.derive.metro_lines",
        () =>
          props.lines.filter(
            (line) => canonicalModeClass(line.mode, line.modeVariant ?? null) === "metro"
          ),
        { lineCount: props.lines.length },
        { minDurationMs: 1, throttleMs: 200 }
      ),
    [props.lines]
  );

  const ensureMetroPresetSelected = () => {
    if (!metroPreset) return;
    if (props.transportPresetId !== metroPreset.id) {
      props.onTransportPresetChange(metroPreset.id);
    }
  };

  return (
    <section
      className={`build-workspace ${draftInProgress ? "is-draft-active" : "is-idle"}`}
      style={{ "--build-accent": accent } as CSSProperties}
    >
      <div className="build-workspace-head">
        <p>Build Workspace</p>
        <strong>Metro Tools</strong>
        <span>{showCostSummary ? pendingStateLabel : "Idle Build Workspace"}</span>
      </div>

      <div className="build-workspace-scroll">
        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Transport Modes</h4>
          </div>
          <div className="build-mode-strip" role="list" aria-label="Transport modes">
            {TRANSPORT_OPTIONS.map((option) => {
              const preset = presetByModeClass.get(option.modeClass);
              const enabled = option.modeClass === "metro" && Boolean(preset);
              const isActive = option.modeClass === "metro";
              return (
                <button
                  key={option.modeClass}
                  className={`build-mode-tile build-mode-strip-tile ${isActive ? "active" : ""} ${
                    enabled ? "" : "disabled"
                  }`}
                  disabled={!enabled}
                  onClick={() => {
                    if (!preset) return;
                    buildPerfEvent("build.ui.transport_mode_click", {
                      modeClass: option.modeClass,
                      enabled,
                    });
                    props.onTransportPresetChange(preset.id);
                    props.onSelectBuildAction("select");
                  }}
                >
                  <span className="build-mode-icon">
                    <ModeIcon kind={option.icon} />
                  </span>
                  <span className="build-mode-copy">
                    <strong>{option.label}</strong>
                    {!enabled ? <small>Later</small> : null}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        {!showCostSummary ? (
          <div className="build-stage-card build-tool-section">
            <div className="build-stage-header">
              <h4>Metro Actions</h4>
            </div>
            <div className="build-action-stack build-action-stack-idle">
              <button
                className={props.buildAction === "start_line" ? "active" : ""}
                onClick={() => {
                  buildPerfEvent("build.ui.new_metro_line_click");
                  ensureMetroPresetSelected();
                  props.onSelectBuildAction("start_line");
                }}
              >
                New Metro Line
              </button>
              <button
                className={props.buildAction === "place_station" ? "active" : ""}
                onClick={() => {
                  buildPerfEvent("build.ui.new_station_click");
                  ensureMetroPresetSelected();
                  props.onSelectBuildAction("place_station");
                }}
              >
                New Station
              </button>
            </div>
          </div>
        ) : (
          <div className="build-stage-card build-tool-section">
            <div className="build-stage-header">
              <h4>Cost Summary</h4>
              <span className="build-status-pill">{pendingStateLabel}</span>
            </div>
            <div className="build-cost-receipt">
              <div className="build-cost-line">
                <span>
                  Stations
                  {stationUnitCostBase !== null
                    ? ` (${addedStations} × ${formatMoney(stationUnitCostBase)})`
                    : ` (${addedStations})`}
                </span>
                <strong>{formatMoney(stationSubtotalBase)}</strong>
              </div>
              <div className="build-cost-line">
                <span>Infrastructure + Track</span>
                <strong>{formatMoney(infrastructureResidualBase)}</strong>
              </div>
              <div className="build-cost-line">
                <span>Fleet Purchase</span>
                <strong>{formatMoney(fleetPurchaseBase)}</strong>
              </div>
              <div className="build-cost-line">
                <span>Fleet Configuration</span>
                <strong>{formatMoney(fleetConfigurationBase)}</strong>
              </div>
              <div className="build-cost-divider" />
              <div className="build-cost-line is-total">
                <span>Capital Cost</span>
                <strong>{formatMoney(capitalCostBase)}</strong>
              </div>
              <div className="build-cost-line is-recurring">
                <span>Operating Cost / hr</span>
                <strong>{formatMoney(operatingCostPerHourBase)}</strong>
              </div>
              <div className="build-cost-line is-commit">
                <span>Commit Total</span>
                <strong>{formatMoney(commitTotalBase)}</strong>
              </div>
            </div>
          </div>
        )}

        <div className="build-stage-card build-tool-section">
          <div className="build-stage-header">
            <h4>Metro Lines</h4>
          </div>
          <div className="build-line-list">
            {metroLines.length === 0 ? (
              <p className="hint-line">No metro lines yet.</p>
            ) : (
              metroLines.map((line) => (
                <button
                  key={line.lineId}
                  className={`build-line-row ${
                    props.selectedLineId === line.lineId ? "active" : ""
                  }`}
                  onClick={() => {
                    buildPerfEvent("build.ui.select_line_from_list_click", {
                      lineId: line.lineId,
                    });
                    props.onSelectLine(line.lineId);
                    props.onSelectBuildAction("select");
                  }}
                >
                  <span
                    className="build-line-chip"
                    style={{ backgroundColor: line.displayColor ?? accent }}
                  />
                  <span>{line.name.trim() ? line.name : "Untitled Line"}</span>
                </button>
              ))
            )}
          </div>
        </div>
      </div>

      <div className="build-workspace-footer">
        <button
          onClick={() => {
            buildPerfEvent("build.ui.exit_build_mode_click");
            props.onExitBuildMode();
          }}
        >
          Exit Build Mode
        </button>
        <button
          disabled={!props.isDirty || props.builderBusy}
          onClick={() => {
            buildPerfEvent("build.ui.apply_changes_click", {
              isDirty: props.isDirty,
              builderBusy: props.builderBusy,
            });
            props.onApplyDraft();
          }}
        >
          {props.builderBusy ? "Applying..." : "Apply Changes"}
        </button>
      </div>

      {props.builderError ? <div className="build-inline-error">{props.builderError}</div> : null}
    </section>
  );
}
