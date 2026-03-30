mod common;

use interlinked_engine::sim::{DemandTimeSliceLabel, TripPurpose};
use interlinked_engine::{PlanningRunOptions, ScenarioService, SimulationService};

const EPS: f64 = 1e-6;

fn purpose_latent_for_zone(
    out: &interlinked_engine::SimulationOutput,
    zone_id: &str,
    purpose: TripPurpose,
) -> f64 {
    out.zone_demand_production_layer
        .iter()
        .find(|z| z.zone_id == zone_id)
        .and_then(|z| z.by_purpose.iter().find(|p| p.purpose == purpose))
        .map(|p| p.latent.max(0.0))
        .unwrap_or(0.0)
}

#[test]
fn phase2_synthetic_geography_shapes_zone_hierarchy_and_attractors() {
    let scenario_path = common::fixture_path("scenario_demand_synthetic_geography_phase2.json");
    let doc = ScenarioService::load_from_path(scenario_path.to_string_lossy().as_ref())
        .expect("scenario should load");
    let out = SimulationService::run_planning(&doc, PlanningRunOptions::default())
        .expect("planning run should succeed");

    assert!(
        !out.zone_economic_geography_layer.is_empty(),
        "economic geography layer should be emitted"
    );
    assert!(
        !out.zone_demand_production_layer.is_empty(),
        "production layer should be emitted"
    );
    assert!(
        !out.zone_demand_attraction_layer.is_empty(),
        "attraction layer should be emitted"
    );
    assert!(
        !out.corridor_desire_lines.is_empty(),
        "corridor desire lines should be emitted"
    );
    assert!(
        out.synthetic_economy_config.is_some(),
        "synthetic economy config should be emitted as source-of-truth metadata"
    );

    let core = out
        .zone_economic_geography_layer
        .iter()
        .find(|z| z.zone_id == "city_core_cbd")
        .expect("city core should exist");
    let suburb = out
        .zone_economic_geography_layer
        .iter()
        .find(|z| z.zone_id == "north_suburb")
        .expect("north suburb should exist");
    let uni = out
        .zone_economic_geography_layer
        .iter()
        .find(|z| z.zone_id == "uni_district")
        .expect("university district should exist");
    let airport = out
        .zone_economic_geography_layer
        .iter()
        .find(|z| z.zone_id == "airport_zone")
        .expect("airport zone should exist");

    assert!(
        core.work_attractiveness > suburb.work_attractiveness,
        "city core should attract more work demand than suburb"
    );
    assert!(
        core.intercity_importance > suburb.intercity_importance,
        "city core should be more intercity-important than suburb"
    );
    assert!(
        uni.education_attractiveness > suburb.education_attractiveness,
        "university district should attract more education demand than suburb"
    );
    assert!(
        airport
            .special_attractors
            .contains(&interlinked_engine::sim::SpecialAttractorType::Airport),
        "airport attractor should be inferred for airport zone"
    );

    let rural_essential = purpose_latent_for_zone(&out, "rural_agri", TripPurpose::Essential);
    let core_essential = purpose_latent_for_zone(&out, "city_core_cbd", TripPurpose::Essential);
    assert!(
        rural_essential > 0.0,
        "rural zones should retain non-zero essential demand"
    );
    assert!(
        rural_essential < core_essential,
        "rural essential demand should be lower than major core"
    );
}

#[test]
fn phase2_corridors_are_legible_and_accounting_is_conserved() {
    let scenario_path = common::fixture_path("scenario_demand_synthetic_geography_phase2.json");
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
        "active latent demand should reconcile with realised+unserved"
    );

    for check in &out.demand_diagnostics.consistency_checks {
        assert!(check.passed, "consistency check failed: {}", check.name);
    }

    let commuter_to_core = out
        .corridor_desire_lines
        .iter()
        .find(|c| {
            c.origin_zone_id == "north_suburb"
                && c.destination_zone_id == "city_core_cbd"
                && c.purpose == TripPurpose::Work
        })
        .expect("north suburb -> core work corridor should exist");
    assert!(
        commuter_to_core.latent_passengers > 0.0,
        "suburb to core commuter corridor should carry demand"
    );

    assert!(
        out.demand_diagnostics.top_intercity_pairs.iter().any(|c| {
            c.purpose == TripPurpose::Intercity
                && ((c.origin_zone_id == "city_core_cbd"
                    && (c.destination_zone_id == "regional_town_centre"
                        || c.destination_zone_id == "airport_zone"))
                    || (c.destination_zone_id == "city_core_cbd"
                        && (c.origin_zone_id == "regional_town_centre"
                            || c.origin_zone_id == "airport_zone")))
        }),
        "top intercity pairs should concentrate among major core/regional/airport settlements"
    );

    let rural_gap = out
        .service_gap_layer
        .iter()
        .find(|z| z.zone_id == "rural_agri")
        .expect("rural gap layer entry should exist");
    assert!(
        rural_gap.total_unserved_demand > 0.0,
        "rural zone should surface unmet demand when disconnected from service"
    );

    for pair in out.corridor_desire_lines.windows(2) {
        assert!(
            pair[0].corridor_score + EPS >= pair[1].corridor_score,
            "corridor list should remain sorted by corridor score"
        );
    }

    assert!(
        !out.demand_diagnostics
            .strongest_commuter_corridors
            .is_empty(),
        "commuter corridor diagnostics should be emitted"
    );
    assert!(
        !out.demand_diagnostics
            .strongest_rural_to_town_flows
            .is_empty(),
        "rural-to-town diagnostics should be emitted"
    );
    assert!(
        out.demand_diagnostics
            .strongest_anchor_flows
            .iter()
            .any(|c| {
                c.origin_zone_id == "airport_zone" || c.destination_zone_id == "airport_zone"
            }),
        "anchor diagnostics should include airport-related flow"
    );
}
