use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::choice::logit_shares;
use super::graph;
use super::graph::BuiltPath;
use super::modes::{
    canonical_mode_from_tokens, travel_mode_family_from_tokens, CanonicalTransitMode,
};
use super::routing::crowding_multiplier;
use super::routing::{
    build_graph, build_graph_with_costs, dedupe_paths, dijkstra, k_shortest_paths,
};
use super::stateful::SimState;
use super::types;
use super::types::{
    AssignedOdFlow, AssignedPathSummary, BoardLoad, BuildPreviewMetrics, BuildPreviewType,
    CitywideModeShareSummary, CommercialStrengthClassification, CorridorClassification,
    CorridorDesireLineData, CorridorFinancialMetrics, CorridorModeShareMetrics,
    CorridorPlanningMetrics, CorridorReference, DemandDiagnostics, DemandTimeSliceLabel,
    Diagnostics, EconomicDiagnostics, EconomicRankingEntry, EventDemandModifier, FareFlowSummary,
    FareModel, FinancialPerformanceMetrics, FlowConsistencyCheck, Kpis, LatentOdDemand,
    LifecycleConservationSummary, LineOrServicePlanningMetrics, LinkLoad, ModalDemandDiagnostics,
    ModalRankingEntry, ModeChoiceContext, ModeChoiceResult, ModeGeneralizedCostBreakdown,
    ModeGeneralizedCostByMode, ModeShareValue, NetworkFinancialSummary, OdPatternMetric,
    OnTimeStatus, OperationalIncidentType, OperationalRankingEntry, OperationsReliabilityConfig,
    OutputMeta, PassengerCohortFlow, PlannerPassengerTrace, PlannerServiceStopTrace,
    PlanningDebugSummary, PlanningOverlayConfig, PurposeDemandValue, PurposeModeShareValue,
    PurposeScoreValue, PurposeTemporalDemandTotals, RecommendedServiceClass, SampleOdPaths,
    SamplePathOption, SeasonalProfile, ServiceDayModeShareSummary, ServiceDayType,
    ServiceFinancialMetrics, ServiceGapRankings, ServiceLoadLayerData, ServiceOperationState,
    ServiceReliabilityDiagnostics, ServiceRoleClassification, ServiceScoreEntry,
    ServiceTransitCaptureContext, ServiceVehicleLoadAggregate, SettlementClass, SimulationOutput,
    SimulationSettings, SocialNecessityClassification, SpecialAttractorType,
    StationFinancialContext, StationFlowAggregate, StationPlanningMetrics, StationScoreEntry,
    StationTransitCaptureContext, StopFlow, StopFlowReference, StopFlowState, StopOperationState,
    StrategicPlannerTimingDiagnostics, SyntheticEconomyConfig, TemporalCorridorPressurePoint,
    TemporalDemandDiagnostics, TemporalDemandSlice, TemporalPlanningSnapshot, TemporalRankingEntry,
    TemporalServiceGapPoint, TemporalServicePressurePoint, TemporalStationPressurePoint,
    TimeSliceDemandTotals, TimeSliceModeShareSummary, TransferOperationMetrics, TravelMode,
    TripPurpose, VehicleLoadState, WaitingByDestination, ZoneArchetype,
    ZoneDemandAttractionLayerData, ZoneDemandLayerData, ZoneDemandProductionLayerData,
    ZoneDemandProfile, ZoneEconomicGeographyLayerData, ZoneFlowReference, ZoneModeShareMetrics,
    ZonePlanningMetrics, ZoneScoreEntry, ZoneServiceGapLayerData,
};
use crate::model::{DemandCell, Scenario, Service, Stop, World, Zone};

mod assignment;
mod contracts;
mod demand;
mod economics;
mod modal;
mod mode_choice;
mod operations;
mod phase3;
mod temporal;
mod zone_geography;
use assignment::run_assignment_kernel;
use demand::build_latent_demand_foundation;
use economics::{apply_economics_to_planning, build_economics_outputs};
use modal::build_modal_outputs;
use mode_choice::apply_mode_choice_capture;
use operations::build_operations_outputs;
use temporal::build_temporal_bundle_outputs;
use zone_geography::{
    build_zone_demand_profiles, euclid_km, euclid_m, settlement_purpose_multiplier,
    settlement_rank, zone_attractor_multiplier,
};

const RT_PLANNER_LOG_INTERVAL: u64 = 10;
const RT_PLANNER_SLOW_TOTAL_MS: f64 = 250.0;
static PLANNER_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

fn service_is_active_for_sim(svc: &Service) -> bool {
    if matches!(svc.service_enabled, Some(false)) {
        return false;
    }
    let units_owned = service_units_owned_for_sim(svc);
    if units_owned == 0 {
        return false;
    }
    let units_assigned = service_units_assigned_for_sim(svc, units_owned);
    if units_assigned == 0 {
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
    true
}

fn service_units_owned_for_sim(svc: &Service) -> usize {
    svc.stock_units_owned
        .or_else(|| {
            svc.rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.units_owned)
        })
        .unwrap_or(0) as usize
}

fn service_units_assigned_for_sim(svc: &Service, units_owned: usize) -> usize {
    let raw_assigned = svc
        .stock_units_assigned
        .or_else(|| {
            svc.rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.units_owned)
        })
        .or(svc.stock_units_owned)
        .unwrap_or(0) as usize;
    let clamped = raw_assigned.min(units_owned);
    if service_mode_is_metro_for_sim(svc) && units_owned > 0 && clamped == 0 {
        units_owned
    } else {
        clamped
    }
}

fn service_mode_is_metro_for_sim(svc: &Service) -> bool {
    let mode = svc.mode.trim().to_ascii_lowercase();
    matches!(
        mode.as_str(),
        "metro" | "subway" | "underground" | "rapid_transit"
    ) || mode.contains("metro")
}

pub(super) fn path_is_boardable_transit(path: &BuiltPath) -> bool {
    matches!(
        classify_boardable_transit(path),
        BoardableTransitClassification::Boardable
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardableTransitClassification {
    Boardable,
    MissingBoardOrAlight,
    UnpairedBoardAlight,
}

pub(super) fn classify_boardable_transit(path: &BuiltPath) -> BoardableTransitClassification {
    if path.board_events.is_empty() || path.alight_events.is_empty() {
        return BoardableTransitClassification::MissingBoardOrAlight;
    }
    if path.board_events.len() != path.alight_events.len() {
        return BoardableTransitClassification::UnpairedBoardAlight;
    }
    BoardableTransitClassification::Boardable
}

fn transit_candidate_search_cap(base_k: usize) -> usize {
    base_k.saturating_mul(8).max(16).min(64)
}

pub(super) fn collect_transit_path_candidates(
    graph: &graph::Graph,
    start: usize,
    goal: usize,
    requested_k: usize,
) -> (Vec<BuiltPath>, Vec<BuiltPath>) {
    let base_k = requested_k.max(1);
    let max_k = transit_candidate_search_cap(base_k);
    let mut search_k = base_k;
    loop {
        let raw_paths = dedupe_paths(k_shortest_paths(graph, start, goal, search_k));
        let mut boardable_paths = raw_paths
            .iter()
            .filter(|path| path_is_boardable_transit(path))
            .cloned()
            .collect::<Vec<_>>();
        if boardable_paths.len() >= base_k || raw_paths.len() < search_k || search_k >= max_k {
            boardable_paths.truncate(base_k);
            return (raw_paths, boardable_paths);
        }
        search_k = search_k.saturating_mul(2).min(max_k);
    }
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
    let planner_call = PLANNER_CALL_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let planner_started = Instant::now();
    let stage_materialize_started = Instant::now();
    let s_eff = materialize_planner_effective_scenario(s);
    let stage_materialize_ms = stage_materialize_started.elapsed().as_secs_f64() * 1000.0;
    let s = &s_eff;
    let stage_validate_index_started = Instant::now();
    validate(s)?;
    let pending_od = state_in.map(|st| &st.pending_od_trips);
    let (stop_index, zone_index) = index_maps(&s.world);
    let stage_validate_index_ms = stage_validate_index_started.elapsed().as_secs_f64() * 1000.0;
    let stage_graph_started = Instant::now();
    let graph = build_graph(s, &stop_index, &zone_index)?;
    let stage_graph_ms = stage_graph_started.elapsed().as_secs_f64() * 1000.0;
    let graph_node_count = graph.adj.len();
    let graph_edge_count = graph.adj.iter().map(|edges| edges.len()).sum::<usize>();
    let sim_clock_s = state_in.map(|st| st.t_s).unwrap_or(12.0 * 3600.0);
    let coverage_mode = if include_temporal_bundle {
        LatentDemandCoverage::CanonicalSlicesForDay
    } else {
        LatentDemandCoverage::SingleContext
    };
    let stage_latent_started = Instant::now();
    let mut latent_build =
        build_latent_demand_foundation(s, &graph, sim_clock_s, temporal_context, coverage_mode)?;
    let stage_latent_ms = stage_latent_started.elapsed().as_secs_f64() * 1000.0;
    let stage_pending_od_started = Instant::now();
    let mut injected_pending_od_rows = 0usize;

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
            injected_pending_od_rows = injected_pending_od_rows.saturating_add(1);
        }
    }
    let stage_pending_od_ms = stage_pending_od_started.elapsed().as_secs_f64() * 1000.0;

    let stage_mode_choice_started = Instant::now();
    let mode_choice_build = apply_mode_choice_capture(
        s,
        settings,
        &graph,
        &latent_build.economy_config,
        &latent_build.zone_profiles,
        &latent_build.active_context,
        &latent_build.active_latent,
    )?;
    let stage_mode_choice_ms = stage_mode_choice_started.elapsed().as_secs_f64() * 1000.0;

    let stage_assignment_started = Instant::now();
    let assignment::AssignmentKernelOutputs {
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
        assignment_od_rows_with_transit_latent,
        assignment_od_rows_with_attempted,
        assignment_attempted_pax_total,
        assignment_candidate_paths_raw_total,
        assignment_candidate_paths_boardable_total,
        assignment_rejected_no_board_or_alight_total,
        assignment_rejected_unpaired_board_alight_total,
        assignment_iter_path_cache_hits,
        assignment_iter_path_cache_misses,
        assignment_kpi_path_cache_hits,
        assignment_kpi_path_cache_misses,
        service_stop_traces,
    } = run_assignment_kernel(
        s,
        settings,
        state_in,
        &stop_index,
        &zone_index,
        &mode_choice_build,
    )?;
    let stage_assignment_ms = stage_assignment_started.elapsed().as_secs_f64() * 1000.0;

    let demand_cells_nonzero_activity = s
        .world
        .demand_cells
        .iter()
        .filter(|cell| cell.residents_night > 0.0 || cell.jobs_day > 0.0)
        .count();
    let zones_nonzero_activity = s
        .world
        .zones
        .iter()
        .filter(|zone| zone.population > 0.0 || zone.jobs > 0.0)
        .count();
    let latent_pax_total = latent_build
        .active_latent
        .iter()
        .map(|od| od.latent_passengers.max(0.0))
        .sum::<f64>();
    let mode_choice_rows_with_transit_capture = mode_choice_build
        .rows
        .iter()
        .filter(|row| row.transit_captured > 0.0)
        .count();
    let mode_choice_transit_captured_pax = mode_choice_build
        .rows
        .iter()
        .map(|row| row.transit_captured.max(0.0))
        .sum::<f64>();
    let mode_choice_candidate_paths_raw_total = mode_choice_build
        .rows
        .iter()
        .map(|row| row.candidate_paths_raw)
        .sum::<usize>();
    let mode_choice_candidate_paths_boardable_total = mode_choice_build
        .rows
        .iter()
        .map(|row| row.candidate_paths_boardable)
        .sum::<usize>();
    let mode_choice_rejected_no_board_or_alight_total = mode_choice_build
        .rows
        .iter()
        .map(|row| row.rejected_no_board_or_alight)
        .sum::<usize>();
    let mode_choice_rejected_unpaired_board_alight_total = mode_choice_build
        .rows
        .iter()
        .map(|row| row.rejected_unpaired_board_alight)
        .sum::<usize>();
    let passenger_cohort_rows_total = passenger_cohorts.len();
    let passenger_cohort_attempted_pax_total = passenger_cohorts
        .iter()
        .map(|cohort| cohort.attempted_pax.max(0.0))
        .sum::<f64>();
    let (first_zero_stage, first_zero_reason) = if latent_pax_total <= 0.0 {
        (
            Some("latent_demand".to_string()),
            Some("active_slice_latent_zero".to_string()),
        )
    } else if mode_choice_transit_captured_pax <= 0.0 {
        (
            Some("mode_capture".to_string()),
            Some("transit_captured_zero".to_string()),
        )
    } else if mode_choice_candidate_paths_boardable_total <= 0 {
        (
            Some("mode_choice_path_filter".to_string()),
            Some("no_boardable_transit_paths".to_string()),
        )
    } else if assignment_attempted_pax_total <= 0.0 {
        (
            Some("assignment_attempted".to_string()),
            Some("attempted_network_zero".to_string()),
        )
    } else if passenger_cohort_attempted_pax_total <= 0.0 {
        (
            Some("cohort_materialization".to_string()),
            Some("passenger_cohorts_zero".to_string()),
        )
    } else {
        (None, None)
    };
    let planner_passenger_trace = PlannerPassengerTrace {
        demand_cells_total: s.world.demand_cells.len(),
        demand_cells_nonzero_activity,
        zones_total: s.world.zones.len(),
        zones_nonzero_activity,
        latent_rows_total: latent_build.active_latent.len(),
        latent_pax_total: latent_pax_total.max(0.0),
        mode_choice_rows_total: mode_choice_build.rows.len(),
        mode_choice_rows_with_transit_capture,
        mode_choice_transit_captured_pax: mode_choice_transit_captured_pax.max(0.0),
        mode_choice_candidate_paths_raw_total,
        mode_choice_candidate_paths_boardable_total,
        mode_choice_rejected_no_board_or_alight_total,
        mode_choice_rejected_unpaired_board_alight_total,
        assignment_od_rows_with_transit_latent,
        assignment_od_rows_with_attempted,
        assignment_attempted_pax_total: assignment_attempted_pax_total.max(0.0),
        assignment_candidate_paths_raw_total,
        assignment_candidate_paths_boardable_total,
        assignment_rejected_no_board_or_alight_total,
        assignment_rejected_unpaired_board_alight_total,
        passenger_cohort_rows_total,
        passenger_cohort_attempted_pax_total: passenger_cohort_attempted_pax_total.max(0.0),
        first_zero_stage,
        first_zero_reason,
        service_stop_traces,
    };

    let mean = |sum: f64| {
        if total_trips_served > 0.0 {
            sum / total_trips_served
        } else {
            0.0
        }
    };

    if settings.lightweight_outputs {
        let stage_lightweight_outputs_started = Instant::now();
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
        let stage_lightweight_outputs_ms =
            stage_lightweight_outputs_started.elapsed().as_secs_f64() * 1000.0;
        let planner_total_ms = planner_started.elapsed().as_secs_f64() * 1000.0;
        let should_log = planner_call.is_multiple_of(RT_PLANNER_LOG_INTERVAL)
            || planner_total_ms > RT_PLANNER_SLOW_TOTAL_MS
            || stage_assignment_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5
            || stage_mode_choice_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5;
        if should_log {
            eprintln!(
                "[rt-planner] call={} mode=lightweight total_ms={:.2} materialize_ms={:.2} validate_index_ms={:.2} graph_ms={:.2} latent_ms={:.2} pending_od_ms={:.2} mode_choice_ms={:.2} assignment_ms={:.2} lightweight_outputs_ms={:.2} include_temporal_bundle={} injected_pending_od_rows={} zones={} stops={} links={} services={} demand_cells={} graph_nodes={} graph_edges={} transfer_edges={} access_edges={} egress_edges={} active_latent_rows={} mode_choice_rows={} assigned_od_rows={} board_load_rows={} cohort_rows={} mode_paths_raw_total={} mode_paths_boardable_total={} assignment_paths_raw_total={} assignment_paths_boardable_total={} assignment_cache_iter_hits={} assignment_cache_iter_misses={} assignment_cache_kpi_hits={} assignment_cache_kpi_misses={} assignment_attempted_pax_total={:.6} first_zero_stage={} first_zero_reason={}",
                planner_call,
                planner_total_ms.max(0.0),
                stage_materialize_ms.max(0.0),
                stage_validate_index_ms.max(0.0),
                stage_graph_ms.max(0.0),
                stage_latent_ms.max(0.0),
                stage_pending_od_ms.max(0.0),
                stage_mode_choice_ms.max(0.0),
                stage_assignment_ms.max(0.0),
                stage_lightweight_outputs_ms.max(0.0),
                include_temporal_bundle,
                injected_pending_od_rows,
                s.world.zones.len(),
                s.world.stops.len(),
                s.world.links.len(),
                s.world.services.len(),
                s.world.demand_cells.len(),
                graph_node_count,
                graph_edge_count,
                graph.transfer_edges,
                graph.access_edges,
                graph.egress_edges,
                latent_build.active_latent.len(),
                mode_choice_build.rows.len(),
                assigned_od_flows.len(),
                final_board_loads.len(),
                passenger_cohorts.len(),
                mode_choice_candidate_paths_raw_total,
                mode_choice_candidate_paths_boardable_total,
                assignment_candidate_paths_raw_total,
                assignment_candidate_paths_boardable_total,
                assignment_iter_path_cache_hits,
                assignment_iter_path_cache_misses,
                assignment_kpi_path_cache_hits,
                assignment_kpi_path_cache_misses,
                assignment_attempted_pax_total.max(0.0),
                planner_passenger_trace
                    .first_zero_stage
                    .as_deref()
                    .unwrap_or("none"),
                planner_passenger_trace
                    .first_zero_reason
                    .as_deref()
                    .unwrap_or("none"),
            );
        }
        let strategic_planner_timing = StrategicPlannerTimingDiagnostics {
            source_label: "engine_strategic_planner".to_string(),
            output_mode: "lightweight".to_string(),
            total_ms: planner_total_ms.max(0.0),
            materialize_effective_scenario_ms: stage_materialize_ms.max(0.0),
            validate_index_ms: stage_validate_index_ms.max(0.0),
            graph_build_ms: stage_graph_ms.max(0.0),
            latent_demand_ms: stage_latent_ms.max(0.0),
            pending_od_ms: stage_pending_od_ms.max(0.0),
            mode_choice_ms: stage_mode_choice_ms.max(0.0),
            assignment_ms: stage_assignment_ms.max(0.0),
            lightweight_outputs_ms: stage_lightweight_outputs_ms.max(0.0),
            include_temporal_bundle,
            injected_pending_od_rows,
            zones: s.world.zones.len(),
            stops: s.world.stops.len(),
            links: s.world.links.len(),
            services: s.world.services.len(),
            demand_cells: s.world.demand_cells.len(),
            graph_nodes: graph_node_count,
            graph_edges: graph_edge_count,
            transfer_edges: graph.transfer_edges,
            access_edges: graph.access_edges,
            egress_edges: graph.egress_edges,
            active_latent_rows: latent_build.active_latent.len(),
            mode_choice_rows: mode_choice_build.rows.len(),
            assigned_od_rows: assigned_od_flows.len(),
            board_load_rows: final_board_loads.len(),
            passenger_cohort_rows: passenger_cohorts.len(),
            mode_choice_candidate_paths_raw_total,
            mode_choice_candidate_paths_boardable_total,
            assignment_candidate_paths_raw_total,
            assignment_candidate_paths_boardable_total,
            assignment_iter_path_cache_hits,
            assignment_iter_path_cache_misses,
            assignment_kpi_path_cache_hits,
            assignment_kpi_path_cache_misses,
            assignment_attempted_pax_total: assignment_attempted_pax_total.max(0.0),
            ..StrategicPlannerTimingDiagnostics::default()
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
            lifecycle_conservation: LifecycleConservationSummary::default(),
            strategic_planner_timing,
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
                planner_passenger_trace: planner_passenger_trace.clone(),
            },
        });
    }

    let stage_full_layers_started = Instant::now();
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

    let stage_full_layers_ms = stage_full_layers_started.elapsed().as_secs_f64() * 1000.0;
    let stage_operations_started = Instant::now();
    let mut operations_outputs = build_operations_outputs(
        s,
        &latent_build.active_context,
        &latent_build.economy_config.operations_reliability_config,
        &final_board_loads,
        &stop_flow_states,
        &vehicle_load_states,
        &mode_choice_build.results,
    );
    let stage_operations_ms = stage_operations_started.elapsed().as_secs_f64() * 1000.0;

    let stage_modal_started = Instant::now();
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
    let stage_modal_ms = stage_modal_started.elapsed().as_secs_f64() * 1000.0;

    let stage_phase3_started = Instant::now();
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
    let stage_phase3_ms = stage_phase3_started.elapsed().as_secs_f64() * 1000.0;

    let stage_economics_started = Instant::now();
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
    let stage_economics_ms = stage_economics_started.elapsed().as_secs_f64() * 1000.0;

    let stage_demand_diagnostics_started = Instant::now();
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

    let stage_demand_diagnostics_ms =
        stage_demand_diagnostics_started.elapsed().as_secs_f64() * 1000.0;
    let stage_temporal_started = Instant::now();
    let temporal_bundle = build_temporal_bundle_outputs(
        s,
        settings,
        include_temporal_bundle,
        &latent_build,
        &assigned_od_flows,
        &modal_outputs,
        &phase3,
        &service_gap_layer,
        &economics_outputs,
        &operations_outputs,
    )?;
    let stage_temporal_ms = stage_temporal_started.elapsed().as_secs_f64() * 1000.0;
    let active_temporal_slice = temporal_bundle.active_temporal_slice;
    let temporal_planning_snapshots = temporal_bundle.temporal_planning_snapshots;
    let temporal_demand_diagnostics = temporal_bundle.temporal_demand_diagnostics;
    let modal_demand_diagnostics = temporal_bundle.modal_demand_diagnostics;
    let economic_diagnostics = temporal_bundle.economic_diagnostics;
    let service_reliability_diagnostics = temporal_bundle.service_reliability_diagnostics;
    operations_outputs.service_reliability_diagnostics = service_reliability_diagnostics.clone();
    let planner_total_ms = planner_started.elapsed().as_secs_f64() * 1000.0;
    let should_log = planner_call.is_multiple_of(RT_PLANNER_LOG_INTERVAL)
        || planner_total_ms > RT_PLANNER_SLOW_TOTAL_MS
        || stage_assignment_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5
        || stage_mode_choice_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5
        || stage_full_layers_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5
        || stage_demand_diagnostics_ms > RT_PLANNER_SLOW_TOTAL_MS * 0.5;
    if should_log {
        eprintln!(
            "[rt-planner] call={} mode=full total_ms={:.2} materialize_ms={:.2} validate_index_ms={:.2} graph_ms={:.2} latent_ms={:.2} pending_od_ms={:.2} mode_choice_ms={:.2} assignment_ms={:.2} full_layers_ms={:.2} operations_ms={:.2} modal_ms={:.2} phase3_ms={:.2} economics_ms={:.2} demand_diagnostics_ms={:.2} temporal_bundle_ms={:.2} include_temporal_bundle={} injected_pending_od_rows={} zones={} stops={} links={} services={} demand_cells={} graph_nodes={} graph_edges={} transfer_edges={} access_edges={} egress_edges={} active_latent_rows={} mode_choice_rows={} assigned_od_rows={} board_load_rows={} cohort_rows={} mode_paths_raw_total={} mode_paths_boardable_total={} assignment_paths_raw_total={} assignment_paths_boardable_total={} assignment_cache_iter_hits={} assignment_cache_iter_misses={} assignment_cache_kpi_hits={} assignment_cache_kpi_misses={} assignment_attempted_pax_total={:.6} first_zero_stage={} first_zero_reason={} temporal_snapshots={}",
            planner_call,
            planner_total_ms.max(0.0),
            stage_materialize_ms.max(0.0),
            stage_validate_index_ms.max(0.0),
            stage_graph_ms.max(0.0),
            stage_latent_ms.max(0.0),
            stage_pending_od_ms.max(0.0),
            stage_mode_choice_ms.max(0.0),
            stage_assignment_ms.max(0.0),
            stage_full_layers_ms.max(0.0),
            stage_operations_ms.max(0.0),
            stage_modal_ms.max(0.0),
            stage_phase3_ms.max(0.0),
            stage_economics_ms.max(0.0),
            stage_demand_diagnostics_ms.max(0.0),
            stage_temporal_ms.max(0.0),
            include_temporal_bundle,
            injected_pending_od_rows,
            s.world.zones.len(),
            s.world.stops.len(),
            s.world.links.len(),
            s.world.services.len(),
            s.world.demand_cells.len(),
            graph_node_count,
            graph_edge_count,
            graph.transfer_edges,
            graph.access_edges,
            graph.egress_edges,
            latent_build.active_latent.len(),
            mode_choice_build.rows.len(),
            assigned_od_flows.len(),
            final_board_loads.len(),
            passenger_cohorts.len(),
            mode_choice_candidate_paths_raw_total,
            mode_choice_candidate_paths_boardable_total,
            assignment_candidate_paths_raw_total,
            assignment_candidate_paths_boardable_total,
            assignment_iter_path_cache_hits,
            assignment_iter_path_cache_misses,
            assignment_kpi_path_cache_hits,
            assignment_kpi_path_cache_misses,
            assignment_attempted_pax_total.max(0.0),
            planner_passenger_trace
                .first_zero_stage
                .as_deref()
                .unwrap_or("none"),
            planner_passenger_trace
                .first_zero_reason
                .as_deref()
                .unwrap_or("none"),
            temporal_planning_snapshots.len(),
        );
    }
    let strategic_planner_timing = StrategicPlannerTimingDiagnostics {
        source_label: "engine_strategic_planner".to_string(),
        output_mode: "full".to_string(),
        total_ms: planner_total_ms.max(0.0),
        materialize_effective_scenario_ms: stage_materialize_ms.max(0.0),
        validate_index_ms: stage_validate_index_ms.max(0.0),
        graph_build_ms: stage_graph_ms.max(0.0),
        latent_demand_ms: stage_latent_ms.max(0.0),
        pending_od_ms: stage_pending_od_ms.max(0.0),
        mode_choice_ms: stage_mode_choice_ms.max(0.0),
        assignment_ms: stage_assignment_ms.max(0.0),
        full_layers_ms: stage_full_layers_ms.max(0.0),
        operations_ms: stage_operations_ms.max(0.0),
        modal_outputs_ms: stage_modal_ms.max(0.0),
        phase3_outputs_ms: stage_phase3_ms.max(0.0),
        economics_outputs_ms: stage_economics_ms.max(0.0),
        demand_diagnostics_ms: stage_demand_diagnostics_ms.max(0.0),
        temporal_bundle_ms: stage_temporal_ms.max(0.0),
        include_temporal_bundle,
        injected_pending_od_rows,
        zones: s.world.zones.len(),
        stops: s.world.stops.len(),
        links: s.world.links.len(),
        services: s.world.services.len(),
        demand_cells: s.world.demand_cells.len(),
        graph_nodes: graph_node_count,
        graph_edges: graph_edge_count,
        transfer_edges: graph.transfer_edges,
        access_edges: graph.access_edges,
        egress_edges: graph.egress_edges,
        active_latent_rows: latent_build.active_latent.len(),
        mode_choice_rows: mode_choice_build.rows.len(),
        assigned_od_rows: assigned_od_flows.len(),
        board_load_rows: final_board_loads.len(),
        passenger_cohort_rows: passenger_cohorts.len(),
        mode_choice_candidate_paths_raw_total,
        mode_choice_candidate_paths_boardable_total,
        assignment_candidate_paths_raw_total,
        assignment_candidate_paths_boardable_total,
        assignment_iter_path_cache_hits,
        assignment_iter_path_cache_misses,
        assignment_kpi_path_cache_hits,
        assignment_kpi_path_cache_misses,
        assignment_attempted_pax_total: assignment_attempted_pax_total.max(0.0),
        ..StrategicPlannerTimingDiagnostics::default()
    };

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
        lifecycle_conservation: LifecycleConservationSummary::default(),
        strategic_planner_timing,
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
            planner_passenger_trace,
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
    candidate_paths_raw: usize,
    candidate_paths_boardable: usize,
    rejected_no_board_or_alight: usize,
    rejected_unpaired_board_alight: usize,
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

fn materialize_planner_effective_scenario(s: &Scenario) -> Scenario {
    // Planner-effective demand authority:
    // if demand cells are present, planner trusts cells as canonical substrate and derives
    // effective zones from them for assignment/demand profile calculations.
    // otherwise planner uses persisted world.zones as legacy fallback.
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
