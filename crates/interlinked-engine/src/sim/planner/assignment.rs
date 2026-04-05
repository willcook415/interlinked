use super::*;

pub(super) struct AssignmentKernelOutputs {
    pub(super) link_loads: Vec<LinkLoad>,
    pub(super) final_board_loads: Vec<BoardLoad>,
    pub(super) stop_flows: Vec<StopFlow>,
    pub(super) passenger_cohorts: Vec<PassengerCohortFlow>,
    pub(super) fare_flow: FareFlowSummary,
    pub(super) assigned_od_flows: Vec<AssignedOdFlow>,
    pub(super) stop_flow_states: Vec<StopFlowState>,
    pub(super) vehicle_load_states: Vec<VehicleLoadState>,
    pub(super) service_load_layer_light: Vec<ServiceLoadLayerData>,
    pub(super) sample_paths: Vec<SampleOdPaths>,
    pub(super) iters_run: usize,
    pub(super) last_max_rel_change: f64,
    pub(super) total_trips_attempted: f64,
    pub(super) total_trips_served: f64,
    pub(super) sum_gc: f64,
    pub(super) sum_walk: f64,
    pub(super) sum_wait: f64,
    pub(super) sum_ivt: f64,
    pub(super) sum_transfer_time: f64,
    pub(super) sum_transfer_pen: f64,
    pub(super) sum_transfers: f64,
    pub(super) sum_boardings: f64,
    pub(super) sum_fare_revenue_base: f64,
    pub(super) total_boardings_attempted: f64,
    pub(super) total_boardings_served: f64,
    pub(super) total_boardings_denied: f64,
    pub(super) total_overflow_dropped: f64,
    pub(super) share_boardings_served: f64,
    pub(super) share_demand_overflow_dropped: f64,
    pub(super) share_trips_served: f64,
}

pub(super) fn run_assignment_kernel(
    s: &Scenario,
    settings: &SimulationSettings,
    state_in: Option<&SimState>,
    stop_index: &HashMap<String, usize>,
    zone_index: &HashMap<String, usize>,
    mode_choice_build: &ModeChoiceBuild,
) -> Result<AssignmentKernelOutputs, String> {
    let init_queue_map = state_in.map(|st| &st.queue);
    let init_ttn_map = state_in.map(|st| &st.time_to_next_departure_s);
    let zone_count = s.world.zones.len();
    let mut active_latent_by_origin: Vec<Vec<usize>> = vec![Vec::new(); zone_count];
    for (idx, row) in mode_choice_build.rows.iter().enumerate() {
        if row.transit_captured > 0.0 {
            active_latent_by_origin[row.latent.origin_idx].push(idx);
        }
    }
    let mut service_index: HashMap<String, usize> = HashMap::new();
    for (i, svc) in s.world.services.iter().enumerate() {
        service_index.insert(svc.id.clone(), i);
    }
    let stop_by_id = s
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.as_str(), stop))
        .collect::<HashMap<_, _>>();

    // Assignment store: passengers per physical link
    let max_iters = settings.msa_max_iters;
    let mut link_passengers = vec![0.0_f64; s.world.links.len()];
    let mut prev_link_passengers = vec![0.0_f64; s.world.links.len()];

    // Milestone 2: capacity + queueing on boarding edges (service/stop)
    let mut prev_extra_wait: HashMap<(String, String), f64> = HashMap::new();
    let mut final_board_loads: Vec<BoardLoad> = Vec::new();
    let mut final_alight_frac: HashMap<(String, String), f64> = HashMap::new();

    let mut iters_run: usize = 0;
    let mut last_max_rel_change: f64 = 0.0;

    // KPI accumulators weighted by trips
    // KPI accumulators
    let mut total_trips_attempted = 0.0_f64;
    let mut total_trips_served = 0.0_f64;

    let mut sum_gc = 0.0_f64;
    let mut sum_walk = 0.0_f64;
    let mut sum_wait = 0.0_f64;
    let mut sum_ivt = 0.0_f64;

    let mut sum_transfer_time = 0.0_f64;
    let mut sum_transfer_pen = 0.0_f64;
    let mut sum_transfers = 0.0_f64;

    let mut sum_boardings = 0.0_f64;
    let mut sum_fare_revenue_base = 0.0_f64;

    #[derive(Debug, Clone)]
    struct PathRecord {
        flow: f64,
        link_indices: Vec<usize>,
        board_events: Vec<(String, String)>,
        alight_events: Vec<(String, String)>,
    }

    for iter in 1..=max_iters {
        // Copy previous iteration flows
        prev_link_passengers.clone_from(&link_passengers);

        // Reset flows for this iteration
        let mut attempted_boardings: HashMap<(String, String), f64> = HashMap::new();
        let mut attempted_alightings: HashMap<(String, String), f64> = HashMap::new();
        let mut path_records: Vec<PathRecord> = Vec::new();

        // Build graph using current crowding-adjusted ride costs
        let graph = build_graph_with_costs(
            s,
            &stop_index,
            &zone_index,
            &prev_link_passengers,
            &prev_extra_wait,
        )?;

        for oi in 0..zone_count {
            let origin_node = graph.svc_index.zone_nodes_start + oi;
            for od_idx in &active_latent_by_origin[oi] {
                let od_row = &mode_choice_build.rows[*od_idx];
                let od = &od_row.latent;
                if od_row.transit_captured <= 0.0 {
                    continue;
                }

                let dest_node = graph.svc_index.zone_nodes_start + zone_count + od.destination_idx;
                let paths = dedupe_paths(k_shortest_paths(
                    &graph,
                    origin_node,
                    dest_node,
                    settings.k_paths,
                ));
                if paths.is_empty() {
                    continue;
                }

                let mut paths = apply_fare_to_paths(paths, &s.params);
                let shares = logit_shares(&paths, settings.route_choice_theta);
                let expected_fare = paths
                    .iter()
                    .zip(shares.iter())
                    .map(|(p, sh)| p.stats.fare_base.max(0.0) * sh.max(0.0))
                    .sum::<f64>();
                let elasticity_mult = fare_elasticity_multiplier(&s.params, expected_fare);
                let od_effective = od_row.transit_captured * elasticity_mult;

                for (p, sh) in paths.iter_mut().zip(shares.iter()) {
                    let flow = od_effective * sh;
                    if flow <= 0.0 {
                        continue;
                    }

                    for (svc_id, stop_id) in &p.board_events {
                        *attempted_boardings
                            .entry((svc_id.clone(), stop_id.clone()))
                            .or_insert(0.0) += flow;
                    }
                    for (svc_id, stop_id) in &p.alight_events {
                        *attempted_alightings
                            .entry((svc_id.clone(), stop_id.clone()))
                            .or_insert(0.0) += flow;
                    }

                    path_records.push(PathRecord {
                        flow,
                        link_indices: p.link_indices.clone(),
                        board_events: p.board_events.clone(),
                        alight_events: p.alight_events.clone(),
                    });
                }
            }
        }
        // Resolve service occupancy + station throughput/queue constraints.
        if s.params.capacity_enabled {
            let mut new_extra_wait: HashMap<(String, String), f64> = HashMap::new();
            let mut board_loads_iter: Vec<BoardLoad> = Vec::new();
            let mut served_board_frac: HashMap<(String, String), f64> = HashMap::new();
            let mut served_alight_frac: HashMap<(String, String), f64> = HashMap::new();
            let period_h = s.meta.time_period_hours.max(0.0);
            let period_s = period_h * 3600.0;
            let retry_share = s.params.overflow_retry_share_clamped();

            let mut service_order: Vec<&str> = s
                .world
                .services
                .iter()
                .filter(|svc| service_is_active_for_sim(svc))
                .map(|svc| svc.id.as_str())
                .collect();
            service_order.sort_unstable();

            for svc_id in service_order {
                let si = *service_index
                    .get(svc_id)
                    .ok_or_else(|| format!("unknown service_id in occupancy pass: {svc_id}"))?;
                let svc = &s.world.services[si];

                if svc.stop_sequence.is_empty() {
                    continue;
                }

                let departures_in_period = if svc.headway_s > 0.0 {
                    (period_s / svc.headway_s).max(0.0)
                } else {
                    0.0
                };
                let vehicle_capacity = svc.vehicle_capacity.max(0.0);
                let mut onboard = 0.0_f64;

                for stop_id in &svc.stop_sequence {
                    let key = (svc.id.clone(), stop_id.clone());
                    let stop = stop_by_id.get(stop_id.as_str()).copied();
                    let station_caps = station_capacity_profile(stop, &s.params);

                    let station_capacity_boarding_pph = station_caps.boarding_pph.max(0.0)
                        * s.params.station_capacity_scale_boarding.max(0.0);
                    let station_capacity_alighting_pph = station_caps.alighting_pph.max(0.0)
                        * s.params.station_capacity_scale_alighting.max(0.0);
                    let station_queue_capacity_pax = station_caps.queue_pax.max(0.0)
                        * s.params.station_queue_capacity_scale.max(0.0);

                    let arrivals = attempted_boardings
                        .get(&key)
                        .copied()
                        .unwrap_or(0.0)
                        .max(0.0);
                    let alight_attempted = attempted_alightings
                        .get(&key)
                        .copied()
                        .unwrap_or(0.0)
                        .max(0.0);
                    let queue_start = init_queue_map
                        .and_then(|m| m.get(&key))
                        .copied()
                        .unwrap_or(0.0)
                        .max(0.0);
                    let init_ttn = init_ttn_map
                        .and_then(|m| m.get(&key))
                        .copied()
                        .unwrap_or_else(|| svc.headway_s.max(0.0));
                    let (time_to_next_end, departures_observed) =
                        advance_departure_phase_with_count(
                            init_ttn,
                            svc.headway_s.max(0.0),
                            period_s,
                        );
                    let departures_window = departures_observed as f64;
                    let capacity_in_period = departures_window * vehicle_capacity;
                    let station_window_h = if svc.headway_s > 0.0 {
                        (departures_window * svc.headway_s) / 3600.0
                    } else {
                        period_h
                    };
                    let station_board_cap_period = station_capacity_boarding_pph * station_window_h;
                    let station_alight_cap_period =
                        station_capacity_alighting_pph * station_window_h;

                    let alight_potential = alight_attempted.min(onboard).max(0.0);
                    let alightings_served =
                        alight_potential.min(station_alight_cap_period).max(0.0);
                    onboard = (onboard - alightings_served).max(0.0);

                    let total_boarding_demand = (queue_start + arrivals).max(0.0);
                    let vehicle_residual = (capacity_in_period - onboard).max(0.0);
                    let served_total = total_boarding_demand
                        .min(vehicle_residual)
                        .min(station_board_cap_period)
                        .max(0.0);
                    let served_from_queue = served_total.min(queue_start).max(0.0);
                    let served_from_arrivals =
                        (served_total - served_from_queue).max(0.0).min(arrivals);
                    onboard += served_total;

                    let queue_after_service = (total_boarding_demand - served_total).max(0.0);
                    let queue_capped = queue_after_service.min(station_queue_capacity_pax);
                    let overflow = (queue_after_service - queue_capped).max(0.0);
                    let retry_target = overflow * retry_share;
                    let retry_room = (station_queue_capacity_pax - queue_capped).max(0.0);
                    let retry_admitted = retry_target.min(retry_room);
                    let queue_end = queue_capped + retry_admitted;
                    let overflow_dropped = (overflow - retry_admitted).max(0.0);
                    let denied_boardings = (arrivals - served_from_arrivals).max(0.0);

                    let extra_wait_s = if vehicle_capacity > 0.0 && svc.headway_s > 0.0 {
                        (svc.headway_s * (queue_end / vehicle_capacity))
                            .min(s.params.queue_max_extra_wait_s.max(0.0))
                    } else {
                        0.0
                    };
                    if extra_wait_s > 0.0 {
                        new_extra_wait.insert(key.clone(), extra_wait_s);
                    }

                    let board_frac = if arrivals > 0.0 {
                        (served_from_arrivals / arrivals).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let alight_frac = if alight_attempted > 0.0 {
                        (alightings_served / alight_attempted).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    served_board_frac.insert(key.clone(), board_frac);
                    served_alight_frac.insert(key.clone(), alight_frac);

                    board_loads_iter.push(BoardLoad {
                        service_id: svc.id.clone(),
                        stop_id: stop_id.clone(),
                        arrivals,
                        served_from_arrivals,
                        served_from_queue,
                        denied_boardings,
                        queue_start,
                        queue_end,
                        headway_s: svc.headway_s,
                        vehicle_capacity,
                        departures_in_period,
                        departures_observed,
                        capacity_in_period,
                        extra_wait_s,
                        time_bins: vec![super::types::BoardingTimeBin {
                            bin_index: 0,
                            arrivals,
                            served: served_total,
                            queue_end,
                            departures: departures_observed,
                            capacity: capacity_in_period,
                        }],
                        time_to_next_departure_s_end: time_to_next_end,
                        alightings_served,
                        station_capacity_boarding_pph,
                        station_capacity_alighting_pph,
                        station_queue_capacity_pax,
                        overflow_dropped,
                    });
                }
            }

            link_passengers = vec![0.0_f64; s.world.links.len()];
            for record in &path_records {
                let mut mult = 1.0_f64;
                for b in &record.board_events {
                    if let Some(f) = served_board_frac.get(b) {
                        mult = mult.min(*f);
                    }
                }
                for a in &record.alight_events {
                    if let Some(f) = served_alight_frac.get(a) {
                        mult = mult.min(*f);
                    }
                }
                let eff_flow = record.flow * mult;
                if eff_flow <= 0.0 {
                    continue;
                }
                for &li in &record.link_indices {
                    link_passengers[li] += eff_flow;
                }
            }

            final_board_loads = board_loads_iter;
            final_alight_frac = served_alight_frac;
            prev_extra_wait = new_extra_wait;
        } else {
            link_passengers = vec![0.0_f64; s.world.links.len()];
            for record in &path_records {
                if record.flow <= 0.0 {
                    continue;
                }
                for &li in &record.link_indices {
                    link_passengers[li] += record.flow;
                }
            }
            final_board_loads.clear();
            final_alight_frac.clear();
            prev_extra_wait.clear();
        }

        // MSA blending
        let lambda = 1.0 / iter as f64;
        for i in 0..link_passengers.len() {
            link_passengers[i] =
                lambda * link_passengers[i] + (1.0 - lambda) * prev_link_passengers[i];
        }

        iters_run = iter;

        // Convergence: max relative change in link flows
        let mut max_rel = 0.0_f64;
        for i in 0..link_passengers.len() {
            let denom = prev_link_passengers[i].abs().max(1.0); // avoid div-by-zero
            let rel = (link_passengers[i] - prev_link_passengers[i]).abs() / denom;
            if rel > max_rel {
                max_rel = rel;
            }
        }
        last_max_rel_change = max_rel;

        // Stop early if stable
        if max_rel < settings.convergence_rel {
            break;
        }
    }

    // --- KPI pass using final equilibrium-ish flows ---
    let final_graph = build_graph_with_costs(
        s,
        &stop_index,
        &zone_index,
        &link_passengers,
        &prev_extra_wait,
    )?;

    let mut sample_paths: Vec<SampleOdPaths> = Vec::new();
    let sample_limit: usize = 6;

    let mut final_board_frac: HashMap<(String, String), f64> = HashMap::new();
    if s.params.capacity_enabled {
        for bl in &final_board_loads {
            let denom = bl.arrivals.max(0.0);
            let frac = if denom > 0.0 {
                (bl.served_from_arrivals / denom).clamp(0.0, 1.0)
            } else {
                1.0
            };
            final_board_frac.insert((bl.service_id.clone(), bl.stop_id.clone()), frac);
        }
    }

    #[derive(Debug, Clone, Default)]
    struct CohortAccumulator {
        attempted: f64,
        boarded: f64,
        alighted: f64,
        queue_end: f64,
    }
    let mut cohort_by_leg = HashMap::<(String, String, String), CohortAccumulator>::new();
    let mut departures_observed_by_stop = HashMap::<(String, String), usize>::new();
    for bl in &final_board_loads {
        let key = (bl.service_id.clone(), bl.stop_id.clone());
        let entry = departures_observed_by_stop.entry(key).or_insert(0usize);
        *entry = entry.saturating_add(bl.departures_observed);
    }
    let mut fare_flow = FareFlowSummary::default();
    let mut assigned_od_flows: Vec<AssignedOdFlow> = Vec::new();
    let mut arrivals_completed_by_stop: HashMap<String, f64> = HashMap::new();

    for od_row in &mode_choice_build.rows {
        let od = &od_row.latent;
        let origin_node = final_graph.svc_index.zone_nodes_start + od.origin_idx;
        let dest_node = final_graph.svc_index.zone_nodes_start + zone_count + od.destination_idx;
        let latent_total = od.latent_passengers.max(0.0);
        if latent_total <= 0.0 {
            continue;
        }
        let transit_latent = od_row.transit_captured.max(0.0);

        let raw_paths = k_shortest_paths(&final_graph, origin_node, dest_node, settings.k_paths);
        let k_paths_raw = raw_paths.len();
        let mut paths = apply_fare_to_paths(dedupe_paths(raw_paths), &s.params);
        let k_paths_after_dedupe = paths.len();

        let mut chosen_paths: Vec<AssignedPathSummary> = Vec::new();
        let mut assigned_total = 0.0_f64;

        if paths.is_empty() || transit_latent <= 0.0 {
            assigned_od_flows.push(AssignedOdFlow {
                origin_zone_id: od.origin_zone_id.clone(),
                destination_zone_id: od.destination_zone_id.clone(),
                purpose: od.purpose,
                time_slice: od.time_slice,
                service_day_type: Some(od.service_day_type),
                seasonal_profile: Some(od.seasonal_profile),
                active_event_ids: od.active_event_ids.clone(),
                assigned_passengers: 0.0,
                unserved_passengers: latent_total,
                suppressed_passengers: od_row.suppressed_or_no_trip.max(0.0),
                chosen_paths,
            });
            continue;
        }

        let shares = logit_shares(&paths, settings.route_choice_theta);
        let expected_fare = paths
            .iter()
            .zip(shares.iter())
            .map(|(p, sh)| p.stats.fare_base.max(0.0) * sh.max(0.0))
            .sum::<f64>();
        let elasticity_mult = fare_elasticity_multiplier(&s.params, expected_fare);
        let attempted_network = transit_latent * elasticity_mult;
        let suppressed =
            (transit_latent - attempted_network).max(0.0) + od_row.suppressed_or_no_trip.max(0.0);
        total_trips_attempted += attempted_network;

        if sample_paths.len() < sample_limit {
            let mut opts: Vec<SamplePathOption> = Vec::new();
            for (p, sh) in paths.iter().zip(shares.iter()) {
                let link_ids = p
                    .link_indices
                    .iter()
                    .map(|&li| s.world.links[li].id.clone())
                    .collect::<Vec<_>>();

                opts.push(SamplePathOption {
                    share: *sh,
                    gc_s: p.stats.gc_s,
                    walk_s: p.stats.walk_s,
                    wait_s: p.stats.wait_s,
                    ivt_s: p.stats.ivt_s,
                    transfer_count: p.stats.transfer_count,
                    boardings: p.stats.boardings,
                    link_ids,
                });
            }
            sample_paths.push(SampleOdPaths {
                origin_zone: od.origin_zone_id.clone(),
                dest_zone: od.destination_zone_id.clone(),
                trips: attempted_network,
                k_paths_raw,
                k_paths_after_dedupe,
                paths: opts,
            });
        }

        for (p, sh) in paths.iter_mut().zip(shares.iter()) {
            let attempted = attempted_network * sh.max(0.0);
            if attempted <= 0.0 {
                continue;
            }
            for (board, alight) in p.board_events.iter().zip(p.alight_events.iter()) {
                let key = (board.0.clone(), board.1.clone(), alight.1.clone());
                let entry = cohort_by_leg.entry(key).or_default();
                entry.attempted += attempted;
            }

            // Apply capacity-thinning for served trips.
            let mut served = attempted;
            if s.params.capacity_enabled {
                let mut mult = 1.0_f64;
                for b in &p.board_events {
                    if let Some(f) = final_board_frac.get(b) {
                        mult = mult.min(*f);
                    }
                }
                for a in &p.alight_events {
                    if let Some(f) = final_alight_frac.get(a) {
                        mult = mult.min(*f);
                    }
                }
                served *= mult;
            }
            served = served.max(0.0).min(attempted);

            chosen_paths.push(AssignedPathSummary {
                share: *sh,
                attempted_passengers: attempted,
                assigned_passengers: served,
                link_ids: p
                    .link_indices
                    .iter()
                    .map(|&li| s.world.links[li].id.clone())
                    .collect(),
            });

            if served <= 0.0 {
                continue;
            }

            for (board, alight) in p.board_events.iter().zip(p.alight_events.iter()) {
                let key = (board.0.clone(), board.1.clone(), alight.1.clone());
                let entry = cohort_by_leg.entry(key).or_default();
                entry.boarded += served;
                entry.alighted += served;
            }
            if let Some((_svc_id, stop_id)) = p.alight_events.last() {
                *arrivals_completed_by_stop
                    .entry(stop_id.clone())
                    .or_insert(0.0) += served;
            }

            assigned_total += served;
            total_trips_served += served;
            sum_gc += served * p.stats.gc_s;
            sum_walk += served * p.stats.walk_s;
            sum_wait += served * p.stats.wait_s;
            sum_ivt += served * p.stats.ivt_s;

            sum_transfer_time += served * p.stats.transfer_time_s;
            sum_transfer_pen += served * p.stats.transfer_penalty_s;
            sum_transfers += served * p.stats.transfer_count;

            sum_boardings += served * p.stats.boardings;
            if s.params.fare_enabled {
                let fare_base = p.stats.fare_base.max(0.0);
                let liability = served * fare_base;
                sum_fare_revenue_base += liability;
                fare_flow.liability_accrued_base += liability;
                fare_flow.liability_accrued_pax += served;
                if let Some((svc_id, stop_id)) = p.alight_events.last() {
                    if departures_observed_by_stop
                        .get(&(svc_id.clone(), stop_id.clone()))
                        .copied()
                        .unwrap_or(0)
                        > 0
                    {
                        fare_flow.completed_journeys_pax += served;
                        fare_flow.recognized_revenue_base += liability;
                    }
                }
            }
        }

        assigned_od_flows.push(AssignedOdFlow {
            origin_zone_id: od.origin_zone_id.clone(),
            destination_zone_id: od.destination_zone_id.clone(),
            purpose: od.purpose,
            time_slice: od.time_slice,
            service_day_type: Some(od.service_day_type),
            seasonal_profile: Some(od.seasonal_profile),
            active_event_ids: od.active_event_ids.clone(),
            assigned_passengers: assigned_total,
            unserved_passengers: (latent_total - assigned_total).max(0.0),
            suppressed_passengers: suppressed,
            chosen_paths,
        });
    }

    let tph = s.meta.time_period_hours;

    // --- Derived transit link capacities (service supply -> link capacity) ---
    // For any link that is used by at least one service, we derive capacity_per_hour from:
    // sum_over_services_on_link( departures_per_hour * vehicle_capacity )
    let mut derived_cap_per_hour: HashMap<(String, String, String), f64> = HashMap::new();

    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) {
            continue;
        }
        let headway = svc.headway_s;
        let veh_cap = svc.vehicle_capacity;

        if headway <= 0.0 || veh_cap <= 0.0 {
            continue;
        }

        let departures_per_hour = 3600.0 / headway;
        let seats_per_hour = departures_per_hour * veh_cap;

        // Each consecutive stop pair implies the service uses the link (from -> to) in this direction.
        for w in svc.stop_sequence.windows(2) {
            let from_stop = &w[0];
            let to_stop = &w[1];

            let key = (from_stop.clone(), to_stop.clone(), svc.mode.clone());
            *derived_cap_per_hour.entry(key).or_insert(0.0) += seats_per_hour;
        }
    }

    let link_loads = s
        .world
        .links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let pax = link_passengers[i];

            let key = (l.from_stop.clone(), l.to_stop.clone(), l.mode.clone());

            // If this link is used by services, we override capacity_per_hour with derived supply.
            // Otherwise we fall back to whatever the scenario provided (e.g., roads, walk links, etc.)
            // If link is used by at least one service, use derived supply.
            // If link mode matches any service mode but is not used by a service in this direction,
            // capacity should be zero (no phantom reverse capacity).
            let cap_ph = if derived_cap_per_hour.contains_key(&key) {
                Some(*derived_cap_per_hour.get(&key).unwrap())
            } else if s
                .world
                .services
                .iter()
                .any(|svc| service_is_active_for_sim(svc) && svc.mode == l.mode)
            {
                Some(0.0)
            } else {
                l.capacity_per_hour
            };

            let cap_period = cap_ph.unwrap_or(0.0) * tph;
            let ratio = if cap_period > 0.0 {
                pax / cap_period
            } else {
                0.0
            };

            let pen_s = crowding_multiplier(pax, cap_ph, tph);

            LinkLoad {
                link_id: l.id.clone(),
                from_stop: l.from_stop.clone(),
                to_stop: l.to_stop.clone(),
                mode: l.mode.clone(),

                passengers: pax,

                capacity_per_hour: cap_ph,
                capacity_in_period: cap_period,
                load_to_capacity: ratio,
                crowding_penalty_s: pen_s,
            }
        })
        .collect::<Vec<_>>();
    let total_boardings_attempted: f64 = final_board_loads.iter().map(|b| b.arrivals).sum();

    let total_boardings_served: f64 = final_board_loads
        .iter()
        .map(|b| b.served_from_arrivals + b.served_from_queue)
        .sum();

    let total_boardings_denied: f64 = final_board_loads.iter().map(|b| b.denied_boardings).sum();
    let total_overflow_dropped: f64 = final_board_loads.iter().map(|b| b.overflow_dropped).sum();

    let share_boardings_served = if total_boardings_attempted > 0.0 {
        total_boardings_served / total_boardings_attempted
    } else {
        1.0
    };
    let share_demand_overflow_dropped = if total_boardings_attempted > 0.0 {
        total_overflow_dropped / total_boardings_attempted
    } else {
        0.0
    };

    let share_trips_served = if total_trips_attempted > 0.0 {
        total_trips_served / total_trips_attempted
    } else {
        1.0
    };

    let mut stop_flow_map: HashMap<String, StopFlow> = HashMap::new();
    for bl in &final_board_loads {
        let entry = stop_flow_map
            .entry(bl.stop_id.clone())
            .or_insert_with(|| StopFlow {
                stop_id: bl.stop_id.clone(),
                boardings_attempted: 0.0,
                boardings_served: 0.0,
                alightings_attempted: 0.0,
                alightings_served: 0.0,
                queue_start: 0.0,
                queue_end: 0.0,
                overflow_dropped: 0.0,
            });
        entry.boardings_attempted += bl.arrivals.max(0.0);
        entry.boardings_served += (bl.served_from_arrivals + bl.served_from_queue).max(0.0);
        let alight_attempted = final_alight_frac
            .get(&(bl.service_id.clone(), bl.stop_id.clone()))
            .copied()
            .filter(|v| *v > 0.0)
            .map(|frac| bl.alightings_served / frac.max(1e-9))
            .unwrap_or(bl.alightings_served);
        entry.alightings_attempted += alight_attempted.max(0.0);
        entry.alightings_served += bl.alightings_served.max(0.0);
        entry.queue_start += bl.queue_start.max(0.0);
        entry.queue_end += bl.queue_end.max(0.0);
        entry.overflow_dropped += bl.overflow_dropped.max(0.0);
    }
    let mut stop_flows = stop_flow_map.into_values().collect::<Vec<_>>();
    stop_flows.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut queue_end_by_board = HashMap::<(String, String), f64>::new();
    for bl in &final_board_loads {
        *queue_end_by_board
            .entry((bl.service_id.clone(), bl.stop_id.clone()))
            .or_insert(0.0) += bl.queue_end.max(0.0);
    }
    let mut residual_by_board = HashMap::<(String, String), f64>::new();
    for ((service_id, board_stop_id, _dest_stop_id), cohort) in &cohort_by_leg {
        let residual = (cohort.attempted - cohort.boarded).max(0.0);
        if residual <= 0.0 {
            continue;
        }
        *residual_by_board
            .entry((service_id.clone(), board_stop_id.clone()))
            .or_insert(0.0) += residual;
    }
    for ((service_id, board_stop_id, _dest_stop_id), cohort) in cohort_by_leg.iter_mut() {
        let residual = (cohort.attempted - cohort.boarded).max(0.0);
        if residual <= 0.0 {
            cohort.queue_end = 0.0;
            continue;
        }
        let board_key = (service_id.clone(), board_stop_id.clone());
        let total_residual = residual_by_board.get(&board_key).copied().unwrap_or(0.0);
        let queue_total = queue_end_by_board.get(&board_key).copied().unwrap_or(0.0);
        if queue_total <= 0.0 || total_residual <= 0.0 {
            cohort.queue_end = 0.0;
        } else {
            cohort.queue_end = queue_total * (residual / total_residual);
        }
    }
    let mut passenger_cohorts = cohort_by_leg
        .into_iter()
        .filter_map(
            |((service_id, board_stop_id, destination_stop_id), cohort)| {
                let attempted = cohort.attempted.max(0.0);
                let boarded = cohort.boarded.max(0.0);
                let alighted = cohort.alighted.max(0.0);
                let queue_end = cohort.queue_end.max(0.0);
                if attempted <= 0.0 && boarded <= 0.0 && alighted <= 0.0 && queue_end <= 0.0 {
                    return None;
                }
                Some(PassengerCohortFlow {
                    service_id,
                    board_stop_id,
                    destination_stop_id,
                    attempted_pax: attempted,
                    boarded_pax: boarded,
                    alighted_pax: alighted,
                    queue_end_pax: queue_end,
                })
            },
        )
        .collect::<Vec<_>>();
    passenger_cohorts.sort_by(|a, b| {
        a.service_id
            .cmp(&b.service_id)
            .then_with(|| a.board_stop_id.cmp(&b.board_stop_id))
            .then_with(|| a.destination_stop_id.cmp(&b.destination_stop_id))
    });
    if !s.params.fare_enabled {
        fare_flow = FareFlowSummary::default();
    } else {
        fare_flow.liability_accrued_base = fare_flow.liability_accrued_base.max(0.0);
        fare_flow.liability_accrued_pax = fare_flow.liability_accrued_pax.max(0.0);
        fare_flow.completed_journeys_pax = fare_flow.completed_journeys_pax.max(0.0);
        fare_flow.recognized_revenue_base = fare_flow.recognized_revenue_base.max(0.0);
    }

    #[derive(Default)]
    struct StopStateBuilder {
        waiting_by_destination: HashMap<String, f64>,
        total_waiting: f64,
        boarded_this_step: f64,
        alighted_this_step: f64,
        denied_this_step: f64,
        arrived_this_step: f64,
        departed_this_step: f64,
    }

    let mut stop_state_builders: HashMap<String, StopStateBuilder> = HashMap::new();
    for bl in &final_board_loads {
        let entry = stop_state_builders.entry(bl.stop_id.clone()).or_default();
        let boarded = (bl.served_from_arrivals + bl.served_from_queue).max(0.0);
        entry.total_waiting += bl.queue_end.max(0.0);
        entry.boarded_this_step += boarded;
        entry.departed_this_step += boarded;
        entry.alighted_this_step += bl.alightings_served.max(0.0);
        entry.denied_this_step += (bl.denied_boardings + bl.overflow_dropped).max(0.0);
    }
    for cohort in &passenger_cohorts {
        let queue = cohort.queue_end_pax.max(0.0);
        if queue <= 0.0 {
            continue;
        }
        let entry = stop_state_builders
            .entry(cohort.board_stop_id.clone())
            .or_default();
        *entry
            .waiting_by_destination
            .entry(cohort.destination_stop_id.clone())
            .or_insert(0.0) += queue;
    }
    for (stop_id, arrivals) in &arrivals_completed_by_stop {
        let entry = stop_state_builders.entry(stop_id.clone()).or_default();
        entry.arrived_this_step += arrivals.max(0.0);
    }
    let mut stop_flow_states = stop_state_builders
        .into_iter()
        .map(|(stop_id, state)| {
            let mut waiting_by_destination = state
                .waiting_by_destination
                .into_iter()
                .map(
                    |(destination_stop_id, waiting_passengers)| WaitingByDestination {
                        destination_stop_id,
                        waiting_passengers: waiting_passengers.max(0.0),
                    },
                )
                .collect::<Vec<_>>();
            waiting_by_destination
                .sort_by(|a, b| a.destination_stop_id.cmp(&b.destination_stop_id));
            StopFlowState {
                stop_id,
                waiting_by_destination,
                total_waiting: state.total_waiting.max(0.0),
                boarded_this_step: state.boarded_this_step.max(0.0),
                alighted_this_step: state.alighted_this_step.max(0.0),
                denied_this_step: state.denied_this_step.max(0.0),
                arrived_this_step: state.arrived_this_step.max(0.0),
                departed_this_step: state.departed_this_step.max(0.0),
            }
        })
        .collect::<Vec<_>>();
    stop_flow_states.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let board_load_by_service_stop = final_board_loads
        .iter()
        .map(|bl| ((bl.service_id.as_str(), bl.stop_id.as_str()), bl))
        .collect::<HashMap<_, _>>();
    let mut vehicle_load_states: Vec<VehicleLoadState> = Vec::new();
    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) || svc.stop_sequence.is_empty() {
            continue;
        }
        let mut current_load = 0.0_f64;
        let mut max_load_seen = 0.0_f64;
        for stop_id in &svc.stop_sequence {
            let bl = board_load_by_service_stop.get(&(svc.id.as_str(), stop_id.as_str()));
            let boardings = bl
                .map(|x| x.served_from_arrivals + x.served_from_queue)
                .unwrap_or(0.0)
                .max(0.0);
            let alightings = bl.map(|x| x.alightings_served).unwrap_or(0.0).max(0.0);
            let load_after_alight = (current_load - alightings).max(0.0);
            let load_after_stop = (load_after_alight + boardings).max(0.0);
            current_load = load_after_stop;
            if current_load > max_load_seen {
                max_load_seen = current_load;
            }
            let capacity = bl
                .map(|x| (x.vehicle_capacity.max(0.0)) * (x.departures_observed as f64).max(1.0))
                .unwrap_or_else(|| svc.vehicle_capacity.max(0.0));
            let crowding_ratio = if capacity > 0.0 {
                load_after_stop / capacity
            } else {
                0.0
            };
            vehicle_load_states.push(VehicleLoadState {
                vehicle_id: format!("{}:period", svc.id),
                run_id: format!("{}:period", svc.id),
                service_id: svc.id.clone(),
                stop_id: stop_id.clone(),
                current_load: current_load.max(0.0),
                boardings_this_stop: boardings,
                alightings_this_stop: alightings,
                load_after_stop: load_after_stop.max(0.0),
                max_load_seen: max_load_seen.max(0.0),
                capacity: capacity.max(0.0),
                crowding_ratio: crowding_ratio.max(0.0),
            });
        }
    }

    let mut service_load_layer_light = Vec::<ServiceLoadLayerData>::new();
    for svc in &s.world.services {
        let mut passengers = 0.0_f64;
        let mut peak_load = 0.0_f64;
        let mut peak_load_stop_id: Option<String> = None;
        for state in vehicle_load_states
            .iter()
            .filter(|x| x.service_id == svc.id)
        {
            passengers += state.boardings_this_stop.max(0.0);
            if state.load_after_stop > peak_load {
                peak_load = state.load_after_stop;
                peak_load_stop_id = Some(state.stop_id.clone());
            }
        }
        service_load_layer_light.push(ServiceLoadLayerData {
            service_id: svc.id.clone(),
            line_id: svc.line_id.clone(),
            passengers: passengers.max(0.0),
            peak_load: peak_load.max(0.0),
            peak_load_stop_id,
        });
    }
    service_load_layer_light.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    Ok(AssignmentKernelOutputs {
        link_loads,
        final_board_loads,
        stop_flows,
        passenger_cohorts,
        fare_flow,
        assigned_od_flows,
        stop_flow_states,
        vehicle_load_states,
        service_load_layer_light,
        sample_paths,
        iters_run,
        last_max_rel_change,
        total_trips_attempted,
        total_trips_served,
        sum_gc,
        sum_walk,
        sum_wait,
        sum_ivt,
        sum_transfer_time,
        sum_transfer_pen,
        sum_transfers,
        sum_boardings,
        sum_fare_revenue_base,
        total_boardings_attempted,
        total_boardings_served,
        total_boardings_denied,
        total_overflow_dropped,
        share_boardings_served,
        share_demand_overflow_dropped,
        share_trips_served,
    })
}

#[derive(Clone, Copy)]
struct StationCapacityProfile {
    boarding_pph: f64,
    alighting_pph: f64,
    queue_pax: f64,
}

fn station_capacity_profile(
    stop: Option<&Stop>,
    _params: &crate::model::Params,
) -> StationCapacityProfile {
    let stop_type = stop
        .and_then(|s| s.stop_type.as_deref())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let mut profile = match stop_type.as_str() {
        "bus_stop" | "bus_bay" => StationCapacityProfile {
            boarding_pph: 2500.0,
            alighting_pph: 2800.0,
            queue_pax: 350.0,
        },
        "tram_stop" => StationCapacityProfile {
            boarding_pph: 6000.0,
            alighting_pph: 6500.0,
            queue_pax: 700.0,
        },
        "metro_station" => StationCapacityProfile {
            boarding_pph: 20_000.0,
            alighting_pph: 22_000.0,
            queue_pax: 3500.0,
        },
        "rail_station" | "station" => StationCapacityProfile {
            boarding_pph: 15_000.0,
            alighting_pph: 17_000.0,
            queue_pax: 2800.0,
        },
        "ferry_terminal" => StationCapacityProfile {
            boarding_pph: 3500.0,
            alighting_pph: 4000.0,
            queue_pax: 500.0,
        },
        _ => StationCapacityProfile {
            boarding_pph: 5000.0,
            alighting_pph: 5000.0,
            queue_pax: 800.0,
        },
    };

    if let Some(s) = stop {
        if let Some(v) = s.station_boarding_capacity_pph {
            if v.is_finite() && v >= 0.0 {
                profile.boarding_pph = v;
            }
        }
        if let Some(v) = s.station_alighting_capacity_pph {
            if v.is_finite() && v >= 0.0 {
                profile.alighting_pph = v;
            }
        }
        if let Some(v) = s.station_queue_capacity_pax {
            if v.is_finite() && v >= 0.0 {
                profile.queue_pax = v;
            }
        }
    }

    profile
}

fn advance_departure_phase_with_count(
    initial_time_to_next_s: f64,
    headway_s: f64,
    elapsed_s: f64,
) -> (f64, usize) {
    if headway_s <= 0.0 {
        return (0.0, 0);
    }

    let mut time_to_next = if initial_time_to_next_s.is_finite() {
        let init = initial_time_to_next_s;
        if init <= 0.0 {
            0.0
        } else {
            let mut x = init % headway_s;
            if x < 0.0 {
                x += headway_s;
            }
            if x <= 1e-9 {
                headway_s
            } else {
                x
            }
        }
    } else {
        headway_s
    };

    let mut remaining = elapsed_s.max(0.0);
    let mut departures_observed = 0usize;
    while remaining > 0.0 {
        if time_to_next <= remaining {
            remaining -= time_to_next.max(0.0);
            time_to_next = headway_s;
            departures_observed += 1;
        } else {
            time_to_next -= remaining;
            remaining = 0.0;
        }
        if time_to_next <= 1e-9 {
            time_to_next = headway_s;
        }
    }

    (time_to_next.max(0.0), departures_observed)
}
