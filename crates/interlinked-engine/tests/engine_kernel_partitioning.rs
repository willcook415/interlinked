mod common;

use interlinked_engine::{
    GameStepRequest, ScenarioService, SimulationScope, SimulationService, StrategicRefreshReason,
};

fn load_fixture() -> interlinked_engine::ScenarioDocument {
    let scenario_path = common::fixture_path("scenario_small_city.json");
    ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("fixture should load")
}

#[test]
fn fast_operational_ticks_reuse_cached_strategic_state() {
    let doc = load_fixture();
    let mut state = SimulationService::init_game_state(&doc);
    state.run_cfg.enable_kernel_partitioning = true;
    state.run_cfg.strategic_refresh_interval_steps = 8;

    let req = GameStepRequest {
        recompute_quick_kpis: true,
        edits: Vec::new(),
        force_strategic_refresh: false,
    };
    let first = SimulationService::step_game_scoped(
        &mut state,
        30.0,
        req.clone(),
        &SimulationScope::default(),
    )
    .expect("first step should succeed");
    assert!(
        first.strategic_refresh_executed,
        "first step should build strategic cache"
    );
    assert_eq!(first.kernel_perf.strategic_steps, 1);

    let second =
        SimulationService::step_game_scoped(&mut state, 30.0, req, &SimulationScope::default())
            .expect("second step should succeed");
    assert!(
        !second.strategic_refresh_executed,
        "second step should run fast operational kernel"
    );
    assert_eq!(second.kernel_perf.strategic_steps, 1);
    assert!(
        second.kernel_perf.fast_steps >= 1,
        "fast-step counter should increase on cached path"
    );
    assert!(
        second.kernel_perf.strategic_cache_hits >= 1,
        "cache-hit counter should increase on cached path"
    );
}

#[test]
fn strategic_refresh_is_triggered_by_cadence_and_force() {
    let doc = load_fixture();
    let mut state = SimulationService::init_game_state(&doc);
    state.run_cfg.enable_kernel_partitioning = true;
    state.run_cfg.strategic_refresh_interval_steps = 2;

    let regular_req = GameStepRequest {
        recompute_quick_kpis: true,
        edits: Vec::new(),
        force_strategic_refresh: false,
    };

    let first = SimulationService::step_game_scoped(
        &mut state,
        20.0,
        regular_req.clone(),
        &SimulationScope::default(),
    )
    .expect("first step should succeed");
    assert!(first.strategic_refresh_executed);
    assert_eq!(
        first.strategic_refresh_reason,
        Some(StrategicRefreshReason::MissingCache)
    );

    let second = SimulationService::step_game_scoped(
        &mut state,
        20.0,
        regular_req.clone(),
        &SimulationScope::default(),
    )
    .expect("second step should succeed");
    assert!(!second.strategic_refresh_executed);

    let third = SimulationService::step_game_scoped(
        &mut state,
        20.0,
        regular_req.clone(),
        &SimulationScope::default(),
    )
    .expect("third step should succeed");
    assert!(!third.strategic_refresh_executed);

    let fourth = SimulationService::step_game_scoped(
        &mut state,
        20.0,
        regular_req,
        &SimulationScope::default(),
    )
    .expect("fourth step should succeed");
    assert!(fourth.strategic_refresh_executed);
    assert_eq!(
        fourth.strategic_refresh_reason,
        Some(StrategicRefreshReason::CadenceInterval)
    );

    let force_req = GameStepRequest {
        recompute_quick_kpis: true,
        edits: Vec::new(),
        force_strategic_refresh: true,
    };
    let forced = SimulationService::step_game_scoped(
        &mut state,
        20.0,
        force_req,
        &SimulationScope::default(),
    )
    .expect("forced step should succeed");
    assert!(forced.strategic_refresh_executed);
    assert_eq!(
        forced.strategic_refresh_reason,
        Some(StrategicRefreshReason::ExplicitForce)
    );
}
