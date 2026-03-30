mod common;

use interlinked_engine::sim::{
    AssignedOdFlow, DemandTimeSliceLabel, LatentOdDemand, SeasonalProfile, ServiceDayType,
    TemporalDemandSlice, TripPurpose,
};
use interlinked_engine::{
    PlanningRunOptions, ScenarioService, SimulationOutput, SimulationService,
};

const EPS: f64 = 1e-6;

fn run_context(
    day: ServiceDayType,
    season: SeasonalProfile,
    time_of_day_s: f64,
) -> SimulationOutput {
    let scenario_path = common::fixture_path("scenario_demand_temporal_phase4.json");
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

fn in_active_context_latent(out: &SimulationOutput) -> impl Iterator<Item = &LatentOdDemand> {
    let ctx = &out.active_temporal_slice;
    out.latent_od_demand.iter().filter(move |x| {
        x.time_slice == ctx.time_slice
            && x.service_day_type == Some(ctx.service_day_type)
            && x.seasonal_profile == Some(ctx.seasonal_profile)
    })
}

fn in_active_context_assigned(out: &SimulationOutput) -> impl Iterator<Item = &AssignedOdFlow> {
    let ctx = &out.active_temporal_slice;
    out.assigned_od_flows.iter().filter(move |x| {
        x.time_slice == ctx.time_slice
            && x.service_day_type == Some(ctx.service_day_type)
            && x.seasonal_profile == Some(ctx.seasonal_profile)
    })
}

fn active_purpose_latent(out: &SimulationOutput, purpose: TripPurpose) -> f64 {
    in_active_context_latent(out)
        .filter(|x| x.purpose == purpose)
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>()
}

fn active_zone_pair_latent(out: &SimulationOutput, zone_id: &str, purpose: TripPurpose) -> f64 {
    in_active_context_latent(out)
        .filter(|x| x.purpose == purpose)
        .filter(|x| x.origin_zone_id == zone_id || x.destination_zone_id == zone_id)
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>()
}

#[test]
fn phase4_temporal_profiles_shift_work_education_and_weekend_mix() {
    let weekday_am = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );
    let saturday_am = run_context(
        ServiceDayType::Saturday,
        SeasonalProfile::Neutral,
        8.0 * 3600.0,
    );
    let saturday_interpeak = run_context(
        ServiceDayType::Saturday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );
    let weekday_interpeak = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );

    let term_weekday_am = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::TermTime,
        8.0 * 3600.0,
    );
    let holiday_weekday_am = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::HolidayPeriod,
        8.0 * 3600.0,
    );

    let weekday_work = active_purpose_latent(&weekday_am, TripPurpose::Work);
    let saturday_work = active_purpose_latent(&saturday_am, TripPurpose::Work);
    assert!(
        weekday_work > saturday_work,
        "weekday AM work demand should exceed Saturday AM work demand"
    );

    let sat_shopping = active_purpose_latent(&saturday_interpeak, TripPurpose::Shopping);
    let sat_leisure = active_purpose_latent(&saturday_interpeak, TripPurpose::Leisure);
    let weekday_interpeak_shopping =
        active_purpose_latent(&weekday_interpeak, TripPurpose::Shopping);
    let weekday_interpeak_leisure = active_purpose_latent(&weekday_interpeak, TripPurpose::Leisure);
    assert!(
        sat_shopping > weekday_interpeak_shopping,
        "Saturday interpeak shopping should exceed weekday interpeak shopping"
    );
    assert!(
        sat_leisure > weekday_interpeak_leisure,
        "Saturday interpeak leisure should exceed weekday interpeak leisure"
    );

    let term_education = active_purpose_latent(&term_weekday_am, TripPurpose::Education);
    let holiday_education = active_purpose_latent(&holiday_weekday_am, TripPurpose::Education);
    assert!(
        term_education > holiday_education,
        "term-time education demand should exceed holiday-period education demand"
    );

    let uni_term = active_zone_pair_latent(
        &term_weekday_am,
        "university_district",
        TripPurpose::Education,
    );
    let uni_holiday = active_zone_pair_latent(
        &holiday_weekday_am,
        "university_district",
        TripPurpose::Education,
    );
    assert!(
        uni_term > uni_holiday,
        "university-linked education demand should be stronger in term time"
    );
}

#[test]
fn phase4_holiday_airport_tourism_and_stadium_spikes_are_visible_and_bounded() {
    let neutral_sat_interpeak = run_context(
        ServiceDayType::Saturday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );
    let holiday_sat_interpeak = run_context(
        ServiceDayType::Saturday,
        SeasonalProfile::HolidayPeriod,
        13.0 * 3600.0,
    );

    let holiday_airport =
        active_zone_pair_latent(
            &holiday_sat_interpeak,
            "airport_zone",
            TripPurpose::Intercity,
        ) + active_zone_pair_latent(&holiday_sat_interpeak, "airport_zone", TripPurpose::Leisure);
    let neutral_airport =
        active_zone_pair_latent(
            &neutral_sat_interpeak,
            "airport_zone",
            TripPurpose::Intercity,
        ) + active_zone_pair_latent(&neutral_sat_interpeak, "airport_zone", TripPurpose::Leisure);
    assert!(
        holiday_airport > neutral_airport,
        "holiday profile should shift demand toward airport-linked flows"
    );

    let holiday_tour = active_zone_pair_latent(
        &holiday_sat_interpeak,
        "tourism_quarter",
        TripPurpose::Leisure,
    );
    let neutral_tour = active_zone_pair_latent(
        &neutral_sat_interpeak,
        "tourism_quarter",
        TripPurpose::Leisure,
    );
    assert!(
        holiday_tour > neutral_tour,
        "holiday profile should increase tourism-linked leisure demand"
    );

    assert!(
        in_active_context_latent(&holiday_sat_interpeak).any(|x| {
            x.active_event_ids
                .iter()
                .any(|id| id == "airport_holiday_surge" || id == "tourism_seasonal_uplift")
        }),
        "holiday context should surface active event-attribution ids"
    );

    let weekday_interpeak = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );
    let weekday_evening = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        20.0 * 3600.0,
    );

    let stadium_interpeak = active_zone_pair_latent(
        &weekday_interpeak,
        "stadium_nightlife",
        TripPurpose::Leisure,
    );
    let stadium_evening =
        active_zone_pair_latent(&weekday_evening, "stadium_nightlife", TripPurpose::Leisure);
    assert!(
        stadium_evening > stadium_interpeak,
        "stadium/nightlife leisure should spike in evening vs interpeak"
    );

    let spike_ratio = stadium_evening / stadium_interpeak.max(1.0);
    assert!(
        spike_ratio < 8.0,
        "event spikes should remain bounded, got ratio {spike_ratio:.2}"
    );

    assert!(
        in_active_context_latent(&weekday_evening).any(|x| {
            (x.origin_zone_id == "stadium_nightlife"
                || x.destination_zone_id == "stadium_nightlife")
                && x.active_event_ids
                    .iter()
                    .any(|id| id == "stadium_evening_spike")
        }),
        "evening stadium flows should carry stadium event attribution"
    );
}

#[test]
fn phase4_temporal_outputs_and_pressure_rankings_are_authoritative_and_non_empty() {
    let out = run_context(
        ServiceDayType::Weekday,
        SeasonalProfile::Neutral,
        13.0 * 3600.0,
    );

    assert_eq!(
        out.active_temporal_slice,
        TemporalDemandSlice {
            service_day_type: ServiceDayType::Weekday,
            time_slice: DemandTimeSliceLabel::Interpeak,
            seasonal_profile: SeasonalProfile::Neutral,
            active_event_ids: out.active_temporal_slice.active_event_ids.clone(),
        }
    );

    assert!(
        !out.temporal_planning_snapshots.is_empty(),
        "temporal planning snapshots should be emitted"
    );
    assert!(
        out.temporal_planning_snapshots.iter().any(|s| {
            s.temporal_slice.service_day_type == ServiceDayType::Saturday
                && s.temporal_slice.time_slice == DemandTimeSliceLabel::Interpeak
                && s.temporal_slice.seasonal_profile == SeasonalProfile::Neutral
        }),
        "temporal snapshots should include Saturday interpeak"
    );
    assert!(
        out.temporal_planning_snapshots.iter().any(|s| {
            s.temporal_slice.service_day_type == ServiceDayType::Weekday
                && s.temporal_slice.time_slice == DemandTimeSliceLabel::AmPeak
                && s.temporal_slice.seasonal_profile == SeasonalProfile::TermTime
        }),
        "temporal snapshots should include term-time weekday AM"
    );
    assert!(
        out.temporal_planning_snapshots.iter().any(|s| {
            s.temporal_slice.service_day_type == ServiceDayType::SundayHoliday
                && s.temporal_slice.time_slice == DemandTimeSliceLabel::Interpeak
                && s.temporal_slice.seasonal_profile == SeasonalProfile::HolidayPeriod
        }),
        "temporal snapshots should include holiday Sunday interpeak"
    );

    assert!(
        !out.temporal_demand_diagnostics.purpose_totals.is_empty(),
        "temporal purpose totals should be emitted"
    );
    assert!(
        !out.temporal_demand_diagnostics
            .top_overloaded_stations_by_slice
            .is_empty(),
        "temporal overloaded-station rankings should be emitted"
    );
    assert!(
        !out.temporal_demand_diagnostics
            .peak_line_overload_by_slice
            .is_empty(),
        "temporal line overload rankings should be emitted"
    );

    let ranked_station = out
        .temporal_demand_diagnostics
        .top_overloaded_stations_by_slice
        .first()
        .expect("ranked station should exist");
    assert!(
        out.temporal_demand_diagnostics
            .station_pressure
            .iter()
            .any(|p| {
                p.temporal_slice == ranked_station.temporal_slice
                    && p.stop_id == ranked_station.id
                    && (p.waiting > 0.0 || p.denied > 0.0 || p.overcrowding_risk_score >= 0.0)
            }),
        "station overload ranking should correspond to authoritative station pressure points"
    );

    let active_latent = in_active_context_latent(&out)
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>();
    let total_realised = in_active_context_assigned(&out)
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    let total_unserved = in_active_context_assigned(&out)
        .map(|x| x.unserved_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        (active_latent - (total_realised + total_unserved)).abs() <= EPS,
        "active temporal context should conserve latent = realised + unserved"
    );

    for check in &out.demand_diagnostics.consistency_checks {
        assert!(
            check.passed,
            "consistency check should pass: {}",
            check.name
        );
    }
}
