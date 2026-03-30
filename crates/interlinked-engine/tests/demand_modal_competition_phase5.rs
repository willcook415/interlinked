mod common;

use interlinked_engine::sim::{
    AssignedOdFlow, DemandTimeSliceLabel, SeasonalProfile, ServiceDayType, TravelMode, TripPurpose,
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
    let scenario_path = common::fixture_path("scenario_demand_modal_phase5.json");
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

fn od_mode_shares(
    out: &SimulationOutput,
    origin: &str,
    destination: &str,
    purpose: TripPurpose,
) -> (f64, f64, f64, f64, f64) {
    let mut latent = 0.0_f64;
    let mut transit = 0.0_f64;
    let mut car = 0.0_f64;
    let mut walk = 0.0_f64;
    let mut suppressed = 0.0_f64;
    for r in &out.mode_choice_results {
        if r.context.origin_zone_id == origin
            && r.context.destination_zone_id == destination
            && r.context.purpose == purpose
        {
            latent += r.latent_passengers.max(0.0);
            transit += r.transit_captured_passengers.max(0.0);
            car += r.car_captured_passengers.max(0.0);
            walk += r.walk_captured_passengers.max(0.0);
            suppressed += r.suppressed_or_no_trip_passengers.max(0.0);
        }
    }
    if latent <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    }
    (
        latent,
        transit / latent,
        car / latent,
        walk / latent,
        suppressed / latent,
    )
}

#[test]
fn phase5_modal_outputs_are_present_and_coherent() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );

    assert!(
        !out.mode_choice_results.is_empty(),
        "mode choice results should be emitted"
    );
    assert!(
        !out.zone_mode_share_metrics.is_empty(),
        "zone mode share metrics should be emitted"
    );
    assert!(
        !out.corridor_mode_share_metrics.is_empty(),
        "corridor mode share metrics should be emitted"
    );
    assert!(
        !out.station_transit_capture_context.is_empty(),
        "station transit-capture context should be emitted"
    );
    assert!(
        !out.service_transit_capture_context.is_empty(),
        "service transit-capture context should be emitted"
    );
    assert!(
        !out.modal_demand_diagnostics
            .mode_share_by_day_type
            .is_empty(),
        "modal diagnostics should include day-type mode shares"
    );

    for result in &out.mode_choice_results {
        let share_sum = result
            .chosen_mode_shares
            .iter()
            .map(|x| x.share.max(0.0))
            .sum::<f64>();
        assert!(
            (share_sum - 1.0).abs() <= 1e-5,
            "mode shares should sum to 1.0 per OD, got {share_sum:.6}"
        );
    }

    for z in &out.zone_mode_share_metrics {
        let sum = z.transit_share + z.car_share + z.walk_share + z.suppressed_share;
        assert!(sum <= 1.0 + 1e-5, "zone mode-share sum should be <= 1.0");
        assert!(
            z.transit_share >= -EPS,
            "zone transit share must be non-negative"
        );
    }
}

#[test]
fn phase5_modal_competition_reflects_geography_and_distance() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );

    let (_latent_urban, urban_transit, urban_car, _urban_walk, _urban_no_trip) = od_mode_shares(
        &out,
        "east_suburb_dense",
        "city_core_cbd",
        TripPurpose::Work,
    );
    let (_latent_rural, rural_transit, rural_car, _rural_walk, _rural_no_trip) = od_mode_shares(
        &out,
        "village_centre",
        "regional_town_centre",
        TripPurpose::Essential,
    );
    let (_latent_intercity, intercity_transit, intercity_car, _intercity_walk, _) = od_mode_shares(
        &out,
        "city_core_cbd",
        "regional_town_centre",
        TripPurpose::Intercity,
    );
    let (_latent_short, _short_transit, short_car, short_walk, _) = od_mode_shares(
        &out,
        "city_core_cbd",
        "university_district",
        TripPurpose::Shopping,
    );

    assert!(
        urban_transit > urban_car,
        "dense commuter radial should favor transit over car"
    );
    assert!(
        rural_car > rural_transit,
        "rural essential connector should remain more car-dominated than transit"
    );
    assert!(
        intercity_transit > intercity_car,
        "strong rail city-pair corridor should win meaningful transit share"
    );
    assert!(
        short_walk > short_car,
        "sufficiently short local shopping trip should favor walking over car"
    );
    assert!(
        out.citywide_mode_share_summary.urban_transit_share
            > out.citywide_mode_share_summary.rural_transit_share,
        "urban transit share should exceed rural transit share"
    );
}

#[test]
fn phase5_crowding_pressure_rankings_and_capture_signals_are_authoritative() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        20.0 * 3600.0,
    );

    let max_transit_crowding_penalty = out
        .mode_choice_results
        .iter()
        .flat_map(|r| r.generalized_costs_by_mode.iter())
        .filter(|g| g.mode == TravelMode::OtherTransit)
        .map(|g| g.breakdown.crowding_penalty_s.max(0.0))
        .fold(0.0_f64, f64::max);
    assert!(
        max_transit_crowding_penalty > 0.0,
        "at least one OD should carry non-zero crowding penalty in transit generalized cost"
    );

    let denied_total = out
        .stop_flow_states
        .iter()
        .map(|s| s.denied_this_step.max(0.0))
        .sum::<f64>();
    assert!(
        denied_total > 0.0,
        "fixture should produce denied boardings under constrained services"
    );

    assert!(
        !out.modal_demand_diagnostics
            .top_overcrowded_corridors_losing_mode_share
            .is_empty(),
        "modal diagnostics should rank overcrowded corridors losing mode share"
    );

    let top = out
        .modal_demand_diagnostics
        .top_overcrowded_corridors_losing_mode_share
        .first()
        .expect("top overcrowded corridor should exist");
    let mut parts = top.id.split("->");
    let oz = parts.next().unwrap_or_default();
    let dz = parts.next().unwrap_or_default();
    assert!(
        out.corridor_mode_share_metrics.iter().any(|c| {
            c.origin_zone_id == oz
                && c.destination_zone_id == dz
                && c.transit_capture_gap > 0.0
                && c.car_share >= 0.0
        }),
        "overcrowded-corridor ranking should correspond to authoritative corridor mode-share rows"
    );
}

#[test]
fn phase5_mode_choice_preserves_transit_backbone_accounting() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );

    let mut transit_capture_by_od = HashMap::<(String, String, TripPurpose), f64>::new();
    for row in &out.mode_choice_results {
        *transit_capture_by_od
            .entry((
                row.context.origin_zone_id.clone(),
                row.context.destination_zone_id.clone(),
                row.context.purpose,
            ))
            .or_insert(0.0) += row.transit_captured_passengers.max(0.0);
    }
    for flow in active_assigned(&out) {
        let key = (
            flow.origin_zone_id.clone(),
            flow.destination_zone_id.clone(),
            flow.purpose,
        );
        let captured = transit_capture_by_od.get(&key).copied().unwrap_or(0.0);
        assert!(
            flow.assigned_passengers <= captured + 1e-6,
            "assigned transit passengers must not exceed transit-captured demand for {:?}",
            key
        );
    }

    let latent_total = out
        .mode_choice_results
        .iter()
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>();
    let captured_total = out
        .mode_choice_results
        .iter()
        .map(|x| {
            x.transit_captured_passengers.max(0.0)
                + x.car_captured_passengers.max(0.0)
                + x.walk_captured_passengers.max(0.0)
                + x.suppressed_or_no_trip_passengers.max(0.0)
        })
        .sum::<f64>();
    assert!(
        (latent_total - captured_total).abs() <= 1e-5,
        "mode-choice conservation should hold: latent == transit+car+walk+suppressed"
    );

    let active_assigned_total = active_assigned(&out)
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    let active_unserved_total = active_assigned(&out)
        .map(|x| x.unserved_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        (latent_total - (active_assigned_total + active_unserved_total)).abs() <= 1e-5,
        "active context should still conserve latent == assigned + unserved"
    );

    let transit_captured_total = out
        .mode_choice_results
        .iter()
        .map(|x| x.transit_captured_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        out.kpis.total_trips_attempted <= transit_captured_total + 1e-6,
        "only transit-captured demand may enter transit assignment attempt totals"
    );

    for check in &out.demand_diagnostics.consistency_checks {
        assert!(
            check.passed,
            "consistency check should pass: {}",
            check.name
        );
    }

    assert!(
        out.active_temporal_slice.time_slice == DemandTimeSliceLabel::Interpeak,
        "test context should run in interpeak slice"
    );
}
