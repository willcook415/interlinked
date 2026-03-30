mod common;

use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};
use serde::Deserialize;

const EPS_SECONDS: f64 = 1e-6;
const EPS_RATIO: f64 = 1e-9;

#[derive(Debug, Deserialize)]
struct ExpectedSnapshot {
    meta: ExpectedMeta,
    diagnostics: ExpectedDiagnostics,
    kpis: ExpectedKpis,
    link_loads: Vec<ExpectedLinkLoad>,
    board_loads: Vec<ExpectedBoardLoad>,
}

#[derive(Debug, Deserialize)]
struct ExpectedMeta {
    scenario_name: String,
    results_version: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedDiagnostics {
    zones: usize,
    stops: usize,
    links: usize,
    services: usize,
    transfers: usize,
    access_edges: usize,
    egress_edges: usize,
}

#[derive(Debug, Deserialize)]
struct ExpectedKpis {
    total_trips_attempted: f64,
    total_trips_served: f64,
    share_trips_served: f64,
    mean_generalized_cost_s: f64,
    mean_in_vehicle_time_s: f64,
    mean_wait_time_s: f64,
    mean_walk_time_s: f64,
    total_boardings_denied: f64,
}

#[derive(Debug, Deserialize)]
struct ExpectedLinkLoad {
    link_id: String,
    passengers: f64,
    load_to_capacity: f64,
    crowding_penalty_s: f64,
}

#[derive(Debug, Deserialize)]
struct ExpectedBoardLoad {
    service_id: String,
    stop_id: String,
    arrivals: f64,
    denied_boardings: f64,
    queue_end: f64,
    extra_wait_s: f64,
}

#[test]
fn planning_output_is_deterministic_and_matches_golden_snapshot() {
    let scenario_path = common::fixture_path("wrapped_transfer_capacity_valid.json");
    let expected_path = common::golden_path("wrapped_transfer_capacity_expected.json");

    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");

    let run_a = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run A should succeed");
    let run_b = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run B should succeed");

    // Determinism check: key numeric outputs should be stable between runs.
    common::assert_abs_close(
        run_a.kpis.total_trips_attempted,
        run_b.kpis.total_trips_attempted,
        EPS_SECONDS,
        "determinism total_trips_attempted",
    );
    common::assert_abs_close(
        run_a.kpis.total_trips_served,
        run_b.kpis.total_trips_served,
        EPS_SECONDS,
        "determinism total_trips_served",
    );
    common::assert_abs_close(
        run_a.kpis.share_trips_served,
        run_b.kpis.share_trips_served,
        EPS_RATIO,
        "determinism share_trips_served",
    );

    let expected: ExpectedSnapshot = serde_json::from_str(
        &std::fs::read_to_string(expected_path).expect("should read golden snapshot"),
    )
    .expect("golden snapshot must be valid json");

    assert_eq!(run_a.meta.scenario_name, expected.meta.scenario_name);
    assert_eq!(run_a.meta.results_version, expected.meta.results_version);

    assert_eq!(run_a.diagnostics.zones, expected.diagnostics.zones);
    assert_eq!(run_a.diagnostics.stops, expected.diagnostics.stops);
    assert_eq!(run_a.diagnostics.links, expected.diagnostics.links);
    assert_eq!(run_a.diagnostics.services, expected.diagnostics.services);
    assert_eq!(run_a.diagnostics.transfers, expected.diagnostics.transfers);
    assert_eq!(
        run_a.diagnostics.access_edges,
        expected.diagnostics.access_edges
    );
    assert_eq!(
        run_a.diagnostics.egress_edges,
        expected.diagnostics.egress_edges
    );

    common::assert_abs_close(
        run_a.kpis.total_trips_attempted,
        expected.kpis.total_trips_attempted,
        EPS_SECONDS,
        "kpi total_trips_attempted",
    );
    common::assert_abs_close(
        run_a.kpis.total_trips_served,
        expected.kpis.total_trips_served,
        EPS_SECONDS,
        "kpi total_trips_served",
    );
    common::assert_abs_close(
        run_a.kpis.share_trips_served,
        expected.kpis.share_trips_served,
        EPS_RATIO,
        "kpi share_trips_served",
    );
    common::assert_abs_close(
        run_a.kpis.mean_generalized_cost_s,
        expected.kpis.mean_generalized_cost_s,
        EPS_SECONDS,
        "kpi mean_generalized_cost_s",
    );
    common::assert_abs_close(
        run_a.kpis.mean_in_vehicle_time_s,
        expected.kpis.mean_in_vehicle_time_s,
        EPS_SECONDS,
        "kpi mean_in_vehicle_time_s",
    );
    common::assert_abs_close(
        run_a.kpis.mean_wait_time_s,
        expected.kpis.mean_wait_time_s,
        EPS_SECONDS,
        "kpi mean_wait_time_s",
    );
    common::assert_abs_close(
        run_a.kpis.mean_walk_time_s,
        expected.kpis.mean_walk_time_s,
        EPS_SECONDS,
        "kpi mean_walk_time_s",
    );
    common::assert_abs_close(
        run_a.kpis.total_boardings_denied,
        expected.kpis.total_boardings_denied,
        EPS_SECONDS,
        "kpi total_boardings_denied",
    );

    for link_expected in &expected.link_loads {
        let actual = run_a
            .link_loads
            .iter()
            .find(|l| l.link_id == link_expected.link_id)
            .unwrap_or_else(|| panic!("missing link_id {}", link_expected.link_id));

        common::assert_abs_close(
            actual.passengers,
            link_expected.passengers,
            EPS_SECONDS,
            &format!("link {} passengers", link_expected.link_id),
        );
        common::assert_abs_close(
            actual.load_to_capacity,
            link_expected.load_to_capacity,
            EPS_RATIO,
            &format!("link {} load_to_capacity", link_expected.link_id),
        );
        common::assert_abs_close(
            actual.crowding_penalty_s,
            link_expected.crowding_penalty_s,
            EPS_SECONDS,
            &format!("link {} crowding_penalty_s", link_expected.link_id),
        );
    }

    for board_expected in &expected.board_loads {
        let actual = run_a
            .board_loads
            .iter()
            .find(|b| {
                b.service_id == board_expected.service_id && b.stop_id == board_expected.stop_id
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing board load {}:{}",
                    board_expected.service_id, board_expected.stop_id
                )
            });

        common::assert_abs_close(
            actual.arrivals,
            board_expected.arrivals,
            EPS_SECONDS,
            &format!(
                "board {}:{} arrivals",
                board_expected.service_id, board_expected.stop_id
            ),
        );
        common::assert_abs_close(
            actual.denied_boardings,
            board_expected.denied_boardings,
            EPS_SECONDS,
            &format!(
                "board {}:{} denied_boardings",
                board_expected.service_id, board_expected.stop_id
            ),
        );
        common::assert_abs_close(
            actual.queue_end,
            board_expected.queue_end,
            EPS_SECONDS,
            &format!(
                "board {}:{} queue_end",
                board_expected.service_id, board_expected.stop_id
            ),
        );
        common::assert_abs_close(
            actual.extra_wait_s,
            board_expected.extra_wait_s,
            EPS_SECONDS,
            &format!(
                "board {}:{} extra_wait_s",
                board_expected.service_id, board_expected.stop_id
            ),
        );
    }
}
