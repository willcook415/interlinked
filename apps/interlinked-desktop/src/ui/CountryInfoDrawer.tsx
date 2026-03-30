import type { RegionStatus } from "../types";

function fmtCompact(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function fmtMoney(value: number | null): string {
  if (value === null) return "-";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

type Group = {
  title: string;
  rows: RegionStatus[];
};

export default function CountryInfoDrawer(props: {
  open: boolean;
  busy: boolean;
  regions: RegionStatus[];
  selectedRegionId: string | null;
  focusRegionId: string | null;
  currentBalanceBase: number | null;
  onClose: () => void;
  onSelectRegion: (regionId: string) => void;
  onFocusRegion: () => void;
  onUnlockRegion: () => void;
}) {
  if (!props.open) return null;

  const selected = props.regions.find((r) => r.region_id === props.selectedRegionId) ?? null;
  const focused = props.regions.find((r) => r.region_id === props.focusRegionId) ?? null;
  const unlockedSet = new Set(props.regions.filter((r) => r.unlocked).map((r) => r.region_id));
  const isFocused = Boolean(selected && selected.region_id === props.focusRegionId);
  const isUnlocked = Boolean(selected?.unlocked);
  const isAdjacent = Boolean(
    selected && selected.adjacent_region_ids.some((regionId) => unlockedSet.has(regionId))
  );
  const hasFunds = Boolean(
    selected && props.currentBalanceBase !== null && props.currentBalanceBase >= selected.unlock_cost_base
  );
  const canFocus = Boolean(selected && isUnlocked && !isFocused);
  const canUnlock = Boolean(selected && !isUnlocked && isAdjacent && hasFunds);

  let unlockReason = "";
  if (selected && !selected.unlocked) {
    if (!isAdjacent) unlockReason = "Unlock a neighboring county first.";
    else if (!hasFunds) unlockReason = "Insufficient funds for this unlock.";
    else unlockReason = "Adjacent county available to unlock.";
  }

  const groups: Group[] = [
    {
      title: "Focused",
      rows: props.regions.filter((r) => r.region_id === props.focusRegionId),
    },
    {
      title: "Unlocked",
      rows: props.regions.filter((r) => r.unlocked && r.region_id !== props.focusRegionId),
    },
    {
      title: "Adjacent Locked",
      rows: props.regions.filter(
        (r) => !r.unlocked && r.adjacent_region_ids.some((regionId) => unlockedSet.has(regionId))
      ),
    },
    {
      title: "Other Locked",
      rows: props.regions.filter(
        (r) => !r.unlocked && !r.adjacent_region_ids.some((regionId) => unlockedSet.has(regionId))
      ),
    },
  ].filter((group) => group.rows.length > 0);

  return (
    <div className="country-info-overlay">
      <aside className="country-info-drawer">
        <div className="country-info-head">
          <div>
            <p>Great Britain</p>
            <h4>Country Info</h4>
          </div>
          <button onClick={props.onClose}>Close</button>
        </div>

        <div className="country-info-strip">
          <span className="pill">Balance: {fmtMoney(props.currentBalanceBase)}</span>
          <span className="pill">Counties: {props.regions.length}</span>
          {focused ? <span className="pill">Focus: {focused.name}</span> : null}
        </div>

        {selected ? (
          <section className="country-section selected-county-card">
            <div className="selected-county-head">
              <div>
                <strong>{selected.name}</strong>
                <p>{isFocused ? "Focused county" : selected.unlocked ? "Unlocked county" : "Locked county"}</p>
              </div>
              {selected.nation ? <span className="status active">{selected.nation}</span> : null}
            </div>
            <div className="selected-county-grid">
              <p>Cells: {fmtCompact(selected.cells_res8)}</p>
              <p>Unlock cost: {fmtCompact(selected.unlock_cost_base)}</p>
            </div>
            {!selected.unlocked && unlockReason ? <p className="hint-line">{unlockReason}</p> : null}
            <div className="form-actions">
              <button onClick={props.onFocusRegion} disabled={props.busy || !canFocus}>
                Focus
              </button>
              <button onClick={props.onUnlockRegion} disabled={props.busy || !canUnlock}>
                Unlock + Focus
              </button>
            </div>
          </section>
        ) : (
          <section className="country-section">
            <p className="hint-line">Select a county on the map or from the list below.</p>
          </section>
        )}

        <div className="country-sections-grid">
          {groups.map((group) => (
            <section key={group.title} className="country-section">
              <div className="country-section-head">
                <h5>{group.title}</h5>
                <span>{group.rows.length}</span>
              </div>
              <div className="county-list">
                {group.rows.map((region) => {
                  const active = region.region_id === props.selectedRegionId;
                  return (
                    <button
                      key={region.region_id}
                      className={`county-row${active ? " active" : ""}`}
                      onClick={() => props.onSelectRegion(region.region_id)}
                    >
                      <span>{region.name}</span>
                      <small>{region.unlocked ? "unlocked" : "locked"}</small>
                    </button>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
      </aside>
    </div>
  );
}
