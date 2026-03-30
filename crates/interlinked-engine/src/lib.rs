pub mod io;
pub mod model;
pub mod sim;

// NEW: platform façade
pub mod platform;

// Keep old exports for compatibility, but prefer platform::* in new code
pub use io::{load_scenario_from_path, write_json_to_path};
pub use sim::{
    compare_outputs, run_simulation, run_simulation_with_settings,
    run_simulation_with_settings_and_context, SimulationDelta, SimulationOutput,
    SimulationSettings,
};

// Platform exports (the “real API” going forward)
pub use platform::{
    countries_in_scenario, default_economy_config, estimate_network_capex_base,
    estimate_service_opex_per_hour_base, from_base_currency, normalize_currency_code,
    planning_economy_kpis, scenario_network_stats, snapshot, to_base_currency, ComparisonService,
    EconomyConfig, EconomyKpis, EconomySnapshot, EconomyState, GameState, GameStepOutput,
    GameStepRequest, KernelPerfMetrics, NetworkEdit, NetworkStore, PlanningRunOptions,
    ScenarioDocument, ScenarioDocumentWire, ScenarioError, ScenarioFileShape, ScenarioService,
    ScenarioStore, SimulationScope, SimulationService, StrategicRefreshReason, WorldStore,
};
