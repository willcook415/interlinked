import type { RegionStatus } from "../types";
import { buildRegionDisplayNames } from "../map/data/regionPresentation";

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

const MAX_ROWS_PER_GROUP = 36;

function countryNameFromIso2(iso2: string | null): string {
  if (!iso2) return "Country";
  const normalized = iso2.trim().toUpperCase();
  if (normalized === "UK" || normalized === "GB") return "United Kingdom";
  return normalized;
}

function parseHexIdFromRegion(region: RegionStatus | null): string | null {
  if (!region) return null;
  const explicit = region.h3_cell_id?.trim().toLowerCase();
  if (explicit) return explicit;
  const parts = region.region_id.trim().split(":");
  if (parts.length >= 3 && parts[0].toLowerCase() === "r6") {
    const token = parts[2].trim().toLowerCase();
    if (/^[0-9a-f]{10,}$/i.test(token)) return token;
  }
  return null;
}

function parseHexNumberFromName(name: string): number | null {
  const match = /^hex\s+#(\d+)$/i.exec(name.trim());
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isFinite(value) ? Math.trunc(value) : null;
}

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
  const countryIso2 =
    selected?.country_iso2 ?? focused?.country_iso2 ?? props.regions[0]?.country_iso2 ?? null;
  const unlockedSet = new Set(props.regions.filter((r) => r.unlocked).map((r) => r.region_id));
  const displayNames = buildRegionDisplayNames(props.regions);
  const regionCountText =
    props.regions.length > 999 ? "999+" : fmtCompact(props.regions.length);
  const regionName = (region: RegionStatus | null): string => {
    if (!region) return "";
    return displayNames.get(region.region_id) ?? region.name;
  };
  const isFocused = Boolean(selected && selected.region_id === props.focusRegionId);
  const isUnlocked = Boolean(selected?.unlocked);
  const selectedHexNumber = selected ? parseHexNumberFromName(selected.name) : null;
  const selectedHexId = parseHexIdFromRegion(selected);
  const isUnassignedHexRegion = Boolean(
    selected && (selected.source_code ?? "").trim().toLowerCase() === "manual_region_unassigned_hex"
  );
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
    if (!isAdjacent) unlockReason = "Unlock a neighboring region first.";
    else if (!hasFunds) unlockReason = "Insufficient funds for this unlock.";
    else unlockReason = "Adjacent region available to unlock.";
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
            <p>{countryNameFromIso2(countryIso2)}</p>
            <h4>Country Info</h4>
          </div>
          <button onClick={props.onClose}>Close</button>
        </div>

        <div className="country-info-strip">
          <span className="pill">Balance: {fmtMoney(props.currentBalanceBase)}</span>
          <span className="pill">Planning Regions: {regionCountText}</span>
          {focused ? <span className="pill">Focus: {regionName(focused)}</span> : null}
        </div>

        {selected ? (
          <section className="country-section selected-county-card">
            <div className="selected-county-head">
              <div>
                <strong>{regionName(selected)}</strong>
                <p>{isFocused ? "Focused region" : selected.unlocked ? "Unlocked region" : "Locked region"}</p>
              </div>
              {selected.nation ? <span className="status active">{selected.nation}</span> : null}
            </div>
            <div className="selected-county-grid">
              <p>Cells: {fmtCompact(selected.cells_res8)}</p>
              <p>Unlock cost: {fmtCompact(selected.unlock_cost_base)}</p>
              {isUnassignedHexRegion ? (
                <>
                  <p>Hex #: {selectedHexNumber !== null ? fmtCompact(selectedHexNumber) : "-"}</p>
                  <p>Hex ID: {selectedHexId ?? "-"}</p>
                </>
              ) : null}
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
            <p className="hint-line">Select a region on the map or from the list below.</p>
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
                {group.rows.slice(0, MAX_ROWS_PER_GROUP).map((region) => {
                  const active = region.region_id === props.selectedRegionId;
                  return (
                    <button
                      key={region.region_id}
                      className={`county-row${active ? " active" : ""}`}
                      onClick={() => props.onSelectRegion(region.region_id)}
                    >
                      <span>{regionName(region)}</span>
                      <small>{region.unlocked ? "unlocked" : "locked"}</small>
                    </button>
                  );
                })}
                {group.rows.length > MAX_ROWS_PER_GROUP ? (
                  <p className="hint-line">
                    Showing first {MAX_ROWS_PER_GROUP}. +{group.rows.length - MAX_ROWS_PER_GROUP} more
                    internal planning regions.
                  </p>
                ) : null}
              </div>
            </section>
          ))}
        </div>
      </aside>
    </div>
  );
}
