use serde::Serialize;
use std::collections::VecDeque;
use std::time::Instant;

use crate::{
    AppState, CounterProvenance, RuntimeFastSnapshot, RuntimeSnapshot, RuntimeStrategicSnapshot,
    SimulationAdvanceEconomy,
};

const RUNTIME_SNAPSHOT_SIZE_SAMPLE_INTERVAL_TICKS: u64 = 20;

fn normalized_snapshot_ring_capacity(ring_capacity: usize) -> usize {
    // No public API exposes historical runtime snapshots today; callers ask
    // for latest fast/strategic/combined snapshots. Honor low configured caps
    // so development builds can keep only the latest payload instead of the
    // previous hard minimum of four full snapshots per ring.
    ring_capacity.clamp(1, 256)
}

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
        passenger_counter_provenance: snapshot.passenger_counter_provenance,
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
        passenger_counter_provenance: CounterProvenance::StrategicEstimate,
        fare_counter_provenance: snapshot.fare_counter_provenance,
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
            passenger_counter_provenance: fast.passenger_counter_provenance,
            fare_counter_provenance: strategic.fare_counter_provenance,
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
        passenger_counter_provenance: fast.passenger_counter_provenance,
        fare_counter_provenance: CounterProvenance::StrategicEstimate,
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
    let cap = normalized_snapshot_ring_capacity(ring_capacity);
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

fn clear_strategic_payload(snapshot: &mut RuntimeSnapshot) {
    snapshot.economy = SimulationAdvanceEconomy {
        current_balance_base: 0.0,
        cumulative_revenue_base: 0.0,
        cumulative_opex_base: 0.0,
        budget_display: 0.0,
    };
    snapshot.frame = None;
    snapshot.delta_revenue_base = 0.0;
    snapshot.delta_opex_base = 0.0;
    snapshot.delta_net_base = 0.0;
    snapshot.fare_counter_provenance = CounterProvenance::StrategicEstimate;
}

pub(crate) fn should_measure_runtime_snapshot_size(
    telemetry: &crate::RuntimePerfTelemetry,
    strategic_requested: bool,
) -> bool {
    // Exact byte telemetry serializes the snapshot, so keep it sampled and
    // diagnostic-triggered instead of paying that cost on every fast publish.
    telemetry.tick_index == 0
        || telemetry
            .tick_index
            .is_multiple_of(RUNTIME_SNAPSHOT_SIZE_SAMPLE_INTERVAL_TICKS)
        || telemetry.backlog_steps > 0
        || telemetry.executed_steps_this_cycle > 1
        || strategic_requested
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
    let strategic_requested = strategic_refresh_due;
    let mut fast = runtime_fast_snapshot_from_combined(&combined);
    let mut strategic = if strategic_refresh_due {
        Some(runtime_strategic_snapshot_from_combined(&combined))
    } else {
        None
    };
    let measure_snapshot_bytes =
        should_measure_runtime_snapshot_size(&fast.telemetry, strategic_requested);
    let (fast_snapshot_bytes, strategic_snapshot_bytes) = if measure_snapshot_bytes {
        (
            runtime_snapshot_size_bytes(&fast),
            strategic
                .as_ref()
                .map(runtime_snapshot_size_bytes)
                .unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    fast.telemetry.fast_snapshot_bytes = fast_snapshot_bytes;
    fast.telemetry.strategic_snapshot_bytes = strategic_snapshot_bytes;
    if let Some(snapshot) = strategic.as_mut() {
        snapshot.telemetry.fast_snapshot_bytes = fast_snapshot_bytes;
        snapshot.telemetry.strategic_snapshot_bytes = strategic_snapshot_bytes;
    }
    // Keep the legacy combined ring logically equivalent to
    // runtime_snapshot_from_parts without cloning fast trains/stations/line_ops
    // a second time. The dedicated fast/strategic rings are the primary
    // publish buffers; combined remains for compatibility/fallback reads.
    if strategic.is_none() {
        clear_strategic_payload(&mut combined);
    }
    combined.telemetry = fast.telemetry.clone();
    let publish_ms = publish_start.elapsed().as_secs_f64() * 1000.0;
    fast.telemetry.snapshot_publish_ms = publish_ms;
    combined.telemetry.snapshot_publish_ms = publish_ms;
    if let Some(snapshot) = strategic.as_mut() {
        snapshot.telemetry.snapshot_publish_ms = publish_ms;
    }
    if fast.telemetry.tick_index == 0
        || fast.telemetry.tick_index.is_multiple_of(20)
        || fast.telemetry.backlog_steps > 0
        || fast.telemetry.executed_steps_this_cycle > 1
        || publish_ms > 30.0
    {
        eprintln!(
            "[rt-snap] publish project={} tick_seconds={:.3} tick_index={} clock_revision={} running={} speed={} publish_ms={:.2} fast_bytes={} strategic_bytes={} strategic_requested={} strategic_published={} ring_capacity={} backlog_steps={} executed_steps_this_cycle={}",
            fast.project_path,
            fast.clock.tick_seconds,
            fast.telemetry.tick_index,
            fast.clock_revision,
            fast.clock.running,
            fast.clock.speed,
            publish_ms.max(0.0),
            fast_snapshot_bytes,
            strategic_snapshot_bytes,
            strategic_requested,
            strategic.is_some(),
            normalized_snapshot_ring_capacity(ring_capacity),
            fast.telemetry.backlog_steps,
            fast.telemetry.executed_steps_this_cycle,
        );
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
    let cap = normalized_snapshot_ring_capacity(ring_capacity);
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
    let cap = normalized_snapshot_ring_capacity(ring_capacity);
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

fn latest_fast_snapshot_tick_index_from_ring(
    snapshots: &VecDeque<RuntimeFastSnapshot>,
    project_path: &str,
) -> Option<u64> {
    snapshots
        .iter()
        .rev()
        .find(|s| s.project_path == project_path)
        .map(|snapshot| snapshot.telemetry.tick_index)
}

fn latest_combined_snapshot_tick_index_from_ring(
    snapshots: &VecDeque<RuntimeSnapshot>,
    project_path: &str,
) -> Option<u64> {
    snapshots
        .iter()
        .rev()
        .find(|s| s.project_path == project_path)
        .map(|snapshot| snapshot.telemetry.tick_index)
}

pub(crate) fn latest_runtime_tick_index_for_project(
    state: &AppState,
    project_path: &str,
) -> Result<Option<u64>, String> {
    {
        let guard = state
            .runtime_fast_snapshots
            .lock()
            .map_err(|_| "runtime_fast_snapshots mutex poisoned".to_string())?;
        if let Some(tick_index) = latest_fast_snapshot_tick_index_from_ring(&guard, project_path) {
            return Ok(Some(tick_index));
        }
    }
    let guard = state
        .runtime_snapshots
        .lock()
        .map_err(|_| "runtime_snapshots mutex poisoned".to_string())?;
    Ok(latest_combined_snapshot_tick_index_from_ring(
        &guard,
        project_path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimePerfTelemetry, SimulationClock, TrainRuntimeView};
    use std::collections::VecDeque;

    fn telemetry(tick_index: u64) -> RuntimePerfTelemetry {
        RuntimePerfTelemetry {
            tick_index,
            ..RuntimePerfTelemetry::default()
        }
    }

    fn test_clock() -> SimulationClock {
        SimulationClock {
            sim_datetime_utc: "2026-01-01T08:00:00Z".to_string(),
            tick_seconds: 42.0,
            running: true,
            speed: 2,
        }
    }

    fn test_train_view() -> TrainRuntimeView {
        TrainRuntimeView {
            train_id: "train:a".to_string(),
            service_id: "svc:a".to_string(),
            line_id: "line:a".to_string(),
            line_name: "Line A".to_string(),
            vehicle_ordinal: 1,
            direction_label: "Outbound".to_string(),
            destination_stop_id: "stop:b".to_string(),
            destination_label: "To B".to_string(),
            mode: "metro".to_string(),
            mode_variant: None,
            stock_tier_id: None,
            vehicle_capacity: 100.0,
            onboard_pax: 12.0,
            x: 1.0,
            y: 2.0,
            at_stop_id: None,
            in_motion: true,
            provenance: CounterProvenance::AnimationOnly,
            passenger_counter_provenance: CounterProvenance::AnimationOnly,
        }
    }

    fn test_snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot {
            project_path: "project-a".to_string(),
            clock_revision: 7,
            clock: test_clock(),
            economy: SimulationAdvanceEconomy {
                current_balance_base: 100.0,
                cumulative_revenue_base: 20.0,
                cumulative_opex_base: 5.0,
                budget_display: 95.0,
            },
            frame: None,
            delta_revenue_base: 2.0,
            delta_opex_base: 1.0,
            delta_net_base: 1.0,
            captured_at_epoch_ms: 123,
            telemetry: telemetry(3),
            trains: vec![test_train_view()],
            stations: Vec::new(),
            line_ops: Vec::new(),
            provenance_warnings: vec!["runtime_projection: test".to_string()],
            trains_authoritative: true,
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
            fare_counter_provenance: CounterProvenance::AuthoritativeSim,
        }
    }

    #[test]
    fn snapshot_size_measurement_is_sampled_for_plain_fast_ticks() {
        assert!(should_measure_runtime_snapshot_size(&telemetry(0), false));
        assert!(should_measure_runtime_snapshot_size(&telemetry(20), false));
        assert!(!should_measure_runtime_snapshot_size(&telemetry(1), false));
    }

    #[test]
    fn snapshot_size_measurement_runs_for_diagnostic_conditions() {
        let mut backlog = telemetry(1);
        backlog.backlog_steps = 1;
        assert!(should_measure_runtime_snapshot_size(&backlog, false));

        let mut catchup = telemetry(1);
        catchup.executed_steps_this_cycle = 2;
        assert!(should_measure_runtime_snapshot_size(&catchup, false));

        assert!(should_measure_runtime_snapshot_size(&telemetry(1), true));
    }

    #[test]
    fn normalized_snapshot_ring_capacity_allows_latest_only_retention() {
        assert_eq!(normalized_snapshot_ring_capacity(0), 1);
        assert_eq!(normalized_snapshot_ring_capacity(1), 1);
        assert_eq!(normalized_snapshot_ring_capacity(4), 4);
        assert_eq!(normalized_snapshot_ring_capacity(999), 256);
    }

    #[test]
    fn latest_tick_index_reads_newest_matching_fast_ring() {
        let mut first = runtime_fast_snapshot_from_combined(&test_snapshot());
        first.project_path = "project-a".to_string();
        first.telemetry.tick_index = 2;

        let mut other_project = first.clone();
        other_project.project_path = "project-b".to_string();
        other_project.telemetry.tick_index = 99;

        let mut newest = first.clone();
        newest.telemetry.tick_index = 7;

        let ring = VecDeque::from([first, other_project, newest]);

        assert_eq!(
            latest_fast_snapshot_tick_index_from_ring(&ring, "project-a"),
            Some(7)
        );
        assert_eq!(
            latest_fast_snapshot_tick_index_from_ring(&ring, "missing"),
            None
        );
    }

    #[test]
    fn latest_tick_index_can_fall_back_to_combined_ring() {
        let mut snapshot = test_snapshot();
        snapshot.telemetry.tick_index = 11;
        let ring = VecDeque::from([snapshot]);

        assert_eq!(
            latest_combined_snapshot_tick_index_from_ring(&ring, "project-a"),
            Some(11)
        );
        assert_eq!(
            latest_combined_snapshot_tick_index_from_ring(&ring, "missing"),
            None
        );
    }

    #[test]
    fn clear_strategic_payload_preserves_fast_operational_payload() {
        let mut combined = test_snapshot();
        let fast = runtime_fast_snapshot_from_combined(&combined);
        let rebuilt_fast_only = runtime_snapshot_from_parts(&fast, None);

        clear_strategic_payload(&mut combined);
        combined.telemetry = fast.telemetry.clone();

        assert_eq!(combined.economy.current_balance_base, 0.0);
        assert!(combined.frame.is_none());
        assert_eq!(combined.delta_revenue_base, 0.0);
        assert_eq!(combined.delta_opex_base, 0.0);
        assert_eq!(combined.delta_net_base, 0.0);
        assert_eq!(
            combined.fare_counter_provenance,
            CounterProvenance::StrategicEstimate
        );
        assert_eq!(combined.trains.len(), rebuilt_fast_only.trains.len());
        assert_eq!(
            combined.trains[0].train_id,
            rebuilt_fast_only.trains[0].train_id
        );
        assert_eq!(
            combined.passenger_counter_provenance,
            rebuilt_fast_only.passenger_counter_provenance
        );
    }
}
