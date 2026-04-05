use serde::Serialize;
use std::time::Instant;

use crate::{
    AppState, RuntimeFastSnapshot, RuntimeSnapshot, RuntimeStrategicSnapshot,
    SimulationAdvanceEconomy,
};

pub(crate) fn runtime_fast_snapshot_from_combined(
    snapshot: &RuntimeSnapshot,
) -> RuntimeFastSnapshot {
    RuntimeFastSnapshot {
        project_path: snapshot.project_path.clone(),
        clock_revision: snapshot.clock_revision,
        clock: snapshot.clock.clone(),
        captured_at_epoch_ms: snapshot.captured_at_epoch_ms,
        telemetry: snapshot.telemetry.clone(),
        trains: snapshot.trains.clone(),
        stations: snapshot.stations.clone(),
        line_ops: snapshot.line_ops.clone(),
        provenance_warnings: snapshot.provenance_warnings.clone(),
        trains_authoritative: snapshot.trains_authoritative,
    }
}

pub(crate) fn runtime_strategic_snapshot_from_combined(
    snapshot: &RuntimeSnapshot,
) -> RuntimeStrategicSnapshot {
    RuntimeStrategicSnapshot {
        project_path: snapshot.project_path.clone(),
        clock_revision: snapshot.clock_revision,
        clock: snapshot.clock.clone(),
        economy: snapshot.economy.clone(),
        frame: snapshot.frame.clone(),
        delta_revenue_base: snapshot.delta_revenue_base,
        delta_opex_base: snapshot.delta_opex_base,
        delta_net_base: snapshot.delta_net_base,
        captured_at_epoch_ms: snapshot.captured_at_epoch_ms,
        telemetry: snapshot.telemetry.clone(),
        provenance_warnings: snapshot.provenance_warnings.clone(),
        trains_authoritative: snapshot.trains_authoritative,
    }
}

pub(crate) fn runtime_snapshot_from_parts(
    fast: &RuntimeFastSnapshot,
    strategic: Option<&RuntimeStrategicSnapshot>,
) -> RuntimeSnapshot {
    // Snapshot ownership contract:
    // - Fast snapshot owns clock + live runtime operational views (trains/stations/line ops/telemetry).
    // - Strategic snapshot owns heavyweight economy/frame deltas only when it is at least as fresh.
    // This prevents stale strategic payloads from mutating fast-owned state.
    if let Some(strategic) = strategic {
        let strategic_matches_fast = strategic.telemetry.tick_index >= fast.telemetry.tick_index;
        return RuntimeSnapshot {
            project_path: fast.project_path.clone(),
            clock_revision: fast.clock_revision.max(strategic.clock_revision),
            clock: fast.clock.clone(),
            economy: strategic.economy.clone(),
            frame: if strategic_matches_fast {
                strategic.frame.clone()
            } else {
                None
            },
            delta_revenue_base: if strategic_matches_fast {
                strategic.delta_revenue_base
            } else {
                0.0
            },
            delta_opex_base: if strategic_matches_fast {
                strategic.delta_opex_base
            } else {
                0.0
            },
            delta_net_base: if strategic_matches_fast {
                strategic.delta_net_base
            } else {
                0.0
            },
            captured_at_epoch_ms: fast
                .captured_at_epoch_ms
                .max(strategic.captured_at_epoch_ms),
            telemetry: fast.telemetry.clone(),
            trains: fast.trains.clone(),
            stations: fast.stations.clone(),
            line_ops: fast.line_ops.clone(),
            provenance_warnings: fast.provenance_warnings.clone(),
            trains_authoritative: fast.trains_authoritative,
        };
    }

    RuntimeSnapshot {
        project_path: fast.project_path.clone(),
        clock_revision: fast.clock_revision,
        clock: fast.clock.clone(),
        economy: SimulationAdvanceEconomy {
            current_balance_base: 0.0,
            cumulative_revenue_base: 0.0,
            cumulative_opex_base: 0.0,
            budget_display: 0.0,
        },
        frame: None,
        delta_revenue_base: 0.0,
        delta_opex_base: 0.0,
        delta_net_base: 0.0,
        captured_at_epoch_ms: fast.captured_at_epoch_ms,
        telemetry: fast.telemetry.clone(),
        trains: fast.trains.clone(),
        stations: fast.stations.clone(),
        line_ops: fast.line_ops.clone(),
        provenance_warnings: fast.provenance_warnings.clone(),
        trains_authoritative: fast.trains_authoritative,
    }
}

fn push_runtime_snapshot(
    state: &AppState,
    snapshot: RuntimeSnapshot,
    ring_capacity: usize,
) -> Result<(), String> {
    let mut guard = state
        .runtime_snapshots
        .lock()
        .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
    guard.push_back(snapshot);
    let cap = ring_capacity.clamp(4, 256);
    while guard.len() > cap {
        guard.pop_front();
    }
    Ok(())
}

fn runtime_snapshot_size_bytes<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

pub(crate) fn publish_strategic_snapshot_for_tick(snapshot: &RuntimeSnapshot) -> bool {
    // Strategic snapshots are published only when a strategic kernel refresh actually ran.
    // Cadence "due" hints can lag, so execution truth from engine telemetry is authoritative.
    snapshot.telemetry.engine_strategic_refresh_executed
}

pub(crate) fn publish_runtime_snapshots(
    state: &AppState,
    mut combined: RuntimeSnapshot,
    ring_capacity: usize,
    strategic_refresh_due: bool,
) -> Result<(), String> {
    let publish_start = Instant::now();
    let mut fast = runtime_fast_snapshot_from_combined(&combined);
    let mut strategic = if strategic_refresh_due {
        Some(runtime_strategic_snapshot_from_combined(&combined))
    } else {
        None
    };
    let fast_snapshot_bytes = runtime_snapshot_size_bytes(&fast);
    let strategic_snapshot_bytes = strategic
        .as_ref()
        .map(runtime_snapshot_size_bytes)
        .unwrap_or(0);
    fast.telemetry.fast_snapshot_bytes = fast_snapshot_bytes;
    fast.telemetry.strategic_snapshot_bytes = strategic_snapshot_bytes;
    if let Some(snapshot) = strategic.as_mut() {
        snapshot.telemetry.fast_snapshot_bytes = fast_snapshot_bytes;
        snapshot.telemetry.strategic_snapshot_bytes = strategic_snapshot_bytes;
    }
    combined = runtime_snapshot_from_parts(&fast, strategic.as_ref());
    let publish_ms = publish_start.elapsed().as_secs_f64() * 1000.0;
    fast.telemetry.snapshot_publish_ms = publish_ms;
    combined.telemetry.snapshot_publish_ms = publish_ms;
    if let Some(snapshot) = strategic.as_mut() {
        snapshot.telemetry.snapshot_publish_ms = publish_ms;
    }

    push_runtime_fast_snapshot(state, fast, ring_capacity)?;
    if let Some(snapshot) = strategic {
        push_runtime_strategic_snapshot(state, snapshot, ring_capacity)?;
    }
    push_runtime_snapshot(state, combined, ring_capacity)
}

pub(crate) fn push_runtime_fast_snapshot(
    state: &AppState,
    snapshot: RuntimeFastSnapshot,
    ring_capacity: usize,
) -> Result<(), String> {
    let mut guard = state
        .runtime_fast_snapshots
        .lock()
        .map_err(|_| "runtime_fast_snapshots mutex poisoned".to_string())?;
    guard.push_back(snapshot);
    let cap = ring_capacity.clamp(4, 256);
    while guard.len() > cap {
        guard.pop_front();
    }
    Ok(())
}

pub(crate) fn push_runtime_strategic_snapshot(
    state: &AppState,
    snapshot: RuntimeStrategicSnapshot,
    ring_capacity: usize,
) -> Result<(), String> {
    let mut guard = state
        .runtime_strategic_snapshots
        .lock()
        .map_err(|_| "runtime_strategic_snapshots mutex poisoned".to_string())?;
    guard.push_back(snapshot);
    let cap = ring_capacity.clamp(4, 256);
    while guard.len() > cap {
        guard.pop_front();
    }
    Ok(())
}

pub(crate) fn latest_runtime_snapshot_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<Option<RuntimeSnapshot>, String> {
    if let Some(fast) = latest_runtime_fast_snapshot_for_project(state, project_path)? {
        let strategic = latest_runtime_strategic_snapshot_for_project(state, project_path)?;
        return Ok(Some(runtime_snapshot_from_parts(&fast, strategic.as_ref())));
    }
    let guard = state
        .runtime_snapshots
        .lock()
        .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
    let latest = guard
        .iter()
        .rev()
        .find(|s| s.project_path == project_path)
        .cloned();
    Ok(latest)
}

pub(crate) fn latest_runtime_fast_snapshot_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<Option<RuntimeFastSnapshot>, String> {
    let guard = state
        .runtime_fast_snapshots
        .lock()
        .map_err(|_| "runtime_fast_snapshots mutex poisoned".to_string())?;
    let latest = guard
        .iter()
        .rev()
        .find(|s| s.project_path == project_path)
        .cloned();
    Ok(latest)
}

pub(crate) fn latest_runtime_strategic_snapshot_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<Option<RuntimeStrategicSnapshot>, String> {
    let guard = state
        .runtime_strategic_snapshots
        .lock()
        .map_err(|_| "runtime_strategic_snapshots mutex poisoned".to_string())?;
    let latest = guard
        .iter()
        .rev()
        .find(|s| s.project_path == project_path)
        .cloned();
    Ok(latest)
}
