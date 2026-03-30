use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Instant;

use crate::model::Scenario;
use crate::sim::{
    BoardLoad, BoardingTimeBin, FareFlowSummary, PassengerCohortFlow, ServiceLoadLayerData,
    SimState, SimulationOutput, StopFlow,
};

type ServiceStopKey = (String, String);
type ServiceStopDestKey = (String, String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategicRefreshReason {
    MissingCache,
    CadenceInterval,
    ScopeChanged,
    TopologyChanged,
    InvalidatedByEdit,
    ExplicitForce,
}

#[derive(Debug, Clone, Default)]
pub struct KernelPerfMetrics {
    pub fast_steps: u64,
    pub strategic_steps: u64,
    pub strategic_cache_hits: u64,
    pub strategic_cache_misses: u64,
    pub invalidation_count: u64,
    pub fast_total_ms: f64,
    pub strategic_total_ms: f64,
    pub last_fast_ms: f64,
    pub last_strategic_ms: f64,
    pub steps_since_last_strategic: u32,
    pub strategic_refresh_interval_steps: u32,
    pub last_refresh_reason: Option<StrategicRefreshReason>,
}

impl KernelPerfMetrics {
    pub fn avg_fast_ms(&self) -> f64 {
        if self.fast_steps == 0 {
            0.0
        } else {
            self.fast_total_ms / self.fast_steps as f64
        }
    }

    pub fn avg_strategic_ms(&self) -> f64 {
        if self.strategic_steps == 0 {
            0.0
        } else {
            self.strategic_total_ms / self.strategic_steps as f64
        }
    }
}

#[derive(Debug, Clone)]
pub struct OperationalBoardTemplate {
    pub service_id: String,
    pub stop_id: String,
    pub arrivals_per_s: f64,
    pub headway_s: f64,
    pub vehicle_capacity: f64,
    pub station_capacity_boarding_pph: f64,
    pub station_capacity_alighting_pph: f64,
    pub station_queue_capacity_pax: f64,
    pub baseline_extra_wait_s: f64,
    pub alightings_per_departure: f64,
}

#[derive(Debug, Clone)]
pub struct OperationalCohortTemplate {
    pub destination_stop_id: String,
    pub arrivals_per_s: f64,
}

#[derive(Debug, Clone)]
pub struct StrategicKernelCache {
    pub topology_signature: u64,
    pub scope_signature: u64,
    pub refreshed_at_tick_s: f64,
    pub board_templates: HashMap<ServiceStopKey, OperationalBoardTemplate>,
    pub cohort_templates_by_board: HashMap<ServiceStopKey, Vec<OperationalCohortTemplate>>,
    pub fast_output_skeleton: SimulationOutput,
}

#[derive(Debug, Clone, Default)]
pub struct KernelPartitionState {
    pub strategic_cache: Option<StrategicKernelCache>,
    pub perf: KernelPerfMetrics,
    pub invalidated: bool,
    pub explicit_refresh_requested: bool,
    pub last_scope_signature: u64,
    pub last_topology_signature: u64,
}

impl KernelPartitionState {
    pub fn mark_invalidated(&mut self, reason: StrategicRefreshReason) {
        self.invalidated = true;
        self.perf.invalidation_count = self.perf.invalidation_count.saturating_add(1);
        self.perf.last_refresh_reason = Some(reason);
    }
}

pub fn scenario_topology_signature(s: &Scenario) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.world.zones.len().hash(&mut hasher);
    s.world.stops.len().hash(&mut hasher);
    s.world.links.len().hash(&mut hasher);
    s.world.services.len().hash(&mut hasher);
    for stop in &s.world.stops {
        stop.id.hash(&mut hasher);
    }
    for link in &s.world.links {
        link.id.hash(&mut hasher);
        link.from_stop.hash(&mut hasher);
        link.to_stop.hash(&mut hasher);
        link.mode.hash(&mut hasher);
    }
    for svc in &s.world.services {
        svc.id.hash(&mut hasher);
        svc.mode.hash(&mut hasher);
        svc.headway_s.to_bits().hash(&mut hasher);
        svc.vehicle_capacity.to_bits().hash(&mut hasher);
        svc.stop_sequence.len().hash(&mut hasher);
    }
    hasher.finish()
}

fn strip_fast_output_skeleton(mut out: SimulationOutput) -> SimulationOutput {
    out.link_loads.clear();
    out.board_loads.clear();
    out.stop_flows.clear();
    out.passenger_cohorts.clear();
    out.stop_flow_states.clear();
    out.vehicle_load_states.clear();
    out.service_operation_states.clear();
    out.stop_operation_states.clear();
    out.transfer_operation_metrics.clear();
    out.zone_demand_profiles.clear();
    out.latent_od_demand.clear();
    out.assigned_od_flows.clear();
    out.mode_choice_results.clear();
    out.zone_demand_layer.clear();
    out.zone_economic_geography_layer.clear();
    out.zone_demand_production_layer.clear();
    out.zone_demand_attraction_layer.clear();
    out.corridor_desire_lines.clear();
    out.service_gap_layer.clear();
    out.service_load_layer.clear();
    out.zone_planning_metrics.clear();
    out.station_planning_metrics.clear();
    out.corridor_planning_metrics.clear();
    out.line_service_planning_metrics.clear();
    out.service_financial_metrics.clear();
    out.corridor_financial_metrics.clear();
    out.station_financial_context.clear();
    out.zone_mode_share_metrics.clear();
    out.corridor_mode_share_metrics.clear();
    out.station_transit_capture_context.clear();
    out.service_transit_capture_context.clear();
    out.build_preview_metrics.clear();
    out.temporal_planning_snapshots.clear();
    out
}

pub fn build_strategic_kernel_cache(
    output: &SimulationOutput,
    topology_signature: u64,
    scope_signature: u64,
    tick_s: f64,
) -> StrategicKernelCache {
    let period_s = (output.meta.time_period_hours * 3600.0).max(1.0);
    let mut board_templates = HashMap::<ServiceStopKey, OperationalBoardTemplate>::new();
    for load in &output.board_loads {
        let departures = if load.departures_observed > 0 {
            load.departures_observed as f64
        } else if load.departures_in_period > 0.0 {
            load.departures_in_period
        } else {
            1.0
        };
        let key = (load.service_id.clone(), load.stop_id.clone());
        board_templates.insert(
            key,
            OperationalBoardTemplate {
                service_id: load.service_id.clone(),
                stop_id: load.stop_id.clone(),
                arrivals_per_s: load.arrivals.max(0.0) / period_s,
                headway_s: load.headway_s.max(1.0),
                vehicle_capacity: load.vehicle_capacity.max(0.0),
                station_capacity_boarding_pph: load.station_capacity_boarding_pph.max(0.0),
                station_capacity_alighting_pph: load.station_capacity_alighting_pph.max(0.0),
                station_queue_capacity_pax: load.station_queue_capacity_pax.max(0.0),
                baseline_extra_wait_s: load.extra_wait_s.max(0.0),
                alightings_per_departure: load.alightings_served.max(0.0) / departures.max(1e-6),
            },
        );
    }

    let mut cohort_templates_by_board =
        HashMap::<ServiceStopKey, Vec<OperationalCohortTemplate>>::new();
    for cohort in &output.passenger_cohorts {
        let arrivals_per_s = cohort.attempted_pax.max(0.0) / period_s;
        if arrivals_per_s <= 0.0 {
            continue;
        }
        cohort_templates_by_board
            .entry((cohort.service_id.clone(), cohort.board_stop_id.clone()))
            .or_default()
            .push(OperationalCohortTemplate {
                destination_stop_id: cohort.destination_stop_id.clone(),
                arrivals_per_s,
            });
    }
    for templates in cohort_templates_by_board.values_mut() {
        templates.sort_by(|a, b| a.destination_stop_id.cmp(&b.destination_stop_id));
    }

    let fast_output_skeleton = strip_fast_output_skeleton(output.clone());
    StrategicKernelCache {
        topology_signature,
        scope_signature,
        refreshed_at_tick_s: tick_s,
        board_templates,
        cohort_templates_by_board,
        fast_output_skeleton,
    }
}

fn queue_total_for_service_stop(
    queue_cohorts: &HashMap<ServiceStopDestKey, f64>,
    service_id: &str,
    stop_id: &str,
) -> f64 {
    queue_cohorts
        .iter()
        .filter_map(|((svc, stop, _dest), queued)| {
            if svc == service_id && stop == stop_id && queued.is_finite() && *queued > 0.0 {
                Some(*queued)
            } else {
                None
            }
        })
        .sum::<f64>()
        .max(0.0)
}

fn board_from_queue_cohorts(
    queue_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    service_id: &str,
    stop_id: &str,
    mut capacity: f64,
    boarded_by_key: &mut HashMap<ServiceStopDestKey, f64>,
) -> f64 {
    if capacity <= 1e-9 {
        return 0.0;
    }
    let mut keys = queue_cohorts
        .keys()
        .filter(|(svc, stop, _dest)| svc == service_id && stop == stop_id)
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    let mut boarded_total = 0.0_f64;
    for key in keys {
        if capacity <= 1e-9 {
            break;
        }
        let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
        if queued <= 1e-9 {
            continue;
        }
        let boarded = queued.min(capacity);
        capacity = (capacity - boarded).max(0.0);
        boarded_total += boarded;
        let remaining = (queued - boarded).max(0.0);
        if remaining > 1e-9 {
            queue_cohorts.insert(key.clone(), remaining);
        } else {
            queue_cohorts.remove(&key);
        }
        *boarded_by_key.entry(key).or_insert(0.0) += boarded;
    }
    boarded_total
}

fn apply_queue_overflow_cap(
    queue_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    service_id: &str,
    stop_id: &str,
    queue_cap: f64,
) -> f64 {
    if !(queue_cap.is_finite() && queue_cap > 0.0) {
        return 0.0;
    }
    let total = queue_total_for_service_stop(queue_cohorts, service_id, stop_id);
    if total <= queue_cap + 1e-9 {
        return 0.0;
    }
    let overflow = total - queue_cap;
    if total <= 1e-9 {
        return 0.0;
    }
    let scale = (queue_cap / total).clamp(0.0, 1.0);
    let keys = queue_cohorts
        .keys()
        .filter(|(svc, stop, _dest)| svc == service_id && stop == stop_id)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
        let next = (queued * scale).max(0.0);
        if next > 1e-9 {
            queue_cohorts.insert(key, next);
        } else {
            queue_cohorts.remove(&key);
        }
    }
    overflow.max(0.0)
}

pub fn run_fast_operational_step(
    cache: &StrategicKernelCache,
    state_in: &SimState,
    dt_s: f64,
) -> Result<(SimulationOutput, SimState), String> {
    if dt_s <= 0.0 || !dt_s.is_finite() {
        return Err("dt_s must be > 0".to_string());
    }
    let mut next = state_in.clone();
    next.t_s += dt_s;
    next.pending_od_trips.clear();
    next.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > 1e-9);

    let mut board_keys = cache.board_templates.keys().cloned().collect::<Vec<_>>();
    board_keys.sort();

    let mut arrivals_by_board = HashMap::<ServiceStopKey, f64>::new();
    let mut attempted_by_cohort = HashMap::<ServiceStopDestKey, f64>::new();
    let mut boarded_by_cohort = HashMap::<ServiceStopDestKey, f64>::new();
    let mut board_loads = Vec::<BoardLoad>::new();
    let mut stop_flows_by_stop = BTreeMap::<String, StopFlow>::new();

    let mut total_attempted = 0.0_f64;
    let mut total_served = 0.0_f64;
    let mut total_denied = 0.0_f64;
    let mut total_overflow = 0.0_f64;
    let mut total_alighted = 0.0_f64;

    for (service_id, stop_id) in board_keys {
        let Some(template) = cache
            .board_templates
            .get(&(service_id.clone(), stop_id.clone()))
        else {
            continue;
        };
        let mut arrivals = 0.0_f64;
        if let Some(cohort_templates) = cache
            .cohort_templates_by_board
            .get(&(service_id.clone(), stop_id.clone()))
        {
            for cohort_template in cohort_templates {
                let attempted = (cohort_template.arrivals_per_s * dt_s).max(0.0);
                if attempted <= 0.0 {
                    continue;
                }
                arrivals += attempted;
                total_attempted += attempted;
                let key = (
                    service_id.clone(),
                    stop_id.clone(),
                    cohort_template.destination_stop_id.clone(),
                );
                *next.queue_cohorts.entry(key.clone()).or_insert(0.0) += attempted;
                *attempted_by_cohort.entry(key).or_insert(0.0) += attempted;
            }
        } else {
            let attempted = (template.arrivals_per_s * dt_s).max(0.0);
            if attempted > 0.0 {
                arrivals += attempted;
                total_attempted += attempted;
                let key = (
                    service_id.clone(),
                    stop_id.clone(),
                    "__unknown__".to_string(),
                );
                *next.queue_cohorts.entry(key.clone()).or_insert(0.0) += attempted;
                *attempted_by_cohort.entry(key).or_insert(0.0) += attempted;
            }
        }
        arrivals_by_board.insert((service_id.clone(), stop_id.clone()), arrivals);

        let queue_start = queue_total_for_service_stop(&next.queue_cohorts, &service_id, &stop_id);
        let mut time_to_next = next
            .time_to_next_departure_s
            .get(&(service_id.clone(), stop_id.clone()))
            .copied()
            .unwrap_or(template.headway_s.max(1.0));
        if !time_to_next.is_finite() || time_to_next < 0.0 {
            time_to_next = template.headway_s.max(1.0);
        }

        let mut remaining = dt_s.max(0.0);
        let mut departures_observed = 0usize;
        let mut served_total = 0.0_f64;
        let mut boarding_cap_remaining = if template.station_capacity_boarding_pph > 0.0 {
            template.station_capacity_boarding_pph * dt_s / 3600.0
        } else {
            f64::INFINITY
        };
        while remaining + 1e-9 >= time_to_next.max(1e-6) {
            remaining -= time_to_next.max(1e-6);
            departures_observed = departures_observed.saturating_add(1);
            time_to_next = template.headway_s.max(1e-3);
            if boarding_cap_remaining <= 1e-9 {
                continue;
            }
            let departure_capacity = template
                .vehicle_capacity
                .max(0.0)
                .min(boarding_cap_remaining.max(0.0));
            let boarded = board_from_queue_cohorts(
                &mut next.queue_cohorts,
                &service_id,
                &stop_id,
                departure_capacity,
                &mut boarded_by_cohort,
            );
            served_total += boarded;
            if boarding_cap_remaining.is_finite() {
                boarding_cap_remaining = (boarding_cap_remaining - boarded).max(0.0);
            }
        }
        let time_to_next_departure_s_end = (time_to_next - remaining).max(0.0);
        next.time_to_next_departure_s.insert(
            (service_id.clone(), stop_id.clone()),
            time_to_next_departure_s_end,
        );

        let overflow_dropped = apply_queue_overflow_cap(
            &mut next.queue_cohorts,
            &service_id,
            &stop_id,
            template.station_queue_capacity_pax,
        );
        let queue_end = queue_total_for_service_stop(&next.queue_cohorts, &service_id, &stop_id);
        next.queue
            .insert((service_id.clone(), stop_id.clone()), queue_end);

        let served_from_arrivals = served_total.min(arrivals.max(0.0));
        let served_from_queue = (served_total - served_from_arrivals).max(0.0);
        let denied_boardings = (arrivals - served_from_arrivals).max(0.0);
        let alightings_served =
            (template.alightings_per_departure * departures_observed as f64).max(0.0);
        let extra_wait_s = if template.arrivals_per_s > 1e-6 {
            (queue_end / template.arrivals_per_s).max(template.baseline_extra_wait_s)
        } else {
            template.baseline_extra_wait_s
        };
        let departures_in_period = departures_observed as f64;
        let capacity_in_period = template.vehicle_capacity.max(0.0) * departures_in_period;

        total_served += served_total;
        total_denied += denied_boardings;
        total_overflow += overflow_dropped;
        total_alighted += alightings_served;

        board_loads.push(BoardLoad {
            service_id: service_id.clone(),
            stop_id: stop_id.clone(),
            arrivals,
            served_from_arrivals,
            served_from_queue,
            denied_boardings,
            queue_start,
            queue_end,
            headway_s: template.headway_s.max(1.0),
            vehicle_capacity: template.vehicle_capacity.max(0.0),
            departures_in_period,
            departures_observed,
            capacity_in_period,
            extra_wait_s,
            time_bins: vec![BoardingTimeBin {
                bin_index: 0,
                arrivals,
                served: served_total,
                queue_end,
                departures: departures_observed,
                capacity: capacity_in_period,
            }],
            time_to_next_departure_s_end,
            alightings_served,
            station_capacity_boarding_pph: template.station_capacity_boarding_pph.max(0.0),
            station_capacity_alighting_pph: template.station_capacity_alighting_pph.max(0.0),
            station_queue_capacity_pax: template.station_queue_capacity_pax.max(0.0),
            overflow_dropped,
        });

        let stop_flow = stop_flows_by_stop
            .entry(stop_id.clone())
            .or_insert_with(|| StopFlow {
                stop_id: stop_id.clone(),
                boardings_attempted: 0.0,
                boardings_served: 0.0,
                alightings_attempted: 0.0,
                alightings_served: 0.0,
                queue_start: 0.0,
                queue_end: 0.0,
                overflow_dropped: 0.0,
            });
        stop_flow.boardings_attempted += arrivals.max(0.0);
        stop_flow.boardings_served += served_total.max(0.0);
        stop_flow.alightings_attempted += alightings_served.max(0.0);
        stop_flow.alightings_served += alightings_served.max(0.0);
        stop_flow.queue_start += queue_start.max(0.0);
        stop_flow.queue_end += queue_end.max(0.0);
        stop_flow.overflow_dropped += overflow_dropped.max(0.0);
    }

    next.queue
        .retain(|_, queued| queued.is_finite() && *queued > 1e-9);
    next.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > 1e-9);
    let valid_board_keys = cache
        .board_templates
        .keys()
        .cloned()
        .collect::<HashSet<ServiceStopKey>>();
    next.time_to_next_departure_s
        .retain(|key, _| valid_board_keys.contains(key));

    let mut cohort_keys = attempted_by_cohort
        .keys()
        .chain(boarded_by_cohort.keys())
        .chain(next.queue_cohorts.keys())
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    cohort_keys.sort();
    let mut passenger_cohorts = Vec::<PassengerCohortFlow>::with_capacity(cohort_keys.len());
    for key in cohort_keys {
        let attempted = attempted_by_cohort
            .get(&key)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let boarded = boarded_by_cohort.get(&key).copied().unwrap_or(0.0).max(0.0);
        let queue_end = next
            .queue_cohorts
            .get(&key)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        passenger_cohorts.push(PassengerCohortFlow {
            service_id: key.0,
            board_stop_id: key.1,
            destination_stop_id: key.2,
            attempted_pax: attempted,
            boarded_pax: boarded,
            alighted_pax: 0.0,
            queue_end_pax: queue_end,
        });
    }

    let mut out = cache.fast_output_skeleton.clone();
    out.meta.time_period_hours = dt_s / 3600.0;
    out.meta.results_version = "0.2.2-fast-operational-v1".to_string();
    out.board_loads = board_loads;
    out.stop_flows = stop_flows_by_stop.into_values().collect::<Vec<_>>();
    out.passenger_cohorts = passenger_cohorts;
    out.service_load_layer = out
        .board_loads
        .iter()
        .fold(
            BTreeMap::<String, ServiceLoadLayerData>::new(),
            |mut acc, load| {
                let service =
                    acc.entry(load.service_id.clone())
                        .or_insert_with(|| ServiceLoadLayerData {
                            service_id: load.service_id.clone(),
                            line_id: None,
                            passengers: 0.0,
                            peak_load: 0.0,
                            peak_load_stop_id: None,
                        });
                let boarded = (load.served_from_arrivals + load.served_from_queue).max(0.0);
                service.passengers += boarded;
                if load.queue_end > service.peak_load {
                    service.peak_load = load.queue_end;
                    service.peak_load_stop_id = Some(load.stop_id.clone());
                }
                acc
            },
        )
        .into_values()
        .collect::<Vec<_>>();
    out.service_load_layer
        .sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let mut kpis = out.kpis.clone();
    kpis.total_boardings_attempted = total_attempted.max(0.0);
    kpis.total_boardings_served = total_served.max(0.0);
    kpis.total_boardings_denied = total_denied.max(0.0);
    kpis.share_boardings_served = if total_attempted > 1e-9 {
        (total_served / total_attempted).clamp(0.0, 1.0)
    } else {
        1.0
    };
    kpis.total_overflow_dropped = total_overflow.max(0.0);
    kpis.share_demand_overflow_dropped = if total_attempted > 1e-9 {
        (total_overflow / total_attempted).clamp(0.0, 1.0)
    } else {
        0.0
    };
    kpis.total_trips_attempted = total_attempted.max(0.0);
    kpis.total_trips_served = total_served.max(0.0);
    kpis.share_trips_served = if total_attempted > 1e-9 {
        (total_served / total_attempted).clamp(0.0, 1.0)
    } else {
        1.0
    };
    kpis.total_trips = total_served.max(0.0);
    kpis.total_fare_revenue_base = 0.0;
    out.kpis = kpis;
    out.fare_flow = FareFlowSummary {
        liability_accrued_base: 0.0,
        liability_accrued_pax: total_served.max(0.0),
        completed_journeys_pax: total_alighted.max(0.0),
        recognized_revenue_base: 0.0,
    };
    Ok((out, next))
}

pub fn should_run_strategic_refresh(
    kernel_state: &KernelPartitionState,
    strategic_refresh_interval_steps: u32,
    topology_signature: u64,
    scope_signature: u64,
) -> Option<StrategicRefreshReason> {
    if kernel_state.explicit_refresh_requested {
        return Some(StrategicRefreshReason::ExplicitForce);
    }
    if kernel_state.strategic_cache.is_none() {
        return Some(StrategicRefreshReason::MissingCache);
    }
    if kernel_state.invalidated {
        return Some(StrategicRefreshReason::InvalidatedByEdit);
    }
    if kernel_state.last_scope_signature != 0
        && kernel_state.last_scope_signature != scope_signature
    {
        return Some(StrategicRefreshReason::ScopeChanged);
    }
    if kernel_state.last_topology_signature != 0
        && kernel_state.last_topology_signature != topology_signature
    {
        return Some(StrategicRefreshReason::TopologyChanged);
    }
    if kernel_state.perf.steps_since_last_strategic >= strategic_refresh_interval_steps.max(1) {
        return Some(StrategicRefreshReason::CadenceInterval);
    }
    None
}

pub fn update_fast_step_metrics(kernel_state: &mut KernelPartitionState, started: Instant) {
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    kernel_state.perf.fast_steps = kernel_state.perf.fast_steps.saturating_add(1);
    kernel_state.perf.strategic_cache_hits =
        kernel_state.perf.strategic_cache_hits.saturating_add(1);
    kernel_state.perf.fast_total_ms += elapsed_ms.max(0.0);
    kernel_state.perf.last_fast_ms = elapsed_ms.max(0.0);
    kernel_state.perf.steps_since_last_strategic = kernel_state
        .perf
        .steps_since_last_strategic
        .saturating_add(1);
}

pub fn update_strategic_step_metrics(
    kernel_state: &mut KernelPartitionState,
    started: Instant,
    reason: StrategicRefreshReason,
    interval_steps: u32,
) {
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    kernel_state.perf.strategic_steps = kernel_state.perf.strategic_steps.saturating_add(1);
    kernel_state.perf.strategic_cache_misses =
        kernel_state.perf.strategic_cache_misses.saturating_add(1);
    kernel_state.perf.strategic_total_ms += elapsed_ms.max(0.0);
    kernel_state.perf.last_strategic_ms = elapsed_ms.max(0.0);
    kernel_state.perf.steps_since_last_strategic = 0;
    kernel_state.perf.strategic_refresh_interval_steps = interval_steps.max(1);
    kernel_state.perf.last_refresh_reason = Some(reason);
}
