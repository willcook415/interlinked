use super::*;

pub(super) fn apply_mode_choice_capture(
    s: &Scenario,
    settings: &SimulationSettings,
    base_graph: &super::graph::Graph,
    economy_cfg: &SyntheticEconomyConfig,
    zone_profiles: &[ZoneDemandProfile],
    active_context: &TemporalDemandSlice,
    active_latent: &[ActiveLatentOd],
) -> Result<ModeChoiceBuild, String> {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum CandidateMode {
        Transit,
        Car,
        Walk,
        NoTrip,
    }

    let mut out = ModeChoiceBuild::default();
    if active_latent.is_empty() || s.world.zones.is_empty() {
        return Ok(out);
    }

    let zone_count = s.world.zones.len();
    let mut nearest_stop_by_zone_idx = vec![None::<String>; zone_count];
    for (zi, zone) in s.world.zones.iter().enumerate() {
        let mut best: Option<(String, f64)> = None;
        for stop in &s.world.stops {
            let d = euclid_m((zone.x, zone.y), (stop.x, stop.y));
            match &best {
                Some((_, bd)) if d >= *bd => {}
                _ => best = Some((stop.id.clone(), d)),
            }
        }
        nearest_stop_by_zone_idx[zi] = best.map(|x| x.0);
    }

    let mut stop_capacity_pph = HashMap::<String, f64>::new();
    let mut stop_frequency_tph = HashMap::<String, f64>::new();
    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) {
            continue;
        }
        let tph = if let Some(x) = svc.operating_tph {
            if x.is_finite() && x > 0.0 {
                x
            } else if svc.headway_s > 0.0 {
                3600.0 / svc.headway_s
            } else {
                0.0
            }
        } else if svc.headway_s > 0.0 {
            3600.0 / svc.headway_s
        } else {
            0.0
        };
        let cap_pph = tph.max(0.0) * svc.vehicle_capacity.max(0.0);
        for stop_id in &svc.stop_sequence {
            *stop_capacity_pph.entry(stop_id.clone()).or_insert(0.0) += cap_pph.max(0.0);
            *stop_frequency_tph.entry(stop_id.clone()).or_insert(0.0) += tph.max(0.0);
        }
    }

    let mut zone_supply_pph = vec![0.0_f64; zone_count];
    let mut zone_frequency_tph = vec![0.0_f64; zone_count];
    for (zi, sid_opt) in nearest_stop_by_zone_idx.iter().enumerate() {
        if let Some(sid) = sid_opt {
            zone_supply_pph[zi] = stop_capacity_pph.get(sid).copied().unwrap_or(0.0).max(0.0);
            zone_frequency_tph[zi] = stop_frequency_tph.get(sid).copied().unwrap_or(0.0).max(0.0);
        }
    }

    let ops_cfg = &economy_cfg.operations_reliability_config;
    let active_services = s
        .world
        .services
        .iter()
        .filter(|svc| service_is_active_for_sim(svc))
        .collect::<Vec<_>>();
    let service_by_id = active_services
        .iter()
        .map(|svc| (svc.id.as_str(), *svc))
        .collect::<HashMap<_, _>>();

    let mut stop_service_count = HashMap::<String, usize>::new();
    for svc in &active_services {
        for stop_id in &svc.stop_sequence {
            *stop_service_count.entry(stop_id.clone()).or_insert(0) += 1;
        }
    }

    let mut latent_proxy_by_zone = vec![0.0_f64; zone_count];
    for od in active_latent {
        let latent = od.latent_passengers.max(0.0);
        latent_proxy_by_zone[od.origin_idx] += latent * 0.65;
        latent_proxy_by_zone[od.destination_idx] += latent * 0.35;
    }
    let mut stop_latent_proxy = HashMap::<String, f64>::new();
    for (zi, stop_opt) in nearest_stop_by_zone_idx.iter().enumerate() {
        if let Some(stop_id) = stop_opt {
            *stop_latent_proxy.entry(stop_id.clone()).or_insert(0.0) +=
                latent_proxy_by_zone[zi].max(0.0);
        }
    }

    let mut stop_operational_pressure = HashMap::<String, f64>::new();
    for stop in &s.world.stops {
        let stop_id = stop.id.clone();
        let latent_proxy = stop_latent_proxy
            .get(&stop_id)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let cap_pph = stop_capacity_pph
            .get(&stop_id)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let freq_tph = stop_frequency_tph
            .get(&stop_id)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let base_pressure = if cap_pph > 0.0 {
            latent_proxy / cap_pph.max(1.0)
        } else {
            latent_proxy / 30.0
        };
        let freq_penalty = if freq_tph > 0.0 {
            (1.0 / freq_tph).clamp(0.0, 1.2)
        } else {
            1.2
        };
        let stop_type = stop
            .stop_type
            .as_deref()
            .unwrap_or("station")
            .to_ascii_lowercase();
        let type_mult = if stop_type.contains("bus") {
            0.92
        } else if stop_type.contains("station") || stop_type.contains("platform") {
            1.08
        } else {
            1.0
        };
        let interchange_bonus = if stop_service_count.get(&stop_id).copied().unwrap_or(0) >= 3 {
            0.18
        } else {
            0.0
        };
        let pressure = (base_pressure * type_mult + freq_penalty + interchange_bonus).max(0.0);
        stop_operational_pressure.insert(stop_id, pressure);
    }

    let mut service_operational_pressure = HashMap::<String, f64>::new();
    for svc in &active_services {
        let mut avg_stop_pressure = 0.0_f64;
        if !svc.stop_sequence.is_empty() {
            avg_stop_pressure = svc
                .stop_sequence
                .iter()
                .map(|sid| stop_operational_pressure.get(sid).copied().unwrap_or(0.0))
                .sum::<f64>()
                / (svc.stop_sequence.len() as f64);
        }
        let tph = if svc.headway_s > 0.0 {
            3600.0 / svc.headway_s
        } else {
            0.0
        };
        let freq_pressure = if tph > 0.0 {
            (1.0 / tph).clamp(0.0, 1.1)
        } else {
            1.1
        };
        let dwell_pressure = if svc.dwell_s > 0.0 {
            (svc.dwell_s / ops_cfg.base_dwell_station_s.max(1.0)).max(0.0) * 0.12
        } else {
            0.0
        };
        service_operational_pressure.insert(
            svc.id.clone(),
            (avg_stop_pressure + freq_pressure + dwell_pressure).max(0.0),
        );
    }

    for od in active_latent {
        let latent = od.latent_passengers.max(0.0);
        if latent <= 0.0 {
            continue;
        }

        let origin_zone = &s.world.zones[od.origin_idx];
        let destination_zone = &s.world.zones[od.destination_idx];
        let origin_profile = &zone_profiles[od.origin_idx];
        let destination_profile = &zone_profiles[od.destination_idx];
        let trip_distance_km = euclid_km(
            (origin_zone.x, origin_zone.y),
            (destination_zone.x, destination_zone.y),
        )
        .max(0.02);

        let purpose_sens = purpose_mode_sensitivity(od.purpose, economy_cfg);
        let o_settlement = settlement_mode_constant(origin_profile.settlement_class, economy_cfg);
        let d_settlement =
            settlement_mode_constant(destination_profile.settlement_class, economy_cfg);
        let avg_settlement_transit =
            0.5 * (o_settlement.transit_constant + d_settlement.transit_constant);
        let avg_settlement_car = 0.5 * (o_settlement.car_constant + d_settlement.car_constant);
        let avg_settlement_walk = 0.5 * (o_settlement.walk_constant + d_settlement.walk_constant);
        let coeff = &economy_cfg.mode_utility_coefficients;
        let value_of_time_h = (s.params.fare_value_of_time_base_per_hour.max(4.0)
            * purpose_sens.value_of_time_weight.max(0.25))
        .max(2.0);

        let origin_node = base_graph.svc_index.zone_nodes_start + od.origin_idx;
        let dest_node = base_graph.svc_index.zone_nodes_start + zone_count + od.destination_idx;
        let mut transit_paths = apply_fare_to_paths(
            dedupe_paths(k_shortest_paths(
                base_graph,
                origin_node,
                dest_node,
                settings.k_paths,
            )),
            &s.params,
        );
        let transit_available = !transit_paths.is_empty();
        let transit_shares = if transit_available {
            logit_shares(&transit_paths, settings.route_choice_theta)
        } else {
            Vec::new()
        };

        let mut expected_walk_s = 0.0_f64;
        let mut expected_wait_s = 0.0_f64;
        let mut expected_ivt_s = 0.0_f64;
        let mut expected_transfer_penalty_s = 0.0_f64;
        let mut expected_transfer_count = 0.0_f64;
        let mut expected_fare_base = 0.0_f64;
        let mut expected_operational_pressure = 0.0_f64;
        let mut weighted_submode = HashMap::<TravelMode, f64>::new();

        if transit_available {
            let submode_pref = transit_submode_preference(od.purpose, economy_cfg);
            for (path, share) in transit_paths.iter_mut().zip(transit_shares.iter()) {
                let w = share.max(0.0);
                if w <= 0.0 {
                    continue;
                }
                expected_walk_s += path.stats.walk_s.max(0.0) * w;
                expected_wait_s += path.stats.wait_s.max(0.0) * w;
                expected_ivt_s += path.stats.ivt_s.max(0.0) * w;
                expected_transfer_penalty_s += path.stats.transfer_penalty_s.max(0.0) * w;
                expected_transfer_count += path.stats.transfer_count.max(0.0) * w;
                expected_fare_base += path.stats.fare_base.max(0.0) * w;

                let board_count = (path.board_events.len() as f64).max(1.0);
                let mut path_pressure = 0.0_f64;
                for (svc_id, stop_id) in &path.board_events {
                    path_pressure += service_operational_pressure
                        .get(svc_id)
                        .copied()
                        .unwrap_or(0.0)
                        .max(0.0);
                    path_pressure += 0.70
                        * stop_operational_pressure
                            .get(stop_id)
                            .copied()
                            .unwrap_or(0.0)
                            .max(0.0);
                    if let Some(svc) = service_by_id.get(svc_id.as_str()) {
                        if svc.headway_s > 0.0 {
                            path_pressure += ops_cfg.headway_irregularity_from_delay.max(0.0)
                                * ((1.0 / (3600.0 / svc.headway_s).max(0.2)).clamp(0.0, 1.2))
                                * 0.55;
                        }
                    }
                }
                path_pressure /= board_count;
                path_pressure += path.stats.transfer_count.max(0.0) * 0.24;
                expected_operational_pressure += path_pressure.max(0.0) * w;

                if path.board_modes.is_empty() {
                    *weighted_submode
                        .entry(TravelMode::OtherTransit)
                        .or_insert(0.0) += w;
                } else {
                    let board_count = (path.board_modes.len() as f64).max(1.0);
                    for board_mode in &path.board_modes {
                        let tm = travel_mode_family_from_tokens(board_mode, None, trip_distance_km);
                        *weighted_submode.entry(tm).or_insert(0.0) += w / board_count;
                    }
                }
            }
            for (mode, val) in weighted_submode.clone() {
                let pref = submode_preference_for(mode, &submode_pref).max(0.05);
                weighted_submode.insert(mode, val * pref);
            }
        }

        let mut transit_submode_split = {
            let sum = weighted_submode.values().sum::<f64>().max(0.0);
            if sum > 0.0 {
                weighted_submode
                    .iter()
                    .map(|(mode, value)| ModeShareValue {
                        mode: *mode,
                        share: (value / sum).clamp(0.0, 1.0),
                        passengers: 0.0,
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        transit_submode_split.sort_by(|a, b| {
            b.share
                .partial_cmp(&a.share)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let origin_supply = zone_supply_pph[od.origin_idx].max(1.0);
        let destination_supply = zone_supply_pph[od.destination_idx].max(1.0);
        let average_supply = (origin_supply + destination_supply) * 0.5;
        let period_h = s.meta.time_period_hours.max(0.25);
        let demand_pressure = latent / (average_supply * period_h + 1.0);
        let crowding_signal = (demand_pressure - 0.70).max(0.0);
        let transit_crowding_penalty_s = crowding_signal
            * 480.0
            * coeff.crowding_penalty_weight.max(0.0)
            * purpose_sens.crowding_aversion_multiplier.max(0.2);

        let average_freq_tph =
            (zone_frequency_tph[od.origin_idx] + zone_frequency_tph[od.destination_idx]) * 0.5;
        let low_frequency_penalty_s = if average_freq_tph > 0.0 {
            (1.0 / average_freq_tph).clamp(0.0, 1.0) * 340.0
        } else {
            420.0
        };
        let operational_reliability_penalty_s =
            (ops_cfg.reliability_penalty_coefficient_s.max(0.0)
                * expected_operational_pressure.max(0.0)
                * 0.18)
                .clamp(0.0, 1800.0);
        let irregularity_wait_penalty_s = ops_cfg.irregularity_wait_penalty_weight.max(0.0)
            * expected_operational_pressure.max(0.0)
            * expected_wait_s.min(1800.0)
            * 0.02;
        let transit_reliability_penalty_s = (coeff.transit_reliability_base_s.max(0.0)
            + 0.20 * expected_wait_s.max(0.0)
            + 70.0 * expected_transfer_count.max(0.0)
            + low_frequency_penalty_s
            + operational_reliability_penalty_s
            + irregularity_wait_penalty_s)
            * coeff.reliability_penalty_weight.max(0.0);

        let transfer_penalty_component_s = expected_transfer_penalty_s.max(0.0)
            * purpose_sens.transfer_aversion_multiplier.max(0.2)
            + expected_transfer_count.max(0.0) * coeff.transfer_aversion_s.max(0.0) * 0.08;
        let fare_equivalent_time_s = monetary_to_time_s(
            expected_fare_base.max(0.0) * coeff.fare_sensitivity.max(0.0),
            value_of_time_h,
            purpose_sens.cost_sensitivity.max(0.2),
        );
        let transit_access_s = expected_walk_s.max(0.0) * 0.5;
        let transit_egress_s = expected_walk_s.max(0.0) * 0.5;
        let transit_time_component_s = (transit_access_s
            + expected_wait_s.max(0.0)
            + expected_ivt_s.max(0.0)
            + transit_egress_s
            + transfer_penalty_component_s)
            * coeff.transit_gc_weight.max(0.0)
            * purpose_sens.value_of_time_weight.max(0.2);
        let transit_total_gc_s = if transit_available {
            transit_time_component_s
                + transit_crowding_penalty_s
                + transit_reliability_penalty_s
                + fare_equivalent_time_s
        } else {
            f64::INFINITY
        };

        let average_centrality =
            (origin_profile.centrality_score + destination_profile.centrality_score) * 0.5;
        let average_car_dependency =
            (origin_profile.car_dependency + destination_profile.car_dependency) * 0.5;
        let car_speed_kph =
            car_speed_kph_for_od(origin_profile, destination_profile, active_context, coeff)
                .max(6.0);
        let car_in_vehicle_time_s = (trip_distance_km / car_speed_kph) * 3600.0;
        let car_access_s = 45.0;
        let car_egress_s = 45.0;
        let car_parking_penalty_s =
            parking_penalty_s_for_destination(destination_profile, economy_cfg, coeff);
        let car_toll_proxy_base = if trip_distance_km >= 24.0 {
            coeff.car_toll_proxy_base.max(0.0)
        } else {
            0.0
        };
        let car_operating_cost_base =
            trip_distance_km.max(0.0) * coeff.car_operating_cost_base_per_km.max(0.0);
        let car_cost_equivalent_time_s = monetary_to_time_s(
            car_operating_cost_base + car_toll_proxy_base,
            value_of_time_h,
            purpose_sens.cost_sensitivity.max(0.2),
        );
        let peak_day_factor = if matches!(
            active_context.time_slice,
            DemandTimeSliceLabel::AmPeak | DemandTimeSliceLabel::PmPeak
        ) {
            1.0
        } else {
            0.0
        };
        let car_reliability_penalty_s = (coeff.car_reliability_base_s.max(0.0)
            + peak_day_factor * 140.0
            + average_centrality * 85.0)
            * coeff.reliability_penalty_weight.max(0.0);
        let car_total_gc_s = (car_access_s
            + car_in_vehicle_time_s.max(0.0)
            + car_egress_s
            + car_parking_penalty_s
            + car_cost_equivalent_time_s
            + car_reliability_penalty_s)
            * coeff.car_gc_weight.max(0.0)
            * purpose_sens.value_of_time_weight.max(0.2);

        let walk_available = trip_distance_km <= coeff.walk_suppression_distance_km.max(0.1);
        let walk_speed_kph = 4.8;
        let walk_in_vehicle_time_s = (trip_distance_km / walk_speed_kph) * 3600.0;
        let walk_penalty_s = if trip_distance_km > coeff.walk_max_distance_km.max(0.1) {
            ((trip_distance_km - coeff.walk_max_distance_km.max(0.1)) * 420.0).max(0.0)
        } else {
            0.0
        };
        let walk_total_gc_s = if walk_available {
            (walk_in_vehicle_time_s + walk_penalty_s)
                * coeff.walk_gc_weight.max(0.0)
                * purpose_sens.value_of_time_weight.max(0.2)
        } else {
            f64::INFINITY
        };

        let transit_constant = purpose_sens.transit_constant
            + avg_settlement_transit
            + (origin_profile.transit_affinity + destination_profile.transit_affinity - 1.0) * 0.32
            - average_car_dependency * 0.10;
        let car_constant =
            purpose_sens.car_constant + avg_settlement_car + average_car_dependency * 0.35
                - average_centrality * 0.24;
        let walk_constant = purpose_sens.walk_constant
            + avg_settlement_walk
            + if trip_distance_km <= 1.0 { 0.45 } else { 0.0 };
        let no_trip_base_constant = purpose_sens.suppression_constant;

        let utility_scale = coeff.utility_scale.max(1e-6);
        let transit_utility = if transit_available {
            transit_constant - utility_scale * transit_total_gc_s.max(0.0)
        } else {
            f64::NEG_INFINITY
        };
        let car_utility = car_constant - utility_scale * car_total_gc_s.max(0.0);
        let walk_utility = if walk_available {
            walk_constant - utility_scale * walk_total_gc_s.max(0.0)
        } else {
            f64::NEG_INFINITY
        };
        let best_cost = transit_total_gc_s
            .min(car_total_gc_s)
            .min(walk_total_gc_s)
            .min(12_000.0);
        let no_trip_utility =
            no_trip_base_constant - 0.85 + (best_cost.max(0.0) / 9000.0).clamp(0.0, 0.9);

        let mut candidates = vec![
            (CandidateMode::Car, car_utility),
            (CandidateMode::NoTrip, no_trip_utility),
        ];
        if transit_available {
            candidates.push((CandidateMode::Transit, transit_utility));
        }
        if walk_available {
            candidates.push((CandidateMode::Walk, walk_utility));
        }

        let max_u = candidates
            .iter()
            .map(|(_, u)| *u)
            .fold(f64::NEG_INFINITY, f64::max);
        let mut denom = 0.0_f64;
        let mut exp_values = Vec::<(CandidateMode, f64)>::new();
        for (mode, u) in &candidates {
            let e = (*u - max_u).exp();
            denom += e;
            exp_values.push((*mode, e));
        }
        if denom <= 0.0 || !denom.is_finite() {
            denom = 1.0;
        }
        let mut share_transit = 0.0_f64;
        let mut share_car = 0.0_f64;
        let mut share_walk = 0.0_f64;
        let mut share_no_trip = 0.0_f64;
        for (mode, e) in exp_values {
            let sh = (e / denom).clamp(0.0, 1.0);
            match mode {
                CandidateMode::Transit => share_transit = sh,
                CandidateMode::Car => share_car = sh,
                CandidateMode::Walk => share_walk = sh,
                CandidateMode::NoTrip => share_no_trip = sh,
            }
        }

        let transit_captured = latent * share_transit.max(0.0);
        let car_captured = latent * share_car.max(0.0);
        let walk_captured = latent * share_walk.max(0.0);
        let suppressed_or_no_trip = latent * share_no_trip.max(0.0);

        for entry in &mut transit_submode_split {
            entry.passengers = entry.share.max(0.0) * transit_captured.max(0.0);
        }
        if transit_submode_split.is_empty() && transit_captured > 0.0 {
            transit_submode_split.push(ModeShareValue {
                mode: TravelMode::OtherTransit,
                share: 1.0,
                passengers: transit_captured.max(0.0),
            });
        }

        let mut chosen_mode_shares = Vec::<ModeShareValue>::new();
        if transit_captured > 0.0 {
            for entry in &transit_submode_split {
                chosen_mode_shares.push(ModeShareValue {
                    mode: entry.mode,
                    share: share_transit * entry.share.max(0.0),
                    passengers: entry.passengers.max(0.0),
                });
            }
        }
        chosen_mode_shares.push(ModeShareValue {
            mode: TravelMode::Car,
            share: share_car.max(0.0),
            passengers: car_captured.max(0.0),
        });
        chosen_mode_shares.push(ModeShareValue {
            mode: TravelMode::Walk,
            share: share_walk.max(0.0),
            passengers: walk_captured.max(0.0),
        });
        chosen_mode_shares.push(ModeShareValue {
            mode: TravelMode::NoTrip,
            share: share_no_trip.max(0.0),
            passengers: suppressed_or_no_trip.max(0.0),
        });
        chosen_mode_shares.sort_by(|a, b| {
            b.passengers
                .partial_cmp(&a.passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let winning_mode = chosen_mode_shares
            .first()
            .map(|x| x.mode)
            .unwrap_or(TravelMode::NoTrip);
        let confidence = if chosen_mode_shares.len() >= 2 {
            Some(
                (chosen_mode_shares[0].share.max(0.0) - chosen_mode_shares[1].share.max(0.0))
                    .clamp(0.0, 1.0),
            )
        } else {
            Some(1.0)
        };
        let explanation = if transit_available && share_transit >= share_car {
            Some(format!(
                "transit competitive: gc {:.0}s vs car {:.0}s, crowding {:.0}s, transfer {:.2}, ops_pressure {:.2}",
                transit_total_gc_s,
                car_total_gc_s,
                transit_crowding_penalty_s,
                expected_transfer_count,
                expected_operational_pressure
            ))
        } else if transit_available {
            Some(format!(
                "car advantage: car gc {:.0}s vs transit {:.0}s (parking {:.0}s, transfers {:.2}, crowding {:.0}s, ops_pressure {:.2})",
                car_total_gc_s,
                transit_total_gc_s,
                car_parking_penalty_s,
                expected_transfer_count,
                transit_crowding_penalty_s,
                expected_operational_pressure
            ))
        } else {
            Some("transit unavailable for this OD in current network".to_string())
        };

        out.rows.push(ActiveModeChoiceOd {
            latent: od.clone(),
            transit_captured: transit_captured.max(0.0),
            suppressed_or_no_trip: suppressed_or_no_trip.max(0.0),
        });

        out.results.push(ModeChoiceResult {
            context: ModeChoiceContext {
                purpose: od.purpose,
                origin_zone_id: od.origin_zone_id.clone(),
                destination_zone_id: od.destination_zone_id.clone(),
                service_day_type: od.service_day_type,
                time_slice: od.time_slice,
                seasonal_profile: od.seasonal_profile,
                active_event_ids: od.active_event_ids.clone(),
                origin_settlement_class: origin_profile.settlement_class,
                destination_settlement_class: destination_profile.settlement_class,
                origin_archetype: origin_profile.archetype,
                destination_archetype: destination_profile.archetype,
                trip_distance_km,
            },
            latent_passengers: latent.max(0.0),
            chosen_mode_shares,
            generalized_costs_by_mode: vec![
                ModeGeneralizedCostByMode {
                    mode: TravelMode::OtherTransit,
                    breakdown: ModeGeneralizedCostBreakdown {
                        access_time_s: transit_access_s.max(0.0),
                        wait_time_s: expected_wait_s.max(0.0),
                        in_vehicle_time_s: expected_ivt_s.max(0.0),
                        transfer_penalty_s: transfer_penalty_component_s.max(0.0),
                        fare_cost_base: expected_fare_base.max(0.0),
                        parking_toll_proxy_base: 0.0,
                        crowding_penalty_s: transit_crowding_penalty_s.max(0.0),
                        reliability_penalty_s: transit_reliability_penalty_s.max(0.0),
                        egress_time_s: transit_egress_s.max(0.0),
                        total_generalized_cost_s: transit_total_gc_s,
                    },
                },
                ModeGeneralizedCostByMode {
                    mode: TravelMode::Car,
                    breakdown: ModeGeneralizedCostBreakdown {
                        access_time_s: car_access_s,
                        wait_time_s: 0.0,
                        in_vehicle_time_s: car_in_vehicle_time_s.max(0.0),
                        transfer_penalty_s: 0.0,
                        fare_cost_base: 0.0,
                        parking_toll_proxy_base: (car_operating_cost_base + car_toll_proxy_base)
                            .max(0.0),
                        crowding_penalty_s: 0.0,
                        reliability_penalty_s: car_reliability_penalty_s.max(0.0),
                        egress_time_s: (car_egress_s + car_parking_penalty_s).max(0.0),
                        total_generalized_cost_s: car_total_gc_s,
                    },
                },
                ModeGeneralizedCostByMode {
                    mode: TravelMode::Walk,
                    breakdown: ModeGeneralizedCostBreakdown {
                        access_time_s: 0.0,
                        wait_time_s: 0.0,
                        in_vehicle_time_s: walk_in_vehicle_time_s.max(0.0),
                        transfer_penalty_s: walk_penalty_s.max(0.0),
                        fare_cost_base: 0.0,
                        parking_toll_proxy_base: 0.0,
                        crowding_penalty_s: 0.0,
                        reliability_penalty_s: 0.0,
                        egress_time_s: 0.0,
                        total_generalized_cost_s: walk_total_gc_s,
                    },
                },
                ModeGeneralizedCostByMode {
                    mode: TravelMode::NoTrip,
                    breakdown: ModeGeneralizedCostBreakdown {
                        access_time_s: 0.0,
                        wait_time_s: 0.0,
                        in_vehicle_time_s: 0.0,
                        transfer_penalty_s: 0.0,
                        fare_cost_base: 0.0,
                        parking_toll_proxy_base: 0.0,
                        crowding_penalty_s: 0.0,
                        reliability_penalty_s: 0.0,
                        egress_time_s: 0.0,
                        total_generalized_cost_s: 0.0,
                    },
                },
            ],
            transit_captured_passengers: transit_captured.max(0.0),
            car_captured_passengers: car_captured.max(0.0),
            walk_captured_passengers: walk_captured.max(0.0),
            suppressed_or_no_trip_passengers: suppressed_or_no_trip.max(0.0),
            winning_mode,
            transit_submode_split,
            confidence,
            explanation,
        });
    }

    Ok(out)
}

fn monetary_to_time_s(cost_base: f64, value_of_time_h: f64, sensitivity: f64) -> f64 {
    if cost_base <= 0.0 {
        return 0.0;
    }
    let vot = value_of_time_h.max(1.0);
    let sens = sensitivity.max(0.1);
    ((cost_base * sens) / vot).max(0.0) * 3600.0
}

fn purpose_mode_sensitivity(
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> super::types::PurposeModeSensitivity {
    cfg.purpose_mode_sensitivities
        .iter()
        .find(|x| x.purpose == purpose)
        .cloned()
        .unwrap_or(super::types::PurposeModeSensitivity {
            purpose,
            value_of_time_weight: 1.0,
            cost_sensitivity: 1.0,
            transfer_aversion_multiplier: 1.0,
            crowding_aversion_multiplier: 1.0,
            transit_constant: 0.0,
            car_constant: 0.0,
            walk_constant: 0.0,
            suppression_constant: -2.0,
        })
}

fn settlement_mode_constant(
    settlement_class: SettlementClass,
    cfg: &SyntheticEconomyConfig,
) -> super::types::SettlementModeConstant {
    cfg.settlement_mode_constants
        .iter()
        .find(|x| x.settlement_class == settlement_class)
        .cloned()
        .unwrap_or(super::types::SettlementModeConstant {
            settlement_class,
            transit_constant: 0.0,
            car_constant: 0.0,
            walk_constant: 0.0,
        })
}

fn transit_submode_preference(
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> super::types::TransitSubmodePreference {
    cfg.transit_submode_preferences
        .iter()
        .find(|x| x.purpose == purpose)
        .cloned()
        .unwrap_or(super::types::TransitSubmodePreference {
            purpose,
            bus: 1.0,
            metro_tram: 1.0,
            suburban_rail: 1.0,
            regional_rail: 1.0,
            high_speed_rail: 1.0,
            other_transit: 1.0,
        })
}

fn submode_preference_for(mode: TravelMode, pref: &super::types::TransitSubmodePreference) -> f64 {
    match mode {
        TravelMode::Bus => pref.bus,
        TravelMode::MetroTram => pref.metro_tram,
        TravelMode::SuburbanRail => pref.suburban_rail,
        TravelMode::RegionalRail => pref.regional_rail,
        TravelMode::HighSpeedRail => pref.high_speed_rail,
        _ => pref.other_transit,
    }
}

fn parking_penalty_s_for_destination(
    destination: &ZoneDemandProfile,
    cfg: &SyntheticEconomyConfig,
    coeff: &super::types::ModeUtilityCoefficients,
) -> f64 {
    let base = match destination.settlement_class {
        SettlementClass::GlobalCityCore => coeff.car_parking_penalty_core_s.max(0.0),
        SettlementClass::MajorCity => coeff.car_parking_penalty_major_city_s.max(0.0),
        SettlementClass::RegionalCity => (coeff.car_parking_penalty_major_city_s * 0.75).max(0.0),
        SettlementClass::LargeTown | SettlementClass::SmallTown => {
            coeff.car_parking_penalty_town_s.max(0.0)
        }
        SettlementClass::Village | SettlementClass::Rural => {
            coeff.car_parking_penalty_town_s.max(0.0) * 0.30
        }
        SettlementClass::SpecialNode => coeff.car_parking_penalty_major_city_s.max(0.0) * 0.80,
    };
    let archetype_extra = cfg
        .archetype_parking_penalties
        .iter()
        .find(|x| x.archetype == destination.archetype)
        .map(|x| x.parking_penalty_s.max(0.0))
        .unwrap_or(0.0);
    (base + archetype_extra).max(0.0)
}

fn car_speed_kph_for_od(
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    context: &TemporalDemandSlice,
    coeff: &super::types::ModeUtilityCoefficients,
) -> f64 {
    let speed_for_settlement = |settlement: SettlementClass| match settlement {
        SettlementClass::GlobalCityCore => coeff.car_speed_kph_core.max(1.0),
        SettlementClass::MajorCity | SettlementClass::RegionalCity => {
            coeff.car_speed_kph_urban.max(1.0)
        }
        SettlementClass::LargeTown | SettlementClass::SmallTown => {
            coeff.car_speed_kph_suburban.max(1.0)
        }
        SettlementClass::Village | SettlementClass::Rural => coeff.car_speed_kph_rural.max(1.0),
        SettlementClass::SpecialNode => coeff.car_speed_kph_suburban.max(1.0),
    };
    let mut speed = 0.5
        * (speed_for_settlement(origin.settlement_class)
            + speed_for_settlement(destination.settlement_class));
    if matches!(
        context.time_slice,
        DemandTimeSliceLabel::AmPeak | DemandTimeSliceLabel::PmPeak
    ) && context.service_day_type == ServiceDayType::Weekday
    {
        speed /= coeff.car_congestion_peak_factor.max(1.0);
    } else if context.service_day_type != ServiceDayType::Weekday {
        speed /= coeff.car_congestion_weekend_factor.max(0.2);
    }
    speed.max(6.0)
}
