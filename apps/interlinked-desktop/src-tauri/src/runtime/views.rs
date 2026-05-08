use crate::build::LineActivationReason;
use crate::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::defaults::runtime_trains_authoritative_for_manifest;
use super::fare::runtime_fare_base_per_boarding;
use super::models::{
    RuntimeBoardingDebug, RuntimeFareEvents, RuntimeOpsState, RuntimeQueueIngestDebug,
    RuntimeServiceProfile, RuntimeTrainPhase,
};
use super::service_profiles::{
    build_runtime_reverse_service_pairs, build_runtime_service_profiles,
    runtime_service_activation_reason,
};
use super::train_kernel::{
    advance_runtime_train, new_runtime_train_state, runtime_train_onboard_total,
    runtime_train_position_xy,
};

const RT_OPS_LOG_INTERVAL: u64 = 20;
const RT_OPS_SLOW_TOTAL_MS: f64 = 90.0;
const RT_OPS_SLOW_STAGE_MS: f64 = 40.0;
static RUNTIME_OPS_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
struct RuntimeQueueDisplayAggregates {
    total_pax: f64,
    by_stop: HashMap<String, f64>,
    by_line: HashMap<String, f64>,
}

fn runtime_queue_display_aggregates(
    queue_cohorts: &HashMap<(String, String, String), f64>,
    profiles_by_service: &HashMap<String, RuntimeServiceProfile>,
) -> RuntimeQueueDisplayAggregates {
    // Display-only projection cache. The authoritative passenger lifecycle is
    // engine-owned; these aggregates only avoid rescanning runtime projection
    // queues while building station/line views and debug telemetry.
    let mut aggregate = RuntimeQueueDisplayAggregates::default();
    for ((service_id, stop_id, _destination_stop_id), queued) in queue_cohorts {
        if !(queued.is_finite() && *queued > RUNTIME_QUEUE_EPS) {
            continue;
        }
        let queued = queued.max(0.0);
        aggregate.total_pax += queued;
        *aggregate.by_stop.entry(stop_id.clone()).or_insert(0.0) += queued;
        if let Some(profile) = profiles_by_service.get(service_id) {
            *aggregate
                .by_line
                .entry(profile.line_id.clone())
                .or_insert(0.0) += queued;
        }
    }
    aggregate.total_pax = aggregate.total_pax.max(0.0);
    aggregate
}

pub(crate) fn build_live_service_loads(
    gs: &interlinked_engine::platform::GameState,
) -> Vec<LiveServiceLoadLite> {
    #[derive(Default)]
    struct ServiceLoadAccumulator {
        departures_observed: usize,
        vehicle_capacity: f64,
        boarded_total: f64,
        boarded_by_stop: HashMap<String, f64>,
        alighted_by_stop: HashMap<String, f64>,
    }

    let mut service_load_map = BTreeMap::<String, ServiceLoadAccumulator>::new();
    if let Some(output) = gs.last_output.as_ref() {
        for board_load in &output.board_loads {
            let service_id = board_load.service_id.trim();
            let stop_id = board_load.stop_id.trim();
            if service_id.is_empty() || stop_id.is_empty() {
                continue;
            }
            let boarded = (board_load.served_from_arrivals + board_load.served_from_queue).max(0.0);
            let alighted = board_load.alightings_served.max(0.0);
            let vehicle_capacity = board_load.vehicle_capacity.max(0.0);
            let accumulator = service_load_map.entry(service_id.to_string()).or_default();
            accumulator.departures_observed = accumulator
                .departures_observed
                .max(board_load.departures_observed);
            accumulator.vehicle_capacity = accumulator.vehicle_capacity.max(vehicle_capacity);
            accumulator.boarded_total += boarded;
            *accumulator
                .boarded_by_stop
                .entry(stop_id.to_string())
                .or_insert(0.0) += boarded;
            *accumulator
                .alighted_by_stop
                .entry(stop_id.to_string())
                .or_insert(0.0) += alighted;
        }
    }

    let mut sequence_by_service = HashMap::<String, Vec<String>>::new();
    for service in &gs.store.scenario().world.services {
        let service_id = service.id.trim();
        if service_id.is_empty() || service.stop_sequence.is_empty() {
            continue;
        }
        sequence_by_service.insert(service_id.to_string(), service.stop_sequence.clone());
    }

    service_load_map
        .into_iter()
        .map(|(service_id, accumulator)| {
            let departures = accumulator.departures_observed as f64;
            let vehicle_capacity = accumulator.vehicle_capacity.max(0.0);
            let mut peak_onboard = 0.0_f64;

            if let Some(sequence) = sequence_by_service.get(&service_id) {
                let mut onboard = 0.0_f64;
                for stop_id in sequence {
                    if let Some(alighted) = accumulator.alighted_by_stop.get(stop_id) {
                        onboard = (onboard - alighted.max(0.0)).max(0.0);
                    }
                    if let Some(boarded) = accumulator.boarded_by_stop.get(stop_id) {
                        onboard += boarded.max(0.0);
                    }
                    peak_onboard = peak_onboard.max(onboard);
                }
            } else {
                peak_onboard = accumulator.boarded_total.max(0.0);
            }

            let per_departure_peak = if departures > 0.0 {
                peak_onboard / departures
            } else {
                0.0
            };
            let load_to_capacity = if vehicle_capacity > 0.0 {
                per_departure_peak / vehicle_capacity
            } else {
                0.0
            };

            LiveServiceLoadLite {
                service_id,
                load_to_capacity: load_to_capacity.clamp(0.0, 1.0),
            }
        })
        .collect::<Vec<_>>()
}

fn runtime_profile_leg_is_boardable(
    profile: &RuntimeServiceProfile,
    board_stop_id: &str,
    destination_stop_id: &str,
) -> bool {
    let board_index = profile
        .stop_ids
        .iter()
        .position(|stop_id| stop_id == board_stop_id);
    let destination_index = profile
        .stop_ids
        .iter()
        .position(|stop_id| stop_id == destination_stop_id);
    matches!(
        (board_index, destination_index),
        (Some(board), Some(destination)) if destination > board
    )
}

fn resolve_runtime_service_for_leg(
    profiles_by_service: &HashMap<String, RuntimeServiceProfile>,
    reverse_service_by_service: &HashMap<String, String>,
    service_id: &str,
    board_stop_id: &str,
    destination_stop_id: &str,
) -> Option<(String, bool)> {
    if let Some(profile) = profiles_by_service.get(service_id) {
        if runtime_profile_leg_is_boardable(profile, board_stop_id, destination_stop_id) {
            return Some((service_id.to_string(), false));
        }
    }
    let reverse_service_id = reverse_service_by_service.get(service_id)?;
    let reverse_profile = profiles_by_service.get(reverse_service_id)?;
    if runtime_profile_leg_is_boardable(reverse_profile, board_stop_id, destination_stop_id) {
        return Some((reverse_service_id.clone(), true));
    }
    None
}

const RUNTIME_QUEUE_EPS: f64 = 1e-12;

pub(crate) fn build_runtime_ops_views(
    state: &AppState,
    project_path: &str,
    scenario: &Scenario,
    output: Option<&SimulationOutput>,
    fare_policy: &FarePolicyManifest,
    dt_s: f64,
    topology_hash: u64,
    emit_runtime_views: bool,
) -> Result<
    (
        Vec<TrainRuntimeView>,
        Vec<StationRuntimeView>,
        Vec<LineOpsRuntimeView>,
        Vec<String>,
        RuntimeFareEvents,
    ),
    String,
> {
    let call_index = RUNTIME_OPS_CALL_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let total_started = Instant::now();
    #[derive(Debug, Clone, Default)]
    struct StationAgg {
        current_inside_pax: f64,
        capacity_pax: f64,
        declined_last_hour: f64,
        entries_per_hour: f64,
        exits_per_hour: f64,
        weighted_wait_sum_s: f64,
        weighted_wait_pax: f64,
    }
    #[derive(Debug, Clone, Default)]
    struct LineAgg {
        boardings_attempted_per_hour: f64,
        boarded_per_hour: f64,
        alighted_per_hour: f64,
        denied_boardings_per_hour: f64,
        queue_end_pax: f64,
        weighted_wait_sum_s: f64,
        weighted_wait_pax: f64,
    }
    let mut station_agg = HashMap::<String, StationAgg>::new();
    let mut line_agg = HashMap::<String, LineAgg>::new();
    let line_id_by_service = scenario
        .world
        .services
        .iter()
        .map(|service| (service.id.clone(), service_line_runtime_id(service)))
        .collect::<HashMap<_, _>>();
    let stage_overlay_aggregate_started = Instant::now();
    let output_board_load_rows = output.map(|sim| sim.board_loads.len()).unwrap_or(0);
    let output_cohort_rows = output.map(|sim| sim.passenger_cohorts.len()).unwrap_or(0);

    if emit_runtime_views {
        if let Some(sim_output) = output {
            for load in &sim_output.board_loads {
                let served_total = (load.served_from_arrivals + load.served_from_queue).max(0.0);
                let alightings = load.alightings_served.max(0.0);
                let period_s = if load.departures_observed > 0 && load.headway_s > 0.0 {
                    (load.departures_observed as f64 * load.headway_s).max(1.0)
                } else if load.departures_in_period > 0.0 && load.headway_s > 0.0 {
                    (load.departures_in_period * load.headway_s).max(1.0)
                } else {
                    300.0
                };
                let to_hour = 3600.0 / period_s.max(1.0);
                let queue_end = load.queue_end.max(0.0);
                let queue_cap = load.station_queue_capacity_pax.max(0.0);
                let overflow = load.overflow_dropped.max(0.0);
                let arrivals = load.arrivals.max(0.0);
                let admitted_entries = (arrivals - overflow).max(0.0);
                let wait_s = load.extra_wait_s.max(0.0);

                let st_entry = station_agg.entry(load.stop_id.clone()).or_default();
                st_entry.current_inside_pax += queue_end;
                st_entry.capacity_pax = st_entry.capacity_pax.max(queue_cap);
                st_entry.declined_last_hour += overflow * to_hour;
                st_entry.entries_per_hour += admitted_entries * to_hour;
                st_entry.exits_per_hour += alightings * to_hour;
                st_entry.weighted_wait_sum_s += wait_s * served_total;
                st_entry.weighted_wait_pax += served_total;

                if let Some(line_id) = line_id_by_service.get(&load.service_id) {
                    let ln_entry = line_agg.entry(line_id.clone()).or_default();
                    ln_entry.boardings_attempted_per_hour += arrivals * to_hour;
                    ln_entry.boarded_per_hour += served_total * to_hour;
                    ln_entry.alighted_per_hour += alightings * to_hour;
                    ln_entry.denied_boardings_per_hour += load.denied_boardings.max(0.0) * to_hour;
                    ln_entry.queue_end_pax += queue_end;
                    ln_entry.weighted_wait_sum_s += wait_s * served_total;
                    ln_entry.weighted_wait_pax += served_total;
                }
            }
        }
    }
    let stage_overlay_aggregate_ms =
        stage_overlay_aggregate_started.elapsed().as_secs_f64() * 1000.0;

    let stage_lock_started = Instant::now();
    let mut guard = state
        .runtime_ops
        .lock()
        .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
    let stage_lock_ms = stage_lock_started.elapsed().as_secs_f64() * 1000.0;
    let should_reset = guard
        .as_ref()
        .map(|ops| ops.project_path != project_path)
        .unwrap_or(true);
    if should_reset {
        *guard = Some(RuntimeOpsState {
            project_path: project_path.to_string(),
            topology_hash,
            profiles_by_service: HashMap::new(),
            stop_name_by_id: HashMap::new(),
            reverse_service_by_service: HashMap::new(),
            stop_ids_by_service: HashMap::new(),
            fare_base_by_service: HashMap::new(),
            dispatch_service_ids: HashSet::new(),
            trains: BTreeMap::new(),
            queue_cohorts: HashMap::new(),
            last_queue_ingest_by_service_stop: HashMap::new(),
            last_boarding_by_service_stop: HashMap::new(),
        });
    }
    let Some(ops) = guard.as_mut() else {
        return Ok((
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RuntimeFareEvents::default(),
        ));
    };
    let stage_topology_refresh_started = Instant::now();
    let mut topology_refreshed = false;
    if ops.topology_hash != topology_hash || ops.profiles_by_service.is_empty() {
        topology_refreshed = true;
        let (profiles_by_service, stop_name_by_id) = build_runtime_service_profiles(scenario);
        let dispatch_service_ids = profiles_by_service.keys().cloned().collect::<HashSet<_>>();
        let reverse_service_by_service = build_runtime_reverse_service_pairs(&profiles_by_service);
        let stop_ids_by_service = scenario
            .world
            .services
            .iter()
            .map(|service| {
                (
                    service.id.clone(),
                    service
                        .stop_sequence
                        .iter()
                        .cloned()
                        .collect::<HashSet<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        let fare_base_by_service = profiles_by_service
            .iter()
            .map(|(service_id, profile)| {
                (
                    service_id.clone(),
                    runtime_fare_base_per_boarding(fare_policy, &profile.mode),
                )
            })
            .collect::<HashMap<_, _>>();
        ops.profiles_by_service = profiles_by_service;
        ops.stop_name_by_id = stop_name_by_id;
        ops.reverse_service_by_service = reverse_service_by_service;
        ops.stop_ids_by_service = stop_ids_by_service;
        ops.fare_base_by_service = fare_base_by_service;
        ops.dispatch_service_ids = dispatch_service_ids;
    }
    let stage_topology_refresh_ms = stage_topology_refresh_started.elapsed().as_secs_f64() * 1000.0;
    ops.topology_hash = topology_hash;
    let queue_cohorts_before_ingest = ops.queue_cohorts.len();
    let stage_queue_ingest_started = Instant::now();
    let mut queue_ingest_debug = HashMap::<(String, String), RuntimeQueueIngestDebug>::new();
    ops.queue_cohorts
        .retain(|(service_id, board_stop_id, destination_stop_id), queued| {
            ops.dispatch_service_ids.contains(service_id)
                && ops
                    .profiles_by_service
                    .get(service_id)
                    .map(|profile| {
                        runtime_profile_leg_is_boardable(
                            profile,
                            board_stop_id,
                            destination_stop_id,
                        )
                    })
                    .unwrap_or(false)
                && queued.is_finite()
                && *queued > RUNTIME_QUEUE_EPS
        });
    if let Some(sim_output) = output {
        let mut arrivals_by_key = HashMap::<(String, String, String), f64>::new();
        let mut remap_logs_emitted = 0usize;
        for cohort in &sim_output.passenger_cohorts {
            let arrivals = cohort.attempted_pax.max(0.0);
            if arrivals <= 0.0 {
                continue;
            }
            let resolved_service = resolve_runtime_service_for_leg(
                &ops.profiles_by_service,
                &ops.reverse_service_by_service,
                &cohort.service_id,
                &cohort.board_stop_id,
                &cohort.destination_stop_id,
            );
            let (runtime_service_id, remapped_to_reverse) =
                if let Some((service_id, remapped)) = resolved_service {
                    (service_id, remapped)
                } else {
                    (cohort.service_id.clone(), false)
                };
            let debug_key = (runtime_service_id.clone(), cohort.board_stop_id.clone());
            let debug_entry = queue_ingest_debug.entry(debug_key).or_default();
            debug_entry.attempted_pax += arrivals;
            if remapped_to_reverse {
                debug_entry.remapped_to_reverse_service_pax += arrivals;
                if remap_logs_emitted < 12 {
                    eprintln!(
                        "[pax-runtime-remap] planner_service={} runtime_service={} board_stop={} destination_stop={} attempted={:.6}",
                        cohort.service_id,
                        runtime_service_id,
                        cohort.board_stop_id,
                        cohort.destination_stop_id,
                        arrivals,
                    );
                    remap_logs_emitted = remap_logs_emitted.saturating_add(1);
                }
            }
            if !ops.dispatch_service_ids.contains(&runtime_service_id) {
                debug_entry.dropped_not_dispatchable_pax += arrivals;
                continue;
            }
            let valid_leg = ops
                .profiles_by_service
                .get(&runtime_service_id)
                .map(|profile| {
                    runtime_profile_leg_is_boardable(
                        profile,
                        &cohort.board_stop_id,
                        &cohort.destination_stop_id,
                    )
                })
                .unwrap_or(false);
            if !valid_leg {
                debug_entry.dropped_invalid_stop_pax += arrivals;
                continue;
            }
            debug_entry.ingested_pax += arrivals;
            let key = (
                runtime_service_id,
                cohort.board_stop_id.clone(),
                cohort.destination_stop_id.clone(),
            );
            *arrivals_by_key.entry(key).or_insert(0.0) += arrivals;
        }
        let mut sorted_arrivals = arrivals_by_key.into_iter().collect::<Vec<_>>();
        sorted_arrivals.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, arrivals) in sorted_arrivals {
            let current = ops.queue_cohorts.get(&key).copied().unwrap_or(0.0);
            let next = (current + arrivals).max(0.0);
            if next > RUNTIME_QUEUE_EPS {
                ops.queue_cohorts.insert(key, next);
            } else {
                ops.queue_cohorts.remove(&key);
            }
        }
    }
    ops.last_queue_ingest_by_service_stop = queue_ingest_debug;
    ops.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > RUNTIME_QUEUE_EPS);
    let stage_queue_ingest_ms = stage_queue_ingest_started.elapsed().as_secs_f64() * 1000.0;
    let queue_cohorts_after_ingest = ops.queue_cohorts.len();

    let stage_train_state_sync_started = Instant::now();
    let mut expected_train_ids = HashSet::<String>::new();
    for profile in ops.profiles_by_service.values() {
        for unit_index in 0..profile.vehicles_on_service {
            let train_id = format!(
                "train::{}::{}::{}",
                profile.line_id,
                profile.service_id,
                unit_index.saturating_add(1)
            );
            expected_train_ids.insert(train_id.clone());
            let entry = ops
                .trains
                .entry(train_id.clone())
                .or_insert_with(|| new_runtime_train_state(profile, unit_index));
            entry.train_id = train_id;
            // Preserve any active reverse-service handoff for this physical train slot.
            if !ops.profiles_by_service.contains_key(&entry.service_id) {
                entry.service_id = profile.service_id.clone();
            }
            if let Some(active_profile) = ops.profiles_by_service.get(&entry.service_id) {
                if active_profile.line_id != profile.line_id {
                    entry.service_id = profile.service_id.clone();
                }
            }
            let active_profile = ops
                .profiles_by_service
                .get(&entry.service_id)
                .unwrap_or(profile);
            entry.line_id = active_profile.line_id.clone();
            entry.line_name = active_profile.line_name.clone();
            entry.mode = active_profile.mode.clone();
            entry.mode_variant = active_profile.mode_variant.clone();
            entry.stock_tier_id = active_profile.stock_tier_id.clone();
            entry.vehicle_capacity = active_profile.vehicle_capacity.max(0.0);
            if entry.current_stop_index >= active_profile.stop_ids.len() {
                entry.current_stop_index = 0;
                entry.phase = RuntimeTrainPhase::Dwell;
                entry.progress = 0.0;
                entry.remaining_s = active_profile.dwell_s;
                entry.direction_step = 1;
                entry.onboard_pax = 0.0;
                entry.onboard_cohorts.clear();
            }
        }
    }
    ops.trains
        .retain(|train_id, _| expected_train_ids.contains(train_id));
    let stage_train_state_sync_ms = stage_train_state_sync_started.elapsed().as_secs_f64() * 1000.0;

    let queue_display_before_boarding =
        runtime_queue_display_aggregates(&ops.queue_cohorts, &ops.profiles_by_service);
    let queue_total_before_boarding = queue_display_before_boarding.total_pax;
    let mut fare_events = RuntimeFareEvents::default();
    let stage_boarding_started = Instant::now();
    let mut boarding_debug = HashMap::<(String, String), RuntimeBoardingDebug>::new();
    let mut boarding_event_count = 0usize;
    for train in ops.trains.values_mut() {
        if let Some(profile) = ops.profiles_by_service.get(&train.service_id) {
            let fare_base_per_boarding = ops
                .fare_base_by_service
                .get(&train.service_id)
                .copied()
                .unwrap_or(0.0);
            let delta = advance_runtime_train(
                train,
                profile,
                dt_s,
                &mut ops.queue_cohorts,
                fare_base_per_boarding,
            );
            fare_events.boarded_pax += delta.fare.boarded_pax;
            fare_events.completed_alightings_pax += delta.fare.completed_alightings_pax;
            fare_events.liability_accrued_base += delta.fare.liability_accrued_base;
            for event in delta.boarding_events {
                boarding_event_count = boarding_event_count.saturating_add(1);
                let key = (event.service_id, event.stop_id);
                let entry = boarding_debug.entry(key).or_default();
                entry.attempts = entry.attempts.saturating_add(1);
                entry.attempted_pax += event.attempted_pax.max(0.0);
                entry.boarded_pax += event.boarded_pax.max(0.0);
                entry.left_behind_pax += event.left_behind_pax.max(0.0);
                entry.queue_total_before_pax += event.queue_total_before_pax.max(0.0);
                entry.queue_total_after_pax += event.queue_total_after_pax.max(0.0);
            }
            train.onboard_pax = runtime_train_onboard_total(train);

            if train.phase == RuntimeTrainPhase::Layover && train.direction_step < 0 {
                if let Some(reverse_service_id) =
                    ops.reverse_service_by_service.get(&train.service_id)
                {
                    if let Some(reverse_profile) = ops.profiles_by_service.get(reverse_service_id) {
                        let current_stop_id = profile
                            .stop_ids
                            .get(train.current_stop_index)
                            .cloned()
                            .unwrap_or_default();
                        if let Some(reverse_index) = reverse_profile
                            .stop_ids
                            .iter()
                            .position(|stop_id| stop_id == &current_stop_id)
                        {
                            train.service_id = reverse_profile.service_id.clone();
                            train.line_id = reverse_profile.line_id.clone();
                            train.line_name = reverse_profile.line_name.clone();
                            train.mode = reverse_profile.mode.clone();
                            train.mode_variant = reverse_profile.mode_variant.clone();
                            train.stock_tier_id = reverse_profile.stock_tier_id.clone();
                            train.vehicle_capacity = reverse_profile.vehicle_capacity.max(0.0);
                            train.current_stop_index = reverse_index;
                            train.direction_step = 1;
                        }
                    }
                }
            }
        }
    }
    let stage_boarding_ms = stage_boarding_started.elapsed().as_secs_f64() * 1000.0;
    ops.last_boarding_by_service_stop = boarding_debug;
    let queue_display_after_boarding =
        runtime_queue_display_aggregates(&ops.queue_cohorts, &ops.profiles_by_service);
    let queue_total_after_boarding = queue_display_after_boarding.total_pax;
    let queue_cohorts_after_boarding = ops.queue_cohorts.len();

    if !emit_runtime_views {
        let mut provenance_warnings = Vec::<String>::new();
        if !ops.profiles_by_service.is_empty() {
            provenance_warnings.push(
                "runtime_projection: runtime ops advanced without materializing views on this tick"
                    .to_string(),
            );
        }
        let total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
        let should_log = call_index.is_multiple_of(RT_OPS_LOG_INTERVAL)
            || total_ms > RT_OPS_SLOW_TOTAL_MS
            || stage_queue_ingest_ms > RT_OPS_SLOW_STAGE_MS
            || stage_boarding_ms > RT_OPS_SLOW_STAGE_MS;
        if should_log {
            eprintln!(
                "[rt-ops] call={} project={} emit_runtime_views={} dt_s={:.3} total_ms={:.2} overlay_aggregate_ms={:.2} lock_ms={:.2} topology_refresh_ms={:.2} topology_refreshed={} queue_ingest_ms={:.2} train_state_sync_ms={:.2} boarding_ms={:.2} services={} profiles={} dispatchable_services={} trains={} output_board_load_rows={} output_cohort_rows={} queue_cohorts={}=>{}=>{} queue_total_before_boarding={:.3} queue_total_after_boarding={:.3} boarding_events={} boarded_pax={:.3} alighted_pax={:.3} warnings={}",
                call_index,
                project_path,
                emit_runtime_views,
                dt_s.max(0.0),
                total_ms.max(0.0),
                stage_overlay_aggregate_ms.max(0.0),
                stage_lock_ms.max(0.0),
                stage_topology_refresh_ms.max(0.0),
                topology_refreshed,
                stage_queue_ingest_ms.max(0.0),
                stage_train_state_sync_ms.max(0.0),
                stage_boarding_ms.max(0.0),
                scenario.world.services.len(),
                ops.profiles_by_service.len(),
                ops.dispatch_service_ids.len(),
                ops.trains.len(),
                output_board_load_rows,
                output_cohort_rows,
                queue_cohorts_before_ingest,
                queue_cohorts_after_ingest,
                queue_cohorts_after_boarding,
                queue_total_before_boarding.max(0.0),
                queue_total_after_boarding.max(0.0),
                boarding_event_count,
                fare_events.boarded_pax.max(0.0),
                fare_events.completed_alightings_pax.max(0.0),
                provenance_warnings.len(),
            );
        }
        return Ok((
            Vec::new(),
            Vec::new(),
            Vec::new(),
            provenance_warnings,
            fare_events,
        ));
    }

    let stage_view_build_started = Instant::now();
    let mut trains_sorted = ops.trains.values().cloned().collect::<Vec<_>>();
    trains_sorted.sort_by(|a, b| {
        a.line_id
            .cmp(&b.line_id)
            .then_with(|| a.train_id.cmp(&b.train_id))
    });
    let mut line_ordinals = HashMap::<String, u32>::new();
    let mut train_views = Vec::<TrainRuntimeView>::new();
    for train in trains_sorted {
        let Some(profile) = ops.profiles_by_service.get(&train.service_id) else {
            continue;
        };
        let (x, y, at_stop_id, in_motion) = runtime_train_position_xy(&train, profile);
        let direction_label = if train.direction_step >= 0 {
            "Outbound".to_string()
        } else {
            "Inbound".to_string()
        };
        let destination_index = if train.direction_step >= 0 {
            profile.stop_ids.len().saturating_sub(1)
        } else {
            0
        };
        let destination_stop_id = profile
            .stop_ids
            .get(destination_index)
            .cloned()
            .unwrap_or_default();
        let destination_label = ops
            .stop_name_by_id
            .get(&destination_stop_id)
            .map(|name| format!("To {name}"))
            .unwrap_or_else(|| direction_label.clone());
        let vehicle_ordinal = line_ordinals
            .entry(train.line_id.clone())
            .and_modify(|v| *v = v.saturating_add(1))
            .or_insert(1);
        train_views.push(TrainRuntimeView {
            train_id: train.train_id.clone(),
            service_id: train.service_id.clone(),
            line_id: train.line_id.clone(),
            line_name: train.line_name.clone(),
            vehicle_ordinal: *vehicle_ordinal,
            direction_label,
            destination_stop_id,
            destination_label,
            mode: train.mode.clone(),
            mode_variant: train.mode_variant.clone(),
            stock_tier_id: train.stock_tier_id.clone(),
            vehicle_capacity: train.vehicle_capacity.max(0.0),
            onboard_pax: train.onboard_pax.max(0.0),
            x,
            y,
            at_stop_id,
            in_motion,
            provenance: CounterProvenance::AnimationOnly,
            passenger_counter_provenance: CounterProvenance::AnimationOnly,
        });
    }

    let mut station_ids = station_agg.keys().cloned().collect::<BTreeSet<_>>();
    station_ids.extend(queue_display_after_boarding.by_stop.keys().cloned());

    let mut station_views = Vec::<StationRuntimeView>::new();
    for stop_id in station_ids {
        let agg = station_agg.get(&stop_id).cloned().unwrap_or_default();
        let queue_inside = queue_display_after_boarding
            .by_stop
            .get(&stop_id)
            .copied()
            .unwrap_or(0.0);
        let current_inside = if agg.capacity_pax > 0.0 {
            queue_inside.min(agg.capacity_pax)
        } else {
            queue_inside
        };
        let avg_wait = if agg.weighted_wait_pax > 0.0 {
            agg.weighted_wait_sum_s / agg.weighted_wait_pax
        } else {
            0.0
        };
        station_views.push(StationRuntimeView {
            stop_id,
            current_inside_pax: current_inside.max(0.0),
            capacity_pax: agg.capacity_pax.max(0.0),
            declined_last_hour: agg.declined_last_hour.max(0.0),
            entries_per_hour: agg.entries_per_hour.max(0.0),
            exits_per_hour: agg.exits_per_hour.max(0.0),
            avg_wait_to_board_s: avg_wait.max(0.0),
            provenance: CounterProvenance::RuntimeProjection,
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
        });
    }
    station_views.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut active_trains_by_line = HashMap::<String, u32>::new();
    for train in &train_views {
        *active_trains_by_line
            .entry(train.line_id.clone())
            .or_insert(0) += 1;
    }
    let mut line_ids = line_agg
        .keys()
        .cloned()
        .chain(active_trains_by_line.keys().cloned())
        .chain(queue_display_after_boarding.by_line.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    line_ids.sort();
    let mut line_ops = Vec::<LineOpsRuntimeView>::new();
    for line_id in line_ids {
        let agg = line_agg.get(&line_id).cloned().unwrap_or_default();
        let mean_wait_s = if agg.weighted_wait_pax > 0.0 {
            agg.weighted_wait_sum_s / agg.weighted_wait_pax
        } else {
            0.0
        };
        line_ops.push(LineOpsRuntimeView {
            line_id: line_id.clone(),
            active_trains: *active_trains_by_line.get(&line_id).unwrap_or(&0),
            boardings_attempted_per_hour: agg.boardings_attempted_per_hour.max(0.0),
            boarded_per_hour: agg.boarded_per_hour.max(0.0),
            alighted_per_hour: agg.alighted_per_hour.max(0.0),
            denied_boardings_per_hour: agg.denied_boardings_per_hour.max(0.0),
            queue_end_pax: queue_display_after_boarding
                .by_line
                .get(&line_id)
                .copied()
                .unwrap_or(0.0)
                .max(0.0),
            mean_wait_s: mean_wait_s.max(0.0),
            provenance: CounterProvenance::RuntimeProjection,
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
        });
    }

    let mut provenance_warnings = Vec::<String>::new();
    if !ops.profiles_by_service.is_empty() {
        provenance_warnings.push(
            "runtime_projection: station/line flow is reconstructed from board-load events; animation_only: train onboard is reconstructed from desktop train cohorts"
                .to_string(),
        );
    }
    let queue_drop = (queue_total_before_boarding - queue_total_after_boarding).max(0.0);
    let boarding_mass_mismatch = (queue_drop - fare_events.boarded_pax.max(0.0)).abs();
    if boarding_mass_mismatch > 1e-3 {
        provenance_warnings.push(format!(
            "runtime_projection: queue/boarding conservation mismatch detected ({boarding_mass_mismatch:.3} pax)"
        ));
    }
    let mut suppressed_reason_counts = BTreeMap::<String, usize>::new();
    for service in &scenario.world.services {
        let reason = runtime_service_activation_reason(service);
        if reason == LineActivationReason::Running {
            continue;
        }
        let reason_key = match reason {
            LineActivationReason::NoTargetTphInActiveBand => "no_target_tph_in_active_band",
            LineActivationReason::NoAssignedUnits => "no_assigned_units",
            LineActivationReason::NoOwnedUnits => "no_owned_units",
            LineActivationReason::FleetInsufficientForRoundTrip => {
                "fleet_insufficient_for_round_trip"
            }
            LineActivationReason::InvalidHeadwayOrDisabled => "invalid_headway_or_disabled",
            LineActivationReason::NoRequiredUnits => "no_required_units",
            LineActivationReason::Running => "running",
        };
        *suppressed_reason_counts
            .entry(reason_key.to_string())
            .or_insert(0) += 1;
    }
    if !suppressed_reason_counts.is_empty() {
        let summary = suppressed_reason_counts
            .into_iter()
            .map(|(reason, count)| format!("{reason}:{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        provenance_warnings.push(format!(
            "runtime_projection: service dispatch suppressed ({summary})"
        ));
    }
    let stage_view_build_ms = stage_view_build_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
    let should_log = call_index.is_multiple_of(RT_OPS_LOG_INTERVAL)
        || total_ms > RT_OPS_SLOW_TOTAL_MS
        || stage_queue_ingest_ms > RT_OPS_SLOW_STAGE_MS
        || stage_boarding_ms > RT_OPS_SLOW_STAGE_MS
        || stage_view_build_ms > RT_OPS_SLOW_STAGE_MS;
    if should_log {
        eprintln!(
            "[rt-ops] call={} project={} emit_runtime_views={} dt_s={:.3} total_ms={:.2} overlay_aggregate_ms={:.2} lock_ms={:.2} topology_refresh_ms={:.2} topology_refreshed={} queue_ingest_ms={:.2} train_state_sync_ms={:.2} boarding_ms={:.2} view_build_ms={:.2} services={} profiles={} dispatchable_services={} trains={} runtime_train_views={} runtime_station_views={} runtime_line_ops={} output_board_load_rows={} output_cohort_rows={} queue_cohorts={}=>{}=>{} queue_total_before_boarding={:.3} queue_total_after_boarding={:.3} boarding_events={} boarded_pax={:.3} alighted_pax={:.3} warnings={}",
            call_index,
            project_path,
            emit_runtime_views,
            dt_s.max(0.0),
            total_ms.max(0.0),
            stage_overlay_aggregate_ms.max(0.0),
            stage_lock_ms.max(0.0),
            stage_topology_refresh_ms.max(0.0),
            topology_refreshed,
            stage_queue_ingest_ms.max(0.0),
            stage_train_state_sync_ms.max(0.0),
            stage_boarding_ms.max(0.0),
            stage_view_build_ms.max(0.0),
            scenario.world.services.len(),
            ops.profiles_by_service.len(),
            ops.dispatch_service_ids.len(),
            ops.trains.len(),
            train_views.len(),
            station_views.len(),
            line_ops.len(),
            output_board_load_rows,
            output_cohort_rows,
            queue_cohorts_before_ingest,
            queue_cohorts_after_ingest,
            queue_cohorts_after_boarding,
            queue_total_before_boarding.max(0.0),
            queue_total_after_boarding.max(0.0),
            boarding_event_count,
            fare_events.boarded_pax.max(0.0),
            fare_events.completed_alightings_pax.max(0.0),
            provenance_warnings.len(),
        );
    }

    Ok((
        train_views,
        station_views,
        line_ops,
        provenance_warnings,
        fare_events,
    ))
}

pub(crate) fn runtime_snapshot_to_advance(
    snapshot: &RuntimeSnapshot,
) -> Option<SimulationAdvanceResult> {
    snapshot.frame.clone().map(|frame| SimulationAdvanceResult {
        frame,
        clock: snapshot.clock.clone(),
        economy: snapshot.economy.clone(),
        delta_revenue_base: snapshot.delta_revenue_base,
        delta_opex_base: snapshot.delta_opex_base,
        delta_net_base: snapshot.delta_net_base,
    })
}

pub(crate) fn merge_runtime_manifest_state(
    mut reloaded: ProjectManifest,
    runtime: &ProjectManifest,
) -> ProjectManifest {
    let use_runtime_economy = runtime.economy.economy_revision > reloaded.economy.economy_revision;
    reloaded.clock_state.tick_seconds = reloaded
        .clock_state
        .tick_seconds
        .max(runtime.clock_state.tick_seconds);
    if use_runtime_economy {
        reloaded.economy = runtime.economy.clone();
        match (
            reloaded.progress_metrics.as_mut(),
            runtime.progress_metrics.as_ref(),
        ) {
            (Some(reloaded_metrics), Some(runtime_metrics)) => {
                reloaded_metrics.budget = runtime_metrics.budget;
                reloaded_metrics.ridership = runtime_metrics.ridership;
            }
            (None, Some(runtime_metrics)) => {
                reloaded.progress_metrics = Some(runtime_metrics.clone());
            }
            _ => {}
        }
    } else {
        sync_progress_budget_from_economy(&mut reloaded);
        match (
            reloaded.progress_metrics.as_mut(),
            runtime.progress_metrics.as_ref(),
        ) {
            (Some(reloaded_metrics), Some(runtime_metrics)) => {
                reloaded_metrics.ridership =
                    reloaded_metrics.ridership.max(runtime_metrics.ridership);
            }
            (None, Some(runtime_metrics)) => {
                reloaded.progress_metrics = Some(runtime_metrics.clone());
            }
            _ => {}
        }
    }
    reloaded
}

pub(crate) fn default_runtime_fast_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeFastSnapshot {
    RuntimeFastSnapshot {
        project_path: project_path.to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry: RuntimePerfTelemetry::default(),
        trains: Vec::new(),
        stations: Vec::new(),
        line_ops: Vec::new(),
        provenance_warnings: Vec::new(),
        trains_authoritative: runtime_trains_authoritative_for_manifest(manifest),
        passenger_counter_provenance: CounterProvenance::RuntimeProjection,
    }
}

pub(crate) fn default_runtime_strategic_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeStrategicSnapshot {
    RuntimeStrategicSnapshot {
        project_path: project_path.to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        economy: SimulationAdvanceEconomy {
            current_balance_base: manifest.economy.current_balance_base,
            cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
            cumulative_opex_base: manifest.economy.cumulative_opex_base,
            budget_display: manifest
                .progress_metrics
                .as_ref()
                .map(|m| m.budget)
                .unwrap_or(manifest.economy.current_balance_base),
        },
        frame: None,
        delta_revenue_base: 0.0,
        delta_opex_base: 0.0,
        delta_net_base: 0.0,
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry: RuntimePerfTelemetry::default(),
        provenance_warnings: Vec::new(),
        trains_authoritative: runtime_trains_authoritative_for_manifest(manifest),
        passenger_counter_provenance: CounterProvenance::StrategicEstimate,
        fare_counter_provenance: CounterProvenance::StrategicEstimate,
    }
}

pub(crate) fn default_runtime_snapshot_for_manifest(
    project_path: &str,
    manifest: &ProjectManifest,
    clock_revision: u64,
) -> RuntimeSnapshot {
    let fast = default_runtime_fast_snapshot_for_manifest(project_path, manifest, clock_revision);
    let strategic =
        default_runtime_strategic_snapshot_for_manifest(project_path, manifest, clock_revision);
    runtime_snapshot_from_parts(&fast, Some(&strategic))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile(service_id: &str, line_id: &str, stops: &[&str]) -> RuntimeServiceProfile {
        RuntimeServiceProfile {
            service_id: service_id.to_string(),
            line_id: line_id.to_string(),
            line_name: line_id.to_string(),
            mode: "metro".to_string(),
            mode_variant: None,
            stock_tier_id: None,
            dwell_s: 15.0,
            turnaround_s: 30.0,
            speed_mps: 12.0,
            vehicle_capacity: 500.0,
            vehicles_on_service: 1,
            stop_ids: stops.iter().map(|value| value.to_string()).collect(),
            stop_xy: vec![(0.0, 0.0), (1.0, 0.0)],
            segment_lengths_m: vec![1000.0],
        }
    }

    #[test]
    fn profile_leg_validation_requires_forward_stop_order() {
        let profile = test_profile("svc_fwd", "line_1", &["A", "B"]);
        assert!(runtime_profile_leg_is_boardable(&profile, "A", "B"));
        assert!(!runtime_profile_leg_is_boardable(&profile, "B", "A"));
        assert!(!runtime_profile_leg_is_boardable(&profile, "A", "A"));
    }

    #[test]
    fn service_resolution_remaps_to_reverse_pair_when_direction_mismatched() {
        let forward_id = "svc_fwd".to_string();
        let reverse_id = "auto_reverse::line_1::svc_fwd".to_string();
        let mut profiles = HashMap::<String, RuntimeServiceProfile>::new();
        profiles.insert(
            forward_id.clone(),
            test_profile(&forward_id, "line_1", &["A", "B"]),
        );
        profiles.insert(
            reverse_id.clone(),
            test_profile(&reverse_id, "line_1", &["B", "A"]),
        );
        let mut reverse_pairs = HashMap::<String, String>::new();
        reverse_pairs.insert(forward_id.clone(), reverse_id.clone());
        reverse_pairs.insert(reverse_id.clone(), forward_id.clone());

        let resolved_forward =
            resolve_runtime_service_for_leg(&profiles, &reverse_pairs, &forward_id, "A", "B");
        assert_eq!(resolved_forward, Some((forward_id.clone(), false)));

        let resolved_from_reverse =
            resolve_runtime_service_for_leg(&profiles, &reverse_pairs, &reverse_id, "A", "B");
        assert_eq!(resolved_from_reverse, Some((forward_id.clone(), true)));

        let resolved_to_reverse =
            resolve_runtime_service_for_leg(&profiles, &reverse_pairs, &forward_id, "B", "A");
        assert_eq!(resolved_to_reverse, Some((reverse_id.clone(), true)));
    }

    #[test]
    fn queue_display_aggregates_match_projection_source_totals() {
        let profile = test_profile("svc_fwd", "line_1", &["A", "B"]);
        let profiles =
            HashMap::<String, RuntimeServiceProfile>::from([(profile.service_id.clone(), profile)]);
        let queue_cohorts = HashMap::<(String, String, String), f64>::from([
            (
                ("svc_fwd".to_string(), "A".to_string(), "B".to_string()),
                4.0,
            ),
            (
                ("svc_fwd".to_string(), "A".to_string(), "C".to_string()),
                3.0,
            ),
            (
                ("svc_unknown".to_string(), "Z".to_string(), "Q".to_string()),
                2.0,
            ),
        ]);

        let aggregate = runtime_queue_display_aggregates(&queue_cohorts, &profiles);

        assert_eq!(aggregate.total_pax, 9.0);
        assert_eq!(aggregate.by_stop.get("A").copied().unwrap_or(0.0), 7.0);
        assert_eq!(aggregate.by_stop.get("Z").copied().unwrap_or(0.0), 2.0);
        assert_eq!(aggregate.by_line.get("line_1").copied().unwrap_or(0.0), 7.0);
        assert!(!aggregate.by_line.contains_key("svc_unknown"));
    }
}

pub(crate) fn bootstrap_runtime_snapshot_from_state(
    state: &AppState,
    project_path: &str,
    manifest: &ProjectManifest,
    scenario: &Scenario,
    clock_revision: u64,
) -> Result<RuntimeSnapshot, String> {
    let mut snapshot =
        default_runtime_snapshot_for_manifest(project_path, manifest, clock_revision);
    snapshot.clock.running = manifest.clock_state.running;
    snapshot.clock.speed = normalize_speed(manifest.clock_state.speed);
    snapshot.clock.tick_seconds = manifest.clock_state.tick_seconds;
    snapshot.clock_revision = clock_revision;
    snapshot.captured_at_epoch_ms = now_epoch_ms();
    snapshot.telemetry.snapshot_age_ms = 0;
    let trains_authoritative = runtime_trains_authoritative_for_manifest(manifest);
    snapshot.trains_authoritative = trains_authoritative;
    if trains_authoritative {
        let topology_hash = scenario_topology_hash(scenario);
        let (trains, stations, line_ops, warnings, _fare_events) = build_runtime_ops_views(
            state,
            project_path,
            scenario,
            None,
            &manifest.economy.fare_policy,
            0.0,
            topology_hash,
            true,
        )?;
        snapshot.trains = trains;
        snapshot.stations = stations;
        snapshot.line_ops = line_ops;
        snapshot.provenance_warnings = warnings;
    }
    Ok(snapshot)
}
