use crate::*;

pub(crate) struct RuntimeLoopHandle {
    pub(crate) project_path: String,
    pub(crate) tx: Sender<RuntimeAction>,
    pub(crate) pending_actions: Arc<AtomicUsize>,
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) speed: Arc<AtomicU32>,
    pub(crate) clock_revision: Arc<AtomicU64>,
    pub(crate) join: Option<thread::JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeMaterializationState {
    pub(crate) project_path: String,
    pub(crate) topology_hash: u64,
    pub(crate) scope_hash: u64,
    pub(crate) fare_hash: u64,
    pub(crate) minute_of_day: u32,
    pub(crate) last_materialized_tick: u64,
    pub(crate) adaptive_max_active_zones: usize,
    pub(crate) last_tick_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeOpsState {
    pub(crate) project_path: String,
    pub(crate) topology_hash: u64,
    pub(crate) profiles_by_service: HashMap<String, RuntimeServiceProfile>,
    pub(crate) stop_name_by_id: HashMap<String, String>,
    pub(crate) reverse_service_by_service: HashMap<String, String>,
    pub(crate) stop_ids_by_service: HashMap<String, HashSet<String>>,
    pub(crate) fare_base_by_service: HashMap<String, f64>,
    pub(crate) dispatch_service_ids: HashSet<String>,
    pub(crate) trains: BTreeMap<String, RuntimeTrainState>,
    pub(crate) queue_cohorts: HashMap<(String, String, String), f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeTrainPhase {
    Dwell,
    Moving,
    Layover,
}

pub(crate) fn default_runtime_train_phase() -> RuntimeTrainPhase {
    RuntimeTrainPhase::Dwell
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeTrainState {
    pub(crate) train_id: String,
    pub(crate) service_id: String,
    pub(crate) line_id: String,
    pub(crate) line_name: String,
    pub(crate) mode: String,
    pub(crate) mode_variant: Option<String>,
    pub(crate) stock_tier_id: Option<String>,
    pub(crate) vehicle_capacity: f64,
    pub(crate) current_stop_index: usize,
    pub(crate) direction_step: i8,
    #[serde(default = "default_runtime_train_phase")]
    pub(crate) phase: RuntimeTrainPhase,
    pub(crate) progress: f64,
    pub(crate) remaining_s: f64,
    pub(crate) onboard_pax: f64,
    pub(crate) onboard_cohorts: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeFareEvents {
    pub(crate) boarded_pax: f64,
    pub(crate) completed_alightings_pax: f64,
    pub(crate) liability_accrued_base: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeServiceProfile {
    pub(crate) service_id: String,
    pub(crate) line_id: String,
    pub(crate) line_name: String,
    pub(crate) mode: String,
    pub(crate) mode_variant: Option<String>,
    pub(crate) stock_tier_id: Option<String>,
    pub(crate) dwell_s: f64,
    pub(crate) turnaround_s: f64,
    pub(crate) speed_mps: f64,
    pub(crate) vehicle_capacity: f64,
    pub(crate) vehicles_on_service: usize,
    pub(crate) stop_ids: Vec<String>,
    pub(crate) stop_xy: Vec<(f64, f64)>,
    pub(crate) segment_lengths_m: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeAction {
    Stop,
    SetRunning(bool),
    SetSpeed(u32),
    InvalidateMaterialization,
    ForceCheckpoint,
    AdvanceOnce { recompute_quick_kpis: bool },
}

pub(crate) struct RuntimeTick {
    pub(crate) project_path: String,
    pub(crate) last_step: Instant,
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

pub(crate) fn runtime_service_units_assigned(service: &Service) -> usize {
    service
        .stock_units_assigned
        .or_else(|| {
            service
                .rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.units_owned)
        })
        .or(service.stock_units_owned)
        .unwrap_or(0) as usize
}

pub(crate) fn runtime_service_enabled(service: &Service) -> bool {
    if matches!(service.service_enabled, Some(false)) {
        return false;
    }
    if !service.headway_s.is_finite() || service.headway_s <= 0.0 || service.headway_s >= 86_399.0 {
        return false;
    }
    if let Some(tph) = service.operating_tph {
        if !tph.is_finite() || tph <= 0.0 {
            return false;
        }
    }
    runtime_service_units_assigned(service) > 0
}

pub(crate) fn runtime_stop_display_name(stop: &interlinked_engine::model::Stop) -> String {
    stop.name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| stop.id.clone())
}

pub(crate) fn runtime_service_line_name(service: &Service) -> String {
    service
        .name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| service_line_runtime_id(service))
}

pub(crate) fn build_runtime_service_profiles(
    scenario: &Scenario,
) -> (
    HashMap<String, RuntimeServiceProfile>,
    HashMap<String, String>,
) {
    #[derive(Debug, Clone)]
    struct RuntimeServiceDraft {
        service_id: String,
        line_id: String,
        line_name: String,
        mode: String,
        mode_variant: Option<String>,
        stock_tier_id: Option<String>,
        dwell_s: f64,
        turnaround_s: f64,
        speed_mps: f64,
        vehicle_capacity: f64,
        stop_ids: Vec<String>,
        stop_xy: Vec<(f64, f64)>,
        segment_lengths_m: Vec<f64>,
        unit_weight: f64,
    }

    let mut stop_xy_by_id = HashMap::<String, (f64, f64)>::new();
    let mut stop_name_by_id = HashMap::<String, String>::new();
    for stop in &scenario.world.stops {
        stop_xy_by_id.insert(stop.id.clone(), (stop.x, stop.y));
        stop_name_by_id.insert(stop.id.clone(), runtime_stop_display_name(stop));
    }
    let mut link_speed_by_pair = HashMap::<(String, String, String), f64>::new();
    for link in &scenario.world.links {
        let key = (
            link.from_stop.clone(),
            link.to_stop.clone(),
            normalized_mode_token(&link.mode),
        );
        link_speed_by_pair
            .entry(key)
            .or_insert(link.speed_mps.max(0.1));
    }

    let mut drafts = Vec::<RuntimeServiceDraft>::new();
    let mut line_units_cap = HashMap::<String, usize>::new();
    for service in &scenario.world.services {
        if !runtime_service_enabled(service) {
            continue;
        }
        let units_assigned = runtime_service_units_assigned(service);
        if units_assigned == 0 || service.stop_sequence.len() < 2 {
            continue;
        }
        let mut stop_ids = Vec::<String>::new();
        let mut stop_xy = Vec::<(f64, f64)>::new();
        for stop_id in &service.stop_sequence {
            if let Some((x, y)) = stop_xy_by_id.get(stop_id).copied() {
                stop_ids.push(stop_id.clone());
                stop_xy.push((x, y));
            }
        }
        if stop_ids.len() < 2 || stop_xy.len() < 2 {
            continue;
        }
        let mut segment_lengths_m = Vec::<f64>::new();
        let mut speed_sum = 0.0_f64;
        let mut speed_count = 0usize;
        for idx in 1..stop_xy.len() {
            let (from_x, from_y) = stop_xy[idx - 1];
            let (to_x, to_y) = stop_xy[idx];
            let dx = to_x - from_x;
            let dy = to_y - from_y;
            let segment_m = (dx * dx + dy * dy).sqrt().max(1.0);
            segment_lengths_m.push(segment_m);
            let speed_key = (
                stop_ids[idx - 1].clone(),
                stop_ids[idx].clone(),
                normalized_mode_token(&service.mode),
            );
            if let Some(speed) = link_speed_by_pair.get(&speed_key).copied() {
                speed_sum += speed.max(0.1);
                speed_count += 1;
            }
        }
        if segment_lengths_m.is_empty() {
            continue;
        }
        let speed_mps = if speed_count > 0 {
            (speed_sum / speed_count as f64).max(0.5)
        } else {
            12.0
        };
        let dwell_s = service.dwell_s.max(8.0);
        let turnaround_s = dwell_s.max(20.0);
        let vehicle_capacity = service.vehicle_capacity.max(0.0);
        let line_id = service_line_runtime_id(service);
        line_units_cap
            .entry(line_id.clone())
            .and_modify(|value| *value = (*value).max(units_assigned))
            .or_insert(units_assigned);
        let unit_weight = service
            .operating_tph
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or_else(|| (3600.0 / service.headway_s.max(1.0)).max(0.0))
            .max(0.1);
        drafts.push(RuntimeServiceDraft {
            service_id: service.id.clone(),
            line_id,
            line_name: runtime_service_line_name(service),
            mode: service.mode.clone(),
            mode_variant: service.mode_variant.clone(),
            stock_tier_id: service
                .rolling_stock_profile
                .as_ref()
                .and_then(|profile| profile.package_id.clone())
                .or_else(|| service.stock_tier_id.clone()),
            dwell_s,
            turnaround_s,
            speed_mps,
            vehicle_capacity,
            stop_ids,
            stop_xy,
            segment_lengths_m,
            unit_weight,
        });
    }

    let mut draft_indices_by_line = BTreeMap::<String, Vec<usize>>::new();
    for (idx, draft) in drafts.iter().enumerate() {
        draft_indices_by_line
            .entry(draft.line_id.clone())
            .or_default()
            .push(idx);
    }

    let mut vehicles_by_service = HashMap::<String, usize>::new();
    for (line_id, indices) in draft_indices_by_line {
        if indices.is_empty() {
            continue;
        }
        let total_units = line_units_cap
            .get(&line_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, 64);
        if total_units == 0 {
            continue;
        }
        let mut allocations = vec![0usize; indices.len()];
        let weight_sum: f64 = indices
            .iter()
            .map(|idx| drafts[*idx].unit_weight.max(0.0))
            .sum::<f64>()
            .max(1e-9);
        let mut fractional = Vec::<(usize, f64, String)>::new();
        let mut assigned = 0usize;
        for (pos, idx) in indices.iter().enumerate() {
            let weight = drafts[*idx].unit_weight.max(0.0);
            let raw = (total_units as f64) * (weight / weight_sum);
            let base = raw.floor().max(0.0) as usize;
            allocations[pos] = base;
            assigned = assigned.saturating_add(base);
            fractional.push((pos, raw - base as f64, drafts[*idx].service_id.clone()));
        }

        let mut remainder = total_units.saturating_sub(assigned);
        fractional.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });
        for (pos, _frac, _service_id) in &fractional {
            if remainder == 0 {
                break;
            }
            allocations[*pos] = allocations[*pos].saturating_add(1);
            remainder = remainder.saturating_sub(1);
        }

        if total_units >= indices.len() {
            let mut donor_order = (0..indices.len()).collect::<Vec<_>>();
            donor_order.sort_by(|a, b| {
                allocations[*b].cmp(&allocations[*a]).then_with(|| {
                    drafts[indices[*a]]
                        .service_id
                        .cmp(&drafts[indices[*b]].service_id)
                })
            });
            for pos in 0..indices.len() {
                if allocations[pos] > 0 {
                    continue;
                }
                if let Some(donor) = donor_order
                    .iter()
                    .copied()
                    .find(|idx| allocations[*idx] > 1)
                {
                    allocations[donor] = allocations[donor].saturating_sub(1);
                    allocations[pos] = 1;
                }
            }
        }

        for (pos, idx) in indices.iter().enumerate() {
            vehicles_by_service.insert(drafts[*idx].service_id.clone(), allocations[pos]);
        }
    }

    let mut out = HashMap::<String, RuntimeServiceProfile>::new();
    for draft in drafts {
        let vehicles_on_service = vehicles_by_service
            .get(&draft.service_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, 64);
        if vehicles_on_service == 0 {
            continue;
        }
        out.insert(
            draft.service_id.clone(),
            RuntimeServiceProfile {
                service_id: draft.service_id,
                line_id: draft.line_id,
                line_name: draft.line_name,
                mode: draft.mode,
                mode_variant: draft.mode_variant,
                stock_tier_id: draft.stock_tier_id,
                dwell_s: draft.dwell_s,
                turnaround_s: draft.turnaround_s,
                speed_mps: draft.speed_mps,
                vehicle_capacity: draft.vehicle_capacity,
                vehicles_on_service,
                stop_ids: draft.stop_ids,
                stop_xy: draft.stop_xy,
                segment_lengths_m: draft.segment_lengths_m,
            },
        );
    }
    (out, stop_name_by_id)
}

pub(crate) fn build_runtime_reverse_service_pairs(
    profiles_by_service: &HashMap<String, RuntimeServiceProfile>,
) -> HashMap<String, String> {
    let mut services_by_line = HashMap::<String, Vec<&RuntimeServiceProfile>>::new();
    for profile in profiles_by_service.values() {
        services_by_line
            .entry(profile.line_id.clone())
            .or_default()
            .push(profile);
    }

    for services in services_by_line.values_mut() {
        services.sort_by(|a, b| a.service_id.cmp(&b.service_id));
    }

    let mut reverse_by_service = HashMap::<String, String>::new();
    for services in services_by_line.values() {
        for profile in services {
            if profile.stop_ids.len() < 2 {
                continue;
            }
            let mut reverse_sequence = profile.stop_ids.clone();
            reverse_sequence.reverse();
            if reverse_sequence == profile.stop_ids {
                continue;
            }
            let reverse_service = services
                .iter()
                .filter(|candidate| candidate.service_id != profile.service_id)
                .find(|candidate| candidate.stop_ids == reverse_sequence)
                .map(|candidate| candidate.service_id.clone());
            if let Some(reverse_id) = reverse_service {
                reverse_by_service.insert(profile.service_id.clone(), reverse_id);
            }
        }
    }
    reverse_by_service
}

pub(crate) fn new_runtime_train_state(
    profile: &RuntimeServiceProfile,
    unit_index: usize,
) -> RuntimeTrainState {
    let segment_count = profile.segment_lengths_m.len().max(1);
    let base_segment = unit_index % segment_count;
    let progress = if profile.vehicles_on_service > 0 {
        (unit_index as f64 / profile.vehicles_on_service as f64).fract()
    } else {
        0.0
    };
    let mut state = RuntimeTrainState {
        train_id: format!(
            "train::{}::{}::{}",
            profile.line_id,
            profile.service_id,
            unit_index.saturating_add(1)
        ),
        service_id: profile.service_id.clone(),
        line_id: profile.line_id.clone(),
        line_name: profile.line_name.clone(),
        mode: profile.mode.clone(),
        mode_variant: profile.mode_variant.clone(),
        stock_tier_id: profile.stock_tier_id.clone(),
        vehicle_capacity: profile.vehicle_capacity.max(0.0),
        current_stop_index: base_segment.min(profile.stop_ids.len().saturating_sub(1)),
        direction_step: 1,
        phase: RuntimeTrainPhase::Moving,
        progress,
        remaining_s: 0.0,
        onboard_pax: 0.0,
        onboard_cohorts: HashMap::new(),
    };
    if profile.stop_ids.len() < 2 {
        state.phase = RuntimeTrainPhase::Dwell;
        state.progress = 0.0;
        state.remaining_s = profile.dwell_s;
        state.current_stop_index = 0;
    }
    state
}

pub(crate) fn runtime_next_stop_index(
    current_stop_index: usize,
    direction_step: i8,
    stop_count: usize,
) -> Option<usize> {
    if stop_count < 2 {
        return None;
    }
    if direction_step >= 0 {
        if current_stop_index + 1 < stop_count {
            Some(current_stop_index + 1)
        } else {
            None
        }
    } else if current_stop_index >= 1 {
        Some(current_stop_index - 1)
    } else {
        None
    }
}

pub(crate) fn runtime_train_onboard_total(train: &RuntimeTrainState) -> f64 {
    train
        .onboard_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>()
        .max(0.0)
}

pub(crate) fn apply_runtime_departure_boarding(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    stop_id: &str,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeFareEvents {
    let mut events = RuntimeFareEvents::default();
    let mut onboard_total = runtime_train_onboard_total(train);
    if let Some(alight_here) = train.onboard_cohorts.remove(stop_id) {
        if alight_here > 0.0 {
            onboard_total = (onboard_total - alight_here).max(0.0);
            events.completed_alightings_pax += alight_here;
        }
    }

    let capacity = profile.vehicle_capacity.max(0.0);
    let mut residual_capacity = (capacity - onboard_total).max(0.0);
    if residual_capacity > 0.0 {
        let stop_index = train
            .current_stop_index
            .min(profile.stop_ids.len().saturating_sub(1));
        if train.direction_step >= 0 {
            for idx in (stop_index + 1)..profile.stop_ids.len() {
                if residual_capacity <= 1e-6 {
                    break;
                }
                let destination = &profile.stop_ids[idx];
                let key = (
                    profile.service_id.clone(),
                    stop_id.to_string(),
                    destination.clone(),
                );
                let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
                if queued <= 0.0 {
                    continue;
                }
                let boarded = residual_capacity.min(queued);
                if boarded <= 0.0 {
                    continue;
                }
                let remaining = (queued - boarded).max(0.0);
                if remaining > 1e-6 {
                    queue_cohorts.insert(key, remaining);
                } else {
                    queue_cohorts.remove(&key);
                }
                *train
                    .onboard_cohorts
                    .entry(destination.clone())
                    .or_insert(0.0) += boarded;
                events.boarded_pax += boarded;
                events.liability_accrued_base += boarded * fare_base_per_boarding.max(0.0);
                residual_capacity = (residual_capacity - boarded).max(0.0);
            }
        } else {
            for idx in (0..stop_index).rev() {
                if residual_capacity <= 1e-6 {
                    break;
                }
                let destination = &profile.stop_ids[idx];
                let key = (
                    profile.service_id.clone(),
                    stop_id.to_string(),
                    destination.clone(),
                );
                let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
                if queued <= 0.0 {
                    continue;
                }
                let boarded = residual_capacity.min(queued);
                if boarded <= 0.0 {
                    continue;
                }
                let remaining = (queued - boarded).max(0.0);
                if remaining > 1e-6 {
                    queue_cohorts.insert(key, remaining);
                } else {
                    queue_cohorts.remove(&key);
                }
                *train
                    .onboard_cohorts
                    .entry(destination.clone())
                    .or_insert(0.0) += boarded;
                events.boarded_pax += boarded;
                events.liability_accrued_base += boarded * fare_base_per_boarding.max(0.0);
                residual_capacity = (residual_capacity - boarded).max(0.0);
            }
        }
    }
    train.onboard_pax = runtime_train_onboard_total(train);
    events
}

pub(crate) fn advance_runtime_train(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    dt_s: f64,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeFareEvents {
    let mut events = RuntimeFareEvents::default();
    if profile.stop_ids.len() < 2 {
        train.phase = RuntimeTrainPhase::Dwell;
        train.current_stop_index = 0;
        train.progress = 0.0;
        train.remaining_s = profile.dwell_s;
        train.onboard_pax = 0.0;
        train.onboard_cohorts.clear();
        return events;
    }
    if train.current_stop_index >= profile.stop_ids.len() {
        train.current_stop_index = profile.stop_ids.len() - 1;
    }
    let mut remaining_dt = dt_s.max(0.0);
    let mut hops = 0usize;
    while remaining_dt > 1e-6 && hops < 24 {
        hops += 1;
        match train.phase {
            RuntimeTrainPhase::Moving => {
                let Some(next_stop_index) = runtime_next_stop_index(
                    train.current_stop_index,
                    train.direction_step,
                    profile.stop_ids.len(),
                ) else {
                    train.phase = RuntimeTrainPhase::Layover;
                    train.progress = 0.0;
                    train.remaining_s = profile.turnaround_s;
                    train.direction_step *= -1;
                    continue;
                };
                let seg_idx = train.current_stop_index.min(next_stop_index);
                let travel_s = (profile.segment_lengths_m[seg_idx].max(1.0)
                    / profile.speed_mps.max(0.5))
                .max(0.1);
                let move_remaining = (1.0 - train.progress.clamp(0.0, 1.0)) * travel_s;
                if remaining_dt < move_remaining {
                    train.progress = (train.progress + (remaining_dt / travel_s)).clamp(0.0, 1.0);
                    remaining_dt = 0.0;
                } else {
                    remaining_dt -= move_remaining;
                    train.current_stop_index = next_stop_index;
                    train.progress = 0.0;
                    train.phase = RuntimeTrainPhase::Dwell;
                    train.remaining_s = profile.dwell_s;
                }
            }
            RuntimeTrainPhase::Dwell | RuntimeTrainPhase::Layover => {
                let phase_remaining = train.remaining_s.max(0.0);
                if remaining_dt < phase_remaining {
                    train.remaining_s -= remaining_dt;
                    remaining_dt = 0.0;
                } else {
                    remaining_dt -= phase_remaining;
                    train.remaining_s = 0.0;
                    let stop_id = profile.stop_ids[train.current_stop_index].clone();
                    let delta = apply_runtime_departure_boarding(
                        train,
                        profile,
                        &stop_id,
                        queue_cohorts,
                        fare_base_per_boarding,
                    );
                    events.boarded_pax += delta.boarded_pax;
                    events.completed_alightings_pax += delta.completed_alightings_pax;
                    events.liability_accrued_base += delta.liability_accrued_base;
                    if runtime_next_stop_index(
                        train.current_stop_index,
                        train.direction_step,
                        profile.stop_ids.len(),
                    )
                    .is_some()
                    {
                        train.phase = RuntimeTrainPhase::Moving;
                        train.progress = 0.0;
                    } else {
                        train.phase = RuntimeTrainPhase::Layover;
                        train.progress = 0.0;
                        train.direction_step *= -1;
                        train.remaining_s = profile.turnaround_s;
                    }
                }
            }
        }
    }
    events
}

pub(crate) fn runtime_train_position_xy(
    train: &RuntimeTrainState,
    profile: &RuntimeServiceProfile,
) -> (f64, f64, Option<String>, bool) {
    if profile.stop_xy.is_empty() || train.current_stop_index >= profile.stop_xy.len() {
        return (0.0, 0.0, None, false);
    }
    if train.phase == RuntimeTrainPhase::Moving {
        if let Some(next_stop_index) = runtime_next_stop_index(
            train.current_stop_index,
            train.direction_step,
            profile.stop_xy.len(),
        ) {
            let (from_x, from_y) = profile.stop_xy[train.current_stop_index];
            let (to_x, to_y) = profile.stop_xy[next_stop_index];
            let t = train.progress.clamp(0.0, 1.0);
            return (
                from_x + (to_x - from_x) * t,
                from_y + (to_y - from_y) * t,
                None,
                true,
            );
        }
    }
    let (x, y) = profile.stop_xy[train.current_stop_index];
    (
        x,
        y,
        Some(profile.stop_ids[train.current_stop_index].clone()),
        false,
    )
}

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

    let mut guard = state
        .runtime_ops
        .lock()
        .map_err(|_| "runtime_ops mutex poisoned".to_string())?;
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
    if ops.topology_hash != topology_hash || ops.profiles_by_service.is_empty() {
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
    ops.topology_hash = topology_hash;
    ops.queue_cohorts
        .retain(|(service_id, board_stop_id, destination_stop_id), queued| {
            ops.dispatch_service_ids.contains(service_id)
                && ops
                    .stop_ids_by_service
                    .get(service_id)
                    .map(|stops| {
                        stops.contains(board_stop_id) && stops.contains(destination_stop_id)
                    })
                    .unwrap_or(false)
                && queued.is_finite()
                && *queued > 1e-6
        });
    if let Some(sim_output) = output {
        let mut arrivals_by_key = HashMap::<(String, String, String), f64>::new();
        for cohort in &sim_output.passenger_cohorts {
            if !ops.dispatch_service_ids.contains(&cohort.service_id) {
                continue;
            }
            let arrivals = cohort.attempted_pax.max(0.0);
            if arrivals <= 0.0 {
                continue;
            }
            let key = (
                cohort.service_id.clone(),
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
            if next > 1e-6 {
                ops.queue_cohorts.insert(key, next);
            } else {
                ops.queue_cohorts.remove(&key);
            }
        }
    }
    ops.queue_cohorts
        .retain(|_, queued| queued.is_finite() && *queued > 1e-6);

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

    let queue_total_before_boarding = ops
        .queue_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>();
    let mut fare_events = RuntimeFareEvents::default();
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
            fare_events.boarded_pax += delta.boarded_pax;
            fare_events.completed_alightings_pax += delta.completed_alightings_pax;
            fare_events.liability_accrued_base += delta.liability_accrued_base;
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
    let queue_total_after_boarding = ops
        .queue_cohorts
        .values()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum::<f64>();

    if !emit_runtime_views {
        let mut provenance_warnings = Vec::<String>::new();
        if !ops.profiles_by_service.is_empty() {
            provenance_warnings.push(
                "derived_calibrated: runtime ops advanced without materializing views on this tick"
                    .to_string(),
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
            provenance: "derived_calibrated".to_string(),
        });
    }

    let mut queue_inside_by_stop = HashMap::<String, f64>::new();
    for ((_service_id, stop_id, _destination_stop_id), queued) in &ops.queue_cohorts {
        *queue_inside_by_stop.entry(stop_id.clone()).or_insert(0.0) += queued.max(0.0);
    }

    let mut station_ids = station_agg.keys().cloned().collect::<BTreeSet<_>>();
    station_ids.extend(queue_inside_by_stop.keys().cloned());

    let mut station_views = Vec::<StationRuntimeView>::new();
    for stop_id in station_ids {
        let agg = station_agg.get(&stop_id).cloned().unwrap_or_default();
        let queue_inside = queue_inside_by_stop.get(&stop_id).copied().unwrap_or(0.0);
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
            provenance: "derived_calibrated".to_string(),
        });
    }
    station_views.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut active_trains_by_line = HashMap::<String, u32>::new();
    for train in &train_views {
        *active_trains_by_line
            .entry(train.line_id.clone())
            .or_insert(0) += 1;
    }
    let mut queue_end_by_line = HashMap::<String, f64>::new();
    for ((service_id, _stop_id, _destination_stop_id), queued) in &ops.queue_cohorts {
        if let Some(profile) = ops.profiles_by_service.get(service_id) {
            *queue_end_by_line
                .entry(profile.line_id.clone())
                .or_insert(0.0) += queued.max(0.0);
        }
    }
    let mut line_ids = line_agg
        .keys()
        .cloned()
        .chain(active_trains_by_line.keys().cloned())
        .chain(queue_end_by_line.keys().cloned())
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
            queue_end_pax: queue_end_by_line
                .get(&line_id)
                .copied()
                .unwrap_or(0.0)
                .max(0.0),
            mean_wait_s: mean_wait_s.max(0.0),
            provenance: "derived_calibrated".to_string(),
        });
    }

    let mut provenance_warnings = Vec::<String>::new();
    if !ops.profiles_by_service.is_empty() {
        provenance_warnings.push(
            "derived_calibrated: train onboard and station flow are reconstructed from deterministic service-stop cohorts and board-load events"
                .to_string(),
        );
    }
    let queue_drop = (queue_total_before_boarding - queue_total_after_boarding).max(0.0);
    let boarding_mass_mismatch = (queue_drop - fare_events.boarded_pax.max(0.0)).abs();
    if boarding_mass_mismatch > 1e-3 {
        provenance_warnings.push(format!(
            "derived_calibrated: queue/boarding conservation mismatch detected ({boarding_mass_mismatch:.3} pax)"
        ));
    }
    if scenario.world.services.iter().any(|service| {
        let service_intent = !matches!(service.service_enabled, Some(false))
            && service.headway_s.is_finite()
            && service.headway_s > 0.0
            && service.headway_s < 86_399.0
            && service.operating_tph.unwrap_or(1.0) > 0.0;
        service_intent && runtime_service_units_assigned(service) == 0
    }) {
        provenance_warnings.push(
            "authored: service has zero assigned stock and is suppressed from dispatch".to_string(),
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

pub(crate) fn runtime_fare_base_per_boarding(policy: &FarePolicyManifest, mode: &str) -> f64 {
    if !policy.enabled {
        return 0.0;
    }
    match fare_mode_bucket_from_tokens(mode, None, 0.0) {
        FareModeBucket::Bus => policy.fare_mode_bus_base.max(0.0),
        FareModeBucket::Tram => policy.fare_mode_tram_base.max(0.0),
        FareModeBucket::Metro => policy.fare_mode_metro_base.max(0.0),
        FareModeBucket::Rail => policy.fare_mode_rail_base.max(0.0),
        FareModeBucket::Ferry => policy.fare_mode_ferry_base.max(0.0),
        FareModeBucket::Default => policy.fare_mode_default_base.max(0.0),
    }
}

pub(crate) fn fare_flow_for_economy(
    gs: &interlinked_engine::platform::GameState,
) -> (f64, f64, f64) {
    let Some(output) = gs.last_output.as_ref() else {
        return (0.0, 0.0, 0.0);
    };
    let liability_base = output.fare_flow.liability_accrued_base.max(0.0);
    let liability_pax = output.fare_flow.liability_accrued_pax.max(0.0);
    let completed_pax = output.fare_flow.completed_journeys_pax.max(0.0);
    if liability_base > 0.0 || liability_pax > 0.0 || completed_pax > 0.0 {
        return (liability_base, liability_pax, completed_pax);
    }

    // Backward-compatible fallback for older outputs without fare_flow population.
    let mut fallback_completed = 0.0_f64;
    for load in &output.board_loads {
        let alightings = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        if alightings <= 0.0 {
            continue;
        }
        if load.departures_observed > 0 {
            fallback_completed += alightings;
        }
    }
    (
        output.kpis.total_fare_revenue_base.max(0.0),
        output.kpis.total_boardings_served.max(0.0),
        fallback_completed.max(0.0),
    )
}

pub(crate) fn adaptive_runtime_zone_cap(
    manifest: &ProjectManifest,
    scenario: &Scenario,
    previous_cap: usize,
    previous_tick_ms: f64,
) -> usize {
    let focus_cap = manifest
        .simulation_scope
        .focus_max_active_zones
        .clamp(120, 6000);
    let floor_cap = manifest
        .simulation_scope
        .remote_max_active_zones
        .clamp(20, focus_cap);
    let base_cap = manifest
        .simulation_scope
        .max_active_zones
        .clamp(floor_cap, focus_cap);
    let stop_count = scenario.world.stops.len();
    let network_cap = if stop_count <= 25 {
        48
    } else if stop_count <= 80 {
        96
    } else if stop_count <= 200 {
        160
    } else if stop_count <= 500 {
        240
    } else if stop_count <= 1_200 {
        320
    } else {
        420
    };
    let mut cap = previous_cap
        .clamp(floor_cap, focus_cap)
        .min(base_cap)
        .min(network_cap);
    let target_ms = manifest.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0);
    if previous_tick_ms > 0.0 {
        if previous_tick_ms > target_ms * 1.35 {
            cap = ((cap as f64) * 0.82).round() as usize;
        } else if previous_tick_ms < target_ms * 0.70 {
            cap = ((cap as f64) * 1.08).round() as usize;
        }
    }
    cap.clamp(floor_cap, focus_cap)
}

pub(crate) fn ensure_runtime_materialized_scenario(
    state: &AppState,
    project_path: &str,
    gs: &mut interlinked_engine::platform::GameState,
    manifest: &ProjectManifest,
    minute_of_day: u32,
    tick_index: u64,
) -> Result<usize, String> {
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let active_scope_hash = scope_hash(manifest);
    let active_fare_hash = fare_hash(&manifest.economy.fare_policy);
    let existing = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?
        .clone();
    let mut materialization = existing.unwrap_or(RuntimeMaterializationState {
        project_path: project_path.to_string(),
        topology_hash: 0,
        scope_hash: 0,
        fare_hash: 0,
        minute_of_day,
        last_materialized_tick: 0,
        adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
        last_tick_ms: 0.0,
    });
    if materialization.project_path != project_path {
        materialization = RuntimeMaterializationState {
            project_path: project_path.to_string(),
            topology_hash: 0,
            scope_hash: 0,
            fare_hash: 0,
            minute_of_day,
            last_materialized_tick: 0,
            adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
            last_tick_ms: 0.0,
        };
    }
    let adaptive_cap = adaptive_runtime_zone_cap(
        manifest,
        gs.store.scenario(),
        materialization.adaptive_max_active_zones,
        materialization.last_tick_ms,
    );
    let remote_interval = manifest
        .simulation_scope
        .remote_update_interval_ticks
        .max(1) as u64;
    let cap_delta = adaptive_cap.abs_diff(materialization.adaptive_max_active_zones);
    let cap_rebalance_due = cap_delta >= 32
        && tick_index.saturating_sub(materialization.last_materialized_tick) >= remote_interval;
    let needs_materialization = topology_hash != materialization.topology_hash
        || active_scope_hash != materialization.scope_hash
        || active_fare_hash != materialization.fare_hash
        || minute_of_day != materialization.minute_of_day
        || cap_rebalance_due;
    if needs_materialization {
        let cfg = economy_config();
        let mut materialized = gs.store.scenario().clone();
        strip_auto_reverse_runtime_artifacts(&mut materialized);
        apply_game_runtime_demand_tuning(&mut materialized.params);
        apply_fare_policy_to_params(&mut materialized.params, &manifest.economy.fare_policy);
        synthesize_auto_reverse_runtime_services(&mut materialized);
        materialize_line_operations_for_minute(&mut materialized, &cfg, minute_of_day);
        apply_game_runtime_perf_budget(&mut materialized, adaptive_cap);
        gs.store = ScenarioStore::new(materialized);
        materialization.topology_hash = topology_hash;
        materialization.scope_hash = active_scope_hash;
        materialization.fare_hash = active_fare_hash;
        materialization.minute_of_day = minute_of_day;
        materialization.last_materialized_tick = tick_index;
        materialization.adaptive_max_active_zones = adaptive_cap;
    } else {
        materialization.adaptive_max_active_zones = adaptive_cap;
    }
    let mut guard = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
    *guard = Some(materialization);
    Ok(adaptive_cap)
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
    let use_runtime_economy = runtime.economy.economy_revision >= reloaded.economy.economy_revision;
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

pub(crate) fn runtime_has_due_purchase_orders(scenario: &Scenario, now_tick_s: f64) -> bool {
    if !now_tick_s.is_finite() || now_tick_s < 0.0 {
        return false;
    }
    scenario.world.services.iter().any(|service| {
        service
            .rolling_stock_profile
            .as_ref()
            .map(|profile| {
                profile.pending_orders.iter().any(|order| {
                    order
                        .eta_at_tick_s
                        .map(|eta| eta.is_finite() && eta <= now_tick_s + 1e-6)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

pub(crate) fn run_simulation_tick(
    state: &AppState,
    project_root: &Path,
    manifest: &mut ProjectManifest,
    dt_s: f64,
    fixed_step_s: f64,
    recompute_quick_kpis: bool,
    tick_index: u64,
    clock_revision: u64,
    queue_depth: usize,
    dropped_steps: u32,
    emit_runtime_views: bool,
    strategic_refresh_due_hint: bool,
) -> Result<RuntimeSnapshot, String> {
    let tick_start = Instant::now();
    let mut telemetry = RuntimePerfTelemetry {
        tick_index,
        dt_s,
        fixed_step_s,
        queue_depth,
        dropped_steps,
        ..RuntimePerfTelemetry::default()
    };
    let mut guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    let gs = guard
        .as_mut()
        .ok_or_else(|| "game not initialised for current session".to_string())?;
    let prepare_start = Instant::now();
    if let Some(step_kernel) = gs.run_cfg.step_kernel.as_mut() {
        step_kernel.k_paths = 1;
        step_kernel.msa_max_iters = 1;
        step_kernel.convergence_rel = 1.0;
        step_kernel.route_choice_theta = 0.002;
    }
    gs.run_cfg.enable_kernel_partitioning = manifest.runtime_scheduling.runtime_ops_kernel_v1;
    gs.run_cfg.strategic_refresh_interval_steps = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
    if runtime_has_due_purchase_orders(gs.store.scenario(), manifest.clock_state.tick_seconds) {
        let mut scenario_with_orders = gs.store.scenario().clone();
        let delivered_orders = settle_pending_purchase_orders(
            &mut scenario_with_orders,
            manifest.clock_state.tick_seconds,
        );
        if delivered_orders > 0 {
            gs.store = ScenarioStore::new(scenario_with_orders);
        }
    }
    let minute_of_day = clock_minute_of_day(&manifest.clock_state);
    let adaptive_cap = ensure_runtime_materialized_scenario(
        state,
        &project_root.to_string_lossy(),
        gs,
        manifest,
        minute_of_day,
        tick_index,
    )?;
    telemetry.adaptive_max_active_zones = adaptive_cap;
    telemetry.stage_prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.0;

    let scope = SimulationScope {
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        remote_regions_mode: manifest.simulation_scope.remote_regions_mode.clone(),
        max_active_zones: adaptive_cap,
    };
    let step_start = Instant::now();
    let req = interlinked_engine::platform::GameStepRequest {
        recompute_quick_kpis,
        edits: Vec::new(),
        force_strategic_refresh: false,
    };
    gs.run_cfg.lightweight_outputs = manifest.runtime_scheduling.lightweight_tick_outputs;
    let step_output = SimulationService::step_game_scoped(gs, dt_s, req, &scope)?;
    telemetry.stage_step_ms = step_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.engine_strategic_refresh_executed = step_output.strategic_refresh_executed;
    telemetry.engine_strategic_refresh_reason = step_output
        .strategic_refresh_reason
        .map(|reason| format!("{reason:?}"));
    telemetry.engine_fast_steps = step_output.kernel_perf.fast_steps;
    telemetry.engine_strategic_steps = step_output.kernel_perf.strategic_steps;
    telemetry.engine_fast_last_ms = step_output.kernel_perf.last_fast_ms;
    telemetry.engine_strategic_last_ms = step_output.kernel_perf.last_strategic_ms;
    telemetry.engine_fast_avg_ms = step_output.kernel_perf.avg_fast_ms();
    telemetry.engine_strategic_avg_ms = step_output.kernel_perf.avg_strategic_ms();
    telemetry.engine_steps_since_last_strategic =
        step_output.kernel_perf.steps_since_last_strategic;
    telemetry.engine_strategic_cache_hits = step_output.kernel_perf.strategic_cache_hits;
    telemetry.engine_strategic_cache_misses = step_output.kernel_perf.strategic_cache_misses;

    let econ_start = Instant::now();
    let cfg = economy_config();
    let frame = interlinked_engine::platform::history_last_frame(gs)
        .ok_or_else(|| "no history frame available after step".to_string())?;
    let service_opex_per_hour = interlinked_engine::platform::estimate_service_opex_per_hour_base(
        gs.store.scenario(),
        &cfg,
    );
    let staff_opex_per_hour =
        builder_support::estimate_staff_opex_per_hour_base(gs.store.scenario(), &cfg);
    manifest.clock_state.tick_seconds = gs.tick_s;
    let frame_lite = HistoryFrameLite {
        t_s: frame.t_s,
        kpis: frame.kpis.clone(),
        queue_summary: frame.queue_summary.clone(),
        service_loads: if step_output.strategic_refresh_executed {
            build_live_service_loads(gs)
        } else {
            Vec::new()
        },
    };
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let trains_authoritative = runtime_trains_authoritative_for_manifest(manifest);
    let ops_start = Instant::now();
    let (
        runtime_trains,
        runtime_stations,
        runtime_line_ops,
        provenance_warnings,
        runtime_fare_events,
    ) = if trains_authoritative {
        let (trains, stations, line_ops, warnings, fare_events) = build_runtime_ops_views(
            state,
            &project_root.to_string_lossy(),
            gs.store.scenario(),
            gs.last_output.as_ref(),
            &manifest.economy.fare_policy,
            dt_s,
            topology_hash,
            emit_runtime_views,
        )?;
        (trains, stations, line_ops, warnings, fare_events)
    } else {
        if let Ok(mut guard) = state.runtime_ops.lock() {
            *guard = None;
        }
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RuntimeFareEvents::default(),
        )
    };
    telemetry.stage_runtime_ops_ms = ops_start.elapsed().as_secs_f64() * 1000.0;
    let fare_recognition_enabled = runtime_fare_recognition_enabled_for_manifest(manifest);
    let (accrued_fare_revenue_base, accrued_boardings_pax, mut completed_alightings_pax) =
        if fare_recognition_enabled {
            if manifest.session_kind == SessionKind::Game && trains_authoritative {
                let boarded = runtime_fare_events.boarded_pax.max(0.0);
                let completed = runtime_fare_events.completed_alightings_pax.max(0.0);
                let liability = runtime_fare_events.liability_accrued_base.max(0.0);
                (liability, boarded, completed)
            } else {
                fare_flow_for_economy(gs)
            }
        } else {
            let served = frame_lite.kpis.total_boardings_served.max(0.0);
            (
                frame_lite.kpis.total_fare_revenue_base.max(0.0),
                served,
                served,
            )
        };
    completed_alightings_pax = completed_alightings_pax.max(0.0);
    let (delta_revenue_base, delta_opex_base, delta_net_base) = apply_economy_realism_tick(
        manifest,
        &frame_lite,
        accrued_fare_revenue_base,
        accrued_boardings_pax,
        completed_alightings_pax,
        service_opex_per_hour,
        staff_opex_per_hour,
        dt_s,
    );
    if let Some(metrics) = manifest.progress_metrics.as_mut() {
        metrics.ridership += frame.kpis.total_trips_served.max(0.0);
    }
    sync_progress_budget_from_economy(manifest);
    let economy = SimulationAdvanceEconomy {
        current_balance_base: manifest.economy.current_balance_base,
        cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
        cumulative_opex_base: manifest.economy.cumulative_opex_base,
        budget_display: manifest
            .progress_metrics
            .as_ref()
            .map(|m| m.budget)
            .unwrap_or(manifest.economy.current_balance_base),
    };
    telemetry.stage_economy_ms = econ_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.strategic_refresh_due = strategic_refresh_due_hint;
    telemetry.strategic_refresh_interval_ticks = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
    telemetry.runtime_views_materialized = emit_runtime_views && trains_authoritative;
    telemetry.tick_total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
    {
        let mut materialization = state
            .runtime_materialization
            .lock()
            .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
        if let Some(current) = materialization.as_mut() {
            if current.project_path == project_root.to_string_lossy() {
                current.last_tick_ms = telemetry.tick_total_ms;
            }
        }
    }
    Ok(RuntimeSnapshot {
        project_path: project_root.to_string_lossy().to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        economy,
        frame: Some(frame_lite),
        delta_revenue_base,
        delta_opex_base,
        delta_net_base,
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry,
        trains: runtime_trains,
        stations: runtime_stations,
        line_ops: runtime_line_ops,
        provenance_warnings,
        trains_authoritative,
    })
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

pub(crate) fn default_runtime_enabled() -> bool {
    true
}

pub(crate) fn default_runtime_fixed_step_s() -> f64 {
    0.5
}

pub(crate) fn default_runtime_max_steps_per_cycle() -> u32 {
    12
}

pub(crate) fn default_runtime_checkpoint_interval_ticks() -> u32 {
    20
}

pub(crate) fn default_runtime_snapshot_ring() -> usize {
    32
}

pub(crate) fn default_runtime_target_tick_ms() -> f64 {
    16.0
}

pub(crate) fn default_runtime_strategic_refresh_interval_ticks() -> u32 {
    8
}

pub(crate) fn default_runtime_lightweight_tick_outputs() -> bool {
    true
}

pub(crate) fn default_runtime_ops_kernel_v1() -> bool {
    true
}

pub(crate) fn default_ui_runtime_trains_v1() -> bool {
    true
}

pub(crate) fn default_fare_recognition_v1() -> bool {
    true
}

pub(crate) fn default_runtime_scheduling_manifest() -> RuntimeSchedulingManifest {
    RuntimeSchedulingManifest {
        enabled: default_runtime_enabled(),
        fixed_step_s: default_runtime_fixed_step_s(),
        max_steps_per_cycle: default_runtime_max_steps_per_cycle(),
        checkpoint_interval_ticks: default_runtime_checkpoint_interval_ticks(),
        snapshot_ring: default_runtime_snapshot_ring(),
        target_tick_ms: default_runtime_target_tick_ms(),
        strategic_refresh_interval_ticks: default_runtime_strategic_refresh_interval_ticks(),
        lightweight_tick_outputs: default_runtime_lightweight_tick_outputs(),
        runtime_ops_kernel_v1: default_runtime_ops_kernel_v1(),
        ui_runtime_trains_v1: default_ui_runtime_trains_v1(),
        fare_recognition_v1: default_fare_recognition_v1(),
    }
}

pub(crate) fn runtime_trains_authoritative_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.runtime_ops_kernel_v1
        && manifest.runtime_scheduling.ui_runtime_trains_v1
}

pub(crate) fn runtime_fare_recognition_enabled_for_manifest(manifest: &ProjectManifest) -> bool {
    if manifest.session_kind == SessionKind::Game {
        return true;
    }
    manifest.runtime_scheduling.fare_recognition_v1
}

pub(crate) fn enforce_game_runtime_hardcut(manifest: &mut ProjectManifest) {
    if manifest.session_kind != SessionKind::Game {
        return;
    }
    manifest.runtime_scheduling.enabled = true;
    manifest.runtime_scheduling.lightweight_tick_outputs = true;
    manifest.runtime_scheduling.runtime_ops_kernel_v1 = true;
    manifest.runtime_scheduling.ui_runtime_trains_v1 = true;
    manifest.runtime_scheduling.fare_recognition_v1 = true;
}
