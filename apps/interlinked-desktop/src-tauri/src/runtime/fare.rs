use crate::*;

pub(crate) fn runtime_fare_base_per_boarding(policy: &FarePolicyManifest, mode: &str) -> f64 {
    if !policy.enabled {
        return 0.0;
    }
    match fare_mode_bucket_from_tokens(mode, None, 0.0) {
        FareModeBucket::Bus => policy.fare_mode_bus_base.max(0.0),
        FareModeBucket::Tram => policy.fare_mode_tram_base.max(0.0),
        FareModeBucket::Metro => policy.fare_mode_metro_base.max(0.0),
        FareModeBucket::Rail => policy.fare_mode_rail_base.max(0.0),
        FareModeBucket::Ferry => policy.fare_mode_ferry_base.max(0.0),
        FareModeBucket::Default => policy.fare_mode_default_base.max(0.0),
    }
}

pub(crate) fn fare_flow_for_economy(
    gs: &interlinked_engine::platform::GameState,
) -> (f64, f64, f64) {
    let Some(output) = gs.last_output.as_ref() else {
        return (0.0, 0.0, 0.0);
    };
    let liability_base = output.fare_flow.liability_accrued_base.max(0.0);
    let liability_pax = output.fare_flow.liability_accrued_pax.max(0.0);
    let completed_pax = output.fare_flow.completed_journeys_pax.max(0.0);
    if liability_base > 0.0 || liability_pax > 0.0 || completed_pax > 0.0 {
        return (liability_base, liability_pax, completed_pax);
    }

    // Backward-compatible fallback for older outputs without fare_flow population.
    let mut fallback_completed = 0.0_f64;
    for load in &output.board_loads {
        let alightings = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        if alightings <= 0.0 {
            continue;
        }
        if load.departures_observed > 0 {
            fallback_completed += alightings;
        }
    }
    (
        output.kpis.total_fare_revenue_base.max(0.0),
        output.kpis.total_boardings_served.max(0.0),
        fallback_completed.max(0.0),
    )
}
