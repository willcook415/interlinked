mod common;

use interlinked_engine::sim::{run_simulation_with_settings, SimulationSettings};
use interlinked_engine::ScenarioService;

#[test]
fn lightweight_outputs_keep_authoritative_core_and_skip_heavy_bundles() {
    let scenario_path = common::fixture_path("scenario_demand_operations_phase6.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");

    let mut settings = SimulationSettings::from_params(&doc.scenario.params);
    settings.lightweight_outputs = true;
    settings.k_paths = 1;
    settings.msa_max_iters = 1;
    settings.convergence_rel = 1.0;

    let out = run_simulation_with_settings(&doc.scenario, &settings, None)
        .expect("lightweight simulation should succeed");

    assert!(
        out.meta.results_version.ends_with("-lite"),
        "lightweight output should be tagged"
    );
    assert!(
        !out.board_loads.is_empty(),
        "authoritative board loads must still be produced"
    );
    assert!(
        !out.stop_flow_states.is_empty(),
        "authoritative stop states must still be produced"
    );
    assert!(
        !out.vehicle_load_states.is_empty(),
        "authoritative vehicle load states must still be produced"
    );
    assert!(
        !out.assigned_od_flows.is_empty(),
        "assigned OD flows must still be produced"
    );
    assert!(
        out.zone_planning_metrics.is_empty()
            && out.station_planning_metrics.is_empty()
            && out.corridor_planning_metrics.is_empty(),
        "heavy planning bundles should be skipped in lightweight mode"
    );
    assert!(
        out.service_financial_metrics.is_empty() && out.corridor_financial_metrics.is_empty(),
        "heavy economic bundles should be skipped in lightweight mode"
    );
}

#[test]
fn metro_auto_assigned_owned_stock_generates_transit_cohorts() {
    let scenario_path = common::fixture_path("scenario_small_city.json");
    let mut doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");

    // Mirror the live metro-first flow: owned stock exists on the line, explicit assigned units
    // remain zero, and metro service should still be considered active for demand assignment.
    let service = doc
        .scenario
        .world
        .services
        .first_mut()
        .expect("fixture service should exist");
    service.mode = "metro".to_string();
    service.service_enabled = Some(true);
    service.operating_tph = Some(12.0);
    service.stock_units_owned = Some(1);
    service.stock_units_assigned = Some(0);
    let service_id = service.id.clone();

    let link = doc
        .scenario
        .world
        .links
        .first_mut()
        .expect("fixture link should exist");
    link.mode = "metro".to_string();
    for stop in &mut doc.scenario.world.stops {
        stop.stop_type = Some("metro_station".to_string());
    }

    let mut settings = SimulationSettings::from_params(&doc.scenario.params);
    settings.lightweight_outputs = true;
    settings.k_paths = 1;
    settings.msa_max_iters = 1;
    settings.convergence_rel = 1.0;

    let out = run_simulation_with_settings(&doc.scenario, &settings, None)
        .expect("lightweight simulation should succeed");

    let attempted_on_service = out
        .passenger_cohorts
        .iter()
        .filter(|cohort| cohort.service_id == service_id)
        .map(|cohort| cohort.attempted_pax.max(0.0))
        .sum::<f64>();
    assert!(
        attempted_on_service > 0.0,
        "metro service should receive non-zero attempted passenger cohorts when owned stock exists"
    );

    let boardings_attempted = out
        .board_loads
        .iter()
        .filter(|load| load.service_id == service_id)
        .map(|load| load.arrivals.max(0.0))
        .sum::<f64>();
    assert!(
        boardings_attempted > 0.0,
        "metro service should have non-zero board-load arrivals under auto-assign semantics"
    );
}
