use std::fs;

use interlinked_engine::model::Scenario;
use interlinked_engine::sim::{
    init_sim_state, step_simulation, HistoryConfig, RunConfig, SimHistory,
};

fn main() -> Result<(), String> {
    let scenario_path = "data/leeds_v0/scenario.json";
    let text = fs::read_to_string(scenario_path)
        .map_err(|e| format!("Failed to read {scenario_path}: {e}"))?;
    let scenario: Scenario =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse scenario.json: {e}"))?;

    let cfg = RunConfig::default();
    let mut state = init_sim_state(&scenario, &cfg);

    let mut hist = SimHistory::new(HistoryConfig {
        enabled: true,
        max_frames: 500,
        record_board_loads: true,
        record_queue_map: false,
    });

    let dt_s = 60.0;
    let steps = 30;

    for _ in 0..steps {
        let (out, next) = step_simulation(&scenario, &cfg, &state, dt_s)?;
        state = next;
        hist.push(&out, &state);
    }

    // Dump to JSON for inspection
    let json = serde_json::to_string_pretty(&hist)
        .map_err(|e| format!("failed to serialize history: {e}"))?;

    fs::write("history_dump.json", json)
        .map_err(|e| format!("failed to write history_dump.json: {e}"))?;

    println!(
        "✅ history_smoke ok. Wrote history_dump.json with {} frames.",
        hist.len()
    );
    Ok(())
}
