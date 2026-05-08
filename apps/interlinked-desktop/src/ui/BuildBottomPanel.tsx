import { useEffect, useMemo, type CSSProperties } from "react";
import type { CurrencyCode, ModeBuildPreset, NetworkMutationSummary } from "../types";
import type { BuildAction, DraftImpact } from "../build/types";
import { canonicalModeClass, type ModeClass } from "../modes";
import { buildPerfEvent, buildPerfMeasure } from "../perf/buildPerf";

type TransportOption = {
  modeClass: Exclude<ModeClass, "rail" | "unknown">;
  label: string;
};

const TRANSPORT_OPTIONS: TransportOption[] = [
  { modeClass: "metro", label: "Metro" },
  { modeClass: "tram", label: "Tram" },
  { modeClass: "bus", label: "Bus" },
  { modeClass: "ferry", label: "Ferry" },
  { modeClass: "commuter_rail", label: "Commuter" },
  { modeClass: "high_speed_rail", label: "High Speed" },
];

function formatMoney(value: number | null | undefined, currency: CurrencyCode): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
}

export default function BuildBottomPanel(props: {
  presets: ModeBuildPreset[];
  transportPresetId: string;
  buildAction: BuildAction;
  budgetCurrency: CurrencyCode;
  mutationPreview?: NetworkMutationSummary | null;
  estimatedCapexBase?: number | null;
  draftImpact?: DraftImpact | null;
  stationCapexBase?: number | null;
  isDirty: boolean;
  builderBusy: boolean;
  builderError?: string | null;
  activeLineStopCount?: number;
  onSelectBuildAction: (action: BuildAction) => void;
  onTransportPresetChange: (presetId: string) => void;
  onApplyDraft: () => void;
}) {
  const presetByModeClass = useMemo(() => {
    return buildPerfMeasure(
      "build.ui.derive.bottom_transport_mode_map",
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
  const fleetPurchaseBase = summary?.fleet_purchase_delta_base ?? null;
  const fleetConfigurationBase = summary?.fleet_configuration_delta_base ?? null;
  const fleetTotalBase =
    fleetPurchaseBase === null && fleetConfigurationBase === null
      ? null
      : (fleetPurchaseBase ?? 0) + (fleetConfigurationBase ?? 0);
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

  const ensureMetroPresetSelected = () => {
    if (!metroPreset) return;
    if (props.transportPresetId !== metroPreset.id) {
      props.onTransportPresetChange(metroPreset.id);
    }
  };

  return (
    <section
      className={`session-bottom-panel build-bottom-panel ${draftInProgress ? "is-draft-active" : "is-idle"}`}
      style={{ "--build-accent": accent } as CSSProperties}
    >
      <div className="bottom-panel-head build-bottom-head">
        <div>
          <p>Build Mode</p>
          <h3>{draftInProgress ? "Drafting Network" : "Construction Actions"}</h3>
        </div>
      </div>

      <div className="build-bottom-grid">
        <section className="build-bottom-section">
          <div className="build-bottom-section-head">
            <h4>Mode</h4>
          </div>
          <div className="build-bottom-mode-strip">
            {TRANSPORT_OPTIONS.map((option) => {
              const preset = presetByModeClass.get(option.modeClass);
              const enabled = option.modeClass === "metro" && Boolean(preset);
              const isActive = option.modeClass === "metro";
              return (
                <button
                  key={option.modeClass}
                  className={isActive ? "active" : ""}
                  disabled={!enabled}
                  onClick={() => {
                    if (!preset) return;
                    buildPerfEvent("build.ui.bottom_transport_mode_click", {
                      modeClass: option.modeClass,
                      enabled,
                    });
                    props.onTransportPresetChange(preset.id);
                    props.onSelectBuildAction("select");
                  }}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </section>

        <section className="build-bottom-section">
          <div className="build-bottom-section-head">
            <h4>Actions</h4>
          </div>
          <div className="build-bottom-action-row">
            <button
              className={props.buildAction === "start_line" ? "active" : ""}
              onClick={() => {
                buildPerfEvent("build.ui.bottom_new_metro_line_click");
                ensureMetroPresetSelected();
                props.onSelectBuildAction("start_line");
              }}
            >
              New Line
            </button>
            <button
              className={props.buildAction === "place_station" ? "active" : ""}
              onClick={() => {
                buildPerfEvent("build.ui.bottom_new_station_click");
                ensureMetroPresetSelected();
                props.onSelectBuildAction("place_station");
              }}
            >
              New Station
            </button>
          </div>
        </section>

        <section className="build-bottom-section build-bottom-costs">
          <div className="build-bottom-section-head">
            <h4>Cost Summary</h4>
          </div>
          <div className="build-bottom-cost-receipt">
            <div>
              <small>Stations</small>
              <strong>{formatMoney(stationSubtotalBase, props.budgetCurrency)}</strong>
            </div>
            <div>
              <small>Line construction</small>
              <strong>{formatMoney(constructionCostBase, props.budgetCurrency)}</strong>
            </div>
            <div>
              <small>Fleet</small>
              <strong>{formatMoney(fleetTotalBase, props.budgetCurrency)}</strong>
            </div>
            <div>
              <small>Opex / hr</small>
              <strong>{formatMoney(operatingCostPerHourBase, props.budgetCurrency)}</strong>
            </div>
            <div className="is-total">
              <small>Commit</small>
              <strong>{formatMoney(commitTotalBase, props.budgetCurrency)}</strong>
            </div>
          </div>
        </section>

        <section className="build-bottom-apply">
          <button
            className="primary"
            disabled={!props.isDirty || props.builderBusy}
            onClick={() => {
              buildPerfEvent("build.ui.bottom_apply_changes_click", {
                isDirty: props.isDirty,
                builderBusy: props.builderBusy,
              });
              props.onApplyDraft();
            }}
          >
            {props.builderBusy ? "Applying..." : "Apply Changes"}
          </button>
        </section>
      </div>

      {props.builderError ? <div className="build-bottom-error">{props.builderError}</div> : null}
    </section>
  );
}
