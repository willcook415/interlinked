use std::collections::{HashMap, HashSet};

use interlinked_engine::model::{Link, Scenario, Service, Stop};
use interlinked_engine::platform::EconomyConfig;
use interlinked_engine::sim::SimulationOutput;
use serde::{Deserialize, Serialize};

use super::defaults::{default_build_defaults, find_mode_preset, ModeBuildPreset};
use super::fleet_state::{
    active_schedule_band, canonical_service, effective_capacity_for_line, enabled_for_service,
    line_roll_profile, line_schedule_from_service, normalize_tier_id, pending_units_for_orders,
    required_units_for_tph, service_display_name, service_line_id, stop_display_name,
    target_tph_for_service, tier_cost_base, tier_label, tph_for_band, window_duration_minutes,
    FleetPurchaseOrderState, LineScheduleState,
};

const AUTO_REVERSE_SERVICE_PREFIX: &str = "auto_reverse::";
const AUTO_REVERSE_LINK_PREFIX: &str = "auto_reverse_link::";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStationSummary {
    pub stop_id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub cumulative_time_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineDirectionSummary {
    pub service_id: String,
    pub name: String,
    pub direction: Option<String>,
    pub direction_name: Option<String>,
    pub stop_sequence: Vec<String>,
    pub headway_s: f64,
    pub dwell_s: f64,
    pub vehicle_capacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineOperationsNow {
    pub active_band: String,
    pub live_tph: f64,
    pub avg_wait_s: Option<f64>,
    pub capacity_per_hour: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineFleetState {
    #[serde(default)]
    pub pending_orders: Vec<FleetPurchaseOrderState>,
    pub package_id: Option<String>,
    pub package_label: Option<String>,
    pub cars_per_unit: u32,
    pub speed_level: Option<String>,
    pub comfort_level: Option<String>,
    pub units_owned: usize,
    pub units_pending: usize,
    pub units_committed: usize,
    pub units_assigned: usize,
    pub units_required_now: usize,
    pub units_shortage_now: usize,
    pub units_surplus_now: usize,
    pub vehicle_capacity_effective: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineCostStory {
    pub fleet_value_base: f64,
    pub fleet_purchase_delta_base: f64,
    pub fleet_configuration_delta_base: f64,
    pub fleet_transfer_fees_base: f64,
    pub fleet_salvage_refund_base: f64,
    pub service_opex_per_hour_base: f64,
    pub staff_opex_per_hour_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineInspection {
    pub line_id: String,
    pub name: String,
    pub mode: String,
    pub mode_variant: Option<String>,
    pub display_color: Option<String>,
    pub station_count: usize,
    pub service_count: usize,
    pub length_m: f64,
    pub estimated_capex_base: f64,
    pub estimated_opex_per_hour_base: f64,
    pub total_passengers: f64,
    pub boardings_attempted: f64,
    pub boardings_served: f64,
    pub alightings_served: f64,
    pub denied_boardings: f64,
    pub queue_end: f64,
    pub service_enabled: bool,
    pub target_tph: f64,
    pub effective_tph: f64,
    pub avg_wait_s: Option<f64>,
    pub vehicle_capacity_effective: f64,
    pub line_capacity_per_hour: f64,
    pub required_units: usize,
    pub owned_units: usize,
    pub assigned_units: usize,
    pub spare_units: usize,
    pub stock_tier_id: Option<String>,
    pub stock_tier_label: Option<String>,
    pub operations_now: LineOperationsNow,
    pub fleet_state: LineFleetState,
    pub schedule_state: LineScheduleState,
    pub cost_story: LineCostStory,
    pub stations: Vec<LineStationSummary>,
    pub directions: Vec<LineDirectionSummary>,
}

#[derive(Debug, Clone)]
pub struct LineComputed {
    pub line_id: String,
    pub name: String,
    pub mode: String,
    pub mode_variant: Option<String>,
    pub display_color: Option<String>,
    pub service_count: usize,
    pub station_ids: Vec<String>,
    pub cumulative_time_s_by_stop_id: HashMap<String, f64>,
    pub link_ids: Vec<String>,
    pub length_m: f64,
    pub service_enabled: bool,
    pub vehicle_capacity_effective: f64,
    pub stock_tier_id: Option<String>,
    pub stock_units_owned: usize,
    pub stock_units_pending: usize,
    pub stock_units_assigned: usize,
    pub pending_orders: Vec<FleetPurchaseOrderState>,
    pub cars_per_unit: u32,
    pub speed_level: Option<String>,
    pub comfort_level: Option<String>,
    pub schedule_state: LineScheduleState,
    pub directions: Vec<LineDirectionSummary>,
}

fn is_auto_reverse_service_for_line(service_id: &str, line_id: &str) -> bool {
    let prefix = format!("{AUTO_REVERSE_SERVICE_PREFIX}{line_id}::");
    service_id.starts_with(&prefix)
}

fn is_auto_reverse_link_for_line(link_id: &str, line_id: &str) -> bool {
    let prefix = format!("{AUTO_REVERSE_LINK_PREFIX}{line_id}::");
    link_id.starts_with(&prefix)
}

fn is_shape_stop(stop: &Stop) -> bool {
    stop.stop_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().contains("shape"))
        .unwrap_or(false)
}

fn line_link_candidates<'a>(scenario: &'a Scenario, line_id: &str, mode: &str) -> Vec<&'a Link> {
    let explicit = scenario
        .world
        .links
        .iter()
        .filter(|link| link.line_id.as_deref() == Some(line_id))
        .collect::<Vec<_>>();
    if !explicit.is_empty() {
        return explicit;
    }
    scenario
        .world
        .links
        .iter()
        .filter(|link| link.mode == mode)
        .collect::<Vec<_>>()
}

fn service_route_link_ids(
    scenario: &Scenario,
    service: &Service,
    line_id: &str,
) -> (Vec<String>, f64, HashMap<String, f64>) {
    let mut link_ids = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    let mut length_m = 0.0;
    let mut cumulative_time_s = HashMap::<String, f64>::new();
    let mut running_time_s = 0.0;
    if let Some(first) = service.stop_sequence.first() {
        cumulative_time_s.insert(first.clone(), 0.0);
    }

    let mut stop_lookup = HashMap::<String, &Stop>::new();
    for stop in &scenario.world.stops {
        stop_lookup.insert(stop.id.clone(), stop);
    }

    for pair in service.stop_sequence.windows(2) {
        let [from_stop, to_stop] = pair else {
            continue;
        };
        let link = scenario
            .world
            .links
            .iter()
            .find(|candidate| {
                candidate.from_stop == *from_stop
                    && candidate.to_stop == *to_stop
                    && candidate.mode == service.mode
                    && candidate
                        .line_id
                        .as_deref()
                        .map(|value| value == line_id)
                        .unwrap_or(true)
            })
            .or_else(|| {
                scenario.world.links.iter().find(|candidate| {
                    candidate.from_stop == *from_stop
                        && candidate.to_stop == *to_stop
                        && candidate.mode == service.mode
                })
            });

        let (distance_m, speed_mps, link_id) = if let Some(link) = link {
            (
                link.distance_m.max(0.0),
                link.speed_mps.max(0.1),
                Some(link.id.clone()),
            )
        } else {
            let distance_m = match (stop_lookup.get(from_stop), stop_lookup.get(to_stop)) {
                (Some(from), Some(to)) => ((from.x - to.x).powi(2) + (from.y - to.y).powi(2)).sqrt(),
                _ => 0.0,
            };
            (distance_m, 12.0, None)
        };

        if let Some(id) = link_id {
            if seen.insert(id.clone()) {
                link_ids.push(id);
            }
        }
        length_m += distance_m;
        running_time_s += distance_m / speed_mps + service.dwell_s.max(0.0);
        cumulative_time_s.insert(to_stop.clone(), running_time_s);
    }

    (link_ids, length_m, cumulative_time_s)
}

pub fn compute_lines(scenario: &Scenario) -> Vec<LineComputed> {
    let defaults = default_build_defaults(&interlinked_engine::platform::default_economy_config());
    let stop_lookup = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), stop))
        .collect::<HashMap<_, _>>();

    let mut groups = HashMap::<String, Vec<&Service>>::new();
    for service in &scenario.world.services {
        groups
            .entry(service_line_id(service))
            .or_default()
            .push(service);
    }

    let mut lines = groups
        .into_iter()
        .filter_map(|(line_id, services)| {
            let canonical = canonical_service(&services)?;
            let (fallback_link_ids, fallback_length_m, cumulative_time_s_by_stop_id) =
                service_route_link_ids(scenario, canonical, &line_id);

            let explicit_line_links = line_link_candidates(scenario, &line_id, &canonical.mode);
            let (link_ids, length_m) = if explicit_line_links
                .iter()
                .any(|link| link.line_id.as_deref() == Some(line_id.as_str()))
            {
                let mut unique = Vec::<String>::new();
                let mut seen = HashSet::<String>::new();
                let mut total_m = 0.0;
                for link in explicit_line_links {
                    if seen.insert(link.id.clone()) {
                        unique.push(link.id.clone());
                        total_m += link.distance_m.max(0.0);
                    }
                }
                (unique, total_m)
            } else {
                (fallback_link_ids, fallback_length_m)
            };

            let station_ids = canonical
                .stop_sequence
                .iter()
                .filter(|stop_id| {
                    stop_lookup
                        .get(*stop_id)
                        .map(|stop| !is_shape_stop(stop))
                        .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>();

            let display_color = services
                .iter()
                .find_map(|service| service.display_color.clone());
            let mode_variant = services
                .iter()
                .find_map(|service| service.mode_variant.clone());
            let preset = find_mode_preset(&defaults, &canonical.mode, mode_variant.as_deref());
            let name = services
                .iter()
                .find_map(|service| {
                    service
                        .name
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                })
                .unwrap_or_else(|| "Untitled Line".to_string());
            let target_tph = target_tph_for_service(canonical);
            let service_enabled = enabled_for_service(canonical, target_tph);
            let (
                package_id,
                stock_units_owned,
                cars_per_unit,
                speed_level,
                comfort_level,
                pending_orders,
            ) = if let Some(preset) = preset {
                line_roll_profile(canonical, preset)
            } else {
                (
                    normalize_tier_id(canonical.stock_tier_id.as_deref()),
                    canonical.stock_units_owned.unwrap_or(0) as usize,
                    1,
                    None,
                    None,
                    Vec::new(),
                )
            };
            let stock_units_pending = pending_units_for_orders(&pending_orders);
            let stock_units_assigned = canonical
                .stock_units_assigned
                .unwrap_or(stock_units_owned as u32)
                .min(stock_units_owned as u32) as usize;
            let schedule_state = line_schedule_from_service(canonical);
            let vehicle_capacity_effective = if let Some(preset) = preset {
                effective_capacity_for_line(preset, Some(package_id.as_str()), cars_per_unit)
            } else {
                canonical.vehicle_capacity.max(0.0)
            };
            let directions = services
                .iter()
                .map(|service| LineDirectionSummary {
                    service_id: service.id.clone(),
                    name: service_display_name(service),
                    direction: service.direction.clone(),
                    direction_name: service.direction_name.clone(),
                    stop_sequence: service.stop_sequence.clone(),
                    headway_s: service.headway_s,
                    dwell_s: service.dwell_s,
                    vehicle_capacity: service.vehicle_capacity,
                })
                .collect::<Vec<_>>();

            Some(LineComputed {
                line_id,
                name,
                mode: canonical.mode.clone(),
                mode_variant,
                display_color,
                service_count: services.len(),
                station_ids,
                cumulative_time_s_by_stop_id,
                link_ids,
                length_m,
                service_enabled,
                vehicle_capacity_effective,
                stock_tier_id: Some(package_id),
                stock_units_owned,
                stock_units_pending,
                stock_units_assigned,
                pending_orders,
                cars_per_unit,
                speed_level,
                comfort_level,
                schedule_state,
                directions,
            })
        })
        .collect::<Vec<_>>();

    lines.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.line_id.cmp(&b.line_id)));
    lines
}

pub(super) fn line_round_trip_seconds(line: &LineComputed) -> f64 {
    let one_way_s = line
        .station_ids
        .iter()
        .filter_map(|stop_id| line.cumulative_time_s_by_stop_id.get(stop_id).copied())
        .fold(0.0_f64, f64::max);
    (one_way_s * 2.0).max(300.0)
}

pub(super) fn staff_opex_for_line(line: &LineComputed, preset: &ModeBuildPreset) -> f64 {
    let round_trip_s = line_round_trip_seconds(line);
    let peak_units = required_units_for_tph(round_trip_s, line.schedule_state.tph_peak);
    let off_peak_units = required_units_for_tph(round_trip_s, line.schedule_state.tph_off_peak);
    let overnight_units = required_units_for_tph(round_trip_s, line.schedule_state.tph_overnight);
    let peak_minutes = window_duration_minutes(
        line.schedule_state.peak_start_minute,
        line.schedule_state.peak_end_minute,
    );
    let overnight_minutes = window_duration_minutes(
        line.schedule_state.overnight_start_minute,
        line.schedule_state.overnight_end_minute,
    );
    let off_peak_minutes = 1440_u32.saturating_sub(peak_minutes.saturating_add(overnight_minutes));
    let peak_hour_weight = peak_minutes as f64 / 60.0 / 24.0;
    let off_peak_hour_weight = off_peak_minutes as f64 / 60.0 / 24.0;
    let overnight_hour_weight = overnight_minutes as f64 / 60.0 / 24.0;
    let base = preset.staff_cost_per_unit_hour_base.max(0.0);
    (peak_units as f64 * base * preset.staff_shift_multiplier_peak.max(0.0) * peak_hour_weight)
        + (off_peak_units as f64 * base * off_peak_hour_weight)
        + (overnight_units as f64
            * base
            * preset.staff_shift_multiplier_overnight.max(0.0)
            * overnight_hour_weight)
}

pub fn inspect_line_from_scenario(
    scenario: &Scenario,
    output: Option<&SimulationOutput>,
    line_id: &str,
    cfg: &EconomyConfig,
    minute_of_day: Option<u32>,
) -> Result<LineInspection, String> {
    let line = compute_lines(scenario)
        .into_iter()
        .find(|candidate| candidate.line_id == line_id)
        .ok_or_else(|| format!("line not found: {line_id}"))?;

    let stop_lookup = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), stop))
        .collect::<HashMap<_, _>>();
    let stations = line
        .station_ids
        .iter()
        .filter_map(|stop_id| {
            let stop = stop_lookup.get(stop_id)?;
            Some(LineStationSummary {
                stop_id: stop.id.clone(),
                name: stop_display_name(stop),
                x: stop.x,
                y: stop.y,
                cumulative_time_s: line
                    .cumulative_time_s_by_stop_id
                    .get(stop_id)
                    .copied()
                    .unwrap_or(0.0),
            })
        })
        .collect::<Vec<_>>();
    let total_passengers = output
        .map(|value| {
            value
                .link_loads
                .iter()
                .filter(|load| {
                    line.link_ids.iter().any(|link_id| link_id == &load.link_id)
                        || is_auto_reverse_link_for_line(&load.link_id, line_id)
                })
                .map(|load| load.passengers)
                .sum()
        })
        .unwrap_or(0.0);
    let line_services = scenario
        .world
        .services
        .iter()
        .filter(|service| service_line_id(service) == line_id)
        .collect::<Vec<_>>();
    let line_service_ids = line_services
        .iter()
        .map(|service| service.id.as_str())
        .collect::<HashSet<_>>();
    let (boardings_attempted, boardings_served, alightings_served, denied_boardings, queue_end) =
        output
            .map(|value| {
                value
                    .board_loads
                    .iter()
                    .filter(|load| {
                        line_service_ids.contains(load.service_id.as_str())
                            || is_auto_reverse_service_for_line(&load.service_id, line_id)
                    })
                    .fold((0.0, 0.0, 0.0, 0.0, 0.0), |acc, load| {
                        (
                            acc.0 + load.arrivals.max(0.0),
                            acc.1 + (load.served_from_arrivals + load.served_from_queue).max(0.0),
                            acc.2 + load.alightings_served.max(0.0),
                            acc.3 + load.denied_boardings.max(0.0),
                            acc.4 + load.queue_end.max(0.0),
                        )
                    })
            })
            .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
    let mode_key = line.mode.to_ascii_lowercase();
    let mode_capex_per_km = cfg
        .mode_capex_per_km_base
        .get(&mode_key)
        .copied()
        .unwrap_or_else(|| {
            if mode_key == "bus" {
                0.0
            } else {
                cfg.link_capex_per_km_default_base
            }
        });
    let estimated_capex_base = (line.length_m.max(0.0) / 1000.0) * mode_capex_per_km
        + (stations.len() as f64 * cfg.station_capex_base);
    let estimated_opex_per_hour_base = line_services
        .iter()
        .map(|service| {
            let service_scenario = Scenario {
                meta: scenario.meta.clone(),
                params: scenario.params.clone(),
                world: interlinked_engine::model::World {
                    zones: vec![],
                    stops: scenario.world.stops.clone(),
                    links: scenario.world.links.clone(),
                    services: vec![(*service).clone()],
                    transfers: vec![],
                    transfer_rules: scenario.world.transfer_rules.clone(),
                    demand_cells: vec![],
                    demand_meta: None,
                },
            };
            interlinked_engine::platform::estimate_service_opex_per_hour_base(&service_scenario, cfg)
        })
        .sum();
    let defaults = default_build_defaults(cfg);
    let preset = find_mode_preset(&defaults, &line.mode, line.mode_variant.as_deref());
    let round_trip_s = line_round_trip_seconds(&line);
    let schedule_state = line.schedule_state.clone();
    let active_band = active_schedule_band(&schedule_state, minute_of_day.unwrap_or(540));
    let target_tph = tph_for_band(&schedule_state, active_band.as_str()).max(0.0);
    let required_units = required_units_for_tph(round_trip_s, target_tph);
    let owned_units = line.stock_units_owned;
    let pending_units = line.stock_units_pending;
    let committed_units = owned_units.saturating_add(pending_units);
    let assigned_units = line.stock_units_assigned.min(owned_units);
    let max_tph_from_fleet = if round_trip_s > 0.0 {
        (assigned_units as f64 * 3600.0) / round_trip_s
    } else {
        0.0
    };
    let effective_tph = if target_tph > 0.0 {
        target_tph.min(max_tph_from_fleet.max(0.0))
    } else {
        0.0
    };
    let avg_wait_s = if effective_tph > 0.0 {
        Some(1800.0 / effective_tph)
    } else {
        None
    };
    let line_capacity_per_hour = effective_tph * line.vehicle_capacity_effective.max(0.0);
    let spare_units = owned_units.saturating_sub(assigned_units);
    let stock_tier_id = Some(normalize_tier_id(line.stock_tier_id.as_deref()));
    let stock_tier_label = preset.and_then(|resolved| tier_label(resolved, stock_tier_id.as_deref()));
    let units_shortage_now = required_units.saturating_sub(assigned_units);
    let units_surplus_now = assigned_units.saturating_sub(required_units);
    let staff_opex_per_hour_base = preset
        .map(|resolved| staff_opex_for_line(&line, resolved))
        .unwrap_or(0.0);
    let fleet_value_base = preset
        .map(|resolved| line.stock_units_owned as f64 * tier_cost_base(resolved, stock_tier_id.as_deref()))
        .unwrap_or(0.0);

    Ok(LineInspection {
        line_id: line.line_id,
        name: line.name,
        mode: line.mode,
        mode_variant: line.mode_variant,
        display_color: line.display_color,
        station_count: stations.len(),
        service_count: line.service_count,
        length_m: line.length_m,
        estimated_capex_base,
        estimated_opex_per_hour_base,
        total_passengers,
        boardings_attempted,
        boardings_served,
        alightings_served,
        denied_boardings,
        queue_end,
        service_enabled: line.service_enabled,
        target_tph,
        effective_tph,
        avg_wait_s,
        vehicle_capacity_effective: line.vehicle_capacity_effective,
        line_capacity_per_hour,
        required_units,
        owned_units,
        assigned_units,
        spare_units,
        stock_tier_id,
        stock_tier_label,
        operations_now: LineOperationsNow {
            active_band,
            live_tph: effective_tph,
            avg_wait_s,
            capacity_per_hour: line_capacity_per_hour,
        },
        fleet_state: LineFleetState {
            pending_orders: line.pending_orders.clone(),
            package_id: line.stock_tier_id.clone(),
            package_label: preset.and_then(|resolved| tier_label(resolved, line.stock_tier_id.as_deref())),
            cars_per_unit: line.cars_per_unit,
            speed_level: line.speed_level.clone(),
            comfort_level: line.comfort_level.clone(),
            units_owned: owned_units,
            units_pending: pending_units,
            units_committed: committed_units,
            units_assigned: assigned_units,
            units_required_now: required_units,
            units_shortage_now,
            units_surplus_now,
            vehicle_capacity_effective: line.vehicle_capacity_effective,
        },
        schedule_state,
        cost_story: LineCostStory {
            fleet_value_base,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            service_opex_per_hour_base: estimated_opex_per_hour_base,
            staff_opex_per_hour_base,
        },
        stations,
        directions: line.directions,
    })
}
