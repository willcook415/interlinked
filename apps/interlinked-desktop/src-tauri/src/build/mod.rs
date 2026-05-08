mod defaults;
mod fleet_state;
mod inspection_line;
mod inspection_station;
mod mutation_costing;
mod operations_materialization;

pub use defaults::{
    default_build_defaults, BuildDefaults, ComfortLevelPreset, ModeBuildPreset,
    RollingStockTierPreset, SpeedLevelPreset,
};
pub use fleet_state::{
    line_activation_diagnostics, resolve_assigned_units_for_line_mode,
    settle_pending_purchase_orders, FleetPurchaseOrderState, LineActivationDiagnostics,
    LineActivationReason, LineScheduleState,
};
pub use inspection_line::{
    compute_lines, inspect_line_from_scenario, LineComputed, LineCostStory, LineDirectionSummary,
    LineFleetState, LineInspection, LineOperationsNow, LineStationSummary,
};
pub use inspection_station::{
    inspect_station_from_scenario, StationInspection, StationJourneyTime, StationLineSummary,
    StationRuntimeDiagnostics, StationRuntimeServiceDiagnostics,
};
pub use mutation_costing::{
    apply_build_budget, balance_display_amount, mutation_cost_breakdown,
    summarize_network_mutation, MutationCostBreakdown, MutationPathValidationMeta,
    NetworkMutationPreviewResult, NetworkMutationResult, NetworkMutationSummary,
};
pub use operations_materialization::{
    estimate_staff_opex_per_hour_base, materialize_line_operations_for_minute,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectManifest, SessionKind};
    use interlinked_engine::model::{
        Crs, DemandCell, Link, Meta, Params, Scenario, Service, Stop, Transfer, World, Zone,
    };
    use interlinked_engine::sim::{
        BoardLoad, BoardingTimeBin, CitywideModeShareSummary, DemandDiagnostics, Diagnostics,
        EconomicDiagnostics, FareFlowSummary, Kpis, LifecycleConservationSummary, LinkLoad,
        ModalDemandDiagnostics, NetworkFinancialSummary, OutputMeta, ServiceReliabilityDiagnostics,
        SimulationOutput, TemporalDemandDiagnostics, TemporalDemandSlice,
    };

    fn approx(a: f64, b: f64, eps: f64) {
        assert!(
            (a - b).abs() <= eps,
            "expected approx equality: {a} vs {b} (eps {eps})"
        );
    }

    fn test_params() -> Params {
        Params {
            walk_weight: 1.0,
            wait_weight: 2.0,
            ivt_weight: 1.0,
            transfer_penalty_s: 300.0,
            access_walk_speed_mps: 1.4,
            access_radius_m: 1200.0,
            gravity_beta: 0.0003,
            trips_per_person: 1.0,
            purpose_share_home_work: 0.52,
            purpose_share_home_education: 0.12,
            purpose_share_home_retail: 0.18,
            purpose_share_home_recreation: 0.10,
            purpose_share_other: 0.08,
            attraction_weight_office: 1.0,
            attraction_weight_retail: 0.9,
            attraction_weight_recreation: 0.7,
            attraction_weight_industrial: 1.1,
            attraction_weight_education: 0.8,
            attraction_weight_health: 0.75,
            route_choice_k: 3,
            route_choice_theta: 0.002,
            assignment_max_iters: 8,
            assignment_convergence_rel: 0.01,
            capacity_enabled: true,
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
            demand_profile: vec![],
            demand_purpose_profile: vec![],
        }
    }

    fn test_scenario() -> Scenario {
        Scenario {
            meta: Meta {
                name: "Builder Test".to_string(),
                seed: 7,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: test_params(),
            world: World {
                zones: vec![Zone {
                    id: "zone_a".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 2000.0,
                    jobs: 800.0,
                    country_iso2: Some("GB".to_string()),
                }],
                stops: vec![
                    Stop {
                        id: "stop_a".to_string(),
                        name: Some("Alpha".to_string()),
                        x: 0.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "stop_b".to_string(),
                        name: Some("Bravo".to_string()),
                        x: 1000.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: Some("hub-1".to_string()),
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "stop_c".to_string(),
                        name: Some("Charlie".to_string()),
                        x: 2200.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                ],
                links: vec![
                    Link {
                        id: "l_ab".to_string(),
                        from_stop: "stop_a".to_string(),
                        to_stop: "stop_b".to_string(),
                        distance_m: 1000.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_bc".to_string(),
                        from_stop: "stop_b".to_string(),
                        to_stop: "stop_c".to_string(),
                        distance_m: 1200.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_cb".to_string(),
                        from_stop: "stop_c".to_string(),
                        to_stop: "stop_b".to_string(),
                        distance_m: 1200.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_ba".to_string(),
                        from_stop: "stop_b".to_string(),
                        to_stop: "stop_a".to_string(),
                        distance_m: 1000.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                ],
                services: vec![
                    Service {
                        id: "svc_reverse".to_string(),
                        line_id: Some("line:test".to_string()),
                        name: Some("Test Metro".to_string()),
                        mode: "metro".to_string(),
                        mode_variant: None,
                        stop_sequence: vec![
                            "stop_c".to_string(),
                            "stop_b".to_string(),
                            "stop_a".to_string(),
                        ],
                        direction: Some("reverse".to_string()),
                        direction_name: Some("Inbound".to_string()),
                        display_color: Some("#123456".to_string()),
                        service_enabled: Some(true),
                        operating_tph: Some(15.0),
                        stock_tier_id: Some("standard".to_string()),
                        stock_units_owned: Some(3),
                        stock_units_assigned: Some(3),
                        rolling_stock_profile: None,
                        schedule_profile: None,
                        headway_s: 240.0,
                        dwell_s: 30.0,
                        vehicle_capacity: 650.0,
                        board_penalty_s: None,
                    },
                    Service {
                        id: "svc_forward".to_string(),
                        line_id: Some("line:test".to_string()),
                        name: Some("Test Metro".to_string()),
                        mode: "metro".to_string(),
                        mode_variant: None,
                        stop_sequence: vec![
                            "stop_a".to_string(),
                            "stop_b".to_string(),
                            "stop_c".to_string(),
                        ],
                        direction: Some("forward".to_string()),
                        direction_name: Some("Outbound".to_string()),
                        display_color: Some("#123456".to_string()),
                        service_enabled: Some(true),
                        operating_tph: Some(15.0),
                        stock_tier_id: Some("standard".to_string()),
                        stock_units_owned: Some(3),
                        stock_units_assigned: Some(3),
                        rolling_stock_profile: None,
                        schedule_profile: None,
                        headway_s: 240.0,
                        dwell_s: 30.0,
                        vehicle_capacity: 650.0,
                        board_penalty_s: None,
                    },
                ],
                transfers: vec![Transfer {
                    from_stop: "stop_a".to_string(),
                    to_stop: "stop_b".to_string(),
                    time_s: 120.0,
                    penalty_s: None,
                    allowed_modes: None,
                }],
                transfer_rules: None,
                demand_cells: vec![
                    DemandCell {
                        cell_id: "dc_b_core".to_string(),
                        x: 1000.0,
                        y: 0.0,
                        area_m2: 350_000.0,
                        residents_night: 1800.0,
                        jobs_day: 900.0,
                        activity_mix_residential: 0.55,
                        activity_mix_office: 0.22,
                        activity_mix_retail: 0.11,
                        activity_mix_recreation: 0.05,
                        activity_mix_industrial: 0.04,
                        activity_mix_education: 0.02,
                        activity_mix_health: 0.01,
                        centrality_score: 0.7,
                        data_quality_score: 0.9,
                        country_iso2: Some("GB".to_string()),
                        allocation_diagnostics: None,
                    },
                    DemandCell {
                        cell_id: "dc_b_east".to_string(),
                        x: 1450.0,
                        y: 40.0,
                        area_m2: 300_000.0,
                        residents_night: 900.0,
                        jobs_day: 1400.0,
                        activity_mix_residential: 0.25,
                        activity_mix_office: 0.30,
                        activity_mix_retail: 0.20,
                        activity_mix_recreation: 0.10,
                        activity_mix_industrial: 0.08,
                        activity_mix_education: 0.04,
                        activity_mix_health: 0.03,
                        centrality_score: 0.75,
                        data_quality_score: 0.9,
                        country_iso2: Some("GB".to_string()),
                        allocation_diagnostics: None,
                    },
                ],
                demand_meta: None,
            },
        }
    }

    fn test_output() -> SimulationOutput {
        SimulationOutput {
            meta: OutputMeta {
                results_version: "test".to_string(),
                scenario_name: "Builder Test".to_string(),
                seed: 7,
                time_period_hours: 1.0,
            },
            kpis: Kpis {
                total_trips_attempted: 100.0,
                total_trips_served: 90.0,
                share_trips_served: 0.9,
                total_trips: 90.0,
                mean_generalized_cost_s: 600.0,
                mean_in_vehicle_time_s: 240.0,
                mean_wait_time_s: 120.0,
                mean_walk_time_s: 80.0,
                mean_transfer_time_s: 0.0,
                mean_transfer_penalty_s: 0.0,
                mean_transfers: 0.0,
                mean_boardings: 1.0,
                total_boardings_attempted: 90.0,
                total_boardings_served: 88.0,
                total_boardings_denied: 2.0,
                share_boardings_served: 88.0 / 90.0,
                total_fare_revenue_base: 140.0,
                total_overflow_dropped: 1.0,
                share_demand_overflow_dropped: 1.0 / 90.0,
            },
            link_loads: vec![
                LinkLoad {
                    link_id: "l_ab".to_string(),
                    from_stop: "stop_a".to_string(),
                    to_stop: "stop_b".to_string(),
                    mode: "metro".to_string(),
                    passengers: 120.0,
                    capacity_per_hour: None,
                    capacity_in_period: 160.0,
                    load_to_capacity: 0.75,
                    crowding_penalty_s: 0.0,
                },
                LinkLoad {
                    link_id: "l_bc".to_string(),
                    from_stop: "stop_b".to_string(),
                    to_stop: "stop_c".to_string(),
                    mode: "metro".to_string(),
                    passengers: 80.0,
                    capacity_per_hour: None,
                    capacity_in_period: 160.0,
                    load_to_capacity: 0.5,
                    crowding_penalty_s: 0.0,
                },
            ],
            board_loads: vec![BoardLoad {
                service_id: "svc_forward".to_string(),
                stop_id: "stop_b".to_string(),
                arrivals: 45.0,
                served_from_arrivals: 40.0,
                served_from_queue: 3.0,
                denied_boardings: 2.0,
                queue_start: 4.0,
                queue_end: 6.0,
                headway_s: 240.0,
                vehicle_capacity: 650.0,
                departures_in_period: 15.0,
                departures_observed: 1,
                capacity_in_period: 9750.0,
                extra_wait_s: 15.0,
                time_bins: vec![BoardingTimeBin {
                    bin_index: 0,
                    arrivals: 45.0,
                    served: 43.0,
                    queue_end: 6.0,
                    departures: 1,
                    capacity: 650.0,
                }],
                time_to_next_departure_s_end: 120.0,
                alightings_served: 21.0,
                station_capacity_boarding_pph: 20_000.0,
                station_capacity_alighting_pph: 22_000.0,
                station_queue_capacity_pax: 3500.0,
                overflow_dropped: 1.0,
            }],
            stop_flows: vec![],
            passenger_cohorts: vec![],
            fare_flow: FareFlowSummary::default(),
            lifecycle_conservation: LifecycleConservationSummary::default(),
            zone_demand_profiles: vec![],
            latent_od_demand: vec![],
            assigned_od_flows: vec![],
            mode_choice_results: vec![],
            stop_flow_states: vec![],
            vehicle_load_states: vec![],
            service_operation_states: vec![],
            stop_operation_states: vec![],
            transfer_operation_metrics: vec![],
            service_reliability_diagnostics: ServiceReliabilityDiagnostics::default(),
            synthetic_economy_config: None,
            zone_demand_layer: vec![],
            zone_economic_geography_layer: vec![],
            zone_demand_production_layer: vec![],
            zone_demand_attraction_layer: vec![],
            corridor_desire_lines: vec![],
            service_gap_layer: vec![],
            service_load_layer: vec![],
            planning_overlay_config: None,
            zone_planning_metrics: vec![],
            station_planning_metrics: vec![],
            corridor_planning_metrics: vec![],
            line_service_planning_metrics: vec![],
            network_financial_summary: NetworkFinancialSummary::default(),
            service_financial_metrics: vec![],
            corridor_financial_metrics: vec![],
            station_financial_context: vec![],
            zone_mode_share_metrics: vec![],
            corridor_mode_share_metrics: vec![],
            station_transit_capture_context: vec![],
            service_transit_capture_context: vec![],
            citywide_mode_share_summary: CitywideModeShareSummary::default(),
            build_preview_metrics: vec![],
            service_gap_rankings: interlinked_engine::sim::ServiceGapRankings::default(),
            planning_debug_summary: interlinked_engine::sim::PlanningDebugSummary::default(),
            demand_diagnostics: DemandDiagnostics::default(),
            active_temporal_slice: TemporalDemandSlice::default(),
            temporal_planning_snapshots: vec![],
            temporal_demand_diagnostics: TemporalDemandDiagnostics::default(),
            modal_demand_diagnostics: ModalDemandDiagnostics::default(),
            economic_diagnostics: EconomicDiagnostics::default(),
            diagnostics: Diagnostics {
                zones: 1,
                stops: 3,
                links: 4,
                services: 2,
                transfers: 1,
                access_edges: 0,
                egress_edges: 0,
                msa_iterations: 1,
                msa_final_max_rel_change: 0.0,
                sample_paths: vec![],
                planner_passenger_trace: interlinked_engine::sim::PlannerPassengerTrace::default(),
            },
        }
    }

    fn test_manifest(balance_base: f64) -> ProjectManifest {
        ProjectManifest {
            project_id: "p1".to_string(),
            name: "Builder Test".to_string(),
            created_at: "0".to_string(),
            updated_at: "0".to_string(),
            session_kind: SessionKind::Game,
            engine_schema_version: 1,
            ui_schema_version: 1,
            last_opened_run_id: None,
            recent_runs: vec![],
            clock_state: crate::SimulationClock {
                sim_datetime_utc: "2026-01-01T08:00:00Z".to_string(),
                tick_seconds: 0.0,
                running: false,
                speed: 1,
            },
            progress_metrics: Some(crate::GameProgressMetrics {
                budget: balance_base,
                currency: "GBP".to_string(),
                ridership: 0.0,
                coverage: 0.0,
                milestones: 0,
            }),
            start_location: None,
            economy: crate::EconomyManifest {
                currency: "GBP".to_string(),
                difficulty: "standard".to_string(),
                difficulty_profile: crate::DifficultyProfile {
                    profile_id: "standard".to_string(),
                    demand_mult: 1.0,
                    capex_mult: 1.0,
                    opex_mult: 1.0,
                    maintenance_mult: 1.0,
                    penalty_mult: 1.0,
                    ancillary_revenue_mult: 1.0,
                    unlock_cost_mult: 1.0,
                },
                economy_revision: 1,
                starting_budget_base: balance_base,
                current_balance_base: balance_base,
                cumulative_capex_base: 0.0,
                cumulative_opex_base: 0.0,
                cumulative_revenue_base: 0.0,
                cumulative_lost_demand_penalty_base: 0.0,
                fare_revenue_deferred_base: 0.0,
                fare_boardings_deferred_pax: 0.0,
                fare_policy: crate::default_fare_policy_manifest(),
                unlocked_countries: vec!["GB".to_string()],
                region_ledger: std::collections::BTreeMap::new(),
                maintenance_rate: crate::default_maintenance_rate(),
                ancillary_revenue_rate: crate::default_ancillary_revenue_rate(),
                quality_penalty_rates: crate::default_quality_penalty_rates(),
                monthly_financials: Vec::new(),
            },
            demand_surface: None,
            region_state: crate::RegionStateManifest::default(),
            simulation_scope: crate::default_simulation_scope_manifest(),
            runtime_scheduling: crate::default_runtime_scheduling_manifest(),
            pack_refs: vec![],
        }
    }

    #[test]
    fn compute_lines_uses_forward_service_for_station_order() {
        let scenario = test_scenario();
        let lines = compute_lines(&scenario);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.line_id, "line:test");
        assert_eq!(
            line.station_ids,
            vec![
                "stop_a".to_string(),
                "stop_b".to_string(),
                "stop_c".to_string()
            ]
        );
        assert_eq!(line.directions.len(), 2);
        assert_eq!(line.display_color.as_deref(), Some("#123456"));
        approx(
            *line
                .cumulative_time_s_by_stop_id
                .get("stop_c")
                .expect("stop_c travel time"),
            170.0,
            1e-6,
        );
    }

    #[test]
    fn station_inspection_reports_zero_based_position_and_live_metrics() {
        let scenario = test_scenario();
        let output = test_output();
        let inspection = inspect_station_from_scenario(&scenario, Some(&output), "stop_b")
            .expect("station inspection");
        assert_eq!(inspection.name, "Bravo");
        approx(inspection.boardings_attempted, 45.0, 1e-6);
        approx(inspection.boardings_served, 43.0, 1e-6);
        approx(inspection.denied_boardings, 2.0, 1e-6);
        approx(inspection.queue_end, 6.0, 1e-6);
        assert_eq!(inspection.served_lines.len(), 1);
        let line = &inspection.served_lines[0];
        assert_eq!(line.station_index, 1);
        assert_eq!(line.station_count, 3);
        assert_eq!(line.previous_station_name.as_deref(), Some("Alpha"));
        assert_eq!(line.next_station_name.as_deref(), Some("Charlie"));
        assert_eq!(line.journey_times.len(), 2);
        approx(line.journey_times[0].travel_time_s, 80.0, 1e-6);
        assert!(inspection.catchment_cells >= 1);
        assert!(inspection.catchment_residents > 0.0);
        assert!(inspection.catchment_jobs > 0.0);
        let catchment_mix_sum = inspection.catchment_mix_residential
            + inspection.catchment_mix_office
            + inspection.catchment_mix_retail
            + inspection.catchment_mix_recreation
            + inspection.catchment_mix_industrial
            + inspection.catchment_mix_education
            + inspection.catchment_mix_health;
        assert!((catchment_mix_sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn station_inspection_uses_nearest_cells_when_radius_is_empty() {
        let mut scenario = test_scenario();
        scenario.params.access_radius_m = 120.0;
        for c in &mut scenario.world.demand_cells {
            c.x += 40_000.0;
            c.y += 40_000.0;
        }
        let inspection =
            inspect_station_from_scenario(&scenario, None, "stop_b").expect("station inspection");
        assert!(inspection.catchment_cells > 0);
        assert!(inspection.catchment_mix_residential < 0.9);
        let non_residential = inspection.catchment_mix_office
            + inspection.catchment_mix_retail
            + inspection.catchment_mix_recreation
            + inspection.catchment_mix_industrial
            + inspection.catchment_mix_education
            + inspection.catchment_mix_health;
        assert!(non_residential > 0.1);
    }

    #[test]
    fn line_inspection_sums_line_loads_and_estimates_opex() {
        let scenario = test_scenario();
        let output = test_output();
        let cfg = interlinked_engine::platform::default_economy_config();
        let inspection =
            inspect_line_from_scenario(&scenario, Some(&output), "line:test", &cfg, Some(540))
                .expect("line inspection");
        assert_eq!(inspection.station_count, 3);
        assert_eq!(inspection.service_count, 2);
        approx(inspection.total_passengers, 200.0, 1e-6);
        assert!(!inspection.stations.is_empty());
        assert!(inspection.estimated_opex_per_hour_base > 0.0);
    }

    #[test]
    fn mutation_summary_charges_only_positive_delta() {
        let current = test_scenario();
        let mut expanded = current.clone();
        expanded.world.stops.push(Stop {
            id: "stop_d".to_string(),
            name: Some("Delta".to_string()),
            x: 3200.0,
            y: 0.0,
            country_iso2: Some("GB".to_string()),
            interchange_id: None,
            stop_type: Some("metro_station".to_string()),
            station_boarding_capacity_pph: None,
            station_alighting_capacity_pph: None,
            station_queue_capacity_pax: None,
        });
        expanded.world.links.push(Link {
            id: "l_cd".to_string(),
            from_stop: "stop_c".to_string(),
            to_stop: "stop_d".to_string(),
            distance_m: 1000.0,
            mode: "metro".to_string(),
            speed_mps: 20.0,
            geometry: None,
            line_id: Some("line:test".to_string()),
            mode_variant: None,
            capacity_per_hour: None,
        });
        let cfg = interlinked_engine::platform::default_economy_config();
        let expanded_summary = summarize_network_mutation(&current, &expanded, &cfg, None);
        assert!(expanded_summary.capex_delta_base > 0.0);

        let reduced_summary = summarize_network_mutation(&expanded, &current, &cfg, None);
        approx(reduced_summary.capex_delta_base, 0.0, 1e-6);
    }

    #[test]
    fn apply_build_budget_updates_balance_and_rejects_overspend() {
        let cfg = interlinked_engine::platform::default_economy_config();
        let mut manifest = test_manifest(1_000_000.0);
        let summary = NetworkMutationSummary {
            capex_delta_base: 250_000.0,
            infra_capex_delta_base: 250_000.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 250_000.0,
            construction_cost_delta_base: 250_000.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 250_000.0,
            projected_balance_after_apply_base: Some(750_000.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        apply_build_budget(&mut manifest, &cfg, &summary, None).expect("budget application");
        approx(manifest.economy.current_balance_base, 750_000.0, 1e-6);
        approx(manifest.economy.cumulative_capex_base, 250_000.0, 1e-6);

        let mut poor_manifest = test_manifest(100.0);
        let poor_summary = NetworkMutationSummary {
            capex_delta_base: 200.0,
            infra_capex_delta_base: 200.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 200.0,
            construction_cost_delta_base: 200.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 200.0,
            projected_balance_after_apply_base: Some(-100.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        let error = apply_build_budget(&mut poor_manifest, &cfg, &poor_summary, None)
            .expect_err("overspend should fail");
        assert!(error.contains("Insufficient funds"));
        approx(poor_manifest.economy.current_balance_base, 100.0, 1e-6);
    }

    #[test]
    fn apply_build_budget_ignores_zero_override_and_uses_summary_delta() {
        let cfg = interlinked_engine::platform::default_economy_config();
        let mut manifest = test_manifest(1_000_000.0);
        let summary = NetworkMutationSummary {
            capex_delta_base: 180_000.0,
            infra_capex_delta_base: 180_000.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 180_000.0,
            construction_cost_delta_base: 180_000.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 180_000.0,
            projected_balance_after_apply_base: Some(820_000.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        apply_build_budget(&mut manifest, &cfg, &summary, Some(0.0))
            .expect("zero override should not suppress capex");
        approx(manifest.economy.current_balance_base, 820_000.0, 1e-6);
        approx(manifest.economy.cumulative_capex_base, 180_000.0, 1e-6);
    }

    #[test]
    fn mutation_summary_charges_pending_order_commitment() {
        let current = test_scenario();
        let mut next = current.clone();
        for service in &mut next.world.services {
            service.rolling_stock_profile = Some(interlinked_engine::model::RollingStockProfile {
                package_id: Some("standard".to_string()),
                units_owned: Some(3),
                cars_per_unit: Some(8),
                speed_level: Some("balanced".to_string()),
                comfort_level: Some("standard".to_string()),
                pending_orders: vec![interlinked_engine::model::PurchaseOrder {
                    order_id: "po:test".to_string(),
                    units: 2,
                    status: Some("pending".to_string()),
                    unit_cost_base: Some(0.0),
                    total_cost_base: Some(1_750_000.0),
                    placed_at_tick_s: Some(0.0),
                    eta_at_tick_s: Some(21_600.0),
                }],
            });
        }
        let cfg = interlinked_engine::platform::default_economy_config();
        let summary = summarize_network_mutation(&current, &next, &cfg, None);
        approx(summary.fleet_purchase_base, 1_750_000.0, 1e-6);
        approx(
            summary.apply_total_delta_base,
            summary.net_capex_delta_base,
            1e-6,
        );
        assert!(summary.apply_total_delta_base >= 1_750_000.0);
    }

    #[test]
    fn settles_due_pending_orders_into_owned_units() {
        let mut scenario = test_scenario();
        for service in &mut scenario.world.services {
            service.stock_units_owned = Some(2);
            service.stock_units_assigned = Some(2);
            service.rolling_stock_profile = Some(interlinked_engine::model::RollingStockProfile {
                package_id: Some("standard".to_string()),
                units_owned: Some(2),
                cars_per_unit: Some(8),
                speed_level: Some("balanced".to_string()),
                comfort_level: Some("standard".to_string()),
                pending_orders: vec![interlinked_engine::model::PurchaseOrder {
                    order_id: "po:deliver".to_string(),
                    units: 3,
                    status: Some("pending".to_string()),
                    unit_cost_base: Some(500_000.0),
                    total_cost_base: Some(1_500_000.0),
                    placed_at_tick_s: Some(300.0),
                    eta_at_tick_s: Some(1200.0),
                }],
            });
        }

        let before_eta = settle_pending_purchase_orders(&mut scenario, 900.0);
        assert_eq!(before_eta, 0);
        assert_eq!(scenario.world.services[0].stock_units_owned, Some(2));
        assert_eq!(scenario.world.services[0].stock_units_assigned, Some(2));

        let delivered = settle_pending_purchase_orders(&mut scenario, 1200.0);
        assert_eq!(delivered, 3);
        for service in &scenario.world.services {
            assert_eq!(service.stock_units_owned, Some(5));
            assert_eq!(service.stock_units_assigned, Some(5));
            assert_eq!(
                service
                    .rolling_stock_profile
                    .as_ref()
                    .map(|profile| profile.pending_orders.len())
                    .unwrap_or_default(),
                0
            );
        }
    }
}
