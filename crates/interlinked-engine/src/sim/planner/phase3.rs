use super::contracts::{purpose_from_index, purpose_index};
use super::*;

pub(super) fn build_phase3_planning_outputs(
    s: &Scenario,
    graph: &crate::sim::graph::Graph,
    zone_profiles: &[ZoneDemandProfile],
    zone_demand_layer: &[ZoneDemandLayerData],
    zone_demand_production_layer: &[ZoneDemandProductionLayerData],
    zone_demand_attraction_layer: &[ZoneDemandAttractionLayerData],
    corridor_desire_lines: &[CorridorDesireLineData],
    service_gap_layer: &[ZoneServiceGapLayerData],
    _service_load_layer: &[ServiceLoadLayerData],
    stop_flow_states: &[StopFlowState],
    vehicle_load_states: &[VehicleLoadState],
    assigned_od_flows: &[AssignedOdFlow],
    modal_outputs: &ModalOutputs,
    operations_outputs: &OperationsOutputs,
) -> Phase3PlanningOutputs {
    let cfg = PlanningOverlayConfig::default();
    let mut out = Phase3PlanningOutputs {
        planning_overlay_config: cfg.clone(),
        zone_planning_metrics: Vec::new(),
        station_planning_metrics: Vec::new(),
        corridor_planning_metrics: Vec::new(),
        line_service_planning_metrics: Vec::new(),
        build_preview_metrics: Vec::new(),
        service_gap_rankings: ServiceGapRankings::default(),
        planning_debug_summary: PlanningDebugSummary::default(),
    };

    if zone_profiles.is_empty() || s.world.zones.is_empty() {
        return out;
    }

    let zones = &s.world.zones;
    let stops = &s.world.stops;
    let zone_count = zones.len();
    let stop_by_id = stops
        .iter()
        .map(|x| (x.id.clone(), x))
        .collect::<HashMap<_, _>>();
    let zone_index_by_id = zones
        .iter()
        .enumerate()
        .map(|(idx, z)| (z.id.clone(), idx))
        .collect::<HashMap<_, _>>();
    let zone_by_id = zones
        .iter()
        .map(|z| (z.id.clone(), z))
        .collect::<HashMap<_, _>>();
    let profile_by_id = zone_profiles
        .iter()
        .map(|p| (p.zone_id.clone(), p))
        .collect::<HashMap<_, _>>();
    let zone_layer_by_id = zone_demand_layer
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();
    let production_by_zone = zone_demand_production_layer
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();
    let attraction_by_zone = zone_demand_attraction_layer
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();
    let _service_gap_by_zone = service_gap_layer
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();
    let stop_state_by_id = stop_flow_states
        .iter()
        .map(|st| (st.stop_id.clone(), st))
        .collect::<HashMap<_, _>>();
    let zone_mode_by_id = modal_outputs
        .zone_mode_share_metrics
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();
    let corridor_mode_by_pair = modal_outputs
        .corridor_mode_share_metrics
        .iter()
        .map(|c| ((c.origin_zone_id.clone(), c.destination_zone_id.clone()), c))
        .collect::<HashMap<_, _>>();
    let station_capture_by_stop = modal_outputs
        .station_transit_capture_context
        .iter()
        .map(|x| (x.stop_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let service_capture_by_id = modal_outputs
        .service_transit_capture_context
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let service_ops_by_id = operations_outputs
        .service_operation_states
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let stop_ops_by_id = operations_outputs
        .stop_operation_states
        .iter()
        .map(|x| (x.stop_id.clone(), x))
        .collect::<HashMap<_, _>>();

    let mut missed_transfer_rate_by_stop = HashMap::<String, f64>::new();
    for stop in &s.world.stops {
        let metrics = operations_outputs
            .transfer_operation_metrics
            .iter()
            .filter(|x| x.interchange_stop_id == stop.id)
            .collect::<Vec<_>>();
        if metrics.is_empty() {
            missed_transfer_rate_by_stop.insert(stop.id.clone(), 0.0);
            continue;
        }
        let weight = metrics
            .iter()
            .map(|x| x.missed_transfer_count.max(0.0) + 1.0)
            .sum::<f64>()
            .max(1e-9);
        let missed = metrics
            .iter()
            .map(|x| x.missed_transfer_rate.max(0.0) * (x.missed_transfer_count.max(0.0) + 1.0))
            .sum::<f64>()
            / weight;
        missed_transfer_rate_by_stop.insert(stop.id.clone(), missed.max(0.0));
    }

    let mut gc_matrix = vec![vec![f64::INFINITY; zone_count]; zone_count];
    for oi in 0..zone_count {
        let origin_node = graph.svc_index.zone_nodes_start + oi;
        let dist = dijkstra(graph, origin_node);
        for dj in 0..zone_count {
            let dest_node = graph.svc_index.zone_nodes_start + zone_count + dj;
            gc_matrix[oi][dj] = dist[dest_node].dist;
        }
    }

    let mut nearest_stop_by_zone = HashMap::<String, String>::new();
    let mut nearest_stop_dist_by_zone = HashMap::<String, f64>::new();
    for z in zones {
        let mut best: Option<(&str, f64)> = None;
        for stop in stops {
            let d = euclid_m((z.x, z.y), (stop.x, stop.y));
            if let Some((_, bd)) = best {
                if d < bd {
                    best = Some((stop.id.as_str(), d));
                }
            } else {
                best = Some((stop.id.as_str(), d));
            }
        }
        if let Some((sid, dist)) = best {
            nearest_stop_by_zone.insert(z.id.clone(), sid.to_string());
            nearest_stop_dist_by_zone.insert(z.id.clone(), dist.max(0.0));
        }
    }

    let mut nearest_zone_by_stop = HashMap::<String, String>::new();
    for stop in stops {
        let mut best: Option<(&str, f64)> = None;
        for z in zones {
            let d = euclid_m((z.x, z.y), (stop.x, stop.y));
            if let Some((_, bd)) = best {
                if d < bd {
                    best = Some((z.id.as_str(), d));
                }
            } else {
                best = Some((z.id.as_str(), d));
            }
        }
        if let Some((zid, _)) = best {
            nearest_zone_by_stop.insert(stop.id.clone(), zid.to_string());
        }
    }

    let mut zone_nearby_wait_board = HashMap::<String, (f64, f64)>::new();
    for z in zones {
        let mut waiting = 0.0_f64;
        let mut boardings = 0.0_f64;
        for st in stop_flow_states {
            if let Some(stop) = stop_by_id.get(&st.stop_id) {
                let d = euclid_m((z.x, z.y), (stop.x, stop.y));
                if d <= cfg.station_nearby_zone_radius_m {
                    waiting += st.total_waiting.max(0.0);
                    boardings += st.boarded_this_step.max(0.0);
                }
            }
        }
        zone_nearby_wait_board.insert(z.id.clone(), (waiting.max(0.0), boardings.max(0.0)));
    }

    let mut access_jobs_raw = vec![0.0_f64; zone_count];
    let mut access_essential_raw = vec![0.0_f64; zone_count];
    let mut access_education_raw = vec![0.0_f64; zone_count];
    let mut access_retail_raw = vec![0.0_f64; zone_count];
    let mut access_intercity_raw = vec![0.0_f64; zone_count];
    for oi in 0..zone_count {
        for dj in 0..zone_count {
            if oi == dj {
                continue;
            }
            let gc = gc_matrix[oi][dj];
            if !gc.is_finite() {
                continue;
            }
            let dz = &zones[dj];
            let dp = &zone_profiles[dj];
            let jobs_decay = accessibility_decay(gc, cfg.accessibility_jobs_gc_threshold_s);
            let ess_decay = accessibility_decay(gc, cfg.accessibility_essential_gc_threshold_s);
            let edu_decay = accessibility_decay(gc, cfg.accessibility_education_gc_threshold_s);
            let ret_decay = accessibility_decay(gc, cfg.accessibility_retail_gc_threshold_s);
            let int_decay = accessibility_decay(gc, cfg.accessibility_intercity_gc_threshold_s);

            access_jobs_raw[oi] += dz.jobs.max(0.0) * jobs_decay;
            access_essential_raw[oi] += (dp.essential_service_attractiveness.max(0.0)
                * (dz.population.max(0.0) + 100.0))
                * ess_decay;
            access_education_raw[oi] += (dp.education_attractiveness.max(0.0)
                * (dz.population.max(0.0) + 100.0))
                * edu_decay;
            access_retail_raw[oi] += ((dp.shopping_attractiveness.max(0.0)
                + dp.leisure_attractiveness.max(0.0))
                * 0.5
                * (dz.population.max(0.0) + 100.0))
                * ret_decay;
            access_intercity_raw[oi] += (dp.intercity_importance.max(0.0)
                * (dz.jobs.max(0.0) + dz.population.max(0.0)))
                * int_decay;
        }
    }
    let access_jobs = normalize_by_max(&access_jobs_raw);
    let access_essential = normalize_by_max(&access_essential_raw);
    let access_education = normalize_by_max(&access_education_raw);
    let access_retail = normalize_by_max(&access_retail_raw);
    let access_intercity = normalize_by_max(&access_intercity_raw);

    #[derive(Debug, Clone, Default)]
    struct PairAggregate {
        latent: f64,
        realised: f64,
        unserved: f64,
        by_purpose_latent: [f64; 6],
    }
    let mut pair_agg = HashMap::<(String, String), PairAggregate>::new();
    for c in corridor_desire_lines {
        let key = (c.origin_zone_id.clone(), c.destination_zone_id.clone());
        let entry = pair_agg.entry(key).or_default();
        entry.latent += c.latent_passengers.max(0.0);
        entry.realised += c.realised_passengers.max(0.0);
        entry.unserved += c.unserved_passengers.max(0.0);
        entry.by_purpose_latent[purpose_index(c.purpose)] += c.latent_passengers.max(0.0);
    }

    let mut zone_planning_metrics = Vec::<ZonePlanningMetrics>::new();
    for (i, z) in zones.iter().enumerate() {
        let profile = if let Some(p) = profile_by_id.get(&z.id) {
            *p
        } else {
            continue;
        };
        let zone_layer = zone_layer_by_id.get(&z.id);
        let production = production_by_zone.get(&z.id);
        let attraction = attraction_by_zone.get(&z.id);
        let total_latent_produced = zone_layer
            .map(|x| x.total_latent_demand_produced.max(0.0))
            .unwrap_or(0.0);
        let total_latent_attracted = zone_layer
            .map(|x| x.total_latent_demand_attracted.max(0.0))
            .unwrap_or(0.0);
        let total_realised_produced = zone_layer
            .map(|x| x.total_realised_demand_produced.max(0.0))
            .unwrap_or(0.0);
        let total_unserved_produced = zone_layer
            .map(|x| x.total_unserved_demand_produced.max(0.0))
            .unwrap_or(0.0);
        let total_realised_attracted = attraction
            .map(|a| {
                a.realised_by_purpose
                    .iter()
                    .map(|x| x.realised.max(0.0))
                    .sum::<f64>()
            })
            .unwrap_or(0.0);
        let service_coverage_score = if total_latent_produced > 0.0 {
            (total_realised_produced / total_latent_produced).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let latent_to_realised_ratio = if total_realised_produced > 0.0 {
            (total_latent_produced / total_realised_produced).max(0.0)
        } else if total_latent_produced > 0.0 {
            999.0
        } else {
            1.0
        };

        let mut dominant_trip_purpose = None::<TripPurpose>;
        if let Some(prod) = production {
            dominant_trip_purpose = prod
                .by_purpose
                .iter()
                .max_by(|a, b| {
                    a.latent
                        .partial_cmp(&b.latent)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|x| x.purpose);
        }

        let mut top_destination_zones = pair_agg
            .iter()
            .filter(|((oz, _), agg)| oz == &z.id && agg.latent > 0.0)
            .map(|((_, dz), agg)| ZoneFlowReference {
                zone_id: dz.clone(),
                passengers: agg.latent.max(0.0),
            })
            .collect::<Vec<_>>();
        top_destination_zones.sort_by(|a, b| {
            b.passengers
                .partial_cmp(&a.passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_destination_zones.truncate(5);

        let mut top_origin_zones = pair_agg
            .iter()
            .filter(|((_, dz), agg)| dz == &z.id && agg.latent > 0.0)
            .map(|((oz, _), agg)| ZoneFlowReference {
                zone_id: oz.clone(),
                passengers: agg.latent.max(0.0),
            })
            .collect::<Vec<_>>();
        top_origin_zones.sort_by(|a, b| {
            b.passengers
                .partial_cmp(&a.passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_origin_zones.truncate(5);

        let strongest_corridor_score = corridor_desire_lines
            .iter()
            .filter(|c| c.origin_zone_id == z.id || c.destination_zone_id == z.id)
            .map(|c| c.corridor_score.max(0.0))
            .fold(0.0_f64, f64::max);

        let nearby = zone_nearby_wait_board
            .get(&z.id)
            .copied()
            .unwrap_or((0.0, 0.0));
        let reliability_penalty_s = nearest_stop_by_zone
            .get(&z.id)
            .and_then(|sid| stop_ops_by_id.get(sid))
            .map(|ops| {
                (1.0 - ops.transfer_success_rate).max(0.0) * 120.0
                    + ops.headway_irregularity.max(0.0) * 210.0
                    + (ops.average_dwell_time_s.max(0.0) * 0.12)
            })
            .unwrap_or(0.0);
        let operational_underservice_score = nearest_stop_by_zone
            .get(&z.id)
            .and_then(|sid| stop_ops_by_id.get(sid))
            .map(|ops| {
                (ops.operational_pressure_score.max(0.0)
                    + (1.0 - ops.transfer_success_rate).max(0.0)
                    + ops.denied_boarding_pressure.max(0.0))
                .max(0.0)
            })
            .unwrap_or(0.0);

        let composite_accessibility_score = (0.28 * access_jobs[i]
            + 0.24 * access_essential[i]
            + 0.18 * access_education[i]
            + 0.18 * access_retail[i]
            + 0.12 * access_intercity[i])
            .clamp(0.0, 1.0);

        zone_planning_metrics.push(ZonePlanningMetrics {
            zone_id: z.id.clone(),
            settlement_class: profile.settlement_class,
            archetype: profile.archetype,
            population: z.population.max(0.0),
            jobs: z.jobs.max(0.0),
            centrality_score: profile.centrality_score,
            total_latent_produced,
            total_latent_attracted,
            total_realised_produced,
            total_realised_attracted,
            total_unserved_produced,
            latent_to_realised_ratio,
            access_to_jobs_score: access_jobs[i],
            access_to_services_score: access_essential[i],
            access_to_education_score: access_education[i],
            access_to_retail_leisure_score: access_retail[i],
            intercity_access_score: access_intercity[i],
            composite_accessibility_score,
            accessibility_score: composite_accessibility_score,
            service_coverage_score,
            dominant_trip_purpose,
            top_destination_zones,
            top_origin_zones,
            strongest_corridor_score,
            current_modeled_waiting_nearby: nearby.0,
            current_boardings_nearby: nearby.1,
            transit_capture_share: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.transit_share.max(0.0))
                .unwrap_or(0.0),
            car_capture_share: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.car_share.max(0.0))
                .unwrap_or(0.0),
            walk_capture_share: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.walk_share.max(0.0))
                .unwrap_or(0.0),
            suppressed_share: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.suppressed_share.max(0.0))
                .unwrap_or(0.0),
            transit_captured_produced: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.transit_captured_demand.max(0.0))
                .unwrap_or(0.0),
            non_transit_captured_produced: zone_mode_by_id
                .get(&z.id)
                .map(|m| m.non_transit_demand.max(0.0))
                .unwrap_or(0.0),
            reliability_penalty_s: reliability_penalty_s.max(0.0),
            operational_underservice_score: operational_underservice_score.max(0.0),
            transit_revenue_generated: 0.0,
            subsidy_need_proxy: 0.0,
        });
    }
    zone_planning_metrics.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));
    out.zone_planning_metrics = zone_planning_metrics.clone();

    let zone_metrics_by_id = zone_planning_metrics
        .iter()
        .map(|z| (z.zone_id.clone(), z))
        .collect::<HashMap<_, _>>();

    let mut stop_frequency_proxy = HashMap::<String, f64>::new();
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
        for stop_id in &svc.stop_sequence {
            *stop_frequency_proxy.entry(stop_id.clone()).or_insert(0.0) += tph.max(0.0);
        }
    }

    let mut catchment_zone_ids_by_stop = HashMap::<String, Vec<String>>::new();
    for stop in stops {
        let mut catch = Vec::<String>::new();
        for z in zones {
            let d = euclid_m((stop.x, stop.y), (z.x, z.y));
            if d <= cfg.station_catchment_radius_m {
                catch.push(z.id.clone());
            }
        }
        catchment_zone_ids_by_stop.insert(stop.id.clone(), catch);
    }

    let mut station_planning_metrics = Vec::<StationPlanningMetrics>::new();
    for stop in stops {
        let catchment_zone_ids = catchment_zone_ids_by_stop
            .get(&stop.id)
            .cloned()
            .unwrap_or_default();
        let catchment_set = catchment_zone_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut catchment_population = 0.0_f64;
        let mut catchment_jobs = 0.0_f64;
        let mut catchment_education = 0.0_f64;
        let mut catchment_retail_leisure = 0.0_f64;
        let mut latent_demand_in_catchment = 0.0_f64;
        let mut realised_demand_in_catchment = 0.0_f64;
        let mut unserved_demand_in_catchment = 0.0_f64;
        let mut purpose_scores = [0.0_f64; 6];
        for zid in &catchment_zone_ids {
            if let Some(z) = zone_by_id.get(zid) {
                catchment_population += z.population.max(0.0);
                catchment_jobs += z.jobs.max(0.0);
            }
            if let Some(profile) = profile_by_id.get(zid) {
                catchment_education += profile.education_intensity.max(0.0)
                    * (profile.population.max(0.0) + profile.jobs.max(0.0));
                catchment_retail_leisure += (profile.retail_intensity.max(0.0)
                    + profile.leisure_intensity.max(0.0))
                    * (profile.population.max(0.0) + profile.jobs.max(0.0));
            }
            if let Some(metrics) = zone_metrics_by_id.get(zid) {
                latent_demand_in_catchment += metrics.total_latent_produced.max(0.0);
                realised_demand_in_catchment += metrics.total_realised_produced.max(0.0);
                unserved_demand_in_catchment += metrics.total_unserved_produced.max(0.0);
            }
            if let Some(prod) = production_by_zone.get(zid) {
                for pv in &prod.by_purpose {
                    purpose_scores[purpose_index(pv.purpose)] += pv.latent.max(0.0);
                }
            }
        }

        let mut primary_trip_purposes_served = purpose_scores
            .iter()
            .enumerate()
            .filter(|(_, score)| **score > 0.0)
            .map(|(idx, score)| PurposeScoreValue {
                purpose: purpose_from_index(idx),
                score: *score,
            })
            .collect::<Vec<_>>();
        primary_trip_purposes_served.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        primary_trip_purposes_served.truncate(3);

        let state = stop_state_by_id.get(&stop.id);
        let boardings_total = state.map(|x| x.boarded_this_step.max(0.0)).unwrap_or(0.0);
        let alightings_total = state.map(|x| x.alighted_this_step.max(0.0)).unwrap_or(0.0);
        let waiting_now = state.map(|x| x.total_waiting.max(0.0)).unwrap_or(0.0);
        let denied_total = state.map(|x| x.denied_this_step.max(0.0)).unwrap_or(0.0);
        let arrivals_completed_total = state.map(|x| x.arrived_this_step.max(0.0)).unwrap_or(0.0);
        let service_frequency_proxy = stop_frequency_proxy.get(&stop.id).copied().unwrap_or(0.0);

        let max_stop_crowding = vehicle_load_states
            .iter()
            .filter(|x| x.stop_id == stop.id)
            .map(|x| x.crowding_ratio.max(0.0))
            .fold(0.0_f64, f64::max);
        let waiting_ratio = waiting_now / (boardings_total + 1.0);
        let denied_ratio = denied_total / (boardings_total + 1.0);
        let load_pressure_score =
            (0.68 * waiting_ratio + 0.32 * denied_ratio * 2.0).clamp(0.0, 5.0);
        let crowding_excess =
            (max_stop_crowding - cfg.overcrowding_crowding_ratio_threshold).max(0.0);
        let overcrowding_risk_score = (0.50
            * (waiting_ratio / cfg.overcrowding_waiting_ratio_threshold.max(0.05))
            + 0.25 * denied_ratio
            + 0.25 * (crowding_excess * 3.0))
            .clamp(0.0, 3.0);
        let stop_ops = stop_ops_by_id.get(&stop.id);
        let average_dwell_time_s = stop_ops
            .map(|x| x.average_dwell_time_s.max(0.0))
            .unwrap_or(0.0);
        let max_dwell_time_s = stop_ops.map(|x| x.max_dwell_time_s.max(0.0)).unwrap_or(0.0);
        let platform_crowding_proxy = stop_ops
            .map(|x| x.platform_crowding_proxy.max(0.0))
            .unwrap_or(waiting_now.max(0.0));
        let transfer_success_rate = stop_ops
            .map(|x| x.transfer_success_rate.clamp(0.0, 1.0))
            .unwrap_or_else(|| {
                let missed = missed_transfer_rate_by_stop
                    .get(&stop.id)
                    .copied()
                    .unwrap_or(0.0);
                (1.0 - missed).clamp(0.0, 1.0)
            });
        let headway_irregularity = stop_ops
            .map(|x| x.headway_irregularity.max(0.0))
            .unwrap_or(0.0);
        let average_headway_realised_s = stop_ops
            .map(|x| x.average_headway_realised_s.max(0.0))
            .unwrap_or(0.0);
        let operational_pressure_score = stop_ops
            .map(|x| x.operational_pressure_score.max(0.0))
            .unwrap_or_else(|| (load_pressure_score + overcrowding_risk_score * 0.5).max(0.0));

        let mut top_destinations_from_station = state
            .map(|x| {
                x.waiting_by_destination
                    .iter()
                    .map(|w| StopFlowReference {
                        stop_id: w.destination_stop_id.clone(),
                        passengers: w.waiting_passengers.max(0.0),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        top_destinations_from_station.sort_by(|a, b| {
            b.passengers
                .partial_cmp(&a.passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_destinations_from_station.truncate(5);
        if top_destinations_from_station.is_empty() {
            let mut fallback_dest = HashMap::<String, f64>::new();
            for flow in assigned_od_flows {
                if flow.assigned_passengers <= 0.0 || !catchment_set.contains(&flow.origin_zone_id)
                {
                    continue;
                }
                if let Some(dest_stop) = nearest_stop_by_zone.get(&flow.destination_zone_id) {
                    *fallback_dest.entry(dest_stop.clone()).or_insert(0.0) +=
                        flow.assigned_passengers.max(0.0);
                }
            }
            top_destinations_from_station = fallback_dest
                .into_iter()
                .map(|(stop_id, passengers)| StopFlowReference {
                    stop_id,
                    passengers: passengers.max(0.0),
                })
                .collect::<Vec<_>>();
            top_destinations_from_station.sort_by(|a, b| {
                b.passengers
                    .partial_cmp(&a.passengers)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            top_destinations_from_station.truncate(5);
        }

        station_planning_metrics.push(StationPlanningMetrics {
            stop_id: stop.id.clone(),
            catchment_population: catchment_population.max(0.0),
            catchment_jobs: catchment_jobs.max(0.0),
            catchment_education: catchment_education.max(0.0),
            catchment_retail_leisure: catchment_retail_leisure.max(0.0),
            boardings_total: boardings_total.max(0.0),
            alightings_total: alightings_total.max(0.0),
            waiting_now: waiting_now.max(0.0),
            denied_total: denied_total.max(0.0),
            arrivals_completed_total: arrivals_completed_total.max(0.0),
            load_pressure_score,
            overcrowding_risk_score,
            service_frequency_proxy: service_frequency_proxy.max(0.0),
            latent_demand_in_catchment: latent_demand_in_catchment.max(0.0),
            realised_demand_in_catchment: realised_demand_in_catchment.max(0.0),
            unserved_demand_in_catchment: unserved_demand_in_catchment.max(0.0),
            transit_captured_demand_in_catchment: station_capture_by_stop
                .get(&stop.id)
                .map(|x| x.transit_captured_demand.max(0.0))
                .unwrap_or(0.0),
            uncaptured_competing_demand_in_catchment: station_capture_by_stop
                .get(&stop.id)
                .map(|x| x.uncaptured_competing_demand.max(0.0))
                .unwrap_or(0.0),
            transit_capture_share_in_catchment: station_capture_by_stop
                .get(&stop.id)
                .map(|x| {
                    if x.catchment_latent_demand > 0.0 {
                        (x.transit_captured_demand / x.catchment_latent_demand).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0),
            capture_limited_by_crowding: station_capture_by_stop
                .get(&stop.id)
                .map(|x| x.limiting_crowding_signal > 0.25)
                .unwrap_or(false),
            average_dwell_time_s,
            max_dwell_time_s,
            platform_crowding_proxy,
            transfer_success_rate,
            operational_pressure_score,
            headway_irregularity,
            average_headway_realised_s,
            associated_revenue: 0.0,
            operating_cost_burden_proxy: 0.0,
            capital_cost_burden_proxy: 0.0,
            strategic_value_proxy: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
            primary_trip_purposes_served,
            top_destinations_from_station,
        });
    }
    station_planning_metrics.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));
    out.station_planning_metrics = station_planning_metrics.clone();

    let mut corridor_planning_metrics = Vec::<CorridorPlanningMetrics>::new();
    for ((oz, dz), agg) in &pair_agg {
        if agg.latent <= 0.0 {
            continue;
        }
        let dom_idx = agg
            .by_purpose_latent
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        let dominant_purpose = purpose_from_index(dom_idx);
        let served_ratio = if agg.latent > 0.0 {
            (agg.realised / agg.latent).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let origin_zone = if let Some(v) = zone_by_id.get(oz) {
            *v
        } else {
            continue;
        };
        let dest_zone = if let Some(v) = zone_by_id.get(dz) {
            *v
        } else {
            continue;
        };
        let origin_profile = if let Some(v) = profile_by_id.get(oz) {
            *v
        } else {
            continue;
        };
        let dest_profile = if let Some(v) = profile_by_id.get(dz) {
            *v
        } else {
            continue;
        };
        let dist_km =
            euclid_km((origin_zone.x, origin_zone.y), (dest_zone.x, dest_zone.y)).max(0.05);
        let gc_opt =
            if let (Some(oi), Some(dj)) = (zone_index_by_id.get(oz), zone_index_by_id.get(dz)) {
                let gc = gc_matrix[*oi][*dj];
                if gc.is_finite() && gc > 0.0 {
                    Some(gc)
                } else {
                    None
                }
            } else {
                None
            };
        let directness_score = gc_opt.map(|gc| {
            let baseline = ((dist_km / 35.0) * 3600.0 + 180.0).max(1.0);
            (baseline / gc.max(1.0)).clamp(0.05, 1.5)
        });
        let corridor_classification = classify_corridor_phase3(
            origin_profile,
            dest_profile,
            dominant_purpose,
            dist_km,
            agg.latent,
            agg.unserved,
            &cfg,
        );
        let likely_mode_fit = recommend_service_class_for_corridor(
            corridor_classification,
            dominant_purpose,
            dist_km,
        );
        let stop_ops_o = nearest_stop_by_zone
            .get(oz)
            .and_then(|sid| stop_ops_by_id.get(sid))
            .copied();
        let stop_ops_d = nearest_stop_by_zone
            .get(dz)
            .and_then(|sid| stop_ops_by_id.get(sid))
            .copied();
        let avg_transfer_success = match (stop_ops_o, stop_ops_d) {
            (Some(a), Some(b)) => 0.5 * (a.transfer_success_rate + b.transfer_success_rate),
            (Some(a), None) => a.transfer_success_rate,
            (None, Some(b)) => b.transfer_success_rate,
            _ => 1.0,
        }
        .clamp(0.0, 1.0);
        let avg_irregularity = match (stop_ops_o, stop_ops_d) {
            (Some(a), Some(b)) => 0.5 * (a.headway_irregularity + b.headway_irregularity),
            (Some(a), None) => a.headway_irregularity,
            (None, Some(b)) => b.headway_irregularity,
            _ => 0.0,
        }
        .max(0.0);
        let avg_operational_pressure = match (stop_ops_o, stop_ops_d) {
            (Some(a), Some(b)) => {
                0.5 * (a.operational_pressure_score + b.operational_pressure_score)
            }
            (Some(a), None) => a.operational_pressure_score,
            (None, Some(b)) => b.operational_pressure_score,
            _ => 0.0,
        }
        .max(0.0);
        let reliability_adjusted_service_quality =
            (served_ratio * avg_transfer_success * (1.0 - 0.55 * avg_irregularity)).clamp(0.0, 1.0);
        let recurring_bottleneck_score = (avg_operational_pressure
            + (1.0 - avg_transfer_success).max(0.0)
            + (agg.unserved / (agg.realised + 1.0)) * 0.35)
            .max(0.0);
        let missed_transfer_sensitivity =
            ((1.0 - avg_transfer_success).max(0.0) + 0.45 * avg_irregularity).max(0.0);
        let crowding_delay_pressure = (avg_operational_pressure
            + 0.5 * avg_irregularity
            + (agg.unserved / (agg.realised + 1.0)) * 0.45)
            .max(0.0);

        corridor_planning_metrics.push(CorridorPlanningMetrics {
            origin_zone_id: oz.clone(),
            destination_zone_id: dz.clone(),
            dominant_purpose,
            latent_volume: agg.latent.max(0.0),
            realised_volume: agg.realised.max(0.0),
            unserved_volume: agg.unserved.max(0.0),
            served_ratio,
            average_generalized_cost_s: gc_opt,
            directness_score,
            corridor_classification,
            likely_mode_fit,
            dominant_mode: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.dominant_mode)
                .unwrap_or(TravelMode::Car),
            transit_share: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.transit_share.max(0.0))
                .unwrap_or(0.0),
            car_share: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.car_share.max(0.0))
                .unwrap_or(0.0),
            walk_share: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.walk_share.max(0.0))
                .unwrap_or(0.0),
            suppressed_share: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.suppressed_share.max(0.0))
                .unwrap_or(0.0),
            strongest_transit_submode: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.strongest_transit_submode)
                .unwrap_or(TravelMode::OtherTransit),
            transit_capture_gap: corridor_mode_by_pair
                .get(&(oz.clone(), dz.clone()))
                .map(|m| m.transit_capture_gap.max(0.0))
                .unwrap_or(0.0),
            reliability_adjusted_service_quality,
            recurring_bottleneck_score,
            missed_transfer_sensitivity,
            crowding_delay_pressure,
            fare_revenue: 0.0,
            operating_cost_allocated: 0.0,
            total_cost_allocated: 0.0,
            subsidy_required: 0.0,
            farebox_recovery_ratio: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
        });
    }
    corridor_planning_metrics.sort_by(|a, b| {
        b.latent_volume
            .partial_cmp(&a.latent_volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.corridor_planning_metrics = corridor_planning_metrics.clone();

    let mut link_dist_by_pair = HashMap::<(String, String), f64>::new();
    let mut link_ids_by_pair = HashMap::<(String, String), Vec<String>>::new();
    for link in &s.world.links {
        link_dist_by_pair
            .entry((link.from_stop.clone(), link.to_stop.clone()))
            .or_insert(link.distance_m.max(0.0));
        link_ids_by_pair
            .entry((link.from_stop.clone(), link.to_stop.clone()))
            .or_default()
            .push(link.id.clone());
    }

    let mut line_service_planning_metrics = Vec::<LineOrServicePlanningMetrics>::new();
    for svc in &s.world.services {
        let states = vehicle_load_states
            .iter()
            .filter(|x| x.service_id == svc.id)
            .collect::<Vec<_>>();
        let total_boardings = states
            .iter()
            .map(|x| x.boardings_this_stop.max(0.0))
            .sum::<f64>();
        let peak_load = states
            .iter()
            .map(|x| x.load_after_stop.max(0.0))
            .fold(0.0_f64, f64::max);
        let average_load = if states.is_empty() {
            0.0
        } else {
            states
                .iter()
                .map(|x| x.load_after_stop.max(0.0))
                .sum::<f64>()
                / (states.len() as f64)
        };
        let max_load_point = states
            .iter()
            .max_by(|a, b| {
                a.load_after_stop
                    .partial_cmp(&b.load_after_stop)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|x| x.stop_id.clone());

        let mut state_by_stop = HashMap::<String, &VehicleLoadState>::new();
        for st in &states {
            state_by_stop.insert(st.stop_id.clone(), *st);
        }

        let mut service_link_ids = std::collections::HashSet::<String>::new();
        let mut passenger_km = 0.0_f64;
        let mut overcrowded_segments = 0usize;
        for win in svc.stop_sequence.windows(2) {
            let from = win[0].clone();
            let to = win[1].clone();
            let dist_m = link_dist_by_pair
                .get(&(from.clone(), to.clone()))
                .copied()
                .unwrap_or(0.0)
                .max(0.0);
            if let Some(ids) = link_ids_by_pair.get(&(from.clone(), to.clone())) {
                for id in ids {
                    service_link_ids.insert(id.clone());
                }
            }
            let onboard = state_by_stop
                .get(&from)
                .map(|x| x.load_after_stop.max(0.0))
                .unwrap_or(average_load.max(0.0));
            passenger_km += onboard * (dist_m / 1000.0);
            let capacity = state_by_stop
                .get(&from)
                .map(|x| x.capacity.max(0.0))
                .unwrap_or(svc.vehicle_capacity.max(0.0));
            if capacity > 0.0
                && onboard / capacity >= cfg.overcrowding_crowding_ratio_threshold.max(0.01)
            {
                overcrowded_segments += 1;
            }
        }

        let mut od_patterns = HashMap::<(String, String, TripPurpose), f64>::new();
        for flow in assigned_od_flows {
            for path in &flow.chosen_paths {
                if path.assigned_passengers <= 0.0 {
                    continue;
                }
                if path.link_ids.iter().any(|x| service_link_ids.contains(x)) {
                    *od_patterns
                        .entry((
                            flow.origin_zone_id.clone(),
                            flow.destination_zone_id.clone(),
                            flow.purpose,
                        ))
                        .or_insert(0.0) += path.assigned_passengers.max(0.0);
                }
            }
        }
        let mut strongest_origin_destination_patterns = od_patterns
            .into_iter()
            .map(|((oz, dz, purpose), passengers)| OdPatternMetric {
                origin_zone_id: oz,
                destination_zone_id: dz,
                purpose,
                passengers: passengers.max(0.0),
            })
            .collect::<Vec<_>>();
        strongest_origin_destination_patterns.sort_by(|a, b| {
            b.passengers
                .partial_cmp(&a.passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        strongest_origin_destination_patterns.truncate(5);

        let endpoint_dist_km = if let (Some(first), Some(last)) =
            (svc.stop_sequence.first(), svc.stop_sequence.last())
        {
            let a = stop_by_id.get(first);
            let b = stop_by_id.get(last);
            if let (Some(aa), Some(bb)) = (a, b) {
                euclid_km((aa.x, aa.y), (bb.x, bb.y))
            } else {
                0.0
            }
        } else {
            0.0
        };
        let canonical_mode =
            canonical_mode_from_tokens(&svc.mode, svc.mode_variant.as_deref(), endpoint_dist_km);
        let role_classification = classify_service_role_phase3(
            svc,
            canonical_mode,
            endpoint_dist_km,
            peak_load,
            average_load,
            &nearest_zone_by_stop,
            &profile_by_id,
            &cfg,
        );
        let capacity = states
            .iter()
            .map(|x| x.capacity.max(0.0))
            .fold(svc.vehicle_capacity.max(0.0), f64::max);
        let utilisation_score = if capacity > 0.0 {
            (average_load / capacity).max(0.0)
        } else {
            0.0
        };
        let service_ops = service_ops_by_id.get(&svc.id).copied();
        let scheduled_headway_s = service_ops
            .map(|x| x.expected_headway_s.max(0.0))
            .unwrap_or(svc.headway_s.max(0.0));
        let realised_headway_s = service_ops
            .map(|x| x.average_headway_realised_s.max(0.0))
            .unwrap_or(svc.headway_s.max(0.0));
        let headway_irregularity = service_ops
            .map(|x| x.headway_irregularity.max(0.0))
            .unwrap_or(0.0);
        let average_delay_s = service_ops
            .map(|x| x.average_delay_s.max(0.0))
            .unwrap_or(0.0);
        let max_delay_s = service_ops.map(|x| x.max_delay_s.max(0.0)).unwrap_or(0.0);
        let average_dwell_s = service_ops
            .map(|x| x.average_dwell_time_s.max(0.0))
            .unwrap_or(svc.dwell_s.max(0.0));
        let max_dwell_s = service_ops
            .map(|x| x.max_dwell_time_s.max(0.0))
            .unwrap_or(svc.dwell_s.max(0.0));
        let reliability_score = service_ops
            .map(|x| x.reliability_score.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let transfer_success_rate = service_ops
            .map(|x| x.transfer_success_rate.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let bunching_risk_score = if scheduled_headway_s > 0.0 {
            (headway_irregularity / 0.18).clamp(0.0, 3.0)
        } else {
            0.0
        };
        let operational_pressure_score = ((1.0 - reliability_score).max(0.0)
            + headway_irregularity.max(0.0)
            + utilisation_score.max(0.0) * 0.35)
            .max(0.0);

        line_service_planning_metrics.push(LineOrServicePlanningMetrics {
            service_id: svc.id.clone(),
            line_id: svc.line_id.clone(),
            total_boardings: total_boardings.max(0.0),
            passenger_km: passenger_km.max(0.0),
            peak_load: peak_load.max(0.0),
            average_load: average_load.max(0.0),
            max_load_point,
            overcrowded_segments,
            strongest_origin_destination_patterns,
            role_classification,
            utilisation_score,
            service_mode_family: service_capture_by_id
                .get(&svc.id)
                .map(|x| x.service_mode)
                .unwrap_or_else(|| canonical_mode.travel_mode_family()),
            transit_captured_demand: service_capture_by_id
                .get(&svc.id)
                .map(|x| x.transit_captured_demand.max(0.0))
                .unwrap_or(0.0),
            uncaptured_competing_demand_near_service: service_capture_by_id
                .get(&svc.id)
                .map(|x| x.uncaptured_competing_demand.max(0.0))
                .unwrap_or(0.0),
            crowding_lost_share_signal: service_capture_by_id
                .get(&svc.id)
                .map(|x| x.crowding_lost_share_signal.max(0.0))
                .unwrap_or(0.0),
            scheduled_headway_s,
            realised_headway_s,
            headway_irregularity,
            average_delay_s,
            max_delay_s,
            average_dwell_s,
            max_dwell_s,
            bunching_risk_score,
            reliability_score,
            transfer_success_rate,
            operational_pressure_score,
            fare_revenue: 0.0,
            operating_cost: 0.0,
            infrastructure_cost_allocated: 0.0,
            rolling_stock_cost_allocated: 0.0,
            total_cost: 0.0,
            operating_surplus_deficit: 0.0,
            full_cost_surplus_deficit: 0.0,
            subsidy_required: 0.0,
            farebox_recovery_ratio: 0.0,
            cost_per_passenger: 0.0,
            cost_per_passenger_km: 0.0,
            revenue_per_passenger: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
            reliability_cost_pressure: 0.0,
        });
    }
    line_service_planning_metrics.sort_by(|a, b| a.service_id.cmp(&b.service_id));
    out.line_service_planning_metrics = line_service_planning_metrics.clone();

    let mut underserved_zone_scores = zone_planning_metrics
        .iter()
        .map(|z| ZoneScoreEntry {
            zone_id: z.zone_id.clone(),
            score: (z.total_unserved_produced.max(0.0)
                * (1.0 + (1.0 - z.service_coverage_score).max(0.0)))
                + (z.total_latent_produced.max(0.0)
                    * (1.0 - z.service_coverage_score).max(0.0)
                    * 0.35),
        })
        .collect::<Vec<_>>();
    underserved_zone_scores.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    underserved_zone_scores.truncate(12);

    let mut underserved_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.unserved_volume > 0.0)
        .cloned()
        .collect::<Vec<_>>();
    underserved_corridors.sort_by(|a, b| {
        b.unserved_volume
            .partial_cmp(&a.unserved_volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    underserved_corridors.truncate(12);

    let mut overcrowded_stations = station_planning_metrics
        .iter()
        .map(|st| StationScoreEntry {
            stop_id: st.stop_id.clone(),
            score: st.overcrowding_risk_score + st.denied_total / (st.boardings_total + 1.0),
            reason: format!(
                "waiting {:.1}, denied {:.1}, freq {:.2} tph",
                st.waiting_now, st.denied_total, st.service_frequency_proxy
            ),
        })
        .collect::<Vec<_>>();
    overcrowded_stations.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overcrowded_stations.truncate(12);

    let mut overcrowded_services = line_service_planning_metrics
        .iter()
        .map(|svc| ServiceScoreEntry {
            service_id: svc.service_id.clone(),
            score: svc.utilisation_score + (svc.overcrowded_segments as f64) * 0.25,
            reason: format!(
                "utilisation {:.2}, overcrowded_segments {}",
                svc.utilisation_score, svc.overcrowded_segments
            ),
        })
        .collect::<Vec<_>>();
    overcrowded_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overcrowded_services.truncate(12);

    let mut weak_access_rural_zones = zone_planning_metrics
        .iter()
        .filter(|z| {
            matches!(
                z.settlement_class,
                SettlementClass::Village | SettlementClass::Rural
            )
        })
        .map(|z| ZoneScoreEntry {
            zone_id: z.zone_id.clone(),
            score: (1.0 - z.accessibility_score).max(0.0) * 200.0
                + z.total_unserved_produced.max(0.0),
        })
        .collect::<Vec<_>>();
    weak_access_rural_zones.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    weak_access_rural_zones.truncate(12);

    let mut transit_capture_opportunity_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.transit_capture_gap > 0.0)
        .cloned()
        .collect::<Vec<_>>();
    transit_capture_opportunity_corridors.sort_by(|a, b| {
        b.transit_capture_gap
            .partial_cmp(&a.transit_capture_gap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    transit_capture_opportunity_corridors.truncate(12);

    let mut car_dominated_transit_viable_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.car_share > c.transit_share && c.transit_share >= 0.08)
        .cloned()
        .collect::<Vec<_>>();
    car_dominated_transit_viable_corridors.sort_by(|a, b| {
        (b.car_share * b.transit_capture_gap)
            .partial_cmp(&(a.car_share * a.transit_capture_gap))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    car_dominated_transit_viable_corridors.truncate(12);

    let mut overcrowded_corridors_losing_mode_share = corridor_planning_metrics
        .iter()
        .filter(|c| c.unserved_volume > 0.0 && c.car_share >= c.transit_share)
        .cloned()
        .collect::<Vec<_>>();
    overcrowded_corridors_losing_mode_share.sort_by(|a, b| {
        (b.unserved_volume * (1.0 + b.car_share))
            .partial_cmp(&(a.unserved_volume * (1.0 + a.car_share)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overcrowded_corridors_losing_mode_share.truncate(12);

    let mut socially_important_low_demand_services = line_service_planning_metrics
        .iter()
        .filter(|svc| {
            svc.total_boardings < 120.0
                && matches!(
                    svc.role_classification,
                    ServiceRoleClassification::LocalCoverage
                        | ServiceRoleClassification::RegionalConnector
                )
        })
        .map(|svc| ServiceScoreEntry {
            service_id: svc.service_id.clone(),
            score: (svc.uncaptured_competing_demand_near_service + 1.0)
                / (svc.total_boardings + 1.0)
                + (1.0 - svc.utilisation_score.min(1.0)),
            reason: format!(
                "boardings {:.1}, uncaptured {:.1}, role {:?}",
                svc.total_boardings,
                svc.uncaptured_competing_demand_near_service,
                svc.role_classification
            ),
        })
        .collect::<Vec<_>>();
    socially_important_low_demand_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    socially_important_low_demand_services.truncate(12);

    let mut top_unreliable_services = line_service_planning_metrics
        .iter()
        .map(|svc| ServiceScoreEntry {
            service_id: svc.service_id.clone(),
            score: (1.0 - svc.reliability_score).max(0.0)
                + svc.headway_irregularity.max(0.0)
                + (svc.average_delay_s / 240.0).max(0.0),
            reason: format!(
                "reliability {:.2}, avg delay {:.1}s, irregularity {:.2}",
                svc.reliability_score, svc.average_delay_s, svc.headway_irregularity
            ),
        })
        .collect::<Vec<_>>();
    top_unreliable_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_unreliable_services.truncate(12);

    let mut top_dwell_pressure_stations = station_planning_metrics
        .iter()
        .map(|st| StationScoreEntry {
            stop_id: st.stop_id.clone(),
            score: (st.average_dwell_time_s / 20.0).max(0.0)
                + (st.max_dwell_time_s / 35.0).max(0.0)
                + st.operational_pressure_score.max(0.0),
            reason: format!(
                "avg dwell {:.1}s, max dwell {:.1}s, pressure {:.2}",
                st.average_dwell_time_s, st.max_dwell_time_s, st.operational_pressure_score
            ),
        })
        .collect::<Vec<_>>();
    top_dwell_pressure_stations.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_dwell_pressure_stations.truncate(12);

    let mut top_bunching_prone_lines = line_service_planning_metrics
        .iter()
        .map(|svc| ServiceScoreEntry {
            service_id: svc.service_id.clone(),
            score: svc.bunching_risk_score.max(0.0)
                + svc.headway_irregularity.max(0.0)
                + (svc.realised_headway_s / svc.scheduled_headway_s.max(1.0) - 1.0).max(0.0),
            reason: format!(
                "bunching {:.2}, scheduled {:.0}s, realised {:.0}s",
                svc.bunching_risk_score, svc.scheduled_headway_s, svc.realised_headway_s
            ),
        })
        .collect::<Vec<_>>();
    top_bunching_prone_lines.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_bunching_prone_lines.truncate(12);

    let mut top_missed_transfer_interchanges = station_planning_metrics
        .iter()
        .filter(|st| st.transfer_success_rate < 0.98 || st.headway_irregularity > 0.0)
        .map(|st| StationScoreEntry {
            stop_id: st.stop_id.clone(),
            score: (1.0 - st.transfer_success_rate).max(0.0)
                + 0.45 * st.headway_irregularity.max(0.0)
                + 0.25 * st.operational_pressure_score.max(0.0),
            reason: format!(
                "transfer success {:.2}, irregularity {:.2}",
                st.transfer_success_rate, st.headway_irregularity
            ),
        })
        .collect::<Vec<_>>();
    top_missed_transfer_interchanges.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_missed_transfer_interchanges.truncate(12);

    let mut top_corridors_losing_capture_due_to_unreliability = corridor_planning_metrics
        .iter()
        .filter(|c| c.transit_capture_gap > 0.0 && c.recurring_bottleneck_score > 0.0)
        .cloned()
        .collect::<Vec<_>>();
    top_corridors_losing_capture_due_to_unreliability.sort_by(|a, b| {
        (b.transit_capture_gap
            * (1.0 + b.recurring_bottleneck_score + b.missed_transfer_sensitivity))
            .partial_cmp(
                &(a.transit_capture_gap
                    * (1.0 + a.recurring_bottleneck_score + a.missed_transfer_sensitivity)),
            )
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_corridors_losing_capture_due_to_unreliability.truncate(12);

    let mut top_operational_bottlenecks = station_planning_metrics
        .iter()
        .map(|st| StationScoreEntry {
            stop_id: st.stop_id.clone(),
            score: st.operational_pressure_score.max(0.0)
                + (1.0 - st.transfer_success_rate).max(0.0)
                + st.denied_total / (st.boardings_total + 1.0),
            reason: format!(
                "pressure {:.2}, waiting {:.1}, denied {:.1}",
                st.operational_pressure_score, st.waiting_now, st.denied_total
            ),
        })
        .collect::<Vec<_>>();
    top_operational_bottlenecks.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_operational_bottlenecks.truncate(12);

    let mut build_preview_metrics = Vec::<BuildPreviewMetrics>::new();

    for (rank, zscore) in underserved_zone_scores.iter().enumerate().take(4) {
        let zone_id = &zscore.zone_id;
        let zone_metric = if let Some(zm) = zone_metrics_by_id.get(zone_id) {
            *zm
        } else {
            continue;
        };
        let zone = if let Some(z) = zone_by_id.get(zone_id) {
            *z
        } else {
            continue;
        };
        let nearest_dist = nearest_stop_dist_by_zone
            .get(zone_id)
            .copied()
            .unwrap_or(cfg.station_catchment_radius_m * 2.0);
        let uncovered_factor = if nearest_dist > cfg.station_catchment_radius_m {
            1.0
        } else {
            (nearest_dist / cfg.station_catchment_radius_m).clamp(0.15, 0.65)
        };
        let latent_demand_interceptable = zone_metric.total_latent_produced.max(0.0)
            * (1.0 - zone_metric.service_coverage_score).max(0.0)
            * (0.45 + 0.35 * uncovered_factor);
        let unserved_demand_addressable = zone_metric.total_unserved_produced.max(0.0) * 0.75;
        let mut strongest_trip_purposes_unlocked = production_by_zone
            .get(zone_id)
            .map(|prod| {
                prod.by_purpose
                    .iter()
                    .map(|v| PurposeScoreValue {
                        purpose: v.purpose,
                        score: v.unserved.max(0.0) + 0.35 * v.latent.max(0.0),
                    })
                    .filter(|v| v.score > 0.0)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        strongest_trip_purposes_unlocked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        strongest_trip_purposes_unlocked.truncate(3);

        let mut strongest_corridors_touched = corridor_planning_metrics
            .iter()
            .filter(|c| c.origin_zone_id == *zone_id || c.destination_zone_id == *zone_id)
            .map(|c| CorridorReference {
                origin_zone_id: c.origin_zone_id.clone(),
                destination_zone_id: c.destination_zone_id.clone(),
                purpose: c.dominant_purpose,
                score: c.unserved_volume.max(0.0),
            })
            .collect::<Vec<_>>();
        strongest_corridors_touched.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        strongest_corridors_touched.truncate(3);

        let explanation = if matches!(
            zone_metric.settlement_class,
            SettlementClass::Village | SettlementClass::Rural
        ) {
            format!(
                "Improves coverage from {} toward nearest service centre and essential demand.",
                zone_id
            )
        } else if zone_metric.dominant_trip_purpose == Some(TripPurpose::Work) {
            format!(
                "Captures commuter demand from {} toward stronger employment centres.",
                zone_id
            )
        } else {
            format!(
                "Adds access capacity around {} where latent demand is under-served.",
                zone_id
            )
        };

        build_preview_metrics.push(BuildPreviewMetrics {
            preview_id: format!("preview_station_{}", rank + 1),
            preview_type: BuildPreviewType::Station,
            affected_zones: vec![zone_id.clone()],
            estimated_new_coverage_population: zone.population.max(0.0) * uncovered_factor,
            estimated_new_coverage_jobs: zone.jobs.max(0.0) * uncovered_factor,
            latent_demand_interceptable: latent_demand_interceptable.max(0.0),
            unserved_demand_addressable: unserved_demand_addressable.max(0.0),
            strongest_trip_purposes_unlocked,
            strongest_corridors_touched,
            expected_nodes_affected: vec![zone_id.clone()],
            accessibility_delta_proxy: ((1.0 - zone_metric.accessibility_score)
                * (1.0 - zone_metric.service_coverage_score))
                .clamp(0.03, 0.40),
            confidence: (0.60 + 0.10 * uncovered_factor).clamp(0.45, 0.9),
            explanation,
            estimated_revenue_uplift: 0.0,
            estimated_operating_cost_uplift: 0.0,
            estimated_capital_cost: 0.0,
            estimated_farebox_recovery: 0.0,
            likely_subsidy_requirement: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
            reinvestment_case_score: 0.0,
        });
    }

    for (rank, corridor) in underserved_corridors.iter().enumerate().take(3) {
        let oz = corridor.origin_zone_id.clone();
        let dz = corridor.destination_zone_id.clone();
        let zone_o = zone_by_id.get(&oz).copied();
        let zone_d = zone_by_id.get(&dz).copied();
        let pop = zone_o.map(|z| z.population.max(0.0)).unwrap_or(0.0)
            + zone_d.map(|z| z.population.max(0.0)).unwrap_or(0.0);
        let jobs = zone_o.map(|z| z.jobs.max(0.0)).unwrap_or(0.0)
            + zone_d.map(|z| z.jobs.max(0.0)).unwrap_or(0.0);
        let explanation = match corridor.corridor_classification {
            CorridorClassification::SuburbanCommuterRadial => {
                "Connects commuter demand into a major centre corridor.".to_string()
            }
            CorridorClassification::Intercity => {
                "Strengthens long-distance core-to-core demand linkage.".to_string()
            }
            CorridorClassification::AirportAccess => {
                "Improves airport access from high-demand urban zones.".to_string()
            }
            CorridorClassification::RuralEssentialConnector => {
                "Improves village/rural essential access to service towns.".to_string()
            }
            _ => "Adds direct capacity on a high unmet-demand corridor.".to_string(),
        };
        build_preview_metrics.push(BuildPreviewMetrics {
            preview_id: format!("preview_line_{}", rank + 1),
            preview_type: BuildPreviewType::LineSegment,
            affected_zones: vec![oz.clone(), dz.clone()],
            estimated_new_coverage_population: (pop * 0.35).max(0.0),
            estimated_new_coverage_jobs: (jobs * 0.35).max(0.0),
            latent_demand_interceptable: (corridor.latent_volume * 0.55).max(0.0),
            unserved_demand_addressable: (corridor.unserved_volume * 0.80).max(0.0),
            strongest_trip_purposes_unlocked: vec![PurposeScoreValue {
                purpose: corridor.dominant_purpose,
                score: corridor.latent_volume.max(0.0),
            }],
            strongest_corridors_touched: vec![CorridorReference {
                origin_zone_id: oz.clone(),
                destination_zone_id: dz.clone(),
                purpose: corridor.dominant_purpose,
                score: corridor.unserved_volume.max(0.0),
            }],
            expected_nodes_affected: vec![
                nearest_stop_by_zone.get(&oz).cloned().unwrap_or(oz.clone()),
                nearest_stop_by_zone.get(&dz).cloned().unwrap_or(dz.clone()),
            ],
            accessibility_delta_proxy: ((1.0 - corridor.served_ratio) * 0.45 + 0.08)
                .clamp(0.03, 0.45),
            confidence: if corridor.unserved_volume > 0.0 {
                0.74
            } else {
                0.56
            },
            explanation,
            estimated_revenue_uplift: 0.0,
            estimated_operating_cost_uplift: 0.0,
            estimated_capital_cost: 0.0,
            estimated_farebox_recovery: 0.0,
            likely_subsidy_requirement: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
            reinvestment_case_score: 0.0,
        });
    }

    for (rank, svc) in overcrowded_services.iter().enumerate().take(3) {
        let line_metric = if let Some(v) = line_service_planning_metrics
            .iter()
            .find(|x| x.service_id == svc.service_id)
        {
            v
        } else {
            continue;
        };
        let service = if let Some(v) = s.world.services.iter().find(|x| x.id == svc.service_id) {
            v
        } else {
            continue;
        };

        let mut affected_zone_set = std::collections::HashSet::<String>::new();
        for stop_id in &service.stop_sequence {
            if let Some(zid) = nearest_zone_by_stop.get(stop_id) {
                affected_zone_set.insert(zid.clone());
            }
        }
        let affected_zones = affected_zone_set.into_iter().collect::<Vec<_>>();
        let mut pop = 0.0_f64;
        let mut jobs = 0.0_f64;
        let mut latent_total = 0.0_f64;
        let mut unserved_total = 0.0_f64;
        let mut purpose_scores = [0.0_f64; 6];
        for zid in &affected_zones {
            if let Some(z) = zone_by_id.get(zid) {
                pop += z.population.max(0.0);
                jobs += z.jobs.max(0.0);
            }
            if let Some(zm) = zone_metrics_by_id.get(zid) {
                latent_total += zm.total_latent_produced.max(0.0);
                unserved_total += zm.total_unserved_produced.max(0.0);
            }
            if let Some(prod) = production_by_zone.get(zid) {
                for p in &prod.by_purpose {
                    purpose_scores[purpose_index(p.purpose)] +=
                        p.unserved.max(0.0) + 0.25 * p.latent.max(0.0);
                }
            }
        }
        let mut strongest_trip_purposes_unlocked = purpose_scores
            .iter()
            .enumerate()
            .filter(|(_, score)| **score > 0.0)
            .map(|(idx, score)| PurposeScoreValue {
                purpose: purpose_from_index(idx),
                score: *score,
            })
            .collect::<Vec<_>>();
        strongest_trip_purposes_unlocked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        strongest_trip_purposes_unlocked.truncate(3);

        let mut strongest_corridors_touched = corridor_planning_metrics
            .iter()
            .filter(|c| {
                affected_zones.contains(&c.origin_zone_id)
                    || affected_zones.contains(&c.destination_zone_id)
            })
            .map(|c| CorridorReference {
                origin_zone_id: c.origin_zone_id.clone(),
                destination_zone_id: c.destination_zone_id.clone(),
                purpose: c.dominant_purpose,
                score: c.unserved_volume.max(0.0),
            })
            .collect::<Vec<_>>();
        strongest_corridors_touched.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        strongest_corridors_touched.truncate(3);

        build_preview_metrics.push(BuildPreviewMetrics {
            preview_id: format!("preview_freq_{}", rank + 1),
            preview_type: BuildPreviewType::ServiceFrequencyIncrease,
            affected_zones: affected_zones.clone(),
            estimated_new_coverage_population: (pop * 0.20).max(0.0),
            estimated_new_coverage_jobs: (jobs * 0.20).max(0.0),
            latent_demand_interceptable: (latent_total * 0.18).max(0.0),
            unserved_demand_addressable: (unserved_total * 0.42).max(0.0),
            strongest_trip_purposes_unlocked,
            strongest_corridors_touched,
            expected_nodes_affected: service.stop_sequence.clone(),
            accessibility_delta_proxy: (line_metric.utilisation_score * 0.2
                + (line_metric.overcrowded_segments as f64) * 0.06)
                .clamp(0.02, 0.30),
            confidence: (0.56 + 0.08 * (line_metric.overcrowded_segments as f64)).clamp(0.45, 0.9),
            explanation: format!(
                "Raises frequency on {} to relieve crowding and improve waiting performance.",
                service.id
            ),
            estimated_revenue_uplift: 0.0,
            estimated_operating_cost_uplift: 0.0,
            estimated_capital_cost: 0.0,
            estimated_farebox_recovery: 0.0,
            likely_subsidy_requirement: 0.0,
            commercial_strength_classification: CommercialStrengthClassification::Marginal,
            social_necessity_classification: SocialNecessityClassification::Supportive,
            reinvestment_case_score: 0.0,
        });
    }

    build_preview_metrics.sort_by(|a, b| {
        preview_score(b, &cfg)
            .partial_cmp(&preview_score(a, &cfg))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    build_preview_metrics.truncate(16);
    out.build_preview_metrics = build_preview_metrics.clone();

    out.service_gap_rankings = ServiceGapRankings {
        top_underserved_zones: underserved_zone_scores.clone(),
        top_underserved_corridors: underserved_corridors.clone(),
        top_overcrowded_stations: overcrowded_stations.clone(),
        top_overcrowded_services: overcrowded_services.clone(),
        top_weak_access_rural_zones: weak_access_rural_zones.clone(),
        top_high_potential_interventions: build_preview_metrics.iter().take(8).cloned().collect(),
        top_transit_capture_opportunity_corridors: transit_capture_opportunity_corridors.clone(),
        top_car_dominated_transit_viable_corridors: car_dominated_transit_viable_corridors.clone(),
        top_overcrowded_corridors_losing_mode_share: overcrowded_corridors_losing_mode_share
            .clone(),
        top_socially_important_low_demand_services: socially_important_low_demand_services.clone(),
        top_unreliable_services: top_unreliable_services.clone(),
        top_dwell_pressure_stations: top_dwell_pressure_stations.clone(),
        top_bunching_prone_lines: top_bunching_prone_lines.clone(),
        top_missed_transfer_interchanges: top_missed_transfer_interchanges.clone(),
        top_corridors_losing_capture_due_to_unreliability:
            top_corridors_losing_capture_due_to_unreliability.clone(),
        top_operational_bottlenecks: top_operational_bottlenecks.clone(),
        top_profitable_services: Vec::new(),
        top_loss_making_high_ridership_services: Vec::new(),
        top_subsidy_dependent_social_corridors: Vec::new(),
        top_expensive_underperforming_services: Vec::new(),
        top_reinvestment_worthy_corridors: Vec::new(),
        top_socially_valuable_commercially_weak_links: Vec::new(),
    };

    let mut strongest_metro_suitable_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.corridor_classification == CorridorClassification::UrbanTrunkMetroSuitable)
        .cloned()
        .collect::<Vec<_>>();
    strongest_metro_suitable_corridors.sort_by(|a, b| {
        b.latent_volume
            .partial_cmp(&a.latent_volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_metro_suitable_corridors.truncate(8);

    let mut strongest_intercity_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.corridor_classification == CorridorClassification::Intercity)
        .cloned()
        .collect::<Vec<_>>();
    strongest_intercity_corridors.sort_by(|a, b| {
        b.latent_volume
            .partial_cmp(&a.latent_volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_intercity_corridors.truncate(8);

    let mut strongest_rural_essential_gaps = corridor_planning_metrics
        .iter()
        .filter(|c| c.corridor_classification == CorridorClassification::RuralEssentialConnector)
        .filter(|c| c.unserved_volume > 0.0)
        .cloned()
        .collect::<Vec<_>>();
    strongest_rural_essential_gaps.sort_by(|a, b| {
        b.unserved_volume
            .partial_cmp(&a.unserved_volume)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_rural_essential_gaps.truncate(8);

    let mut top_zones_by_transit_share = zone_planning_metrics
        .iter()
        .map(|z| ZoneScoreEntry {
            zone_id: z.zone_id.clone(),
            score: z.transit_capture_share.max(0.0),
        })
        .collect::<Vec<_>>();
    top_zones_by_transit_share.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zones_by_transit_share.truncate(8);

    let mut top_car_dominated_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.car_share > c.transit_share)
        .cloned()
        .collect::<Vec<_>>();
    top_car_dominated_corridors.sort_by(|a, b| {
        (b.car_share * b.latent_volume)
            .partial_cmp(&(a.car_share * a.latent_volume))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_car_dominated_corridors.truncate(8);

    let mut strongest_commuter_transit_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.dominant_purpose == TripPurpose::Work && c.transit_share >= 0.35)
        .cloned()
        .collect::<Vec<_>>();
    strongest_commuter_transit_corridors.sort_by(|a, b| {
        (b.transit_share * b.latent_volume)
            .partial_cmp(&(a.transit_share * a.latent_volume))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_commuter_transit_corridors.truncate(8);

    let mut strongest_intercity_rail_corridors = corridor_planning_metrics
        .iter()
        .filter(|c| c.dominant_purpose == TripPurpose::Intercity)
        .filter(|c| {
            matches!(
                c.strongest_transit_submode,
                TravelMode::RegionalRail | TravelMode::HighSpeedRail | TravelMode::SuburbanRail
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    strongest_intercity_rail_corridors.sort_by(|a, b| {
        (b.transit_share * b.latent_volume)
            .partial_cmp(&(a.transit_share * a.latent_volume))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    strongest_intercity_rail_corridors.truncate(8);

    let mut zones_losing_transit_due_to_transfers = modal_outputs
        .modal_demand_diagnostics
        .top_zones_losing_due_to_transfers
        .iter()
        .map(|x| ZoneScoreEntry {
            zone_id: x.id.clone(),
            score: x.score,
        })
        .collect::<Vec<_>>();
    zones_losing_transit_due_to_transfers.truncate(8);

    let mut zones_losing_transit_due_to_crowding = modal_outputs
        .modal_demand_diagnostics
        .top_zones_losing_due_to_crowding
        .iter()
        .map(|x| ZoneScoreEntry {
            zone_id: x.id.clone(),
            score: x.score,
        })
        .collect::<Vec<_>>();
    zones_losing_transit_due_to_crowding.truncate(8);

    let mut zones_where_parking_penalty_supports_transit = modal_outputs
        .modal_demand_diagnostics
        .top_zones_where_parking_penalty_supports_transit
        .iter()
        .map(|x| ZoneScoreEntry {
            zone_id: x.id.clone(),
            score: x.score,
        })
        .collect::<Vec<_>>();
    zones_where_parking_penalty_supports_transit.truncate(8);

    let mut top_nominally_frequent_but_poor_delivery_services = line_service_planning_metrics
        .iter()
        .filter(|svc| {
            svc.scheduled_headway_s > 0.0
                && svc.scheduled_headway_s <= 600.0
                && (svc.reliability_score < 0.78
                    || svc.realised_headway_s > svc.scheduled_headway_s * 1.15)
        })
        .map(|svc| ServiceScoreEntry {
            service_id: svc.service_id.clone(),
            score: (1.0 - svc.reliability_score).max(0.0)
                + (svc.realised_headway_s / svc.scheduled_headway_s.max(1.0) - 1.0).max(0.0),
            reason: format!(
                "scheduled {:.0}s, realised {:.0}s, reliability {:.2}",
                svc.scheduled_headway_s, svc.realised_headway_s, svc.reliability_score
            ),
        })
        .collect::<Vec<_>>();
    top_nominally_frequent_but_poor_delivery_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_nominally_frequent_but_poor_delivery_services.truncate(8);

    out.planning_debug_summary = PlanningDebugSummary {
        top_underserved_zones_with_reasons: underserved_zone_scores
            .iter()
            .take(8)
            .cloned()
            .collect(),
        top_overcrowded_stations_with_causes: overcrowded_stations
            .iter()
            .take(8)
            .cloned()
            .collect(),
        strongest_metro_suitable_corridors,
        strongest_intercity_corridors,
        strongest_rural_essential_gaps,
        top_candidate_interventions: build_preview_metrics.iter().take(6).cloned().collect(),
        top_zones_by_transit_share,
        top_car_dominated_corridors,
        strongest_commuter_transit_corridors,
        strongest_intercity_rail_corridors,
        zones_losing_transit_due_to_transfers,
        zones_losing_transit_due_to_crowding,
        zones_where_parking_penalty_supports_transit,
        top_unreliable_services: top_unreliable_services.iter().take(8).cloned().collect(),
        top_dwell_pressure_stations: top_dwell_pressure_stations
            .iter()
            .take(8)
            .cloned()
            .collect(),
        top_bunching_prone_lines: top_bunching_prone_lines.iter().take(8).cloned().collect(),
        top_missed_transfer_interchanges: top_missed_transfer_interchanges
            .iter()
            .take(8)
            .cloned()
            .collect(),
        top_corridors_losing_capture_due_to_unreliability:
            top_corridors_losing_capture_due_to_unreliability
                .iter()
                .take(8)
                .cloned()
                .collect(),
        top_nominally_frequent_but_poor_delivery_services,
        top_revenue_corridors: Vec::new(),
        top_operating_cost_heavy_services: Vec::new(),
        best_farebox_recovery_services: Vec::new(),
        worst_full_cost_deficit_services: Vec::new(),
        strongest_commercial_opportunities: Vec::new(),
        strongest_social_necessity_corridors: Vec::new(),
        corridors_where_unreliability_hurts_finances: Vec::new(),
        overloaded_highly_profitable_services: Vec::new(),
    };

    out
}

fn normalize_by_max(values: &[f64]) -> Vec<f64> {
    let max = values.iter().copied().fold(0.0_f64, f64::max);
    if max > 0.0 {
        values.iter().map(|v| (v / max).clamp(0.0, 1.0)).collect()
    } else {
        vec![0.0; values.len()]
    }
}

fn accessibility_decay(gc_s: f64, threshold_s: f64) -> f64 {
    if !gc_s.is_finite() || threshold_s <= 0.0 {
        0.0
    } else {
        (-gc_s.max(0.0) / threshold_s.max(1.0))
            .exp()
            .clamp(0.0, 1.0)
    }
}

fn classify_corridor_phase3(
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    dominant_purpose: TripPurpose,
    dist_km: f64,
    latent_volume: f64,
    unserved_volume: f64,
    cfg: &PlanningOverlayConfig,
) -> CorridorClassification {
    let has_airport = origin
        .special_attractors
        .contains(&SpecialAttractorType::Airport)
        || destination
            .special_attractors
            .contains(&SpecialAttractorType::Airport);
    let has_university = origin
        .special_attractors
        .contains(&SpecialAttractorType::University)
        || destination
            .special_attractors
            .contains(&SpecialAttractorType::University);
    let origin_rank = settlement_rank(origin.settlement_class);
    let dest_rank = settlement_rank(destination.settlement_class);

    if dominant_purpose == TripPurpose::Intercity
        && dist_km >= cfg.corridor_intercity_distance_km_threshold
        && (origin_rank >= settlement_rank(SettlementClass::RegionalCity)
            || dest_rank >= settlement_rank(SettlementClass::RegionalCity))
    {
        return CorridorClassification::Intercity;
    }
    if has_airport
        && matches!(
            dominant_purpose,
            TripPurpose::Intercity | TripPurpose::Work | TripPurpose::Leisure
        )
    {
        return CorridorClassification::AirportAccess;
    }
    if has_university && dominant_purpose == TripPurpose::Education {
        return CorridorClassification::EducationConnector;
    }
    if matches!(dominant_purpose, TripPurpose::Essential)
        && (matches!(
            origin.settlement_class,
            SettlementClass::Village | SettlementClass::Rural
        ) || matches!(
            destination.settlement_class,
            SettlementClass::Village | SettlementClass::Rural
        ))
    {
        return CorridorClassification::RuralEssentialConnector;
    }
    if dominant_purpose == TripPurpose::Work
        && dist_km <= cfg.corridor_commuter_distance_km_max
        && ((origin_rank >= settlement_rank(SettlementClass::RegionalCity)
            && matches!(
                destination.settlement_class,
                SettlementClass::LargeTown
                    | SettlementClass::SmallTown
                    | SettlementClass::Village
                    | SettlementClass::Rural
            ))
            || (dest_rank >= settlement_rank(SettlementClass::RegionalCity)
                && matches!(
                    origin.settlement_class,
                    SettlementClass::LargeTown
                        | SettlementClass::SmallTown
                        | SettlementClass::Village
                        | SettlementClass::Rural
                )))
    {
        return CorridorClassification::SuburbanCommuterRadial;
    }
    if dist_km <= 9.0 && latent_volume >= cfg.corridor_metro_volume_threshold {
        return CorridorClassification::UrbanTrunkMetroSuitable;
    }
    if dist_km <= 16.0 {
        return CorridorClassification::UrbanLocal;
    }
    if unserved_volume > 0.0 && dist_km > cfg.corridor_intercity_distance_km_threshold {
        return CorridorClassification::RegionalConnector;
    }
    CorridorClassification::Mixed
}

fn recommend_service_class_for_corridor(
    class: CorridorClassification,
    dominant_purpose: TripPurpose,
    dist_km: f64,
) -> RecommendedServiceClass {
    match class {
        CorridorClassification::UrbanTrunkMetroSuitable => RecommendedServiceClass::MetroTrunk,
        CorridorClassification::SuburbanCommuterRadial => RecommendedServiceClass::SuburbanRail,
        CorridorClassification::Intercity => RecommendedServiceClass::IntercityRail,
        CorridorClassification::RegionalConnector => RecommendedServiceClass::RegionalRail,
        CorridorClassification::AirportAccess => RecommendedServiceClass::AirportExpress,
        CorridorClassification::EducationConnector => RecommendedServiceClass::TramOrBrt,
        CorridorClassification::RuralEssentialConnector => RecommendedServiceClass::CoverageBus,
        CorridorClassification::UrbanLocal => RecommendedServiceClass::FrequentBus,
        CorridorClassification::Mixed => {
            if dominant_purpose == TripPurpose::Intercity || dist_km > 55.0 {
                RecommendedServiceClass::RegionalRail
            } else {
                RecommendedServiceClass::FrequentBus
            }
        }
    }
}

fn classify_service_role_phase3(
    service: &Service,
    canonical_mode: CanonicalTransitMode,
    endpoint_dist_km: f64,
    peak_load: f64,
    average_load: f64,
    nearest_zone_by_stop: &HashMap<String, String>,
    profile_by_id: &HashMap<String, &ZoneDemandProfile>,
    cfg: &PlanningOverlayConfig,
) -> ServiceRoleClassification {
    let has_airport = service.stop_sequence.iter().any(|stop_id| {
        nearest_zone_by_stop
            .get(stop_id)
            .and_then(|zid| profile_by_id.get(zid))
            .map(|z| {
                z.special_attractors
                    .contains(&SpecialAttractorType::Airport)
            })
            .unwrap_or(false)
    });
    if has_airport {
        return ServiceRoleClassification::AirportExpress;
    }
    match canonical_mode {
        CanonicalTransitMode::HighSpeedRail => ServiceRoleClassification::Intercity,
        CanonicalTransitMode::RegionalRail => {
            if endpoint_dist_km >= cfg.corridor_intercity_distance_km_threshold {
                ServiceRoleClassification::Intercity
            } else {
                ServiceRoleClassification::RegionalConnector
            }
        }
        CanonicalTransitMode::SuburbanRail => ServiceRoleClassification::CommuterRadial,
        CanonicalTransitMode::Metro | CanonicalTransitMode::Tram => {
            if peak_load >= cfg.corridor_metro_volume_threshold * 0.6
                || average_load >= cfg.corridor_metro_volume_threshold * 0.4
            {
                ServiceRoleClassification::UrbanTrunk
            } else {
                ServiceRoleClassification::Feeder
            }
        }
        CanonicalTransitMode::Bus => {
            if endpoint_dist_km > 18.0 {
                ServiceRoleClassification::RegionalConnector
            } else {
                ServiceRoleClassification::LocalCoverage
            }
        }
        CanonicalTransitMode::Ferry => ServiceRoleClassification::RegionalConnector,
        CanonicalTransitMode::OtherTransit => ServiceRoleClassification::Mixed,
    }
}

fn preview_score(preview: &BuildPreviewMetrics, cfg: &PlanningOverlayConfig) -> f64 {
    preview.unserved_demand_addressable.max(0.0) * cfg.preview_intercept_weight_unserved.max(0.0)
        + preview.latent_demand_interceptable.max(0.0)
            * cfg.preview_intercept_weight_latent.max(0.0)
        + preview.accessibility_delta_proxy.max(0.0)
            * 100.0
            * cfg.preview_accessibility_delta_weight
}
