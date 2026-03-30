use interlinked_engine::model::{
    Crs, DemandCell, DemandPurposeTimeSlice, DemandTimeSlice, Link, Meta, Params, Scenario,
    Service, Stop, World, Zone,
};
use interlinked_engine::sim::{init_sim_state, step_simulation, RunConfig};
use interlinked_engine::{
    run_simulation, PlanningRunOptions, ScenarioDocument, SimulationOutput, SimulationService,
};

fn base_params() -> Params {
    Params {
        walk_weight: 1.0,
        wait_weight: 1.0,
        ivt_weight: 1.0,
        transfer_penalty_s: 0.0,
        access_walk_speed_mps: 1.4,
        access_radius_m: 250.0,
        gravity_beta: 0.0,
        trips_per_person: 0.2,
        purpose_share_home_work: 0.52,
        purpose_share_home_education: 0.12,
        purpose_share_home_retail: 0.18,
        purpose_share_home_recreation: 0.10,
        purpose_share_other: 0.08,
        attraction_weight_office: 1.0,
        attraction_weight_retail: 1.0,
        attraction_weight_recreation: 1.0,
        attraction_weight_industrial: 1.0,
        attraction_weight_education: 1.0,
        attraction_weight_health: 1.0,
        route_choice_k: 1,
        route_choice_theta: 0.003,
        assignment_max_iters: 1,
        assignment_convergence_rel: 0.0,
        capacity_enabled: false,
        queue_max_extra_wait_s: 3600.0,
        fare_enabled: true,
        fare_value_of_time_base_per_hour: 12.0,
        fare_elasticity: 0.35,
        fare_reference_base: 2.5,
        fare_transfer_window_s: 2700.0,
        fare_free_transfers_per_trip: 1,
        fare_overflow_retry_share: 0.15,
        fare_mode_bus_base: 1.8,
        fare_mode_tram_base: 2.3,
        fare_mode_metro_base: 2.7,
        fare_mode_rail_base: 3.6,
        fare_mode_ferry_base: 3.0,
        fare_mode_default_base: 2.5,
        station_capacity_scale_boarding: 1.0,
        station_capacity_scale_alighting: 1.0,
        station_queue_capacity_scale: 1.0,
        debug_sample_origin_zone: None,
        debug_sample_dest_zone: None,
        demand_profile: Vec::<DemandTimeSlice>::new(),
        demand_purpose_profile: Vec::<DemandPurposeTimeSlice>::new(),
    }
}

fn two_destination_scenario() -> Scenario {
    Scenario {
        meta: Meta {
            name: "purpose_test".to_string(),
            seed: 99,
            time_period_hours: 1.0,
            crs: Crs::Local {
                origin_lon: 0.0,
                origin_lat: 0.0,
            },
        },
        params: base_params(),
        world: World {
            zones: vec![
                Zone {
                    id: "C0".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 1000.0,
                    jobs: 0.0,
                    country_iso2: Some("GB".to_string()),
                },
                Zone {
                    id: "CR".to_string(),
                    x: 1000.0,
                    y: 0.0,
                    population: 100.0,
                    jobs: 250.0,
                    country_iso2: Some("GB".to_string()),
                },
                Zone {
                    id: "CO".to_string(),
                    x: 0.0,
                    y: 1000.0,
                    population: 100.0,
                    jobs: 250.0,
                    country_iso2: Some("GB".to_string()),
                },
            ],
            stops: vec![
                Stop {
                    id: "S0".to_string(),
                    name: None,
                    x: 0.0,
                    y: 0.0,
                    country_iso2: Some("GB".to_string()),
                    interchange_id: None,
                    stop_type: Some("station".to_string()),
                    station_boarding_capacity_pph: None,
                    station_alighting_capacity_pph: None,
                    station_queue_capacity_pax: None,
                },
                Stop {
                    id: "SR".to_string(),
                    name: None,
                    x: 1000.0,
                    y: 0.0,
                    country_iso2: Some("GB".to_string()),
                    interchange_id: None,
                    stop_type: Some("station".to_string()),
                    station_boarding_capacity_pph: None,
                    station_alighting_capacity_pph: None,
                    station_queue_capacity_pax: None,
                },
                Stop {
                    id: "SO".to_string(),
                    name: None,
                    x: 0.0,
                    y: 1000.0,
                    country_iso2: Some("GB".to_string()),
                    interchange_id: None,
                    stop_type: Some("station".to_string()),
                    station_boarding_capacity_pph: None,
                    station_alighting_capacity_pph: None,
                    station_queue_capacity_pax: None,
                },
            ],
            links: vec![
                Link {
                    id: "L_TO_RETAIL".to_string(),
                    from_stop: "S0".to_string(),
                    to_stop: "SR".to_string(),
                    distance_m: 1000.0,
                    mode: "metro".to_string(),
                    speed_mps: 20.0,
                    geometry: None,
                    line_id: None,
                    mode_variant: None,
                    capacity_per_hour: None,
                },
                Link {
                    id: "L_RETAIL_BACK".to_string(),
                    from_stop: "SR".to_string(),
                    to_stop: "S0".to_string(),
                    distance_m: 1000.0,
                    mode: "metro".to_string(),
                    speed_mps: 20.0,
                    geometry: None,
                    line_id: None,
                    mode_variant: None,
                    capacity_per_hour: None,
                },
                Link {
                    id: "L_TO_OFFICE".to_string(),
                    from_stop: "S0".to_string(),
                    to_stop: "SO".to_string(),
                    distance_m: 1000.0,
                    mode: "metro".to_string(),
                    speed_mps: 20.0,
                    geometry: None,
                    line_id: None,
                    mode_variant: None,
                    capacity_per_hour: None,
                },
                Link {
                    id: "L_OFFICE_BACK".to_string(),
                    from_stop: "SO".to_string(),
                    to_stop: "S0".to_string(),
                    distance_m: 1000.0,
                    mode: "metro".to_string(),
                    speed_mps: 20.0,
                    geometry: None,
                    line_id: None,
                    mode_variant: None,
                    capacity_per_hour: None,
                },
            ],
            services: vec![
                Service {
                    id: "SV_RETAIL".to_string(),
                    line_id: None,
                    name: None,
                    mode: "metro".to_string(),
                    mode_variant: None,
                    stop_sequence: vec!["S0".to_string(), "SR".to_string()],
                    direction: None,
                    direction_name: None,
                    display_color: None,
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    headway_s: 120.0,
                    dwell_s: 20.0,
                    vehicle_capacity: 10000.0,
                    board_penalty_s: Some(0.0),
                },
                Service {
                    id: "SV_OFFICE".to_string(),
                    line_id: None,
                    name: None,
                    mode: "metro".to_string(),
                    mode_variant: None,
                    stop_sequence: vec!["S0".to_string(), "SO".to_string()],
                    direction: None,
                    direction_name: None,
                    display_color: None,
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    headway_s: 120.0,
                    dwell_s: 20.0,
                    vehicle_capacity: 10000.0,
                    board_penalty_s: Some(0.0),
                },
            ],
            transfers: vec![],
            transfer_rules: None,
            demand_cells: vec![
                DemandCell {
                    cell_id: "C0".to_string(),
                    x: 0.0,
                    y: 0.0,
                    area_m2: 1_000_000.0,
                    residents_night: 1000.0,
                    jobs_day: 0.0,
                    activity_mix_residential: 1.0,
                    activity_mix_office: 0.0,
                    activity_mix_retail: 0.0,
                    activity_mix_recreation: 0.0,
                    activity_mix_industrial: 0.0,
                    activity_mix_education: 0.0,
                    activity_mix_health: 0.0,
                    centrality_score: 0.5,
                    data_quality_score: 1.0,
                    country_iso2: Some("GB".to_string()),
                },
                DemandCell {
                    cell_id: "CR".to_string(),
                    x: 1000.0,
                    y: 0.0,
                    area_m2: 1_000_000.0,
                    residents_night: 100.0,
                    jobs_day: 250.0,
                    activity_mix_residential: 0.0,
                    activity_mix_office: 0.0,
                    activity_mix_retail: 1.0,
                    activity_mix_recreation: 0.0,
                    activity_mix_industrial: 0.0,
                    activity_mix_education: 0.0,
                    activity_mix_health: 0.0,
                    centrality_score: 0.5,
                    data_quality_score: 1.0,
                    country_iso2: Some("GB".to_string()),
                },
                DemandCell {
                    cell_id: "CO".to_string(),
                    x: 0.0,
                    y: 1000.0,
                    area_m2: 1_000_000.0,
                    residents_night: 100.0,
                    jobs_day: 250.0,
                    activity_mix_residential: 0.0,
                    activity_mix_office: 1.0,
                    activity_mix_retail: 0.0,
                    activity_mix_recreation: 0.0,
                    activity_mix_industrial: 0.0,
                    activity_mix_education: 0.0,
                    activity_mix_health: 0.0,
                    centrality_score: 0.5,
                    data_quality_score: 1.0,
                    country_iso2: Some("GB".to_string()),
                },
            ],
            demand_meta: None,
        },
    }
}

fn link_passengers(out: &SimulationOutput, link_id: &str) -> f64 {
    out.link_loads
        .iter()
        .find(|l| l.link_id == link_id)
        .map(|l| l.passengers)
        .unwrap_or(0.0)
}

#[test]
fn purpose_aware_od_prefers_retail_destination_when_retail_share_dominates() {
    let mut scenario = two_destination_scenario();
    scenario.params.purpose_share_home_work = 0.0;
    scenario.params.purpose_share_home_education = 0.0;
    scenario.params.purpose_share_home_retail = 1.0;
    scenario.params.purpose_share_home_recreation = 0.0;
    scenario.params.purpose_share_other = 0.0;

    let out = run_simulation(&scenario).expect("simulation should run");
    let to_retail = link_passengers(&out, "L_TO_RETAIL");
    let to_office = link_passengers(&out, "L_TO_OFFICE");

    assert!(to_retail > 0.0, "retail flow should be positive");
    assert!(
        to_retail > to_office * 1.15,
        "retail destination should dominate"
    );
}

#[test]
fn purpose_aware_od_prefers_office_destination_when_work_share_dominates() {
    let mut scenario = two_destination_scenario();
    scenario.params.purpose_share_home_work = 1.0;
    scenario.params.purpose_share_home_education = 0.0;
    scenario.params.purpose_share_home_retail = 0.0;
    scenario.params.purpose_share_home_recreation = 0.0;
    scenario.params.purpose_share_other = 0.0;

    let out = run_simulation(&scenario).expect("simulation should run");
    let to_retail = link_passengers(&out, "L_TO_RETAIL");
    let to_office = link_passengers(&out, "L_TO_OFFICE");

    assert!(to_office > 0.0, "office flow should be positive");
    assert!(
        to_office > to_retail * 1.15,
        "office destination should dominate"
    );
}

#[test]
fn planning_time_of_day_purpose_multipliers_shift_od_and_remain_deterministic() {
    let mut scenario = two_destination_scenario();
    scenario.params.purpose_share_home_work = 0.5;
    scenario.params.purpose_share_home_education = 0.0;
    scenario.params.purpose_share_home_retail = 0.5;
    scenario.params.purpose_share_home_recreation = 0.0;
    scenario.params.purpose_share_other = 0.0;
    scenario.params.demand_purpose_profile = vec![
        DemandPurposeTimeSlice {
            label: "am".to_string(),
            start_s: 0.0,
            end_s: 43_200.0,
            home_work_mult: 2.0,
            home_education_mult: 1.0,
            home_retail_mult: 0.2,
            home_recreation_mult: 1.0,
            other_mult: 1.0,
        },
        DemandPurposeTimeSlice {
            label: "pm".to_string(),
            start_s: 43_200.0,
            end_s: 86_399.0,
            home_work_mult: 0.2,
            home_education_mult: 1.0,
            home_retail_mult: 2.0,
            home_recreation_mult: 1.0,
            other_mult: 1.0,
        },
    ];

    let doc = ScenarioDocument::new_current(scenario);

    let am_opts = PlanningRunOptions {
        time_of_day_s: Some(8.0 * 3600.0),
        ..Default::default()
    };
    let pm_opts = PlanningRunOptions {
        time_of_day_s: Some(20.0 * 3600.0),
        ..Default::default()
    };

    let am_out_1 =
        SimulationService::run_planning(&doc, am_opts.clone()).expect("am planning should run");
    let am_out_2 = SimulationService::run_planning(&doc, am_opts).expect("am repeat should run");
    let pm_out = SimulationService::run_planning(&doc, pm_opts).expect("pm planning should run");

    let am_to_retail = link_passengers(&am_out_1, "L_TO_RETAIL");
    let am_to_office = link_passengers(&am_out_1, "L_TO_OFFICE");
    let pm_to_retail = link_passengers(&pm_out, "L_TO_RETAIL");
    let pm_to_office = link_passengers(&pm_out, "L_TO_OFFICE");

    assert!(am_to_office > am_to_retail, "AM should bias office demand");
    assert!(pm_to_retail > pm_to_office, "PM should bias retail demand");

    let delta_am = (am_to_office - link_passengers(&am_out_2, "L_TO_OFFICE")).abs();
    assert!(
        delta_am < 1e-9,
        "same seed/time_of_day should remain deterministic"
    );
}

#[test]
fn step_simulation_uses_midpoint_purpose_multiplier_across_ticks() {
    let mut scenario = two_destination_scenario();
    scenario.params.purpose_share_home_work = 0.5;
    scenario.params.purpose_share_home_education = 0.0;
    scenario.params.purpose_share_home_retail = 0.5;
    scenario.params.purpose_share_home_recreation = 0.0;
    scenario.params.purpose_share_other = 0.0;
    scenario.params.demand_purpose_profile = vec![
        DemandPurposeTimeSlice {
            label: "work_window".to_string(),
            start_s: 0.0,
            end_s: 43_200.0,
            home_work_mult: 2.0,
            home_education_mult: 1.0,
            home_retail_mult: 0.2,
            home_recreation_mult: 1.0,
            other_mult: 1.0,
        },
        DemandPurposeTimeSlice {
            label: "retail_window".to_string(),
            start_s: 43_200.0,
            end_s: 86_399.0,
            home_work_mult: 0.2,
            home_education_mult: 1.0,
            home_retail_mult: 2.0,
            home_recreation_mult: 1.0,
            other_mult: 1.0,
        },
    ];

    let cfg_work_mid = RunConfig {
        clock_start_s: 10.5 * 3600.0,
        ..RunConfig::default()
    };
    let cfg_retail_mid = RunConfig {
        clock_start_s: 11.5 * 3600.0,
        ..RunConfig::default()
    };

    let st_work = init_sim_state(&scenario, &cfg_work_mid);
    let st_work_repeat = init_sim_state(&scenario, &cfg_work_mid);
    let st_retail = init_sim_state(&scenario, &cfg_retail_mid);

    let (out_work_1, _) =
        step_simulation(&scenario, &cfg_work_mid, &st_work, 3600.0).expect("work step should run");
    let (out_work_2, _) = step_simulation(&scenario, &cfg_work_mid, &st_work_repeat, 3600.0)
        .expect("work step repeat should run");
    let (out_retail, _) = step_simulation(&scenario, &cfg_retail_mid, &st_retail, 3600.0)
        .expect("retail step should run");

    let work_to_office = link_passengers(&out_work_1, "L_TO_OFFICE");
    let work_to_retail = link_passengers(&out_work_1, "L_TO_RETAIL");
    let retail_to_office = link_passengers(&out_retail, "L_TO_OFFICE");
    let retail_to_retail = link_passengers(&out_retail, "L_TO_RETAIL");

    assert!(
        work_to_office > work_to_retail,
        "midpoint in work window should favor office demand"
    );
    assert!(
        retail_to_retail > retail_to_office,
        "midpoint in retail window should favor retail demand"
    );

    let repeat_delta = (work_to_office - link_passengers(&out_work_2, "L_TO_OFFICE")).abs();
    assert!(
        repeat_delta < 1e-9,
        "repeated ticks from same state/time should be deterministic"
    );
}
