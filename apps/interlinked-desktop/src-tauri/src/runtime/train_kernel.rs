use crate::*;

use super::models::{
    RuntimeFareEvents, RuntimeServiceProfile, RuntimeTrainPhase, RuntimeTrainState,
};

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
