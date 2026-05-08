use interlinked_engine::model::Scenario;
use interlinked_engine::platform::EconomyConfig;

use super::defaults::{default_build_defaults, find_mode_preset};
use super::fleet_state::{
    effective_capacity_for_line, effective_speed_for_line, line_activation_diagnostics,
    service_line_id, sync_legacy_fields_from_profiles,
};
use super::inspection_line::{compute_lines, line_round_trip_seconds, staff_opex_for_line};

pub fn materialize_line_operations_for_minute(
    scenario: &mut Scenario,
    cfg: &EconomyConfig,
    minute_of_day: u32,
) {
    let defaults = default_build_defaults(cfg);
    let lines = compute_lines(scenario);
    for line in lines {
        let Some(preset) = find_mode_preset(&defaults, &line.mode, line.mode_variant.as_deref())
        else {
            continue;
        };
        let schedule_state = line.schedule_state.clone();
        let round_trip_s = line_round_trip_seconds(&line);
        let units_owned = line.stock_units_owned;
        let activation = line_activation_diagnostics(
            &line.mode,
            &schedule_state,
            minute_of_day,
            round_trip_s,
            units_owned,
            line.stock_units_assigned,
            None,
        );
        let headway_s = if activation.enabled && activation.effective_tph > 0.0 {
            (3600.0 / activation.effective_tph).max(1.0)
        } else {
            86_400.0
        };
        let vehicle_capacity =
            effective_capacity_for_line(preset, line.stock_tier_id.as_deref(), line.cars_per_unit);
        let speed_mps = effective_speed_for_line(
            preset,
            line.stock_tier_id.as_deref(),
            line.speed_level.as_deref(),
        );
        for service in scenario.world.services.iter_mut() {
            if service_line_id(service) != line.line_id {
                continue;
            }
            service.service_enabled = Some(activation.enabled);
            service.operating_tph = Some(if activation.enabled {
                activation.effective_tph
            } else {
                0.0
            });
            service.headway_s = headway_s;
            service.vehicle_capacity = vehicle_capacity;
            sync_legacy_fields_from_profiles(
                service,
                line.stock_tier_id.as_deref().unwrap_or("standard"),
                units_owned,
                activation.units_assigned,
                line.cars_per_unit,
                line.speed_level.clone(),
                line.comfort_level.clone(),
                &line.pending_orders,
                &schedule_state,
            );
        }
        for link in scenario.world.links.iter_mut() {
            if link.line_id.as_deref() == Some(line.line_id.as_str()) {
                link.speed_mps = speed_mps;
            }
        }
    }
}

pub fn estimate_staff_opex_per_hour_base(scenario: &Scenario, cfg: &EconomyConfig) -> f64 {
    let defaults = default_build_defaults(cfg);
    compute_lines(scenario)
        .iter()
        .filter_map(|line| {
            let preset = find_mode_preset(&defaults, &line.mode, line.mode_variant.as_deref())?;
            Some(staff_opex_for_line(line, preset))
        })
        .sum()
}
