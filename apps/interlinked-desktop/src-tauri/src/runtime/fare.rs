use crate::*;

use super::models::RuntimeFareEvents;
use std::collections::HashMap;

use interlinked_engine::model::Scenario;
use interlinked_engine::sim::{
    BoardLoad, EngineFarePolicyContext, PassengerCohortFlow, SimulationOutput,
};

pub(crate) fn runtime_fare_base_per_boarding(policy: &FarePolicyManifest, mode: &str) -> f64 {
    if !policy.enabled {
        return 0.0;
    }
    match fare_mode_bucket_from_tokens(mode, None, 0.0) {
        FareModeBucket::Bus => policy.fare_mode_bus_base.max(0.0),
        FareModeBucket::Tram => policy.fare_mode_tram_base.max(0.0),
        FareModeBucket::Metro => policy.fare_mode_metro_base.max(0.0),
        FareModeBucket::Rail => policy.fare_mode_rail_base.max(0.0),
        FareModeBucket::Ferry => policy.fare_mode_ferry_base.max(0.0),
        FareModeBucket::Default => policy.fare_mode_default_base.max(0.0),
    }
}

pub(crate) fn engine_fare_policy_context_from_scenario_manifest(
    scenario: &Scenario,
    fare_policy: &FarePolicyManifest,
) -> EngineFarePolicyContext {
    let fare_by_service_id = scenario
        .world
        .services
        .iter()
        .filter_map(|service| {
            let fare = runtime_fare_base_per_boarding(fare_policy, service.mode.as_str());
            if fare.is_finite() && fare > 0.0 {
                Some((service.id.clone(), fare))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>();

    EngineFarePolicyContext::from_service_fares(
        fare_policy.enabled,
        "runtime_manifest_simple_mode_fares",
        fare_by_service_id,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassengerFareEventKind {
    FareRecognized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PassengerLifecycleEventKind {
    Boarding,
    Alighting,
    FareRecognized,
}

/// Aggregate passenger lifecycle event seam.
///
/// This is not an individual passenger model. Authoritative lifecycle events
/// must come from simulation-owned passenger state, not desktop animation or
/// projection counters. Current engine board/fare outputs are still treated as
/// strategic estimates; desktop runtime ops remain an explicit projection fallback.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PassengerLifecycleEvent {
    pub(crate) simulation_time_s: f64,
    pub(crate) kind: PassengerLifecycleEventKind,
    pub(crate) stop_id: Option<String>,
    pub(crate) line_id: Option<String>,
    pub(crate) service_id: Option<String>,
    pub(crate) vehicle_id: Option<String>,
    pub(crate) passenger_count: f64,
    pub(crate) fare_delta_base: Option<f64>,
    pub(crate) provenance: CounterProvenance,
    pub(crate) source_label: String,
}

impl PassengerLifecycleEvent {
    fn aggregate(
        simulation_time_s: f64,
        kind: PassengerLifecycleEventKind,
        passenger_count: f64,
        fare_delta_base: Option<f64>,
        provenance: CounterProvenance,
        source_label: &str,
    ) -> Self {
        Self {
            simulation_time_s: sanitize_simulation_time_s(simulation_time_s),
            kind,
            stop_id: None,
            line_id: None,
            service_id: None,
            vehicle_id: None,
            passenger_count: passenger_count.max(0.0),
            fare_delta_base: fare_delta_base.map(|value| value.max(0.0)),
            provenance,
            source_label: source_label.to_string(),
        }
    }

    fn service_stop(
        simulation_time_s: f64,
        kind: PassengerLifecycleEventKind,
        service_id: &str,
        stop_id: &str,
        passenger_count: f64,
        provenance: CounterProvenance,
        source_label: &str,
    ) -> Self {
        Self {
            simulation_time_s: sanitize_simulation_time_s(simulation_time_s),
            kind,
            stop_id: Some(stop_id.to_string()),
            line_id: None,
            service_id: Some(service_id.to_string()),
            vehicle_id: None,
            passenger_count: passenger_count.max(0.0),
            fare_delta_base: None,
            provenance,
            source_label: source_label.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PassengerLifecycleEventBatch {
    pub(crate) events: Vec<PassengerLifecycleEvent>,
}

impl PassengerLifecycleEventBatch {
    fn from_events(events: Vec<PassengerLifecycleEvent>) -> Self {
        Self { events }
    }

    fn is_authoritative_sim(&self) -> bool {
        !self.events.is_empty()
            && self
                .events
                .iter()
                .all(|event| event.provenance == CounterProvenance::AuthoritativeSim)
    }

    fn has_authoritative_fare_recognition(&self) -> bool {
        self.events.iter().any(|event| {
            event.provenance == CounterProvenance::AuthoritativeSim
                && event.kind == PassengerLifecycleEventKind::FareRecognized
                && event.fare_delta_base.is_some_and(f64::is_finite)
        })
    }

    fn has_fare_recognition(&self) -> bool {
        self.events.iter().any(|event| {
            event.kind == PassengerLifecycleEventKind::FareRecognized
                && event.fare_delta_base.is_some_and(f64::is_finite)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PassengerFareEvent {
    pub(crate) simulation_time_s: f64,
    pub(crate) kind: PassengerFareEventKind,
    pub(crate) passenger_count: f64,
    pub(crate) completed_passenger_count: f64,
    pub(crate) fare_delta_base: f64,
    pub(crate) provenance: CounterProvenance,
    pub(crate) source_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FareSourceSelectionDiagnostic {
    pub(crate) selected_provenance: CounterProvenance,
    pub(crate) selected_source_label: String,
    pub(crate) used_authoritative_sim: bool,
    pub(crate) used_strategic_estimate: bool,
    pub(crate) used_runtime_projection_fallback: bool,
}

pub(crate) fn fare_source_selection_diagnostic(
    selected: &PassengerFareEvent,
) -> FareSourceSelectionDiagnostic {
    FareSourceSelectionDiagnostic {
        selected_provenance: selected.provenance,
        selected_source_label: selected.source_label.clone(),
        used_authoritative_sim: selected.provenance == CounterProvenance::AuthoritativeSim,
        used_strategic_estimate: selected.provenance == CounterProvenance::StrategicEstimate,
        used_runtime_projection_fallback: selected.provenance
            == CounterProvenance::RuntimeProjection,
    }
}

pub(crate) fn runtime_fare_source_telemetry(
    selected: &PassengerFareEvent,
) -> RuntimeFareSourceTelemetry {
    let diagnostic = fare_source_selection_diagnostic(selected);
    RuntimeFareSourceTelemetry {
        selected_provenance: diagnostic.selected_provenance,
        selected_source_label: diagnostic.selected_source_label,
        selected_fare_delta_base: selected.fare_delta_base.max(0.0),
        selected_passenger_count: selected.passenger_count.max(0.0),
        used_authoritative_fare: diagnostic.used_authoritative_sim,
        used_strategic_fare_fallback: diagnostic.used_strategic_estimate,
        used_runtime_projection_fare_fallback: diagnostic.used_runtime_projection_fallback,
        fallback_used: diagnostic.used_strategic_estimate
            || diagnostic.used_runtime_projection_fallback,
    }
}

impl PassengerFareEvent {
    fn sanitized(
        simulation_time_s: f64,
        passenger_count: f64,
        completed_passenger_count: f64,
        fare_delta_base: f64,
        provenance: CounterProvenance,
        source_label: &str,
    ) -> Self {
        Self {
            simulation_time_s: sanitize_simulation_time_s(simulation_time_s),
            kind: PassengerFareEventKind::FareRecognized,
            passenger_count: passenger_count.max(0.0),
            completed_passenger_count: completed_passenger_count.max(0.0),
            fare_delta_base: fare_delta_base.max(0.0),
            provenance,
            source_label: source_label.to_string(),
        }
    }
}

fn sanitize_simulation_time_s(simulation_time_s: f64) -> f64 {
    if simulation_time_s.is_finite() {
        simulation_time_s.max(0.0)
    } else {
        0.0
    }
}

fn lifecycle_events_from_totals(
    simulation_time_s: f64,
    fare_delta_base: f64,
    passenger_count: f64,
    completed_passenger_count: f64,
    provenance: CounterProvenance,
    source_label: &str,
) -> PassengerLifecycleEventBatch {
    PassengerLifecycleEventBatch::from_events(vec![
        PassengerLifecycleEvent::aggregate(
            simulation_time_s,
            PassengerLifecycleEventKind::Boarding,
            passenger_count,
            None,
            provenance,
            source_label,
        ),
        PassengerLifecycleEvent::aggregate(
            simulation_time_s,
            PassengerLifecycleEventKind::Alighting,
            completed_passenger_count,
            None,
            provenance,
            source_label,
        ),
        PassengerLifecycleEvent::aggregate(
            simulation_time_s,
            PassengerLifecycleEventKind::FareRecognized,
            passenger_count,
            Some(fare_delta_base),
            provenance,
            source_label,
        ),
    ])
}

pub(crate) fn fare_event_from_lifecycle_events(
    lifecycle_events: &PassengerLifecycleEventBatch,
) -> PassengerFareEvent {
    let mut simulation_time_s = 0.0_f64;
    let mut passenger_count = 0.0_f64;
    let mut completed_passenger_count = 0.0_f64;
    let mut fare_delta_base = 0.0_f64;
    let mut provenance = CounterProvenance::DebugLegacy;
    let mut source_label = "lifecycle_events";

    for event in &lifecycle_events.events {
        simulation_time_s = simulation_time_s.max(event.simulation_time_s);
        match event.kind {
            PassengerLifecycleEventKind::Boarding => {
                passenger_count += event.passenger_count.max(0.0);
            }
            PassengerLifecycleEventKind::Alighting => {
                completed_passenger_count += event.passenger_count.max(0.0);
            }
            PassengerLifecycleEventKind::FareRecognized => {
                fare_delta_base += event.fare_delta_base.unwrap_or(0.0).max(0.0);
                provenance = event.provenance;
                source_label = &event.source_label;
                if passenger_count <= 0.0 {
                    passenger_count = event.passenger_count.max(0.0);
                }
            }
        }
    }

    PassengerFareEvent::sanitized(
        simulation_time_s,
        passenger_count,
        completed_passenger_count,
        fare_delta_base,
        provenance,
        source_label,
    )
}

pub(crate) fn strategic_lifecycle_events_from_totals(
    simulation_time_s: f64,
    passenger_count: f64,
    completed_passenger_count: f64,
    fare_delta_base: f64,
) -> PassengerLifecycleEventBatch {
    lifecycle_events_from_totals(
        simulation_time_s,
        fare_delta_base,
        passenger_count,
        completed_passenger_count,
        CounterProvenance::StrategicEstimate,
        "strategic_model_fare_flow",
    )
}

pub(crate) fn strategic_kpi_lifecycle_events_for_economy(
    frame_lite: &HistoryFrameLite,
) -> PassengerLifecycleEventBatch {
    let served = frame_lite.kpis.total_boardings_served.max(0.0);
    lifecycle_events_from_totals(
        frame_lite.t_s,
        frame_lite.kpis.total_fare_revenue_base.max(0.0),
        served,
        served,
        CounterProvenance::StrategicEstimate,
        "strategic_kpi_fare_flow",
    )
}

/// Export the simulation-owned slice currently available from the engine fast kernel.
///
/// The fast operational kernel owns queue cohorts and consumes them into
/// per-step boardings, then carries destination-aware onboard cohorts until
/// they alight. Boarding and Alighting events from this output can be marked
/// authoritative. Fare recognition is only emitted when the completed/alighted
/// cohort can be priced with the current deterministic fare basis.
pub(crate) fn authoritative_lifecycle_events_from_engine_fast_output(
    simulation_time_s: f64,
    output: &SimulationOutput,
) -> Option<PassengerLifecycleEventBatch> {
    authoritative_lifecycle_events_from_engine_fast_board_loads_with_fares(
        simulation_time_s,
        &output.meta.results_version,
        &output.board_loads,
        &output.passenger_cohorts,
    )
}

#[cfg(test)]
fn authoritative_lifecycle_events_from_engine_fast_board_loads(
    simulation_time_s: f64,
    results_version: &str,
    board_loads: &[BoardLoad],
    passenger_cohorts: &[PassengerCohortFlow],
) -> Option<PassengerLifecycleEventBatch> {
    authoritative_lifecycle_events_from_engine_fast_board_loads_with_fares(
        simulation_time_s,
        results_version,
        board_loads,
        passenger_cohorts,
    )
}

fn authoritative_lifecycle_events_from_engine_fast_board_loads_with_fares(
    simulation_time_s: f64,
    results_version: &str,
    board_loads: &[BoardLoad],
    passenger_cohorts: &[PassengerCohortFlow],
) -> Option<PassengerLifecycleEventBatch> {
    if !results_version.contains("fast-operational") {
        return None;
    }

    let mut events = Vec::<PassengerLifecycleEvent>::new();
    for load in board_loads {
        let boarded =
            (load.served_from_arrivals.max(0.0) + load.served_from_queue.max(0.0)).max(0.0);
        if boarded <= 0.0 {
            continue;
        }
        events.push(PassengerLifecycleEvent::service_stop(
            simulation_time_s,
            PassengerLifecycleEventKind::Boarding,
            &load.service_id,
            &load.stop_id,
            boarded,
            CounterProvenance::AuthoritativeSim,
            "engine_fast_queue_boardings",
        ));
    }
    for cohort in passenger_cohorts {
        let alighted = cohort.alighted_pax.max(0.0);
        if alighted <= 0.0 {
            continue;
        }
        events.push(PassengerLifecycleEvent::service_stop(
            simulation_time_s,
            PassengerLifecycleEventKind::Alighting,
            &cohort.service_id,
            &cohort.destination_stop_id,
            alighted,
            CounterProvenance::AuthoritativeSim,
            "engine_fast_onboard_alightings",
        ));
    }

    for cohort in passenger_cohorts {
        let alighted = cohort.alighted_pax.max(0.0);
        if alighted <= 0.0 {
            continue;
        }

        let fare_delta_base = cohort.fare_delta_base.max(0.0);
        if fare_delta_base > 0.0 {
            // Aggregate fare ledger: completed/alighted onboard cohorts are the
            // first simulation-owned place where fare can be recognized without
            // relying on desktop projection or animation state. The fare amount
            // is produced by the engine fast kernel from RunConfig fare context.
            events.push(PassengerLifecycleEvent {
                simulation_time_s,
                kind: PassengerLifecycleEventKind::FareRecognized,
                stop_id: Some(cohort.destination_stop_id.clone()),
                line_id: None,
                service_id: Some(cohort.service_id.clone()),
                vehicle_id: None,
                passenger_count: alighted,
                fare_delta_base: Some(fare_delta_base),
                provenance: CounterProvenance::AuthoritativeSim,
                source_label: "engine_fast_completed_cohort_fare".to_string(),
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(PassengerLifecycleEventBatch::from_events(events))
    }
}

/// Temporary economy fallback for desktop runtime ops.
///
/// Projection boarding/alighting events are useful for operational feel, but they
/// are not authoritative simulation truth. Keep this path explicitly named and
/// provenance-tagged until a simulation-owned passenger lifecycle source replaces it.
pub(crate) fn collect_projection_lifecycle_events_fallback(
    simulation_time_s: f64,
    events: RuntimeFareEvents,
) -> PassengerLifecycleEventBatch {
    lifecycle_events_from_totals(
        simulation_time_s,
        events.liability_accrued_base,
        events.boarded_pax,
        events.completed_alightings_pax,
        CounterProvenance::RuntimeProjection,
        "desktop_projection_fare_events",
    )
}

pub(crate) fn select_fare_event_for_economy(
    simulation_owned_lifecycle_events: Option<&PassengerLifecycleEventBatch>,
    strategic_lifecycle_events: &PassengerLifecycleEventBatch,
    projection_lifecycle_fallback: Option<&PassengerLifecycleEventBatch>,
    projection_fallback_enabled: bool,
) -> PassengerFareEvent {
    if let Some(events) = simulation_owned_lifecycle_events.filter(|events| {
        events.is_authoritative_sim() && events.has_authoritative_fare_recognition()
    }) {
        return fare_event_from_lifecycle_events(events);
    }
    if strategic_lifecycle_events.has_fare_recognition() {
        return fare_event_from_lifecycle_events(strategic_lifecycle_events);
    }
    if projection_fallback_enabled {
        if let Some(events) = projection_lifecycle_fallback {
            return fare_event_from_lifecycle_events(events);
        }
    }
    fare_event_from_lifecycle_events(strategic_lifecycle_events)
}

pub(crate) fn strategic_lifecycle_events_for_economy(
    gs: &interlinked_engine::platform::GameState,
) -> PassengerLifecycleEventBatch {
    let simulation_time_s = gs.tick_s;
    let Some(output) = gs.last_output.as_ref() else {
        return strategic_lifecycle_events_from_totals(simulation_time_s, 0.0, 0.0, 0.0);
    };
    let liability_base = output.fare_flow.liability_accrued_base.max(0.0);
    let liability_pax = output.fare_flow.liability_accrued_pax.max(0.0);
    let completed_pax = output.fare_flow.completed_journeys_pax.max(0.0);
    if liability_base > 0.0 || liability_pax > 0.0 || completed_pax > 0.0 {
        return strategic_lifecycle_events_from_totals(
            simulation_time_s,
            liability_pax,
            completed_pax,
            liability_base,
        );
    }

    // Backward-compatible fallback for older outputs without fare_flow population.
    let mut fallback_completed = 0.0_f64;
    for load in &output.board_loads {
        let alightings = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        if alightings <= 0.0 {
            continue;
        }
        if load.departures_observed > 0 {
            fallback_completed += alightings;
        }
    }
    strategic_lifecycle_events_from_totals(
        simulation_time_s,
        output.kpis.total_boardings_served.max(0.0),
        fallback_completed.max(0.0),
        output.kpis.total_fare_revenue_base.max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn test_board_load(service_id: &str, stop_id: &str, served: f64) -> BoardLoad {
        BoardLoad {
            service_id: service_id.to_string(),
            stop_id: stop_id.to_string(),
            arrivals: served,
            served_from_arrivals: served,
            served_from_queue: 0.0,
            denied_boardings: 0.0,
            queue_start: 0.0,
            queue_end: 0.0,
            headway_s: 300.0,
            vehicle_capacity: 100.0,
            departures_in_period: 1.0,
            departures_observed: 1,
            capacity_in_period: 100.0,
            extra_wait_s: 0.0,
            time_bins: Vec::new(),
            time_to_next_departure_s_end: 0.0,
            alightings_served: 5.0,
            station_capacity_boarding_pph: 2500.0,
            station_capacity_alighting_pph: 2800.0,
            station_queue_capacity_pax: 500.0,
            overflow_dropped: 0.0,
        }
    }

    fn test_passenger_cohort(
        service_id: &str,
        board_stop_id: &str,
        destination_stop_id: &str,
        alighted: f64,
    ) -> PassengerCohortFlow {
        PassengerCohortFlow {
            service_id: service_id.to_string(),
            board_stop_id: board_stop_id.to_string(),
            destination_stop_id: destination_stop_id.to_string(),
            attempted_pax: 0.0,
            boarded_pax: 0.0,
            alighted_pax: alighted,
            fare_delta_base: 0.0,
            queue_end_pax: 0.0,
        }
    }

    fn test_passenger_cohort_with_fare(
        service_id: &str,
        board_stop_id: &str,
        destination_stop_id: &str,
        alighted: f64,
        fare_delta_base: f64,
    ) -> PassengerCohortFlow {
        PassengerCohortFlow {
            fare_delta_base,
            ..test_passenger_cohort(service_id, board_stop_id, destination_stop_id, alighted)
        }
    }

    #[test]
    fn projection_fare_events_fallback_is_runtime_projection() {
        let lifecycle_events = collect_projection_lifecycle_events_fallback(
            42.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );
        let event = fare_event_from_lifecycle_events(&lifecycle_events);

        assert_eq!(event.provenance, CounterProvenance::RuntimeProjection);
        assert_ne!(event.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(event.kind, PassengerFareEventKind::FareRecognized);
        assert_eq!(event.simulation_time_s, 42.0);
        assert_eq!(event.passenger_count, 12.5);
        assert_eq!(event.completed_passenger_count, 9.0);
        assert_eq!(event.fare_delta_base, 31.25);
    }

    #[test]
    fn projection_lifecycle_events_are_not_authoritative() {
        let events = collect_projection_lifecycle_events_fallback(
            42.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );

        assert!(events
            .events
            .iter()
            .all(|event| event.provenance == CounterProvenance::RuntimeProjection));
        assert!(!events.is_authoritative_sim());
        assert!(events
            .events
            .iter()
            .all(|event| event.source_label == "desktop_projection_fare_events"));
    }

    #[test]
    fn strategic_fare_events_are_strategic_estimates() {
        let lifecycle_events = strategic_lifecycle_events_from_totals(7.0, 8.0, 6.0, 19.0);
        let event = fare_event_from_lifecycle_events(&lifecycle_events);

        assert_eq!(event.provenance, CounterProvenance::StrategicEstimate);
        assert_ne!(event.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(event.kind, PassengerFareEventKind::FareRecognized);
        assert_eq!(event.simulation_time_s, 7.0);
        assert_eq!(event.passenger_count, 8.0);
        assert_eq!(event.completed_passenger_count, 6.0);
        assert_eq!(event.fare_delta_base, 19.0);
    }

    #[test]
    fn strategic_lifecycle_events_preserve_provenance() {
        let events = strategic_lifecycle_events_from_totals(7.0, 8.0, 6.0, 19.0);
        let fare_event = fare_event_from_lifecycle_events(&events);

        assert_eq!(events.events.len(), 3);
        assert!(events
            .events
            .iter()
            .all(|event| event.provenance == CounterProvenance::StrategicEstimate));
        assert_eq!(fare_event.provenance, CounterProvenance::StrategicEstimate);
        assert_eq!(fare_event.source_label, "strategic_model_fare_flow");
        assert_eq!(fare_event.passenger_count, 8.0);
        assert_eq!(fare_event.completed_passenger_count, 6.0);
        assert_eq!(fare_event.fare_delta_base, 19.0);
    }

    #[test]
    fn engine_fast_boarding_export_is_authoritative_without_fare_authority() {
        let events = authoritative_lifecycle_events_from_engine_fast_board_loads(
            12.0,
            "0.2.2-fast-operational-v1",
            &[test_board_load("svc:a", "stop:a", 14.0)],
            &[],
        )
        .unwrap();

        assert_eq!(events.events.len(), 1);
        let event = &events.events[0];
        assert_eq!(event.kind, PassengerLifecycleEventKind::Boarding);
        assert_eq!(event.service_id.as_deref(), Some("svc:a"));
        assert_eq!(event.stop_id.as_deref(), Some("stop:a"));
        assert_eq!(event.passenger_count, 14.0);
        assert_eq!(event.fare_delta_base, None);
        assert_eq!(event.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(event.source_label, "engine_fast_queue_boardings");
        assert!(!events.has_authoritative_fare_recognition());
    }

    #[test]
    fn engine_fast_alighting_export_is_authoritative_from_onboard_state() {
        let events = authoritative_lifecycle_events_from_engine_fast_board_loads(
            12.0,
            "0.2.2-fast-operational-v1",
            &[],
            &[test_passenger_cohort("svc:a", "stop:a", "stop:b", 6.0)],
        )
        .unwrap();

        assert_eq!(events.events.len(), 1);
        let event = &events.events[0];
        assert_eq!(event.kind, PassengerLifecycleEventKind::Alighting);
        assert_eq!(event.service_id.as_deref(), Some("svc:a"));
        assert_eq!(event.stop_id.as_deref(), Some("stop:b"));
        assert_eq!(event.passenger_count, 6.0);
        assert_eq!(event.fare_delta_base, None);
        assert_eq!(event.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(event.source_label, "engine_fast_onboard_alightings");
        assert!(!events.has_authoritative_fare_recognition());
    }

    #[test]
    fn strategic_outputs_do_not_create_authoritative_engine_lifecycle_events() {
        let events = authoritative_lifecycle_events_from_engine_fast_board_loads(
            12.0,
            "0.2.2-strategic",
            &[test_board_load("svc:a", "stop:a", 14.0)],
            &[test_passenger_cohort("svc:a", "stop:a", "stop:b", 6.0)],
        );

        assert!(events.is_none());
    }

    #[test]
    fn economy_selection_prefers_authoritative_lifecycle_over_projection_fallback() {
        let authoritative = lifecycle_events_from_totals(
            10.0,
            100.0,
            40.0,
            35.0,
            CounterProvenance::AuthoritativeSim,
            "simulation_owned_lifecycle",
        );
        let strategic = strategic_lifecycle_events_from_totals(10.0, 8.0, 6.0, 19.0);
        let projection = collect_projection_lifecycle_events_fallback(
            10.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );

        let selected = select_fare_event_for_economy(
            Some(&authoritative),
            &strategic,
            Some(&projection),
            true,
        );

        assert_eq!(selected.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(selected.source_label, "simulation_owned_lifecycle");
        assert_eq!(selected.passenger_count, 40.0);
        assert_eq!(selected.completed_passenger_count, 35.0);
        assert_eq!(selected.fare_delta_base, 100.0);
        let diagnostic = fare_source_selection_diagnostic(&selected);
        assert_eq!(
            diagnostic.selected_provenance,
            CounterProvenance::AuthoritativeSim
        );
        assert_eq!(
            diagnostic.selected_source_label,
            "simulation_owned_lifecycle"
        );
        assert!(diagnostic.used_authoritative_sim);
        assert!(!diagnostic.used_strategic_estimate);
        assert!(!diagnostic.used_runtime_projection_fallback);
        let telemetry = runtime_fare_source_telemetry(&selected);
        assert_eq!(
            telemetry.selected_provenance,
            CounterProvenance::AuthoritativeSim
        );
        assert!(telemetry.used_authoritative_fare);
        assert!(!telemetry.fallback_used);
    }

    #[test]
    fn economy_selection_prefers_strategic_over_projection_fallback() {
        let strategic = strategic_lifecycle_events_from_totals(10.0, 8.0, 6.0, 19.0);
        let projection = collect_projection_lifecycle_events_fallback(
            10.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );

        let selected = select_fare_event_for_economy(None, &strategic, Some(&projection), true);

        assert_eq!(selected.provenance, CounterProvenance::StrategicEstimate);
        assert_eq!(selected.source_label, "strategic_model_fare_flow");
        assert_eq!(selected.fare_delta_base, 19.0);
        let diagnostic = fare_source_selection_diagnostic(&selected);
        assert_eq!(
            diagnostic.selected_provenance,
            CounterProvenance::StrategicEstimate
        );
        assert!(diagnostic.used_strategic_estimate);
        assert!(!diagnostic.used_runtime_projection_fallback);
        let telemetry = runtime_fare_source_telemetry(&selected);
        assert_eq!(
            telemetry.selected_provenance,
            CounterProvenance::StrategicEstimate
        );
        assert!(telemetry.used_strategic_fare_fallback);
        assert!(telemetry.fallback_used);
    }

    #[test]
    fn economy_selection_uses_projection_only_as_final_fallback() {
        let strategic = PassengerLifecycleEventBatch::from_events(Vec::new());
        let projection = collect_projection_lifecycle_events_fallback(
            10.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );

        let selected = select_fare_event_for_economy(None, &strategic, Some(&projection), true);

        assert_eq!(selected.provenance, CounterProvenance::RuntimeProjection);
        assert_eq!(selected.source_label, "desktop_projection_fare_events");
        assert_ne!(selected.provenance, CounterProvenance::AuthoritativeSim);
        let diagnostic = fare_source_selection_diagnostic(&selected);
        assert_eq!(
            diagnostic.selected_provenance,
            CounterProvenance::RuntimeProjection
        );
        assert_eq!(
            diagnostic.selected_source_label,
            "desktop_projection_fare_events"
        );
        assert!(diagnostic.used_runtime_projection_fallback);
        assert!(!diagnostic.used_authoritative_sim);
        let telemetry = runtime_fare_source_telemetry(&selected);
        assert_eq!(
            telemetry.selected_provenance,
            CounterProvenance::RuntimeProjection
        );
        assert_eq!(telemetry.selected_fare_delta_base, 31.25);
        assert_eq!(telemetry.selected_passenger_count, 12.5);
        assert!(telemetry.used_runtime_projection_fare_fallback);
        assert!(telemetry.fallback_used);
    }

    #[test]
    fn economy_selection_does_not_treat_authoritative_boarding_only_as_fare_truth() {
        let authoritative_boardings = authoritative_lifecycle_events_from_engine_fast_board_loads(
            10.0,
            "0.2.2-fast-operational-v1",
            &[test_board_load("svc:a", "stop:a", 14.0)],
            &[test_passenger_cohort("svc:a", "stop:a", "stop:b", 6.0)],
        )
        .unwrap();
        let strategic = strategic_lifecycle_events_from_totals(10.0, 8.0, 6.0, 19.0);
        let projection = collect_projection_lifecycle_events_fallback(
            10.0,
            RuntimeFareEvents {
                boarded_pax: 12.5,
                completed_alightings_pax: 9.0,
                liability_accrued_base: 31.25,
            },
        );

        let selected = select_fare_event_for_economy(
            Some(&authoritative_boardings),
            &strategic,
            Some(&projection),
            false,
        );

        assert_eq!(selected.provenance, CounterProvenance::StrategicEstimate);
        assert_eq!(selected.source_label, "strategic_model_fare_flow");
        assert_eq!(selected.fare_delta_base, 19.0);
    }

    #[test]
    fn completed_engine_fast_cohorts_emit_authoritative_fare_with_fare_basis() {
        let batch = authoritative_lifecycle_events_from_engine_fast_board_loads_with_fares(
            42.0,
            "fast-operational:v1",
            &[],
            &[test_passenger_cohort_with_fare(
                "svc-a", "stop-a", "stop-b", 11.0, 30.25,
            )],
        )
        .expect("completed cohort should produce lifecycle events");

        let fare_event = fare_event_from_lifecycle_events(&batch);

        assert_eq!(fare_event.provenance, CounterProvenance::AuthoritativeSim);
        assert_eq!(fare_event.passenger_count, 11.0);
        assert_eq!(fare_event.fare_delta_base, 30.25);
        assert_eq!(fare_event.source_label, "engine_fast_completed_cohort_fare");
    }

    #[test]
    fn completed_engine_fast_cohorts_without_fare_basis_remain_without_fare_authority() {
        let batch = authoritative_lifecycle_events_from_engine_fast_board_loads_with_fares(
            42.0,
            "fast-operational:v1",
            &[],
            &[test_passenger_cohort("svc-a", "stop-a", "stop-b", 11.0)],
        )
        .expect("completed cohort should still produce alighting lifecycle events");

        assert!(batch.is_authoritative_sim());
        assert!(!batch.has_authoritative_fare_recognition());
        assert_ne!(
            fare_event_from_lifecycle_events(&batch).provenance,
            CounterProvenance::AuthoritativeSim
        );
    }

    #[test]
    fn temporary_python_dependency_cache_is_gitignored() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let gitignore = fs::read_to_string(repo_root.join(".gitignore")).unwrap();

        assert!(gitignore.lines().any(|line| line.trim() == ".tmp_pydeps/"));
    }
}
