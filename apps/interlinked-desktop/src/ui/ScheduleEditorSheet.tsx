import { useEffect, useState } from "react";
import type { CurrencyCode, ModeBuildPreset } from "../types";
import type { LineSchedulePatch } from "../build/helpers";

function formatMoney(value: number | null | undefined, currency: CurrencyCode): string {
  if (value === null || value === undefined) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
}

function toNumber(value: string, fallback = 0): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function minuteToInput(minute: number): string {
  const normalized = ((Math.round(minute) % 1440) + 1440) % 1440;
  const hh = Math.floor(normalized / 60)
    .toString()
    .padStart(2, "0");
  const mm = (normalized % 60).toString().padStart(2, "0");
  return `${hh}:${mm}`;
}

function inputToMinute(value: string, fallback: number): number {
  const parts = value.split(":").map((item) => Number(item));
  if (parts.length !== 2 || !Number.isFinite(parts[0]) || !Number.isFinite(parts[1])) return fallback;
  const hh = Math.max(0, Math.min(23, Math.round(parts[0])));
  const mm = Math.max(0, Math.min(59, Math.round(parts[1])));
  return hh * 60 + mm;
}

function requiredUnits(roundTripS: number, tph: number): number {
  if (roundTripS <= 0 || tph <= 0) return 0;
  return Math.ceil((roundTripS * tph) / 3600);
}

function avgWaitS(tph: number): number | null {
  if (tph <= 0) return null;
  return 1800 / tph;
}

function formatSeconds(value: number | null): string {
  if (value === null) return "-";
  if (value >= 3600) return `${(value / 3600).toFixed(1)}h`;
  if (value >= 60) return `${Math.round(value / 60)} min`;
  return `${Math.round(value)}s`;
}

type BandKey = "off_peak" | "peak" | "overnight";

type ScheduleState = {
  peak_start_minute: number;
  peak_end_minute: number;
  overnight_start_minute: number;
  overnight_end_minute: number;
  tph_peak: number;
  tph_off_peak: number;
  tph_overnight: number;
};

export default function ScheduleEditorSheet(props: {
  open: boolean;
  editable: boolean;
  lineName: string;
  preset: ModeBuildPreset | null;
  budgetCurrency: CurrencyCode;
  unitsOwned: number;
  roundTripS: number;
  schedule: ScheduleState;
  onClose: () => void;
  onOpenRollingStockEditor: () => void;
  onSave: (patch: LineSchedulePatch) => void;
}) {
  const [draft, setDraft] = useState<ScheduleState>(props.schedule);

  useEffect(() => {
    if (!props.open) return;
    setDraft(props.schedule);
  }, [props.open, props.schedule]);

  const preset = props.preset;
  if (!props.open || !preset) return null;

  const canEditSchedule = props.editable && props.unitsOwned > 0;
  const dirty =
    draft.peak_start_minute !== props.schedule.peak_start_minute ||
    draft.peak_end_minute !== props.schedule.peak_end_minute ||
    draft.overnight_start_minute !== props.schedule.overnight_start_minute ||
    draft.overnight_end_minute !== props.schedule.overnight_end_minute ||
    draft.tph_peak !== props.schedule.tph_peak ||
    draft.tph_off_peak !== props.schedule.tph_off_peak ||
    draft.tph_overnight !== props.schedule.tph_overnight;
  const bandRows: Array<{
    key: BandKey;
    label: string;
    tph: number;
    shiftMultiplier: number;
  }> = [
    { key: "off_peak", label: "Off-Peak", tph: draft.tph_off_peak, shiftMultiplier: 1 },
    {
      key: "peak",
      label: "Peak",
      tph: draft.tph_peak,
      shiftMultiplier: Math.max(preset.staff_shift_multiplier_peak, 0),
    },
    {
      key: "overnight",
      label: "Overnight",
      tph: draft.tph_overnight,
      shiftMultiplier: Math.max(preset.staff_shift_multiplier_overnight, 0),
    },
  ];

  const handleBack = () => {
    if (props.editable && dirty) {
      const discard = window.confirm("You have unsaved timetable changes. Discard them?");
      if (!discard) return;
    }
    props.onClose();
  };

  const handleSave = () => {
    props.onSave({
      peak_start_minute: draft.peak_start_minute,
      peak_end_minute: draft.peak_end_minute,
      overnight_start_minute: draft.overnight_start_minute,
      overnight_end_minute: draft.overnight_end_minute,
      tph_peak: draft.tph_peak,
      tph_off_peak: draft.tph_off_peak,
      tph_overnight: draft.tph_overnight,
    });
  };
  const handleSaveAndBack = () => {
    handleSave();
    props.onClose();
  };

  return (
    <aside className="editor-drawer-sheet">
      <div className="editor-drawer-head">
        <div>
          <p>Scheduling Editor</p>
          <h4>{props.lineName}</h4>
        </div>
        <button onClick={handleBack}>Back To Line</button>
      </div>

      {!canEditSchedule ? (
        <div className="inspector-section">
          <p className="hint-line">This line has no vehicles yet. Buy rolling stock first to unlock scheduling.</p>
          <button onClick={props.onOpenRollingStockEditor}>Open Rolling Stock Editor</button>
        </div>
      ) : null}

      <div className="inspector-grid">
        <label>
          Peak Starts
          <input
            disabled={!canEditSchedule}
            type="time"
            value={minuteToInput(draft.peak_start_minute)}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                peak_start_minute: inputToMinute(event.target.value, prev.peak_start_minute),
              }))
            }
          />
        </label>
        <label>
          Peak Ends
          <input
            disabled={!canEditSchedule}
            type="time"
            value={minuteToInput(draft.peak_end_minute)}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                peak_end_minute: inputToMinute(event.target.value, prev.peak_end_minute),
              }))
            }
          />
        </label>
        <label>
          Overnight Starts
          <input
            disabled={!canEditSchedule}
            type="time"
            value={minuteToInput(draft.overnight_start_minute)}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                overnight_start_minute: inputToMinute(event.target.value, prev.overnight_start_minute),
              }))
            }
          />
        </label>
        <label>
          Overnight Ends
          <input
            disabled={!canEditSchedule}
            type="time"
            value={minuteToInput(draft.overnight_end_minute)}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                overnight_end_minute: inputToMinute(event.target.value, prev.overnight_end_minute),
              }))
            }
          />
        </label>
      </div>

      <div className="inspector-section">
        <div className="inspector-section-head">
          <h5>Service By Time Band</h5>
          <span>{props.unitsOwned.toLocaleString()} vehicles available</span>
        </div>
        <div className="schedule-band-list">
          {bandRows.map((band) => {
            const needed = requiredUnits(props.roundTripS, band.tph);
            const shortBy = Math.max(needed - props.unitsOwned, 0);
            const staffCost =
              needed * preset.staff_cost_per_unit_hour_base * Math.max(band.shiftMultiplier, 0);
            return (
              <div key={band.key} className={`schedule-band-card ${shortBy > 0 ? "is-warning" : ""}`}>
                <div className="schedule-band-row">
                  <strong>{band.label}</strong>
                  <label>
                    TPH
                    <input
                      disabled={!canEditSchedule}
                      type="number"
                      min={preset.tph_min}
                      max={preset.tph_max}
                      step={preset.tph_step}
                      value={band.tph}
                      onChange={(event) => {
                        const next = toNumber(event.target.value, band.tph);
                        setDraft((prev) =>
                          band.key === "peak"
                            ? { ...prev, tph_peak: next }
                            : band.key === "overnight"
                              ? { ...prev, tph_overnight: next }
                              : { ...prev, tph_off_peak: next }
                        );
                      }}
                    />
                  </label>
                </div>
                <div className="schedule-band-metrics">
                  <span>Average Wait: {formatSeconds(avgWaitS(band.tph))}</span>
                  <span>Vehicles Needed: {needed.toLocaleString()}</span>
                  <span>Staff Cost / Hour: {formatMoney(staffCost, props.budgetCurrency)}</span>
                </div>
                {shortBy > 0 ? <p className="hint-line">Short by {shortBy.toLocaleString()} vehicles for this band.</p> : null}
              </div>
            );
          })}
        </div>
      </div>

      {props.editable ? (
        <div className="editor-drawer-footer">
          <button onClick={handleBack}>Back</button>
          <button className="primary" disabled={!dirty} onClick={handleSaveAndBack}>
            Save And Back
          </button>
        </div>
      ) : null}
    </aside>
  );
}
