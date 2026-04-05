use std::collections::{HashMap, HashSet};

use interlinked_engine::model::Scenario;
use interlinked_engine::platform::{from_base_currency, EconomyConfig};
use serde::{Deserialize, Serialize};

use crate::{ProjectManifest, ScenarioDocumentLite, SessionKind};

use super::defaults::{default_build_defaults, find_mode_preset, BuildDefaults};
use super::fleet_state::{
    normalize_tier_id, pending_commitment_base_for_orders, resolve_comfort_level,
    resolve_speed_level, tier_cost_base,
};
use super::inspection_line::compute_lines;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMutationSummary {
    // Legacy fields kept for compatibility.
    pub capex_delta_base: f64,
    pub infra_capex_delta_base: f64,
    pub fleet_purchase_base: f64,
    pub fleet_upgrade_base: f64,
    pub fleet_transfer_fees_base: f64,
    pub fleet_salvage_refund_base: f64,
    pub net_capex_delta_base: f64,
    // New player-facing cost fields.
    pub construction_cost_delta_base: f64,
    pub fleet_purchase_delta_base: f64,
    pub fleet_configuration_delta_base: f64,
    pub apply_total_delta_base: f64,
    pub projected_balance_after_apply_base: Option<f64>,
    pub projected_opex_per_hour_base: f64,
    pub projected_staff_opex_per_hour_base: f64,
    pub estimated_total_capex_base: f64,
    pub estimated_total_opex_per_hour_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationCostBreakdown {
    pub construction_base: f64,
    pub fleet_purchase_base: f64,
    pub fleet_configuration_base: f64,
    pub fleet_transfer_fees_base: f64,
    pub fleet_salvage_refund_base: f64,
    pub apply_total_base: f64,
    pub projected_balance_after_apply_base: Option<f64>,
    pub projected_opex_per_hour_base: f64,
    pub projected_staff_opex_per_hour_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MutationPathValidationMeta {
    pub path_validation_mode: String,
    pub road_snap_tolerance_m: f64,
    pub water_path_tolerance_m: f64,
    pub water_terminal_tolerance_m: f64,
    pub changed_links_checked: usize,
    pub changed_stops_checked: usize,
    pub bus_links_checked: usize,
    pub bus_links_invalid: usize,
    pub ferry_links_checked: usize,
    pub ferry_links_invalid: usize,
    pub road_stops_checked: usize,
    pub road_stops_invalid: usize,
    pub water_stops_checked: usize,
    pub water_stops_invalid: usize,
    pub locked_county_hits: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMutationResult {
    pub scenario: ScenarioDocumentLite,
    pub manifest: ProjectManifest,
    pub summary: NetworkMutationSummary,
    pub cost_breakdown: MutationCostBreakdown,
    #[serde(default)]
    pub path_validation: MutationPathValidationMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMutationPreviewResult {
    pub summary: NetworkMutationSummary,
    pub cost_breakdown: MutationCostBreakdown,
    #[serde(default)]
    pub path_validation: MutationPathValidationMeta,
}

pub fn mutation_cost_breakdown(summary: &NetworkMutationSummary) -> MutationCostBreakdown {
    MutationCostBreakdown {
        construction_base: summary.construction_cost_delta_base,
        fleet_purchase_base: summary.fleet_purchase_delta_base,
        fleet_configuration_base: summary.fleet_configuration_delta_base,
        fleet_transfer_fees_base: summary.fleet_transfer_fees_base,
        fleet_salvage_refund_base: summary.fleet_salvage_refund_base,
        apply_total_base: summary.apply_total_delta_base,
        projected_balance_after_apply_base: summary.projected_balance_after_apply_base,
        projected_opex_per_hour_base: summary.projected_opex_per_hour_base,
        projected_staff_opex_per_hour_base: summary.projected_staff_opex_per_hour_base,
    }
}

#[derive(Debug, Clone)]
struct FleetLineState {
    family_key: String,
    unit_cost_base: f64,
    transfer_fee_per_unit_base: f64,
    salvage_rate: f64,
    owned_units: i64,
    pending_commitment_base: f64,
}

#[derive(Debug, Clone)]
struct FleetFlowIncrease {
    units: i64,
    unit_cost_base: f64,
}

#[derive(Debug, Clone)]
struct FleetFlowDecrease {
    units: i64,
    unit_cost_base: f64,
    salvage_rate: f64,
}

fn line_family_key(mode: &str, mode_variant: Option<&str>) -> String {
    format!(
        "{}|{}",
        mode.trim().to_ascii_lowercase(),
        mode_variant.unwrap_or_default().trim().to_ascii_lowercase()
    )
}

fn fleet_states_for_scenario(
    scenario: &Scenario,
    defaults: &BuildDefaults,
) -> HashMap<String, FleetLineState> {
    compute_lines(scenario)
        .into_iter()
        .map(|line| {
            let preset = find_mode_preset(defaults, &line.mode, line.mode_variant.as_deref());
            let tier_id = normalize_tier_id(line.stock_tier_id.as_deref());
            let unit_cost_base = preset
                .map(|resolved| {
                    let base_cost = tier_cost_base(resolved, Some(&tier_id));
                    let speed_mult = resolve_speed_level(resolved, line.speed_level.as_deref())
                        .map(|item| item.cost_multiplier.max(0.0))
                        .unwrap_or(1.0);
                    let comfort_mult =
                        resolve_comfort_level(resolved, line.comfort_level.as_deref())
                            .map(|item| item.cost_multiplier.max(0.0))
                            .unwrap_or(1.0);
                    let cars_mult = if resolved.supports_carriages {
                        let base = resolved.cars_default.max(1) as f64;
                        (line.cars_per_unit.max(1) as f64 / base).max(0.5)
                    } else {
                        1.0
                    };
                    base_cost * speed_mult * comfort_mult * cars_mult
                })
                .unwrap_or(0.0);
            let transfer_fee_per_unit_base = preset
                .map(|resolved| resolved.transfer_fee_per_unit_base.max(0.0))
                .unwrap_or(0.0);
            let salvage_rate = preset
                .map(|resolved| resolved.salvage_rate.clamp(0.0, 1.0))
                .unwrap_or(0.6);
            (
                line.line_id.clone(),
                FleetLineState {
                    family_key: line_family_key(&line.mode, line.mode_variant.as_deref()),
                    unit_cost_base,
                    transfer_fee_per_unit_base,
                    salvage_rate,
                    owned_units: line.stock_units_owned as i64,
                    pending_commitment_base: pending_commitment_base_for_orders(
                        &line.pending_orders,
                        unit_cost_base,
                    ),
                },
            )
        })
        .collect::<HashMap<_, _>>()
}

pub fn summarize_network_mutation(
    current: &Scenario,
    next: &Scenario,
    cfg: &EconomyConfig,
    current_balance_base: Option<f64>,
) -> NetworkMutationSummary {
    let defaults = default_build_defaults(cfg);
    let previous_capex_base = interlinked_engine::platform::estimate_network_capex_base(current, cfg);
    let next_capex_base = interlinked_engine::platform::estimate_network_capex_base(next, cfg);
    let infra_capex_delta_base = (next_capex_base - previous_capex_base).max(0.0);

    let current_fleet = fleet_states_for_scenario(current, &defaults);
    let next_fleet = fleet_states_for_scenario(next, &defaults);
    let mut fleet_purchase_base = 0.0;
    let mut fleet_upgrade_base = 0.0;
    let mut fleet_transfer_fees_base = 0.0;
    let mut fleet_salvage_refund_base = 0.0;
    let mut pending_commitment_delta_base = 0.0;

    let mut line_ids = current_fleet.keys().cloned().collect::<HashSet<_>>();
    line_ids.extend(next_fleet.keys().cloned());

    let mut increases_by_family = HashMap::<String, Vec<FleetFlowIncrease>>::new();
    let mut decreases_by_family = HashMap::<String, Vec<FleetFlowDecrease>>::new();
    let mut transfer_fee_by_family = HashMap::<String, f64>::new();

    for line_id in line_ids {
        let current_line = current_fleet.get(&line_id);
        let next_line = next_fleet.get(&line_id);
        let family_key = next_line
            .map(|line| line.family_key.clone())
            .or_else(|| current_line.map(|line| line.family_key.clone()));
        let Some(family_key) = family_key else {
            continue;
        };
        if let Some(fee) = next_line
            .map(|line| line.transfer_fee_per_unit_base)
            .or_else(|| current_line.map(|line| line.transfer_fee_per_unit_base))
        {
            transfer_fee_by_family.insert(family_key.clone(), fee.max(0.0));
        }

        if let (Some(left), Some(right)) = (current_line, next_line) {
            if left.family_key == right.family_key {
                let overlap = left.owned_units.min(right.owned_units).max(0) as f64;
                if overlap > 0.0 && right.unit_cost_base > left.unit_cost_base {
                    fleet_upgrade_base += overlap * (right.unit_cost_base - left.unit_cost_base);
                }
            }
        }

        let current_owned = current_line.map(|line| line.owned_units).unwrap_or(0);
        let next_owned = next_line.map(|line| line.owned_units).unwrap_or(0);
        let current_pending_commitment = current_line
            .map(|line| line.pending_commitment_base.max(0.0))
            .unwrap_or(0.0);
        let next_pending_commitment = next_line
            .map(|line| line.pending_commitment_base.max(0.0))
            .unwrap_or(0.0);
        pending_commitment_delta_base += next_pending_commitment - current_pending_commitment;
        let delta = next_owned - current_owned;
        if delta > 0 {
            let unit_cost_base = next_line.map(|line| line.unit_cost_base).unwrap_or(0.0);
            increases_by_family
                .entry(family_key.clone())
                .or_default()
                .push(FleetFlowIncrease {
                    units: delta,
                    unit_cost_base,
                });
        } else if delta < 0 {
            let unit_cost_base = current_line.map(|line| line.unit_cost_base).unwrap_or(0.0);
            let salvage_rate = current_line.map(|line| line.salvage_rate).unwrap_or(0.6);
            decreases_by_family
                .entry(family_key.clone())
                .or_default()
                .push(FleetFlowDecrease {
                    units: -delta,
                    unit_cost_base,
                    salvage_rate,
                });
        }
    }

    let families = increases_by_family
        .keys()
        .chain(decreases_by_family.keys())
        .cloned()
        .collect::<HashSet<_>>();

    for family in families {
        let mut increases = increases_by_family.remove(&family).unwrap_or_default();
        let mut decreases = decreases_by_family.remove(&family).unwrap_or_default();
        let fee_per_unit = transfer_fee_by_family.get(&family).copied().unwrap_or(0.0);
        let mut inc_index = 0usize;
        let mut dec_index = 0usize;

        while inc_index < increases.len() && dec_index < decreases.len() {
            let moved = increases[inc_index]
                .units
                .min(decreases[dec_index].units)
                .max(0);
            if moved <= 0 {
                break;
            }
            increases[inc_index].units -= moved;
            decreases[dec_index].units -= moved;
            fleet_transfer_fees_base += (moved as f64) * fee_per_unit;

            if increases[inc_index].units == 0 {
                inc_index += 1;
            }
            if decreases[dec_index].units == 0 {
                dec_index += 1;
            }
        }

        fleet_purchase_base += increases
            .into_iter()
            .map(|entry| entry.units.max(0) as f64 * entry.unit_cost_base.max(0.0))
            .sum::<f64>();
        fleet_salvage_refund_base += decreases
            .into_iter()
            .map(|entry| {
                entry.units.max(0) as f64
                    * entry.unit_cost_base.max(0.0)
                    * entry.salvage_rate.clamp(0.0, 1.0)
            })
            .sum::<f64>();
    }

    if pending_commitment_delta_base > 0.0 {
        fleet_purchase_base += pending_commitment_delta_base;
    } else if pending_commitment_delta_base < 0.0 {
        fleet_salvage_refund_base += -pending_commitment_delta_base;
    }

    let net_capex_delta_base = infra_capex_delta_base
        + fleet_purchase_base
        + fleet_upgrade_base
        + fleet_transfer_fees_base
        - fleet_salvage_refund_base;
    let capex_delta_base = net_capex_delta_base.max(0.0);
    let service_opex_per_hour_base = interlinked_engine::platform::estimate_service_opex_per_hour_base(next, cfg);
    let projected_staff_opex_per_hour_base = super::operations_materialization::estimate_staff_opex_per_hour_base(next, cfg);
    let estimated_total_opex_per_hour_base = service_opex_per_hour_base + projected_staff_opex_per_hour_base;
    let projected_balance_after_apply_base =
        current_balance_base.map(|balance| balance - net_capex_delta_base);

    NetworkMutationSummary {
        capex_delta_base,
        infra_capex_delta_base,
        fleet_purchase_base,
        fleet_upgrade_base,
        fleet_transfer_fees_base,
        fleet_salvage_refund_base,
        net_capex_delta_base,
        construction_cost_delta_base: infra_capex_delta_base,
        fleet_purchase_delta_base: fleet_purchase_base,
        fleet_configuration_delta_base: fleet_upgrade_base,
        apply_total_delta_base: net_capex_delta_base,
        projected_balance_after_apply_base,
        projected_opex_per_hour_base: estimated_total_opex_per_hour_base,
        projected_staff_opex_per_hour_base,
        estimated_total_capex_base: next_capex_base,
        estimated_total_opex_per_hour_base,
    }
}

pub fn apply_build_budget(
    manifest: &mut ProjectManifest,
    cfg: &EconomyConfig,
    summary: &NetworkMutationSummary,
    capex_override_base: Option<f64>,
) -> Result<(), String> {
    let net_capex_delta_base = capex_override_base
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(summary.net_capex_delta_base);
    if manifest.session_kind == SessionKind::Game {
        if net_capex_delta_base > 0.0
            && net_capex_delta_base > manifest.economy.current_balance_base + 1e-6
        {
            let balance = balance_display_amount(manifest, cfg);
            return Err(format!(
                "Insufficient funds for build commit. Need {:.0} GBP base, balance {:.0} {}.",
                net_capex_delta_base, balance, manifest.economy.currency
            ));
        }
        manifest.economy.current_balance_base -= net_capex_delta_base;
        if net_capex_delta_base > 0.0 {
            manifest.economy.cumulative_capex_base += net_capex_delta_base;
        }
    }
    Ok(())
}

pub fn balance_display_amount(manifest: &ProjectManifest, cfg: &EconomyConfig) -> f64 {
    from_base_currency(
        manifest.economy.current_balance_base,
        &manifest.economy.currency,
        cfg,
    )
}
