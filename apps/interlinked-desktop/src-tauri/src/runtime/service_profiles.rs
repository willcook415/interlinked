use crate::*;

use super::models::RuntimeServiceProfile;

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
