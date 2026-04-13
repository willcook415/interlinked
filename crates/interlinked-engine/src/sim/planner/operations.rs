use super::*;

pub(super) fn build_operations_outputs(
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
