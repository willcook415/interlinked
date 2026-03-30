mod common;

use interlinked_engine::sim::{
    AssignedOdFlow, DemandTimeSliceLabel, SeasonalProfile, ServiceDayType, TravelMode,
};
use interlinked_engine::{
    PlanningRunOptions, ScenarioService, SimulationOutput, SimulationService,
};
use std::collections::HashMap;

const EPS: f64 = 1e-6;

fn run_context(
    day: ServiceDayType,
    season: SeasonalProfile,
    time_of_day_s: f64,
) -> SimulationOutput {
    let scenario_path = common::fixture_path("scenario_demand_operations_phase6.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let opts = PlanningRunOptions {
        time_of_day_s: Some(time_of_day_s),
        service_day_type: Some(day),
        seasonal_profile: Some(season),
        active_event_ids: None,
        ..Default::default()
    };
    SimulationService::run_planning(&doc, opts).expect("planning run should succeed")
}

fn active_assigned(out: &SimulationOutput) -> impl Iterator<Item = &AssignedOdFlow> {
    let ctx = &out.active_temporal_slice;
    out.assigned_od_flows.iter().filter(move |x| {
        x.time_slice == ctx.time_slice
            && x.service_day_type == Some(ctx.service_day_type)
            && x.seasonal_profile == Some(ctx.seasonal_profile)
    })
}

#[test]
fn phase6_operational_outputs_are_present_and_temporally_coherent() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    assert!(
        !out.service_operation_states.is_empty(),
        "service operation states should be emitted"
    );
    assert!(
        !out.stop_operation_states.is_empty(),
        "stop operation states should be emitted"
    );
    assert!(
        !out.transfer_operation_metrics.is_empty(),
        "transfer operation metrics should be emitted"
    );
    assert!(
        !out.service_reliability_diagnostics
            .delay_by_service
            .is_empty(),
        "service reliability diagnostics should include delay rankings"
    );
    assert!(
        !out.service_reliability_diagnostics
            .worst_reliability_by_time_slice
            .is_empty(),
        "temporal reliability rankings should be emitted"
    );

    for svc in &out.service_operation_states {
        assert!(
            (0.0..=1.0).contains(&svc.reliability_score),
            "service reliability score should be in [0,1]"
        );
        assert!(
            svc.expected_headway_s >= 0.0,
            "expected headway should be non-negative"
        );
        assert!(
            svc.average_headway_realised_s >= 0.0,
            "realised headway should be non-negative"
        );
    }

    assert!(
        !out.service_gap_rankings.top_unreliable_services.is_empty(),
        "service-gap rankings should include unreliable service list"
    );
    assert!(
        !out.planning_debug_summary
            .top_corridors_losing_capture_due_to_unreliability
            .is_empty(),
        "planning debug should include reliability-loss corridors"
    );
}

#[test]
fn phase6_higher_station_activity_inflates_dwell_and_delay() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );
    let cfg = out
        .synthetic_economy_config
        .as_ref()
        .expect("synthetic config should be present")
        .operations_reliability_config
        .clone();

    let mut boardings_by_stop = out
        .stop_flow_states
        .iter()
        .map(|s| (s.stop_id.clone(), s.boarded_this_step.max(0.0)))
        .collect::<Vec<_>>();
    boardings_by_stop.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let busiest = boardings_by_stop
        .first()
        .expect("at least one stop should exist");
    let quietest = boardings_by_stop
        .last()
        .expect("at least one stop should exist");

    let busy_ops = out
        .stop_operation_states
        .iter()
        .find(|x| x.stop_id == busiest.0)
        .expect("busy stop should have operations state");
    let quiet_ops = out
        .stop_operation_states
        .iter()
        .find(|x| x.stop_id == quietest.0)
        .expect("quiet stop should have operations state");

    assert!(
        out.stop_operation_states
            .iter()
            .any(|x| x.average_dwell_time_s > cfg.base_dwell_station_s + 1.0),
        "at least one stop should exceed base dwell under demand pressure"
    );
    assert!(
        busy_ops.average_dwell_time_s >= quiet_ops.average_dwell_time_s,
        "busiest stop should not have lower average dwell than quietest stop"
    );

    let max_delay = out
        .service_operation_states
        .iter()
        .map(|x| x.max_delay_s.max(0.0))
        .fold(0.0_f64, f64::max);
    assert!(max_delay > 0.0, "dwell pressure should propagate to delay");
}

#[test]
fn phase6_missed_transfers_worsen_with_delay_pressure() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    let service_delay = out
        .service_operation_states
        .iter()
        .map(|x| (x.service_id.clone(), x.max_delay_s.max(0.0)))
        .collect::<HashMap<_, _>>();
    let mut delays = service_delay.values().copied().collect::<Vec<_>>();
    delays.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_delay = if delays.is_empty() {
        0.0
    } else {
        delays[delays.len() / 2]
    };

    let mut high_delay_weight = 0.0_f64;
    let mut high_delay_missed = 0.0_f64;
    let mut low_delay_weight = 0.0_f64;
    let mut low_delay_missed = 0.0_f64;

    for t in &out.transfer_operation_metrics {
        let from_delay = service_delay
            .get(&t.from_service_id)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let weight = (t.missed_transfer_count.max(0.0) + 1.0).max(1.0);
        if from_delay >= median_delay {
            high_delay_weight += weight;
            high_delay_missed += t.missed_transfer_rate.max(0.0) * weight;
        } else {
            low_delay_weight += weight;
            low_delay_missed += t.missed_transfer_rate.max(0.0) * weight;
        }
    }

    let high_delay_rate = if high_delay_weight > 0.0 {
        high_delay_missed / high_delay_weight
    } else {
        0.0
    };
    let low_delay_rate = if low_delay_weight > 0.0 {
        low_delay_missed / low_delay_weight
    } else {
        0.0
    };

    assert!(
        out.transfer_operation_metrics
            .iter()
            .any(|x| x.missed_transfer_rate > 0.0),
        "there should be non-zero missed-transfer risk under operational pressure"
    );
    assert!(
        high_delay_rate + EPS >= low_delay_rate,
        "higher-delay feeders should have at least as much missed-transfer risk"
    );
}

#[test]
fn phase6_operational_penalties_are_reflected_in_mode_choice_and_corridors() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    let max_reliability_penalty = out
        .mode_choice_results
        .iter()
        .flat_map(|r| r.generalized_costs_by_mode.iter())
        .filter(|g| g.mode == TravelMode::OtherTransit)
        .map(|g| g.breakdown.reliability_penalty_s.max(0.0))
        .fold(0.0_f64, f64::max);
    assert!(
        max_reliability_penalty > 0.0,
        "transit generalized cost should include non-zero reliability penalties"
    );

    let worst = out
        .corridor_planning_metrics
        .iter()
        .max_by(|a, b| {
            a.recurring_bottleneck_score
                .partial_cmp(&b.recurring_bottleneck_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("corridor planning metrics should exist");
    assert!(
        worst.reliability_adjusted_service_quality <= 1.0,
        "reliability-adjusted service quality should be bounded"
    );
    assert!(
        worst.transit_capture_gap >= 0.0,
        "corridor transit capture gap should remain non-negative"
    );

    assert!(
        !out.service_gap_rankings
            .top_corridors_losing_capture_due_to_unreliability
            .is_empty(),
        "service-gap rankings should expose unreliability-driven capture loss corridors"
    );
}

#[test]
fn phase6_preserves_authoritative_accounting_with_operations_layer() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    let total_transit_capture = out
        .mode_choice_results
        .iter()
        .map(|x| x.transit_captured_passengers.max(0.0))
        .sum::<f64>();
    let total_assigned = active_assigned(&out)
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        total_assigned <= total_transit_capture + 1e-5,
        "assigned transit passengers should not exceed transit-captured mode-choice demand"
    );

    let station_boarded_total = out
        .stop_flow_states
        .iter()
        .map(|x| x.boarded_this_step.max(0.0))
        .sum::<f64>();
    let vehicle_boarded_total = out
        .vehicle_load_states
        .iter()
        .map(|x| x.boardings_this_stop.max(0.0))
        .sum::<f64>();
    assert!(
        (station_boarded_total - vehicle_boarded_total).abs() <= 1e-6,
        "station and vehicle boarded totals should remain reconciled"
    );

    assert!(
        out.demand_diagnostics
            .consistency_checks
            .iter()
            .all(|c| c.passed),
        "all core consistency checks should remain valid after operations realism"
    );

    assert!(
        out.active_temporal_slice.time_slice == DemandTimeSliceLabel::AmPeak,
        "test context should run AM-peak for operational pressure"
    );
}
