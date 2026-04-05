use super::*;
use crate::sim::types;

fn canonical_mode_priority(mode: CanonicalTransitMode) -> u8 {
    match mode {
        CanonicalTransitMode::HighSpeedRail => 7,
        CanonicalTransitMode::RegionalRail => 6,
        CanonicalTransitMode::SuburbanRail => 5,
        CanonicalTransitMode::Metro => 4,
        CanonicalTransitMode::Tram => 3,
        CanonicalTransitMode::Ferry => 2,
        CanonicalTransitMode::Bus => 1,
        CanonicalTransitMode::OtherTransit => 0,
    }
}

fn dominant_canonical_mode(
    modes: Option<&Vec<CanonicalTransitMode>>,
) -> Option<CanonicalTransitMode> {
    modes.and_then(|items| {
        items
            .iter()
            .copied()
            .max_by_key(|mode| canonical_mode_priority(*mode))
    })
}

fn classify_commercial_strength(
    farebox: f64,
    full_surplus: f64,
    policy: &types::EconomicsPolicyConfig,
) -> CommercialStrengthClassification {
    if full_surplus >= 0.0 || farebox >= policy.commercial_strong_farebox_threshold.max(0.0) {
        CommercialStrengthClassification::Strong
    } else if farebox >= policy.commercial_viable_farebox_threshold.max(0.0) {
        CommercialStrengthClassification::Viable
    } else if farebox >= policy.commercial_marginal_farebox_threshold.max(0.0) {
        CommercialStrengthClassification::Marginal
    } else {
        CommercialStrengthClassification::Weak
    }
}

fn classify_social_necessity_for_service(
    service_role: ServiceRoleClassification,
    mode: TravelMode,
    ridership: f64,
    policy: &types::EconomicsPolicyConfig,
) -> SocialNecessityClassification {
    if matches!(
        service_role,
        ServiceRoleClassification::UrbanTrunk
            | ServiceRoleClassification::CommuterRadial
            | ServiceRoleClassification::Intercity
    ) && ridership >= 220.0
    {
        return SocialNecessityClassification::Core;
    }
    if matches!(
        service_role,
        ServiceRoleClassification::LocalCoverage | ServiceRoleClassification::RegionalConnector
    ) || (mode == TravelMode::Bus && ridership <= 150.0)
    {
        return SocialNecessityClassification::Important;
    }
    if ridership >= (120.0 * policy.social_necessity_essential_threshold.max(0.05)) {
        SocialNecessityClassification::Supportive
    } else {
        SocialNecessityClassification::Low
    }
}

fn classify_social_necessity_for_corridor(
    corridor: &CorridorPlanningMetrics,
    policy: &types::EconomicsPolicyConfig,
) -> SocialNecessityClassification {
    if corridor.corridor_classification == CorridorClassification::RuralEssentialConnector
        || corridor.dominant_purpose == TripPurpose::Essential
    {
        return if corridor.served_ratio <= policy.social_necessity_rural_threshold.max(0.0) {
            SocialNecessityClassification::Core
        } else {
            SocialNecessityClassification::Important
        };
    }
    if matches!(
        corridor.corridor_classification,
        CorridorClassification::Intercity
            | CorridorClassification::SuburbanCommuterRadial
            | CorridorClassification::AirportAccess
    ) {
        return SocialNecessityClassification::Supportive;
    }
    SocialNecessityClassification::Low
}

fn compute_financial_performance(
    fare_revenue: f64,
    operating_cost: f64,
    infrastructure_cost_allocated: f64,
    rolling_stock_cost_allocated: f64,
    riders: f64,
    passenger_km: f64,
    social_value_proxy: f64,
) -> FinancialPerformanceMetrics {
    let total_cost =
        (operating_cost + infrastructure_cost_allocated + rolling_stock_cost_allocated).max(0.0);
    let operating_surplus_deficit = fare_revenue - operating_cost;
    let full_cost_surplus_deficit = fare_revenue - total_cost;
    let subsidy_required = (-full_cost_surplus_deficit).max(0.0);
    let farebox_recovery_ratio = if operating_cost > 0.0 {
        (fare_revenue / operating_cost).max(0.0)
    } else {
        0.0
    };
    let cost_per_passenger = if riders > 0.0 {
        (total_cost / riders).max(0.0)
    } else {
        0.0
    };
    let cost_per_passenger_km = if passenger_km > 0.0 {
        (total_cost / passenger_km).max(0.0)
    } else {
        0.0
    };
    let revenue_per_passenger = if riders > 0.0 {
        (fare_revenue / riders).max(0.0)
    } else {
        0.0
    };

    FinancialPerformanceMetrics {
        fare_revenue: fare_revenue.max(0.0),
        operating_cost: operating_cost.max(0.0),
        infrastructure_cost_allocated: infrastructure_cost_allocated.max(0.0),
        rolling_stock_cost_allocated: rolling_stock_cost_allocated.max(0.0),
        total_cost: total_cost.max(0.0),
        operating_surplus_deficit,
        full_cost_surplus_deficit,
        subsidy_required: subsidy_required.max(0.0),
        farebox_recovery_ratio: farebox_recovery_ratio.max(0.0),
        cost_per_passenger: cost_per_passenger.max(0.0),
        cost_per_passenger_km: cost_per_passenger_km.max(0.0),
        revenue_per_passenger: revenue_per_passenger.max(0.0),
        social_value_proxy: social_value_proxy.max(0.0),
    }
}

fn service_cost_profile_for_mode(
    cfg: &SyntheticEconomyConfig,
    mode: TravelMode,
) -> types::ServiceCostProfile {
    cfg.service_cost_profiles
        .iter()
        .find(|x| x.mode_family == mode)
        .cloned()
        .or_else(|| {
            cfg.service_cost_profiles
                .iter()
                .find(|x| x.mode_family == TravelMode::OtherTransit)
                .cloned()
        })
        .unwrap_or(types::ServiceCostProfile {
            mode_family: mode,
            fixed_cost_per_period: 60.0,
            vehicle_hour_cost: 90.0,
            vehicle_km_cost: 2.2,
            crew_cost_proxy_per_vehicle_hour: 28.0,
            energy_cost_proxy_per_vehicle_km: 0.8,
            maintenance_cost_proxy_per_vehicle_km: 0.5,
            station_stop_call_cost: 0.3,
            peak_uplift_multiplier: 1.1,
            reliability_penalty_uplift: 0.2,
        })
}

fn infrastructure_cost_profile_for_mode(
    cfg: &SyntheticEconomyConfig,
    mode: TravelMode,
) -> types::InfrastructureCostProfile {
    cfg.infrastructure_cost_profiles
        .iter()
        .find(|x| x.mode_family == mode)
        .cloned()
        .or_else(|| {
            cfg.infrastructure_cost_profiles
                .iter()
                .find(|x| x.mode_family == TravelMode::OtherTransit)
                .cloned()
        })
        .unwrap_or(types::InfrastructureCostProfile {
            mode_family: mode,
            track_km_capex: 1_200_000.0,
            station_capex: 1_000_000.0,
            stop_capex: 80_000.0,
            complexity_multiplier: 1.0,
            annualized_maintenance_cost_per_km: 30_000.0,
            infrastructure_renewal_cost_per_km: 15_000.0,
        })
}

fn rolling_stock_cost_profile_for_mode(
    cfg: &SyntheticEconomyConfig,
    mode: TravelMode,
) -> types::RollingStockCostProfile {
    cfg.rolling_stock_cost_profiles
        .iter()
        .find(|x| x.mode_family == mode)
        .cloned()
        .or_else(|| {
            cfg.rolling_stock_cost_profiles
                .iter()
                .find(|x| x.mode_family == TravelMode::OtherTransit)
                .cloned()
        })
        .unwrap_or(types::RollingStockCostProfile {
            mode_family: mode,
            purchase_cost_per_vehicle: 1_000_000.0,
            lease_cost_per_period: 120.0,
            annualized_capital_cost_per_vehicle: 120_000.0,
            maintenance_cost_per_vehicle_period: 40.0,
            capacity_reference: 120.0,
            operating_efficiency: 1.0,
        })
}

fn fare_for_assigned_path(
    cfg: &SyntheticEconomyConfig,
    fare_model: FareModel,
    path_distance_km: f64,
    mode_families: &[TravelMode],
    service_count: usize,
) -> f64 {
    let fare_cfg = &cfg.fare_model_config;
    let mut base = match fare_model {
        FareModel::FlatFare => fare_cfg.flat_fare_base.max(0.0),
        FareModel::DistanceBased | FareModel::TransferDiscount => {
            fare_cfg.distance_fare_base.max(0.0)
                + path_distance_km.max(0.0) * fare_cfg.distance_fare_per_km.max(0.0)
        }
        FareModel::ZoneBased => {
            let zone_steps = (path_distance_km / 5.0).ceil().max(1.0);
            fare_cfg.flat_fare_base.max(0.0) + zone_steps * fare_cfg.zone_step_fare_base.max(0.0)
        }
        FareModel::ModeBased => fare_cfg.flat_fare_base.max(0.0),
    };

    let mut additive = 0.0_f64;
    let mut multiplier = 1.0_f64;
    let mut seen_modes = std::collections::HashSet::<TravelMode>::new();
    for mode in mode_families {
        if !seen_modes.insert(*mode) {
            continue;
        }
        if let Some(sup) = fare_cfg.mode_supplements.iter().find(|x| x.mode == *mode) {
            additive += sup.additive_base.max(0.0);
            multiplier *= sup.multiplier.max(0.25);
        }
    }
    base = (base + additive).max(0.0) * multiplier.max(0.25);

    if service_count > 1 {
        let discounted_count = (service_count - 1).min(fare_cfg.transfer_discount_max_count) as f64;
        let discount =
            (fare_cfg.transfer_discount_rate.max(0.0) * discounted_count).clamp(0.0, 0.75);
        base *= 1.0 - discount;
    }

    base.max(0.0)
}

pub(super) fn build_economics_outputs(
    s: &Scenario,
    active_context: &TemporalDemandSlice,
    economy_cfg: &SyntheticEconomyConfig,
    assigned_od_flows: &[AssignedOdFlow],
    line_service_planning_metrics: &[LineOrServicePlanningMetrics],
    corridor_planning_metrics: &[CorridorPlanningMetrics],
    station_planning_metrics: &[StationPlanningMetrics],
    zone_planning_metrics: &[ZonePlanningMetrics],
    operations_outputs: &OperationsOutputs,
) -> EconomicsOutputs {
    let mut out = EconomicsOutputs::default();
    if s.world.services.is_empty() {
        return out;
    }

    #[derive(Debug, Clone, Default)]
    struct ServiceAggregate {
        fare_revenue: f64,
        ridership_from_paths: f64,
        passenger_km_from_paths: f64,
        vehicle_km: f64,
        vehicle_hours: f64,
        operating_cost: f64,
        rolling_stock_cost: f64,
        infrastructure_cost: f64,
        reliability_cost_uplift: f64,
        mode: TravelMode,
        role: ServiceRoleClassification,
        line_id: Option<String>,
    }

    #[derive(Debug, Clone, Default)]
    struct CorridorAggregate {
        fare_revenue: f64,
        demand_served: f64,
        passenger_km: f64,
    }

    let service_metric_by_id = line_service_planning_metrics
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let service_ops_by_id = operations_outputs
        .service_operation_states
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let corridor_metric_by_pair = corridor_planning_metrics
        .iter()
        .map(|x| ((x.origin_zone_id.clone(), x.destination_zone_id.clone()), x))
        .collect::<HashMap<_, _>>();
    let zone_metric_by_id = zone_planning_metrics
        .iter()
        .map(|x| (x.zone_id.clone(), x))
        .collect::<HashMap<_, _>>();

    let link_by_id = s
        .world
        .links
        .iter()
        .map(|x| (x.id.clone(), x))
        .collect::<HashMap<_, _>>();

    let mut link_to_services = HashMap::<String, Vec<String>>::new();
    let mut service_mode = HashMap::<String, TravelMode>::new();
    let mut service_link_distance = HashMap::<(String, String), f64>::new();
    let mut service_stop_count = HashMap::<String, usize>::new();
    let mut stop_serving_modes = HashMap::<String, Vec<CanonicalTransitMode>>::new();

    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) {
            continue;
        }
        let endpoint_dist_km =
            if let (Some(a), Some(b)) = (svc.stop_sequence.first(), svc.stop_sequence.last()) {
                let sa = s.world.stops.iter().find(|x| x.id == *a);
                let sb = s.world.stops.iter().find(|x| x.id == *b);
                if let (Some(sa), Some(sb)) = (sa, sb) {
                    euclid_km((sa.x, sa.y), (sb.x, sb.y))
                } else {
                    0.0
                }
            } else {
                0.0
            };
        let canonical_mode =
            canonical_mode_from_tokens(&svc.mode, svc.mode_variant.as_deref(), endpoint_dist_km);
        let mode = canonical_mode.travel_mode_family();
        service_mode.insert(svc.id.clone(), mode);
        service_stop_count.insert(svc.id.clone(), svc.stop_sequence.len());
        for stop_id in &svc.stop_sequence {
            stop_serving_modes
                .entry(stop_id.clone())
                .or_default()
                .push(canonical_mode);
        }

        for win in svc.stop_sequence.windows(2) {
            let from = &win[0];
            let to = &win[1];
            for link in &s.world.links {
                if &link.from_stop == from && &link.to_stop == to {
                    link_to_services
                        .entry(link.id.clone())
                        .or_default()
                        .push(svc.id.clone());
                    service_link_distance.insert(
                        (svc.id.clone(), link.id.clone()),
                        (link.distance_m / 1000.0).max(0.0),
                    );
                }
            }
        }
    }

    let mut service_aggs = HashMap::<String, ServiceAggregate>::new();
    let mut corridor_aggs = HashMap::<(String, String, TripPurpose), CorridorAggregate>::new();

    let period_h = s.meta.time_period_hours.max(1e-9);
    let period_s = period_h * 3600.0;
    let peak_multiplier = if matches!(
        active_context.time_slice,
        DemandTimeSliceLabel::AmPeak | DemandTimeSliceLabel::PmPeak
    ) {
        1.0
    } else {
        0.0
    };

    let policy = &economy_cfg.economics_policy_config;
    let fare_model = economy_cfg.fare_model_config.fare_model;

    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) {
            continue;
        }
        let mode = service_mode
            .get(&svc.id)
            .copied()
            .unwrap_or(TravelMode::OtherTransit);
        let service_profile = service_cost_profile_for_mode(economy_cfg, mode);
        let rolling_profile = rolling_stock_cost_profile_for_mode(economy_cfg, mode);
        let reliability = service_ops_by_id
            .get(&svc.id)
            .map(|x| x.reliability_score.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let reliability_penalty = (1.0 - reliability).max(0.0);

        let departures = if let Some(tph) = svc.operating_tph {
            if tph.is_finite() && tph > 0.0 {
                tph * period_h
            } else if svc.headway_s > 0.0 {
                period_s / svc.headway_s
            } else {
                0.0
            }
        } else if svc.headway_s > 0.0 {
            period_s / svc.headway_s
        } else {
            0.0
        }
        .max(0.0);

        let mut route_km = 0.0_f64;
        let mut runtime_s = 0.0_f64;
        for win in svc.stop_sequence.windows(2) {
            let mut found = false;
            for link in &s.world.links {
                if link.from_stop == win[0] && link.to_stop == win[1] {
                    route_km += (link.distance_m / 1000.0).max(0.0);
                    if link.speed_mps > 0.0 {
                        runtime_s += (link.distance_m / link.speed_mps).max(0.0);
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                let a = s.world.stops.iter().find(|x| x.id == win[0]);
                let b = s.world.stops.iter().find(|x| x.id == win[1]);
                if let (Some(a), Some(b)) = (a, b) {
                    let d = euclid_m((a.x, a.y), (b.x, b.y));
                    route_km += (d / 1000.0).max(0.0);
                    runtime_s += (d / 12.0).max(0.0);
                }
            }
        }
        runtime_s += svc.dwell_s.max(0.0) * svc.stop_sequence.len() as f64;

        let vehicle_km = (route_km * departures).max(0.0);
        let vehicle_hours = ((runtime_s / 3600.0) * departures).max(0.0);
        let stop_calls = (svc.stop_sequence.len() as f64 * departures).max(0.0);
        let peak_uplift =
            1.0 + peak_multiplier * (service_profile.peak_uplift_multiplier.max(1.0) - 1.0);
        let operating_base = service_profile.fixed_cost_per_period.max(0.0)
            + vehicle_hours
                * (service_profile.vehicle_hour_cost.max(0.0)
                    + service_profile.crew_cost_proxy_per_vehicle_hour.max(0.0))
            + vehicle_km
                * (service_profile.vehicle_km_cost.max(0.0)
                    + service_profile.energy_cost_proxy_per_vehicle_km.max(0.0)
                    + service_profile
                        .maintenance_cost_proxy_per_vehicle_km
                        .max(0.0))
            + stop_calls * service_profile.station_stop_call_cost.max(0.0);
        let reliability_cost_uplift = operating_base
            * service_profile.reliability_penalty_uplift.max(0.0)
            * reliability_penalty;
        let operating_cost = (operating_base + reliability_cost_uplift) * peak_uplift;

        let vehicles_required = if let Some(units) = svc.stock_units_assigned {
            (units as f64).max(1.0)
        } else if svc.headway_s > 0.0 {
            (runtime_s / svc.headway_s).ceil().max(1.0)
        } else {
            1.0
        };
        let rolling_stock_cost = vehicles_required
            * (rolling_profile.annualized_capital_cost_per_vehicle.max(0.0)
                * policy.capital_annualization_factor.max(0.0)
                + rolling_profile.lease_cost_per_period.max(0.0) * period_h
                + rolling_profile.maintenance_cost_per_vehicle_period.max(0.0) * period_h);

        let default_role = match mode {
            TravelMode::Bus => ServiceRoleClassification::LocalCoverage,
            TravelMode::MetroTram => ServiceRoleClassification::UrbanTrunk,
            TravelMode::SuburbanRail => ServiceRoleClassification::CommuterRadial,
            TravelMode::RegionalRail | TravelMode::HighSpeedRail => {
                ServiceRoleClassification::Intercity
            }
            _ => ServiceRoleClassification::Mixed,
        };
        let role = service_metric_by_id
            .get(&svc.id)
            .map(|x| x.role_classification)
            .unwrap_or(default_role);

        service_aggs.insert(
            svc.id.clone(),
            ServiceAggregate {
                fare_revenue: 0.0,
                ridership_from_paths: 0.0,
                passenger_km_from_paths: 0.0,
                vehicle_km,
                vehicle_hours,
                operating_cost,
                rolling_stock_cost,
                infrastructure_cost: 0.0,
                reliability_cost_uplift,
                mode,
                role,
                line_id: svc.line_id.clone(),
            },
        );
    }

    for flow in assigned_od_flows {
        let corridor_entry = corridor_aggs
            .entry((
                flow.origin_zone_id.clone(),
                flow.destination_zone_id.clone(),
                flow.purpose,
            ))
            .or_default();
        corridor_entry.demand_served += flow.assigned_passengers.max(0.0);

        for path in &flow.chosen_paths {
            let assigned = path.assigned_passengers.max(0.0);
            if assigned <= 0.0 {
                continue;
            }

            let mut path_km = 0.0_f64;
            let mut service_distance = HashMap::<String, f64>::new();
            let mut mode_families = Vec::<TravelMode>::new();
            for lid in &path.link_ids {
                if let Some(link) = link_by_id.get(lid) {
                    path_km += (link.distance_m / 1000.0).max(0.0);
                }
                if let Some(svcs) = link_to_services.get(lid) {
                    for sid in svcs {
                        let dist = service_link_distance
                            .get(&(sid.clone(), lid.clone()))
                            .copied()
                            .unwrap_or(0.0);
                        *service_distance.entry(sid.clone()).or_insert(0.0) += dist.max(0.0);
                        if let Some(mode) = service_mode.get(sid) {
                            mode_families.push(*mode);
                        }
                    }
                }
            }
            if service_distance.is_empty() {
                continue;
            }
            let total_service_km = service_distance.values().sum::<f64>().max(1e-9);
            let fare_pp = fare_for_assigned_path(
                economy_cfg,
                fare_model,
                path_km.max(0.05),
                &mode_families,
                service_distance.len(),
            );
            let revenue = fare_pp.max(0.0) * assigned;

            corridor_entry.fare_revenue += revenue.max(0.0);
            corridor_entry.passenger_km += assigned * path_km.max(0.0);

            for (sid, dist_km) in service_distance {
                if let Some(agg) = service_aggs.get_mut(&sid) {
                    let share = (dist_km / total_service_km).clamp(0.0, 1.0);
                    agg.fare_revenue += revenue * share;
                    agg.ridership_from_paths += assigned * share;
                    agg.passenger_km_from_paths += assigned * path_km.max(0.0) * share;
                }
            }
        }
    }

    let mut total_infrastructure_cost = 0.0_f64;
    for link in &s.world.links {
        let mode = canonical_mode_from_tokens(
            &link.mode,
            link.mode_variant.as_deref(),
            (link.distance_m / 1000.0).max(0.0),
        )
        .travel_mode_family();
        let p = infrastructure_cost_profile_for_mode(economy_cfg, mode);
        let km = (link.distance_m / 1000.0).max(0.0);
        total_infrastructure_cost += km
            * (p.track_km_capex.max(0.0)
                * policy.capital_annualization_factor.max(0.0)
                * p.complexity_multiplier.max(0.5)
                + p.annualized_maintenance_cost_per_km.max(0.0)
                + p.infrastructure_renewal_cost_per_km.max(0.0));
    }
    for stop in &s.world.stops {
        let stop_type = stop
            .stop_type
            .as_deref()
            .unwrap_or("station")
            .to_ascii_lowercase();
        let canonical_mode = dominant_canonical_mode(stop_serving_modes.get(&stop.id))
            .unwrap_or_else(|| canonical_mode_from_tokens(&stop_type, None, 0.0));
        let mode = canonical_mode.travel_mode_family();
        let p = infrastructure_cost_profile_for_mode(economy_cfg, mode);
        let base_capex = if canonical_mode == CanonicalTransitMode::Bus {
            p.stop_capex.max(0.0)
        } else {
            p.station_capex.max(0.0)
        };
        total_infrastructure_cost += base_capex
            * policy.capital_annualization_factor.max(0.0)
            * p.complexity_multiplier.max(0.5);
    }

    let total_vehicle_km = service_aggs
        .values()
        .map(|x| x.vehicle_km.max(0.0))
        .sum::<f64>();
    let total_ridership = service_aggs
        .iter()
        .map(|(sid, agg)| {
            service_metric_by_id
                .get(sid)
                .map(|m| m.total_boardings.max(0.0))
                .unwrap_or(agg.ridership_from_paths.max(0.0))
        })
        .sum::<f64>()
        .max(1e-9);

    for (sid, agg) in &mut service_aggs {
        let vehicle_km_share = if total_vehicle_km > 0.0 {
            agg.vehicle_km / total_vehicle_km
        } else {
            0.0
        };
        let ridership = service_metric_by_id
            .get(sid)
            .map(|m| m.total_boardings.max(0.0))
            .unwrap_or(agg.ridership_from_paths.max(0.0));
        let ridership_share = (ridership / total_ridership).clamp(0.0, 1.0);
        let alloc_share = (policy.shared_infrastructure_allocation_weight.max(0.0)
            * vehicle_km_share
            + (1.0 - policy.shared_infrastructure_allocation_weight.max(0.0)) * ridership_share)
            .clamp(0.0, 1.0);
        agg.infrastructure_cost = total_infrastructure_cost * alloc_share;
    }

    let mut service_financial_metrics = Vec::<ServiceFinancialMetrics>::new();
    let mut service_fin_by_id = HashMap::<String, ServiceFinancialMetrics>::new();
    let total_operating_cost = service_aggs
        .values()
        .map(|x| x.operating_cost.max(0.0))
        .sum::<f64>();
    let total_rolling_stock_cost = service_aggs
        .values()
        .map(|x| x.rolling_stock_cost.max(0.0))
        .sum::<f64>();
    let total_passenger_km = service_aggs
        .iter()
        .map(|(sid, agg)| {
            service_metric_by_id
                .get(sid)
                .map(|m| m.passenger_km.max(0.0))
                .unwrap_or(agg.passenger_km_from_paths.max(0.0))
        })
        .sum::<f64>()
        .max(1e-9);

    for (sid, agg) in &service_aggs {
        let line_metric = service_metric_by_id.get(sid);
        let ridership = line_metric
            .map(|m| m.total_boardings.max(0.0))
            .unwrap_or(agg.ridership_from_paths.max(0.0));
        let passenger_km = line_metric
            .map(|m| m.passenger_km.max(0.0))
            .unwrap_or(agg.passenger_km_from_paths.max(0.0));
        let social_value_proxy = line_metric
            .map(|m| {
                (m.transit_captured_demand + 0.8 * m.uncaptured_competing_demand_near_service)
                    / (m.total_boardings + 1.0)
            })
            .unwrap_or(0.0)
            + if matches!(
                agg.role,
                ServiceRoleClassification::LocalCoverage
                    | ServiceRoleClassification::RegionalConnector
            ) {
                0.35
            } else {
                0.0
            };
        let metrics = compute_financial_performance(
            agg.fare_revenue,
            agg.operating_cost,
            agg.infrastructure_cost,
            agg.rolling_stock_cost,
            ridership,
            passenger_km,
            social_value_proxy,
        );
        let commercial = classify_commercial_strength(
            metrics.farebox_recovery_ratio,
            metrics.full_cost_surplus_deficit,
            policy,
        );
        let social = classify_social_necessity_for_service(agg.role, agg.mode, ridership, policy);
        let row = ServiceFinancialMetrics {
            service_id: sid.clone(),
            line_id: agg.line_id.clone(),
            service_mode_family: agg.mode,
            ridership: ridership.max(0.0),
            passenger_km: passenger_km.max(0.0),
            vehicle_km: agg.vehicle_km.max(0.0),
            vehicle_hours: agg.vehicle_hours.max(0.0),
            metrics,
            reliability_cost_uplift: agg.reliability_cost_uplift.max(0.0),
            commercial_strength_classification: commercial,
            social_necessity_classification: social,
            active_temporal_slice: active_context.clone(),
        };
        service_fin_by_id.insert(sid.clone(), row.clone());
        service_financial_metrics.push(row);
    }
    service_financial_metrics.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let mut corridor_financial_metrics = Vec::<CorridorFinancialMetrics>::new();
    let total_network_full_cost = service_financial_metrics
        .iter()
        .map(|x| x.metrics.total_cost.max(0.0))
        .sum::<f64>();
    for corridor in corridor_planning_metrics {
        let key = (
            corridor.origin_zone_id.clone(),
            corridor.destination_zone_id.clone(),
            corridor.dominant_purpose,
        );
        let agg = corridor_aggs.get(&key).cloned().unwrap_or_default();
        let demand_served = agg.demand_served.max(corridor.realised_volume.max(0.0));
        let passenger_km = if agg.passenger_km > 0.0 {
            agg.passenger_km.max(0.0)
        } else {
            let dist_km = if let (Some(oz), Some(dz)) = (
                s.world
                    .zones
                    .iter()
                    .find(|z| z.id == corridor.origin_zone_id),
                s.world
                    .zones
                    .iter()
                    .find(|z| z.id == corridor.destination_zone_id),
            ) {
                euclid_km((oz.x, oz.y), (dz.x, dz.y)).max(0.01)
            } else {
                0.0
            };
            demand_served.max(0.0) * dist_km
        };
        let cost_share = if total_passenger_km > 0.0 {
            (passenger_km / total_passenger_km).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let operating_cost = total_operating_cost * cost_share;
        let infra_cost = total_infrastructure_cost * cost_share;
        let stock_cost = total_rolling_stock_cost * cost_share;
        let social = classify_social_necessity_for_corridor(corridor, policy);
        let social_bonus = match social {
            SocialNecessityClassification::Core => 1.0,
            SocialNecessityClassification::Important => 0.75,
            SocialNecessityClassification::Supportive => 0.45,
            SocialNecessityClassification::Low => 0.20,
        };
        let metrics = compute_financial_performance(
            agg.fare_revenue.max(0.0),
            operating_cost.max(0.0),
            infra_cost.max(0.0),
            stock_cost.max(0.0),
            demand_served.max(0.0),
            passenger_km.max(0.0),
            social_bonus * (corridor.unserved_volume + corridor.latent_volume * 0.15)
                / (demand_served + 1.0),
        );
        let commercial = classify_commercial_strength(
            metrics.farebox_recovery_ratio,
            metrics.full_cost_surplus_deficit,
            policy,
        );
        corridor_financial_metrics.push(CorridorFinancialMetrics {
            origin_zone_id: corridor.origin_zone_id.clone(),
            destination_zone_id: corridor.destination_zone_id.clone(),
            purpose: corridor.dominant_purpose,
            demand_served: demand_served.max(0.0),
            passenger_km: passenger_km.max(0.0),
            metrics,
            commercial_strength_classification: commercial,
            social_necessity_classification: social,
            active_temporal_slice: active_context.clone(),
        });
    }
    corridor_financial_metrics.sort_by(|a, b| {
        b.metrics
            .fare_revenue
            .partial_cmp(&a.metrics.fare_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let revenue_total = service_financial_metrics
        .iter()
        .map(|x| x.metrics.fare_revenue.max(0.0))
        .sum::<f64>();
    let total_boardings = station_planning_metrics
        .iter()
        .map(|x| x.boardings_total.max(0.0))
        .sum::<f64>()
        .max(1e-9);
    let avg_revenue_per_boarding = if total_boardings > 0.0 {
        revenue_total / total_boardings
    } else {
        0.0
    };

    let mut services_by_stop = HashMap::<String, Vec<String>>::new();
    for svc in &s.world.services {
        for stop_id in &svc.stop_sequence {
            services_by_stop
                .entry(stop_id.clone())
                .or_default()
                .push(svc.id.clone());
        }
    }

    let mut station_financial_context = Vec::<StationFinancialContext>::new();
    for station in station_planning_metrics {
        let stop_services = services_by_stop
            .get(&station.stop_id)
            .cloned()
            .unwrap_or_default();
        let mut op_cost_proxy = 0.0_f64;
        let mut cap_cost_proxy = 0.0_f64;
        for sid in &stop_services {
            if let Some(fin) = service_fin_by_id.get(sid) {
                let stop_count = service_stop_count.get(sid).copied().unwrap_or(1).max(1) as f64;
                let board_share = (station.boardings_total / (fin.ridership + 1.0)).clamp(0.0, 1.0);
                op_cost_proxy +=
                    fin.metrics.operating_cost / stop_count * (0.35 + 0.65 * board_share);
                cap_cost_proxy += (fin.metrics.infrastructure_cost_allocated
                    + fin.metrics.rolling_stock_cost_allocated)
                    / stop_count
                    * (0.35 + 0.65 * board_share);
            }
        }
        let associated_revenue =
            station.boardings_total.max(0.0) * avg_revenue_per_boarding.max(0.0);
        let strategic_value_proxy =
            ((station.catchment_population + station.catchment_jobs) / 10_000.0).min(2.5)
                + (1.0 - station.transfer_success_rate).max(0.0) * 0.35
                + station.transit_capture_share_in_catchment.max(0.0);
        let farebox_proxy = if op_cost_proxy > 0.0 {
            associated_revenue / op_cost_proxy
        } else {
            0.0
        };
        let commercial = classify_commercial_strength(
            farebox_proxy,
            associated_revenue - (op_cost_proxy + cap_cost_proxy),
            policy,
        );
        let social = if station.catchment_population > 25_000.0
            || station.catchment_jobs > 25_000.0
            || station.transit_capture_share_in_catchment < 0.30
        {
            SocialNecessityClassification::Important
        } else {
            SocialNecessityClassification::Supportive
        };
        station_financial_context.push(StationFinancialContext {
            stop_id: station.stop_id.clone(),
            boardings: station.boardings_total.max(0.0),
            alightings: station.alightings_total.max(0.0),
            associated_revenue: associated_revenue.max(0.0),
            operating_cost_burden_proxy: op_cost_proxy.max(0.0),
            capital_cost_burden_proxy: cap_cost_proxy.max(0.0),
            strategic_value_proxy: strategic_value_proxy.max(0.0),
            commercial_strength_classification: commercial,
            social_necessity_classification: social,
            active_temporal_slice: active_context.clone(),
        });
    }
    station_financial_context.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let network_metrics = compute_financial_performance(
        revenue_total,
        total_operating_cost,
        total_infrastructure_cost,
        total_rolling_stock_cost,
        total_ridership,
        total_passenger_km,
        corridor_financial_metrics
            .iter()
            .map(|x| x.metrics.social_value_proxy.max(0.0))
            .sum::<f64>()
            / (corridor_financial_metrics.len() as f64 + 1.0),
    );
    let network_financial_summary = NetworkFinancialSummary {
        metrics: network_metrics,
        total_realised_transit_trips: total_ridership.max(0.0),
        total_passenger_km: total_passenger_km.max(0.0),
        total_vehicle_km: total_vehicle_km.max(0.0),
        total_vehicle_hours: service_aggs
            .values()
            .map(|x| x.vehicle_hours.max(0.0))
            .sum::<f64>(),
        total_infrastructure_annualized_cost: total_infrastructure_cost.max(0.0),
        total_rolling_stock_annualized_cost: total_rolling_stock_cost.max(0.0),
        active_temporal_slice: active_context.clone(),
    };

    let mut top_profitable_services = service_financial_metrics
        .iter()
        .filter(|x| x.metrics.full_cost_surplus_deficit > 0.0)
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.metrics.full_cost_surplus_deficit.max(0.0),
            reason: format!(
                "surplus {:.1}, farebox {:.2}",
                x.metrics.full_cost_surplus_deficit, x.metrics.farebox_recovery_ratio
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_profitable_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_profitable_services.truncate(12);

    let mut top_loss_making_high_ridership_services = service_financial_metrics
        .iter()
        .filter(|x| x.metrics.full_cost_surplus_deficit < 0.0)
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.ridership.max(0.0),
            reason: format!(
                "deficit {:.1} with ridership {:.1}",
                -x.metrics.full_cost_surplus_deficit, x.ridership
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_loss_making_high_ridership_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_loss_making_high_ridership_services.truncate(12);

    let mut top_subsidy_dependent_social_corridors = corridor_financial_metrics
        .iter()
        .filter(|x| {
            x.metrics.subsidy_required > 0.0
                && matches!(
                    x.social_necessity_classification,
                    SocialNecessityClassification::Core | SocialNecessityClassification::Important
                )
        })
        .map(|x| EconomicRankingEntry {
            id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
            score: x.metrics.subsidy_required.max(0.0),
            reason: format!(
                "subsidy {:.1}, social {:?}",
                x.metrics.subsidy_required, x.social_necessity_classification
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_subsidy_dependent_social_corridors.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_subsidy_dependent_social_corridors.truncate(12);

    let mut top_expensive_underperforming_services = service_financial_metrics
        .iter()
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.metrics.total_cost.max(0.0) / (x.ridership + 1.0),
            reason: format!(
                "cost {:.1}, ridership {:.1}",
                x.metrics.total_cost, x.ridership
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_expensive_underperforming_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_expensive_underperforming_services.truncate(12);

    let mut top_reinvestment_worthy_corridors = corridor_financial_metrics
        .iter()
        .map(|x| {
            let corridor = corridor_metric_by_pair
                .get(&(x.origin_zone_id.clone(), x.destination_zone_id.clone()))
                .copied();
            let unserved = corridor.map(|c| c.unserved_volume).unwrap_or(0.0).max(0.0);
            EconomicRankingEntry {
                id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
                score: (x.metrics.fare_revenue + 0.6 * unserved).max(0.0),
                reason: format!(
                    "revenue {:.1}, unserved {:.1}",
                    x.metrics.fare_revenue, unserved
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    top_reinvestment_worthy_corridors.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_reinvestment_worthy_corridors.truncate(12);

    let mut top_socially_valuable_commercially_weak_links = corridor_financial_metrics
        .iter()
        .filter(|x| {
            x.commercial_strength_classification == CommercialStrengthClassification::Weak
                && matches!(
                    x.social_necessity_classification,
                    SocialNecessityClassification::Core | SocialNecessityClassification::Important
                )
        })
        .map(|x| EconomicRankingEntry {
            id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
            score: x.metrics.social_value_proxy.max(0.0),
            reason: format!(
                "social {:.2}, deficit {:.1}",
                x.metrics.social_value_proxy, -x.metrics.full_cost_surplus_deficit
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_socially_valuable_commercially_weak_links.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_socially_valuable_commercially_weak_links.truncate(12);

    let mut top_revenue_generating_corridors = corridor_financial_metrics
        .iter()
        .map(|x| EconomicRankingEntry {
            id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
            score: x.metrics.fare_revenue.max(0.0),
            reason: format!("revenue {:.1}", x.metrics.fare_revenue),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_revenue_generating_corridors.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_revenue_generating_corridors.truncate(12);

    let mut top_operating_cost_heavy_services = service_financial_metrics
        .iter()
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.metrics.operating_cost.max(0.0),
            reason: format!("operating cost {:.1}", x.metrics.operating_cost),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_operating_cost_heavy_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_operating_cost_heavy_services.truncate(12);

    let mut best_farebox_recovery_services = service_financial_metrics
        .iter()
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.metrics.farebox_recovery_ratio.max(0.0),
            reason: format!(
                "farebox {:.2}, surplus {:.1}",
                x.metrics.farebox_recovery_ratio, x.metrics.operating_surplus_deficit
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    best_farebox_recovery_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    best_farebox_recovery_services.truncate(12);

    let mut worst_full_cost_deficits_services = service_financial_metrics
        .iter()
        .map(|x| EconomicRankingEntry {
            id: x.service_id.clone(),
            score: x.metrics.subsidy_required.max(0.0),
            reason: format!(
                "deficit {:.1}, farebox {:.2}",
                -x.metrics.full_cost_surplus_deficit, x.metrics.farebox_recovery_ratio
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    worst_full_cost_deficits_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    worst_full_cost_deficits_services.truncate(12);

    let mut corridors_where_unreliability_hurts_finances = corridor_financial_metrics
        .iter()
        .map(|x| {
            let pressure = corridor_metric_by_pair
                .get(&(x.origin_zone_id.clone(), x.destination_zone_id.clone()))
                .map(|c| c.crowding_delay_pressure + c.missed_transfer_sensitivity)
                .unwrap_or(0.0);
            EconomicRankingEntry {
                id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
                score: pressure.max(0.0) * (x.metrics.subsidy_required + 1.0),
                reason: format!(
                    "pressure {:.2}, subsidy {:.1}",
                    pressure, x.metrics.subsidy_required
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    corridors_where_unreliability_hurts_finances.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    corridors_where_unreliability_hurts_finances.truncate(12);

    let mut overloaded_highly_profitable_services = service_financial_metrics
        .iter()
        .filter(|x| x.metrics.full_cost_surplus_deficit > 0.0)
        .map(|x| {
            let overload = service_metric_by_id
                .get(&x.service_id)
                .map(|m| m.utilisation_score + m.overcrowded_segments as f64 * 0.2)
                .unwrap_or(0.0);
            EconomicRankingEntry {
                id: x.service_id.clone(),
                score: overload.max(0.0) * (x.metrics.full_cost_surplus_deficit + 1.0),
                reason: format!(
                    "overload {:.2}, surplus {:.1}",
                    overload, x.metrics.full_cost_surplus_deficit
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    overloaded_highly_profitable_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overloaded_highly_profitable_services.truncate(12);

    let mut strongest_commercial_opportunities = corridor_financial_metrics
        .iter()
        .map(|x| {
            let capture_gap = corridor_metric_by_pair
                .get(&(x.origin_zone_id.clone(), x.destination_zone_id.clone()))
                .map(|c| c.transit_capture_gap)
                .unwrap_or(0.0);
            EconomicRankingEntry {
                id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
                score: capture_gap.max(0.0) * (x.metrics.revenue_per_passenger + 1.0),
                reason: format!(
                    "capture_gap {:.1}, rev/pax {:.2}",
                    capture_gap, x.metrics.revenue_per_passenger
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    strongest_commercial_opportunities.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_commercial_opportunities.truncate(12);

    let mut strongest_social_necessity_corridors = corridor_financial_metrics
        .iter()
        .map(|x| {
            let zone_o = zone_metric_by_id.get(&x.origin_zone_id);
            let zone_d = zone_metric_by_id.get(&x.destination_zone_id);
            let rural_factor = zone_o
                .map(|z| {
                    matches!(
                        z.settlement_class,
                        SettlementClass::Village | SettlementClass::Rural
                    ) as i32 as f64
                })
                .unwrap_or(0.0)
                + zone_d
                    .map(|z| {
                        matches!(
                            z.settlement_class,
                            SettlementClass::Village | SettlementClass::Rural
                        ) as i32 as f64
                    })
                    .unwrap_or(0.0);
            let essential = if x.purpose == TripPurpose::Essential {
                1.0
            } else {
                0.0
            };
            EconomicRankingEntry {
                id: format!("{}->{}", x.origin_zone_id, x.destination_zone_id),
                score: x.metrics.subsidy_required.max(0.0) * (1.0 + essential + 0.4 * rural_factor),
                reason: format!(
                    "subsidy {:.1}, social {:?}",
                    x.metrics.subsidy_required, x.social_necessity_classification
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    strongest_social_necessity_corridors.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_social_necessity_corridors.truncate(12);

    out.network_financial_summary = network_financial_summary;
    out.service_financial_metrics = service_financial_metrics;
    out.corridor_financial_metrics = corridor_financial_metrics;
    out.station_financial_context = station_financial_context;
    out.economic_diagnostics = EconomicDiagnostics {
        top_profitable_services,
        top_loss_making_high_ridership_services,
        top_subsidy_dependent_social_corridors,
        top_expensive_underperforming_services,
        top_reinvestment_worthy_corridors,
        top_socially_valuable_commercially_weak_links,
        top_revenue_generating_corridors,
        top_operating_cost_heavy_services,
        best_farebox_recovery_services,
        worst_full_cost_deficits_services,
        corridors_where_unreliability_hurts_finances,
        overloaded_highly_profitable_services,
        strongest_commercial_opportunities,
        strongest_social_necessity_corridors,
        network_financial_by_time_slice: Vec::new(),
        network_financial_by_day_type: Vec::new(),
    };

    let _ = total_network_full_cost;
    out
}

pub(super) fn apply_economics_to_planning(
    phase3: &mut Phase3PlanningOutputs,
    economics: &EconomicsOutputs,
    zone_profiles: &[ZoneDemandProfile],
    economy_cfg: &SyntheticEconomyConfig,
) {
    let policy = &economy_cfg.economics_policy_config;

    let service_fin_by_id = economics
        .service_financial_metrics
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let corridor_fin_by_key = economics
        .corridor_financial_metrics
        .iter()
        .map(|x| ((x.origin_zone_id.clone(), x.destination_zone_id.clone()), x))
        .collect::<HashMap<_, _>>();
    let station_fin_by_stop = economics
        .station_financial_context
        .iter()
        .map(|x| (x.stop_id.clone(), x))
        .collect::<HashMap<_, _>>();

    for z in &mut phase3.zone_planning_metrics {
        let mut transit_revenue_generated = 0.0_f64;
        let mut subsidy_proxy = 0.0_f64;
        for c in &phase3.corridor_planning_metrics {
            if c.origin_zone_id == z.zone_id {
                if let Some(fin) = corridor_fin_by_key
                    .get(&(c.origin_zone_id.clone(), c.destination_zone_id.clone()))
                {
                    transit_revenue_generated += fin.metrics.fare_revenue.max(0.0);
                    subsidy_proxy += fin.metrics.subsidy_required.max(0.0);
                }
            }
        }
        z.transit_revenue_generated = transit_revenue_generated.max(0.0);
        z.subsidy_need_proxy = subsidy_proxy.max(0.0);
    }

    for st in &mut phase3.station_planning_metrics {
        if let Some(fin) = station_fin_by_stop.get(&st.stop_id) {
            st.associated_revenue = fin.associated_revenue.max(0.0);
            st.operating_cost_burden_proxy = fin.operating_cost_burden_proxy.max(0.0);
            st.capital_cost_burden_proxy = fin.capital_cost_burden_proxy.max(0.0);
            st.strategic_value_proxy = fin.strategic_value_proxy.max(0.0);
            st.commercial_strength_classification = fin.commercial_strength_classification;
            st.social_necessity_classification = fin.social_necessity_classification;
        }
    }

    for c in &mut phase3.corridor_planning_metrics {
        if let Some(fin) =
            corridor_fin_by_key.get(&(c.origin_zone_id.clone(), c.destination_zone_id.clone()))
        {
            c.fare_revenue = fin.metrics.fare_revenue.max(0.0);
            c.operating_cost_allocated = fin.metrics.operating_cost.max(0.0);
            c.total_cost_allocated = fin.metrics.total_cost.max(0.0);
            c.subsidy_required = fin.metrics.subsidy_required.max(0.0);
            c.farebox_recovery_ratio = fin.metrics.farebox_recovery_ratio.max(0.0);
            c.commercial_strength_classification = fin.commercial_strength_classification;
            c.social_necessity_classification = fin.social_necessity_classification;
        }
    }

    for svc in &mut phase3.line_service_planning_metrics {
        if let Some(fin) = service_fin_by_id.get(&svc.service_id) {
            svc.fare_revenue = fin.metrics.fare_revenue.max(0.0);
            svc.operating_cost = fin.metrics.operating_cost.max(0.0);
            svc.infrastructure_cost_allocated = fin.metrics.infrastructure_cost_allocated.max(0.0);
            svc.rolling_stock_cost_allocated = fin.metrics.rolling_stock_cost_allocated.max(0.0);
            svc.total_cost = fin.metrics.total_cost.max(0.0);
            svc.operating_surplus_deficit = fin.metrics.operating_surplus_deficit;
            svc.full_cost_surplus_deficit = fin.metrics.full_cost_surplus_deficit;
            svc.subsidy_required = fin.metrics.subsidy_required.max(0.0);
            svc.farebox_recovery_ratio = fin.metrics.farebox_recovery_ratio.max(0.0);
            svc.cost_per_passenger = fin.metrics.cost_per_passenger.max(0.0);
            svc.cost_per_passenger_km = fin.metrics.cost_per_passenger_km.max(0.0);
            svc.revenue_per_passenger = fin.metrics.revenue_per_passenger.max(0.0);
            svc.commercial_strength_classification = fin.commercial_strength_classification;
            svc.social_necessity_classification = fin.social_necessity_classification;
            svc.reliability_cost_pressure = fin.reliability_cost_uplift.max(0.0);
        }
    }

    let avg_revenue_per_latent = if !phase3.zone_planning_metrics.is_empty() {
        let total_rev = phase3
            .zone_planning_metrics
            .iter()
            .map(|x| x.transit_revenue_generated.max(0.0))
            .sum::<f64>();
        let total_latent = phase3
            .zone_planning_metrics
            .iter()
            .map(|x| x.total_latent_produced.max(0.0))
            .sum::<f64>();
        if total_latent > 0.0 {
            total_rev / total_latent
        } else {
            0.0
        }
    } else {
        0.0
    };

    for preview in &mut phase3.build_preview_metrics {
        let coverage_factor = (preview.estimated_new_coverage_population
            + 0.6 * preview.estimated_new_coverage_jobs)
            .max(0.0)
            / 100_000.0;
        let latent_intercept = preview.latent_demand_interceptable.max(0.0);
        let unserved_addressable = preview.unserved_demand_addressable.max(0.0);
        preview.estimated_revenue_uplift =
            (latent_intercept * (avg_revenue_per_latent + 1.2) * (0.55 + 0.25 * coverage_factor))
                .max(0.0);

        let mode_hint = preview
            .strongest_corridors_touched
            .iter()
            .filter_map(|c| {
                phase3
                    .corridor_planning_metrics
                    .iter()
                    .find(|x| {
                        x.origin_zone_id == c.origin_zone_id
                            && x.destination_zone_id == c.destination_zone_id
                    })
                    .map(|x| x.strongest_transit_submode)
            })
            .next()
            .unwrap_or(TravelMode::Bus);
        let infra_profile = infrastructure_cost_profile_for_mode(economy_cfg, mode_hint);
        let operating_profile = service_cost_profile_for_mode(economy_cfg, mode_hint);

        preview.estimated_operating_cost_uplift = match preview.preview_type {
            BuildPreviewType::Station => {
                operating_profile.fixed_cost_per_period.max(0.0) * (0.20 + 0.45 * coverage_factor)
                    + operating_profile.station_stop_call_cost.max(0.0) * 120.0
            }
            BuildPreviewType::LineSegment => {
                let corridor_dist_km = preview
                    .strongest_corridors_touched
                    .iter()
                    .map(|c| {
                        phase3
                            .corridor_planning_metrics
                            .iter()
                            .find(|x| {
                                x.origin_zone_id == c.origin_zone_id
                                    && x.destination_zone_id == c.destination_zone_id
                            })
                            .map(|x| x.realised_volume.max(0.0))
                            .unwrap_or(0.0)
                    })
                    .sum::<f64>()
                    / 120.0;
                operating_profile.vehicle_km_cost.max(0.0) * corridor_dist_km.max(1.0) * 80.0
                    + operating_profile.fixed_cost_per_period.max(0.0)
            }
            BuildPreviewType::ServiceFrequencyIncrease => {
                operating_profile.fixed_cost_per_period.max(0.0) * 0.65
                    + operating_profile.vehicle_hour_cost.max(0.0) * 42.0
                    + operating_profile.vehicle_km_cost.max(0.0) * 210.0
            }
        };

        preview.estimated_capital_cost = match preview.preview_type {
            BuildPreviewType::Station => {
                infra_profile.station_capex.max(0.0)
                    * policy.capital_annualization_factor.max(0.0)
                    * (0.8 + 0.6 * coverage_factor)
            }
            BuildPreviewType::LineSegment => {
                infra_profile.track_km_capex.max(0.0)
                    * policy.capital_annualization_factor.max(0.0)
                    * (1.2 + preview.strongest_corridors_touched.len() as f64 * 0.25)
            }
            BuildPreviewType::ServiceFrequencyIncrease => {
                let rolling = rolling_stock_cost_profile_for_mode(economy_cfg, mode_hint);
                rolling.annualized_capital_cost_per_vehicle.max(0.0)
                    * policy.capital_annualization_factor.max(0.0)
                    * 1.5
            }
        };

        let full_cost = preview.estimated_operating_cost_uplift + preview.estimated_capital_cost;
        preview.estimated_farebox_recovery = if preview.estimated_operating_cost_uplift > 0.0 {
            (preview.estimated_revenue_uplift / preview.estimated_operating_cost_uplift).max(0.0)
        } else {
            0.0
        };
        preview.likely_subsidy_requirement =
            (full_cost - preview.estimated_revenue_uplift).max(0.0);

        let commercial = classify_commercial_strength(
            preview.estimated_farebox_recovery,
            preview.estimated_revenue_uplift - full_cost,
            policy,
        );
        preview.commercial_strength_classification = commercial;

        let social = if preview
            .strongest_trip_purposes_unlocked
            .iter()
            .any(|p| p.purpose == TripPurpose::Essential)
            || preview
                .affected_zones
                .iter()
                .filter_map(|zid| zone_profiles.iter().find(|z| z.zone_id == *zid))
                .any(|z| {
                    matches!(
                        z.settlement_class,
                        SettlementClass::Village | SettlementClass::Rural
                    )
                }) {
            SocialNecessityClassification::Important
        } else {
            SocialNecessityClassification::Supportive
        };
        preview.social_necessity_classification = social;

        let social_boost = match social {
            SocialNecessityClassification::Core => 0.45,
            SocialNecessityClassification::Important => 0.30,
            SocialNecessityClassification::Supportive => 0.15,
            SocialNecessityClassification::Low => 0.0,
        };
        preview.reinvestment_case_score = (preview.estimated_revenue_uplift
            + unserved_addressable * 0.9
            + social_boost * (latent_intercept + 1.0) * 10.0)
            / (full_cost + 1.0);
    }

    phase3.build_preview_metrics.sort_by(|a, b| {
        b.reinvestment_case_score
            .partial_cmp(&a.reinvestment_case_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut top_profitable_services = economics
        .service_financial_metrics
        .iter()
        .filter(|x| x.metrics.full_cost_surplus_deficit > 0.0)
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.metrics.full_cost_surplus_deficit.max(0.0),
            reason: format!("farebox {:.2}", x.metrics.farebox_recovery_ratio),
        })
        .collect::<Vec<_>>();
    top_profitable_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_profitable_services.truncate(12);

    let mut top_loss_making_high_ridership_services = economics
        .service_financial_metrics
        .iter()
        .filter(|x| x.metrics.full_cost_surplus_deficit < 0.0)
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.ridership.max(0.0),
            reason: format!("deficit {:.1}", -x.metrics.full_cost_surplus_deficit),
        })
        .collect::<Vec<_>>();
    top_loss_making_high_ridership_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_loss_making_high_ridership_services.truncate(12);

    let mut top_expensive_underperforming_services = economics
        .service_financial_metrics
        .iter()
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.metrics.total_cost.max(0.0) / (x.ridership + 1.0),
            reason: format!(
                "cost {:.1}, ridership {:.1}",
                x.metrics.total_cost, x.ridership
            ),
        })
        .collect::<Vec<_>>();
    top_expensive_underperforming_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_expensive_underperforming_services.truncate(12);

    let mut top_subsidy_dependent_social_corridors = phase3
        .corridor_planning_metrics
        .iter()
        .filter(|c| {
            c.subsidy_required > 0.0
                && matches!(
                    c.social_necessity_classification,
                    SocialNecessityClassification::Core | SocialNecessityClassification::Important
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    top_subsidy_dependent_social_corridors.sort_by(|a, b| {
        b.subsidy_required
            .partial_cmp(&a.subsidy_required)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_subsidy_dependent_social_corridors.truncate(12);

    let mut top_reinvestment_worthy_corridors = phase3.corridor_planning_metrics.to_vec();
    top_reinvestment_worthy_corridors.sort_by(|a, b| {
        (b.fare_revenue + 0.7 * b.unserved_volume)
            .partial_cmp(&(a.fare_revenue + 0.7 * a.unserved_volume))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_reinvestment_worthy_corridors.truncate(12);

    let mut top_socially_valuable_commercially_weak_links = phase3
        .corridor_planning_metrics
        .iter()
        .filter(|c| {
            c.commercial_strength_classification == CommercialStrengthClassification::Weak
                && matches!(
                    c.social_necessity_classification,
                    SocialNecessityClassification::Core | SocialNecessityClassification::Important
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    top_socially_valuable_commercially_weak_links.sort_by(|a, b| {
        b.subsidy_required
            .partial_cmp(&a.subsidy_required)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_socially_valuable_commercially_weak_links.truncate(12);

    phase3.service_gap_rankings.top_profitable_services = top_profitable_services.clone();
    phase3
        .service_gap_rankings
        .top_loss_making_high_ridership_services = top_loss_making_high_ridership_services.clone();
    phase3
        .service_gap_rankings
        .top_subsidy_dependent_social_corridors = top_subsidy_dependent_social_corridors.clone();
    phase3
        .service_gap_rankings
        .top_expensive_underperforming_services = top_expensive_underperforming_services.clone();
    phase3
        .service_gap_rankings
        .top_reinvestment_worthy_corridors = top_reinvestment_worthy_corridors.clone();
    phase3
        .service_gap_rankings
        .top_socially_valuable_commercially_weak_links =
        top_socially_valuable_commercially_weak_links.clone();

    let mut top_revenue_corridors = phase3.corridor_planning_metrics.clone();
    top_revenue_corridors.sort_by(|a, b| {
        b.fare_revenue
            .partial_cmp(&a.fare_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_revenue_corridors.truncate(8);

    let mut top_operating_cost_heavy_services = phase3
        .line_service_planning_metrics
        .iter()
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.operating_cost.max(0.0),
            reason: format!("operating {:.1}", x.operating_cost),
        })
        .collect::<Vec<_>>();
    top_operating_cost_heavy_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_operating_cost_heavy_services.truncate(8);

    let mut best_farebox_recovery_services = phase3
        .line_service_planning_metrics
        .iter()
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.farebox_recovery_ratio.max(0.0),
            reason: format!("farebox {:.2}", x.farebox_recovery_ratio),
        })
        .collect::<Vec<_>>();
    best_farebox_recovery_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    best_farebox_recovery_services.truncate(8);

    let mut worst_full_cost_deficit_services = phase3
        .line_service_planning_metrics
        .iter()
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: x.subsidy_required.max(0.0),
            reason: format!("subsidy {:.1}", x.subsidy_required),
        })
        .collect::<Vec<_>>();
    worst_full_cost_deficit_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    worst_full_cost_deficit_services.truncate(8);

    let mut strongest_commercial_opportunities = phase3.corridor_planning_metrics.clone();
    strongest_commercial_opportunities.sort_by(|a, b| {
        (b.transit_capture_gap * (b.fare_revenue + 1.0))
            .partial_cmp(&(a.transit_capture_gap * (a.fare_revenue + 1.0)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_commercial_opportunities.truncate(8);

    let mut strongest_social_necessity_corridors = phase3
        .corridor_planning_metrics
        .iter()
        .filter(|c| {
            matches!(
                c.social_necessity_classification,
                SocialNecessityClassification::Core | SocialNecessityClassification::Important
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    strongest_social_necessity_corridors.sort_by(|a, b| {
        b.subsidy_required
            .partial_cmp(&a.subsidy_required)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_social_necessity_corridors.truncate(8);

    let mut corridors_where_unreliability_hurts_finances = phase3.corridor_planning_metrics.clone();
    corridors_where_unreliability_hurts_finances.sort_by(|a, b| {
        (b.crowding_delay_pressure + b.missed_transfer_sensitivity)
            .partial_cmp(&(a.crowding_delay_pressure + a.missed_transfer_sensitivity))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    corridors_where_unreliability_hurts_finances.truncate(8);

    let mut overloaded_highly_profitable_services = phase3
        .line_service_planning_metrics
        .iter()
        .filter(|x| x.full_cost_surplus_deficit > 0.0)
        .map(|x| ServiceScoreEntry {
            service_id: x.service_id.clone(),
            score: (x.utilisation_score + x.overcrowded_segments as f64 * 0.2)
                * (x.full_cost_surplus_deficit + 1.0),
            reason: format!(
                "utilisation {:.2}, surplus {:.1}",
                x.utilisation_score, x.full_cost_surplus_deficit
            ),
        })
        .collect::<Vec<_>>();
    overloaded_highly_profitable_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overloaded_highly_profitable_services.truncate(8);

    phase3.planning_debug_summary.top_revenue_corridors = top_revenue_corridors;
    phase3
        .planning_debug_summary
        .top_operating_cost_heavy_services = top_operating_cost_heavy_services;
    phase3.planning_debug_summary.best_farebox_recovery_services = best_farebox_recovery_services;
    phase3
        .planning_debug_summary
        .worst_full_cost_deficit_services = worst_full_cost_deficit_services;
    phase3
        .planning_debug_summary
        .strongest_commercial_opportunities = strongest_commercial_opportunities;
    phase3
        .planning_debug_summary
        .strongest_social_necessity_corridors = strongest_social_necessity_corridors;
    phase3
        .planning_debug_summary
        .corridors_where_unreliability_hurts_finances =
        corridors_where_unreliability_hurts_finances;
    phase3
        .planning_debug_summary
        .overloaded_highly_profitable_services = overloaded_highly_profitable_services;
}
