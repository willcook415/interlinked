mod common;

use interlinked_engine::sim::{
    AssignedOdFlow, CommercialStrengthClassification, SeasonalProfile, ServiceDayType,
    SocialNecessityClassification,
};
use interlinked_engine::{
    PlanningRunOptions, ScenarioService, SimulationOutput, SimulationService,
};
use std::collections::HashSet;

const EPS: f64 = 1e-6;

fn run_context(
    day: ServiceDayType,
    season: SeasonalProfile,
    time_of_day_s: f64,
) -> SimulationOutput {
    let scenario_path = common::fixture_path("scenario_demand_economics_phase7.json");
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
fn phase7_financial_outputs_are_present_and_temporally_populated() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    assert!(
        !out.service_financial_metrics.is_empty(),
        "service financial metrics should be emitted"
    );
    assert!(
        !out.corridor_financial_metrics.is_empty(),
        "corridor financial metrics should be emitted"
    );
    assert!(
        !out.station_financial_context.is_empty(),
        "station financial context should be emitted"
    );
    assert!(
        !out.economic_diagnostics
            .top_revenue_generating_corridors
            .is_empty(),
        "economic diagnostics should include revenue corridor rankings"
    );
    assert!(
        !out.economic_diagnostics
            .top_operating_cost_heavy_services
            .is_empty(),
        "economic diagnostics should include operating-cost rankings"
    );
    assert!(
        !out.economic_diagnostics
            .network_financial_by_time_slice
            .is_empty(),
        "temporal financial summaries by slice should be populated"
    );
    assert!(
        !out.economic_diagnostics
            .network_financial_by_day_type
            .is_empty(),
        "temporal financial summaries by day type should be populated"
    );

    let day_types = out
        .economic_diagnostics
        .network_financial_by_day_type
        .iter()
        .map(|x| x.service_day_type)
        .collect::<HashSet<_>>();
    assert!(
        day_types.contains(&ServiceDayType::Weekday)
            && day_types.contains(&ServiceDayType::Saturday)
            && day_types.contains(&ServiceDayType::SundayHoliday),
        "day-type financial summaries should cover weekday/saturday/sunday_holiday"
    );

    let net = &out.network_financial_summary.metrics;
    assert!(
        net.fare_revenue >= 0.0,
        "network fare revenue must be non-negative"
    );
    assert!(
        net.total_cost + EPS >= net.operating_cost,
        "network total cost should include operating cost"
    );
    assert!(
        (net.total_cost
            - (net.operating_cost
                + net.infrastructure_cost_allocated
                + net.rolling_stock_cost_allocated))
            .abs()
            <= 1e-5,
        "network total cost should reconcile to operating + infra + rolling stock"
    );
    assert!(
        out.network_financial_summary
            .total_infrastructure_annualized_cost
            > 0.0,
        "network infrastructure annualized cost should be positive"
    );
    assert!(
        out.network_financial_summary
            .total_rolling_stock_annualized_cost
            > 0.0,
        "network rolling-stock annualized cost should be positive"
    );

    assert!(
        out.zone_planning_metrics
            .iter()
            .any(|z| z.transit_revenue_generated > 0.0),
        "zone planning metrics should carry positive transit revenue attribution"
    );
    assert!(
        out.line_service_planning_metrics
            .iter()
            .any(|s| s.fare_revenue > 0.0 && s.total_cost > 0.0),
        "line/service planning metrics should carry service-level finance fields"
    );
    assert!(
        out.corridor_planning_metrics
            .iter()
            .any(|c| c.fare_revenue > 0.0 && c.subsidy_required >= 0.0),
        "corridor planning metrics should carry corridor-level finance fields"
    );
}

#[test]
fn phase7_revenue_is_grounded_in_realized_transit_and_conservation() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
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
        "assigned transit passengers should not exceed mode-choice transit-captured demand"
    );

    let service_ridership_sum = out
        .service_financial_metrics
        .iter()
        .map(|x| x.ridership.max(0.0))
        .sum::<f64>();
    let station_boardings_sum = out
        .stop_flow_states
        .iter()
        .map(|x| x.boarded_this_step.max(0.0))
        .sum::<f64>();
    assert!(
        (service_ridership_sum - station_boardings_sum).abs() <= 1e-5,
        "service ridership and stop boardings should reconcile"
    );
    assert!(
        (service_ridership_sum - out.network_financial_summary.total_realised_transit_trips).abs()
            <= 1e-5,
        "network realised transit trips should reconcile with service ridership totals"
    );

    let service_revenue_sum = out
        .service_financial_metrics
        .iter()
        .map(|x| x.metrics.fare_revenue.max(0.0))
        .sum::<f64>();
    assert!(
        (service_revenue_sum - out.network_financial_summary.metrics.fare_revenue).abs() <= 1e-5,
        "network fare revenue should reconcile with sum of service fare revenues"
    );

    if service_ridership_sum <= EPS {
        assert!(
            out.network_financial_summary.metrics.fare_revenue <= EPS,
            "with zero realised transit ridership, network fare revenue should be zero"
        );
    } else {
        assert!(
            out.network_financial_summary.metrics.fare_revenue > 0.0,
            "with positive realised transit ridership, network fare revenue should be positive"
        );
    }
}

#[test]
fn phase7_trunk_vs_rural_financial_differentiation_and_cost_scaling_hold() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    let sub_in = out
        .service_financial_metrics
        .iter()
        .find(|x| x.service_id == "SV_SUB_IN")
        .expect("SV_SUB_IN should exist");
    let vil_in = out
        .service_financial_metrics
        .iter()
        .find(|x| x.service_id == "SV_VIL_IN")
        .expect("SV_VIL_IN should exist");
    let reg_in = out
        .service_financial_metrics
        .iter()
        .find(|x| x.service_id == "SV_REG_IN")
        .expect("SV_REG_IN should exist");
    let std_in = out
        .service_financial_metrics
        .iter()
        .find(|x| x.service_id == "SV_STD_IN")
        .expect("SV_STD_IN should exist");

    assert!(
        sub_in.ridership > vil_in.ridership,
        "dense suburban trunk should carry more ridership than weak village connector"
    );
    assert!(
        sub_in.metrics.fare_revenue > vil_in.metrics.fare_revenue,
        "dense suburban trunk should generate more fare revenue than weak village connector"
    );
    assert!(
        sub_in.metrics.farebox_recovery_ratio > vil_in.metrics.farebox_recovery_ratio,
        "dense suburban trunk should recover a higher share of operating cost than weak village connector"
    );

    assert!(
        sub_in.metrics.operating_cost > vil_in.metrics.operating_cost,
        "higher-supply trunk service should carry higher operating cost than low-frequency village service"
    );
    assert!(
        reg_in.vehicle_km > std_in.vehicle_km && reg_in.metrics.operating_cost > std_in.metrics.operating_cost,
        "service operating cost should scale upward with larger supplied vehicle-km in this fixture"
    );
}

#[test]
fn phase7_classifications_and_build_preview_economics_are_meaningful() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        20.0 * 3600.0,
    );

    let commercial_classes = out
        .service_financial_metrics
        .iter()
        .map(|x| x.commercial_strength_classification)
        .collect::<HashSet<_>>();
    let social_classes = out
        .service_financial_metrics
        .iter()
        .map(|x| x.social_necessity_classification)
        .collect::<HashSet<_>>();

    assert!(
        commercial_classes.len() >= 2,
        "service commercial classifications should be differentiated across services"
    );
    assert!(
        social_classes.len() >= 2,
        "service social-necessity classifications should be differentiated across services"
    );
    assert!(
        out.service_financial_metrics.iter().any(|x| {
            x.commercial_strength_classification == CommercialStrengthClassification::Weak
                && x.social_necessity_classification == SocialNecessityClassification::Important
        }),
        "fixture should include at least one commercially weak but socially important service"
    );

    assert!(
        !out.service_gap_rankings
            .top_subsidy_dependent_social_corridors
            .is_empty(),
        "service-gap rankings should expose subsidy-dependent social corridors"
    );
    assert!(
        !out.service_gap_rankings
            .top_expensive_underperforming_services
            .is_empty(),
        "service-gap rankings should expose expensive underperforming services"
    );

    assert!(
        !out.build_preview_metrics.is_empty(),
        "build preview metrics should exist"
    );
    assert!(
        out.build_preview_metrics.iter().any(|p| {
            p.estimated_revenue_uplift > 0.0
                && p.estimated_operating_cost_uplift > 0.0
                && p.estimated_capital_cost > 0.0
                && p.reinvestment_case_score > 0.0
        }),
        "build preview rows should carry populated economics fields"
    );
    assert!(
        out.build_preview_metrics.iter().all(|p| {
            p.estimated_farebox_recovery.is_finite()
                && p.likely_subsidy_requirement >= 0.0
                && p.reinvestment_case_score.is_finite()
        }),
        "build preview economics values should be finite and coherent"
    );
}

#[test]
fn phase7_temporal_economics_and_reliability_finance_links_are_coherent() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );

    let day_rows = &out.economic_diagnostics.network_financial_by_day_type;
    let weekday = day_rows
        .iter()
        .find(|x| x.service_day_type == ServiceDayType::Weekday)
        .expect("weekday financial summary should exist");
    let sunday = day_rows
        .iter()
        .find(|x| x.service_day_type == ServiceDayType::SundayHoliday)
        .expect("sunday_holiday financial summary should exist");
    assert!(
        weekday.fare_revenue > sunday.fare_revenue,
        "weekday fare revenue should exceed sunday-holiday in this commuter-heavy fixture"
    );

    assert!(
        !out.economic_diagnostics
            .corridors_where_unreliability_hurts_finances
            .is_empty(),
        "economic diagnostics should expose corridors where unreliability hurts finances"
    );
    assert!(
        out.line_service_planning_metrics
            .iter()
            .any(|x| x.reliability_cost_pressure > 0.0),
        "line/service planning should expose reliability-linked cost pressure"
    );
}
