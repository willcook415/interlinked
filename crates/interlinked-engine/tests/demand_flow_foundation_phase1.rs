mod common;

use std::collections::BTreeSet;

use interlinked_engine::sim::DemandTimeSliceLabel;
use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};

const EPS: f64 = 1e-6;

#[test]
fn phase1_foundation_emits_authoritative_flow_outputs() {
    let scenario_path = common::fixture_path("scenario_demand_flow_phase1.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    assert!(
        !out.zone_demand_profiles.is_empty(),
        "zone demand profiles should be produced"
    );
    assert!(
        !out.latent_od_demand.is_empty(),
        "latent OD demand should be produced"
    );
    assert!(
        !out.assigned_od_flows.is_empty(),
        "assigned OD flows should be produced"
    );
    assert!(
        !out.stop_flow_states.is_empty(),
        "stop flow states should be produced"
    );
    assert!(
        !out.vehicle_load_states.is_empty(),
        "vehicle load states should be produced"
    );
    assert!(
        !out.zone_demand_layer.is_empty(),
        "zone demand layer output should be produced"
    );
    assert!(
        !out.service_load_layer.is_empty(),
        "service load layer output should be produced"
    );

    let active_latent = out
        .latent_od_demand
        .iter()
        .filter(|x| x.time_slice == DemandTimeSliceLabel::Interpeak)
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>();
    let total_realised = out
        .assigned_od_flows
        .iter()
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    let total_unserved = out
        .assigned_od_flows
        .iter()
        .map(|x| x.unserved_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        (active_latent - (total_realised + total_unserved)).abs() <= EPS,
        "active-slice latent demand should reconcile with assigned + unserved"
    );

    for v in &out.vehicle_load_states {
        assert!(v.current_load >= -EPS, "vehicle load must be non-negative");
        assert!(
            v.current_load <= v.capacity + EPS,
            "vehicle load must not exceed effective capacity"
        );
    }

    let mut slices = BTreeSet::new();
    for od in &out.latent_od_demand {
        slices.insert(od.time_slice);
    }
    assert_eq!(
        slices.len(),
        6,
        "all six canonical time slices should be present"
    );
}

#[test]
fn phase1_foundation_tracks_waiting_denials_and_unserved() {
    let scenario_path = common::fixture_path("scenario_demand_flow_phase1.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    let total_waiting = out.demand_diagnostics.total_waiting_passengers_network;
    let total_unserved = out.demand_diagnostics.total_unserved_demand;
    let any_denied = out
        .stop_flow_states
        .iter()
        .any(|x| x.denied_this_step > 0.0 || x.total_waiting > 0.0);
    let any_modal_diversion = out
        .mode_choice_results
        .iter()
        .any(|x| x.transit_captured_passengers + EPS < x.latent_passengers);

    assert!(
        total_waiting > 0.0 || total_unserved > 0.0,
        "the constrained fixture should surface waiting or unserved demand"
    );
    assert!(
        out.assigned_od_flows
            .iter()
            .any(|x| x.unserved_passengers > 0.0),
        "at least one OD pair should be unserved"
    );
    assert!(
        any_denied || any_modal_diversion,
        "scenario should surface denied/waiting pressure or modal diversion away from transit"
    );

    let checks = &out.demand_diagnostics.consistency_checks;
    assert!(!checks.is_empty(), "consistency checks should be emitted");
    for check in checks {
        assert!(
            check.passed,
            "consistency check should pass: {}",
            check.name
        );
    }
}
