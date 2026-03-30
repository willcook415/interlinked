mod common;

use interlinked_engine::sim::{
    BuildPreviewType, CorridorClassification, DemandTimeSliceLabel, TripPurpose,
};
use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};

const EPS: f64 = 1e-6;

#[test]
fn phase3_outputs_expose_legible_zone_station_corridor_and_preview_metrics() {
    let scenario_path = common::fixture_path("scenario_demand_planning_phase3.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    assert!(
        out.planning_overlay_config.is_some(),
        "planning overlay config should be emitted"
    );
    assert!(
        !out.zone_planning_metrics.is_empty(),
        "zone planning metrics should be emitted"
    );
    assert!(
        !out.station_planning_metrics.is_empty(),
        "station planning metrics should be emitted"
    );
    assert!(
        !out.corridor_planning_metrics.is_empty(),
        "corridor planning metrics should be emitted"
    );
    assert!(
        !out.line_service_planning_metrics.is_empty(),
        "line/service planning metrics should be emitted"
    );
    assert!(
        !out.build_preview_metrics.is_empty(),
        "build preview metrics should be emitted"
    );

    let core = out
        .zone_planning_metrics
        .iter()
        .find(|z| z.zone_id == "city_core_cbd")
        .expect("city core zone metrics should exist");
    let rural = out
        .zone_planning_metrics
        .iter()
        .find(|z| z.zone_id == "rural_cluster")
        .expect("rural zone metrics should exist");
    assert!(
        core.accessibility_score > rural.accessibility_score,
        "well-connected core should score higher accessibility than disconnected rural"
    );
    assert!(
        rural.total_unserved_produced > 0.0,
        "rural zone should surface non-zero unserved demand"
    );

    assert!(
        out.service_gap_rankings
            .top_weak_access_rural_zones
            .iter()
            .any(|z| z.zone_id == "rural_cluster"),
        "rural weak-access ranking should include rural cluster"
    );

    let sub_station = out
        .station_planning_metrics
        .iter()
        .find(|s| s.stop_id == "S_SUB")
        .expect("suburb station metrics should exist");
    let village_station = out
        .station_planning_metrics
        .iter()
        .find(|s| s.stop_id == "S_VIL")
        .expect("village station metrics should exist");
    assert!(
        sub_station.catchment_population > village_station.catchment_population,
        "dense suburb station should have larger catchment than village station"
    );

    assert!(
        out.service_gap_rankings
            .top_overcrowded_stations
            .iter()
            .any(|s| s.score > 0.0),
        "overcrowded station ranking should provide non-zero pressure scores"
    );

    assert!(
        out.corridor_planning_metrics.iter().any(|c| {
            c.corridor_classification == CorridorClassification::SuburbanCommuterRadial
                || c.corridor_classification == CorridorClassification::UrbanTrunkMetroSuitable
        }),
        "corridor classification should identify commuter or trunk-style corridors"
    );

    assert!(
        out.corridor_planning_metrics.iter().any(|c| {
            c.corridor_classification == CorridorClassification::Intercity
                || c.corridor_classification == CorridorClassification::AirportAccess
        }),
        "corridor classification should identify intercity or airport-oriented corridors"
    );

    assert!(
        out.build_preview_metrics
            .iter()
            .any(|p| p.preview_type == BuildPreviewType::Station),
        "station preview candidate should be present"
    );
    assert!(
        out.build_preview_metrics
            .iter()
            .any(|p| p.preview_type == BuildPreviewType::LineSegment),
        "line-segment preview candidate should be present"
    );
    assert!(
        out.build_preview_metrics
            .iter()
            .any(|p| p.preview_type == BuildPreviewType::ServiceFrequencyIncrease),
        "service-frequency preview candidate should be present"
    );

    assert!(
        out.build_preview_metrics.iter().any(|p| {
            p.unserved_demand_addressable > 0.0
                && p.latent_demand_interceptable > 0.0
                && p.accessibility_delta_proxy > 0.0
        }),
        "preview candidates should carry meaningful intervention signal"
    );
}

#[test]
fn phase3_legibility_outputs_preserve_authoritative_flow_conservation() {
    let scenario_path = common::fixture_path("scenario_demand_planning_phase3.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    let active_latent = out
        .latent_od_demand
        .iter()
        .filter(|x| x.time_slice == DemandTimeSliceLabel::Interpeak)
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>();
    let total_realised = out
        .assigned_od_flows
        .iter()
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    let total_unserved = out
        .assigned_od_flows
        .iter()
        .map(|x| x.unserved_passengers.max(0.0))
        .sum::<f64>();
    assert!(
        (active_latent - (total_realised + total_unserved)).abs() <= EPS,
        "Phase 1/2 conservation should still hold after Phase 3 overlays"
    );

    for check in &out.demand_diagnostics.consistency_checks {
        assert!(check.passed, "consistency check failed: {}", check.name);
    }

    assert!(
        out.zone_planning_metrics
            .iter()
            .any(|z| z.dominant_trip_purpose == Some(TripPurpose::Work)),
        "zone planning outputs should preserve purpose legibility"
    );
}
