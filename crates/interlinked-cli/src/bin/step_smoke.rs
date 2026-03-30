use std::fs;

use interlinked_engine::model::Scenario;
use interlinked_engine::sim::{init_sim_state, step_simulation, RunConfig};

fn main() -> Result<(), String> {
    // 1) Load your scenario JSON (adjust path if your CLI runs from a different CWD)
    let scenario_path = "data/leeds_v0/scenario.json";
    let text = fs::read_to_string(scenario_path)
        .map_err(|e| format!("Failed to read {scenario_path}: {e}"))?;
    let scenario: Scenario =
        serde_json::from_str(&text).map_err(|e| format!("Failed to parse scenario.json: {e}"))?;

    // 2) Config + state
    let cfg = RunConfig::default();
    let mut state = init_sim_state(&scenario, &cfg);

    // 3) Step loop
    let dt_s = 60.0; // 1-minute steps
    let steps = 60; // 60 minutes total

    println!("Loaded scenario. Starting step test: dt_s={dt_s}, steps={steps}");

    for i in 0..steps {
        let (out, next) = step_simulation(&scenario, &cfg, &state, dt_s)?;

        // ---- sanity checks (hard fail if violated) ----
        // KPIs should be finite
        if !out.kpis.total_trips.is_finite() || !out.kpis.mean_generalized_cost_s.is_finite() {
            return Err(format!(
                "Non-finite KPI at step {i}: total_trips={}, mean_generalized_cost_s={}",
                out.kpis.total_trips, out.kpis.mean_generalized_cost_s
            ));
        }

        // queues must be finite & non-negative
        for ((svc, stop), q) in next.queue.iter() {
            if !q.is_finite() || *q < 0.0 {
                return Err(format!("Bad queue at step {i} ({svc} @ {stop}): {q}"));
            }
        }

        // time-to-next-departure must be finite & non-negative
        for ((svc, stop), ttn) in next.time_to_next_departure_s.iter() {
            if !ttn.is_finite() || *ttn < 0.0 {
                return Err(format!(
                    "Bad time_to_next_departure at step {i} ({svc} @ {stop}): {ttn}"
                ));
            }
        }

        // ---- print a compact line so you can eyeball behaviour ----
        println!(
            "step {:>3}  t={:>6.0}s  trips={:>10.2}  served={:>6.2}%  mean_gc={:>8.1}s  ivt={:>7.1}s  wait={:>7.1}s  walk={:>7.1}s  denied_board={:>10.2}",
            i + 1,
            next.t_s,
            out.kpis.total_trips,
            100.0 * out.kpis.share_trips_served,
            out.kpis.mean_generalized_cost_s,
            out.kpis.mean_in_vehicle_time_s,
            out.kpis.mean_wait_time_s,
            out.kpis.mean_walk_time_s,
            out.kpis.total_boardings_denied
        );

        state = next;
    }

    println!("✅ Step smoke test passed.");
    Ok(())
}
