mod common;

use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};

#[test]
fn planning_requires_deterministic_mode() {
    let scenario_path = common::fixture_path("scenario_small_city.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");

    let opts = PlanningRunOptions {
        deterministic_mode: false,
        ..Default::default()
    };
    let err = SimulationService::run_planning(&doc, opts)
        .expect_err("must reject non-deterministic mode");
    assert!(err.contains("deterministic_mode must be true"));
}

#[test]
fn planning_honors_seed_override() {
    let scenario_path = common::fixture_path("scenario_small_city.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");

    let opts = PlanningRunOptions {
        deterministic_seed: Some(999),
        ..Default::default()
    };
    let out = SimulationService::run_planning(&doc, opts).expect("planning run should succeed");
    assert_eq!(out.meta.seed, 999);
}
