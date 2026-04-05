use std::collections::HashMap;

use interlinked_engine::model::{PurchaseOrder, Scenario, Service, Stop};
use serde::{Deserialize, Serialize};

use super::defaults::{ModeBuildPreset, RollingStockTierPreset};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetPurchaseOrderState {
    pub order_id: String,
    pub units: u32,
    pub status: String,
    pub unit_cost_base: Option<f64>,
    pub total_cost_base: Option<f64>,
    pub placed_at_tick_s: Option<f64>,
    pub eta_at_tick_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineScheduleState {
    pub peak_start_minute: u32,
    pub peak_end_minute: u32,
    pub overnight_start_minute: u32,
    pub overnight_end_minute: u32,
    pub tph_peak: f64,
    pub tph_off_peak: f64,
    pub tph_overnight: f64,
}

pub(super) fn normalize_tier_id(raw: Option<&str>) -> String {
    let fallback = "standard".to_string();
    raw.map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
}

pub(super) fn resolve_tier<'a>(
    preset: &'a ModeBuildPreset,
    tier_id: Option<&str>,
) -> Option<&'a RollingStockTierPreset> {
    let wanted = normalize_tier_id(tier_id);
    preset
        .package_options
        .iter()
        .find(|tier| tier.id.eq_ignore_ascii_case(&wanted))
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
                .find(|tier| tier.id.eq_ignore_ascii_case(&wanted))
        })
        .or_else(|| {
            preset
                .tiers
                .iter()
                .find(|tier| tier.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.tiers.first())
}

pub(super) fn resolve_speed_level<'a>(
    preset: &'a ModeBuildPreset,
    speed_level_id: Option<&str>,
) -> Option<&'a super::defaults::SpeedLevelPreset> {
    let wanted = speed_level_id
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "balanced".to_string());
    preset
        .speed_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&wanted))
        .or_else(|| {
            preset
                .speed_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("balanced"))
        })
        .or_else(|| preset.speed_levels.first())
}

pub(super) fn resolve_comfort_level<'a>(
    preset: &'a ModeBuildPreset,
    comfort_level_id: Option<&str>,
) -> Option<&'a super::defaults::ComfortLevelPreset> {
    let wanted = comfort_level_id
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    preset
        .comfort_levels
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(&wanted))
        .or_else(|| {
            preset
                .comfort_levels
                .iter()
                .find(|item| item.id.eq_ignore_ascii_case("standard"))
        })
        .or_else(|| preset.comfort_levels.first())
}

pub(super) fn line_schedule_from_service(service: &Service) -> LineScheduleState {
    let defaults = LineScheduleState {
        peak_start_minute: 420,
        peak_end_minute: 570,
        overnight_start_minute: 0,
        overnight_end_minute: 300,
        tph_peak: 0.0,
        tph_off_peak: 0.0,
        tph_overnight: 0.0,
    };
    let Some(profile) = service.schedule_profile.as_ref() else {
        let tph = target_tph_for_service(service).max(0.0);
        return LineScheduleState {
            tph_peak: tph,
            tph_off_peak: tph,
            tph_overnight: tph,
            ..defaults
        };
    };
    LineScheduleState {
        peak_start_minute: profile
            .peak_start_minute
            .unwrap_or(defaults.peak_start_minute)
            % 1440,
        peak_end_minute: profile.peak_end_minute.unwrap_or(defaults.peak_end_minute) % 1440,
        overnight_start_minute: profile
            .overnight_start_minute
            .unwrap_or(defaults.overnight_start_minute)
            % 1440,
        overnight_end_minute: profile
            .overnight_end_minute
            .unwrap_or(defaults.overnight_end_minute)
            % 1440,
        tph_peak: profile.tph_peak.unwrap_or(defaults.tph_peak).max(0.0),
        tph_off_peak: profile
            .tph_off_peak
            .unwrap_or(defaults.tph_off_peak)
            .max(0.0),
        tph_overnight: profile
            .tph_overnight
            .unwrap_or(defaults.tph_overnight)
            .max(0.0),
    }
}

fn minute_in_window(minute: u32, start: u32, end: u32) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    }
}

pub(super) fn active_schedule_band(schedule: &LineScheduleState, minute_of_day: u32) -> String {
    let m = minute_of_day % 1440;
    if minute_in_window(m, schedule.peak_start_minute, schedule.peak_end_minute) {
        "peak".to_string()
    } else if minute_in_window(
        m,
        schedule.overnight_start_minute,
        schedule.overnight_end_minute,
    ) {
        "overnight".to_string()
    } else {
        "off_peak".to_string()
    }
}

pub(super) fn tph_for_band(schedule: &LineScheduleState, band: &str) -> f64 {
    match band {
        "peak" => schedule.tph_peak.max(0.0),
        "overnight" => schedule.tph_overnight.max(0.0),
        _ => schedule.tph_off_peak.max(0.0),
    }
}

pub(super) fn normalized_cars_for_mode(preset: &ModeBuildPreset, cars_per_unit: Option<u32>) -> u32 {
    if !preset.supports_carriages {
        return 1;
    }
    let min = preset.cars_min.max(1);
    let max = preset.cars_max.max(min);
    cars_per_unit.unwrap_or(preset.cars_default).clamp(min, max)
}

pub(super) fn effective_capacity_for_line(
    preset: &ModeBuildPreset,
    package_id: Option<&str>,
    cars_per_unit: u32,
) -> f64 {
    let package_multiplier = resolve_tier(preset, package_id)
        .map(|tier| tier.capacity_multiplier.max(0.1))
        .unwrap_or(1.0);
    let cars_ratio = if preset.supports_carriages {
        let base = preset.cars_default.max(1) as f64;
        (cars_per_unit.max(1) as f64 / base).max(0.25)
    } else {
        1.0
    };
    (preset.default_vehicle_capacity.max(1.0) * package_multiplier * cars_ratio).max(1.0)
}

pub(super) fn effective_speed_for_line(
    preset: &ModeBuildPreset,
    package_id: Option<&str>,
    speed_level: Option<&str>,
) -> f64 {
    let package_multiplier = resolve_tier(preset, package_id)
        .map(|tier| tier.speed_multiplier.max(0.1))
        .unwrap_or(1.0);
    let speed_multiplier = resolve_speed_level(preset, speed_level)
        .map(|level| level.speed_multiplier.max(0.1))
        .unwrap_or(1.0);
    (preset.default_speed_mps.max(0.1) * package_multiplier * speed_multiplier).max(0.1)
}

pub(super) fn normalize_pending_orders(orders: &[PurchaseOrder]) -> Vec<FleetPurchaseOrderState> {
    orders
        .iter()
        .filter_map(|order| {
            let order_id = order.order_id.trim().to_string();
            if order_id.is_empty() {
                return None;
            }
            let units = order.units;
            if units == 0 {
                return None;
            }
            let status = order
                .status
                .as_ref()
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "pending".to_string());
            if status != "pending" {
                return None;
            }
            Some(FleetPurchaseOrderState {
                order_id,
                units,
                status,
                unit_cost_base: order
                    .unit_cost_base
                    .filter(|value| value.is_finite() && *value >= 0.0),
                total_cost_base: order
                    .total_cost_base
                    .filter(|value| value.is_finite() && *value >= 0.0),
                placed_at_tick_s: order
                    .placed_at_tick_s
                    .filter(|value| value.is_finite() && *value >= 0.0),
                eta_at_tick_s: order
                    .eta_at_tick_s
                    .filter(|value| value.is_finite() && *value >= 0.0),
            })
        })
        .collect::<Vec<_>>()
}

pub(super) fn pending_units_for_orders(orders: &[FleetPurchaseOrderState]) -> usize {
    orders
        .iter()
        .map(|order| order.units as usize)
        .sum::<usize>()
}

pub(super) fn pending_commitment_base_for_orders(
    orders: &[FleetPurchaseOrderState],
    fallback_unit_cost_base: f64,
) -> f64 {
    orders
        .iter()
        .map(|order| {
            if let Some(total) = order.total_cost_base {
                total.max(0.0)
            } else {
                order.units as f64
                    * order
                        .unit_cost_base
                        .unwrap_or(fallback_unit_cost_base)
                        .max(0.0)
            }
        })
        .sum::<f64>()
}

pub(super) fn line_roll_profile(
    service: &Service,
    preset: &ModeBuildPreset,
) -> (
    String,
    usize,
    u32,
    Option<String>,
    Option<String>,
    Vec<FleetPurchaseOrderState>,
) {
    let package_id = service
        .rolling_stock_profile
        .as_ref()
        .and_then(|profile| profile.package_id.clone())
        .or_else(|| service.stock_tier_id.clone())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    let units_owned = service
        .rolling_stock_profile
        .as_ref()
        .and_then(|profile| profile.units_owned)
        .unwrap_or_else(|| service.stock_units_owned.unwrap_or(0)) as usize;
    let cars_per_unit = normalized_cars_for_mode(
        preset,
        service
            .rolling_stock_profile
            .as_ref()
            .and_then(|profile| profile.cars_per_unit),
    );
    let speed_level = service
        .rolling_stock_profile
        .as_ref()
        .and_then(|profile| profile.speed_level.clone())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let comfort_level = service
        .rolling_stock_profile
        .as_ref()
        .and_then(|profile| profile.comfort_level.clone())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let pending_orders = service
        .rolling_stock_profile
        .as_ref()
        .map(|profile| normalize_pending_orders(&profile.pending_orders))
        .unwrap_or_default();
    (
        package_id,
        units_owned,
        cars_per_unit,
        speed_level,
        comfort_level,
        pending_orders,
    )
}

pub(super) fn tier_cost_base(preset: &ModeBuildPreset, tier_id: Option<&str>) -> f64 {
    let multiplier = resolve_tier(preset, tier_id)
        .map(|tier| tier.purchase_cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    preset.base_unit_purchase_cost_base.max(0.0) * multiplier
}

pub(super) fn tier_label(preset: &ModeBuildPreset, tier_id: Option<&str>) -> Option<String> {
    resolve_tier(preset, tier_id).map(|tier| tier.label.clone())
}

pub(super) fn stop_display_name(stop: &Stop) -> String {
    stop.name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| stop.id.clone())
}

pub(super) fn service_line_id(service: &Service) -> String {
    service
        .line_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| service.id.clone())
}

pub(super) fn service_display_name(service: &Service) -> String {
    service
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| service.id.clone())
}

pub(super) fn is_shape_stop(stop: &Stop) -> bool {
    stop.stop_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().contains("shape"))
        .unwrap_or(false)
}

pub(super) fn canonical_service<'a>(services: &[&'a Service]) -> Option<&'a Service> {
    services
        .iter()
        .copied()
        .find(|service| service.direction.as_deref() == Some("forward"))
        .or_else(|| {
            services
                .iter()
                .copied()
                .max_by_key(|service| service.stop_sequence.len())
        })
}

pub(super) fn target_tph_for_service(service: &Service) -> f64 {
    if let Some(tph) = service.operating_tph {
        return tph.max(0.0);
    }
    if service.headway_s > 0.0 {
        return (3600.0 / service.headway_s).max(0.0);
    }
    0.0
}

pub(super) fn enabled_for_service(service: &Service, target_tph: f64) -> bool {
    service
        .service_enabled
        .unwrap_or(target_tph > 0.0 && service.headway_s < 86_399.0)
}

pub(super) fn required_units_for_tph(round_trip_s: f64, tph: f64) -> usize {
    if round_trip_s <= 0.0 || tph <= 0.0 {
        return 0;
    }
    ((round_trip_s * tph) / 3600.0).ceil().max(0.0) as usize
}

pub(super) fn window_duration_minutes(start: u32, end: u32) -> u32 {
    if start == end {
        0
    } else if start < end {
        end - start
    } else {
        (1440 - start) + end
    }
}

fn purchase_order_from_state(order: &FleetPurchaseOrderState) -> PurchaseOrder {
    PurchaseOrder {
        order_id: order.order_id.clone(),
        units: order.units,
        status: Some(order.status.clone()),
        unit_cost_base: order.unit_cost_base,
        total_cost_base: order.total_cost_base,
        placed_at_tick_s: order.placed_at_tick_s,
        eta_at_tick_s: order.eta_at_tick_s,
    }
}

pub(super) fn sync_legacy_fields_from_profiles(
    service: &mut Service,
    package_id: &str,
    units_owned: usize,
    units_assigned: usize,
    cars_per_unit: u32,
    speed_level: Option<String>,
    comfort_level: Option<String>,
    pending_orders: &[FleetPurchaseOrderState],
    schedule_state: &LineScheduleState,
) {
    service.stock_tier_id = Some(package_id.to_string());
    service.stock_units_owned = Some(units_owned as u32);
    service.stock_units_assigned = Some(units_assigned as u32);
    service.rolling_stock_profile = Some(interlinked_engine::model::RollingStockProfile {
        package_id: Some(package_id.to_string()),
        units_owned: Some(units_owned as u32),
        cars_per_unit: Some(cars_per_unit),
        speed_level,
        comfort_level,
        pending_orders: pending_orders
            .iter()
            .map(purchase_order_from_state)
            .collect::<Vec<_>>(),
    });
    service.schedule_profile = Some(interlinked_engine::model::LineScheduleProfile {
        peak_start_minute: Some(schedule_state.peak_start_minute),
        peak_end_minute: Some(schedule_state.peak_end_minute),
        overnight_start_minute: Some(schedule_state.overnight_start_minute),
        overnight_end_minute: Some(schedule_state.overnight_end_minute),
        tph_peak: Some(schedule_state.tph_peak.max(0.0)),
        tph_off_peak: Some(schedule_state.tph_off_peak.max(0.0)),
        tph_overnight: Some(schedule_state.tph_overnight.max(0.0)),
    });
}

pub fn settle_pending_purchase_orders(scenario: &mut Scenario, now_tick_s: f64) -> usize {
    if !now_tick_s.is_finite() || now_tick_s < 0.0 {
        return 0;
    }
    let mut services_by_line = HashMap::<String, Vec<usize>>::new();
    for (index, service) in scenario.world.services.iter().enumerate() {
        services_by_line
            .entry(service_line_id(service))
            .or_default()
            .push(index);
    }
    let mut delivered_total = 0usize;
    for service_indexes in services_by_line.values() {
        let Some(first_index) = service_indexes.first().copied() else {
            continue;
        };
        let Some(sample) = scenario.world.services.get(first_index).cloned() else {
            continue;
        };
        let profile = sample.rolling_stock_profile.clone().unwrap_or_default();
        let current_units_owned = profile
            .units_owned
            .unwrap_or_else(|| sample.stock_units_owned.unwrap_or(0))
            as usize;
        let pending = normalize_pending_orders(&profile.pending_orders);
        if pending.is_empty() {
            continue;
        }
        let mut kept_orders = Vec::<FleetPurchaseOrderState>::new();
        let mut delivered_units = 0usize;
        for order in pending {
            let eta = order.eta_at_tick_s.unwrap_or(f64::INFINITY);
            if eta.is_finite() && eta <= now_tick_s + 1e-6 {
                delivered_units = delivered_units.saturating_add(order.units as usize);
            } else {
                kept_orders.push(order);
            }
        }
        if delivered_units == 0 {
            continue;
        }
        delivered_total = delivered_total.saturating_add(delivered_units);
        let next_units_owned = current_units_owned.saturating_add(delivered_units);
        for service_index in service_indexes {
            let Some(service) = scenario.world.services.get_mut(*service_index) else {
                continue;
            };
            let mut service_profile = service.rolling_stock_profile.clone().unwrap_or_default();
            service_profile.units_owned = Some(next_units_owned as u32);
            service_profile.pending_orders = kept_orders
                .iter()
                .map(purchase_order_from_state)
                .collect::<Vec<_>>();
            service.stock_units_owned = Some(next_units_owned as u32);
            service.stock_units_assigned = Some(next_units_owned as u32);
            service.rolling_stock_profile = Some(service_profile);
        }
    }
    delivered_total
}
