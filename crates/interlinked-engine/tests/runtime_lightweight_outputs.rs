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
