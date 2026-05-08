import { useMemo, useState } from "react";
import type { RegionStatus } from "../types";
import { buildRegionDisplayNames } from "../map/data/regionPresentation";

function fmtCompact(value: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function fmtMoney(value: number | null): string {
  if (value === null) return "-";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function fmtPopulation(value: number | null | undefined): string {
  if (!Number.isFinite(value) || (value ?? 0) <= 0) return "-";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value ?? 0);
}

function fmtJobs(value: number | null | undefined): string {
  if (!Number.isFinite(value) || (value ?? 0) <= 0) return "-";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value ?? 0);
}

const MAX_UNLOCKED_ROWS = 18;
const MAX_CANDIDATE_ROWS = 24;

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

export type CountryInfoContentProps = {
  busy: boolean;
  regions: RegionStatus[];
  selectedRegionId: string | null;
  currentBalanceBase: number | null;
  onClose?: () => void;
  onSelectRegion: (regionId: string) => void;
  onUnlockRegion: () => void;
  compact?: boolean;
};

export function CountryInfoContent(props: CountryInfoContentProps) {
  const [otherLockedQuery, setOtherLockedQuery] = useState("");
  const [otherLockedFilterStub, setOtherLockedFilterStub] = useState("all");

  const displayNames = useMemo(() => buildRegionDisplayNames(props.regions), [props.regions]);
  const buckets = useMemo(() => {
    const unlockedSet = new Set(
      props.regions.filter((region) => region.unlocked).map((region) => region.region_id)
    );
    const unlockedRows = props.regions.filter((row) => row.unlocked);
    const adjacentLockedRows = props.regions.filter(
      (row) => !row.unlocked && row.adjacent_region_ids.some((id) => unlockedSet.has(id))
    );
    const otherLockedRows = props.regions.filter(
      (row) => !row.unlocked && !row.adjacent_region_ids.some((id) => unlockedSet.has(id))
    );
    return {
      unlockedSet,
      unlockedRows,
      adjacentLockedRows,
      otherLockedRows,
    };
  }, [props.regions]);

  const regionName = (region: RegionStatus | null): string => {
    if (!region) return "";
    return displayNames.get(region.region_id) ?? region.name;
  };
  const selected = props.regions.find((r) => r.region_id === props.selectedRegionId) ?? null;
  const countryIso2 = selected?.country_iso2 ?? props.regions[0]?.country_iso2 ?? null;
  const otherLockedRowsFiltered = useMemo(() => {
    const query = otherLockedQuery.trim().toLocaleLowerCase();
    if (!query) return buckets.otherLockedRows;
    return buckets.otherLockedRows.filter((row) =>
      (displayNames.get(row.region_id) ?? row.name).toLocaleLowerCase().includes(query)
    );
  }, [buckets.otherLockedRows, displayNames, otherLockedQuery]);

  const isUnlocked = Boolean(selected?.unlocked);
  const isAdjacent = Boolean(
    selected && selected.adjacent_region_ids.some((regionId) => buckets.unlockedSet.has(regionId))
  );
  const hasFunds = Boolean(
    selected && props.currentBalanceBase !== null && props.currentBalanceBase >= selected.unlock_cost_base
  );
  const canUnlock = Boolean(selected && !isUnlocked && isAdjacent && hasFunds);
  const unlockReason = selected && !isUnlocked
    ? !isAdjacent
      ? "Not adjacent yet. Unlock a neighboring region first."
      : !hasFunds
        ? "Available next, but current balance is below unlock cost."
        : null
    : null;
  const selectedHexNumber = selected ? parseHexNumberFromName(selected.name) : null;
  const selectedHexId = parseHexIdFromRegion(selected);
  const isUnassignedHexRegion = Boolean(
    selected && (selected.source_code ?? "").trim().toLowerCase() === "manual_region_unassigned_hex"
  );
  const selectedStatus = selected
    ? selected.unlocked
      ? "Unlocked"
      : isAdjacent
        ? "Available to Unlock"
        : "Locked - Not Adjacent"
    : null;
  const selectedStateLine = selected
    ? selected.unlocked
      ? "This region is already in your active planning scope."
      : !isAdjacent
        ? "Expand through adjacent regions to unlock this area."
        : !hasFunds
          ? "This is a viable next expansion once funds are available."
          : "This region can be unlocked now."
    : null;

  return (
    <div className={`country-info-content ${props.compact ? "is-compact" : ""}`.trim()}>
        <div className="country-info-head">
          <div>
            <p>{countryNameFromIso2(countryIso2)}</p>
            <h4>Region Progression</h4>
          </div>
          {props.onClose ? <button onClick={props.onClose}>Close</button> : null}
        </div>

        <div className="country-info-strip">
          <span className="pill">Balance: {fmtMoney(props.currentBalanceBase)}</span>
          <span className="pill">Unlocked: {fmtCompact(buckets.unlockedRows.length)}</span>
        </div>

        {selected ? (
          <section
            className={`country-section selected-county-card${selected.unlocked ? " is-unlocked" : " is-locked"}`}
          >
            <div className="selected-county-head">
              <div>
                <strong>{regionName(selected)}</strong>
                <p>{selectedStatus}</p>
              </div>
              {selected.nation ? <span className="status active region-status-tag">{selected.nation}</span> : null}
            </div>
            <div className="selected-county-grid">
              <p>Population: {fmtPopulation(selected.population)}</p>
              <p>Jobs: {fmtJobs(selected.jobs)}</p>
              {!selected.unlocked ? <p>Unlock cost: {fmtCompact(selected.unlock_cost_base)}</p> : null}
              {isUnassignedHexRegion ? (
                <>
                  <p>Hex #: {selectedHexNumber !== null ? fmtCompact(selectedHexNumber) : "-"}</p>
                  <p>Hex ID: {selectedHexId ?? "-"}</p>
                </>
              ) : null}
            </div>
            {selectedStateLine ? <p className="hint-line">{selectedStateLine}</p> : null}
            {!selected.unlocked ? (
              <div className="form-actions">
                <button onClick={props.onUnlockRegion} disabled={props.busy || !canUnlock}>
                  Unlock for {fmtCompact(selected.unlock_cost_base)}
                </button>
                {unlockReason ? <p className="hint-line">{unlockReason}</p> : null}
              </div>
            ) : null}
          </section>
        ) : (
          <section className="country-section">
            <p className="hint-line">Select a region on the map or from the list below.</p>
          </section>
        )}

        <div className="country-sections-grid">
          <section className="country-section country-section-unlocked">
            <div className="country-section-intro">
              <div className="country-section-head">
                <h5>Unlocked</h5>
                <span>{buckets.unlockedRows.length}</span>
              </div>
              <p className="hint-line">Current strategic footprint.</p>
            </div>
            <div className="county-list">
              {buckets.unlockedRows.slice(0, MAX_UNLOCKED_ROWS).map((region) => {
                const active = region.region_id === props.selectedRegionId;
                return (
                  <button
                    key={region.region_id}
                    className={`county-row${active ? " active" : ""} is-unlocked`}
                    onClick={() => props.onSelectRegion(region.region_id)}
                  >
                    <span>{regionName(region)}</span>
                    <small>unlocked</small>
                  </button>
                );
              })}
              {buckets.unlockedRows.length > MAX_UNLOCKED_ROWS ? (
                <p className="hint-line">
                  Showing first {MAX_UNLOCKED_ROWS}. +{buckets.unlockedRows.length - MAX_UNLOCKED_ROWS} more
                  unlocked regions.
                </p>
              ) : null}
            </div>
          </section>

          <section className="country-section country-section-candidates">
            <div className="country-section-intro">
              <div className="country-section-head">
                <h5>Next Unlock Candidates</h5>
                <span>{buckets.adjacentLockedRows.length}</span>
              </div>
              <p className="hint-line">Strongest adjacent options to expand next.</p>
            </div>
            <div className="county-list">
              {buckets.adjacentLockedRows.slice(0, MAX_CANDIDATE_ROWS).map((region) => {
                const active = region.region_id === props.selectedRegionId;
                return (
                  <button
                    key={region.region_id}
                    className={`county-row${active ? " active" : ""} is-candidate`}
                    onClick={() => props.onSelectRegion(region.region_id)}
                  >
                    <span>{regionName(region)}</span>
                    <small>unlock {fmtCompact(region.unlock_cost_base)}</small>
                  </button>
                );
              })}
              {buckets.adjacentLockedRows.length > MAX_CANDIDATE_ROWS ? (
                <p className="hint-line">
                  Showing first {MAX_CANDIDATE_ROWS}. +{buckets.adjacentLockedRows.length - MAX_CANDIDATE_ROWS} more
                  candidates.
                </p>
              ) : null}
            </div>
          </section>

          <section className="country-section country-section-other-locked">
            <div className="country-section-intro">
              <div className="country-section-head">
                <h5>Other Locked</h5>
                <span>{otherLockedRowsFiltered.length}</span>
              </div>
              <p className="hint-line">Search the remaining locked regions.</p>
            </div>
            <div className="country-list-controls">
              <input
                type="search"
                value={otherLockedQuery}
                placeholder="Search locked regions"
                onChange={(event) => setOtherLockedQuery(event.target.value)}
              />
              <select
                aria-label="Locked region filter placeholder"
                value={otherLockedFilterStub}
                onChange={(event) => setOtherLockedFilterStub(event.target.value)}
              >
                <option value="all">Filter (placeholder)</option>
                <option value="soon">More filters coming soon</option>
              </select>
            </div>
            <div className="country-list-viewport">
              <div className="county-list county-list-scroll" data-allow-surface-scroll="true">
                {otherLockedRowsFiltered.map((region) => {
                  const active = region.region_id === props.selectedRegionId;
                  return (
                    <button
                      key={region.region_id}
                      className={`county-row${active ? " active" : ""} is-locked`}
                      onClick={() => props.onSelectRegion(region.region_id)}
                    >
                      <span>{regionName(region)}</span>
                      <small>locked</small>
                    </button>
                  );
                })}
                {otherLockedRowsFiltered.length === 0 ? (
                  <p className="hint-line">No locked regions match this search.</p>
                ) : null}
              </div>
            </div>
          </section>
        </div>
    </div>
  );
}

export default function CountryInfoDrawer(props: CountryInfoContentProps & {
  open: boolean;
  onClose: () => void;
}) {
  if (!props.open) return null;

  return (
    <div className="country-info-overlay">
      <aside className="country-info-drawer">
        <CountryInfoContent {...props} />
      </aside>
    </div>
  );
}
