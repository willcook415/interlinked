use std::collections::{BTreeMap, HashMap};

use super::choice::logit_shares;
use super::graph::BuiltPath;
use super::modes::{
    canonical_mode_from_tokens, travel_mode_family_from_tokens, CanonicalTransitMode,
};
use super::routing::crowding_multiplier;
use super::routing::{
    build_graph, build_graph_with_costs, dedupe_paths, dijkstra, k_shortest_paths,
};
use super::stateful::SimState;
use super::types::{
    AssignedOdFlow, AssignedPathSummary, BoardLoad, BuildPreviewMetrics, BuildPreviewType,
    CitywideModeShareSummary, CommercialStrengthClassification, CorridorClassification,
    CorridorDesireLineData, CorridorFinancialMetrics, CorridorModeShareMetrics,
    CorridorPlanningMetrics, CorridorReference, DemandDiagnostics, DemandTimeSliceLabel,
    Diagnostics, EconomicDiagnostics, EconomicRankingEntry, EventDemandModifier, FareFlowSummary,
    FareModel, FinancialPerformanceMetrics, FlowConsistencyCheck, Kpis, LatentOdDemand,
    LineOrServicePlanningMetrics, LinkLoad, ModalDemandDiagnostics, ModalRankingEntry,
    ModeChoiceContext, ModeChoiceResult, ModeGeneralizedCostBreakdown, ModeGeneralizedCostByMode,
    ModeShareValue, NetworkFinancialSummary, OdPatternMetric, OnTimeStatus,
    OperationalIncidentType, OperationalRankingEntry, OperationsReliabilityConfig, OutputMeta,
    PassengerCohortFlow, PlanningDebugSummary, PlanningOverlayConfig, PurposeDemandValue,
    PurposeModeShareValue, PurposeScoreValue, PurposeTemporalDemandTotals, RecommendedServiceClass,
    SampleOdPaths, SamplePathOption, SeasonalProfile, ServiceDayModeShareSummary, ServiceDayType,
    ServiceFinancialMetrics, ServiceGapRankings, ServiceLoadLayerData, ServiceOperationState,
    ServiceReliabilityDiagnostics, ServiceRoleClassification, ServiceScoreEntry,
    ServiceTransitCaptureContext, ServiceVehicleLoadAggregate, SettlementClass, SimulationOutput,
    SimulationSettings, SocialNecessityClassification, SpecialAttractorType,
    StationFinancialContext, StationFlowAggregate, StationPlanningMetrics, StationScoreEntry,
    StationTransitCaptureContext, StopFlow, StopFlowReference, StopFlowState, StopOperationState,
    SyntheticEconomyConfig, TemporalCorridorPressurePoint, TemporalDemandDiagnostics,
    TemporalDemandSlice, TemporalPlanningSnapshot, TemporalRankingEntry, TemporalServiceGapPoint,
    TemporalServicePressurePoint, TemporalStationPressurePoint, TimeSliceDemandTotals,
    TimeSliceModeShareSummary, TransferOperationMetrics, TravelMode, TripPurpose, VehicleLoadState,
    WaitingByDestination, ZoneArchetype, ZoneDemandAttractionLayerData, ZoneDemandLayerData,
    ZoneDemandProductionLayerData, ZoneDemandProfile, ZoneEconomicGeographyLayerData,
    ZoneFlowReference, ZoneModeShareMetrics, ZonePlanningMetrics, ZoneScoreEntry,
    ZoneServiceGapLayerData,
};
use crate::model::{DemandCell, Scenario, Service, Stop, World, Zone};

mod phase3;

fn service_is_active_for_sim(svc: &Service) -> bool {
    if matches!(svc.service_enabled, Some(false)) {
        return false;
    }
    if let Some(tph) = svc.operating_tph {
        if !tph.is_finite() || tph <= 0.0 {
            return false;
        }
    }
    if !svc.headway_s.is_finite() || svc.headway_s <= 0.0 || svc.headway_s >= 86_399.0 {
        return false;
    }
    if let Some(units) = svc.stock_units_assigned {
        if units == 0 {
            return false;
        }
    }
    true
}

pub fn run_simulation(s: &Scenario) -> Result<SimulationOutput, String> {
    let settings = SimulationSettings::from_params(&s.params);
    run_simulation_with_settings(s, &settings, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalBundlePolicy {
    AutoFromStateInput,
    AlwaysInclude,
    NeverInclude,
}

pub fn run_simulation_with_settings_and_context(
    s: &Scenario,
    settings: &SimulationSettings,
    state_in: Option<&SimState>,
    temporal_context: Option<TemporalDemandSlice>,
) -> Result<SimulationOutput, String> {
    run_simulation_with_settings_and_context_with_policy(
        s,
        settings,
        state_in,
        temporal_context,
        TemporalBundlePolicy::AutoFromStateInput,
    )
}

pub fn run_simulation_with_settings_and_context_with_policy(
    s: &Scenario,
    settings: &SimulationSettings,
    state_in: Option<&SimState>,
    temporal_context: Option<TemporalDemandSlice>,
    temporal_bundle_policy: TemporalBundlePolicy,
) -> Result<SimulationOutput, String> {
    // Planning behavior must be explicit. Auto mode preserves legacy behavior while callers that
    // require deterministic planning parity can opt into AlwaysInclude.
    let include_temporal_bundle = match temporal_bundle_policy {
        TemporalBundlePolicy::AutoFromStateInput => state_in.is_none(),
        TemporalBundlePolicy::AlwaysInclude => true,
        TemporalBundlePolicy::NeverInclude => false,
    };
    run_simulation_internal(
        s,
        settings,
        state_in,
        temporal_context,
        include_temporal_bundle,
    )
}

pub fn run_simulation_with_settings(
    s: &Scenario,
    settings: &SimulationSettings,
    state_in: Option<&SimState>,
) -> Result<SimulationOutput, String> {
    run_simulation_with_settings_and_context(s, settings, state_in, None)
}

fn run_simulation_internal(
    s: &Scenario,
    settings: &SimulationSettings,
    state_in: Option<&SimState>,
    temporal_context: Option<TemporalDemandSlice>,
    include_temporal_bundle: bool,
) -> Result<SimulationOutput, String> {
    let s_eff = materialize_effective_zones(s);
    let s = &s_eff;
    validate(s)?;
    let init_queue_map = state_in.map(|st| &st.queue);
    let init_ttn_map = state_in.map(|st| &st.time_to_next_departure_s);
    let pending_od = state_in.map(|st| &st.pending_od_trips);
    let (stop_index, zone_index) = index_maps(&s.world);
    let graph = build_graph(s, &stop_index, &zone_index)?;
    let zone_count = s.world.zones.len();
    let sim_clock_s = state_in.map(|st| st.t_s).unwrap_or(12.0 * 3600.0);
    let coverage_mode = if include_temporal_bundle {
        LatentDemandCoverage::CanonicalSlicesForDay
    } else {
        LatentDemandCoverage::SingleContext
    };
    let mut latent_build =
        build_latent_demand_foundation(s, &graph, sim_clock_s, temporal_context, coverage_mode)?;

    // Inject any explicit OD events into the active slice. These are authoritative latent trips
    // introduced by the stateful stepping layer.
    if let Some(m) = pending_od {
        for ((oz, dz), trips) in m.iter() {
            if *trips <= 0.0 {
                continue;
            }
            let oi = match zone_index.get(oz) {
                Some(x) => *x,
                None => continue,
            };
            let dj = match zone_index.get(dz) {
                Some(x) => *x,
                None => continue,
            };

            let latent = trips.max(0.0);
            latent_build.all_latent.push(LatentOdDemand {
                origin_zone_id: oz.clone(),
                destination_zone_id: dz.clone(),
                purpose: TripPurpose::Essential,
                time_slice: latent_build.active_context.time_slice,
                service_day_type: Some(latent_build.active_context.service_day_type),
                seasonal_profile: Some(latent_build.active_context.seasonal_profile),
                active_event_ids: latent_build.active_context.active_event_ids.clone(),
                latent_passengers: latent,
            });
            latent_build.active_latent.push(ActiveLatentOd {
                origin_idx: oi,
                destination_idx: dj,
                origin_zone_id: oz.clone(),
                destination_zone_id: dz.clone(),
                purpose: TripPurpose::Essential,
                time_slice: latent_build.active_context.time_slice,
                service_day_type: latent_build.active_context.service_day_type,
                seasonal_profile: latent_build.active_context.seasonal_profile,
                active_event_ids: latent_build.active_context.active_event_ids.clone(),
                latent_passengers: latent,
            });
        }
    }

    let mode_choice_build = apply_mode_choice_capture(
        s,
        settings,
        &graph,
        &latent_build.economy_config,
        &latent_build.zone_profiles,
        &latent_build.active_context,
        &latent_build.active_latent,
    )?;

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

    let mean = |sum: f64| {
        if total_trips_served > 0.0 {
            sum / total_trips_served
        } else {
            0.0
        }
    };

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

    if settings.lightweight_outputs {
        let mut totals_by_slice: BTreeMap<DemandTimeSliceLabel, TimeSliceDemandTotals> =
            BTreeMap::new();
        for od in &latent_build.active_latent {
            let entry =
                totals_by_slice
                    .entry(od.time_slice)
                    .or_insert_with(|| TimeSliceDemandTotals {
                        time_slice: od.time_slice,
                        total_latent: 0.0,
                        total_realised: 0.0,
                        total_unserved: 0.0,
                    });
            entry.total_latent += od.latent_passengers.max(0.0);
        }
        for flow in &assigned_od_flows {
            let entry =
                totals_by_slice
                    .entry(flow.time_slice)
                    .or_insert_with(|| TimeSliceDemandTotals {
                        time_slice: flow.time_slice,
                        total_latent: 0.0,
                        total_realised: 0.0,
                        total_unserved: 0.0,
                    });
            entry.total_realised += flow.assigned_passengers.max(0.0);
            entry.total_unserved += flow.unserved_passengers.max(0.0);
        }
        let totals_by_time_slice = totals_by_slice.into_values().collect::<Vec<_>>();

        let mut boardings_alightings_by_station = stop_flow_states
            .iter()
            .map(|s| StationFlowAggregate {
                stop_id: s.stop_id.clone(),
                boarded: s.boarded_this_step.max(0.0),
                alighted: s.alighted_this_step.max(0.0),
                denied: s.denied_this_step.max(0.0),
                waiting: s.total_waiting.max(0.0),
            })
            .collect::<Vec<_>>();
        boardings_alightings_by_station.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

        let mut vehicle_loads_by_service = service_load_layer_light
            .iter()
            .map(|svc| ServiceVehicleLoadAggregate {
                service_id: svc.service_id.clone(),
                current_load: vehicle_load_states
                    .iter()
                    .rfind(|x| x.service_id == svc.service_id)
                    .map(|x| x.current_load.max(0.0))
                    .unwrap_or(0.0),
                max_load_seen: svc.peak_load.max(0.0),
                capacity: vehicle_load_states
                    .iter()
                    .filter(|x| x.service_id == svc.service_id)
                    .map(|x| x.capacity.max(0.0))
                    .fold(0.0_f64, f64::max),
            })
            .collect::<Vec<_>>();
        vehicle_loads_by_service.sort_by(|a, b| a.service_id.cmp(&b.service_id));

        let total_latent_active = latent_build
            .active_latent
            .iter()
            .map(|x| x.latent_passengers.max(0.0))
            .sum::<f64>();
        let total_realised = assigned_od_flows
            .iter()
            .map(|x| x.assigned_passengers.max(0.0))
            .sum::<f64>();
        let total_unserved = assigned_od_flows
            .iter()
            .map(|x| x.unserved_passengers.max(0.0))
            .sum::<f64>();
        let total_waiting_network = stop_flow_states
            .iter()
            .map(|x| x.total_waiting.max(0.0))
            .sum::<f64>();

        let vehicle_boarded_total = vehicle_load_states
            .iter()
            .map(|x| x.boardings_this_stop.max(0.0))
            .sum::<f64>();
        let station_boarded_total = stop_flow_states
            .iter()
            .map(|x| x.boarded_this_step.max(0.0))
            .sum::<f64>();
        let vehicle_alighted_total = vehicle_load_states
            .iter()
            .map(|x| x.alightings_this_stop.max(0.0))
            .sum::<f64>();
        let station_alighted_total = stop_flow_states
            .iter()
            .map(|x| x.alighted_this_step.max(0.0))
            .sum::<f64>();

        let mut consistency_checks = Vec::<FlowConsistencyCheck>::new();
        consistency_checks.push(FlowConsistencyCheck {
            name: "latent_equals_realised_plus_unserved".to_string(),
            passed: (total_latent_active - (total_realised + total_unserved)).abs() <= 1e-6,
            lhs: total_latent_active,
            rhs: total_realised + total_unserved,
            tolerance: 1e-6,
            details: "Active-slice latent demand should reconcile with assigned+unserved."
                .to_string(),
        });
        consistency_checks.push(FlowConsistencyCheck {
            name: "station_boardings_match_vehicle_boardings".to_string(),
            passed: (station_boarded_total - vehicle_boarded_total).abs() <= 1e-6,
            lhs: station_boarded_total,
            rhs: vehicle_boarded_total,
            tolerance: 1e-6,
            details: "Boardings aggregated by station should equal boardings aggregated by vehicle stop states.".to_string(),
        });
        consistency_checks.push(FlowConsistencyCheck {
            name: "station_alightings_match_vehicle_alightings".to_string(),
            passed: (station_alighted_total - vehicle_alighted_total).abs() <= 1e-6,
            lhs: station_alighted_total,
            rhs: vehicle_alighted_total,
            tolerance: 1e-6,
            details: "Alightings aggregated by station should equal alightings aggregated by vehicle stop states.".to_string(),
        });
        let mut load_over_capacity = 0.0_f64;
        for v in &vehicle_load_states {
            if v.load_after_stop > v.capacity + 1e-6 {
                load_over_capacity += 1.0;
            }
        }
        consistency_checks.push(FlowConsistencyCheck {
            name: "vehicle_load_not_over_capacity".to_string(),
            passed: load_over_capacity <= 0.0,
            lhs: load_over_capacity,
            rhs: 0.0,
            tolerance: 0.0,
            details: "Vehicle load snapshots should not exceed effective period capacity."
                .to_string(),
        });

        let mut top_centrality_zones = latent_build
            .zone_profiles
            .iter()
            .map(|p| ZoneScoreEntry {
                zone_id: p.zone_id.clone(),
                score: p.centrality_score,
            })
            .collect::<Vec<_>>();
        top_centrality_zones.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_centrality_zones.truncate(12);

        let mut top_work_attractors = latent_build
            .zone_profiles
            .iter()
            .map(|p| ZoneScoreEntry {
                zone_id: p.zone_id.clone(),
                score: p.work_attractiveness,
            })
            .collect::<Vec<_>>();
        top_work_attractors.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_work_attractors.truncate(12);

        let mut top_od_pairs = assigned_od_flows.clone();
        top_od_pairs.sort_by(|a, b| {
            b.unserved_passengers
                .partial_cmp(&a.unserved_passengers)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_od_pairs.truncate(12);

        let demand_diagnostics = DemandDiagnostics {
            totals_by_time_slice,
            total_latent_demand: total_latent_active.max(0.0),
            total_realised_demand: total_realised.max(0.0),
            total_unserved_demand: total_unserved.max(0.0),
            total_waiting_passengers_network: total_waiting_network.max(0.0),
            boardings_alightings_by_station,
            vehicle_loads_by_service,
            top_od_pairs,
            top_centrality_zones,
            top_work_attractors,
            top_intercity_pairs: Vec::new(),
            strongest_commuter_corridors: Vec::new(),
            strongest_rural_to_town_flows: Vec::new(),
            strongest_anchor_flows: Vec::new(),
            consistency_checks,
        };

        return Ok(SimulationOutput {
            meta: OutputMeta {
                results_version: "0.2.2-lite".to_string(),
                scenario_name: s.meta.name.clone(),
                seed: s.meta.seed,
                time_period_hours: s.meta.time_period_hours,
            },
            kpis: Kpis {
                total_trips_attempted,
                total_trips_served,
                share_trips_served,
                total_trips: total_trips_served,
                mean_generalized_cost_s: mean(sum_gc),
                mean_in_vehicle_time_s: mean(sum_ivt),
                mean_wait_time_s: mean(sum_wait),
                mean_walk_time_s: mean(sum_walk),
                mean_transfer_time_s: mean(sum_transfer_time),
                mean_transfer_penalty_s: mean(sum_transfer_pen),
                mean_transfers: mean(sum_transfers),
                mean_boardings: mean(sum_boardings),
                total_boardings_attempted,
                total_boardings_served,
                total_boardings_denied,
                share_boardings_served,
                total_fare_revenue_base: sum_fare_revenue_base.max(0.0),
                total_overflow_dropped,
                share_demand_overflow_dropped,
            },
            link_loads,
            board_loads: final_board_loads,
            stop_flows,
            passenger_cohorts,
            fare_flow,
            zone_demand_profiles: latent_build.zone_profiles.clone(),
            latent_od_demand: latent_build.all_latent.clone(),
            assigned_od_flows: assigned_od_flows.clone(),
            mode_choice_results: mode_choice_build.results.clone(),
            stop_flow_states,
            vehicle_load_states,
            service_operation_states: Vec::new(),
            stop_operation_states: Vec::new(),
            transfer_operation_metrics: Vec::new(),
            service_reliability_diagnostics: ServiceReliabilityDiagnostics::default(),
            synthetic_economy_config: Some(latent_build.economy_config.clone()),
            zone_demand_layer: Vec::new(),
            zone_economic_geography_layer: Vec::new(),
            zone_demand_production_layer: Vec::new(),
            zone_demand_attraction_layer: Vec::new(),
            corridor_desire_lines: Vec::new(),
            service_gap_layer: Vec::new(),
            service_load_layer: service_load_layer_light,
            planning_overlay_config: Some(PlanningOverlayConfig::default()),
            zone_planning_metrics: Vec::new(),
            station_planning_metrics: Vec::new(),
            corridor_planning_metrics: Vec::new(),
            line_service_planning_metrics: Vec::new(),
            network_financial_summary: NetworkFinancialSummary::default(),
            service_financial_metrics: Vec::new(),
            corridor_financial_metrics: Vec::new(),
            station_financial_context: Vec::new(),
            zone_mode_share_metrics: Vec::new(),
            corridor_mode_share_metrics: Vec::new(),
            station_transit_capture_context: Vec::new(),
            service_transit_capture_context: Vec::new(),
            citywide_mode_share_summary: CitywideModeShareSummary::default(),
            build_preview_metrics: Vec::new(),
            service_gap_rankings: ServiceGapRankings::default(),
            planning_debug_summary: PlanningDebugSummary::default(),
            demand_diagnostics,
            active_temporal_slice: latent_build.active_context.clone(),
            temporal_planning_snapshots: Vec::new(),
            temporal_demand_diagnostics: TemporalDemandDiagnostics::default(),
            modal_demand_diagnostics: ModalDemandDiagnostics::default(),
            economic_diagnostics: EconomicDiagnostics::default(),
            diagnostics: Diagnostics {
                zones: s.world.zones.len(),
                stops: s.world.stops.len(),
                links: s.world.links.len(),
                services: s.world.services.len(),
                transfers: graph.transfer_edges,
                access_edges: graph.access_edges,
                egress_edges: graph.egress_edges,
                msa_iterations: iters_run,
                msa_final_max_rel_change: last_max_rel_change,
                sample_paths,
            },
        });
    }

    let profile_by_zone = latent_build
        .zone_profiles
        .iter()
        .map(|p| (p.zone_id.clone(), p.clone()))
        .collect::<HashMap<_, _>>();
    let mut zone_layer_by_id: HashMap<String, ZoneDemandLayerData> = latent_build
        .zone_profiles
        .iter()
        .map(|p| {
            (
                p.zone_id.clone(),
                ZoneDemandLayerData {
                    zone_id: p.zone_id.clone(),
                    settlement_class: Some(p.settlement_class),
                    archetype: Some(p.archetype),
                    centrality_score: Some(p.centrality_score),
                    regional_importance: Some(p.regional_importance),
                    population_density: Some(p.population_density),
                    employment_density: Some(p.employment_density),
                    retail_intensity: Some(p.retail_intensity),
                    leisure_intensity: Some(p.leisure_intensity),
                    education_intensity: Some(p.education_intensity),
                    industry_intensity: Some(p.industry_intensity),
                    work_attractiveness: Some(p.work_attractiveness),
                    education_attractiveness: Some(p.education_attractiveness),
                    shopping_attractiveness: Some(p.shopping_attractiveness),
                    leisure_attractiveness: Some(p.leisure_attractiveness),
                    essential_service_attractiveness: Some(p.essential_service_attractiveness),
                    intercity_importance: Some(p.intercity_importance),
                    special_attractors: p.special_attractors.clone(),
                    total_latent_demand_produced: 0.0,
                    total_latent_demand_attracted: 0.0,
                    total_realised_demand_produced: 0.0,
                    total_unserved_demand_produced: 0.0,
                    accessibility_score: None,
                    service_coverage_score: None,
                },
            )
        })
        .collect();
    let mut production_by_zone_purpose: HashMap<(String, TripPurpose), (f64, f64, f64)> =
        HashMap::new();
    let mut attraction_latent_by_zone_purpose: HashMap<(String, TripPurpose), f64> = HashMap::new();
    let mut attraction_realised_by_zone_purpose: HashMap<(String, TripPurpose), f64> =
        HashMap::new();
    let mut corridor_map: HashMap<(String, String, TripPurpose), CorridorDesireLineData> =
        HashMap::new();
    for od in &latent_build.active_latent {
        if let Some(entry) = zone_layer_by_id.get_mut(&od.origin_zone_id) {
            entry.total_latent_demand_produced += od.latent_passengers.max(0.0);
        }
        if let Some(entry) = zone_layer_by_id.get_mut(&od.destination_zone_id) {
            entry.total_latent_demand_attracted += od.latent_passengers.max(0.0);
        }
        let pkey = (od.origin_zone_id.clone(), od.purpose);
        let pentry = production_by_zone_purpose
            .entry(pkey)
            .or_insert((0.0, 0.0, 0.0));
        pentry.0 += od.latent_passengers.max(0.0);

        let akey = (od.destination_zone_id.clone(), od.purpose);
        *attraction_latent_by_zone_purpose.entry(akey).or_insert(0.0) +=
            od.latent_passengers.max(0.0);

        let ckey = (
            od.origin_zone_id.clone(),
            od.destination_zone_id.clone(),
            od.purpose,
        );
        let corridor = corridor_map.entry(ckey).or_insert(CorridorDesireLineData {
            origin_zone_id: od.origin_zone_id.clone(),
            destination_zone_id: od.destination_zone_id.clone(),
            purpose: od.purpose,
            latent_passengers: 0.0,
            realised_passengers: 0.0,
            unserved_passengers: 0.0,
            corridor_score: 0.0,
            is_underserved: false,
        });
        corridor.latent_passengers += od.latent_passengers.max(0.0);
    }
    for flow in &assigned_od_flows {
        if let Some(entry) = zone_layer_by_id.get_mut(&flow.origin_zone_id) {
            entry.total_realised_demand_produced += flow.assigned_passengers.max(0.0);
            entry.total_unserved_demand_produced += flow.unserved_passengers.max(0.0);
        }
        let pkey = (flow.origin_zone_id.clone(), flow.purpose);
        let pentry = production_by_zone_purpose
            .entry(pkey)
            .or_insert((0.0, 0.0, 0.0));
        pentry.1 += flow.assigned_passengers.max(0.0);
        pentry.2 += flow.unserved_passengers.max(0.0);
        let akey = (flow.destination_zone_id.clone(), flow.purpose);
        *attraction_realised_by_zone_purpose
            .entry(akey)
            .or_insert(0.0) += flow.assigned_passengers.max(0.0);

        let ckey = (
            flow.origin_zone_id.clone(),
            flow.destination_zone_id.clone(),
            flow.purpose,
        );
        let corridor = corridor_map.entry(ckey).or_insert(CorridorDesireLineData {
            origin_zone_id: flow.origin_zone_id.clone(),
            destination_zone_id: flow.destination_zone_id.clone(),
            purpose: flow.purpose,
            latent_passengers: 0.0,
            realised_passengers: 0.0,
            unserved_passengers: 0.0,
            corridor_score: 0.0,
            is_underserved: false,
        });
        corridor.realised_passengers += flow.assigned_passengers.max(0.0);
        corridor.unserved_passengers += flow.unserved_passengers.max(0.0);
    }
    let mut zone_demand_layer = zone_layer_by_id.into_values().collect::<Vec<_>>();
    for zone in &mut zone_demand_layer {
        let coverage = if zone.total_latent_demand_produced > 0.0 {
            (zone.total_realised_demand_produced / zone.total_latent_demand_produced)
                .clamp(0.0, 1.0)
        } else {
            1.0
        };
        let accessibility = profile_by_zone
            .get(&zone.zone_id)
            .map(|p| {
                (0.45 * p.transit_affinity
                    + 0.35 * p.centrality_score
                    + 0.20 * p.regional_importance)
                    .clamp(0.0, 1.0)
            })
            .unwrap_or(0.5);
        zone.service_coverage_score = Some(coverage);
        zone.accessibility_score = Some(accessibility);
    }
    zone_demand_layer.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut zone_economic_geography_layer = latent_build
        .zone_profiles
        .iter()
        .map(|p| ZoneEconomicGeographyLayerData {
            zone_id: p.zone_id.clone(),
            settlement_class: p.settlement_class,
            archetype: p.archetype,
            centrality_score: p.centrality_score,
            regional_importance: p.regional_importance,
            population_density: p.population_density,
            employment_density: p.employment_density,
            retail_intensity: p.retail_intensity,
            leisure_intensity: p.leisure_intensity,
            education_intensity: p.education_intensity,
            industry_intensity: p.industry_intensity,
            work_attractiveness: p.work_attractiveness,
            education_attractiveness: p.education_attractiveness,
            shopping_attractiveness: p.shopping_attractiveness,
            leisure_attractiveness: p.leisure_attractiveness,
            essential_service_attractiveness: p.essential_service_attractiveness,
            intercity_importance: p.intercity_importance,
            special_attractors: p.special_attractors.clone(),
        })
        .collect::<Vec<_>>();
    zone_economic_geography_layer.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut zone_demand_production_layer = latent_build
        .zone_profiles
        .iter()
        .map(|p| {
            let mut by_purpose = Vec::new();
            for purpose in TripPurpose::ALL {
                let tuple = production_by_zone_purpose
                    .get(&(p.zone_id.clone(), purpose))
                    .copied()
                    .unwrap_or((0.0, 0.0, 0.0));
                by_purpose.push(PurposeDemandValue {
                    purpose,
                    latent: tuple.0.max(0.0),
                    realised: tuple.1.max(0.0),
                    unserved: tuple.2.max(0.0),
                });
            }
            ZoneDemandProductionLayerData {
                zone_id: p.zone_id.clone(),
                by_purpose,
            }
        })
        .collect::<Vec<_>>();
    zone_demand_production_layer.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut zone_demand_attraction_layer = latent_build
        .zone_profiles
        .iter()
        .map(|p| {
            let mut latent_by_purpose = Vec::new();
            let mut realised_by_purpose = Vec::new();
            for purpose in TripPurpose::ALL {
                let latent = attraction_latent_by_zone_purpose
                    .get(&(p.zone_id.clone(), purpose))
                    .copied()
                    .unwrap_or(0.0);
                let realised = attraction_realised_by_zone_purpose
                    .get(&(p.zone_id.clone(), purpose))
                    .copied()
                    .unwrap_or(0.0);
                latent_by_purpose.push(PurposeDemandValue {
                    purpose,
                    latent: latent.max(0.0),
                    realised: 0.0,
                    unserved: 0.0,
                });
                realised_by_purpose.push(PurposeDemandValue {
                    purpose,
                    latent: 0.0,
                    realised: realised.max(0.0),
                    unserved: 0.0,
                });
            }
            ZoneDemandAttractionLayerData {
                zone_id: p.zone_id.clone(),
                latent_by_purpose,
                realised_by_purpose,
            }
        })
        .collect::<Vec<_>>();
    zone_demand_attraction_layer.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut corridor_desire_lines = corridor_map
        .into_values()
        .map(|mut c| {
            let gap_ratio = if c.latent_passengers > 0.0 {
                (c.unserved_passengers / c.latent_passengers).clamp(0.0, 1.0)
            } else {
                0.0
            };
            c.corridor_score = c.latent_passengers * (1.0 + 0.8 * gap_ratio);
            c.is_underserved = gap_ratio >= 0.25;
            c
        })
        .collect::<Vec<_>>();
    corridor_desire_lines.sort_by(|a, b| {
        b.corridor_score
            .partial_cmp(&a.corridor_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut service_gap_layer = Vec::<ZoneServiceGapLayerData>::new();
    for prod in &zone_demand_production_layer {
        let total_latent = prod
            .by_purpose
            .iter()
            .map(|x| x.latent.max(0.0))
            .sum::<f64>();
        let total_realised = prod
            .by_purpose
            .iter()
            .map(|x| x.realised.max(0.0))
            .sum::<f64>();
        let total_unserved = prod
            .by_purpose
            .iter()
            .map(|x| x.unserved.max(0.0))
            .sum::<f64>();
        let ratio = if total_realised > 0.0 {
            total_latent / total_realised
        } else if total_latent > 0.0 {
            999.0
        } else {
            1.0
        };
        let coverage = if total_latent > 0.0 {
            (total_realised / total_latent).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let access = profile_by_zone.get(&prod.zone_id).map(|p| {
            (0.45 * p.transit_affinity + 0.35 * p.centrality_score + 0.20 * p.regional_importance)
                .clamp(0.0, 1.0)
        });
        let unserved_by_purpose = prod
            .by_purpose
            .iter()
            .map(|x| PurposeDemandValue {
                purpose: x.purpose,
                latent: 0.0,
                realised: 0.0,
                unserved: x.unserved.max(0.0),
            })
            .collect::<Vec<_>>();
        service_gap_layer.push(ZoneServiceGapLayerData {
            zone_id: prod.zone_id.clone(),
            total_unserved_demand: total_unserved.max(0.0),
            unserved_by_purpose,
            latent_vs_realised_ratio: ratio.max(0.0),
            accessibility_score: access,
            service_coverage_score: Some(coverage),
        });
    }
    service_gap_layer.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut service_load_layer: Vec<ServiceLoadLayerData> = Vec::new();
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
        service_load_layer.push(ServiceLoadLayerData {
            service_id: svc.id.clone(),
            line_id: svc.line_id.clone(),
            passengers: passengers.max(0.0),
            peak_load: peak_load.max(0.0),
            peak_load_stop_id,
        });
    }
    service_load_layer.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let mut operations_outputs = build_operations_outputs(
        s,
        &latent_build.active_context,
        &latent_build.economy_config.operations_reliability_config,
        &final_board_loads,
        &stop_flow_states,
        &vehicle_load_states,
        &mode_choice_build.results,
    );

    let modal_outputs = build_modal_outputs(
        s,
        &latent_build.zone_profiles,
        &mode_choice_build.results,
        &assigned_od_flows,
        &stop_flow_states,
        &vehicle_load_states,
        &service_load_layer,
        &latent_build.active_context,
    );

    let mut phase3 = phase3::build_phase3_planning_outputs(
        s,
        &graph,
        &latent_build.zone_profiles,
        &zone_demand_layer,
        &zone_demand_production_layer,
        &zone_demand_attraction_layer,
        &corridor_desire_lines,
        &service_gap_layer,
        &service_load_layer,
        &stop_flow_states,
        &vehicle_load_states,
        &assigned_od_flows,
        &modal_outputs,
        &operations_outputs,
    );

    let economics_outputs = build_economics_outputs(
        s,
        &latent_build.active_context,
        &latent_build.economy_config,
        &assigned_od_flows,
        &phase3.line_service_planning_metrics,
        &phase3.corridor_planning_metrics,
        &phase3.station_planning_metrics,
        &phase3.zone_planning_metrics,
        &operations_outputs,
    );
    apply_economics_to_planning(
        &mut phase3,
        &economics_outputs,
        &latent_build.zone_profiles,
        &latent_build.economy_config,
    );

    let mut totals_by_slice: BTreeMap<DemandTimeSliceLabel, TimeSliceDemandTotals> =
        BTreeMap::new();
    for od in &latent_build.all_latent {
        let entry = totals_by_slice
            .entry(od.time_slice)
            .or_insert_with(|| TimeSliceDemandTotals {
                time_slice: od.time_slice,
                total_latent: 0.0,
                total_realised: 0.0,
                total_unserved: 0.0,
            });
        entry.total_latent += od.latent_passengers.max(0.0);
    }
    for flow in &assigned_od_flows {
        let entry =
            totals_by_slice
                .entry(flow.time_slice)
                .or_insert_with(|| TimeSliceDemandTotals {
                    time_slice: flow.time_slice,
                    total_latent: 0.0,
                    total_realised: 0.0,
                    total_unserved: 0.0,
                });
        entry.total_realised += flow.assigned_passengers.max(0.0);
        entry.total_unserved += flow.unserved_passengers.max(0.0);
    }
    let totals_by_time_slice = totals_by_slice.into_values().collect::<Vec<_>>();

    let mut boardings_alightings_by_station = stop_flow_states
        .iter()
        .map(|s| StationFlowAggregate {
            stop_id: s.stop_id.clone(),
            boarded: s.boarded_this_step.max(0.0),
            alighted: s.alighted_this_step.max(0.0),
            denied: s.denied_this_step.max(0.0),
            waiting: s.total_waiting.max(0.0),
        })
        .collect::<Vec<_>>();
    boardings_alightings_by_station.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut vehicle_loads_by_service = service_load_layer
        .iter()
        .map(|svc| ServiceVehicleLoadAggregate {
            service_id: svc.service_id.clone(),
            current_load: vehicle_load_states
                .iter()
                .rfind(|x| x.service_id == svc.service_id)
                .map(|x| x.current_load.max(0.0))
                .unwrap_or(0.0),
            max_load_seen: svc.peak_load.max(0.0),
            capacity: vehicle_load_states
                .iter()
                .filter(|x| x.service_id == svc.service_id)
                .map(|x| x.capacity.max(0.0))
                .fold(0.0_f64, f64::max),
        })
        .collect::<Vec<_>>();
    vehicle_loads_by_service.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let total_latent_active = latent_build
        .active_latent
        .iter()
        .map(|x| x.latent_passengers.max(0.0))
        .sum::<f64>();
    let total_realised = assigned_od_flows
        .iter()
        .map(|x| x.assigned_passengers.max(0.0))
        .sum::<f64>();
    let total_unserved = assigned_od_flows
        .iter()
        .map(|x| x.unserved_passengers.max(0.0))
        .sum::<f64>();
    let total_waiting_network = stop_flow_states
        .iter()
        .map(|x| x.total_waiting.max(0.0))
        .sum::<f64>();

    let vehicle_boarded_total = vehicle_load_states
        .iter()
        .map(|x| x.boardings_this_stop.max(0.0))
        .sum::<f64>();
    let station_boarded_total = stop_flow_states
        .iter()
        .map(|x| x.boarded_this_step.max(0.0))
        .sum::<f64>();
    let vehicle_alighted_total = vehicle_load_states
        .iter()
        .map(|x| x.alightings_this_stop.max(0.0))
        .sum::<f64>();
    let station_alighted_total = stop_flow_states
        .iter()
        .map(|x| x.alighted_this_step.max(0.0))
        .sum::<f64>();

    let mut consistency_checks: Vec<FlowConsistencyCheck> = Vec::new();
    consistency_checks.push(FlowConsistencyCheck {
        name: "latent_equals_realised_plus_unserved".to_string(),
        passed: (total_latent_active - (total_realised + total_unserved)).abs() <= 1e-6,
        lhs: total_latent_active,
        rhs: total_realised + total_unserved,
        tolerance: 1e-6,
        details: "Active-slice latent demand should reconcile with assigned+unserved.".to_string(),
    });
    consistency_checks.push(FlowConsistencyCheck {
        name: "station_boardings_match_vehicle_boardings".to_string(),
        passed: (station_boarded_total - vehicle_boarded_total).abs() <= 1e-6,
        lhs: station_boarded_total,
        rhs: vehicle_boarded_total,
        tolerance: 1e-6,
        details: "Boardings aggregated by station should equal boardings aggregated by vehicle stop states.".to_string(),
    });
    consistency_checks.push(FlowConsistencyCheck {
        name: "station_alightings_match_vehicle_alightings".to_string(),
        passed: (station_alighted_total - vehicle_alighted_total).abs() <= 1e-6,
        lhs: station_alighted_total,
        rhs: vehicle_alighted_total,
        tolerance: 1e-6,
        details: "Alightings aggregated by station should equal alightings aggregated by vehicle stop states.".to_string(),
    });
    let mut load_over_capacity = 0.0_f64;
    for v in &vehicle_load_states {
        if v.load_after_stop > v.capacity + 1e-6 {
            load_over_capacity += 1.0;
        }
    }
    consistency_checks.push(FlowConsistencyCheck {
        name: "vehicle_load_not_over_capacity".to_string(),
        passed: load_over_capacity <= 0.0,
        lhs: load_over_capacity,
        rhs: 0.0,
        tolerance: 0.0,
        details: "Vehicle load snapshots should not exceed effective period capacity.".to_string(),
    });

    let mut top_od_pairs = assigned_od_flows.clone();
    top_od_pairs.sort_by(|a, b| {
        b.unserved_passengers
            .partial_cmp(&a.unserved_passengers)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_od_pairs.truncate(12);

    let mut top_centrality_zones = latent_build
        .zone_profiles
        .iter()
        .map(|p| ZoneScoreEntry {
            zone_id: p.zone_id.clone(),
            score: p.centrality_score,
        })
        .collect::<Vec<_>>();
    top_centrality_zones.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_centrality_zones.truncate(12);

    let mut top_work_attractors = latent_build
        .zone_profiles
        .iter()
        .map(|p| ZoneScoreEntry {
            zone_id: p.zone_id.clone(),
            score: p.work_attractiveness,
        })
        .collect::<Vec<_>>();
    top_work_attractors.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_work_attractors.truncate(12);

    let mut top_intercity_pairs = corridor_desire_lines
        .iter()
        .filter(|c| c.purpose == TripPurpose::Intercity)
        .cloned()
        .collect::<Vec<_>>();
    top_intercity_pairs.truncate(12);

    let mut strongest_commuter_corridors = corridor_desire_lines
        .iter()
        .filter(|c| c.purpose == TripPurpose::Work)
        .filter(|c| {
            let origin = profile_by_zone.get(&c.origin_zone_id);
            let dest = profile_by_zone.get(&c.destination_zone_id);
            if let (Some(o), Some(d)) = (origin, dest) {
                (matches!(
                    o.archetype,
                    ZoneArchetype::OuterSuburb
                        | ZoneArchetype::InnerResidential
                        | ZoneArchetype::VillageCentre
                        | ZoneArchetype::RuralResidential
                ) || matches!(
                    o.settlement_class,
                    SettlementClass::LargeTown
                        | SettlementClass::SmallTown
                        | SettlementClass::Village
                        | SettlementClass::Rural
                )) && settlement_rank(d.settlement_class)
                    >= settlement_rank(SettlementClass::RegionalCity)
            } else {
                false
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    strongest_commuter_corridors.truncate(12);

    let mut strongest_rural_to_town_flows = corridor_desire_lines
        .iter()
        .filter(|c| {
            let origin = profile_by_zone.get(&c.origin_zone_id);
            let dest = profile_by_zone.get(&c.destination_zone_id);
            if let (Some(o), Some(d)) = (origin, dest) {
                matches!(
                    o.settlement_class,
                    SettlementClass::Village | SettlementClass::Rural
                ) && settlement_rank(d.settlement_class)
                    >= settlement_rank(SettlementClass::SmallTown)
            } else {
                false
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    strongest_rural_to_town_flows.truncate(12);

    let mut strongest_anchor_flows = corridor_desire_lines
        .iter()
        .filter(|c| {
            let origin = profile_by_zone.get(&c.origin_zone_id);
            let dest = profile_by_zone.get(&c.destination_zone_id);
            if let (Some(o), Some(d)) = (origin, dest) {
                o.special_attractors.iter().any(|a| {
                    matches!(
                        a,
                        SpecialAttractorType::Airport | SpecialAttractorType::University
                    )
                }) || d.special_attractors.iter().any(|a| {
                    matches!(
                        a,
                        SpecialAttractorType::Airport | SpecialAttractorType::University
                    )
                })
            } else {
                false
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    strongest_anchor_flows.truncate(12);

    let demand_diagnostics = DemandDiagnostics {
        totals_by_time_slice,
        total_latent_demand: total_latent_active.max(0.0),
        total_realised_demand: total_realised.max(0.0),
        total_unserved_demand: total_unserved.max(0.0),
        total_waiting_passengers_network: total_waiting_network.max(0.0),
        boardings_alightings_by_station,
        vehicle_loads_by_service,
        top_od_pairs,
        top_centrality_zones,
        top_work_attractors,
        top_intercity_pairs,
        strongest_commuter_corridors,
        strongest_rural_to_town_flows,
        strongest_anchor_flows,
        consistency_checks,
    };

    let active_temporal_slice = latent_build.active_context.clone();
    let (
        temporal_planning_snapshots,
        temporal_demand_diagnostics,
        temporal_mode_share_by_slice,
        temporal_mode_share_by_day,
    ) = if include_temporal_bundle {
        let mut contexts = vec![TemporalContextAggregate {
            context: active_temporal_slice.clone(),
            latent_od_demand: latent_build.all_latent.clone(),
            assigned_od_flows: assigned_od_flows.clone(),
            mode_total_latent: modal_outputs
                .mode_choice_results
                .iter()
                .map(|x| x.latent_passengers.max(0.0))
                .sum::<f64>(),
            mode_share_summary: modal_outputs.citywide_mode_share_summary.clone(),
            zone_planning_metrics: phase3.zone_planning_metrics.clone(),
            station_planning_metrics: phase3.station_planning_metrics.clone(),
            corridor_planning_metrics: phase3.corridor_planning_metrics.clone(),
            line_service_planning_metrics: phase3.line_service_planning_metrics.clone(),
            service_gap_layer: service_gap_layer.clone(),
            service_gap_rankings: phase3.service_gap_rankings.clone(),
            network_financial_summary: economics_outputs.network_financial_summary.clone(),
        }];

        for ctx in temporal_context_catalog(&active_temporal_slice) {
            if ctx.time_slice == active_temporal_slice.time_slice
                && ctx.service_day_type == active_temporal_slice.service_day_type
                && ctx.seasonal_profile == active_temporal_slice.seasonal_profile
            {
                continue;
            }
            let nested = run_simulation_internal(s, settings, None, Some(ctx.clone()), false)?;
            contexts.push(TemporalContextAggregate {
                context: nested.active_temporal_slice.clone(),
                latent_od_demand: nested.latent_od_demand,
                assigned_od_flows: nested.assigned_od_flows,
                mode_total_latent: nested
                    .mode_choice_results
                    .iter()
                    .map(|x| x.latent_passengers.max(0.0))
                    .sum::<f64>(),
                mode_share_summary: nested.citywide_mode_share_summary,
                zone_planning_metrics: nested.zone_planning_metrics,
                station_planning_metrics: nested.station_planning_metrics,
                corridor_planning_metrics: nested.corridor_planning_metrics,
                line_service_planning_metrics: nested.line_service_planning_metrics,
                service_gap_layer: nested.service_gap_layer,
                service_gap_rankings: nested.service_gap_rankings,
                network_financial_summary: nested.network_financial_summary,
            });
        }
        let (snapshots, temporal_diag) = build_temporal_outputs(&contexts);
        let (mode_by_slice, mode_by_day) = build_temporal_mode_summaries(&contexts);
        (snapshots, temporal_diag, mode_by_slice, mode_by_day)
    } else {
        (
            Vec::new(),
            TemporalDemandDiagnostics::default(),
            Vec::new(),
            Vec::new(),
        )
    };

    let mut modal_demand_diagnostics = modal_outputs.modal_demand_diagnostics.clone();
    let mut economic_diagnostics = economics_outputs.economic_diagnostics.clone();
    let mut service_reliability_diagnostics =
        operations_outputs.service_reliability_diagnostics.clone();
    if !temporal_mode_share_by_slice.is_empty() {
        modal_demand_diagnostics.mode_share_by_time_slice = temporal_mode_share_by_slice;
    }
    if !temporal_mode_share_by_day.is_empty() {
        modal_demand_diagnostics.mode_share_by_day_type = temporal_mode_share_by_day;
    }
    if include_temporal_bundle {
        let mut worst_reliability_by_time_slice = Vec::<TemporalRankingEntry>::new();
        let mut worst_dwell_pressure_stations_by_time_slice = Vec::<TemporalRankingEntry>::new();
        let mut worst_transfer_nodes_by_time_slice = Vec::<TemporalRankingEntry>::new();
        for ctx in &temporal_planning_snapshots {
            if let Some(worst_line) = ctx.line_service_planning_metrics.iter().min_by(|a, b| {
                a.reliability_score
                    .partial_cmp(&b.reliability_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                worst_reliability_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_line.service_id.clone(),
                    score: worst_line.reliability_score.max(0.0),
                    reason: format!(
                        "delay {:.1}s, irregularity {:.2}",
                        worst_line.average_delay_s.max(0.0),
                        worst_line.headway_irregularity.max(0.0)
                    ),
                });
            }
            if let Some(worst_station) = ctx.station_planning_metrics.iter().max_by(|a, b| {
                a.operational_pressure_score
                    .partial_cmp(&b.operational_pressure_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                worst_dwell_pressure_stations_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_station.stop_id.clone(),
                    score: worst_station.operational_pressure_score.max(0.0),
                    reason: format!(
                        "avg_dwell {:.1}s, transfer_success {:.2}",
                        worst_station.average_dwell_time_s.max(0.0),
                        worst_station.transfer_success_rate.clamp(0.0, 1.0)
                    ),
                });
            }
            if let Some(worst_transfer_station) = ctx
                .station_planning_metrics
                .iter()
                .filter(|x| x.transfer_success_rate > 0.0)
                .min_by(|a, b| {
                    a.transfer_success_rate
                        .partial_cmp(&b.transfer_success_rate)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                worst_transfer_nodes_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_transfer_station.stop_id.clone(),
                    score: (1.0 - worst_transfer_station.transfer_success_rate).max(0.0),
                    reason: format!(
                        "transfer success {:.2}",
                        worst_transfer_station.transfer_success_rate.clamp(0.0, 1.0)
                    ),
                });
            }
        }
        service_reliability_diagnostics.worst_reliability_by_time_slice =
            worst_reliability_by_time_slice;
        service_reliability_diagnostics.worst_dwell_pressure_stations_by_time_slice =
            worst_dwell_pressure_stations_by_time_slice;
        service_reliability_diagnostics.worst_transfer_nodes_by_time_slice =
            worst_transfer_nodes_by_time_slice;

        let mut by_slice = Vec::<super::types::TemporalFinancialSummary>::new();
        for ctx in &temporal_planning_snapshots {
            by_slice.push(super::types::TemporalFinancialSummary {
                temporal_slice: ctx.temporal_slice.clone(),
                fare_revenue: ctx.network_financial_summary.metrics.fare_revenue.max(0.0),
                operating_cost: ctx
                    .network_financial_summary
                    .metrics
                    .operating_cost
                    .max(0.0),
                total_cost: ctx.network_financial_summary.metrics.total_cost.max(0.0),
                subsidy_required: ctx
                    .network_financial_summary
                    .metrics
                    .subsidy_required
                    .max(0.0),
                farebox_recovery_ratio: ctx
                    .network_financial_summary
                    .metrics
                    .farebox_recovery_ratio
                    .max(0.0),
            });
        }
        by_slice.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));
        economic_diagnostics.network_financial_by_time_slice = by_slice;

        let mut day_map = HashMap::<ServiceDayType, (f64, f64, f64, f64, f64)>::new();
        for ctx in &temporal_planning_snapshots {
            let weight = ctx
                .network_financial_summary
                .total_realised_transit_trips
                .max(0.0)
                .max(1.0);
            let entry = day_map
                .entry(ctx.temporal_slice.service_day_type)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 += weight;
            entry.1 += ctx.network_financial_summary.metrics.fare_revenue.max(0.0) * weight;
            entry.2 += ctx
                .network_financial_summary
                .metrics
                .operating_cost
                .max(0.0)
                * weight;
            entry.3 += ctx.network_financial_summary.metrics.total_cost.max(0.0) * weight;
            entry.4 += ctx
                .network_financial_summary
                .metrics
                .subsidy_required
                .max(0.0)
                * weight;
        }
        let mut by_day = day_map
            .into_iter()
            .map(|(day, agg)| {
                let denom = agg.0.max(1e-9);
                let fare = agg.1 / denom;
                let op_cost = agg.2 / denom;
                let total_cost = agg.3 / denom;
                let subsidy = agg.4 / denom;
                super::types::ServiceDayFinancialSummary {
                    service_day_type: day,
                    fare_revenue: fare.max(0.0),
                    operating_cost: op_cost.max(0.0),
                    total_cost: total_cost.max(0.0),
                    subsidy_required: subsidy.max(0.0),
                    farebox_recovery_ratio: if op_cost > 0.0 {
                        (fare / op_cost).max(0.0)
                    } else {
                        0.0
                    },
                }
            })
            .collect::<Vec<_>>();
        by_day.sort_by(|a, b| a.service_day_type.cmp(&b.service_day_type));
        economic_diagnostics.network_financial_by_day_type = by_day;
    }
    operations_outputs.service_reliability_diagnostics = service_reliability_diagnostics.clone();

    Ok(SimulationOutput {
        meta: OutputMeta {
            results_version: "0.2.2".to_string(),
            scenario_name: s.meta.name.clone(),
            seed: s.meta.seed,
            time_period_hours: s.meta.time_period_hours,
        },
        kpis: Kpis {
            total_trips_attempted,
            total_trips_served,
            share_trips_served,

            // Back-compat field: now equals served trips
            total_trips: total_trips_served,

            mean_generalized_cost_s: mean(sum_gc),
            mean_in_vehicle_time_s: mean(sum_ivt),
            mean_wait_time_s: mean(sum_wait),
            mean_walk_time_s: mean(sum_walk),

            mean_transfer_time_s: mean(sum_transfer_time),
            mean_transfer_penalty_s: mean(sum_transfer_pen),
            mean_transfers: mean(sum_transfers),

            mean_boardings: mean(sum_boardings),

            total_boardings_attempted,
            total_boardings_served,
            total_boardings_denied,
            share_boardings_served,
            total_fare_revenue_base: sum_fare_revenue_base.max(0.0),
            total_overflow_dropped,
            share_demand_overflow_dropped,
        },
        link_loads,
        board_loads: final_board_loads,
        stop_flows,
        passenger_cohorts,
        fare_flow,
        zone_demand_profiles: latent_build.zone_profiles,
        latent_od_demand: latent_build.all_latent,
        assigned_od_flows,
        mode_choice_results: modal_outputs.mode_choice_results,
        stop_flow_states,
        vehicle_load_states,
        service_operation_states: operations_outputs.service_operation_states,
        stop_operation_states: operations_outputs.stop_operation_states,
        transfer_operation_metrics: operations_outputs.transfer_operation_metrics,
        service_reliability_diagnostics,
        synthetic_economy_config: Some(latent_build.economy_config),
        zone_demand_layer,
        zone_economic_geography_layer,
        zone_demand_production_layer,
        zone_demand_attraction_layer,
        corridor_desire_lines,
        service_gap_layer,
        service_load_layer,
        planning_overlay_config: Some(phase3.planning_overlay_config),
        zone_planning_metrics: phase3.zone_planning_metrics,
        station_planning_metrics: phase3.station_planning_metrics,
        corridor_planning_metrics: phase3.corridor_planning_metrics,
        line_service_planning_metrics: phase3.line_service_planning_metrics,
        network_financial_summary: economics_outputs.network_financial_summary,
        service_financial_metrics: economics_outputs.service_financial_metrics,
        corridor_financial_metrics: economics_outputs.corridor_financial_metrics,
        station_financial_context: economics_outputs.station_financial_context,
        zone_mode_share_metrics: modal_outputs.zone_mode_share_metrics,
        corridor_mode_share_metrics: modal_outputs.corridor_mode_share_metrics,
        station_transit_capture_context: modal_outputs.station_transit_capture_context,
        service_transit_capture_context: modal_outputs.service_transit_capture_context,
        citywide_mode_share_summary: modal_outputs.citywide_mode_share_summary,
        build_preview_metrics: phase3.build_preview_metrics,
        service_gap_rankings: phase3.service_gap_rankings,
        planning_debug_summary: phase3.planning_debug_summary,
        demand_diagnostics,
        active_temporal_slice,
        temporal_planning_snapshots,
        temporal_demand_diagnostics,
        modal_demand_diagnostics,
        economic_diagnostics,
        diagnostics: Diagnostics {
            zones: s.world.zones.len(),
            stops: s.world.stops.len(),
            links: s.world.links.len(),
            services: s.world.services.len(),
            transfers: graph.transfer_edges,
            access_edges: graph.access_edges,
            egress_edges: graph.egress_edges,

            msa_iterations: iters_run,
            msa_final_max_rel_change: last_max_rel_change,
            sample_paths,
        },
    })
}

#[derive(Debug, Clone)]
struct ActiveLatentOd {
    origin_idx: usize,
    destination_idx: usize,
    origin_zone_id: String,
    destination_zone_id: String,
    purpose: TripPurpose,
    time_slice: DemandTimeSliceLabel,
    service_day_type: ServiceDayType,
    seasonal_profile: SeasonalProfile,
    active_event_ids: Vec<String>,
    latent_passengers: f64,
}

#[derive(Debug, Clone)]
struct ActiveModeChoiceOd {
    latent: ActiveLatentOd,
    transit_captured: f64,
    suppressed_or_no_trip: f64,
}

#[derive(Debug, Clone, Default)]
struct ModeChoiceBuild {
    rows: Vec<ActiveModeChoiceOd>,
    results: Vec<ModeChoiceResult>,
}

#[derive(Debug, Clone)]
struct LatentDemandBuild {
    economy_config: SyntheticEconomyConfig,
    zone_profiles: Vec<ZoneDemandProfile>,
    all_latent: Vec<LatentOdDemand>,
    active_latent: Vec<ActiveLatentOd>,
    active_context: TemporalDemandSlice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatentDemandCoverage {
    SingleContext,
    CanonicalSlicesForDay,
}

#[derive(Debug, Clone)]
struct Phase3PlanningOutputs {
    planning_overlay_config: PlanningOverlayConfig,
    zone_planning_metrics: Vec<ZonePlanningMetrics>,
    station_planning_metrics: Vec<StationPlanningMetrics>,
    corridor_planning_metrics: Vec<CorridorPlanningMetrics>,
    line_service_planning_metrics: Vec<LineOrServicePlanningMetrics>,
    build_preview_metrics: Vec<BuildPreviewMetrics>,
    service_gap_rankings: ServiceGapRankings,
    planning_debug_summary: PlanningDebugSummary,
}

#[derive(Debug, Clone, Default)]
struct ModalOutputs {
    mode_choice_results: Vec<ModeChoiceResult>,
    zone_mode_share_metrics: Vec<ZoneModeShareMetrics>,
    corridor_mode_share_metrics: Vec<CorridorModeShareMetrics>,
    station_transit_capture_context: Vec<StationTransitCaptureContext>,
    service_transit_capture_context: Vec<ServiceTransitCaptureContext>,
    citywide_mode_share_summary: CitywideModeShareSummary,
    modal_demand_diagnostics: ModalDemandDiagnostics,
}

#[derive(Debug, Clone, Default)]
struct OperationsOutputs {
    service_operation_states: Vec<ServiceOperationState>,
    stop_operation_states: Vec<StopOperationState>,
    transfer_operation_metrics: Vec<TransferOperationMetrics>,
    service_reliability_diagnostics: ServiceReliabilityDiagnostics,
}

#[derive(Debug, Clone, Default)]
struct EconomicsOutputs {
    network_financial_summary: super::types::NetworkFinancialSummary,
    service_financial_metrics: Vec<super::types::ServiceFinancialMetrics>,
    corridor_financial_metrics: Vec<super::types::CorridorFinancialMetrics>,
    station_financial_context: Vec<super::types::StationFinancialContext>,
    economic_diagnostics: super::types::EconomicDiagnostics,
}

#[derive(Debug, Clone)]
struct TemporalContextAggregate {
    context: TemporalDemandSlice,
    latent_od_demand: Vec<LatentOdDemand>,
    assigned_od_flows: Vec<AssignedOdFlow>,
    mode_total_latent: f64,
    mode_share_summary: CitywideModeShareSummary,
    zone_planning_metrics: Vec<ZonePlanningMetrics>,
    station_planning_metrics: Vec<StationPlanningMetrics>,
    corridor_planning_metrics: Vec<CorridorPlanningMetrics>,
    line_service_planning_metrics: Vec<LineOrServicePlanningMetrics>,
    service_gap_layer: Vec<ZoneServiceGapLayerData>,
    service_gap_rankings: ServiceGapRankings,
    network_financial_summary: super::types::NetworkFinancialSummary,
}

#[derive(Debug, Clone, Copy)]
struct SliceWindow {
    label: DemandTimeSliceLabel,
    start_s: f64,
    end_s: f64,
}

const DEMAND_SLICE_WINDOWS: [SliceWindow; 6] = [
    SliceWindow {
        label: DemandTimeSliceLabel::EarlyMorning,
        start_s: 4.0 * 3600.0,
        end_s: 6.0 * 3600.0,
    },
    SliceWindow {
        label: DemandTimeSliceLabel::AmPeak,
        start_s: 6.0 * 3600.0,
        end_s: 10.0 * 3600.0,
    },
    SliceWindow {
        label: DemandTimeSliceLabel::Interpeak,
        start_s: 10.0 * 3600.0,
        end_s: 16.0 * 3600.0,
    },
    SliceWindow {
        label: DemandTimeSliceLabel::PmPeak,
        start_s: 16.0 * 3600.0,
        end_s: 19.0 * 3600.0,
    },
    SliceWindow {
        label: DemandTimeSliceLabel::Evening,
        start_s: 19.0 * 3600.0,
        end_s: 23.0 * 3600.0,
    },
    SliceWindow {
        label: DemandTimeSliceLabel::LateNight,
        start_s: 23.0 * 3600.0,
        end_s: 4.0 * 3600.0,
    },
];

const CANONICAL_DAY_TYPES: [ServiceDayType; 3] = [
    ServiceDayType::Weekday,
    ServiceDayType::Saturday,
    ServiceDayType::SundayHoliday,
];

fn temporal_context_catalog(active: &TemporalDemandSlice) -> Vec<TemporalDemandSlice> {
    let mut contexts = Vec::<TemporalDemandSlice>::new();
    let mut seen =
        std::collections::BTreeSet::<(ServiceDayType, DemandTimeSliceLabel, SeasonalProfile)>::new(
        );

    let mut push_ctx = |service_day_type: ServiceDayType,
                        time_slice: DemandTimeSliceLabel,
                        seasonal_profile: SeasonalProfile| {
        if seen.insert((service_day_type, time_slice, seasonal_profile)) {
            contexts.push(TemporalDemandSlice {
                service_day_type,
                time_slice,
                seasonal_profile,
                active_event_ids: Vec::new(),
            });
        }
    };

    // Always include the active context first.
    push_ctx(
        active.service_day_type,
        active.time_slice,
        active.seasonal_profile,
    );

    // Core weekly rhythm for the currently selected seasonal profile.
    for day in CANONICAL_DAY_TYPES {
        for window in DEMAND_SLICE_WINDOWS {
            push_ctx(day, window.label, active.seasonal_profile);
        }
    }

    // Explicit term-time education lenses.
    for slice in [
        DemandTimeSliceLabel::AmPeak,
        DemandTimeSliceLabel::Interpeak,
        DemandTimeSliceLabel::PmPeak,
    ] {
        push_ctx(ServiceDayType::Weekday, slice, SeasonalProfile::TermTime);
    }

    // Holiday weekend airport/tourism stress lenses.
    for day in [ServiceDayType::Saturday, ServiceDayType::SundayHoliday] {
        for slice in [
            DemandTimeSliceLabel::EarlyMorning,
            DemandTimeSliceLabel::Interpeak,
            DemandTimeSliceLabel::Evening,
        ] {
            push_ctx(day, slice, SeasonalProfile::HolidayPeriod);
        }
    }

    // Summer tourism uplift lenses.
    for day in [ServiceDayType::Saturday, ServiceDayType::SundayHoliday] {
        for slice in [
            DemandTimeSliceLabel::Interpeak,
            DemandTimeSliceLabel::Evening,
        ] {
            push_ctx(day, slice, SeasonalProfile::SummerPeak);
        }
    }

    contexts
}

fn temporal_context_label(ctx: &TemporalDemandSlice) -> String {
    format!(
        "{:?}/{:?}/{:?}",
        ctx.service_day_type, ctx.time_slice, ctx.seasonal_profile
    )
}

fn build_temporal_outputs(
    contexts: &[TemporalContextAggregate],
) -> (Vec<TemporalPlanningSnapshot>, TemporalDemandDiagnostics) {
    let mut snapshots = contexts
        .iter()
        .map(|ctx| TemporalPlanningSnapshot {
            temporal_slice: ctx.context.clone(),
            zone_planning_metrics: ctx.zone_planning_metrics.clone(),
            station_planning_metrics: ctx.station_planning_metrics.clone(),
            corridor_planning_metrics: ctx.corridor_planning_metrics.clone(),
            line_service_planning_metrics: ctx.line_service_planning_metrics.clone(),
            service_gap_rankings: ctx.service_gap_rankings.clone(),
            network_financial_summary: ctx.network_financial_summary.clone(),
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));

    let mut purpose_totals = Vec::<PurposeTemporalDemandTotals>::new();
    let mut station_pressure = Vec::<TemporalStationPressurePoint>::new();
    let mut service_pressure = Vec::<TemporalServicePressurePoint>::new();
    let mut corridor_pressure = Vec::<TemporalCorridorPressurePoint>::new();
    let mut service_gap_summaries = Vec::<TemporalServiceGapPoint>::new();

    let mut latent_to_realised_ratio_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut top_overloaded_stations_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut top_overloaded_services_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut strongest_corridors_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_waiting_by_station_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_denied_by_station_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_corridor_unserved_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_line_overload_by_slice = Vec::<TemporalRankingEntry>::new();

    let mut station_score_series = HashMap::<String, Vec<(TemporalDemandSlice, f64)>>::new();
    let mut service_score_series = HashMap::<String, Vec<(TemporalDemandSlice, f64)>>::new();

    for ctx in contexts {
        let label = temporal_context_label(&ctx.context);

        for purpose in TripPurpose::ALL {
            let latent = ctx
                .latent_od_demand
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.latent_passengers.max(0.0))
                .sum::<f64>();
            let realised = ctx
                .assigned_od_flows
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.assigned_passengers.max(0.0))
                .sum::<f64>();
            let unserved = ctx
                .assigned_od_flows
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.unserved_passengers.max(0.0))
                .sum::<f64>();
            purpose_totals.push(PurposeTemporalDemandTotals {
                temporal_slice: ctx.context.clone(),
                purpose,
                latent,
                realised,
                unserved,
            });
        }

        for st in &ctx.station_planning_metrics {
            station_pressure.push(TemporalStationPressurePoint {
                temporal_slice: ctx.context.clone(),
                stop_id: st.stop_id.clone(),
                waiting: st.waiting_now.max(0.0),
                denied: st.denied_total.max(0.0),
                boarded: st.boardings_total.max(0.0),
                alighted: st.alightings_total.max(0.0),
                load_pressure_score: st.load_pressure_score.max(0.0),
                overcrowding_risk_score: st.overcrowding_risk_score.max(0.0),
            });
            let station_score = st.overcrowding_risk_score
                + (st.denied_total / (st.boardings_total + 1.0))
                + 0.20 * (st.waiting_now / (st.boardings_total + 1.0));
            station_score_series
                .entry(st.stop_id.clone())
                .or_default()
                .push((ctx.context.clone(), station_score));
        }

        for svc in &ctx.line_service_planning_metrics {
            service_pressure.push(TemporalServicePressurePoint {
                temporal_slice: ctx.context.clone(),
                service_id: svc.service_id.clone(),
                line_id: svc.line_id.clone(),
                peak_load: svc.peak_load.max(0.0),
                average_load: svc.average_load.max(0.0),
                utilisation_score: svc.utilisation_score.max(0.0),
                overcrowded_segments: svc.overcrowded_segments,
            });
            let service_score = svc.utilisation_score + (svc.overcrowded_segments as f64) * 0.25;
            service_score_series
                .entry(svc.service_id.clone())
                .or_default()
                .push((ctx.context.clone(), service_score));
        }

        for corridor in &ctx.corridor_planning_metrics {
            corridor_pressure.push(TemporalCorridorPressurePoint {
                temporal_slice: ctx.context.clone(),
                origin_zone_id: corridor.origin_zone_id.clone(),
                destination_zone_id: corridor.destination_zone_id.clone(),
                purpose: corridor.dominant_purpose,
                latent_volume: corridor.latent_volume.max(0.0),
                realised_volume: corridor.realised_volume.max(0.0),
                unserved_volume: corridor.unserved_volume.max(0.0),
                served_ratio: corridor.served_ratio.clamp(0.0, 1.0),
            });
        }

        for gap in &ctx.service_gap_layer {
            service_gap_summaries.push(TemporalServiceGapPoint {
                temporal_slice: ctx.context.clone(),
                zone_id: gap.zone_id.clone(),
                total_unserved_demand: gap.total_unserved_demand.max(0.0),
                latent_vs_realised_ratio: gap.latent_vs_realised_ratio.max(0.0),
                accessibility_score: gap.accessibility_score,
                service_coverage_score: gap.service_coverage_score,
            });
        }

        let latent_total = ctx
            .latent_od_demand
            .iter()
            .map(|x| x.latent_passengers.max(0.0))
            .sum::<f64>();
        let realised_total = ctx
            .assigned_od_flows
            .iter()
            .map(|x| x.assigned_passengers.max(0.0))
            .sum::<f64>();
        let unserved_total = ctx
            .assigned_od_flows
            .iter()
            .map(|x| x.unserved_passengers.max(0.0))
            .sum::<f64>();
        let ratio = if realised_total > 0.0 {
            latent_total / realised_total
        } else if latent_total > 0.0 {
            999.0
        } else {
            1.0
        };
        latent_to_realised_ratio_by_slice.push(TemporalRankingEntry {
            temporal_slice: ctx.context.clone(),
            id: label.clone(),
            score: ratio,
            reason: format!(
                "latent {:.1}, realised {:.1}, unserved {:.1}",
                latent_total, realised_total, unserved_total
            ),
        });

        if let Some(top_station) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            (a.overcrowding_risk_score + a.denied_total / (a.boardings_total + 1.0))
                .partial_cmp(
                    &(b.overcrowding_risk_score + b.denied_total / (b.boardings_total + 1.0)),
                )
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            top_overloaded_stations_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: top_station.stop_id.clone(),
                score: top_station.overcrowding_risk_score
                    + top_station.denied_total / (top_station.boardings_total + 1.0),
                reason: format!(
                    "waiting {:.1}, denied {:.1}",
                    top_station.waiting_now, top_station.denied_total
                ),
            });
        }

        if let Some(top_service) = ctx.line_service_planning_metrics.iter().max_by(|a, b| {
            (a.utilisation_score + (a.overcrowded_segments as f64) * 0.25)
                .partial_cmp(&(b.utilisation_score + (b.overcrowded_segments as f64) * 0.25))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            top_overloaded_services_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: top_service.service_id.clone(),
                score: top_service.utilisation_score
                    + (top_service.overcrowded_segments as f64) * 0.25,
                reason: format!(
                    "utilisation {:.2}, overcrowded_segments {}",
                    top_service.utilisation_score, top_service.overcrowded_segments
                ),
            });
        }

        if let Some(top_corridor) = ctx.corridor_planning_metrics.iter().max_by(|a, b| {
            (a.unserved_volume + 0.3 * a.latent_volume)
                .partial_cmp(&(b.unserved_volume + 0.3 * b.latent_volume))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            strongest_corridors_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: format!(
                    "{}->{}",
                    top_corridor.origin_zone_id, top_corridor.destination_zone_id
                ),
                score: top_corridor.unserved_volume + 0.3 * top_corridor.latent_volume,
                reason: format!(
                    "purpose {:?}, latent {:.1}, unserved {:.1}",
                    top_corridor.dominant_purpose,
                    top_corridor.latent_volume,
                    top_corridor.unserved_volume
                ),
            });
        }

        if let Some(st_wait) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            a.waiting_now
                .partial_cmp(&b.waiting_now)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_waiting_by_station_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: st_wait.stop_id.clone(),
                score: st_wait.waiting_now.max(0.0),
                reason: format!("context {label}"),
            });
        }
        if let Some(st_denied) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            a.denied_total
                .partial_cmp(&b.denied_total)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_denied_by_station_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: st_denied.stop_id.clone(),
                score: st_denied.denied_total.max(0.0),
                reason: format!("context {label}"),
            });
        }
        if let Some(c_unserved) = ctx.corridor_planning_metrics.iter().max_by(|a, b| {
            a.unserved_volume
                .partial_cmp(&b.unserved_volume)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_corridor_unserved_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: format!(
                    "{}->{}",
                    c_unserved.origin_zone_id, c_unserved.destination_zone_id
                ),
                score: c_unserved.unserved_volume.max(0.0),
                reason: format!("purpose {:?}", c_unserved.dominant_purpose),
            });
        }
        if let Some(s_overload) = ctx.line_service_planning_metrics.iter().max_by(|a, b| {
            (a.utilisation_score + (a.overcrowded_segments as f64) * 0.25)
                .partial_cmp(&(b.utilisation_score + (b.overcrowded_segments as f64) * 0.25))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_line_overload_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: s_overload.service_id.clone(),
                score: s_overload.utilisation_score
                    + (s_overload.overcrowded_segments as f64) * 0.25,
                reason: format!("context {label}"),
            });
        }
    }

    let mut overload_flip_classifications = Vec::<TemporalRankingEntry>::new();
    for (stop_id, series) in station_score_series {
        if series.is_empty() {
            continue;
        }
        let max_entry = series
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let min_entry = series
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some((max_ctx, max_score)), Some((_min_ctx, min_score))) = (max_entry, min_entry) {
            if *max_score >= 1.0 && *max_score > (*min_score * 1.50 + 0.10) {
                overload_flip_classifications.push(TemporalRankingEntry {
                    temporal_slice: max_ctx.clone(),
                    id: format!("station:{stop_id}"),
                    score: *max_score,
                    reason: format!(
                        "pressure rises from {:.2} to {:.2} at {}",
                        min_score,
                        max_score,
                        temporal_context_label(max_ctx)
                    ),
                });
            }
        }
    }
    for (service_id, series) in service_score_series {
        if series.is_empty() {
            continue;
        }
        let max_entry = series
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let min_entry = series
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some((max_ctx, max_score)), Some((_min_ctx, min_score))) = (max_entry, min_entry) {
            if *max_score >= 0.95 && *max_score > (*min_score * 1.35 + 0.05) {
                overload_flip_classifications.push(TemporalRankingEntry {
                    temporal_slice: max_ctx.clone(),
                    id: format!("service:{service_id}"),
                    score: *max_score,
                    reason: format!(
                        "load pressure rises from {:.2} to {:.2} at {}",
                        min_score,
                        max_score,
                        temporal_context_label(max_ctx)
                    ),
                });
            }
        }
    }

    let diagnostics = TemporalDemandDiagnostics {
        purpose_totals,
        station_pressure,
        service_pressure,
        corridor_pressure,
        service_gap_summaries,
        latent_to_realised_ratio_by_slice,
        top_overloaded_stations_by_slice,
        top_overloaded_services_by_slice,
        strongest_corridors_by_slice,
        peak_waiting_by_station_by_slice,
        peak_denied_by_station_by_slice,
        peak_corridor_unserved_by_slice,
        peak_line_overload_by_slice,
        overload_flip_classifications,
    };

    (snapshots, diagnostics)
}

fn build_temporal_mode_summaries(
    contexts: &[TemporalContextAggregate],
) -> (
    Vec<TimeSliceModeShareSummary>,
    Vec<ServiceDayModeShareSummary>,
) {
    let mut by_time_slice = contexts
        .iter()
        .map(|ctx| TimeSliceModeShareSummary {
            temporal_slice: ctx.context.clone(),
            transit_share: ctx.mode_share_summary.transit_share.max(0.0),
            car_share: ctx.mode_share_summary.car_share.max(0.0),
            walk_share: ctx.mode_share_summary.walk_share.max(0.0),
            suppressed_share: ctx.mode_share_summary.suppressed_share.max(0.0),
        })
        .collect::<Vec<_>>();
    by_time_slice.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));

    let mut agg_by_day = HashMap::<ServiceDayType, (f64, f64, f64, f64, f64)>::new();
    for ctx in contexts {
        let weight = ctx.mode_total_latent.max(0.0);
        let entry = agg_by_day
            .entry(ctx.context.service_day_type)
            .or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
        entry.0 += weight;
        entry.1 += ctx.mode_share_summary.transit_share.max(0.0) * weight;
        entry.2 += ctx.mode_share_summary.car_share.max(0.0) * weight;
        entry.3 += ctx.mode_share_summary.walk_share.max(0.0) * weight;
        entry.4 += ctx.mode_share_summary.suppressed_share.max(0.0) * weight;
    }
    let mut by_day_type = agg_by_day
        .into_iter()
        .map(|(day, agg)| {
            let denom = agg.0.max(1e-9);
            ServiceDayModeShareSummary {
                service_day_type: day,
                transit_share: (agg.1 / denom).clamp(0.0, 1.0),
                car_share: (agg.2 / denom).clamp(0.0, 1.0),
                walk_share: (agg.3 / denom).clamp(0.0, 1.0),
                suppressed_share: (agg.4 / denom).clamp(0.0, 1.0),
            }
        })
        .collect::<Vec<_>>();
    by_day_type.sort_by(|a, b| a.service_day_type.cmp(&b.service_day_type));

    (by_time_slice, by_day_type)
}

fn build_latent_demand_foundation(
    s: &Scenario,
    base_graph: &super::graph::Graph,
    clock_s: f64,
    temporal_context: Option<TemporalDemandSlice>,
    coverage_mode: LatentDemandCoverage,
) -> Result<LatentDemandBuild, String> {
    let economy_cfg = SyntheticEconomyConfig::default();
    let zone_count = s.world.zones.len();
    let purpose_shares = normalized_purpose_shares(&s.params);
    let purpose_shares6 = [
        (purpose_shares[0].max(0.0)) * economy_cfg.purpose_trip_rates.work.max(0.0),
        (purpose_shares[1].max(0.0)) * economy_cfg.purpose_trip_rates.education.max(0.0),
        (purpose_shares[2].max(0.0)) * economy_cfg.purpose_trip_rates.shopping.max(0.0),
        (purpose_shares[3].max(0.0)) * economy_cfg.purpose_trip_rates.leisure.max(0.0),
        (purpose_shares[4] * 0.65).max(0.0) * economy_cfg.purpose_trip_rates.essential.max(0.0),
        (purpose_shares[4] * 0.35).max(0.0) * economy_cfg.purpose_trip_rates.intercity.max(0.0),
    ];
    let zone_profiles = build_zone_demand_profiles(s, &economy_cfg);
    let mut active_context = temporal_context.unwrap_or(TemporalDemandSlice {
        service_day_type: ServiceDayType::Weekday,
        time_slice: demand_time_slice_at(clock_s),
        seasonal_profile: SeasonalProfile::Neutral,
        active_event_ids: Vec::new(),
    });
    if active_context.active_event_ids.is_empty() {
        active_context.active_event_ids =
            resolve_active_event_ids(&active_context, &economy_cfg, &zone_profiles);
    } else {
        active_context.active_event_ids =
            normalize_active_event_ids(&active_context.active_event_ids, &economy_cfg);
    }

    let mut contexts = match coverage_mode {
        LatentDemandCoverage::SingleContext => vec![active_context.clone()],
        LatentDemandCoverage::CanonicalSlicesForDay => DEMAND_SLICE_WINDOWS
            .iter()
            .map(|window| TemporalDemandSlice {
                service_day_type: active_context.service_day_type,
                time_slice: window.label,
                seasonal_profile: active_context.seasonal_profile,
                active_event_ids: Vec::new(),
            })
            .collect::<Vec<_>>(),
    };
    for ctx in &mut contexts {
        if ctx.active_event_ids.is_empty() {
            ctx.active_event_ids = resolve_active_event_ids(ctx, &economy_cfg, &zone_profiles);
        } else {
            ctx.active_event_ids = normalize_active_event_ids(&ctx.active_event_ids, &economy_cfg);
        }
    }

    let zone_coords = s.world.zones.iter().map(|z| (z.x, z.y)).collect::<Vec<_>>();

    // Use base generalized costs (without iterated crowding) as one component of impedance.
    // When no path exists in current network, we still generate latent demand from spatial
    // impedance so assignment can correctly report that demand as unserved.
    let mut dist_by_origin = Vec::with_capacity(zone_count);
    for oi in 0..zone_count {
        let origin_node = base_graph.svc_index.zone_nodes_start + oi;
        dist_by_origin.push(dijkstra(base_graph, origin_node));
    }

    let mut all_latent: Vec<LatentOdDemand> = Vec::new();
    let mut active_latent: Vec<ActiveLatentOd> = Vec::new();
    for context in &contexts {
        let is_active_context = context.time_slice == active_context.time_slice
            && context.service_day_type == active_context.service_day_type
            && context.seasonal_profile == active_context.seasonal_profile;
        let active_events =
            active_event_modifiers_for_context(context, &economy_cfg, &zone_profiles);
        for oi in 0..zone_count {
            let origin_zone = &s.world.zones[oi];
            let profile = &zone_profiles[oi];
            let produced_total =
                origin_zone.population.max(0.0) * s.params.trips_per_person.max(0.0);
            if produced_total <= 0.0 {
                continue;
            }

            for (purpose_idx, purpose) in TripPurpose::ALL.iter().enumerate() {
                let purpose_share = purpose_shares6[purpose_idx].max(0.0);
                if purpose_share <= 0.0 {
                    continue;
                }
                let mut produced = produced_total
                    * purpose_share
                    * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                    * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                    * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg)
                    * purpose_trip_modifier(&profile.trip_rate_modifiers, *purpose);
                produced *= origin_production_factor(profile, *purpose, &economy_cfg).max(0.0);

                // Keep rural/village zones alive with a persistent essential floor.
                if matches!(
                    profile.settlement_class,
                    SettlementClass::Village | SettlementClass::Rural
                ) && *purpose == TripPurpose::Essential
                {
                    let floor = profile.population.max(0.0)
                        * economy_cfg.rural_essential_demand_floor_per_person.max(0.0)
                        * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                        * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                        * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg);
                    produced = produced.max(floor);
                } else if matches!(
                    profile.settlement_class,
                    SettlementClass::Village | SettlementClass::Rural
                ) {
                    let floor = profile.population.max(0.0)
                        * economy_cfg.rural_baseline_trip_floor_per_person.max(0.0)
                        * 0.25
                        * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                        * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                        * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg);
                    produced = produced.max(floor);
                }

                if produced <= 0.0 {
                    continue;
                }

                let mut weights: Vec<(usize, f64, f64, Vec<String>)> = Vec::new();
                let mut base_wsum = 0.0_f64;
                let mut weighted_event_wsum = 0.0_f64;
                let mut wsum = 0.0_f64;
                for dj in 0..zone_count {
                    if oi == dj {
                        continue;
                    }
                    let attraction =
                        purpose_attraction(dj, *purpose, &zone_profiles, &economy_cfg).max(0.0);
                    if attraction <= 0.0 {
                        continue;
                    }
                    let dest_node = base_graph.svc_index.zone_nodes_start + zone_count + dj;
                    let gc_s = dist_by_origin[oi][dest_node].dist;
                    let dist_km = euclid_km(zone_coords[oi], zone_coords[dj]).max(0.01);
                    let gc_effective_s = if gc_s.is_finite() {
                        gc_s.max(1.0)
                    } else {
                        // Synthetic fallback if no viable path exists yet.
                        (dist_km / 40.0) * 3600.0 + 600.0
                    };
                    let impedance = purpose_impedance(
                        *purpose,
                        gc_effective_s,
                        dist_km,
                        s.params.gravity_beta.max(0.0),
                        &economy_cfg,
                    );
                    let corridor_mult =
                        corridor_bonus(oi, dj, *purpose, &zone_profiles, &economy_cfg, dist_km);
                    let service_center_mult =
                        rural_service_center_bias(profile, &zone_profiles[dj], *purpose);
                    let base_w = attraction * impedance * corridor_mult * service_center_mult;
                    if base_w <= 0.0 {
                        continue;
                    }
                    let (event_mult, event_ids) = pair_event_multiplier(
                        *purpose,
                        profile,
                        &zone_profiles[dj],
                        &active_events,
                        &economy_cfg,
                    );
                    let w = base_w * event_mult;
                    if w > 0.0 {
                        base_wsum += base_w;
                        weighted_event_wsum += base_w * event_mult;
                        wsum += w;
                        weights.push((dj, w, event_mult, event_ids));
                    }
                }
                if wsum <= 0.0 {
                    continue;
                }

                let produced_event_adjusted = if base_wsum > 0.0 {
                    let avg_event_mult = (weighted_event_wsum / base_wsum).clamp(0.25, 4.0);
                    produced * avg_event_mult
                } else {
                    produced
                };

                for (dj, w, _event_mult, event_ids) in weights {
                    let latent = (produced_event_adjusted * (w / wsum)).max(0.0);
                    if latent <= 0.0 {
                        continue;
                    }
                    let row = LatentOdDemand {
                        origin_zone_id: origin_zone.id.clone(),
                        destination_zone_id: s.world.zones[dj].id.clone(),
                        purpose: *purpose,
                        time_slice: context.time_slice,
                        service_day_type: Some(context.service_day_type),
                        seasonal_profile: Some(context.seasonal_profile),
                        active_event_ids: event_ids.clone(),
                        latent_passengers: latent,
                    };
                    if is_active_context {
                        active_latent.push(ActiveLatentOd {
                            origin_idx: oi,
                            destination_idx: dj,
                            origin_zone_id: row.origin_zone_id.clone(),
                            destination_zone_id: row.destination_zone_id.clone(),
                            purpose: row.purpose,
                            time_slice: row.time_slice,
                            service_day_type: context.service_day_type,
                            seasonal_profile: context.seasonal_profile,
                            active_event_ids: event_ids,
                            latent_passengers: row.latent_passengers,
                        });
                    }
                    all_latent.push(row);
                }
            }
        }
    }

    Ok(LatentDemandBuild {
        economy_config: economy_cfg,
        zone_profiles,
        all_latent,
        active_latent,
        active_context,
    })
}

fn apply_mode_choice_capture(
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

fn demand_time_slice_at(clock_s: f64) -> DemandTimeSliceLabel {
    let day_s = 86_400.0;
    let mut t = clock_s % day_s;
    if t < 0.0 {
        t += day_s;
    }
    for window in DEMAND_SLICE_WINDOWS {
        if window.start_s <= window.end_s {
            if t >= window.start_s && t < window.end_s {
                return window.label;
            }
        } else if t >= window.start_s || t < window.end_s {
            return window.label;
        }
    }
    DemandTimeSliceLabel::Interpeak
}

fn normalize_active_event_ids(
    active_event_ids: &[String],
    cfg: &SyntheticEconomyConfig,
) -> Vec<String> {
    let mut dedup = std::collections::BTreeSet::<String>::new();
    for id in active_event_ids {
        if cfg.event_demand_modifiers.iter().any(|x| x.event_id == *id) {
            dedup.insert(id.clone());
        }
    }
    dedup.into_iter().collect::<Vec<_>>()
}

fn resolve_active_event_ids(
    context: &TemporalDemandSlice,
    cfg: &SyntheticEconomyConfig,
    zone_profiles: &[ZoneDemandProfile],
) -> Vec<String> {
    let explicit = normalize_active_event_ids(&context.active_event_ids, cfg);
    let use_explicit = !explicit.is_empty();

    let mut ids = Vec::<String>::new();
    for event in &cfg.event_demand_modifiers {
        if use_explicit {
            if !explicit.iter().any(|id| id == &event.event_id) {
                continue;
            }
        } else {
            if !event.applies_day_types.is_empty()
                && !event.applies_day_types.contains(&context.service_day_type)
            {
                continue;
            }
            if !event.applies_time_slices.is_empty()
                && !event.applies_time_slices.contains(&context.time_slice)
            {
                continue;
            }
            if !event.applies_seasonal_profiles.is_empty()
                && !event
                    .applies_seasonal_profiles
                    .contains(&context.seasonal_profile)
            {
                continue;
            }
        }

        if let Some(attractor) = event.attractor_type {
            let exists = zone_profiles
                .iter()
                .any(|z| z.special_attractors.contains(&attractor));
            if !exists {
                continue;
            }
        }

        ids.push(event.event_id.clone());
    }
    ids
}

fn active_event_modifiers_for_context<'a>(
    context: &'a TemporalDemandSlice,
    cfg: &'a SyntheticEconomyConfig,
    zone_profiles: &'a [ZoneDemandProfile],
) -> Vec<&'a EventDemandModifier> {
    let active_ids = resolve_active_event_ids(context, cfg, zone_profiles);
    cfg.event_demand_modifiers
        .iter()
        .filter(|event| active_ids.iter().any(|id| id == &event.event_id))
        .collect::<Vec<_>>()
}

fn pair_event_multiplier(
    purpose: TripPurpose,
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    active_events: &[&EventDemandModifier],
    cfg: &SyntheticEconomyConfig,
) -> (f64, Vec<String>) {
    let mut mult = 1.0_f64;
    let mut applied_ids = Vec::<String>::new();

    for event in active_events {
        let applies_to_pair = if let Some(attractor) = event.attractor_type {
            origin.special_attractors.contains(&attractor)
                || destination.special_attractors.contains(&attractor)
        } else {
            true
        };
        if !applies_to_pair {
            continue;
        }

        let base = match purpose {
            TripPurpose::Work => event.purpose_multipliers.work,
            TripPurpose::Education => event.purpose_multipliers.education,
            TripPurpose::Shopping => event.purpose_multipliers.shopping,
            TripPurpose::Leisure => event.purpose_multipliers.leisure,
            TripPurpose::Essential => event.purpose_multipliers.essential,
            TripPurpose::Intercity => event.purpose_multipliers.intercity,
        }
        .max(0.05);
        let scaled = 1.0
            + ((base - 1.0)
                * event.intensity.max(0.0)
                * cfg.event_modifier_strength_scale.max(0.0));
        mult *= scaled.clamp(0.1, 6.0);
        applied_ids.push(event.event_id.clone());
    }

    (mult.clamp(0.1, 8.0), applied_ids)
}

fn day_type_multiplier(
    service_day_type: ServiceDayType,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .day_type_purpose_multipliers
        .iter()
        .find(|x| x.service_day_type == service_day_type)
    {
        let m = match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        };
        return m.max(0.0);
    }

    match (service_day_type, purpose) {
        (ServiceDayType::Weekday, TripPurpose::Work) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Education) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Shopping) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Leisure) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Essential) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Intercity) => 1.0,
        (ServiceDayType::Saturday, TripPurpose::Work) => 0.55,
        (ServiceDayType::Saturday, TripPurpose::Education) => 0.60,
        (ServiceDayType::Saturday, TripPurpose::Shopping) => 1.25,
        (ServiceDayType::Saturday, TripPurpose::Leisure) => 1.20,
        (ServiceDayType::Saturday, TripPurpose::Essential) => 0.95,
        (ServiceDayType::Saturday, TripPurpose::Intercity) => 1.05,
        (ServiceDayType::SundayHoliday, TripPurpose::Work) => 0.35,
        (ServiceDayType::SundayHoliday, TripPurpose::Education) => 0.35,
        (ServiceDayType::SundayHoliday, TripPurpose::Shopping) => 1.05,
        (ServiceDayType::SundayHoliday, TripPurpose::Leisure) => 1.18,
        (ServiceDayType::SundayHoliday, TripPurpose::Essential) => 0.92,
        (ServiceDayType::SundayHoliday, TripPurpose::Intercity) => 1.10,
    }
}

fn seasonal_multiplier(
    seasonal_profile: SeasonalProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .seasonal_purpose_multipliers
        .iter()
        .find(|x| x.seasonal_profile == seasonal_profile)
    {
        let m = match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        };
        return m.max(0.0);
    }

    match (seasonal_profile, purpose) {
        (SeasonalProfile::Neutral, _) => 1.0,
        (SeasonalProfile::SummerPeak, TripPurpose::Work) => 0.95,
        (SeasonalProfile::SummerPeak, TripPurpose::Education) => 0.92,
        (SeasonalProfile::SummerPeak, TripPurpose::Shopping) => 1.10,
        (SeasonalProfile::SummerPeak, TripPurpose::Leisure) => 1.25,
        (SeasonalProfile::SummerPeak, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::SummerPeak, TripPurpose::Intercity) => 1.18,
        (SeasonalProfile::WinterPeak, TripPurpose::Work) => 1.05,
        (SeasonalProfile::WinterPeak, TripPurpose::Education) => 1.00,
        (SeasonalProfile::WinterPeak, TripPurpose::Shopping) => 0.96,
        (SeasonalProfile::WinterPeak, TripPurpose::Leisure) => 0.90,
        (SeasonalProfile::WinterPeak, TripPurpose::Essential) => 1.08,
        (SeasonalProfile::WinterPeak, TripPurpose::Intercity) => 0.96,
        (SeasonalProfile::TermTime, TripPurpose::Work) => 1.02,
        (SeasonalProfile::TermTime, TripPurpose::Education) => 1.35,
        (SeasonalProfile::TermTime, TripPurpose::Shopping) => 0.95,
        (SeasonalProfile::TermTime, TripPurpose::Leisure) => 0.92,
        (SeasonalProfile::TermTime, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::TermTime, TripPurpose::Intercity) => 0.98,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Work) => 0.88,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Education) => 0.45,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Shopping) => 1.12,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Leisure) => 1.25,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Intercity) => 1.28,
    }
}

fn time_slice_multiplier(
    slice: DemandTimeSliceLabel,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .time_slice_purpose_multipliers
        .iter()
        .find(|x| x.time_slice == slice)
    {
        return match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        }
        .max(0.0);
    }

    // Fallback keeps the generator robust if config entries are partially omitted.
    match (slice, purpose) {
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Work) => 0.60,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Education) => 0.55,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Shopping) => 0.35,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Leisure) => 0.30,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Essential) => 0.70,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Intercity) => 0.80,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Work) => 1.65,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Education) => 1.35,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Shopping) => 0.75,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Leisure) => 0.60,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Essential) => 0.95,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Intercity) => 0.85,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Work) => 0.85,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Education) => 0.95,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Shopping) => 1.05,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Leisure) => 1.00,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Essential) => 1.00,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Intercity) => 1.00,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Work) => 1.30,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Education) => 0.90,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Shopping) => 1.20,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Leisure) => 1.10,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Essential) => 1.00,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Intercity) => 1.15,
        (DemandTimeSliceLabel::Evening, TripPurpose::Work) => 0.40,
        (DemandTimeSliceLabel::Evening, TripPurpose::Education) => 0.35,
        (DemandTimeSliceLabel::Evening, TripPurpose::Shopping) => 1.05,
        (DemandTimeSliceLabel::Evening, TripPurpose::Leisure) => 1.35,
        (DemandTimeSliceLabel::Evening, TripPurpose::Essential) => 0.95,
        (DemandTimeSliceLabel::Evening, TripPurpose::Intercity) => 1.10,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Work) => 0.12,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Education) => 0.18,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Shopping) => 0.22,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Leisure) => 0.68,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Essential) => 0.72,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Intercity) => 0.58,
    }
}

fn purpose_trip_modifier(
    modifiers: &super::types::PurposeTripRateModifiers,
    purpose: TripPurpose,
) -> f64 {
    let raw = match purpose {
        TripPurpose::Work => modifiers.work,
        TripPurpose::Education => modifiers.education,
        TripPurpose::Shopping => modifiers.shopping,
        TripPurpose::Leisure => modifiers.leisure,
        TripPurpose::Essential => modifiers.essential,
        TripPurpose::Intercity => modifiers.intercity,
    };
    raw.max(0.1)
}

fn purpose_impedance(
    purpose: TripPurpose,
    gc_s: f64,
    dist_km: f64,
    gravity_beta: f64,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let (purpose_gc_beta, purpose_dist_beta) = match purpose {
        TripPurpose::Work => (
            cfg.purpose_gc_decay_beta.work.max(0.0),
            cfg.purpose_distance_decay_beta.work.max(0.0),
        ),
        TripPurpose::Education => (
            cfg.purpose_gc_decay_beta.education.max(0.0),
            cfg.purpose_distance_decay_beta.education.max(0.0),
        ),
        TripPurpose::Shopping => (
            cfg.purpose_gc_decay_beta.shopping.max(0.0),
            cfg.purpose_distance_decay_beta.shopping.max(0.0),
        ),
        TripPurpose::Leisure => (
            cfg.purpose_gc_decay_beta.leisure.max(0.0),
            cfg.purpose_distance_decay_beta.leisure.max(0.0),
        ),
        TripPurpose::Essential => (
            cfg.purpose_gc_decay_beta.essential.max(0.0),
            cfg.purpose_distance_decay_beta.essential.max(0.0),
        ),
        TripPurpose::Intercity => (
            cfg.purpose_gc_decay_beta.intercity.max(0.0),
            cfg.purpose_distance_decay_beta.intercity.max(0.0),
        ),
    };
    let beta_gc = (gravity_beta + purpose_gc_beta).max(0.0);
    let gc_term = (-beta_gc * gc_s.max(0.0)).exp();
    let dist_term = (-purpose_dist_beta * dist_km.max(0.0)).exp();
    (gc_term * dist_term).max(0.0)
}

fn purpose_attraction(
    zone_idx: usize,
    purpose: TripPurpose,
    zone_profiles: &[ZoneDemandProfile],
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let z = &zone_profiles[zone_idx];
    let base = match purpose {
        TripPurpose::Work => z.work_attractiveness,
        TripPurpose::Education => z.education_attractiveness,
        TripPurpose::Shopping => z.shopping_attractiveness,
        TripPurpose::Leisure => z.leisure_attractiveness,
        TripPurpose::Essential => z.essential_service_attractiveness,
        TripPurpose::Intercity => z.intercity_importance,
    }
    .max(0.0);
    let settlement_mult = settlement_purpose_multiplier(z.settlement_class, purpose, cfg);
    let anchor_mult = zone_attractor_multiplier(z, purpose, cfg);
    (base * settlement_mult * anchor_mult).max(0.0)
}

fn origin_production_factor(
    origin: &ZoneDemandProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let residential_bias = match origin.archetype {
        ZoneArchetype::InnerResidential => 1.15,
        ZoneArchetype::OuterSuburb => 1.25,
        ZoneArchetype::VillageCentre => 1.10,
        ZoneArchetype::RuralResidential => 1.08,
        ZoneArchetype::RuralAgricultural => 0.95,
        ZoneArchetype::Cbd => 0.65,
        _ => 1.0,
    };
    let purpose_factor = match purpose {
        TripPurpose::Work => 0.75 + 0.25 * residential_bias + 0.30 * origin.transit_affinity,
        TripPurpose::Education => {
            0.70 + 0.25 * residential_bias + 0.20 * origin.education_intensity
        }
        TripPurpose::Shopping => {
            0.62 + 0.20 * residential_bias + 0.10 * (1.0 - origin.car_dependency)
        }
        TripPurpose::Leisure => 0.58 + 0.20 * residential_bias + 0.22 * origin.tourism_score,
        TripPurpose::Essential => {
            0.48 + 0.18 * residential_bias + 0.20 * (1.0 - origin.car_dependency)
        }
        TripPurpose::Intercity => {
            0.35 + 0.20 * origin.regional_importance + 0.15 * origin.centrality_score
        }
    };
    let settlement_mult = settlement_purpose_multiplier(origin.settlement_class, purpose, cfg);
    let transit_factor = (0.35 + origin.transit_affinity).clamp(0.2, 1.35);
    let car_penalty = (1.10 - 0.45 * origin.car_dependency).clamp(0.45, 1.10);
    (purpose_factor * settlement_mult * transit_factor * car_penalty).max(0.05)
}

fn corridor_bonus(
    oi: usize,
    dj: usize,
    purpose: TripPurpose,
    profiles: &[ZoneDemandProfile],
    cfg: &SyntheticEconomyConfig,
    dist_km: f64,
) -> f64 {
    let o = &profiles[oi];
    let d = &profiles[dj];
    let mut bonus = 1.0_f64;
    let o_rank = settlement_rank(o.settlement_class);
    let d_rank = settlement_rank(d.settlement_class);

    let both_major = o_rank >= settlement_rank(SettlementClass::MajorCity)
        && d_rank >= settlement_rank(SettlementClass::MajorCity);
    if both_major && matches!(purpose, TripPurpose::Work | TripPurpose::Intercity) {
        bonus *= cfg.corridor_bonus_major_major.max(1.0);
    }

    let commuter_origin = matches!(
        o.archetype,
        ZoneArchetype::OuterSuburb
            | ZoneArchetype::InnerResidential
            | ZoneArchetype::VillageCentre
            | ZoneArchetype::RuralResidential
    ) || matches!(
        o.settlement_class,
        SettlementClass::LargeTown
            | SettlementClass::SmallTown
            | SettlementClass::Village
            | SettlementClass::Rural
    );
    let commuter_dest = d_rank >= settlement_rank(SettlementClass::RegionalCity);
    if purpose == TripPurpose::Work && commuter_origin && commuter_dest && dist_km <= 90.0 {
        bonus *= cfg.corridor_bonus_commuter.max(1.0);
    }

    let airport_core = (o
        .special_attractors
        .contains(&SpecialAttractorType::Airport)
        && (d.archetype == ZoneArchetype::Cbd
            || d_rank >= settlement_rank(SettlementClass::MajorCity)))
        || (d
            .special_attractors
            .contains(&SpecialAttractorType::Airport)
            && (o.archetype == ZoneArchetype::Cbd
                || o_rank >= settlement_rank(SettlementClass::MajorCity)));
    if airport_core
        && matches!(
            purpose,
            TripPurpose::Intercity | TripPurpose::Work | TripPurpose::Leisure
        )
    {
        bonus *= cfg.corridor_bonus_airport_core.max(1.0);
    }

    let regional_link = matches!(
        o.settlement_class,
        SettlementClass::SmallTown | SettlementClass::Village | SettlementClass::Rural
    ) && d_rank >= settlement_rank(SettlementClass::RegionalCity);
    if regional_link
        && matches!(
            purpose,
            TripPurpose::Essential | TripPurpose::Intercity | TripPurpose::Work
        )
    {
        bonus *= cfg.corridor_bonus_regional_link.max(1.0);
    }

    bonus.clamp(0.2, 5.0)
}

fn rural_service_center_bias(
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    purpose: TripPurpose,
) -> f64 {
    if !matches!(
        origin.settlement_class,
        SettlementClass::Village | SettlementClass::Rural
    ) {
        return 1.0;
    }
    if purpose == TripPurpose::Essential {
        if origin.nearest_service_centre_id.as_deref() == Some(destination.zone_id.as_str()) {
            return 1.35;
        }
        return 0.48;
    }
    if matches!(purpose, TripPurpose::Shopping | TripPurpose::Education)
        && origin.nearest_service_centre_id.as_deref() == Some(destination.zone_id.as_str())
    {
        return 1.20;
    }
    1.0
}

fn build_zone_demand_profiles(
    s: &Scenario,
    cfg: &SyntheticEconomyConfig,
) -> Vec<ZoneDemandProfile> {
    let by_cell = s
        .world
        .demand_cells
        .iter()
        .map(|c| (c.cell_id.as_str(), c))
        .collect::<HashMap<_, _>>();
    let max_pop = s
        .world
        .zones
        .iter()
        .map(|z| z.population.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_jobs = s
        .world
        .zones
        .iter()
        .map(|z| z.jobs.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_mass = s
        .world
        .zones
        .iter()
        .map(|z| z.population.max(0.0) + z.jobs.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut profiles = s
        .world
        .zones
        .iter()
        .map(|z| {
            let pop = z.population.max(0.0);
            let jobs = z.jobs.max(0.0);
            let mut activity_mix = [0.52, 0.18, 0.12, 0.08, 0.06, 0.03, 0.01];
            let mut centrality_score = (jobs / (pop + jobs + 1.0)).clamp(0.0, 1.0);
            let area_m2 = if let Some(cell) = by_cell.get(z.id.as_str()) {
                activity_mix = normalize_activity_mix([
                    cell.activity_mix_residential,
                    cell.activity_mix_office,
                    cell.activity_mix_retail,
                    cell.activity_mix_recreation,
                    cell.activity_mix_industrial,
                    cell.activity_mix_education,
                    cell.activity_mix_health,
                ]);
                centrality_score = cell.centrality_score.clamp(0.0, 1.0);
                cell.area_m2.max(1_000_000.0)
            } else {
                1_000_000.0
            };

            let area_km2 = (area_m2 / 1_000_000.0).max(0.05);
            let population_density = pop / area_km2;
            let employment_density = jobs / area_km2;
            let retail_intensity =
                (activity_mix[2] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let leisure_intensity =
                (activity_mix[3] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let education_intensity =
                (activity_mix[5] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let industry_intensity =
                (activity_mix[4] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let pop_norm = pop / (max_pop + 1.0);
            let jobs_norm = jobs / (max_jobs + 1.0);
            let centrality_term =
                (centrality_score * cfg.centrality_weight.max(0.05)).clamp(0.0, 2.0);
            let regional_importance =
                ((0.46 * jobs_norm + 0.26 * pop_norm + 0.28 * centrality_term)
                    * cfg.regional_importance_weight.max(0.05))
                .clamp(0.0, 2.0);
            let regional_term =
                (regional_importance * cfg.regional_importance_weight.max(0.05)).clamp(0.0, 2.0);
            let tourism_score =
                (0.45 * leisure_intensity + 0.25 * retail_intensity + 0.30 * centrality_term)
                    .clamp(0.0, 1.5);

            let archetype = classify_zone_archetype(
                z.id.as_str(),
                activity_mix,
                centrality_score,
                population_density,
                employment_density,
                pop,
                jobs,
            );
            let settlement_class =
                classify_settlement_class(pop + jobs, centrality_score, archetype);
            let special_attractors = infer_special_attractors(
                z.id.as_str(),
                archetype,
                settlement_class,
                activity_mix,
                centrality_score,
                tourism_score,
            );

            let trait_cfg = archetype_trait(archetype, cfg);
            let settlement_work =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Work, cfg);
            let settlement_education =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Education, cfg);
            let settlement_shopping =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Shopping, cfg);
            let settlement_leisure =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Leisure, cfg);
            let settlement_essential =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Essential, cfg);
            let settlement_intercity =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Intercity, cfg);

            let base_transit = match settlement_class {
                SettlementClass::GlobalCityCore => 0.92,
                SettlementClass::MajorCity => 0.85,
                SettlementClass::RegionalCity => 0.74,
                SettlementClass::LargeTown => 0.63,
                SettlementClass::SmallTown => 0.53,
                SettlementClass::Village => 0.42,
                SettlementClass::Rural => 0.30,
                SettlementClass::SpecialNode => 0.68,
            };
            let transit_affinity =
                (base_transit + 0.18 * centrality_score + 0.08 * trait_cfg.centrality_weight)
                    .clamp(0.05, 0.99);
            let car_dependency = (1.0 - transit_affinity + 0.12).clamp(0.01, 0.99);

            let work_attractiveness = ((0.52 * jobs_norm
                + 0.24 * (employment_density / (employment_density + 8000.0))
                + 0.16 * centrality_term
                + 0.08 * regional_term)
                * settlement_work
                * (0.8 + trait_cfg.employment_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Work, cfg))
            .max(0.02);
            let education_attractiveness = ((0.42 * education_intensity
                + 0.24 * centrality_term
                + 0.16 * pop_norm
                + 0.18 * regional_term)
                * settlement_education
                * (0.7 + trait_cfg.education_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Education, cfg))
            .max(0.02);
            let shopping_attractiveness = ((0.46 * retail_intensity
                + 0.22 * centrality_term
                + 0.16 * pop_norm
                + 0.16 * regional_term)
                * settlement_shopping
                * (0.7 + trait_cfg.retail_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Shopping, cfg))
            .max(0.02);
            let leisure_attractiveness = ((0.38 * leisure_intensity
                + 0.30 * tourism_score
                + 0.14 * centrality_term
                + 0.18 * regional_term)
                * settlement_leisure
                * (0.65 + trait_cfg.leisure_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Leisure, cfg))
            .max(0.02);
            let essential_service_attractiveness = ((0.42 * pop_norm
                + 0.20 * education_intensity
                + 0.14 * centrality_term
                + 0.24 * regional_term)
                * settlement_essential
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Essential, cfg))
            .max(0.02);
            let intercity_importance = ((0.30 * jobs_norm
                + 0.30 * centrality_term
                + 0.40 * regional_term)
                * settlement_intercity
                * (0.7 + trait_cfg.centrality_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Intercity, cfg))
            .max(0.02);

            let trip_rate_modifiers = super::types::PurposeTripRateModifiers {
                work: (0.68
                    + 0.20 * transit_affinity
                    + 0.24 * settlement_work
                    + 0.12 * trait_cfg.residential_weight)
                    .clamp(0.15, 2.0),
                education: (0.64
                    + 0.20 * transit_affinity
                    + 0.24 * settlement_education
                    + 0.10 * education_intensity)
                    .clamp(0.15, 2.0),
                shopping: (0.58
                    + 0.18 * (1.0 - car_dependency)
                    + 0.24 * settlement_shopping
                    + 0.10 * retail_intensity)
                    .clamp(0.15, 2.0),
                leisure: (0.52
                    + 0.20 * tourism_score
                    + 0.18 * settlement_leisure
                    + 0.12 * leisure_intensity)
                    .clamp(0.15, 2.0),
                essential: (0.48 + 0.24 * settlement_essential + 0.22 * (1.0 - car_dependency))
                    .clamp(0.15, 2.0),
                intercity: (0.30 + 0.52 * intercity_importance + 0.18 * settlement_intercity)
                    .clamp(0.15, 2.0),
            };

            ZoneDemandProfile {
                zone_id: z.id.clone(),
                population: pop,
                jobs,
                archetype,
                settlement_class,
                population_density,
                employment_density,
                retail_intensity,
                leisure_intensity,
                education_intensity,
                industry_intensity,
                centrality_score,
                regional_importance,
                tourism_score,
                car_dependency,
                transit_affinity,
                nearest_service_centre_id: None,
                special_attractors,
                trip_rate_modifiers,
                work_attractiveness,
                education_attractiveness,
                shopping_attractiveness,
                leisure_attractiveness,
                essential_service_attractiveness,
                intercity_importance,
            }
        })
        .collect::<Vec<_>>();

    // Find nearest service centre for each zone (town centre, city core, or special node).
    let mut service_centres: Vec<usize> = Vec::new();
    for (idx, p) in profiles.iter().enumerate() {
        if is_service_centre(p) {
            service_centres.push(idx);
        }
    }
    if service_centres.is_empty() && !profiles.is_empty() {
        let mut best_idx = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (idx, p) in profiles.iter().enumerate() {
            let score = p.essential_service_attractiveness + p.regional_importance;
            if score > best_score {
                best_score = score;
                best_idx = idx;
            }
        }
        service_centres.push(best_idx);
    }
    for i in 0..profiles.len() {
        let mut best: Option<(usize, f64)> = None;
        for j in &service_centres {
            let dist = euclid_m(
                (s.world.zones[i].x, s.world.zones[i].y),
                (s.world.zones[*j].x, s.world.zones[*j].y),
            );
            if let Some((_, best_dist)) = best {
                if dist < best_dist {
                    best = Some((*j, dist));
                }
            } else {
                best = Some((*j, dist));
            }
        }
        profiles[i].nearest_service_centre_id = best.map(|(idx, _)| profiles[idx].zone_id.clone());
    }

    profiles
}

fn settlement_rank(class: SettlementClass) -> i32 {
    match class {
        SettlementClass::GlobalCityCore => 8,
        SettlementClass::MajorCity => 7,
        SettlementClass::RegionalCity => 6,
        SettlementClass::SpecialNode => 5,
        SettlementClass::LargeTown => 4,
        SettlementClass::SmallTown => 3,
        SettlementClass::Village => 2,
        SettlementClass::Rural => 1,
    }
}

fn classify_settlement_class(
    settlement_mass: f64,
    centrality_score: f64,
    archetype: ZoneArchetype,
) -> SettlementClass {
    if matches!(
        archetype,
        ZoneArchetype::AirportZone | ZoneArchetype::PortLogisticsZone
    ) && settlement_mass >= 6_000.0
    {
        return SettlementClass::SpecialNode;
    }
    if centrality_score >= 0.92 && settlement_mass >= 80_000.0 {
        return SettlementClass::GlobalCityCore;
    }
    if settlement_mass >= 45_000.0 || (centrality_score >= 0.84 && settlement_mass >= 30_000.0) {
        return SettlementClass::MajorCity;
    }
    if settlement_mass >= 20_000.0 || (centrality_score >= 0.70 && settlement_mass >= 12_000.0) {
        return SettlementClass::RegionalCity;
    }
    if settlement_mass >= 8_000.0 {
        return SettlementClass::LargeTown;
    }
    if settlement_mass >= 3_500.0 {
        return SettlementClass::SmallTown;
    }
    if settlement_mass >= 1_200.0 {
        return SettlementClass::Village;
    }
    SettlementClass::Rural
}

fn classify_zone_archetype(
    zone_id: &str,
    mix: [f64; 7],
    centrality_score: f64,
    population_density: f64,
    employment_density: f64,
    population: f64,
    jobs: f64,
) -> ZoneArchetype {
    let id = zone_id.to_ascii_lowercase();
    if id.contains("airport") || id.contains("airfield") {
        return ZoneArchetype::AirportZone;
    }
    if id.contains("port") || id.contains("harbour") || id.contains("harbor") {
        return ZoneArchetype::PortLogisticsZone;
    }
    if mix[5] >= 0.26 {
        return ZoneArchetype::UniversityDistrict;
    }
    if centrality_score >= 0.90 && mix[1] >= 0.28 && jobs >= population * 1.1 {
        return ZoneArchetype::Cbd;
    }
    if mix[4] >= 0.35 {
        return ZoneArchetype::IndustrialEstate;
    }
    if mix[1] >= 0.33 && centrality_score >= 0.55 {
        return ZoneArchetype::BusinessPark;
    }
    if (mix[2] + mix[3]) >= 0.40 && centrality_score >= 0.55 {
        return ZoneArchetype::RetailLeisureDistrict;
    }
    if mix[2] >= 0.22 && centrality_score >= 0.45 {
        return ZoneArchetype::TownCentre;
    }
    if population_density >= 5_500.0 && mix[0] >= 0.45 {
        return ZoneArchetype::InnerResidential;
    }
    if population_density >= 2_500.0 && mix[0] >= 0.42 {
        return ZoneArchetype::OuterSuburb;
    }
    if population_density >= 900.0 && mix[0] >= 0.40 {
        return ZoneArchetype::VillageCentre;
    }
    if mix[0] >= 0.40 || employment_density >= 350.0 {
        return ZoneArchetype::RuralResidential;
    }
    ZoneArchetype::RuralAgricultural
}

fn infer_special_attractors(
    zone_id: &str,
    archetype: ZoneArchetype,
    settlement_class: SettlementClass,
    mix: [f64; 7],
    centrality_score: f64,
    tourism_score: f64,
) -> Vec<SpecialAttractorType> {
    let id = zone_id.to_ascii_lowercase();
    let mut attractors: Vec<SpecialAttractorType> = Vec::new();
    if matches!(archetype, ZoneArchetype::AirportZone) {
        attractors.push(SpecialAttractorType::Airport);
    }
    if matches!(archetype, ZoneArchetype::PortLogisticsZone) {
        attractors.push(SpecialAttractorType::Port);
        attractors.push(SpecialAttractorType::LogisticsHub);
    }
    if matches!(archetype, ZoneArchetype::UniversityDistrict) || mix[5] >= 0.30 {
        attractors.push(SpecialAttractorType::University);
    }
    if mix[6] >= 0.16 || id.contains("hospital") || id.contains("medical") {
        attractors.push(SpecialAttractorType::Hospital);
    }
    if mix[3] >= 0.38 || id.contains("stadium") {
        attractors.push(SpecialAttractorType::Stadium);
    }
    if tourism_score >= 0.65 || id.contains("tour") {
        attractors.push(SpecialAttractorType::TourismLandmark);
    }
    if matches!(archetype, ZoneArchetype::Cbd)
        && settlement_rank(settlement_class) >= settlement_rank(SettlementClass::RegionalCity)
    {
        attractors.push(SpecialAttractorType::GovernmentCentre);
    }
    if mix[4] >= 0.30 && centrality_score >= 0.50 {
        attractors.push(SpecialAttractorType::LogisticsHub);
    }

    attractors.sort_unstable();
    attractors.dedup();
    attractors
}

fn archetype_trait(
    archetype: ZoneArchetype,
    cfg: &SyntheticEconomyConfig,
) -> super::types::ArchetypeTraitConfig {
    if let Some(found) = cfg
        .archetype_traits
        .iter()
        .find(|x| x.archetype == archetype)
    {
        return found.clone();
    }
    super::types::ArchetypeTraitConfig {
        archetype,
        residential_weight: 0.9,
        employment_weight: 0.9,
        retail_weight: 0.9,
        leisure_weight: 0.9,
        education_weight: 0.9,
        industry_weight: 0.9,
        centrality_weight: 0.9,
    }
}

fn settlement_purpose_multiplier(
    class: SettlementClass,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let record = cfg
        .settlement_class_multipliers
        .iter()
        .find(|x| x.settlement_class == class);
    let m = if let Some(v) = record {
        match purpose {
            TripPurpose::Work => v.work,
            TripPurpose::Education => v.education,
            TripPurpose::Shopping => v.shopping,
            TripPurpose::Leisure => v.leisure,
            TripPurpose::Essential => v.essential,
            TripPurpose::Intercity => v.intercity,
        }
    } else {
        1.0
    };
    m.max(0.05)
}

fn zone_attractor_multiplier_raw(
    attractors: &[SpecialAttractorType],
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let mut mult = 1.0_f64;
    for attractor in attractors {
        if let Some(record) = cfg
            .attractor_strength_multipliers
            .iter()
            .find(|x| x.attractor_type == *attractor)
        {
            let m = match purpose {
                TripPurpose::Work => record.work,
                TripPurpose::Education => record.education,
                TripPurpose::Shopping => record.shopping,
                TripPurpose::Leisure => record.leisure,
                TripPurpose::Essential => record.essential,
                TripPurpose::Intercity => record.intercity,
            };
            mult *= m.max(0.05);
        }
    }
    mult.clamp(0.2, 8.0)
}

fn zone_attractor_multiplier(
    zone: &ZoneDemandProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    zone_attractor_multiplier_raw(&zone.special_attractors, purpose, cfg)
}

fn is_service_centre(zone: &ZoneDemandProfile) -> bool {
    if matches!(
        zone.settlement_class,
        SettlementClass::GlobalCityCore
            | SettlementClass::MajorCity
            | SettlementClass::RegionalCity
            | SettlementClass::LargeTown
            | SettlementClass::SpecialNode
    ) {
        return true;
    }
    matches!(
        zone.archetype,
        ZoneArchetype::Cbd | ZoneArchetype::TownCentre
    ) || zone.special_attractors.iter().any(|a| {
        matches!(
            a,
            SpecialAttractorType::Hospital | SpecialAttractorType::GovernmentCentre
        )
    })
}

fn euclid_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

fn euclid_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    euclid_m(a, b) / 1000.0
}

fn build_modal_outputs(
    s: &Scenario,
    zone_profiles: &[ZoneDemandProfile],
    mode_choice_results: &[ModeChoiceResult],
    assigned_od_flows: &[AssignedOdFlow],
    stop_flow_states: &[StopFlowState],
    vehicle_load_states: &[VehicleLoadState],
    service_load_layer: &[ServiceLoadLayerData],
    active_context: &TemporalDemandSlice,
) -> ModalOutputs {
    #[derive(Debug, Clone, Default)]
    struct ZoneModeAgg {
        latent: f64,
        transit: f64,
        car: f64,
        walk: f64,
        suppressed: f64,
        by_purpose: [(f64, f64, f64, f64, f64); 6],
        transfer_penalty_s: f64,
        crowding_penalty_s: f64,
        parking_proxy_base: f64,
    }

    #[derive(Debug, Clone, Default)]
    struct CorridorModeAgg {
        latent: f64,
        transit: f64,
        car: f64,
        walk: f64,
        suppressed: f64,
        by_purpose_latent: [f64; 6],
        submode_pax: HashMap<TravelMode, f64>,
    }

    let mut out = ModalOutputs::default();
    out.mode_choice_results = mode_choice_results.to_vec();
    if zone_profiles.is_empty() || mode_choice_results.is_empty() {
        return out;
    }

    let profile_by_id = zone_profiles
        .iter()
        .map(|p| (p.zone_id.clone(), p))
        .collect::<HashMap<_, _>>();

    let mut zone_agg = HashMap::<String, ZoneModeAgg>::new();
    let mut corridor_agg = HashMap::<(String, String), CorridorModeAgg>::new();

    let mut total_latent = 0.0_f64;
    let mut total_transit = 0.0_f64;
    let mut total_car = 0.0_f64;
    let mut total_walk = 0.0_f64;
    let mut total_suppressed = 0.0_f64;
    let mut purpose_totals = [(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); 6];
    let mut urban_latent = 0.0_f64;
    let mut urban_transit = 0.0_f64;
    let mut rural_latent = 0.0_f64;
    let mut rural_transit = 0.0_f64;
    let mut intercity_latent = 0.0_f64;
    let mut intercity_transit = 0.0_f64;
    let mut airport_latent = 0.0_f64;
    let mut airport_transit = 0.0_f64;

    let mut lost_due_crowding = 0.0_f64;
    let mut lost_due_fare = 0.0_f64;
    let mut lost_due_indirectness = 0.0_f64;
    let mut lost_due_reliability = 0.0_f64;

    for result in mode_choice_results {
        let latent = result.latent_passengers.max(0.0);
        if latent <= 0.0 {
            continue;
        }
        let purpose_idx = phase3::purpose_index(result.context.purpose);
        let transit = result.transit_captured_passengers.max(0.0);
        let car = result.car_captured_passengers.max(0.0);
        let walk = result.walk_captured_passengers.max(0.0);
        let suppressed = result.suppressed_or_no_trip_passengers.max(0.0);
        let lost = (latent - transit).max(0.0);

        total_latent += latent;
        total_transit += transit;
        total_car += car;
        total_walk += walk;
        total_suppressed += suppressed;
        purpose_totals[purpose_idx].0 += latent;
        purpose_totals[purpose_idx].1 += transit;
        purpose_totals[purpose_idx].2 += car;
        purpose_totals[purpose_idx].3 += walk;
        purpose_totals[purpose_idx].4 += suppressed;

        if matches!(
            result.context.origin_settlement_class,
            SettlementClass::GlobalCityCore
                | SettlementClass::MajorCity
                | SettlementClass::RegionalCity
                | SettlementClass::LargeTown
        ) {
            urban_latent += latent;
            urban_transit += transit;
        }
        if matches!(
            result.context.origin_settlement_class,
            SettlementClass::Village | SettlementClass::Rural
        ) {
            rural_latent += latent;
            rural_transit += transit;
        }
        if result.context.purpose == TripPurpose::Intercity {
            intercity_latent += latent;
            intercity_transit += transit;
        }
        let airport_pair = profile_by_id
            .get(&result.context.origin_zone_id)
            .map(|p| {
                p.special_attractors
                    .contains(&SpecialAttractorType::Airport)
            })
            .unwrap_or(false)
            || profile_by_id
                .get(&result.context.destination_zone_id)
                .map(|p| {
                    p.special_attractors
                        .contains(&SpecialAttractorType::Airport)
                })
                .unwrap_or(false);
        if airport_pair {
            airport_latent += latent;
            airport_transit += transit;
        }

        let transfer_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.transfer_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let crowding_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.crowding_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let fare_cost_base = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.fare_cost_base.max(0.0))
            .unwrap_or(0.0);
        let reliability_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.reliability_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let transit_gc_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.total_generalized_cost_s.max(0.0))
            .unwrap_or(0.0);
        let car_gc_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::Car)
            .map(|x| x.breakdown.total_generalized_cost_s.max(0.0))
            .unwrap_or(transit_gc_s);
        let indirectness_signal = (transit_gc_s - car_gc_s).max(0.0);

        let reason_sum =
            crowding_penalty_s + fare_cost_base + indirectness_signal + reliability_penalty_s;
        if reason_sum > 0.0 && lost > 0.0 {
            lost_due_crowding += lost * (crowding_penalty_s / reason_sum);
            lost_due_fare += lost * (fare_cost_base / reason_sum);
            lost_due_indirectness += lost * (indirectness_signal / reason_sum);
            lost_due_reliability += lost * (reliability_penalty_s / reason_sum);
        }

        let zentry = zone_agg
            .entry(result.context.origin_zone_id.clone())
            .or_default();
        zentry.latent += latent;
        zentry.transit += transit;
        zentry.car += car;
        zentry.walk += walk;
        zentry.suppressed += suppressed;
        zentry.by_purpose[purpose_idx].0 += latent;
        zentry.by_purpose[purpose_idx].1 += transit;
        zentry.by_purpose[purpose_idx].2 += car;
        zentry.by_purpose[purpose_idx].3 += walk;
        zentry.by_purpose[purpose_idx].4 += suppressed;
        zentry.transfer_penalty_s += transfer_penalty_s * latent;
        zentry.crowding_penalty_s += crowding_penalty_s * latent;
        let parking_proxy_base = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::Car)
            .map(|x| x.breakdown.parking_toll_proxy_base.max(0.0))
            .unwrap_or(0.0);
        zentry.parking_proxy_base += parking_proxy_base * latent;

        let ckey = (
            result.context.origin_zone_id.clone(),
            result.context.destination_zone_id.clone(),
        );
        let centry = corridor_agg.entry(ckey).or_default();
        centry.latent += latent;
        centry.transit += transit;
        centry.car += car;
        centry.walk += walk;
        centry.suppressed += suppressed;
        centry.by_purpose_latent[purpose_idx] += latent;
        for sub in &result.transit_submode_split {
            *centry.submode_pax.entry(sub.mode).or_insert(0.0) += sub.passengers.max(0.0);
        }
    }

    let mut zone_mode_share_metrics = zone_profiles
        .iter()
        .map(|profile| {
            let agg = zone_agg.get(&profile.zone_id).cloned().unwrap_or_default();
            let denom = agg.latent.max(1e-9);
            let mut mode_share_by_purpose = Vec::<PurposeModeShareValue>::new();
            for (idx, vals) in agg.by_purpose.iter().enumerate() {
                if vals.0 <= 0.0 {
                    continue;
                }
                mode_share_by_purpose.push(PurposeModeShareValue {
                    purpose: phase3::purpose_from_index(idx),
                    transit_share: (vals.1 / vals.0).clamp(0.0, 1.0),
                    car_share: (vals.2 / vals.0).clamp(0.0, 1.0),
                    walk_share: (vals.3 / vals.0).clamp(0.0, 1.0),
                    suppressed_share: (vals.4 / vals.0).clamp(0.0, 1.0),
                });
            }
            mode_share_by_purpose.sort_by(|a, b| {
                b.transit_share
                    .partial_cmp(&a.transit_share)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ZoneModeShareMetrics {
                zone_id: profile.zone_id.clone(),
                settlement_class: profile.settlement_class,
                archetype: profile.archetype,
                transit_share: (agg.transit / denom).clamp(0.0, 1.0),
                car_share: (agg.car / denom).clamp(0.0, 1.0),
                walk_share: (agg.walk / denom).clamp(0.0, 1.0),
                suppressed_share: (agg.suppressed / denom).clamp(0.0, 1.0),
                transit_captured_demand: agg.transit.max(0.0),
                non_transit_demand: (agg.car + agg.walk + agg.suppressed).max(0.0),
                mode_share_by_purpose,
                active_temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    zone_mode_share_metrics.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut corridor_mode_share_metrics = corridor_agg
        .iter()
        .map(|((oz, dz), agg)| {
            let denom = agg.latent.max(1e-9);
            let transit_share = (agg.transit / denom).clamp(0.0, 1.0);
            let car_share = (agg.car / denom).clamp(0.0, 1.0);
            let walk_share = (agg.walk / denom).clamp(0.0, 1.0);
            let suppressed_share = (agg.suppressed / denom).clamp(0.0, 1.0);
            let mut dominant_mode = TravelMode::Car;
            let mut dominant_share = car_share;
            if transit_share > dominant_share {
                dominant_share = transit_share;
                dominant_mode = TravelMode::OtherTransit;
            }
            if walk_share > dominant_share {
                dominant_share = walk_share;
                dominant_mode = TravelMode::Walk;
            }
            if suppressed_share > dominant_share {
                dominant_mode = TravelMode::NoTrip;
            }
            let strongest_purpose = agg
                .by_purpose_latent
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| phase3::purpose_from_index(idx))
                .unwrap_or(TripPurpose::Work);
            let strongest_transit_submode = agg
                .submode_pax
                .iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(mode, _)| *mode)
                .unwrap_or(TravelMode::OtherTransit);
            CorridorModeShareMetrics {
                origin_zone_id: oz.clone(),
                destination_zone_id: dz.clone(),
                dominant_mode,
                transit_share,
                car_share,
                walk_share,
                suppressed_share,
                strongest_purpose,
                strongest_transit_submode,
                transit_captured_demand: agg.transit.max(0.0),
                transit_capture_gap: (agg.latent - agg.transit).max(0.0),
                active_temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    corridor_mode_share_metrics.sort_by(|a, b| {
        b.transit_capture_gap
            .partial_cmp(&a.transit_capture_gap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut nearest_zone_by_stop = HashMap::<String, String>::new();
    for stop in &s.world.stops {
        let mut best: Option<(String, f64)> = None;
        for z in &s.world.zones {
            let d = euclid_m((stop.x, stop.y), (z.x, z.y));
            match &best {
                Some((_, bd)) if d >= *bd => {}
                _ => best = Some((z.id.clone(), d)),
            }
        }
        if let Some((zid, _)) = best {
            nearest_zone_by_stop.insert(stop.id.clone(), zid);
        }
    }

    let planning_cfg = PlanningOverlayConfig::default();
    let mut station_transit_capture_context = Vec::<StationTransitCaptureContext>::new();
    for stop in &s.world.stops {
        let mut catchment_latent = 0.0_f64;
        let mut catchment_transit = 0.0_f64;
        let mut catchment_uncaptured = 0.0_f64;
        let mut weighted_transfer = 0.0_f64;
        let mut weighted_indirectness = 0.0_f64;

        for z in &s.world.zones {
            let d = euclid_m((stop.x, stop.y), (z.x, z.y));
            if d > planning_cfg.station_catchment_radius_m {
                continue;
            }
            if let Some(agg) = zone_agg.get(&z.id) {
                catchment_latent += agg.latent.max(0.0);
                catchment_transit += agg.transit.max(0.0);
                catchment_uncaptured += (agg.car + agg.walk + agg.suppressed).max(0.0);
                weighted_transfer += agg.transfer_penalty_s.max(0.0);
                weighted_indirectness += agg.crowding_penalty_s.max(0.0);
            }
        }

        let flow = stop_flow_states.iter().find(|x| x.stop_id == stop.id);
        let waiting = flow.map(|x| x.total_waiting.max(0.0)).unwrap_or(0.0);
        let denied = flow.map(|x| x.denied_this_step.max(0.0)).unwrap_or(0.0);
        let boarded = flow.map(|x| x.boarded_this_step.max(0.0)).unwrap_or(0.0);
        let crowding_signal = (waiting + denied) / (boarded + 1.0);
        let capture_share = if catchment_latent > 0.0 {
            (catchment_transit / catchment_latent).clamp(0.0, 1.0)
        } else {
            0.0
        };
        station_transit_capture_context.push(StationTransitCaptureContext {
            stop_id: stop.id.clone(),
            catchment_latent_demand: catchment_latent.max(0.0),
            transit_captured_demand: catchment_transit.max(0.0),
            uncaptured_competing_demand: catchment_uncaptured.max(0.0),
            limiting_crowding_signal: crowding_signal.max(0.0),
            limiting_transfer_signal: if catchment_latent > 0.0 {
                (weighted_transfer / catchment_latent).max(0.0)
            } else {
                0.0
            },
            limiting_indirectness_signal: if catchment_latent > 0.0 {
                (weighted_indirectness / catchment_latent).max(0.0)
            } else {
                0.0
            },
        });
        if capture_share <= 0.0 {
            continue;
        }
    }
    station_transit_capture_context.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut service_transit_capture_context = Vec::<ServiceTransitCaptureContext>::new();
    let mut canonical_mode_by_service = HashMap::<String, CanonicalTransitMode>::new();
    for svc in &s.world.services {
        let endpoint_dist_km =
            if let (Some(a), Some(b)) = (svc.stop_sequence.first(), svc.stop_sequence.last()) {
                let aa = s.world.stops.iter().find(|x| x.id == *a);
                let bb = s.world.stops.iter().find(|x| x.id == *b);
                if let (Some(aa), Some(bb)) = (aa, bb) {
                    euclid_km((aa.x, aa.y), (bb.x, bb.y))
                } else {
                    0.0
                }
            } else {
                0.0
            };
        let canonical_mode =
            canonical_mode_from_tokens(&svc.mode, svc.mode_variant.as_deref(), endpoint_dist_km);
        let mode = canonical_mode.travel_mode_family();
        canonical_mode_by_service.insert(svc.id.clone(), canonical_mode);
        let mut affected_zone_ids = std::collections::HashSet::<String>::new();
        for stop_id in &svc.stop_sequence {
            if let Some(zid) = nearest_zone_by_stop.get(stop_id) {
                affected_zone_ids.insert(zid.clone());
            }
        }
        let mut latent_exposed = 0.0_f64;
        let mut transit_captured = 0.0_f64;
        let mut uncaptured = 0.0_f64;
        for zid in &affected_zone_ids {
            if let Some(agg) = zone_agg.get(zid) {
                latent_exposed += agg.latent.max(0.0);
                transit_captured += agg.transit.max(0.0);
                uncaptured += (agg.car + agg.walk + agg.suppressed).max(0.0);
            }
        }
        let utilisation_score = service_load_layer
            .iter()
            .find(|x| x.service_id == svc.id)
            .map(|x| x.peak_load.max(0.0) / (svc.vehicle_capacity.max(1.0)))
            .unwrap_or(0.0);
        let max_crowding = vehicle_load_states
            .iter()
            .filter(|x| x.service_id == svc.id)
            .map(|x| x.crowding_ratio.max(0.0))
            .fold(0.0_f64, f64::max);
        service_transit_capture_context.push(ServiceTransitCaptureContext {
            service_id: svc.id.clone(),
            line_id: svc.line_id.clone(),
            service_mode: mode,
            latent_demand_exposed: latent_exposed.max(0.0),
            transit_captured_demand: transit_captured.max(0.0),
            uncaptured_competing_demand: uncaptured.max(0.0),
            utilisation_score: utilisation_score.max(0.0),
            crowding_lost_share_signal: (max_crowding - 0.75).max(0.0)
                * (uncaptured / (latent_exposed + 1.0)).max(0.0),
        });
    }
    service_transit_capture_context.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let by_purpose = purpose_totals
        .iter()
        .enumerate()
        .filter(|(_, vals)| vals.0 > 0.0)
        .map(|(idx, vals)| PurposeModeShareValue {
            purpose: phase3::purpose_from_index(idx),
            transit_share: (vals.1 / vals.0).clamp(0.0, 1.0),
            car_share: (vals.2 / vals.0).clamp(0.0, 1.0),
            walk_share: (vals.3 / vals.0).clamp(0.0, 1.0),
            suppressed_share: (vals.4 / vals.0).clamp(0.0, 1.0),
        })
        .collect::<Vec<_>>();
    let citywide_mode_share_summary = CitywideModeShareSummary {
        transit_share: if total_latent > 0.0 {
            (total_transit / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        car_share: if total_latent > 0.0 {
            (total_car / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        walk_share: if total_latent > 0.0 {
            (total_walk / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        suppressed_share: if total_latent > 0.0 {
            (total_suppressed / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        by_purpose: by_purpose.clone(),
        by_time_slice: vec![TimeSliceModeShareSummary {
            temporal_slice: active_context.clone(),
            transit_share: if total_latent > 0.0 {
                (total_transit / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            car_share: if total_latent > 0.0 {
                (total_car / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            walk_share: if total_latent > 0.0 {
                (total_walk / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            suppressed_share: if total_latent > 0.0 {
                (total_suppressed / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }],
        by_day_type: vec![ServiceDayModeShareSummary {
            service_day_type: active_context.service_day_type,
            transit_share: if total_latent > 0.0 {
                (total_transit / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            car_share: if total_latent > 0.0 {
                (total_car / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            walk_share: if total_latent > 0.0 {
                (total_walk / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            suppressed_share: if total_latent > 0.0 {
                (total_suppressed / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }],
        urban_transit_share: if urban_latent > 0.0 {
            (urban_transit / urban_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        rural_transit_share: if rural_latent > 0.0 {
            (rural_transit / rural_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        intercity_transit_share: if intercity_latent > 0.0 {
            (intercity_transit / intercity_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        airport_access_transit_share: if airport_latent > 0.0 {
            (airport_transit / airport_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
    };

    let mut top_capture_opportunity = corridor_mode_share_metrics
        .iter()
        .map(|c| ModalRankingEntry {
            id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
            score: c.transit_capture_gap.max(0.0) * (0.6 + c.car_share.max(0.0)),
            reason: format!(
                "latent gap {:.1}, transit_share {:.2}, car_share {:.2}",
                c.transit_capture_gap, c.transit_share, c.car_share
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_capture_opportunity.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_capture_opportunity.truncate(12);

    let mut top_car_dominated = corridor_mode_share_metrics
        .iter()
        .filter(|c| c.car_share > c.transit_share && c.transit_share >= 0.08)
        .map(|c| ModalRankingEntry {
            id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
            score: c.car_share.max(0.0) * c.transit_capture_gap.max(0.0),
            reason: format!(
                "car {:.2} > transit {:.2}, gap {:.1}",
                c.car_share, c.transit_share, c.transit_capture_gap
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_car_dominated.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_car_dominated.truncate(12);

    let mut top_overcrowded_losing = corridor_mode_share_metrics
        .iter()
        .map(|c| {
            let crowding_signal = assigned_od_flows
                .iter()
                .find(|f| {
                    f.origin_zone_id == c.origin_zone_id
                        && f.destination_zone_id == c.destination_zone_id
                        && f.time_slice == active_context.time_slice
                })
                .map(|f| f.unserved_passengers.max(0.0))
                .unwrap_or(0.0);
            ModalRankingEntry {
                id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
                score: crowding_signal * (1.0 + c.car_share.max(0.0)),
                reason: format!(
                    "unserved {:.1} with car_share {:.2}",
                    crowding_signal, c.car_share
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    top_overcrowded_losing.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_overcrowded_losing.truncate(12);

    let mut top_rural_services = service_transit_capture_context
        .iter()
        .filter(|svc| {
            canonical_mode_by_service
                .get(&svc.service_id)
                .copied()
                .map(|mode| mode.is_rural_essential_candidate())
                .unwrap_or(matches!(
                    svc.service_mode,
                    TravelMode::Bus | TravelMode::SuburbanRail | TravelMode::RegionalRail
                ))
        })
        .map(|svc| ModalRankingEntry {
            id: svc.service_id.clone(),
            score: (svc.latent_demand_exposed - svc.transit_captured_demand).max(0.0)
                + (1.0 - svc.utilisation_score.min(1.0)) * 20.0,
            reason: format!(
                "latent {:.1}, captured {:.1}, utilisation {:.2}",
                svc.latent_demand_exposed, svc.transit_captured_demand, svc.utilisation_score
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_rural_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_rural_services.truncate(12);

    let mut top_zone_transit = zone_mode_share_metrics
        .iter()
        .map(|z| ModalRankingEntry {
            id: z.zone_id.clone(),
            score: z.transit_share.max(0.0),
            reason: format!(
                "transit_share {:.2}, car_share {:.2}",
                z.transit_share, z.car_share
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_transit.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_transit.truncate(12);

    let mut top_zone_transfer_loss = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.transfer_penalty_s / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "transfer penalty {:.1}s per latent pax",
                if agg.latent > 0.0 {
                    agg.transfer_penalty_s / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_transfer_loss.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_transfer_loss.truncate(12);

    let mut top_zone_crowding_loss = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.crowding_penalty_s / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "crowding penalty {:.1}s per latent pax",
                if agg.latent > 0.0 {
                    agg.crowding_penalty_s / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_crowding_loss.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_crowding_loss.truncate(12);

    let mut top_zone_parking = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.parking_proxy_base / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "parking/toll proxy {:.2} base per latent pax",
                if agg.latent > 0.0 {
                    agg.parking_proxy_base / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_parking.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_parking.truncate(12);

    out.zone_mode_share_metrics = zone_mode_share_metrics.clone();
    out.corridor_mode_share_metrics = corridor_mode_share_metrics.clone();
    out.station_transit_capture_context = station_transit_capture_context.clone();
    out.service_transit_capture_context = service_transit_capture_context.clone();
    out.citywide_mode_share_summary = citywide_mode_share_summary.clone();
    out.modal_demand_diagnostics = ModalDemandDiagnostics {
        mode_share_by_purpose: by_purpose,
        mode_share_by_zone: zone_mode_share_metrics,
        mode_share_by_corridor: corridor_mode_share_metrics,
        mode_share_by_time_slice: citywide_mode_share_summary.by_time_slice.clone(),
        mode_share_by_day_type: citywide_mode_share_summary.by_day_type.clone(),
        transit_capture_total: total_transit.max(0.0),
        transit_lost_total: (total_latent - total_transit).max(0.0),
        transit_lost_due_to_crowding: lost_due_crowding.max(0.0),
        transit_lost_due_to_fare: lost_due_fare.max(0.0),
        transit_lost_due_to_indirectness: lost_due_indirectness.max(0.0),
        transit_lost_due_to_reliability: lost_due_reliability.max(0.0),
        top_transit_capture_opportunity_corridors: top_capture_opportunity,
        top_car_dominated_transit_viable_corridors: top_car_dominated,
        top_overcrowded_corridors_losing_mode_share: top_overcrowded_losing,
        top_rural_essential_low_demand_services: top_rural_services,
        top_zones_by_transit_share: top_zone_transit,
        top_zones_losing_due_to_transfers: top_zone_transfer_loss,
        top_zones_losing_due_to_crowding: top_zone_crowding_loss,
        top_zones_where_parking_penalty_supports_transit: top_zone_parking,
    };

    out
}

fn build_operations_outputs(
    s: &Scenario,
    active_context: &TemporalDemandSlice,
    cfg: &OperationsReliabilityConfig,
    board_loads: &[BoardLoad],
    stop_flow_states: &[StopFlowState],
    vehicle_load_states: &[VehicleLoadState],
    mode_choice_results: &[ModeChoiceResult],
) -> OperationsOutputs {
    #[derive(Debug, Clone, Default)]
    struct StopObservation {
        dwell_s: f64,
        scheduled_calls: f64,
        actual_calls: f64,
        realised_headway_s: f64,
        irregularity: f64,
        delay_s: f64,
    }

    let mut out = OperationsOutputs::default();
    if s.world.services.is_empty() || s.world.stops.is_empty() {
        return out;
    }

    let stop_by_id = s
        .world
        .stops
        .iter()
        .map(|x| (x.id.clone(), x))
        .collect::<HashMap<_, _>>();
    let flow_by_stop = stop_flow_states
        .iter()
        .map(|x| (x.stop_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let board_by_key = board_loads
        .iter()
        .map(|x| ((x.service_id.clone(), x.stop_id.clone()), x))
        .collect::<HashMap<_, _>>();
    let vehicle_by_key = vehicle_load_states
        .iter()
        .map(|x| ((x.service_id.clone(), x.stop_id.clone()), x))
        .collect::<HashMap<_, _>>();

    let mut runtime_s_by_pair = HashMap::<(String, String), f64>::new();
    for link in &s.world.links {
        let runtime = if link.speed_mps > 0.0 {
            link.distance_m.max(0.0) / link.speed_mps.max(0.1)
        } else {
            0.0
        };
        runtime_s_by_pair
            .entry((link.from_stop.clone(), link.to_stop.clone()))
            .or_insert(runtime.max(0.0));
    }

    let mut services_by_stop = HashMap::<String, Vec<String>>::new();
    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) {
            continue;
        }
        for stop_id in &svc.stop_sequence {
            services_by_stop
                .entry(stop_id.clone())
                .or_default()
                .push(svc.id.clone());
        }
    }

    let mut stop_obs = HashMap::<String, Vec<StopObservation>>::new();
    let mut service_states = Vec::<ServiceOperationState>::new();

    for svc in &s.world.services {
        if !service_is_active_for_sim(svc) || svc.stop_sequence.is_empty() {
            continue;
        }

        let board_rows = svc
            .stop_sequence
            .iter()
            .filter_map(|stop_id| board_by_key.get(&(svc.id.clone(), stop_id.clone())))
            .copied()
            .collect::<Vec<_>>();
        let scheduled_calls = if board_rows.is_empty() {
            if svc.headway_s > 0.0 {
                (s.meta.time_period_hours * 3600.0 / svc.headway_s).max(0.0)
            } else {
                0.0
            }
        } else {
            board_rows
                .iter()
                .map(|x| x.departures_in_period.max(0.0))
                .sum::<f64>()
                / (board_rows.len() as f64)
        };
        let actual_calls = if board_rows.is_empty() {
            scheduled_calls
        } else {
            board_rows
                .iter()
                .map(|x| x.departures_observed as f64)
                .sum::<f64>()
                / (board_rows.len() as f64)
        };

        let mut cumulative_delay_s = 0.0_f64;
        let mut delay_series = Vec::<f64>::new();
        let mut dwell_series = Vec::<f64>::new();
        let mut runtime_series = Vec::<f64>::new();
        let mut per_stop_scheduled = Vec::<f64>::new();
        let mut per_stop_actual = Vec::<f64>::new();
        let mut bottleneck_stop = None::<String>;
        let mut bottleneck_score = 0.0_f64;
        let mut denied_proxy_sum = 0.0_f64;
        let mut skipped_capacity_opportunities = 0.0_f64;

        for (idx, stop_id) in svc.stop_sequence.iter().enumerate() {
            let key = (svc.id.clone(), stop_id.clone());
            let board_row = board_by_key.get(&key).copied();
            let vehicle_row = vehicle_by_key.get(&key).copied();
            let flow = flow_by_stop.get(stop_id.as_str()).copied();
            let stop = stop_by_id.get(stop_id.as_str()).copied();
            let stop_services_count = services_by_stop
                .get(stop_id.as_str())
                .map(|x| x.len())
                .unwrap_or(1);

            let boardings = vehicle_row
                .map(|x| x.boardings_this_stop.max(0.0))
                .unwrap_or_else(|| {
                    board_row
                        .map(|x| (x.served_from_arrivals + x.served_from_queue).max(0.0))
                        .unwrap_or(0.0)
                });
            let alightings = vehicle_row
                .map(|x| x.alightings_this_stop.max(0.0))
                .unwrap_or_else(|| {
                    board_row
                        .map(|x| x.alightings_served.max(0.0))
                        .unwrap_or(0.0)
                });
            let crowding_ratio = vehicle_row
                .map(|x| x.crowding_ratio.max(0.0))
                .unwrap_or_else(|| {
                    if svc.vehicle_capacity > 0.0 {
                        (boardings / svc.vehicle_capacity.max(1.0)).max(0.0)
                    } else {
                        0.0
                    }
                });
            let waiting = flow.map(|x| x.total_waiting.max(0.0)).unwrap_or(0.0);
            let denied = flow.map(|x| x.denied_this_step.max(0.0)).unwrap_or(0.0);
            denied_proxy_sum += denied;
            skipped_capacity_opportunities += denied / svc.vehicle_capacity.max(1.0);
            let waiting_ratio = waiting / (boardings + 1.0);
            let denied_ratio = denied / (boardings + 1.0);

            let stop_type = stop
                .and_then(|x| x.stop_type.as_deref())
                .unwrap_or("station")
                .to_ascii_lowercase();
            let stop_base = if stop_type.contains("bus") {
                cfg.base_dwell_bus_stop_s.max(1.0)
            } else {
                cfg.base_dwell_station_s.max(1.0)
            };
            let mut stop_class_mult = if stop_type.contains("bus") {
                0.90
            } else {
                1.08
            };
            if stop_services_count >= 3 {
                stop_class_mult *= cfg.interchange_dwell_multiplier.max(1.0);
            }
            let base_dwell = svc.dwell_s.max(stop_base);
            let dwell_s = (base_dwell
                + cfg.boarding_dwell_s_per_pax.max(0.0) * boardings
                + cfg.alighting_dwell_s_per_pax.max(0.0) * alightings)
                * stop_class_mult
                * (1.0 + cfg.crowding_dwell_multiplier.max(0.0) * (crowding_ratio - 1.0).max(0.0));

            let mut runtime_s = 0.0_f64;
            let mut base_runtime_s = 0.0_f64;
            if idx + 1 < svc.stop_sequence.len() {
                let next = &svc.stop_sequence[idx + 1];
                base_runtime_s = runtime_s_by_pair
                    .get(&(stop_id.clone(), next.clone()))
                    .copied()
                    .unwrap_or_else(|| {
                        if let (Some(a), Some(b)) = (stop, stop_by_id.get(next).copied()) {
                            (euclid_m((a.x, a.y), (b.x, b.y)) / 13.0).max(0.0)
                        } else {
                            0.0
                        }
                    });
                runtime_s = base_runtime_s
                    * (1.0
                        + cfg.runtime_delay_per_crowding_ratio.max(0.0)
                            * (crowding_ratio - 0.85).max(0.0)
                        + cfg.runtime_delay_per_waiting_ratio.max(0.0) * waiting_ratio.min(4.0));
            }

            let dwell_overrun_s = (dwell_s - base_dwell).max(0.0);
            let runtime_overrun_s = (runtime_s - base_runtime_s).max(0.0);
            let recovery_s = cfg
                .delay_recovery_margin_s
                .max(0.0)
                .min(cumulative_delay_s * 0.35 + 20.0);
            cumulative_delay_s =
                (cumulative_delay_s + dwell_overrun_s + runtime_overrun_s - recovery_s).max(0.0);

            let pressure_score = (waiting / cfg.stop_pressure_waiting_threshold.max(1.0))
                + (denied / cfg.stop_pressure_denied_threshold.max(1.0))
                + 1.25 * (crowding_ratio - 1.0).max(0.0)
                + 0.8 * denied_ratio
                + 0.35 * (dwell_overrun_s / base_dwell.max(1.0));
            if pressure_score > bottleneck_score {
                bottleneck_score = pressure_score;
                bottleneck_stop = Some(stop_id.clone());
            }

            delay_series.push(cumulative_delay_s);
            dwell_series.push(dwell_s.max(0.0));
            runtime_series.push(runtime_s.max(0.0));
            per_stop_scheduled.push(
                board_row
                    .map(|x| x.departures_in_period.max(0.0))
                    .unwrap_or(scheduled_calls),
            );
            per_stop_actual.push(
                board_row
                    .map(|x| x.departures_observed as f64)
                    .unwrap_or(actual_calls),
            );
        }

        let avg_dwell = if dwell_series.is_empty() {
            0.0
        } else {
            dwell_series.iter().sum::<f64>() / (dwell_series.len() as f64)
        };
        let max_dwell = dwell_series.iter().copied().fold(0.0_f64, f64::max);
        let avg_runtime = if runtime_series.is_empty() {
            0.0
        } else {
            runtime_series.iter().sum::<f64>() / (runtime_series.len() as f64)
        };
        let max_runtime = runtime_series.iter().copied().fold(0.0_f64, f64::max);
        let avg_delay = if delay_series.is_empty() {
            0.0
        } else {
            delay_series.iter().sum::<f64>() / (delay_series.len() as f64)
        };
        let max_delay = delay_series.iter().copied().fold(0.0_f64, f64::max);

        let expected_headway_s = svc.headway_s.max(1.0);
        let average_headway_realised_s = if actual_calls > 0.0 {
            (s.meta.time_period_hours * 3600.0 / actual_calls).max(0.0)
        } else {
            expected_headway_s + max_delay * 0.4
        };
        let headway_irregularity = ((average_headway_realised_s - expected_headway_s).abs()
            / expected_headway_s.max(1.0))
            + cfg.headway_irregularity_from_delay.max(0.0)
                * (max_delay / expected_headway_s.max(1.0));
        let bunching_gap_ahead_s = (expected_headway_s - 0.5 * max_delay).max(0.0);
        let bunching_gap_behind_s = (expected_headway_s + 0.5 * max_delay).max(0.0);
        let bunching_risk = if expected_headway_s <= 720.0 {
            (headway_irregularity / cfg.bunching_sensitivity_threshold.max(0.05)).clamp(0.0, 3.0)
        } else {
            (headway_irregularity * 0.45).clamp(0.0, 3.0)
        };
        let denied_pressure =
            denied_proxy_sum / (svc.vehicle_capacity.max(1.0) * svc.stop_sequence.len() as f64);

        let mut reliability_score = (1.0
            / (1.0
                + max_delay / 240.0
                + headway_irregularity * 1.2
                + bunching_risk * 0.8
                + denied_pressure))
            .clamp(0.0, 1.0);
        if reliability_score.is_nan() {
            reliability_score = 0.0;
        }

        let on_time_status = if max_delay <= cfg.service_on_time_threshold_minor_s.max(0.0) {
            OnTimeStatus::OnTime
        } else if max_delay <= cfg.service_on_time_threshold_major_s.max(0.0) {
            OnTimeStatus::SlightlyLate
        } else if max_delay <= cfg.service_on_time_threshold_major_s.max(0.0) * 2.0 {
            OnTimeStatus::Late
        } else {
            OnTimeStatus::SevereDelay
        };
        let incident_type = if max_delay > cfg.service_on_time_threshold_major_s.max(0.0) * 1.4 {
            OperationalIncidentType::MajorDelay
        } else if max_dwell > svc.dwell_s.max(1.0) * 1.8 {
            OperationalIncidentType::DwellOverrun
        } else if bunching_risk > 1.2 {
            OperationalIncidentType::ServiceGap
        } else if denied_pressure > 0.25 {
            OperationalIncidentType::Congestion
        } else if max_delay > cfg.service_on_time_threshold_minor_s.max(0.0) {
            OperationalIncidentType::MinorDelay
        } else {
            OperationalIncidentType::None
        };

        let mut delay_causes = Vec::<String>::new();
        if max_dwell > svc.dwell_s.max(1.0) * 1.25 {
            delay_causes.push("dwell_overrun".to_string());
        }
        if bunching_risk > 0.85 {
            delay_causes.push("headway_irregularity".to_string());
        }
        if denied_pressure > 0.15 {
            delay_causes.push("capacity_pressure".to_string());
        }
        if delay_causes.is_empty() {
            delay_causes.push("nominal".to_string());
        }

        let service_state = ServiceOperationState {
            service_id: svc.id.clone(),
            run_id: format!("{}::{:?}", svc.id, active_context.time_slice),
            scheduled_departure_time_s: 0.0,
            expected_headway_s,
            actual_departure_time_s: max_delay,
            cumulative_delay_s,
            delay_at_last_stop_s: *delay_series.last().unwrap_or(&0.0),
            dwell_time_last_stop_s: *dwell_series.last().unwrap_or(&0.0),
            runtime_last_segment_s: *runtime_series.last().unwrap_or(&0.0),
            bunching_gap_ahead_s,
            bunching_gap_behind_s,
            missed_departures: (scheduled_calls - actual_calls).max(0.0),
            skipped_capacity_opportunities: skipped_capacity_opportunities.max(0.0),
            reliability_score,
            on_time_status,
            incident_type,
            average_delay_s: avg_delay.max(0.0),
            max_delay_s: max_delay.max(0.0),
            average_dwell_time_s: avg_dwell.max(0.0),
            max_dwell_time_s: max_dwell.max(0.0),
            average_runtime_segment_s: avg_runtime.max(0.0),
            max_runtime_segment_s: max_runtime.max(0.0),
            headway_irregularity: headway_irregularity.max(0.0),
            scheduled_service_calls: scheduled_calls.max(0.0),
            actual_service_calls: actual_calls.max(0.0),
            average_headway_realised_s: average_headway_realised_s.max(0.0),
            transfer_success_rate: 1.0,
            strongest_bottleneck_stop_id: bottleneck_stop.clone(),
            delay_causes,
            active_temporal_slice: active_context.clone(),
        };

        for (idx, stop_id) in svc.stop_sequence.iter().enumerate() {
            stop_obs
                .entry(stop_id.clone())
                .or_default()
                .push(StopObservation {
                    dwell_s: dwell_series.get(idx).copied().unwrap_or(0.0),
                    scheduled_calls: per_stop_scheduled.get(idx).copied().unwrap_or(0.0),
                    actual_calls: per_stop_actual.get(idx).copied().unwrap_or(0.0),
                    realised_headway_s: average_headway_realised_s.max(0.0),
                    irregularity: headway_irregularity.max(0.0),
                    delay_s: delay_series.get(idx).copied().unwrap_or(0.0),
                });
        }

        service_states.push(service_state);
    }

    service_states.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let mut stop_states = Vec::<StopOperationState>::new();
    for stop in &s.world.stops {
        let obs = stop_obs.get(&stop.id).cloned().unwrap_or_default();
        let scheduled_calls = obs.iter().map(|x| x.scheduled_calls.max(0.0)).sum::<f64>();
        let actual_calls = obs.iter().map(|x| x.actual_calls.max(0.0)).sum::<f64>();
        let avg_headway_realised_s = if obs.is_empty() {
            0.0
        } else {
            obs.iter()
                .map(|x| x.realised_headway_s.max(0.0))
                .sum::<f64>()
                / (obs.len() as f64)
        };
        let headway_irregularity = if obs.is_empty() {
            0.0
        } else {
            obs.iter().map(|x| x.irregularity.max(0.0)).sum::<f64>() / (obs.len() as f64)
        };
        let average_dwell_time_s = if obs.is_empty() {
            0.0
        } else {
            obs.iter().map(|x| x.dwell_s.max(0.0)).sum::<f64>() / (obs.len() as f64)
        };
        let max_dwell_time_s = obs
            .iter()
            .map(|x| x.dwell_s.max(0.0))
            .fold(0.0_f64, f64::max);
        let avg_delay_here = if obs.is_empty() {
            0.0
        } else {
            obs.iter().map(|x| x.delay_s.max(0.0)).sum::<f64>() / (obs.len() as f64)
        };

        let flow = flow_by_stop.get(&stop.id).copied();
        let waiting = flow.map(|x| x.total_waiting.max(0.0)).unwrap_or(0.0);
        let boarded = flow.map(|x| x.boarded_this_step.max(0.0)).unwrap_or(0.0);
        let denied = flow.map(|x| x.denied_this_step.max(0.0)).unwrap_or(0.0);
        let average_wait_s = if actual_calls > 0.0 {
            waiting / actual_calls.max(1.0)
        } else {
            waiting
        };
        let platform_crowding_proxy = waiting + 0.55 * boarded + 1.15 * denied;
        let denied_boarding_pressure = denied / (boarded + 1.0);

        let mut transfer_success_rate = 1.0
            - (0.45 * headway_irregularity
                + 0.35 * denied_boarding_pressure
                + 0.20 * (platform_crowding_proxy / cfg.stop_pressure_waiting_threshold.max(1.0)))
            .clamp(0.0, 0.95);
        transfer_success_rate = transfer_success_rate.clamp(0.0, 1.0);

        let operational_pressure_score = (0.38
            * (platform_crowding_proxy / cfg.stop_pressure_waiting_threshold.max(1.0))
            + 0.26 * denied_boarding_pressure
            + 0.20 * headway_irregularity
            + 0.16 * (avg_delay_here / cfg.service_on_time_threshold_minor_s.max(1.0)))
        .max(0.0);
        let incident_type = if denied_boarding_pressure > 0.25 {
            OperationalIncidentType::Congestion
        } else if average_dwell_time_s > cfg.base_dwell_station_s.max(1.0) * 1.8 {
            OperationalIncidentType::DwellOverrun
        } else if headway_irregularity > 0.3 {
            OperationalIncidentType::ServiceGap
        } else if operational_pressure_score > 0.7 {
            OperationalIncidentType::MinorDelay
        } else {
            OperationalIncidentType::None
        };

        stop_states.push(StopOperationState {
            stop_id: stop.id.clone(),
            scheduled_service_calls: scheduled_calls.max(0.0),
            actual_service_calls: actual_calls.max(0.0),
            average_headway_realised_s: avg_headway_realised_s.max(0.0),
            headway_irregularity: headway_irregularity.max(0.0),
            average_dwell_time_s: average_dwell_time_s.max(0.0),
            max_dwell_time_s: max_dwell_time_s.max(0.0),
            average_wait_s: average_wait_s.max(0.0),
            platform_crowding_proxy: platform_crowding_proxy.max(0.0),
            denied_boarding_pressure: denied_boarding_pressure.max(0.0),
            transfer_success_rate,
            operational_pressure_score,
            incident_type,
            active_temporal_slice: active_context.clone(),
        });
    }
    stop_states.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let stop_state_by_id = stop_states
        .iter()
        .map(|x| (x.stop_id.clone(), x))
        .collect::<HashMap<_, _>>();
    let service_state_by_id = service_states
        .iter()
        .map(|x| (x.service_id.clone(), x))
        .collect::<HashMap<_, _>>();

    let mut transfer_metrics = Vec::<TransferOperationMetrics>::new();
    for (stop_id, svc_ids) in &services_by_stop {
        if svc_ids.len() < 2 {
            continue;
        }
        let stop_pressure = stop_state_by_id
            .get(stop_id)
            .map(|x| x.platform_crowding_proxy / cfg.stop_pressure_waiting_threshold.max(1.0))
            .unwrap_or(0.0);
        let flow = flow_by_stop.get(stop_id).copied();
        let boarded_here = flow.map(|x| x.boarded_this_step.max(0.0)).unwrap_or(0.0);
        let pair_count = (svc_ids.len() * (svc_ids.len() - 1)).max(1) as f64;

        for from_service in svc_ids {
            for to_service in svc_ids {
                if from_service == to_service {
                    continue;
                }
                let from_state = if let Some(v) = service_state_by_id.get(from_service) {
                    *v
                } else {
                    continue;
                };
                let to_state = if let Some(v) = service_state_by_id.get(to_service) {
                    *v
                } else {
                    continue;
                };

                let scheduled_transfer_window_s = (cfg.transfer_base_window_s.max(0.0)
                    + 0.5 * to_state.expected_headway_s.max(0.0))
                .max(1.0);
                let realised_transfer_window_s = (scheduled_transfer_window_s
                    - cfg.transfer_delay_impact.max(0.0)
                        * from_state.delay_at_last_stop_s.max(0.0)
                    - cfg.transfer_crowding_impact.max(0.0) * stop_pressure.max(0.0) * 120.0
                    - 0.30
                        * to_state.headway_irregularity.max(0.0)
                        * to_state.expected_headway_s.max(0.0))
                .max(0.0);
                let missed_transfer_rate = (1.0
                    - realised_transfer_window_s / scheduled_transfer_window_s.max(1.0))
                .clamp(0.0, 1.0);
                let transfer_volume_proxy = ((boarded_here * 0.20) / pair_count)
                    + (from_state
                        .actual_service_calls
                        .min(to_state.actual_service_calls)
                        * 0.35);
                let missed_transfer_count =
                    transfer_volume_proxy.max(0.0) * missed_transfer_rate.max(0.0);
                let average_transfer_wait_s = (0.5
                    * to_state.expected_headway_s.max(0.0)
                    * (1.0 + to_state.headway_irregularity.max(0.0) + missed_transfer_rate))
                    .max(0.0);
                let delay_caused_transfer_failures = missed_transfer_count.max(0.0)
                    * (from_state.delay_at_last_stop_s / from_state.expected_headway_s.max(60.0))
                        .clamp(0.0, 2.5);
                let interchange_pressure_score = (missed_transfer_rate.max(0.0)
                    + stop_pressure.max(0.0) * 0.45
                    + (1.0 - from_state.reliability_score).max(0.0) * 0.35)
                    .max(0.0);

                transfer_metrics.push(TransferOperationMetrics {
                    interchange_stop_id: stop_id.clone(),
                    from_service_id: from_service.clone(),
                    to_service_id: to_service.clone(),
                    scheduled_transfer_window_s,
                    realised_transfer_window_s,
                    missed_transfer_count: missed_transfer_count.max(0.0),
                    missed_transfer_rate,
                    average_transfer_wait_s,
                    delay_caused_transfer_failures: delay_caused_transfer_failures.max(0.0),
                    interchange_pressure_score,
                    active_temporal_slice: active_context.clone(),
                });
            }
        }
    }
    transfer_metrics.sort_by(|a, b| {
        b.missed_transfer_count
            .partial_cmp(&a.missed_transfer_count)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    transfer_metrics.truncate(300);

    let mut transfer_success_by_stop = HashMap::<String, f64>::new();
    for stop in &stop_states {
        let metrics = transfer_metrics
            .iter()
            .filter(|x| x.interchange_stop_id == stop.stop_id)
            .collect::<Vec<_>>();
        if metrics.is_empty() {
            transfer_success_by_stop.insert(stop.stop_id.clone(), stop.transfer_success_rate);
            continue;
        }
        let total_weight = metrics
            .iter()
            .map(|x| x.missed_transfer_count.max(0.0) + 1.0)
            .sum::<f64>()
            .max(1e-9);
        let missed = metrics
            .iter()
            .map(|x| x.missed_transfer_rate.max(0.0) * (x.missed_transfer_count.max(0.0) + 1.0))
            .sum::<f64>()
            / total_weight;
        transfer_success_by_stop.insert(stop.stop_id.clone(), (1.0 - missed).clamp(0.0, 1.0));
    }
    for stop in &mut stop_states {
        stop.transfer_success_rate = transfer_success_by_stop
            .get(&stop.stop_id)
            .copied()
            .unwrap_or(stop.transfer_success_rate)
            .clamp(0.0, 1.0);
        if stop.transfer_success_rate < 0.55 {
            stop.incident_type = OperationalIncidentType::TransferFailure;
        }
    }

    for state in &mut service_states {
        let related = transfer_metrics
            .iter()
            .filter(|x| {
                x.from_service_id == state.service_id || x.to_service_id == state.service_id
            })
            .collect::<Vec<_>>();
        if !related.is_empty() {
            let total_weight = related
                .iter()
                .map(|x| x.missed_transfer_count.max(0.0) + 1.0)
                .sum::<f64>()
                .max(1e-9);
            let missed = related
                .iter()
                .map(|x| x.missed_transfer_rate.max(0.0) * (x.missed_transfer_count.max(0.0) + 1.0))
                .sum::<f64>()
                / total_weight;
            state.transfer_success_rate = (1.0 - missed).clamp(0.0, 1.0);
            state.reliability_score = (state.reliability_score
                * (0.70 + 0.30 * state.transfer_success_rate))
                .clamp(0.0, 1.0);
            if state.transfer_success_rate < 0.55 {
                state.incident_type = OperationalIncidentType::TransferFailure;
            }
        }
    }

    let mut delay_by_service = service_states
        .iter()
        .map(|svc| OperationalRankingEntry {
            id: svc.service_id.clone(),
            score: svc.max_delay_s.max(0.0),
            reason: format!(
                "avg delay {:.1}s, max delay {:.1}s",
                svc.average_delay_s, svc.max_delay_s
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    delay_by_service.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    delay_by_service.truncate(20);

    let mut dwell_inflation_causes = service_states
        .iter()
        .map(|svc| OperationalRankingEntry {
            id: svc.service_id.clone(),
            score: (svc.average_dwell_time_s
                - s.world
                    .services
                    .iter()
                    .find(|x| x.id == svc.service_id)
                    .map(|x| x.dwell_s.max(0.0))
                    .unwrap_or(0.0))
            .max(0.0),
            reason: format!(
                "avg dwell {:.1}s, max dwell {:.1}s",
                svc.average_dwell_time_s, svc.max_dwell_time_s
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    dwell_inflation_causes.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    dwell_inflation_causes.truncate(20);

    let mut bunching_indicators = service_states
        .iter()
        .map(|svc| {
            let score = if svc.expected_headway_s > 0.0 {
                (svc.average_headway_realised_s - svc.expected_headway_s).abs()
                    / svc.expected_headway_s
            } else {
                0.0
            };
            OperationalRankingEntry {
                id: svc.service_id.clone(),
                score: score.max(0.0),
                reason: format!(
                    "expected {:.0}s, realised {:.0}s, irregularity {:.2}",
                    svc.expected_headway_s,
                    svc.average_headway_realised_s,
                    svc.headway_irregularity
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    bunching_indicators.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    bunching_indicators.truncate(20);

    let mut missed_transfers = transfer_metrics
        .iter()
        .map(|x| OperationalRankingEntry {
            id: format!(
                "{}:{}->{}",
                x.interchange_stop_id, x.from_service_id, x.to_service_id
            ),
            score: x.missed_transfer_count.max(0.0),
            reason: format!(
                "missed rate {:.2}, realised window {:.0}s",
                x.missed_transfer_rate, x.realised_transfer_window_s
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    missed_transfers.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    missed_transfers.truncate(24);

    let mut realised_vs_scheduled_headway = service_states
        .iter()
        .map(|svc| OperationalRankingEntry {
            id: svc.service_id.clone(),
            score: if svc.expected_headway_s > 0.0 {
                (svc.average_headway_realised_s / svc.expected_headway_s).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "scheduled {:.0}s vs realised {:.0}s",
                svc.expected_headway_s, svc.average_headway_realised_s
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    realised_vs_scheduled_headway.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    realised_vs_scheduled_headway.truncate(20);

    let mut operational_bottlenecks = stop_states
        .iter()
        .map(|stop| OperationalRankingEntry {
            id: stop.stop_id.clone(),
            score: stop.operational_pressure_score.max(0.0),
            reason: format!(
                "crowding {:.1}, denied pressure {:.2}, transfer success {:.2}",
                stop.platform_crowding_proxy,
                stop.denied_boarding_pressure,
                stop.transfer_success_rate
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    operational_bottlenecks.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    operational_bottlenecks.truncate(24);

    let mut reliability_linked_mode_choice_penalties = mode_choice_results
        .iter()
        .map(|row| {
            let penalty_s = row
                .generalized_costs_by_mode
                .iter()
                .find(|g| g.mode == TravelMode::OtherTransit)
                .map(|g| g.breakdown.reliability_penalty_s.max(0.0))
                .unwrap_or(0.0);
            OperationalRankingEntry {
                id: format!(
                    "{}->{}:{:?}",
                    row.context.origin_zone_id,
                    row.context.destination_zone_id,
                    row.context.purpose
                ),
                score: penalty_s,
                reason: format!(
                    "transit capture {:.1}/{:.1}",
                    row.transit_captured_passengers.max(0.0),
                    row.latent_passengers.max(0.0)
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    reliability_linked_mode_choice_penalties.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    reliability_linked_mode_choice_penalties.truncate(24);

    out.service_operation_states = service_states;
    out.stop_operation_states = stop_states;
    out.transfer_operation_metrics = transfer_metrics;
    out.service_reliability_diagnostics = ServiceReliabilityDiagnostics {
        delay_by_service,
        dwell_inflation_causes,
        bunching_indicators,
        missed_transfers,
        realised_vs_scheduled_headway,
        operational_bottlenecks,
        reliability_linked_mode_choice_penalties,
        worst_reliability_by_time_slice: Vec::new(),
        worst_dwell_pressure_stations_by_time_slice: Vec::new(),
        worst_transfer_nodes_by_time_slice: Vec::new(),
    };

    out
}

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
    policy: &super::types::EconomicsPolicyConfig,
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
    policy: &super::types::EconomicsPolicyConfig,
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
    policy: &super::types::EconomicsPolicyConfig,
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
) -> super::types::ServiceCostProfile {
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
        .unwrap_or(super::types::ServiceCostProfile {
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
) -> super::types::InfrastructureCostProfile {
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
        .unwrap_or(super::types::InfrastructureCostProfile {
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
) -> super::types::RollingStockCostProfile {
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
        .unwrap_or(super::types::RollingStockCostProfile {
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

fn build_economics_outputs(
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

fn apply_economics_to_planning(
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

fn validate(s: &Scenario) -> Result<(), String> {
    if s.world.zones.is_empty() {
        return Err("world.zones is empty".into());
    }
    if s.world.stops.is_empty() {
        return Err("world.stops is empty".into());
    }
    if s.world.links.is_empty() {
        return Err("world.links is empty".into());
    }
    if s.params.access_walk_speed_mps <= 0.0 {
        return Err("params.access_walk_speed_mps must be > 0".into());
    }
    if s.params.access_radius_m <= 0.0 {
        return Err("params.access_radius_m must be > 0".into());
    }
    if s.params.gravity_beta < 0.0 {
        return Err("params.gravity_beta must be >= 0".into());
    }
    Ok(())
}

fn materialize_effective_zones(s: &Scenario) -> Scenario {
    if s.world.demand_cells.is_empty() {
        return s.clone();
    }
    let mut out = s.clone();
    out.world.zones = out
        .world
        .demand_cells
        .iter()
        .map(|c| demand_cell_to_zone(c, &out.params))
        .collect();
    out
}

fn demand_cell_to_zone(c: &DemandCell, p: &crate::model::Params) -> Zone {
    let purpose_sum = p.purpose_share_home_work
        + p.purpose_share_home_education
        + p.purpose_share_home_retail
        + p.purpose_share_home_recreation
        + p.purpose_share_other;
    let norm = if purpose_sum > 0.0 { purpose_sum } else { 1.0 };
    let work = p.purpose_share_home_work / norm;
    let education = p.purpose_share_home_education / norm;
    let retail = p.purpose_share_home_retail / norm;
    let recreation = p.purpose_share_home_recreation / norm;
    let other = p.purpose_share_other / norm;

    let weighted_mix = work
        * (c.activity_mix_office * p.attraction_weight_office
            + c.activity_mix_industrial * p.attraction_weight_industrial
            + c.activity_mix_health * p.attraction_weight_health)
        + education * (c.activity_mix_education * p.attraction_weight_education)
        + retail * (c.activity_mix_retail * p.attraction_weight_retail)
        + recreation * (c.activity_mix_recreation * p.attraction_weight_recreation)
        + other * (c.activity_mix_residential * 0.35 + c.activity_mix_retail * 0.25);

    let attraction_scale = weighted_mix.max(0.05);

    Zone {
        id: c.cell_id.clone(),
        x: c.x,
        y: c.y,
        population: c.residents_night.max(0.0),
        jobs: (c.jobs_day.max(0.0) * attraction_scale).max(0.0),
        country_iso2: c.country_iso2.clone(),
    }
}

fn normalized_purpose_shares(p: &crate::model::Params) -> [f64; 5] {
    let raw = [
        p.purpose_share_home_work.max(0.0),
        p.purpose_share_home_education.max(0.0),
        p.purpose_share_home_retail.max(0.0),
        p.purpose_share_home_recreation.max(0.0),
        p.purpose_share_other.max(0.0),
    ];
    let sum: f64 = raw.iter().sum();
    if sum > 0.0 && sum.is_finite() {
        [
            raw[0] / sum,
            raw[1] / sum,
            raw[2] / sum,
            raw[3] / sum,
            raw[4] / sum,
        ]
    } else {
        [1.0, 0.0, 0.0, 0.0, 0.0]
    }
}

fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
    let mut cleaned = [0.0; 7];
    let mut sum = 0.0_f64;
    for (idx, value) in values.iter().enumerate() {
        let v = if value.is_finite() && *value >= 0.0 {
            *value
        } else {
            0.0
        };
        cleaned[idx] = v;
        sum += v;
    }
    if sum > 0.0 {
        cleaned.map(|v| v / sum)
    } else {
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    }
}

fn index_maps(world: &World) -> (HashMap<String, usize>, HashMap<String, usize>) {
    let stop_index = world
        .stops
        .iter()
        .enumerate()
        .map(|(i, st)| (st.id.clone(), i))
        .collect::<HashMap<_, _>>();

    let zone_index = world
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| (z.id.clone(), i))
        .collect::<HashMap<_, _>>();

    (stop_index, zone_index)
}

fn apply_fare_to_paths(mut paths: Vec<BuiltPath>, params: &crate::model::Params) -> Vec<BuiltPath> {
    for path in &mut paths {
        let fare_base = fare_for_path(path, params).max(0.0);
        path.stats.fare_base = fare_base;
        if params.fare_enabled {
            let vot_per_s = params.fare_value_of_time_per_s();
            if vot_per_s > 0.0 {
                path.stats.gc_s += fare_base / vot_per_s;
            }
        }
    }
    paths
}

fn fare_for_path(path: &BuiltPath, params: &crate::model::Params) -> f64 {
    if path.board_modes.is_empty() {
        return 0.0;
    }

    let transfer_window_s = params.fare_transfer_window_s.max(0.0);
    let free_transfers_per_trip = params.fare_free_transfers_per_trip as usize;

    let mut total = 0.0_f64;
    let mut chain_start_time = path.board_times_s.first().copied().unwrap_or(0.0);
    let mut free_remaining = free_transfers_per_trip;

    for (idx, mode) in path.board_modes.iter().enumerate() {
        let fare = params.fare_for_mode(mode).max(0.0);
        if idx == 0 {
            total += fare;
            continue;
        }

        let board_time = path
            .board_times_s
            .get(idx)
            .copied()
            .unwrap_or(chain_start_time);
        let within_window =
            transfer_window_s > 0.0 && (board_time - chain_start_time) <= transfer_window_s;

        if within_window && free_remaining > 0 {
            free_remaining -= 1;
            continue;
        }

        total += fare;
        chain_start_time = board_time;
        free_remaining = free_transfers_per_trip;
    }

    total.max(0.0)
}

fn fare_elasticity_multiplier(params: &crate::model::Params, expected_fare_base: f64) -> f64 {
    if !params.fare_enabled {
        return 1.0;
    }
    let fare_ref = params.fare_reference_base.max(0.01);
    let elasticity = params.fare_elasticity.max(0.0);
    (1.0 + expected_fare_base.max(0.0) / fare_ref).powf(-elasticity)
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
