type PaletteCommand = {
  id: string;
  label: string;
  detail?: string;
  shortcut?: string;
  disabled?: boolean;
};

export default function CommandPalette(props: {
  open: boolean;
  query: string;
  commands: PaletteCommand[];
  onQueryChange: (value: string) => void;
  onRun: (id: string) => void;
  onClose: () => void;
}) {
  if (!props.open) return null;
  const query = props.query.trim().toLowerCase();
  const filtered = props.commands.filter((command) => {
    if (!query) return true;
    const labelHit = command.label.toLowerCase().includes(query);
    const detailHit = command.detail?.toLowerCase().includes(query) ?? false;
    const keyHit = command.shortcut?.toLowerCase().includes(query) ?? false;
    return labelHit || detailHit || keyHit;
  });

  return (
    <div className="command-palette-overlay" onClick={props.onClose}>
      <aside className="command-palette" onClick={(event) => event.stopPropagation()}>
        <div className="command-palette-head">
          <p>Command Palette</p>
          <button onClick={props.onClose}>Close</button>
        </div>
        <input
          autoFocus
          placeholder="Search commands..."
          value={props.query}
          onChange={(event) => props.onQueryChange(event.target.value)}
        />
        <div className="command-palette-list">
          {filtered.length ? (
            filtered.map((command) => (
              <button
                key={command.id}
                className="command-palette-row"
                disabled={command.disabled}
                onClick={() => props.onRun(command.id)}
              >
                <span>
                  <strong>{command.label}</strong>
                  {command.detail ? <small>{command.detail}</small> : null}
                </span>
                {command.shortcut ? <kbd>{command.shortcut}</kbd> : null}
              </button>
            ))
          ) : (
            <p className="hint-line">No matching commands.</p>
          )}
        </div>
      </aside>
    </div>
  );
}
