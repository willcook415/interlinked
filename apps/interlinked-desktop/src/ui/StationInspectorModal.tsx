import { useEffect, useState } from "react";
import type { StationInspection, StationLineSummary, StopLite } from "../types";

type StationInspectorTab = "overview" | "connected_lines" | "demand_usage" | "upgrades" | "performance";

function formatSeconds(value: number): string {
  if (value >= 3600) return `${(value / 3600).toFixed(1)}h`;
  if (value >= 60) return `${Math.round(value / 60)} min`;
  return `${Math.round(value)}s`;
}

function formatCapacityValue(value: number): string {
  const safe = Number.isFinite(value) ? Math.max(value, 0) : 0;
  if (safe >= 1000) return Math.round(safe).toLocaleString();
  if (safe >= 100) return safe.toFixed(0);
  if (safe >= 10) return safe.toFixed(1);
  if (safe >= 1) return safe.toFixed(2);
  if (safe > 0) return safe.toFixed(3);
  return safe.toFixed(2);
}

function formatDistanceMeters(value: number): string {
  const safe = Number.isFinite(value) ? Math.max(value, 0) : 0;
  if (safe >= 1000) return `${(safe / 1000).toFixed(2)} km`;
  return `${Math.round(safe)} m`;
}

function transferDirectionLabel(direction: "to" | "from" | "both"): string {
  if (direction === "both") return "Bidirectional";
  if (direction === "from") return "Inbound";
  return "Outbound";
}

function humanizeStopType(value: string | null | undefined): string {
  if (!value?.trim()) return "Station";
  const normalized = value
    .trim()
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .toLowerCase();
  return normalized
    .split(" ")
    .map((token) => token.charAt(0).toUpperCase() + token.slice(1))
    .join(" ");
}

export default function StationInspectorModal(props: {
  open: boolean;
  stop: StopLite | null;
  inspection: StationInspection | null;
  localLines: StationLineSummary[];
  interchangeMembers: Array<{ stopId: string; name: string; distanceM: number }>;
  suggestedInterchanges: Array<{ interchangeId: string; memberCount: number; nearestDistanceM: number }>;
  transferLinks: Array<{
    stopId: string;
    name: string;
    distanceM: number;
    transferTimeS: number;
    penaltyS: number;
    direction: "to" | "from" | "both";
  }>;
  editable?: boolean;
  onClose: () => void;
  onNameChange: (value: string) => void;
  onInterchangeChange: (value: string) => void;
  onCreateInterchangeGroup: () => void;
  onClearInterchangeGroup: () => void;
  onApplySuggestedInterchange: (interchangeId: string) => void;
  onSelectLinkedStop: (stopId: string) => void;
  onDelete: () => void;
}) {
  if (!props.open || !props.stop) return null;

  const canEdit = props.editable ?? true;
  const stopName = props.stop.name ?? "";
  const stopNameDisplay = stopName.trim() ? stopName : props.stop.id;
  const lines = canEdit
    ? props.localLines
    : props.inspection?.served_lines.length
      ? props.inspection.served_lines
      : props.localLines;
  const queueEnd = Math.max(props.inspection?.queue_end ?? 0, 0);
  const rawCurrentLoad = Math.max(props.inspection?.station_load_current_pax ?? queueEnd, 0);
  const queueCapPax = props.inspection?.station_queue_capacity_pax ?? 0;
  const queueCapSafe = Math.max(queueCapPax, 0);
  const currentLoad = queueCapSafe > 0 ? Math.min(rawCurrentLoad, queueCapSafe) : rawCurrentLoad;
  const queueUsage = queueCapSafe > 0 ? currentLoad / queueCapSafe : 0;
  const queueUsagePct = Math.max(0, queueUsage * 100);
  const queueAtCapacity = queueCapSafe > 0 && queueUsage >= 1.0;
  const queueNearCapacity = queueCapSafe > 0 && queueUsage >= 0.9;
  const passengersDeclinedLastHour = Math.max(props.inspection?.passengers_declined_last_hour ?? 0, 0);
  const entriesPerHour = Math.max(props.inspection?.station_entries_per_hour ?? 0, 0);
  const exitsPerHour = Math.max(props.inspection?.station_exits_per_hour ?? 0, 0);
  const averageWaitToBoardS = Math.max(props.inspection?.average_wait_to_board_s ?? 0, 0);
  const selectedInterchangeId = props.stop.interchange_id?.trim() ?? "";
  const [activeTab, setActiveTab] = useState<StationInspectorTab>("overview");

  useEffect(() => {
    setActiveTab("overview");
  }, [props.stop.id]);

  return (
    <aside className="station-inspector-sheet">
      <div className="station-modal-head">
        <div>
          <p>Station Inspector</p>
          <h4>{stopNameDisplay}</h4>
        </div>
        <button onClick={props.onClose}>Close</button>
      </div>

      <div className="inspector-tab-row" role="tablist" aria-label="Station inspector sections">
        <button className={activeTab === "overview" ? "active" : ""} onClick={() => setActiveTab("overview")}>
          Overview
        </button>
        <button
          className={activeTab === "connected_lines" ? "active" : ""}
          onClick={() => setActiveTab("connected_lines")}
        >
          Lines
        </button>
        <button className={activeTab === "demand_usage" ? "active" : ""} onClick={() => setActiveTab("demand_usage")}>
          Demand
        </button>
        <button className={activeTab === "upgrades" ? "active" : ""} onClick={() => setActiveTab("upgrades")}>
          Upgrades
        </button>
        <button className={activeTab === "performance" ? "active" : ""} onClick={() => setActiveTab("performance")}>
          Performance
        </button>
      </div>

      {activeTab === "overview" ? (
        <section className="inspector-section">
          {canEdit ? (
            <div className="station-modal-grid">
              <label>
                Station Name
                <input
                  value={stopName}
                  placeholder={props.stop.id}
                  onChange={(event) => props.onNameChange(event.target.value)}
                />
              </label>
              <label>
                Interchange Group
                <input
                  placeholder="Optional interchange id"
                  value={props.stop.interchange_id ?? ""}
                  onChange={(event) => props.onInterchangeChange(event.target.value)}
                />
              </label>
            </div>
          ) : (
            <div className="inspector-read-grid">
              <div className="inspector-read-field">
                <small>Station Name</small>
                <strong>{stopNameDisplay}</strong>
              </div>
              <div className="inspector-read-field">
                <small>Interchange Group</small>
                <strong>{props.stop.interchange_id?.trim() ? props.stop.interchange_id : "-"}</strong>
              </div>
            </div>
          )}

          <div
            className={`station-capacity-hero ${
              queueAtCapacity ? "is-critical" : queueNearCapacity ? "is-warning" : ""
            }`}
          >
            <small>Station Capacity</small>
            <strong>
              {formatCapacityValue(currentLoad)} / {formatCapacityValue(queueCapSafe)} pax
            </strong>
            <span>
              {queueCapSafe > 0 ? `${queueUsagePct.toFixed(0)}% used` : "No capacity limit set"} |{" "}
              {humanizeStopType(props.stop.stop_type)}
            </span>
            {queueNearCapacity ? (
              <p className="station-capacity-alert">
                {queueAtCapacity
                  ? "Warning: station is at capacity. New entries are being declined."
                  : "Warning: station is nearing capacity."}
              </p>
            ) : null}
          </div>
        </section>
      ) : null}

      {activeTab === "connected_lines" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Served Lines</h5>
            <span>{lines.length}</span>
          </div>
          {lines.length === 0 ? (
            <p className="hint-line">This station is not served by a line yet.</p>
          ) : (
            <div className="station-line-list">
              {lines.map((line) => (
                <div key={line.line_id} className="station-line-card">
                  <div className="station-line-head">
                    <div className="station-line-title">
                      <span
                        className="station-line-swatch"
                        style={{ backgroundColor: line.display_color ?? "#1f3e63" }}
                      />
                      <strong>{line.line_name}</strong>
                    </div>
                    <span className="station-line-position">
                      Stop {line.station_index + 1}/{line.station_count}
                    </span>
                  </div>
                  <p>{line.mode_variant ? `${line.mode} / ${line.mode_variant}` : line.mode}</p>
                  <div className="station-line-neighbours">
                    <span>{line.previous_station_name ?? "Origin"}</span>
                    <span>{">"}</span>
                    <span>{line.next_station_name ?? "Terminus"}</span>
                  </div>
                  {line.journey_times.length ? (
                    <div className="journey-chip-list">
                      {line.journey_times.slice(0, 3).map((journey) => (
                        <div key={journey.stop_id} className="journey-chip">
                          <span>{journey.stop_name}</span>
                          <strong>
                            {formatSeconds(journey.travel_time_s)} | {journey.stops_away} stops
                          </strong>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="hint-line">Journey times appear after the line has enough geometry to analyse.</p>
                  )}
                </div>
              ))}
            </div>
          )}
          <div className="inspector-section-head">
            <h5>Transfer Links</h5>
            <span>{props.transferLinks.length}</span>
          </div>
          {props.transferLinks.length === 0 ? (
            <p className="hint-line">No out-of-station transfers are configured for this station.</p>
          ) : (
            <div className="transfer-link-list">
              {props.transferLinks.map((link) => (
                <button
                  key={link.stopId}
                  className="transfer-link-row"
                  onClick={() => props.onSelectLinkedStop(link.stopId)}
                >
                  <div>
                    <strong>{link.name}</strong>
                    <small>
                      {transferDirectionLabel(link.direction)} | {formatDistanceMeters(link.distanceM)}
                    </small>
                  </div>
                  <span>
                    {formatSeconds(link.transferTimeS)} ({Math.max(Math.round(link.penaltyS), 0)}s penalty)
                  </span>
                </button>
              ))}
            </div>
          )}
        </section>
      ) : null}

      {activeTab === "demand_usage" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Passenger Flow</h5>
            <span>Last Hour</span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Passengers Declined</small>
              <strong>{Math.round(passengersDeclinedLastHour).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>In Station</small>
              <strong>
                {queueCapSafe > 0
                  ? `${Math.round(currentLoad).toLocaleString()} / ${Math.round(queueCapSafe).toLocaleString()}`
                  : Math.round(currentLoad).toLocaleString()}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Entry / Exit per Hour</small>
              <strong>
                {Math.round(entriesPerHour).toLocaleString()} / {Math.round(exitsPerHour).toLocaleString()}
              </strong>
            </div>
            <div className="inspector-stat">
              <small>Avg Wait To Board</small>
              <strong>{formatSeconds(averageWaitToBoardS)}</strong>
            </div>
          </div>
          {!props.inspection ? (
            <p className="hint-line">Demand and usage metrics appear after runtime inspection data is available.</p>
          ) : null}
        </section>
      ) : null}

      {activeTab === "upgrades" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Interchange Management</h5>
            <span>{selectedInterchangeId ? selectedInterchangeId : "No group"}</span>
          </div>
          {canEdit ? (
            <div className="interchange-toolbar">
              <button onClick={props.onCreateInterchangeGroup}>Create Group</button>
              <button onClick={props.onClearInterchangeGroup} disabled={!selectedInterchangeId}>
                Clear Group
              </button>
            </div>
          ) : null}
          {props.interchangeMembers.length > 0 ? (
            <div className="interchange-member-list">
              {props.interchangeMembers.map((member) => (
                <button
                  key={member.stopId}
                  className="interchange-member-row"
                  onClick={() => props.onSelectLinkedStop(member.stopId)}
                >
                  <strong>{member.name}</strong>
                  <span>{formatDistanceMeters(member.distanceM)}</span>
                </button>
              ))}
            </div>
          ) : (
            <p className="hint-line">
              {selectedInterchangeId
                ? "No connected interchange members yet."
                : "Assign this station to an interchange group to auto-create transfer links."}
            </p>
          )}
          {canEdit && props.suggestedInterchanges.length > 0 ? (
            <div className="interchange-suggestion-list">
              <p className="hint-line">Nearby groups</p>
              {props.suggestedInterchanges.map((suggestion) => (
                <button
                  key={suggestion.interchangeId}
                  className="interchange-suggestion-row"
                  onClick={() => props.onApplySuggestedInterchange(suggestion.interchangeId)}
                >
                  <strong>{suggestion.interchangeId}</strong>
                  <span>
                    {suggestion.memberCount.toLocaleString()} stops | nearest{" "}
                    {formatDistanceMeters(suggestion.nearestDistanceM)}
                  </span>
                </button>
              ))}
            </div>
          ) : null}
          <p className="hint-line">
            Facilities and station upgrades hooks are reserved here for future simulation layers.
          </p>
        </section>
      ) : null}

      {activeTab === "performance" ? (
        <section className="inspector-section">
          <div className="inspector-section-head">
            <h5>Performance</h5>
            <span>{humanizeStopType(props.stop.stop_type)}</span>
          </div>
          <div className="inspector-stat-row">
            <div className="inspector-stat">
              <small>Queue End</small>
              <strong>{Math.round(queueEnd).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Queue Capacity</small>
              <strong>{Math.round(queueCapSafe).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Utilization</small>
              <strong>{queueCapSafe > 0 ? `${queueUsagePct.toFixed(0)}%` : "-"}</strong>
            </div>
            <div className="inspector-stat">
              <small>Entry / Hr</small>
              <strong>{Math.round(entriesPerHour).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Exit / Hr</small>
              <strong>{Math.round(exitsPerHour).toLocaleString()}</strong>
            </div>
            <div className="inspector-stat">
              <small>Declined / Hr</small>
              <strong>{Math.round(passengersDeclinedLastHour).toLocaleString()}</strong>
            </div>
          </div>
        </section>
      ) : null}

      {canEdit ? (
        <div className="inspector-actions">
          <button className="danger-button" onClick={props.onDelete}>
            Delete Station
          </button>
        </div>
      ) : null}
    </aside>
  );
}
