mod common;

use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};
use serde::Deserialize;
use std::collections::HashMap;

const EPS_SECONDS: f64 = 1e-6;
const EPS_RATIO: f64 = 1e-9;

#[derive(Debug, Deserialize)]
struct ExpectedScenario {
    scenario_name: String,
    results_version: String,
    zones: usize,
    stops: usize,
    links: usize,
    services: usize,
    transfers: usize,
    total_trips_attempted: f64,
    total_trips_served: f64,
    share_trips_served: f64,
    mean_generalized_cost_s: f64,
    total_boardings_denied: f64,
}

#[test]
fn fixture_pack_regression_contract_is_stable() {
    let expected: HashMap<String, ExpectedScenario> = serde_json::from_str(
        &std::fs::read_to_string(common::golden_path("platform_contract_expected.json"))
            .expect("must read platform contract golden"),
    )
    .expect("golden json should parse");

    run_case(
        "scenario_small_city.json",
        expected.get("small").expect("missing small expected"),
    );
    run_case(
        "scenario_medium_city.json",
        expected.get("medium").expect("missing medium expected"),
    );
    run_case(
        "scenario_disrupted_network.json",
        expected
            .get("disrupted")
            .expect("missing disrupted expected"),
    );
}

fn run_case(fixture_name: &str, expected: &ExpectedScenario) {
    let scenario_path = common::fixture_path(fixture_name);
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("fixture should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    assert_eq!(out.meta.scenario_name, expected.scenario_name);
    assert_eq!(out.meta.results_version, expected.results_version);
    assert_eq!(out.diagnostics.zones, expected.zones);
    assert_eq!(out.diagnostics.stops, expected.stops);
    assert_eq!(out.diagnostics.links, expected.links);
    assert_eq!(out.diagnostics.services, expected.services);
    assert_eq!(out.diagnostics.transfers, expected.transfers);

    common::assert_abs_close(
        out.kpis.total_trips_attempted,
        expected.total_trips_attempted,
        EPS_SECONDS,
        "total_trips_attempted",
    );
    common::assert_abs_close(
        out.kpis.total_trips_served,
        expected.total_trips_served,
        EPS_SECONDS,
        "total_trips_served",
    );
    common::assert_abs_close(
        out.kpis.share_trips_served,
        expected.share_trips_served,
        EPS_RATIO,
        "share_trips_served",
    );
    common::assert_abs_close(
        out.kpis.mean_generalized_cost_s,
        expected.mean_generalized_cost_s,
        EPS_SECONDS,
        "mean_generalized_cost_s",
    );
    common::assert_abs_close(
        out.kpis.total_boardings_denied,
        expected.total_boardings_denied,
        EPS_SECONDS,
        "total_boardings_denied",
    );
}
