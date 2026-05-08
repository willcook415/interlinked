use crate::*;

use super::models::{
    RuntimeFareEvents, RuntimeServiceProfile, RuntimeTrainPhase, RuntimeTrainState,
};

const RUNTIME_BOARDING_EPS: f64 = 1e-12;

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

#[cfg(test)]
fn runtime_queue_total_for_service_stop_scan(
    queue_cohorts: &HashMap<(String, String, String), f64>,
    service_id: &str,
    stop_id: &str,
) -> f64 {
    queue_cohorts
        .iter()
        .filter_map(|((service, stop, _destination), queued)| {
            if service == service_id && stop == stop_id && queued.is_finite() && *queued > 0.0 {
                Some(*queued)
            } else {
                None
            }
        })
        .sum::<f64>()
        .max(0.0)
}

fn runtime_queue_total_for_destinations(
    queue_cohorts: &HashMap<(String, String, String), f64>,
    profile: &RuntimeServiceProfile,
    stop_id: &str,
    destination_indices: &[usize],
) -> f64 {
    destination_indices
        .iter()
        .filter_map(|idx| {
            let destination = profile.stop_ids.get(*idx)?;
            let queued = queue_cohorts
                .get(&(
                    profile.service_id.clone(),
                    stop_id.to_string(),
                    destination.clone(),
                ))
                .copied()
                .unwrap_or(0.0);
            if queued.is_finite() && queued > 0.0 {
                Some(queued)
            } else {
                None
            }
        })
        .sum::<f64>()
        .max(0.0)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeBoardingEvent {
    pub(crate) service_id: String,
    pub(crate) stop_id: String,
    pub(crate) attempted_pax: f64,
    pub(crate) boarded_pax: f64,
    pub(crate) left_behind_pax: f64,
    pub(crate) queue_total_before_pax: f64,
    pub(crate) queue_total_after_pax: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeDepartureBoardingOutcome {
    pub(crate) fare: RuntimeFareEvents,
    pub(crate) debug: RuntimeBoardingEvent,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeTrainAdvanceDelta {
    pub(crate) fare: RuntimeFareEvents,
    pub(crate) boarding_events: Vec<RuntimeBoardingEvent>,
}

pub(crate) fn apply_runtime_departure_boarding(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    stop_id: &str,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeDepartureBoardingOutcome {
    let mut fare = RuntimeFareEvents::default();
    let mut onboard_total = runtime_train_onboard_total(train);
    if let Some(alight_here) = train.onboard_cohorts.remove(stop_id) {
        if alight_here > 0.0 {
            onboard_total = (onboard_total - alight_here).max(0.0);
            fare.completed_alightings_pax += alight_here;
        }
    }

    let capacity = profile.vehicle_capacity.max(0.0);
    let mut residual_capacity = (capacity - onboard_total).max(0.0);
    let stop_index = train
        .current_stop_index
        .min(profile.stop_ids.len().saturating_sub(1));
    let destination_indices = if train.direction_step >= 0 {
        ((stop_index + 1)..profile.stop_ids.len()).collect::<Vec<_>>()
    } else {
        (0..stop_index).rev().collect::<Vec<_>>()
    };
    // Projection-only queue totals use the known boardable destinations for
    // this train instead of scanning every runtime queue cohort. These debug
    // counters are display/fare-fallback adjacent and are not authoritative
    // lifecycle accounting.
    let queue_total_before_pax =
        runtime_queue_total_for_destinations(queue_cohorts, profile, stop_id, &destination_indices);
    let mut attempted_pax = 0.0_f64;
    for idx in &destination_indices {
        let destination = &profile.stop_ids[*idx];
        let key = (
            profile.service_id.clone(),
            stop_id.to_string(),
            destination.clone(),
        );
        let queued = queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
        if queued <= 0.0 {
            continue;
        }
        attempted_pax += queued;
        if residual_capacity <= RUNTIME_BOARDING_EPS {
            continue;
        }
        let boarded = residual_capacity.min(queued);
        if boarded <= 0.0 {
            continue;
        }
        let remaining = (queued - boarded).max(0.0);
        if remaining > RUNTIME_BOARDING_EPS {
            queue_cohorts.insert(key, remaining);
        } else {
            queue_cohorts.remove(&key);
        }
        *train
            .onboard_cohorts
            .entry(destination.clone())
            .or_insert(0.0) += boarded;
        fare.boarded_pax += boarded;
        fare.liability_accrued_base += boarded * fare_base_per_boarding.max(0.0);
        residual_capacity = (residual_capacity - boarded).max(0.0);
    }
    let mut left_behind_pax = 0.0_f64;
    for idx in &destination_indices {
        let destination = &profile.stop_ids[*idx];
        let key = (
            profile.service_id.clone(),
            stop_id.to_string(),
            destination.clone(),
        );
        left_behind_pax += queue_cohorts.get(&key).copied().unwrap_or(0.0).max(0.0);
    }
    train.onboard_pax = runtime_train_onboard_total(train);
    let queue_total_after_pax = left_behind_pax;

    RuntimeDepartureBoardingOutcome {
        fare,
        debug: RuntimeBoardingEvent {
            service_id: profile.service_id.clone(),
            stop_id: stop_id.to_string(),
            attempted_pax: attempted_pax.max(0.0),
            boarded_pax: fare.boarded_pax.max(0.0),
            left_behind_pax: left_behind_pax.max(0.0),
            queue_total_before_pax: queue_total_before_pax.max(0.0),
            queue_total_after_pax: queue_total_after_pax.max(0.0),
        },
    }
}

pub(crate) fn advance_runtime_train(
    train: &mut RuntimeTrainState,
    profile: &RuntimeServiceProfile,
    dt_s: f64,
    queue_cohorts: &mut HashMap<(String, String, String), f64>,
    fare_base_per_boarding: f64,
) -> RuntimeTrainAdvanceDelta {
    let mut delta = RuntimeTrainAdvanceDelta::default();
    if profile.stop_ids.len() < 2 {
        train.phase = RuntimeTrainPhase::Dwell;
        train.current_stop_index = 0;
        train.progress = 0.0;
        train.remaining_s = profile.dwell_s;
        train.onboard_pax = 0.0;
        train.onboard_cohorts.clear();
        return delta;
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
                    let boarding = apply_runtime_departure_boarding(
                        train,
                        profile,
                        &stop_id,
                        queue_cohorts,
                        fare_base_per_boarding,
                    );
                    delta.fare.boarded_pax += boarding.fare.boarded_pax;
                    delta.fare.completed_alightings_pax += boarding.fare.completed_alightings_pax;
                    delta.fare.liability_accrued_base += boarding.fare.liability_accrued_base;
                    delta.boarding_events.push(boarding.debug);
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
    delta
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> RuntimeServiceProfile {
        RuntimeServiceProfile {
            service_id: "svc:a".to_string(),
            line_id: "line:a".to_string(),
            line_name: "Line A".to_string(),
            mode: "metro".to_string(),
            mode_variant: None,
            stock_tier_id: None,
            dwell_s: 15.0,
            turnaround_s: 30.0,
            speed_mps: 12.0,
            vehicle_capacity: 100.0,
            vehicles_on_service: 1,
            stop_ids: vec![
                "stop:a".to_string(),
                "stop:b".to_string(),
                "stop:c".to_string(),
            ],
            stop_xy: vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)],
            segment_lengths_m: vec![1000.0, 1000.0],
        }
    }

    #[test]
    fn projection_queue_destination_total_matches_service_stop_scan_for_valid_legs() {
        let profile = test_profile();
        let queue_cohorts = HashMap::<(String, String, String), f64>::from([
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
        let destination_indices = vec![1, 2];

        assert_eq!(
            runtime_queue_total_for_destinations(
                &queue_cohorts,
                &profile,
                "stop:a",
                &destination_indices
            ),
            runtime_queue_total_for_service_stop_scan(&queue_cohorts, "svc:a", "stop:a")
        );
    }
}
