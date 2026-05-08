use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use crate::model::Scenario;
use crate::sim::{
    step_simulation, BoardLoad, BoardingTimeBin, EngineFarePolicyContext, FareFlowSummary,
    LifecycleConservationSummary, PassengerCohortFlow, RunConfig, ServiceLoadLayerData, SimState,
    SimulationOutput, StopFlow,
};

type ServiceStopKey = (String, String);
type ServiceStopDestKey = (String, String, String);
type CohortServiceStopIndex = HashMap<ServiceStopKey, Vec<ServiceStopDestKey>>;
const FAST_KERNEL_QUEUE_EPS: f64 = 1e-12;
const RT_FAST_KERNEL_LOG_INTERVAL: u64 = 30;
const RT_FAST_KERNEL_SLOW_MS: f64 = 80.0;
static FAST_KERNEL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    pub strategic_async_launches: u64,
    pub strategic_async_completions: u64,
    pub strategic_async_discards: u64,
    pub strategic_async_failures: u64,
    pub strategic_async_last_ms: f64,
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

#[derive(Debug)]
struct StrategicRefreshJobCompletion {
    topology_signature: u64,
    scope_signature: u64,
    cache: Result<StrategicKernelCache, String>,
    elapsed_ms: f64,
}

#[derive(Debug)]
struct PendingStrategicRefreshJob {
    requested_reason: StrategicRefreshReason,
    topology_signature: u64,
    scope_signature: u64,
    rx: Receiver<StrategicRefreshJobCompletion>,
}

#[derive(Debug, Default)]
pub struct KernelPartitionState {
    pub strategic_cache: Option<StrategicKernelCache>,
    pub perf: KernelPerfMetrics,
    pub invalidated: bool,
    pub explicit_refresh_requested: bool,
    pub last_scope_signature: u64,
    pub last_topology_signature: u64,
    pending_cadence_refresh: Option<PendingStrategicRefreshJob>,
}

impl Clone for KernelPartitionState {
    fn clone(&self) -> Self {
        Self {
            strategic_cache: self.strategic_cache.clone(),
            perf: self.perf.clone(),
            invalidated: self.invalidated,
            explicit_refresh_requested: self.explicit_refresh_requested,
            last_scope_signature: self.last_scope_signature,
            last_topology_signature: self.last_topology_signature,
            pending_cadence_refresh: None,
        }
    }
}

impl KernelPartitionState {
    pub fn mark_invalidated(&mut self, reason: StrategicRefreshReason) {
        self.invalidated = true;
        self.perf.invalidation_count = self.perf.invalidation_count.saturating_add(1);
        self.perf.last_refresh_reason = Some(reason);
        if self.pending_cadence_refresh.take().is_some() {
            self.perf.strategic_async_discards =
                self.perf.strategic_async_discards.saturating_add(1);
        }
    }

    pub fn has_pending_cadence_refresh(&self) -> bool {
        self.pending_cadence_refresh.is_some()
    }

    pub fn discard_pending_cadence_refresh(&mut self) {
        if self.pending_cadence_refresh.take().is_some() {
            self.perf.strategic_async_discards =
                self.perf.strategic_async_discards.saturating_add(1);
        }
    }

    pub fn launch_cadence_refresh_job(
        &mut self,
        scenario: &Scenario,
        run_cfg: &RunConfig,
        sim_state: &SimState,
        dt_s: f64,
        topology_signature: u64,
        scope_signature: u64,
    ) {
        if self.pending_cadence_refresh.is_some() {
            return;
        }
        let scenario_owned = scenario.clone();
        let run_cfg_owned = run_cfg.clone();
        let sim_state_owned = sim_state.clone();
        let (tx, rx) = mpsc::channel::<StrategicRefreshJobCompletion>();
        let _ = std::thread::Builder::new()
            .name("interlinked-strategic-refresh".to_string())
            .spawn(move || {
                let started = Instant::now();
                let cache =
                    step_simulation(&scenario_owned, &run_cfg_owned, &sim_state_owned, dt_s).map(
                        |(out, next)| {
                            build_strategic_kernel_cache(
                                &out,
                                topology_signature,
                                scope_signature,
                                next.t_s,
                            )
                        },
                    );
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                let _ = tx.send(StrategicRefreshJobCompletion {
                    topology_signature,
                    scope_signature,
                    cache,
                    elapsed_ms: elapsed_ms.max(0.0),
                });
            });
        self.pending_cadence_refresh = Some(PendingStrategicRefreshJob {
            requested_reason: StrategicRefreshReason::CadenceInterval,
            topology_signature,
            scope_signature,
            rx,
        });
        self.perf.strategic_async_launches = self.perf.strategic_async_launches.saturating_add(1);
    }

    pub fn poll_cadence_refresh_job(
        &mut self,
        topology_signature: u64,
        scope_signature: u64,
    ) -> Option<StrategicRefreshReason> {
        let completion = {
            let Some(job) = self.pending_cadence_refresh.as_mut() else {
                return None;
            };
            match job.rx.try_recv() {
                Ok(done) => done,
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.pending_cadence_refresh = None;
                    self.perf.strategic_async_failures =
                        self.perf.strategic_async_failures.saturating_add(1);
                    return None;
                }
            }
        };
        let pending = self.pending_cadence_refresh.take();
        let pending_reason = pending
            .as_ref()
            .map(|job| job.requested_reason)
            .unwrap_or(StrategicRefreshReason::CadenceInterval);
        let pending_topology_signature = pending
            .as_ref()
            .map(|job| job.topology_signature)
            .unwrap_or(completion.topology_signature);
        let pending_scope_signature = pending
            .as_ref()
            .map(|job| job.scope_signature)
            .unwrap_or(completion.scope_signature);
        self.perf.strategic_async_last_ms = completion.elapsed_ms.max(0.0);
        match completion.cache {
            Ok(cache) => {
                let signature_matches = pending_topology_signature == topology_signature
                    && pending_scope_signature == scope_signature
                    && completion.topology_signature == topology_signature
                    && completion.scope_signature == scope_signature;
                if self.invalidated || !signature_matches {
                    self.perf.strategic_async_discards =
                        self.perf.strategic_async_discards.saturating_add(1);
                    return None;
                }
                self.strategic_cache = Some(cache);
                self.perf.strategic_async_completions =
                    self.perf.strategic_async_completions.saturating_add(1);
                self.perf.strategic_steps = self.perf.strategic_steps.saturating_add(1);
                self.perf.strategic_cache_misses =
                    self.perf.strategic_cache_misses.saturating_add(1);
                self.perf.strategic_total_ms += completion.elapsed_ms.max(0.0);
                self.perf.last_strategic_ms = completion.elapsed_ms.max(0.0);
                self.perf.steps_since_last_strategic = 0;
                self.perf.last_refresh_reason = Some(pending_reason);
                Some(pending_reason)
            }
            Err(error) => {
                eprintln!("[rt-kernel] strategic_async_error error={}", error);
                self.perf.strategic_async_failures =
                    self.perf.strategic_async_failures.saturating_add(1);
                None
            }
        }
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

#[cfg(test)]
fn queue_total_for_service_stop_scan(
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

fn board_index_key(key: &ServiceStopDestKey) -> ServiceStopKey {
    (key.0.clone(), key.1.clone())
}

fn alight_index_key(key: &ServiceStopDestKey) -> ServiceStopKey {
    (key.0.clone(), key.2.clone())
}

fn push_cohort_index_key(
    index: &mut CohortServiceStopIndex,
    service_stop_key: ServiceStopKey,
    cohort_key: ServiceStopDestKey,
) {
    let keys = index.entry(service_stop_key).or_default();
    if !keys.contains(&cohort_key) {
        keys.push(cohort_key);
    }
}

fn build_queue_board_index(
    queue_cohorts: &HashMap<ServiceStopDestKey, f64>,
) -> CohortServiceStopIndex {
    let mut index = CohortServiceStopIndex::new();
    for (key, queued) in queue_cohorts {
        if queued.is_finite() && *queued > FAST_KERNEL_QUEUE_EPS {
            push_cohort_index_key(&mut index, board_index_key(key), key.clone());
        }
    }
    for keys in index.values_mut() {
        keys.sort();
    }
    index
}

fn build_onboard_alight_index(
    onboard_cohorts: &HashMap<ServiceStopDestKey, f64>,
) -> CohortServiceStopIndex {
    let mut index = CohortServiceStopIndex::new();
    for (key, onboard) in onboard_cohorts {
        if onboard.is_finite() && *onboard > FAST_KERNEL_QUEUE_EPS {
            push_cohort_index_key(&mut index, alight_index_key(key), key.clone());
        }
    }
    for keys in index.values_mut() {
        keys.sort();
    }
    index
}

fn indexed_cohort_total(
    cohorts: &HashMap<ServiceStopDestKey, f64>,
    index: &CohortServiceStopIndex,
    service_stop_key: &ServiceStopKey,
) -> f64 {
    index
        .get(service_stop_key)
        .into_iter()
        .flat_map(|keys| keys.iter())
        .filter_map(|key| {
            let pax = cohorts.get(key).copied().unwrap_or(0.0);
            if pax.is_finite() && pax > 0.0 {
                Some(pax)
            } else {
                None
            }
        })
        .sum::<f64>()
        .max(0.0)
}

fn cohort_total(cohorts: &HashMap<ServiceStopDestKey, f64>) -> f64 {
    cohorts
        .values()
        .filter_map(|pax| {
            if pax.is_finite() && *pax > 0.0 {
                Some(*pax)
            } else {
                None
            }
        })
        .sum::<f64>()
        .max(0.0)
}

fn board_from_queue_cohorts(
    queue_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    onboard_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    queue_board_index: &CohortServiceStopIndex,
    onboard_alight_index: &mut CohortServiceStopIndex,
    service_stop_key: &ServiceStopKey,
    mut capacity: f64,
    boarded_by_key: &mut HashMap<ServiceStopDestKey, f64>,
) -> f64 {
    if capacity <= FAST_KERNEL_QUEUE_EPS {
        return 0.0;
    }
    let mut boarded_total = 0.0_f64;
    let Some(keys) = queue_board_index.get(service_stop_key) else {
        return 0.0;
    };
    for key in keys.iter().cloned() {
        if capacity <= FAST_KERNEL_QUEUE_EPS {
            break;
        }
        let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
        if queued <= FAST_KERNEL_QUEUE_EPS {
            continue;
        }
        let boarded = queued.min(capacity);
        capacity = (capacity - boarded).max(0.0);
        boarded_total += boarded;
        let remaining = (queued - boarded).max(0.0);
        if remaining > FAST_KERNEL_QUEUE_EPS {
            queue_cohorts.insert(key.clone(), remaining);
        } else {
            queue_cohorts.remove(&key);
        }
        *onboard_cohorts.entry(key.clone()).or_insert(0.0) += boarded;
        push_cohort_index_key(onboard_alight_index, alight_index_key(&key), key.clone());
        *boarded_by_key.entry(key).or_insert(0.0) += boarded;
    }
    boarded_total
}

fn alight_from_onboard_cohorts(
    onboard_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    onboard_alight_index: &CohortServiceStopIndex,
    service_stop_key: &ServiceStopKey,
    alighted_by_key: &mut HashMap<ServiceStopDestKey, f64>,
) -> f64 {
    let mut alighted_total = 0.0_f64;
    let Some(keys) = onboard_alight_index.get(service_stop_key) else {
        return 0.0;
    };
    for key in keys.iter().cloned() {
        let alighted = onboard_cohorts.remove(&key).unwrap_or(0.0).max(0.0);
        if alighted <= FAST_KERNEL_QUEUE_EPS {
            continue;
        }
        alighted_total += alighted;
        *alighted_by_key.entry(key).or_insert(0.0) += alighted;
    }

    alighted_total.max(0.0)
}

fn completed_cohort_fare_delta_base(
    fare_policy_context: &EngineFarePolicyContext,
    service_id: &str,
    alighted_pax: f64,
) -> f64 {
    let alighted_pax = alighted_pax.max(0.0);
    if alighted_pax <= FAST_KERNEL_QUEUE_EPS {
        return 0.0;
    }
    fare_policy_context
        .fare_for_service(service_id)
        .map(|fare| fare * alighted_pax)
        .unwrap_or(0.0)
        .max(0.0)
}

#[allow(clippy::too_many_arguments)]
fn build_lifecycle_conservation_summary(
    queue_start_pax: f64,
    new_waiting_pax: f64,
    boarded_pax: f64,
    queue_overflow_dropped_pax: f64,
    queue_end_pax: f64,
    onboard_start_pax: f64,
    onboard_end_pax: f64,
    alighted_pax: f64,
    fare_recognized_pax: f64,
    fare_recognized_base: f64,
    missing_fare_basis_pax: f64,
) -> LifecycleConservationSummary {
    // This is deliberately aggregate-only and fast-kernel-only. It exists to
    // catch lifecycle regressions before/after optimisation, not to reconcile
    // strategic planner estimates or desktop projection counters.
    LifecycleConservationSummary {
        source_label: "engine_fast_lifecycle_conservation".to_string(),
        queue_start_pax: queue_start_pax.max(0.0),
        new_waiting_pax: new_waiting_pax.max(0.0),
        boarded_pax: boarded_pax.max(0.0),
        queue_overflow_dropped_pax: queue_overflow_dropped_pax.max(0.0),
        queue_end_pax: queue_end_pax.max(0.0),
        queue_balance_error: queue_start_pax.max(0.0) + new_waiting_pax.max(0.0)
            - boarded_pax.max(0.0)
            - queue_overflow_dropped_pax.max(0.0)
            - queue_end_pax.max(0.0),
        onboard_start_pax: onboard_start_pax.max(0.0),
        onboard_end_pax: onboard_end_pax.max(0.0),
        alighted_pax: alighted_pax.max(0.0),
        onboard_balance_error: onboard_start_pax.max(0.0) + boarded_pax.max(0.0)
            - alighted_pax.max(0.0)
            - onboard_end_pax.max(0.0),
        fare_recognized_pax: fare_recognized_pax.max(0.0),
        fare_recognized_base: fare_recognized_base.max(0.0),
        missing_fare_basis_pax: missing_fare_basis_pax.max(0.0),
    }
}

fn apply_queue_overflow_cap(
    queue_cohorts: &mut HashMap<ServiceStopDestKey, f64>,
    queue_board_index: &CohortServiceStopIndex,
    service_stop_key: &ServiceStopKey,
    queue_cap: f64,
) -> f64 {
    if !(queue_cap.is_finite() && queue_cap > 0.0) {
        return 0.0;
    }
    let total = indexed_cohort_total(queue_cohorts, queue_board_index, service_stop_key);
    if total <= queue_cap + FAST_KERNEL_QUEUE_EPS {
        return 0.0;
    }
    let overflow = total - queue_cap;
    if total <= FAST_KERNEL_QUEUE_EPS {
        return 0.0;
    }
    let scale = (queue_cap / total).clamp(0.0, 1.0);
    let keys = queue_board_index
        .get(service_stop_key)
        .cloned()
        .unwrap_or_default();
    for key in keys {
        let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
        let next = (queued * scale).max(0.0);
        if next > FAST_KERNEL_QUEUE_EPS {
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
    fare_policy_context: &EngineFarePolicyContext,
    dt_s: f64,
) -> Result<(SimulationOutput, SimState), String> {
    let call_index = FAST_KERNEL_CALL_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let total_started = Instant::now();
    if dt_s <= 0.0 || !dt_s.is_finite() {
        return Err("dt_s must be > 0".to_string());
    }
    let queue_before = state_in.queue_cohorts.len();
    let stage_prepare_started = Instant::now();
    let mut next = state_in.clone();
    next.t_s += dt_s;
    next.pending_od_trips.clear();
    next.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > FAST_KERNEL_QUEUE_EPS);
    next.onboard_cohorts
        .retain(|_, onboard| onboard.is_finite() && *onboard > FAST_KERNEL_QUEUE_EPS);
    let queue_start_pax = cohort_total(&next.queue_cohorts);
    let onboard_start_pax = cohort_total(&next.onboard_cohorts);
    // Step-local indexes are derived from the authoritative cohort maps. They
    // may contain keys that become empty during the step; every read still
    // checks the source map value, keeping conservation tied to cohort state
    // while avoiding repeated full-map scans by service/stop.
    let mut queue_board_index = build_queue_board_index(&next.queue_cohorts);
    let mut onboard_alight_index = build_onboard_alight_index(&next.onboard_cohorts);

    let mut board_keys = cache.board_templates.keys().cloned().collect::<Vec<_>>();
    board_keys.sort();
    let stage_prepare_ms = stage_prepare_started.elapsed().as_secs_f64() * 1000.0;

    let mut arrivals_by_board = HashMap::<ServiceStopKey, f64>::new();
    let mut attempted_by_cohort = HashMap::<ServiceStopDestKey, f64>::new();
    let mut boarded_by_cohort = HashMap::<ServiceStopDestKey, f64>::new();
    let mut alighted_by_cohort = HashMap::<ServiceStopDestKey, f64>::new();
    let mut board_loads = Vec::<BoardLoad>::new();
    let mut stop_flows_by_stop = BTreeMap::<String, StopFlow>::new();

    let mut total_attempted = 0.0_f64;
    let mut total_served = 0.0_f64;
    let mut total_denied = 0.0_f64;
    let mut total_overflow = 0.0_f64;
    let mut total_alighted = 0.0_f64;
    let mut total_fare_recognized_pax = 0.0_f64;
    let mut total_fare_recognized_base = 0.0_f64;
    let mut total_missing_fare_basis_pax = 0.0_f64;
    let stage_service_stop_loop_started = Instant::now();
    let service_stop_rows = board_keys.len();

    for (service_id, stop_id) in board_keys {
        let service_stop_key = (service_id.clone(), stop_id.clone());
        let Some(template) = cache.board_templates.get(&service_stop_key) else {
            continue;
        };
        let mut arrivals = 0.0_f64;
        if let Some(cohort_templates) = cache.cohort_templates_by_board.get(&service_stop_key) {
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
                push_cohort_index_key(
                    &mut queue_board_index,
                    service_stop_key.clone(),
                    key.clone(),
                );
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
                push_cohort_index_key(
                    &mut queue_board_index,
                    service_stop_key.clone(),
                    key.clone(),
                );
                *attempted_by_cohort.entry(key).or_insert(0.0) += attempted;
            }
        }
        arrivals_by_board.insert(service_stop_key.clone(), arrivals);

        let queue_start =
            indexed_cohort_total(&next.queue_cohorts, &queue_board_index, &service_stop_key);
        let mut time_to_next = next
            .time_to_next_departure_s
            .get(&service_stop_key)
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
                &mut next.onboard_cohorts,
                &queue_board_index,
                &mut onboard_alight_index,
                &service_stop_key,
                departure_capacity,
                &mut boarded_by_cohort,
            );
            served_total += boarded;
            if boarding_cap_remaining.is_finite() {
                boarding_cap_remaining = (boarding_cap_remaining - boarded).max(0.0);
            }
        }
        let time_to_next_departure_s_end = (time_to_next - remaining).max(0.0);
        next.time_to_next_departure_s
            .insert(service_stop_key.clone(), time_to_next_departure_s_end);

        let overflow_dropped = apply_queue_overflow_cap(
            &mut next.queue_cohorts,
            &queue_board_index,
            &service_stop_key,
            template.station_queue_capacity_pax,
        );
        let queue_end =
            indexed_cohort_total(&next.queue_cohorts, &queue_board_index, &service_stop_key);
        next.queue.insert(service_stop_key.clone(), queue_end);

        let served_from_arrivals = served_total.min(arrivals.max(0.0));
        let served_from_queue = (served_total - served_from_arrivals).max(0.0);
        let denied_boardings = (arrivals - served_from_arrivals).max(0.0);
        let alightings_served = if departures_observed > 0 {
            alight_from_onboard_cohorts(
                &mut next.onboard_cohorts,
                &onboard_alight_index,
                &service_stop_key,
                &mut alighted_by_cohort,
            )
        } else {
            0.0
        };
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
    let stage_service_stop_loop_ms =
        stage_service_stop_loop_started.elapsed().as_secs_f64() * 1000.0;

    let stage_finalize_started = Instant::now();
    next.queue
        .retain(|_, queued| queued.is_finite() && *queued > FAST_KERNEL_QUEUE_EPS);
    next.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > FAST_KERNEL_QUEUE_EPS);
    next.onboard_cohorts
        .retain(|_, onboard| onboard.is_finite() && *onboard > FAST_KERNEL_QUEUE_EPS);
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
        .chain(alighted_by_cohort.keys())
        .chain(next.queue_cohorts.keys())
        .chain(next.onboard_cohorts.keys())
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
        let alighted = alighted_by_cohort
            .get(&key)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let fare_delta_base =
            completed_cohort_fare_delta_base(fare_policy_context, &key.0, alighted);
        if alighted > FAST_KERNEL_QUEUE_EPS {
            if fare_delta_base > FAST_KERNEL_QUEUE_EPS {
                total_fare_recognized_pax += alighted;
            } else if fare_policy_context.fare_for_service(&key.0).is_none() {
                total_missing_fare_basis_pax += alighted;
            }
        }
        total_fare_recognized_base += fare_delta_base;
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
            alighted_pax: alighted,
            fare_delta_base,
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
    kpis.share_boardings_served = if total_attempted > FAST_KERNEL_QUEUE_EPS {
        (total_served / total_attempted).clamp(0.0, 1.0)
    } else {
        1.0
    };
    kpis.total_overflow_dropped = total_overflow.max(0.0);
    kpis.share_demand_overflow_dropped = if total_attempted > FAST_KERNEL_QUEUE_EPS {
        (total_overflow / total_attempted).clamp(0.0, 1.0)
    } else {
        0.0
    };
    kpis.total_trips_attempted = total_attempted.max(0.0);
    kpis.total_trips_served = total_served.max(0.0);
    kpis.share_trips_served = if total_attempted > FAST_KERNEL_QUEUE_EPS {
        (total_served / total_attempted).clamp(0.0, 1.0)
    } else {
        1.0
    };
    kpis.total_trips = total_served.max(0.0);
    kpis.total_fare_revenue_base = total_fare_recognized_base.max(0.0);
    out.kpis = kpis;
    out.fare_flow = FareFlowSummary {
        liability_accrued_base: total_fare_recognized_base.max(0.0),
        liability_accrued_pax: total_served.max(0.0),
        completed_journeys_pax: total_alighted.max(0.0),
        recognized_revenue_base: total_fare_recognized_base.max(0.0),
    };
    let queue_end_pax = cohort_total(&next.queue_cohorts);
    let onboard_end_pax = cohort_total(&next.onboard_cohorts);
    out.lifecycle_conservation = build_lifecycle_conservation_summary(
        queue_start_pax,
        total_attempted,
        total_served,
        total_overflow,
        queue_end_pax,
        onboard_start_pax,
        onboard_end_pax,
        total_alighted,
        total_fare_recognized_pax,
        total_fare_recognized_base,
        total_missing_fare_basis_pax,
    );
    let stage_finalize_ms = stage_finalize_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
    let cohort_template_rows = cache
        .cohort_templates_by_board
        .values()
        .map(|rows| rows.len())
        .sum::<usize>();
    let should_log = call_index.is_multiple_of(RT_FAST_KERNEL_LOG_INTERVAL)
        || total_ms > RT_FAST_KERNEL_SLOW_MS
        || stage_service_stop_loop_ms > RT_FAST_KERNEL_SLOW_MS * 0.5;
    if should_log {
        eprintln!(
            "[rt-kernel] fast_step call={} total_ms={:.2} prepare_ms={:.2} service_stop_loop_ms={:.2} finalize_ms={:.2} dt_s={:.3} board_templates={} cohort_template_rows={} service_stop_rows={} board_load_rows={} passenger_cohort_rows={} queue_cohorts={}=>{} queue_scalar={}=>{} attempted={:.6} served={:.6} denied={:.6} overflow={:.6} alighted={:.6}",
            call_index,
            total_ms.max(0.0),
            stage_prepare_ms.max(0.0),
            stage_service_stop_loop_ms.max(0.0),
            stage_finalize_ms.max(0.0),
            dt_s.max(0.0),
            cache.board_templates.len(),
            cohort_template_rows,
            service_stop_rows,
            out.board_loads.len(),
            out.passenger_cohorts.len(),
            queue_before,
            next.queue_cohorts.len(),
            state_in.queue.len(),
            next.queue.len(),
            total_attempted.max(0.0),
            total_served.max(0.0),
            total_denied.max(0.0),
            total_overflow.max(0.0),
            total_alighted.max(0.0),
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boarded_queue_cohorts_become_onboard_aggregates() {
        let mut queue_cohorts = HashMap::<ServiceStopDestKey, f64>::from([(
            (
                "svc:a".to_string(),
                "stop:a".to_string(),
                "stop:b".to_string(),
            ),
            10.0,
        )]);
        let mut onboard_cohorts = HashMap::<ServiceStopDestKey, f64>::new();
        let mut boarded_by_key = HashMap::<ServiceStopDestKey, f64>::new();
        let queue_board_index = build_queue_board_index(&queue_cohorts);
        let mut onboard_alight_index = build_onboard_alight_index(&onboard_cohorts);
        let service_stop_key = ("svc:a".to_string(), "stop:a".to_string());

        let boarded = board_from_queue_cohorts(
            &mut queue_cohorts,
            &mut onboard_cohorts,
            &queue_board_index,
            &mut onboard_alight_index,
            &service_stop_key,
            6.0,
            &mut boarded_by_key,
        );

        let key = (
            "svc:a".to_string(),
            "stop:a".to_string(),
            "stop:b".to_string(),
        );
        assert_eq!(boarded, 6.0);
        assert_eq!(queue_cohorts.get(&key).copied().unwrap_or(0.0), 4.0);
        assert_eq!(onboard_cohorts.get(&key).copied().unwrap_or(0.0), 6.0);
        assert_eq!(boarded_by_key.get(&key).copied().unwrap_or(0.0), 6.0);
        assert_eq!(
            indexed_cohort_total(
                &onboard_cohorts,
                &onboard_alight_index,
                &("svc:a".to_string(), "stop:b".to_string())
            ),
            6.0
        );
    }

    #[test]
    fn indexed_queue_total_matches_source_scan() {
        let queue_cohorts = HashMap::<ServiceStopDestKey, f64>::from([
            (
                (
                    "svc:a".to_string(),
                    "stop:a".to_string(),
                    "stop:b".to_string(),
                ),
                4.0,
            ),
            (
                (
                    "svc:a".to_string(),
                    "stop:a".to_string(),
                    "stop:c".to_string(),
                ),
                3.0,
            ),
            (
                (
                    "svc:b".to_string(),
                    "stop:a".to_string(),
                    "stop:b".to_string(),
                ),
                9.0,
            ),
        ]);
        let queue_board_index = build_queue_board_index(&queue_cohorts);
        let service_stop_key = ("svc:a".to_string(), "stop:a".to_string());

        assert_eq!(
            indexed_cohort_total(&queue_cohorts, &queue_board_index, &service_stop_key),
            queue_total_for_service_stop_scan(&queue_cohorts, "svc:a", "stop:a")
        );
    }

    #[test]
    fn alighting_drains_only_matching_onboard_destination() {
        let key_match = (
            "svc:a".to_string(),
            "stop:a".to_string(),
            "stop:b".to_string(),
        );
        let key_other_dest = (
            "svc:a".to_string(),
            "stop:a".to_string(),
            "stop:c".to_string(),
        );
        let key_other_service = (
            "svc:b".to_string(),
            "stop:a".to_string(),
            "stop:b".to_string(),
        );
        let mut onboard_cohorts = HashMap::<ServiceStopDestKey, f64>::from([
            (key_match.clone(), 6.0),
            (key_other_dest.clone(), 3.0),
            (key_other_service.clone(), 4.0),
        ]);
        let mut alighted_by_key = HashMap::<ServiceStopDestKey, f64>::new();
        let onboard_alight_index = build_onboard_alight_index(&onboard_cohorts);
        let service_stop_key = ("svc:a".to_string(), "stop:b".to_string());

        let alighted = alight_from_onboard_cohorts(
            &mut onboard_cohorts,
            &onboard_alight_index,
            &service_stop_key,
            &mut alighted_by_key,
        );

        assert_eq!(alighted, 6.0);
        assert!(!onboard_cohorts.contains_key(&key_match));
        assert_eq!(
            onboard_cohorts.get(&key_other_dest).copied().unwrap_or(0.0),
            3.0
        );
        assert_eq!(
            onboard_cohorts
                .get(&key_other_service)
                .copied()
                .unwrap_or(0.0),
            4.0
        );
        assert_eq!(alighted_by_key.get(&key_match).copied().unwrap_or(0.0), 6.0);
    }

    #[test]
    fn alighting_cannot_exceed_onboard_passengers() {
        let key = (
            "svc:a".to_string(),
            "stop:a".to_string(),
            "stop:b".to_string(),
        );
        let mut onboard_cohorts = HashMap::<ServiceStopDestKey, f64>::from([(key.clone(), 2.5)]);
        let mut alighted_by_key = HashMap::<ServiceStopDestKey, f64>::new();
        let onboard_alight_index = build_onboard_alight_index(&onboard_cohorts);
        let service_stop_key = ("svc:a".to_string(), "stop:b".to_string());

        let first = alight_from_onboard_cohorts(
            &mut onboard_cohorts,
            &onboard_alight_index,
            &service_stop_key,
            &mut alighted_by_key,
        );
        let second = alight_from_onboard_cohorts(
            &mut onboard_cohorts,
            &onboard_alight_index,
            &service_stop_key,
            &mut alighted_by_key,
        );

        assert_eq!(first, 2.5);
        assert_eq!(second, 0.0);
        assert!(onboard_cohorts.is_empty());
        assert_eq!(alighted_by_key.get(&key).copied().unwrap_or(0.0), 2.5);
    }

    #[test]
    fn completed_cohort_fare_uses_engine_run_config_context() {
        let fare_context = EngineFarePolicyContext::from_service_fares(
            true,
            "test_engine_fares",
            HashMap::from([("svc:a".to_string(), 2.5)]),
        );

        assert_eq!(
            completed_cohort_fare_delta_base(&fare_context, "svc:a", 4.0),
            10.0
        );
        assert_eq!(
            completed_cohort_fare_delta_base(&fare_context, "svc:missing", 4.0),
            0.0
        );
        assert_eq!(
            completed_cohort_fare_delta_base(&EngineFarePolicyContext::default(), "svc:a", 4.0),
            0.0
        );
    }

    #[test]
    fn lifecycle_conservation_summary_balances_queue_and_onboard_mass() {
        let summary = build_lifecycle_conservation_summary(
            3.0, // queue start
            7.0, // newly waiting in this step
            6.0, // boarded
            1.0, // overflow dropped
            3.0, // queue end
            2.0, // onboard start
            5.0, // onboard end
            3.0, // alighted
            3.0, // priced pax
            7.5, // recognized fare
            0.0, // missing fare basis
        );

        assert_eq!(summary.source_label, "engine_fast_lifecycle_conservation");
        assert_eq!(summary.queue_balance_error, 0.0);
        assert_eq!(summary.onboard_balance_error, 0.0);
        assert_eq!(summary.fare_recognized_pax, 3.0);
        assert_eq!(summary.fare_recognized_base, 7.5);
    }

    #[test]
    fn lifecycle_conservation_summary_reports_missing_fare_basis_without_authoritative_fare() {
        let summary = build_lifecycle_conservation_summary(
            0.0, 4.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 4.0,
        );

        assert_eq!(summary.queue_balance_error, 0.0);
        assert_eq!(summary.onboard_balance_error, 0.0);
        assert_eq!(summary.fare_recognized_pax, 0.0);
        assert_eq!(summary.fare_recognized_base, 0.0);
        assert_eq!(summary.missing_fare_basis_pax, 4.0);
    }
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
