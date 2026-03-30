type UiSettings = {
  uiScale: number;
  textScale: number;
  highContrast: boolean;
  reducedMotion: boolean;
  quietAlerts: boolean;
  showDiagnostics: boolean;
  masterVolume: number;
  uiVolume: number;
  gameplayVolume: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

export default function SettingsPanel(props: {
  open: boolean;
  settings: UiSettings;
  onClose: () => void;
  onChange: (next: UiSettings) => void;
  onReset: () => void;
}) {
  if (!props.open) return null;

  const set = <K extends keyof UiSettings>(key: K, value: UiSettings[K]) => {
    props.onChange({ ...props.settings, [key]: value });
  };

  return (
    <div className="settings-overlay" onClick={props.onClose}>
      <aside className="settings-sheet" onClick={(event) => event.stopPropagation()}>
        <div className="settings-head">
          <div>
            <p>Settings</p>
            <h4>Gameplay & Accessibility</h4>
          </div>
          <button onClick={props.onClose}>Close</button>
        </div>

        <section className="settings-section">
          <div className="settings-section-head">
            <h5>Display</h5>
          </div>
          <label>
            UI Scale ({Math.round(props.settings.uiScale * 100)}%)
            <input
              type="range"
              min={80}
              max={125}
              step={1}
              value={Math.round(props.settings.uiScale * 100)}
              onChange={(event) => set("uiScale", clamp(Number(event.target.value) / 100, 0.8, 1.25))}
            />
          </label>
          <label>
            Text Scale ({Math.round(props.settings.textScale * 100)}%)
            <input
              type="range"
              min={85}
              max={130}
              step={1}
              value={Math.round(props.settings.textScale * 100)}
              onChange={(event) => set("textScale", clamp(Number(event.target.value) / 100, 0.85, 1.3))}
            />
          </label>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={props.settings.highContrast}
              onChange={(event) => set("highContrast", event.target.checked)}
            />
            High contrast UI
          </label>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={props.settings.reducedMotion}
              onChange={(event) => set("reducedMotion", event.target.checked)}
            />
            Reduced motion
          </label>
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <h5>Audio</h5>
          </div>
          <label>
            Master Volume ({Math.round(props.settings.masterVolume * 100)}%)
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={Math.round(props.settings.masterVolume * 100)}
              onChange={(event) =>
                set("masterVolume", clamp(Number(event.target.value) / 100, 0, 1))
              }
            />
          </label>
          <label>
            UI Feedback Volume ({Math.round(props.settings.uiVolume * 100)}%)
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={Math.round(props.settings.uiVolume * 100)}
              onChange={(event) => set("uiVolume", clamp(Number(event.target.value) / 100, 0, 1))}
            />
          </label>
          <label>
            Gameplay Cue Volume ({Math.round(props.settings.gameplayVolume * 100)}%)
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={Math.round(props.settings.gameplayVolume * 100)}
              onChange={(event) =>
                set("gameplayVolume", clamp(Number(event.target.value) / 100, 0, 1))
              }
            />
          </label>
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <h5>Operations</h5>
          </div>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={props.settings.quietAlerts}
              onChange={(event) => set("quietAlerts", event.target.checked)}
            />
            Quiet non-critical alerts
          </label>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={props.settings.showDiagnostics}
              onChange={(event) => set("showDiagnostics", event.target.checked)}
            />
            Show diagnostics overlay
          </label>
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <h5>Shortcuts</h5>
          </div>
          <div className="settings-shortcuts">
            <span>Space</span>
            <span>Start/Pause</span>
            <span>1 / 2 / 3</span>
            <span>Set speed (1x / 2x / 4x)</span>
            <span>B / V</span>
            <span>Build mode / View mode</span>
            <span>Ctrl/Cmd+S</span>
            <span>Quick save</span>
            <span>Ctrl/Cmd+K</span>
            <span>Command palette</span>
            <span>Esc</span>
            <span>Close active panel</span>
          </div>
        </section>

        <div className="settings-actions">
          <button onClick={props.onReset}>Reset Defaults</button>
          <button className="primary" onClick={props.onClose}>
            Done
          </button>
        </div>
      </aside>
    </div>
  );
}
