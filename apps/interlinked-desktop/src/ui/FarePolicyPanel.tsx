import type { FarePolicyManifest } from "../types";

function formatMoney(value: number): string {
  return value.toFixed(2);
}

type FareDraft = Partial<FarePolicyManifest>;

function fareField(
  label: string,
  value: number,
  busy: boolean,
  onChange: (next: number) => void
) {
  return (
    <label>
      {label}
      <input
        type="number"
        step="0.1"
        value={formatMoney(value)}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
        disabled={busy}
      />
    </label>
  );
}

export default function FarePolicyPanel(props: {
  open: boolean;
  busy: boolean;
  policy: FarePolicyManifest | null;
  onClose: () => void;
  onChange: (patch: FareDraft) => void;
}) {
  if (!props.open || !props.policy) return null;
  const p = props.policy;

  return (
    <aside className="editor-drawer-sheet fare-policy-sheet">
      <div className="editor-drawer-head">
        <div>
          <p>Operations</p>
          <h4>Fare Policy</h4>
        </div>
        <button onClick={props.onClose}>Close</button>
      </div>
      <label className="fare-toggle">
        <input
          type="checkbox"
          checked={p.enabled}
          onChange={(e) => props.onChange({ enabled: e.target.checked })}
          disabled={props.busy}
        />
        <span>Enable fare effects</span>
      </label>
      <div className="inspector-grid">
        {fareField("Bus Fare", p.fare_mode_bus_base, props.busy, (value) =>
          props.onChange({ fare_mode_bus_base: value })
        )}
        {fareField("Tram Fare", p.fare_mode_tram_base, props.busy, (value) =>
          props.onChange({ fare_mode_tram_base: value })
        )}
        {fareField("Metro Fare", p.fare_mode_metro_base, props.busy, (value) =>
          props.onChange({ fare_mode_metro_base: value })
        )}
        {fareField("Rail Fare", p.fare_mode_rail_base, props.busy, (value) =>
          props.onChange({ fare_mode_rail_base: value })
        )}
        {fareField("Ferry Fare", p.fare_mode_ferry_base, props.busy, (value) =>
          props.onChange({ fare_mode_ferry_base: value })
        )}
        {fareField("Default Fare", p.fare_mode_default_base, props.busy, (value) =>
          props.onChange({ fare_mode_default_base: value })
        )}
      </div>
      <label>
        Transfer Window (minutes)
        <input
          type="number"
          step="1"
          min="0"
          value={Math.round(p.transfer_window_s / 60)}
          onChange={(e) =>
            props.onChange({ transfer_window_s: Math.max(0, Number(e.target.value) || 0) * 60 })
          }
          disabled={props.busy}
        />
      </label>
      <p className="hint-line">Free transfers per trip: {p.free_transfers_per_trip} (fixed in v1)</p>
    </aside>
  );
}
