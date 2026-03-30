use interlinked_engine::model::{Crs, Scenario};

#[test]
fn scenario_without_crs_still_deserializes_and_defaults_to_local() {
    // Minimal scenario JSON: has meta but omits meta.crs.
    // Also includes a link omitting geometry.
    let json = r#"
    {
      "meta": { "name": "test", "seed": 1, "time_period_hours": 1.0 },
      "params": {
        "walk_weight": 1.0,
        "wait_weight": 2.0,
        "ivt_weight": 1.0,
        "transfer_penalty_s": 300.0,
        "access_walk_speed_mps": 1.4,
        "access_radius_m": 1200.0,
        "gravity_beta": 0.0003,
        "trips_per_person": 1.0,
        "assignment_max_iters": 8
        },
      "world": {
        "zones": [],
        "stops": [
          { "id": "A", "name": "A", "x": 0.0, "y": 0.0, "zone_id": null, "kind": "station" },
          { "id": "B", "name": "B", "x": 1000.0, "y": 0.0, "zone_id": null, "kind": "station" }
        ],
        "links": [
          { "id": "L1", "from_stop": "A", "to_stop": "B", "distance_m": 1000.0, "mode": "rail", "speed_mps": 10.0, "capacity_per_hour": null }
        ],
        "services": [],
        "transfers": []
      }
    }"#;

    let s: Scenario = serde_json::from_str(json).expect("scenario should deserialize");

    // CRS default should kick in
    match s.meta.crs {
        Crs::Local { .. } => {}
        _ => panic!("expected default CRS to be Local"),
    }

    // geometry should default to None if absent
    assert!(s.world.links[0].geometry.is_none());
}
