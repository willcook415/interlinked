use crate::*;

pub(crate) const AUTO_REVERSE_SERVICE_PREFIX: &str = "auto_reverse::";
pub(crate) const AUTO_REVERSE_LINK_PREFIX: &str = "auto_reverse_link::";
pub(crate) const FLEET_EXPEDITE_MULTIPLIER: f64 = 1.75;
pub(crate) const FLEET_EXPEDITE_MIN_SURCHARGE_BASE: f64 = 100_000.0;

pub(crate) fn hash_string_seq(
    values: &[String],
    hasher: &mut std::collections::hash_map::DefaultHasher,
) {
    for value in values {
        value.hash(hasher);
    }
}

pub(crate) fn is_auto_reverse_service_id(id: &str) -> bool {
    id.starts_with(AUTO_REVERSE_SERVICE_PREFIX)
}

pub(crate) fn is_auto_reverse_link_id(id: &str) -> bool {
    id.starts_with(AUTO_REVERSE_LINK_PREFIX)
}

pub(crate) fn strip_auto_reverse_runtime_artifacts(scenario: &mut Scenario) {
    scenario
        .world
        .services
        .retain(|service| !is_auto_reverse_service_id(&service.id));
    scenario
        .world
        .links
        .retain(|link| !is_auto_reverse_link_id(&link.id));
}

pub(crate) fn normalized_mode_token(mode: &str) -> String {
    mode.trim().to_ascii_lowercase()
}

pub(crate) fn normalized_variant_token(variant: Option<&str>) -> String {
    variant
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub(crate) fn normalized_line_token(line_id: Option<&str>) -> String {
    line_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub(crate) fn service_line_runtime_id(service: &Service) -> String {
    service
        .line_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| service.id.clone())
}

pub(crate) fn is_pending_purchase_order_status(status: Option<&str>) -> bool {
    let normalized = status
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    normalized.is_empty() || normalized == "pending"
}

pub(crate) fn estimate_unit_purchase_cost_base_for_service(
    service: &Service,
    defaults: &BuildDefaults,
) -> Option<f64> {
    let mode = service.mode.trim();
    let service_variant = service
        .mode_variant
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let preset = defaults.presets.iter().find(|candidate| {
        candidate.engine_mode.eq_ignore_ascii_case(mode)
            && candidate
                .mode_variant
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                == service_variant
    })?;
    let profile = service.rolling_stock_profile.as_ref();
    let package_id = profile
        .and_then(|value| value.package_id.as_deref())
        .or(service.stock_tier_id.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    let package_multiplier = preset
        .package_options
        .iter()
        .find(|tier| tier.id.eq_ignore_ascii_case(&package_id))
        .or_else(|| {
            preset
                .package_options
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.package_options.first())
        .or_else(|| {
            preset
                .tiers
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case(&package_id))
        })
        .or_else(|| {
            preset
                .tiers
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.tiers.first())
        .map(|tier| tier.purchase_cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let speed_id = profile
        .and_then(|value| value.speed_level.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "balanced".to_string());
    let speed_multiplier = preset
        .speed_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&speed_id))
        .or_else(|| {
            preset
                .speed_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("balanced"))
        })
        .or_else(|| preset.speed_levels.first())
        .map(|item| item.cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let comfort_id = profile
        .and_then(|value| value.comfort_level.as_deref())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    let comfort_multiplier = preset
        .comfort_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&comfort_id))
        .or_else(|| {
            preset
                .comfort_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.comfort_levels.first())
        .map(|item| item.cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    let cars_per_unit = profile
        .and_then(|value| value.cars_per_unit)
        .unwrap_or(1)
        .max(1) as f64;
    let cars_multiplier = if preset.supports_carriages {
        let base = preset.cars_default.max(1) as f64;
        (cars_per_unit / base).max(0.5)
    } else {
        1.0
    };
    let unit_cost = preset.base_unit_purchase_cost_base.max(0.0)
        * package_multiplier
        * speed_multiplier
        * comfort_multiplier
        * cars_multiplier;
    if unit_cost.is_finite() && unit_cost > 0.0 {
        Some(unit_cost)
    } else {
        None
    }
}

pub(crate) fn resolve_order_unit_cost_base(
    order: &PurchaseOrder,
    fallback: Option<f64>,
) -> Option<f64> {
    if let Some(unit_cost) = order.unit_cost_base {
        if unit_cost.is_finite() && unit_cost > 0.0 {
            return Some(unit_cost);
        }
    }
    if let Some(total_cost) = order.total_cost_base {
        if total_cost.is_finite() && total_cost >= 0.0 && order.units > 0 {
            let per_unit = total_cost / order.units as f64;
            if per_unit.is_finite() && per_unit > 0.0 {
                return Some(per_unit);
            }
        }
    }
    fallback.filter(|value| value.is_finite() && *value > 0.0)
}

pub(crate) fn reverse_direction_fields(
    direction: Option<&str>,
    direction_name: Option<&str>,
) -> (Option<String>, Option<String>) {
    let direction_token = direction
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let direction_name_token = direction_name
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let reversed_direction = if direction_token.contains("forward")
        || direction_token.contains("outbound")
        || direction_token.contains("clockwise")
    {
        "reverse"
    } else if direction_token.contains("reverse")
        || direction_token.contains("inbound")
        || direction_token.contains("backward")
        || direction_token.contains("counterclockwise")
    {
        "forward"
    } else {
        "reverse"
    };
    let reversed_name = if direction_name_token.contains("outbound")
        || direction_name_token.contains("clockwise")
    {
        "Inbound".to_string()
    } else if direction_name_token.contains("inbound")
        || direction_name_token.contains("counterclockwise")
    {
        "Outbound".to_string()
    } else if reversed_direction == "reverse" {
        "Inbound".to_string()
    } else {
        "Outbound".to_string()
    };
    (Some(reversed_direction.to_string()), Some(reversed_name))
}

pub(crate) fn link_key_exact(
    from_stop: &str,
    to_stop: &str,
    line_id: Option<&str>,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_line_token(line_id),
        normalized_mode_token(mode),
        normalized_variant_token(mode_variant)
    )
}

pub(crate) fn link_key_no_line(
    from_stop: &str,
    to_stop: &str,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_mode_token(mode),
        normalized_variant_token(mode_variant)
    )
}

pub(crate) fn reverse_link_geometry(geometry: &Option<Vec<[f64; 2]>>) -> Option<Vec<[f64; 2]>> {
    geometry.as_ref().map(|coords| {
        let mut reversed = coords.clone();
        reversed.reverse();
        reversed
    })
}

pub(crate) fn synthetic_reverse_link_id(
    line_id: &str,
    from_stop: &str,
    to_stop: &str,
    mode: &str,
    mode_variant: Option<&str>,
) -> String {
    let mut id = format!(
        "{AUTO_REVERSE_LINK_PREFIX}{line_id}::{}->{}::{}",
        from_stop.trim(),
        to_stop.trim(),
        normalized_mode_token(mode)
    );
    let variant = normalized_variant_token(mode_variant);
    if !variant.is_empty() {
        id.push_str("::");
        id.push_str(&variant);
    }
    id
}

pub(crate) fn estimate_link_distance_from_stops(
    stop_xy: &HashMap<String, (f64, f64)>,
    from_stop: &str,
    to_stop: &str,
) -> f64 {
    let Some((from_x, from_y)) = stop_xy.get(from_stop) else {
        return 1_000.0;
    };
    let Some((to_x, to_y)) = stop_xy.get(to_stop) else {
        return 1_000.0;
    };
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist.is_finite() && dist > 10.0 {
        dist
    } else {
        1_000.0
    }
}

pub(crate) fn synthesize_auto_reverse_runtime_services(scenario: &mut Scenario) -> usize {
    let mut exact_link_map = HashMap::<String, Link>::new();
    let mut no_line_link_map = HashMap::<String, Link>::new();
    let mut exact_link_keys = HashSet::<String>::new();
    let mut no_line_link_keys = HashSet::<String>::new();
    let mut existing_link_ids = HashSet::<String>::new();
    for link in &scenario.world.links {
        existing_link_ids.insert(link.id.clone());
        let exact_key = link_key_exact(
            &link.from_stop,
            &link.to_stop,
            link.line_id.as_deref(),
            &link.mode,
            link.mode_variant.as_deref(),
        );
        let no_line_key = link_key_no_line(
            &link.from_stop,
            &link.to_stop,
            &link.mode,
            link.mode_variant.as_deref(),
        );
        exact_link_keys.insert(exact_key.clone());
        no_line_link_keys.insert(no_line_key.clone());
        exact_link_map
            .entry(exact_key)
            .or_insert_with(|| link.clone());
        no_line_link_map
            .entry(no_line_key)
            .or_insert_with(|| link.clone());
    }

    let stop_xy = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), (stop.x, stop.y)))
        .collect::<HashMap<_, _>>();

    let mut sequences_by_line = HashMap::<String, HashSet<Vec<String>>>::new();
    for service in &scenario.world.services {
        if is_auto_reverse_service_id(&service.id) || service.stop_sequence.len() < 2 {
            continue;
        }
        sequences_by_line
            .entry(service_line_runtime_id(service))
            .or_default()
            .insert(service.stop_sequence.clone());
    }

    let mut existing_service_ids = scenario
        .world
        .services
        .iter()
        .map(|service| service.id.clone())
        .collect::<HashSet<_>>();
    let mut synthetic_services = Vec::<Service>::new();
    let mut synthetic_links = Vec::<Link>::new();
    let base_services = scenario.world.services.clone();
    for service in base_services {
        if is_auto_reverse_service_id(&service.id) || service.stop_sequence.len() < 2 {
            continue;
        }
        let line_id = service_line_runtime_id(&service);
        let mut reverse_sequence = service.stop_sequence.clone();
        reverse_sequence.reverse();
        if reverse_sequence == service.stop_sequence {
            continue;
        }
        if sequences_by_line
            .get(&line_id)
            .map(|sequences| sequences.contains(&reverse_sequence))
            .unwrap_or(false)
        {
            continue;
        }
        let synthetic_service_id =
            format!("{AUTO_REVERSE_SERVICE_PREFIX}{}::{}", line_id, service.id);
        if existing_service_ids.contains(&synthetic_service_id) {
            continue;
        }
        for segment in reverse_sequence.windows(2) {
            let from_stop = &segment[0];
            let to_stop = &segment[1];
            let reverse_exact_key = link_key_exact(
                from_stop,
                to_stop,
                service.line_id.as_deref(),
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let reverse_no_line_key = link_key_no_line(
                from_stop,
                to_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            if exact_link_keys.contains(&reverse_exact_key)
                || no_line_link_keys.contains(&reverse_no_line_key)
            {
                continue;
            }
            let forward_exact_key = link_key_exact(
                to_stop,
                from_stop,
                service.line_id.as_deref(),
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let forward_no_line_key = link_key_no_line(
                to_stop,
                from_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            let template = exact_link_map
                .get(&forward_exact_key)
                .or_else(|| no_line_link_map.get(&forward_no_line_key))
                .cloned();
            let synthetic_link_id = synthetic_reverse_link_id(
                &line_id,
                from_stop,
                to_stop,
                &service.mode,
                service.mode_variant.as_deref(),
            );
            if existing_link_ids.contains(&synthetic_link_id) {
                exact_link_keys.insert(reverse_exact_key.clone());
                no_line_link_keys.insert(reverse_no_line_key.clone());
                continue;
            }
            let reverse_link = if let Some(forward) = template {
                Link {
                    id: synthetic_link_id.clone(),
                    from_stop: from_stop.clone(),
                    to_stop: to_stop.clone(),
                    distance_m: forward.distance_m.max(1.0),
                    mode: forward.mode.clone(),
                    speed_mps: forward.speed_mps.max(0.1),
                    geometry: reverse_link_geometry(&forward.geometry),
                    line_id: service.line_id.clone().or(forward.line_id.clone()),
                    mode_variant: service
                        .mode_variant
                        .clone()
                        .or(forward.mode_variant.clone()),
                    capacity_per_hour: forward.capacity_per_hour,
                }
            } else {
                Link {
                    id: synthetic_link_id.clone(),
                    from_stop: from_stop.clone(),
                    to_stop: to_stop.clone(),
                    distance_m: estimate_link_distance_from_stops(&stop_xy, from_stop, to_stop),
                    mode: service.mode.clone(),
                    speed_mps: 12.0,
                    geometry: None,
                    line_id: service.line_id.clone(),
                    mode_variant: service.mode_variant.clone(),
                    capacity_per_hour: None,
                }
            };
            exact_link_keys.insert(reverse_exact_key.clone());
            no_line_link_keys.insert(reverse_no_line_key.clone());
            exact_link_map.insert(reverse_exact_key, reverse_link.clone());
            no_line_link_map.insert(reverse_no_line_key, reverse_link.clone());
            existing_link_ids.insert(synthetic_link_id);
            synthetic_links.push(reverse_link);
        }

        let (reverse_direction, reverse_direction_name) = reverse_direction_fields(
            service.direction.as_deref(),
            service.direction_name.as_deref(),
        );
        let mut reverse_service = service.clone();
        reverse_service.id = synthetic_service_id.clone();
        reverse_service.stop_sequence = reverse_sequence.clone();
        reverse_service.direction = reverse_direction;
        reverse_service.direction_name = reverse_direction_name;
        synthetic_services.push(reverse_service);
        existing_service_ids.insert(synthetic_service_id);
        sequences_by_line
            .entry(line_id)
            .or_default()
            .insert(reverse_sequence);
    }

    if !synthetic_links.is_empty() {
        scenario.world.links.extend(synthetic_links);
    }
    let added_services = synthetic_services.len();
    if added_services > 0 {
        scenario.world.services.extend(synthetic_services);
    }
    added_services
}

pub(crate) fn scenario_topology_hash(scenario: &Scenario) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let real_link_count = scenario
        .world
        .links
        .iter()
        .filter(|link| !is_auto_reverse_link_id(&link.id))
        .count();
    let real_service_count = scenario
        .world
        .services
        .iter()
        .filter(|service| !is_auto_reverse_service_id(&service.id))
        .count();
    scenario.world.stops.len().hash(&mut hasher);
    real_link_count.hash(&mut hasher);
    real_service_count.hash(&mut hasher);
    for stop in scenario.world.stops.iter().take(256) {
        stop.id.hash(&mut hasher);
        stop.stop_type.hash(&mut hasher);
        stop.x.to_bits().hash(&mut hasher);
        stop.y.to_bits().hash(&mut hasher);
    }
    for link in scenario
        .world
        .links
        .iter()
        .filter(|link| !is_auto_reverse_link_id(&link.id))
        .take(512)
    {
        link.id.hash(&mut hasher);
        link.from_stop.hash(&mut hasher);
        link.to_stop.hash(&mut hasher);
        link.mode.hash(&mut hasher);
        link.mode_variant.hash(&mut hasher);
        link.distance_m.to_bits().hash(&mut hasher);
    }
    for service in scenario
        .world
        .services
        .iter()
        .filter(|service| !is_auto_reverse_service_id(&service.id))
        .take(512)
    {
        service.id.hash(&mut hasher);
        service.line_id.hash(&mut hasher);
        service.mode.hash(&mut hasher);
        service.mode_variant.hash(&mut hasher);
        service.headway_s.to_bits().hash(&mut hasher);
        service.dwell_s.to_bits().hash(&mut hasher);
        service.vehicle_capacity.to_bits().hash(&mut hasher);
        hash_string_seq(&service.stop_sequence, &mut hasher);
    }
    hasher.finish()
}

pub(crate) fn scope_hash(manifest: &ProjectManifest) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    manifest.simulation_scope.max_active_zones.hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_regions_mode
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_update_interval_ticks
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .focus_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .adjacent_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .remote_max_active_zones
        .hash(&mut hasher);
    manifest
        .simulation_scope
        .adjacent_update_interval_ticks
        .hash(&mut hasher);
    hash_string_seq(&manifest.region_state.active_region_ids, &mut hasher);
    manifest
        .region_state
        .primary_focus_region_id
        .hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn fare_hash(policy: &FarePolicyManifest) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    policy.enabled.hash(&mut hasher);
    policy.fare_mode_bus_base.to_bits().hash(&mut hasher);
    policy.fare_mode_tram_base.to_bits().hash(&mut hasher);
    policy.fare_mode_metro_base.to_bits().hash(&mut hasher);
    policy.fare_mode_rail_base.to_bits().hash(&mut hasher);
    policy.fare_mode_ferry_base.to_bits().hash(&mut hasher);
    policy.fare_mode_default_base.to_bits().hash(&mut hasher);
    policy.transfer_window_s.to_bits().hash(&mut hasher);
    policy.free_transfers_per_trip.hash(&mut hasher);
    hasher.finish()
}
