use std::collections::{HashMap, HashSet};

use interlinked_engine::model::{Link, PurchaseOrder, Scenario, Service, Stop};
use interlinked_engine::platform::{from_base_currency, EconomyConfig};
use interlinked_engine::sim::SimulationOutput;
use serde::{Deserialize, Serialize};

use crate::{ProjectManifest, ScenarioDocumentLite, SessionKind};

const AUTO_REVERSE_SERVICE_PREFIX: &str = "auto_reverse::";
const AUTO_REVERSE_LINK_PREFIX: &str = "auto_reverse_link::";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingStockTierPreset {
    pub id: String,
    pub label: String,
    pub purchase_cost_multiplier: f64,
    pub capacity_multiplier: f64,
    pub speed_multiplier: f64,
    pub ui_badge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedLevelPreset {
    pub id: String,
    pub label: String,
    pub speed_multiplier: f64,
    pub cost_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortLevelPreset {
    pub id: String,
    pub label: String,
    pub demand_multiplier: f64,
    pub cost_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeBuildPreset {
    pub id: String,
    pub label: String,
    pub engine_mode: String,
    pub mode_variant: Option<String>,
    pub default_color: String,
    pub default_speed_mps: f64,
    pub default_headway_s: f64,
    pub default_dwell_s: f64,
    pub default_vehicle_capacity: f64,
    pub default_stop_type: String,
    pub base_unit_purchase_cost_base: f64,
    pub tph_min: f64,
    pub tph_max: f64,
    pub tph_step: f64,
    pub salvage_rate: f64,
    pub transfer_fee_per_unit_base: f64,
    pub tiers: Vec<RollingStockTierPreset>,
    pub package_options: Vec<RollingStockTierPreset>,
    pub supports_carriages: bool,
    pub cars_min: u32,
    pub cars_max: u32,
    pub cars_default: u32,
    pub speed_levels: Vec<SpeedLevelPreset>,
    pub comfort_levels: Vec<ComfortLevelPreset>,
    pub staff_cost_per_unit_hour_base: f64,
    pub staff_shift_multiplier_peak: f64,
    pub staff_shift_multiplier_overnight: f64,
    pub capex_per_km_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildDefaults {
    pub station_capex_base: f64,
    pub default_interchange_walk_time_s: f64,
    pub presets: Vec<ModeBuildPreset>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationJourneyTime {
    pub stop_id: String,
    pub stop_name: String,
    pub travel_time_s: f64,
    pub stops_away: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationLineSummary {
    pub line_id: String,
    pub line_name: String,
    pub mode: String,
    pub mode_variant: Option<String>,
    pub display_color: Option<String>,
    pub station_index: usize,
    pub station_count: usize,
    pub previous_station_name: Option<String>,
    pub next_station_name: Option<String>,
    pub journey_times: Vec<StationJourneyTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationInspection {
    pub stop_id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub stop_type: Option<String>,
    pub interchange_id: Option<String>,
    pub boardings_attempted: f64,
    pub boardings_served: f64,
    pub alightings_served: f64,
    pub denied_boardings: f64,
    pub queue_end: f64,
    pub station_load_current_pax: f64,
    pub station_capacity_boarding_pph: f64,
    pub station_capacity_alighting_pph: f64,
    pub station_queue_capacity_pax: f64,
    pub overflow_dropped: f64,
    pub passengers_declined_last_hour: f64,
    pub station_entries_per_hour: f64,
    pub station_exits_per_hour: f64,
    pub average_wait_to_board_s: f64,
    pub catchment_radius_m: f64,
    pub catchment_cells: usize,
    pub catchment_residents: f64,
    pub catchment_jobs: f64,
    pub catchment_mix_residential: f64,
    pub catchment_mix_office: f64,
    pub catchment_mix_retail: f64,
    pub catchment_mix_recreation: f64,
    pub catchment_mix_industrial: f64,
    pub catchment_mix_education: f64,
    pub catchment_mix_health: f64,
    pub served_lines: Vec<StationLineSummary>,
}

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

fn default_stock_tiers() -> Vec<RollingStockTierPreset> {
    vec![
        RollingStockTierPreset {
            id: "standard".to_string(),
            label: "Standard".to_string(),
            purchase_cost_multiplier: 1.0,
            capacity_multiplier: 1.0,
            speed_multiplier: 1.0,
            ui_badge: "STD".to_string(),
        },
        RollingStockTierPreset {
            id: "improved".to_string(),
            label: "Improved".to_string(),
            purchase_cost_multiplier: 1.4,
            capacity_multiplier: 1.18,
            speed_multiplier: 1.08,
            ui_badge: "IMP".to_string(),
        },
        RollingStockTierPreset {
            id: "premium".to_string(),
            label: "Premium".to_string(),
            purchase_cost_multiplier: 1.95,
            capacity_multiplier: 1.32,
            speed_multiplier: 1.15,
            ui_badge: "PRM".to_string(),
        },
    ]
}

fn default_speed_levels() -> Vec<SpeedLevelPreset> {
    vec![
        SpeedLevelPreset {
            id: "economy".to_string(),
            label: "Economy".to_string(),
            speed_multiplier: 0.92,
            cost_multiplier: 0.93,
        },
        SpeedLevelPreset {
            id: "balanced".to_string(),
            label: "Balanced".to_string(),
            speed_multiplier: 1.0,
            cost_multiplier: 1.0,
        },
        SpeedLevelPreset {
            id: "express".to_string(),
            label: "Express".to_string(),
            speed_multiplier: 1.14,
            cost_multiplier: 1.24,
        },
    ]
}

fn default_comfort_levels() -> Vec<ComfortLevelPreset> {
    vec![
        ComfortLevelPreset {
            id: "basic".to_string(),
            label: "Basic".to_string(),
            demand_multiplier: 0.96,
            cost_multiplier: 0.93,
        },
        ComfortLevelPreset {
            id: "standard".to_string(),
            label: "Standard".to_string(),
            demand_multiplier: 1.0,
            cost_multiplier: 1.0,
        },
        ComfortLevelPreset {
            id: "premium".to_string(),
            label: "Premium".to_string(),
            demand_multiplier: 1.08,
            cost_multiplier: 1.2,
        },
    ]
}

pub fn default_build_defaults(cfg: &EconomyConfig) -> BuildDefaults {
    let rail_capex = cfg
        .mode_capex_per_km_base
        .get("rail")
        .copied()
        .unwrap_or(cfg.link_capex_per_km_default_base);
    let stock_tiers = default_stock_tiers();
    let speed_levels = default_speed_levels();
    let comfort_levels = default_comfort_levels();
    BuildDefaults {
        station_capex_base: cfg.station_capex_base,
        default_interchange_walk_time_s: 90.0,
        presets: vec![
            ModeBuildPreset {
                id: "metro".to_string(),
                label: "Metro".to_string(),
                engine_mode: "metro".to_string(),
                mode_variant: None,
                default_color: "#0f5ca8".to_string(),
                default_speed_mps: 16.0,
                default_headway_s: 240.0,
                default_dwell_s: 30.0,
                default_vehicle_capacity: 650.0,
                default_stop_type: "metro_station".to_string(),
                base_unit_purchase_cost_base: 16_000_000.0,
                tph_min: 0.0,
                tph_max: 40.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 16_000_000.0 * 0.03,
                tiers: stock_tiers.clone(),
                package_options: stock_tiers.clone(),
                supports_carriages: true,
                cars_min: 4,
                cars_max: 12,
                cars_default: 8,
                speed_levels: speed_levels.clone(),
                comfort_levels: comfort_levels.clone(),
                staff_cost_per_unit_hour_base: 370.0,
                staff_shift_multiplier_peak: 1.24,
                staff_shift_multiplier_overnight: 1.46,
                capex_per_km_base: cfg
                    .mode_capex_per_km_base
                    .get("metro")
                    .copied()
                    .unwrap_or(cfg.link_capex_per_km_default_base),
            },
            ModeBuildPreset {
                id: "tram".to_string(),
                label: "Tram".to_string(),
                engine_mode: "tram".to_string(),
                mode_variant: None,
                default_color: "#e65a2b".to_string(),
                default_speed_mps: 11.0,
                default_headway_s: 300.0,
                default_dwell_s: 22.0,
                default_vehicle_capacity: 220.0,
                default_stop_type: "tram_stop".to_string(),
                base_unit_purchase_cost_base: 5_600_000.0,
                tph_min: 0.0,
                tph_max: 24.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 5_600_000.0 * 0.03,
                tiers: stock_tiers.clone(),
                package_options: stock_tiers.clone(),
                supports_carriages: true,
                cars_min: 2,
                cars_max: 6,
                cars_default: 3,
                speed_levels: speed_levels.clone(),
                comfort_levels: comfort_levels.clone(),
                staff_cost_per_unit_hour_base: 225.0,
                staff_shift_multiplier_peak: 1.22,
                staff_shift_multiplier_overnight: 1.42,
                capex_per_km_base: cfg
                    .mode_capex_per_km_base
                    .get("tram")
                    .copied()
                    .unwrap_or(cfg.link_capex_per_km_default_base),
            },
            ModeBuildPreset {
                id: "bus".to_string(),
                label: "Bus".to_string(),
                engine_mode: "bus".to_string(),
                mode_variant: None,
                default_color: "#146c58".to_string(),
                default_speed_mps: 8.5,
                default_headway_s: 420.0,
                default_dwell_s: 18.0,
                default_vehicle_capacity: 90.0,
                default_stop_type: "bus_stop".to_string(),
                base_unit_purchase_cost_base: 525_000.0,
                tph_min: 0.0,
                tph_max: 36.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 525_000.0 * 0.03,
                tiers: stock_tiers.clone(),
                package_options: stock_tiers.clone(),
                supports_carriages: false,
                cars_min: 1,
                cars_max: 1,
                cars_default: 1,
                speed_levels: speed_levels.clone(),
                comfort_levels: comfort_levels.clone(),
                staff_cost_per_unit_hour_base: 105.0,
                staff_shift_multiplier_peak: 1.18,
                staff_shift_multiplier_overnight: 1.52,
                capex_per_km_base: cfg
                    .mode_capex_per_km_base
                    .get("bus")
                    .copied()
                    .unwrap_or(0.0),
            },
            ModeBuildPreset {
                id: "ferry".to_string(),
                label: "Ferry".to_string(),
                engine_mode: "ferry".to_string(),
                mode_variant: None,
                default_color: "#2969b2".to_string(),
                default_speed_mps: 10.0,
                default_headway_s: 900.0,
                default_dwell_s: 60.0,
                default_vehicle_capacity: 260.0,
                default_stop_type: "ferry_terminal".to_string(),
                base_unit_purchase_cost_base: 11_000_000.0,
                tph_min: 0.0,
                tph_max: 10.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 11_000_000.0 * 0.03,
                tiers: stock_tiers.clone(),
                package_options: stock_tiers.clone(),
                supports_carriages: false,
                cars_min: 1,
                cars_max: 1,
                cars_default: 1,
                speed_levels: speed_levels.clone(),
                comfort_levels: comfort_levels.clone(),
                staff_cost_per_unit_hour_base: 290.0,
                staff_shift_multiplier_peak: 1.24,
                staff_shift_multiplier_overnight: 1.48,
                capex_per_km_base: cfg
                    .mode_capex_per_km_base
                    .get("ferry")
                    .copied()
                    .unwrap_or(cfg.link_capex_per_km_default_base),
            },
            ModeBuildPreset {
                id: "commuter_rail".to_string(),
                label: "Commuter Rail".to_string(),
                engine_mode: "rail".to_string(),
                mode_variant: Some("commuter_rail".to_string()),
                default_color: "#6c3bcf".to_string(),
                default_speed_mps: 24.0,
                default_headway_s: 900.0,
                default_dwell_s: 35.0,
                default_vehicle_capacity: 900.0,
                default_stop_type: "rail_station".to_string(),
                base_unit_purchase_cost_base: 26_000_000.0,
                tph_min: 0.0,
                tph_max: 16.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 26_000_000.0 * 0.03,
                tiers: stock_tiers.clone(),
                package_options: stock_tiers.clone(),
                supports_carriages: true,
                cars_min: 4,
                cars_max: 14,
                cars_default: 8,
                speed_levels: speed_levels.clone(),
                comfort_levels: comfort_levels.clone(),
                staff_cost_per_unit_hour_base: 430.0,
                staff_shift_multiplier_peak: 1.28,
                staff_shift_multiplier_overnight: 1.5,
                capex_per_km_base: rail_capex,
            },
            ModeBuildPreset {
                id: "high_speed_rail".to_string(),
                label: "High Speed Rail".to_string(),
                engine_mode: "rail".to_string(),
                mode_variant: Some("high_speed_rail".to_string()),
                default_color: "#b11f3a".to_string(),
                default_speed_mps: 55.0,
                default_headway_s: 1800.0,
                default_dwell_s: 45.0,
                default_vehicle_capacity: 1100.0,
                default_stop_type: "rail_station".to_string(),
                base_unit_purchase_cost_base: 58_000_000.0,
                tph_min: 0.0,
                tph_max: 8.0,
                tph_step: 1.0,
                salvage_rate: 0.6,
                transfer_fee_per_unit_base: 58_000_000.0 * 0.03,
                tiers: stock_tiers,
                package_options: default_stock_tiers(),
                supports_carriages: true,
                cars_min: 6,
                cars_max: 20,
                cars_default: 10,
                speed_levels,
                comfort_levels,
                staff_cost_per_unit_hour_base: 640.0,
                staff_shift_multiplier_peak: 1.3,
                staff_shift_multiplier_overnight: 1.58,
                capex_per_km_base: rail_capex,
            },
        ],
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

fn find_mode_preset<'a>(
    defaults: &'a BuildDefaults,
    mode: &str,
    mode_variant: Option<&str>,
) -> Option<&'a ModeBuildPreset> {
    defaults.presets.iter().find(|preset| {
        preset.engine_mode.eq_ignore_ascii_case(mode)
            && preset
                .mode_variant
                .as_deref()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case(mode_variant.unwrap_or_default().trim())
    })
}

fn normalize_tier_id(raw: Option<&str>) -> String {
    let fallback = "standard".to_string();
    raw.map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
}

fn resolve_tier<'a>(
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

fn resolve_speed_level<'a>(
    preset: &'a ModeBuildPreset,
    speed_level_id: Option<&str>,
) -> Option<&'a SpeedLevelPreset> {
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

fn resolve_comfort_level<'a>(
    preset: &'a ModeBuildPreset,
    comfort_level_id: Option<&str>,
) -> Option<&'a ComfortLevelPreset> {
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

fn line_schedule_from_service(service: &Service) -> LineScheduleState {
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

fn active_schedule_band(schedule: &LineScheduleState, minute_of_day: u32) -> String {
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

fn tph_for_band(schedule: &LineScheduleState, band: &str) -> f64 {
    match band {
        "peak" => schedule.tph_peak.max(0.0),
        "overnight" => schedule.tph_overnight.max(0.0),
        _ => schedule.tph_off_peak.max(0.0),
    }
}

fn normalized_cars_for_mode(preset: &ModeBuildPreset, cars_per_unit: Option<u32>) -> u32 {
    if !preset.supports_carriages {
        return 1;
    }
    let min = preset.cars_min.max(1);
    let max = preset.cars_max.max(min);
    cars_per_unit.unwrap_or(preset.cars_default).clamp(min, max)
}

fn effective_capacity_for_line(
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

fn effective_speed_for_line(
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

fn normalize_pending_orders(orders: &[PurchaseOrder]) -> Vec<FleetPurchaseOrderState> {
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

fn pending_units_for_orders(orders: &[FleetPurchaseOrderState]) -> usize {
    orders
        .iter()
        .map(|order| order.units as usize)
        .sum::<usize>()
}

fn pending_commitment_base_for_orders(
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

fn line_roll_profile(
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

fn tier_cost_base(preset: &ModeBuildPreset, tier_id: Option<&str>) -> f64 {
    let multiplier = resolve_tier(preset, tier_id)
        .map(|tier| tier.purchase_cost_multiplier.max(0.0))
        .unwrap_or(1.0);
    preset.base_unit_purchase_cost_base.max(0.0) * multiplier
}

fn tier_label(preset: &ModeBuildPreset, tier_id: Option<&str>) -> Option<String> {
    resolve_tier(preset, tier_id).map(|tier| tier.label.clone())
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
    let previous_capex_base =
        interlinked_engine::platform::estimate_network_capex_base(current, cfg);
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
    let service_opex_per_hour_base =
        interlinked_engine::platform::estimate_service_opex_per_hour_base(next, cfg);
    let projected_staff_opex_per_hour_base = estimate_staff_opex_per_hour_base(next, cfg);
    let estimated_total_opex_per_hour_base =
        service_opex_per_hour_base + projected_staff_opex_per_hour_base;
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

fn stop_display_name(stop: &Stop) -> String {
    stop.name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| stop.id.clone())
}

fn service_line_id(service: &Service) -> String {
    service
        .line_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| service.id.clone())
}

fn is_auto_reverse_service_for_line(service_id: &str, line_id: &str) -> bool {
    let prefix = format!("{AUTO_REVERSE_SERVICE_PREFIX}{line_id}::");
    service_id.starts_with(&prefix)
}

fn is_auto_reverse_link_for_line(link_id: &str, line_id: &str) -> bool {
    let prefix = format!("{AUTO_REVERSE_LINK_PREFIX}{line_id}::");
    link_id.starts_with(&prefix)
}

fn service_display_name(service: &Service) -> String {
    service
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| service.id.clone())
}

fn is_shape_stop(stop: &Stop) -> bool {
    stop.stop_type
        .as_deref()
        .map(|value| value.to_ascii_lowercase().contains("shape"))
        .unwrap_or(false)
}

fn canonical_service<'a>(services: &[&'a Service]) -> Option<&'a Service> {
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

fn target_tph_for_service(service: &Service) -> f64 {
    if let Some(tph) = service.operating_tph {
        return tph.max(0.0);
    }
    if service.headway_s > 0.0 {
        return (3600.0 / service.headway_s).max(0.0);
    }
    0.0
}

fn enabled_for_service(service: &Service, target_tph: f64) -> bool {
    service
        .service_enabled
        .unwrap_or(target_tph > 0.0 && service.headway_s < 86_399.0)
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
                (Some(from), Some(to)) => {
                    ((from.x - to.x).powi(2) + (from.y - to.y).powi(2)).sqrt()
                }
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

fn line_round_trip_seconds(line: &LineComputed) -> f64 {
    let one_way_s = line
        .station_ids
        .iter()
        .filter_map(|stop_id| line.cumulative_time_s_by_stop_id.get(stop_id).copied())
        .fold(0.0_f64, f64::max);
    (one_way_s * 2.0).max(300.0)
}

fn required_units_for_tph(round_trip_s: f64, tph: f64) -> usize {
    if round_trip_s <= 0.0 || tph <= 0.0 {
        return 0;
    }
    ((round_trip_s * tph) / 3600.0).ceil().max(0.0) as usize
}

fn window_duration_minutes(start: u32, end: u32) -> u32 {
    if start == end {
        0
    } else if start < end {
        end - start
    } else {
        (1440 - start) + end
    }
}

fn staff_opex_for_line(line: &LineComputed, preset: &ModeBuildPreset) -> f64 {
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

// Compatibility bridge: while runtime/build fully consume structured profiles, we still mirror
// legacy scalar fields so older saves/tools remain readable without behavior changes.
fn sync_legacy_fields_from_profiles(
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
        let active_band = active_schedule_band(&schedule_state, minute_of_day);
        let target_tph = tph_for_band(&schedule_state, active_band.as_str()).max(0.0);
        let round_trip_s = line_round_trip_seconds(&line);
        let units_owned = line.stock_units_owned;
        let units_assigned = line.stock_units_assigned.min(units_owned);
        let required_units = required_units_for_tph(round_trip_s, target_tph);
        let max_tph = if round_trip_s > 0.0 {
            (units_assigned as f64 * 3600.0) / round_trip_s
        } else {
            0.0
        };
        let effective_tph = if target_tph > 0.0 {
            target_tph.min(max_tph.max(0.0))
        } else {
            0.0
        };
        let enabled = effective_tph > 0.0 && units_assigned > 0 && required_units > 0;
        let headway_s = if enabled && effective_tph > 0.0 {
            (3600.0 / effective_tph).max(1.0)
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
            service.service_enabled = Some(enabled);
            service.operating_tph = Some(if enabled { effective_tph } else { 0.0 });
            service.headway_s = headway_s;
            service.vehicle_capacity = vehicle_capacity;
            sync_legacy_fields_from_profiles(
                service,
                line.stock_tier_id.as_deref().unwrap_or("standard"),
                units_owned,
                units_assigned,
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

pub fn inspect_station_from_scenario(
    scenario: &Scenario,
    output: Option<&SimulationOutput>,
    stop_id: &str,
) -> Result<StationInspection, String> {
    let stop = scenario
        .world
        .stops
        .iter()
        .find(|candidate| candidate.id == stop_id)
        .ok_or_else(|| format!("stop not found: {stop_id}"))?;

    let lines = compute_lines(scenario);
    let board_loads = output
        .map(|value| {
            value
                .board_loads
                .iter()
                .filter(|load| load.stop_id == stop_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let served_lines = lines
        .iter()
        .filter_map(|line| {
            let station_index = line
                .station_ids
                .iter()
                .position(|candidate| candidate == stop_id)?;
            let stop_lookup = scenario
                .world
                .stops
                .iter()
                .map(|entry| (entry.id.clone(), entry))
                .collect::<HashMap<_, _>>();

            let previous_station_name = station_index
                .checked_sub(1)
                .and_then(|idx| line.station_ids.get(idx))
                .and_then(|id| stop_lookup.get(id))
                .map(|stop| stop_display_name(stop));
            let next_station_name = line
                .station_ids
                .get(station_index + 1)
                .and_then(|id| stop_lookup.get(id))
                .map(|stop| stop_display_name(stop));
            let current_time = line
                .cumulative_time_s_by_stop_id
                .get(stop_id)
                .copied()
                .unwrap_or(0.0);
            let journey_times = line
                .station_ids
                .iter()
                .enumerate()
                .filter_map(|(idx, candidate)| {
                    if candidate == stop_id {
                        return None;
                    }
                    let other = stop_lookup.get(candidate)?;
                    let other_time = line
                        .cumulative_time_s_by_stop_id
                        .get(candidate)
                        .copied()
                        .unwrap_or(current_time);
                    Some(StationJourneyTime {
                        stop_id: candidate.clone(),
                        stop_name: stop_display_name(other),
                        travel_time_s: (other_time - current_time).abs(),
                        stops_away: station_index.abs_diff(idx),
                    })
                })
                .collect::<Vec<_>>();

            Some(StationLineSummary {
                line_id: line.line_id.clone(),
                line_name: line.name.clone(),
                mode: line.mode.clone(),
                mode_variant: line.mode_variant.clone(),
                display_color: line.display_color.clone(),
                station_index,
                station_count: line.station_ids.len(),
                previous_station_name,
                next_station_name,
                journey_times,
            })
        })
        .collect::<Vec<_>>();

    let catchment_radius_m = scenario.params.access_radius_m.max(0.0);
    let effective_radius = catchment_radius_m.max(1.0);
    let mut effective_catchment_radius_m = catchment_radius_m;
    let mut catchment_cells = 0usize;
    let mut catchment_residents = 0.0_f64;
    let mut catchment_jobs = 0.0_f64;
    let mut mix_weight_sum = 0.0_f64;
    let mut mix_residential_sum = 0.0_f64;
    let mut mix_office_sum = 0.0_f64;
    let mut mix_retail_sum = 0.0_f64;
    let mut mix_recreation_sum = 0.0_f64;
    let mut mix_industrial_sum = 0.0_f64;
    let mut mix_education_sum = 0.0_f64;
    let mut mix_health_sum = 0.0_f64;
    let mut selected_cells = Vec::<(usize, f64)>::new();
    for (idx, c) in scenario.world.demand_cells.iter().enumerate() {
        let dx = c.x - stop.x;
        let dy = c.y - stop.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 > effective_radius * effective_radius {
            continue;
        }
        let dist = dist2.sqrt();
        let proximity = (1.0 - dist / effective_radius).clamp(0.0, 1.0);
        if proximity <= 0.0 {
            continue;
        }
        let decay_weight = proximity * proximity;
        selected_cells.push((idx, decay_weight));
    }
    if selected_cells.is_empty() && !scenario.world.demand_cells.is_empty() {
        // Sparse legacy surfaces can leave no cells in strict access radius.
        // Use nearest-cell soft fallback rather than hard [100% residential].
        let mut nearest = scenario
            .world
            .demand_cells
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let dx = c.x - stop.x;
                let dy = c.y - stop.y;
                let dist = (dx * dx + dy * dy).sqrt();
                (idx, dist)
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let nearest_scale =
            effective_radius.max(nearest.first().map(|x| x.1).unwrap_or(1.0).max(1.0));
        for (idx, dist) in nearest.into_iter().take(24) {
            let rel = dist / nearest_scale;
            let decay_weight = 1.0 / (1.0 + rel * rel);
            if decay_weight <= 0.0 {
                continue;
            }
            effective_catchment_radius_m = effective_catchment_radius_m.max(dist);
            selected_cells.push((idx, decay_weight));
        }
    }
    for (idx, decay_weight) in selected_cells {
        let c = &scenario.world.demand_cells[idx];
        let residents = c.residents_night.max(0.0);
        let jobs = c.jobs_day.max(0.0);
        let activity_mass = (residents + jobs).max(1.0);
        let mix_weight = decay_weight * activity_mass;
        catchment_cells += 1;
        catchment_residents += residents * decay_weight;
        catchment_jobs += jobs * decay_weight;
        mix_weight_sum += mix_weight;
        mix_residential_sum += c.activity_mix_residential.max(0.0) * mix_weight;
        mix_office_sum += c.activity_mix_office.max(0.0) * mix_weight;
        mix_retail_sum += c.activity_mix_retail.max(0.0) * mix_weight;
        mix_recreation_sum += c.activity_mix_recreation.max(0.0) * mix_weight;
        mix_industrial_sum += c.activity_mix_industrial.max(0.0) * mix_weight;
        mix_education_sum += c.activity_mix_education.max(0.0) * mix_weight;
        mix_health_sum += c.activity_mix_health.max(0.0) * mix_weight;
    }
    let mut mix_values = if mix_weight_sum > 0.0 {
        [
            mix_residential_sum / mix_weight_sum,
            mix_office_sum / mix_weight_sum,
            mix_retail_sum / mix_weight_sum,
            mix_recreation_sum / mix_weight_sum,
            mix_industrial_sum / mix_weight_sum,
            mix_education_sum / mix_weight_sum,
            mix_health_sum / mix_weight_sum,
        ]
    } else {
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    };
    for v in &mut mix_values {
        if !v.is_finite() || *v < 0.0 {
            *v = 0.0;
        }
    }
    let mix_sum: f64 = mix_values.iter().sum();
    if mix_sum > 0.0 {
        for v in &mut mix_values {
            *v /= mix_sum;
        }
    } else {
        mix_values = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }

    let mut boardings_attempted = 0.0_f64;
    let mut boardings_served = 0.0_f64;
    let mut alightings_served = 0.0_f64;
    let mut denied_boardings = 0.0_f64;
    let mut queue_end = 0.0_f64;
    let mut station_load_current_pax = 0.0_f64;
    let mut station_capacity_boarding_pph = 0.0_f64;
    let mut station_capacity_alighting_pph = 0.0_f64;
    let mut station_queue_capacity_pax = 0.0_f64;
    let mut overflow_dropped = 0.0_f64;
    let mut passengers_declined_last_hour = 0.0_f64;
    let mut station_entries_per_hour = 0.0_f64;
    let mut station_exits_per_hour = 0.0_f64;
    let mut weighted_wait_sum_s = 0.0_f64;
    let mut weighted_wait_pax = 0.0_f64;

    for load in &board_loads {
        let arrivals = if load.arrivals.is_finite() {
            load.arrivals.max(0.0)
        } else {
            0.0
        };
        let served_arrivals = if load.served_from_arrivals.is_finite() {
            load.served_from_arrivals.max(0.0)
        } else {
            0.0
        };
        let served_queue = if load.served_from_queue.is_finite() {
            load.served_from_queue.max(0.0)
        } else {
            0.0
        };
        let served_total = served_arrivals + served_queue;
        let alighted = if load.alightings_served.is_finite() {
            load.alightings_served.max(0.0)
        } else {
            0.0
        };
        let denied = if load.denied_boardings.is_finite() {
            load.denied_boardings.max(0.0)
        } else {
            0.0
        };
        let queue_value = if load.queue_end.is_finite() {
            load.queue_end.max(0.0)
        } else {
            0.0
        };
        let queue_capacity = if load.station_queue_capacity_pax.is_finite() {
            load.station_queue_capacity_pax.max(0.0)
        } else {
            0.0
        };
        let overflow = if load.overflow_dropped.is_finite() {
            load.overflow_dropped.max(0.0)
        } else {
            0.0
        };
        let wait_s = if load.extra_wait_s.is_finite() {
            load.extra_wait_s.max(0.0)
        } else {
            0.0
        };
        let period_s =
            if load.departures_observed > 0 && load.headway_s.is_finite() && load.headway_s > 0.0 {
                ((load.departures_observed as f64) * load.headway_s).max(1.0)
            } else if load.departures_in_period.is_finite()
                && load.departures_in_period > 0.0
                && load.headway_s.is_finite()
                && load.headway_s > 0.0
            {
                (load.departures_in_period * load.headway_s).max(1.0)
            } else {
                300.0
            };
        let to_hour = 3600.0 / period_s.max(1.0);
        let admitted_entries = (arrivals - overflow).max(0.0);

        boardings_attempted += arrivals;
        boardings_served += served_total;
        alightings_served += alighted;
        denied_boardings += denied;
        queue_end += queue_value;
        station_load_current_pax += queue_value;
        station_capacity_boarding_pph += if load.station_capacity_boarding_pph.is_finite() {
            load.station_capacity_boarding_pph.max(0.0)
        } else {
            0.0
        };
        station_capacity_alighting_pph += if load.station_capacity_alighting_pph.is_finite() {
            load.station_capacity_alighting_pph.max(0.0)
        } else {
            0.0
        };
        station_queue_capacity_pax += queue_capacity;
        overflow_dropped += overflow;
        passengers_declined_last_hour += overflow * to_hour;
        station_entries_per_hour += admitted_entries * to_hour;
        station_exits_per_hour += alighted * to_hour;
        weighted_wait_sum_s += wait_s * served_total;
        weighted_wait_pax += served_total;
    }

    if station_queue_capacity_pax > 0.0 {
        queue_end = queue_end.min(station_queue_capacity_pax);
        station_load_current_pax = station_load_current_pax.min(station_queue_capacity_pax);
    }
    let average_wait_to_board_s = if weighted_wait_pax > 0.0 {
        weighted_wait_sum_s / weighted_wait_pax
    } else {
        0.0
    };

    Ok(StationInspection {
        stop_id: stop.id.clone(),
        name: stop_display_name(stop),
        x: stop.x,
        y: stop.y,
        stop_type: stop.stop_type.clone(),
        interchange_id: stop.interchange_id.clone(),
        boardings_attempted,
        boardings_served,
        alightings_served,
        denied_boardings,
        queue_end,
        station_load_current_pax,
        station_capacity_boarding_pph,
        station_capacity_alighting_pph,
        station_queue_capacity_pax,
        overflow_dropped,
        passengers_declined_last_hour,
        station_entries_per_hour,
        station_exits_per_hour,
        average_wait_to_board_s,
        catchment_radius_m: effective_catchment_radius_m,
        catchment_cells,
        catchment_residents,
        catchment_jobs,
        catchment_mix_residential: mix_values[0],
        catchment_mix_office: mix_values[1],
        catchment_mix_retail: mix_values[2],
        catchment_mix_recreation: mix_values[3],
        catchment_mix_industrial: mix_values[4],
        catchment_mix_education: mix_values[5],
        catchment_mix_health: mix_values[6],
        served_lines,
    })
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
            interlinked_engine::platform::estimate_service_opex_per_hour_base(
                &service_scenario,
                cfg,
            )
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
    let stock_tier_label =
        preset.and_then(|resolved| tier_label(resolved, stock_tier_id.as_deref()));
    let units_shortage_now = required_units.saturating_sub(assigned_units);
    let units_surplus_now = assigned_units.saturating_sub(required_units);
    let staff_opex_per_hour_base = preset
        .map(|resolved| staff_opex_for_line(&line, resolved))
        .unwrap_or(0.0);
    let fleet_value_base = preset
        .map(|resolved| {
            line.stock_units_owned as f64 * tier_cost_base(resolved, stock_tier_id.as_deref())
        })
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
            package_label: preset
                .and_then(|resolved| tier_label(resolved, line.stock_tier_id.as_deref())),
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

pub fn balance_display_amount(manifest: &ProjectManifest, cfg: &EconomyConfig) -> f64 {
    from_base_currency(
        manifest.economy.current_balance_base,
        &manifest.economy.currency,
        cfg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use interlinked_engine::model::{Crs, DemandCell, Meta, Params, Transfer, World, Zone};
    use interlinked_engine::sim::{
        BoardLoad, BoardingTimeBin, CitywideModeShareSummary, DemandDiagnostics, Diagnostics,
        EconomicDiagnostics, FareFlowSummary, Kpis, LinkLoad, ModalDemandDiagnostics,
        NetworkFinancialSummary, OutputMeta, ServiceReliabilityDiagnostics, SimulationOutput,
        TemporalDemandDiagnostics, TemporalDemandSlice,
    };

    fn approx(a: f64, b: f64, eps: f64) {
        assert!(
            (a - b).abs() <= eps,
            "expected approx equality: {a} vs {b} (eps {eps})"
        );
    }

    fn test_params() -> Params {
        Params {
            walk_weight: 1.0,
            wait_weight: 2.0,
            ivt_weight: 1.0,
            transfer_penalty_s: 300.0,
            access_walk_speed_mps: 1.4,
            access_radius_m: 1200.0,
            gravity_beta: 0.0003,
            trips_per_person: 1.0,
            purpose_share_home_work: 0.52,
            purpose_share_home_education: 0.12,
            purpose_share_home_retail: 0.18,
            purpose_share_home_recreation: 0.10,
            purpose_share_other: 0.08,
            attraction_weight_office: 1.0,
            attraction_weight_retail: 0.9,
            attraction_weight_recreation: 0.7,
            attraction_weight_industrial: 1.1,
            attraction_weight_education: 0.8,
            attraction_weight_health: 0.75,
            route_choice_k: 3,
            route_choice_theta: 0.002,
            assignment_max_iters: 8,
            assignment_convergence_rel: 0.01,
            capacity_enabled: true,
            queue_max_extra_wait_s: 3600.0,
            fare_enabled: true,
            fare_value_of_time_base_per_hour: 12.0,
            fare_elasticity: 0.35,
            fare_reference_base: 2.5,
            fare_transfer_window_s: 2700.0,
            fare_free_transfers_per_trip: 1,
            fare_overflow_retry_share: 0.15,
            fare_mode_bus_base: 1.8,
            fare_mode_tram_base: 2.3,
            fare_mode_metro_base: 2.7,
            fare_mode_rail_base: 3.6,
            fare_mode_ferry_base: 3.0,
            fare_mode_default_base: 2.5,
            station_capacity_scale_boarding: 1.0,
            station_capacity_scale_alighting: 1.0,
            station_queue_capacity_scale: 1.0,
            debug_sample_origin_zone: None,
            debug_sample_dest_zone: None,
            demand_profile: vec![],
            demand_purpose_profile: vec![],
        }
    }

    fn test_scenario() -> Scenario {
        Scenario {
            meta: Meta {
                name: "Builder Test".to_string(),
                seed: 7,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: test_params(),
            world: World {
                zones: vec![Zone {
                    id: "zone_a".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 2000.0,
                    jobs: 800.0,
                    country_iso2: Some("GB".to_string()),
                }],
                stops: vec![
                    Stop {
                        id: "stop_a".to_string(),
                        name: Some("Alpha".to_string()),
                        x: 0.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "stop_b".to_string(),
                        name: Some("Bravo".to_string()),
                        x: 1000.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: Some("hub-1".to_string()),
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "stop_c".to_string(),
                        name: Some("Charlie".to_string()),
                        x: 2200.0,
                        y: 0.0,
                        country_iso2: Some("GB".to_string()),
                        interchange_id: None,
                        stop_type: Some("metro_station".to_string()),
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                ],
                links: vec![
                    Link {
                        id: "l_ab".to_string(),
                        from_stop: "stop_a".to_string(),
                        to_stop: "stop_b".to_string(),
                        distance_m: 1000.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_bc".to_string(),
                        from_stop: "stop_b".to_string(),
                        to_stop: "stop_c".to_string(),
                        distance_m: 1200.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_cb".to_string(),
                        from_stop: "stop_c".to_string(),
                        to_stop: "stop_b".to_string(),
                        distance_m: 1200.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                    Link {
                        id: "l_ba".to_string(),
                        from_stop: "stop_b".to_string(),
                        to_stop: "stop_a".to_string(),
                        distance_m: 1000.0,
                        mode: "metro".to_string(),
                        speed_mps: 20.0,
                        geometry: None,
                        line_id: Some("line:test".to_string()),
                        mode_variant: None,
                        capacity_per_hour: None,
                    },
                ],
                services: vec![
                    Service {
                        id: "svc_reverse".to_string(),
                        line_id: Some("line:test".to_string()),
                        name: Some("Test Metro".to_string()),
                        mode: "metro".to_string(),
                        mode_variant: None,
                        stop_sequence: vec![
                            "stop_c".to_string(),
                            "stop_b".to_string(),
                            "stop_a".to_string(),
                        ],
                        direction: Some("reverse".to_string()),
                        direction_name: Some("Inbound".to_string()),
                        display_color: Some("#123456".to_string()),
                        service_enabled: Some(true),
                        operating_tph: Some(15.0),
                        stock_tier_id: Some("standard".to_string()),
                        stock_units_owned: Some(3),
                        stock_units_assigned: Some(3),
                        rolling_stock_profile: None,
                        schedule_profile: None,
                        headway_s: 240.0,
                        dwell_s: 30.0,
                        vehicle_capacity: 650.0,
                        board_penalty_s: None,
                    },
                    Service {
                        id: "svc_forward".to_string(),
                        line_id: Some("line:test".to_string()),
                        name: Some("Test Metro".to_string()),
                        mode: "metro".to_string(),
                        mode_variant: None,
                        stop_sequence: vec![
                            "stop_a".to_string(),
                            "stop_b".to_string(),
                            "stop_c".to_string(),
                        ],
                        direction: Some("forward".to_string()),
                        direction_name: Some("Outbound".to_string()),
                        display_color: Some("#123456".to_string()),
                        service_enabled: Some(true),
                        operating_tph: Some(15.0),
                        stock_tier_id: Some("standard".to_string()),
                        stock_units_owned: Some(3),
                        stock_units_assigned: Some(3),
                        rolling_stock_profile: None,
                        schedule_profile: None,
                        headway_s: 240.0,
                        dwell_s: 30.0,
                        vehicle_capacity: 650.0,
                        board_penalty_s: None,
                    },
                ],
                transfers: vec![Transfer {
                    from_stop: "stop_a".to_string(),
                    to_stop: "stop_b".to_string(),
                    time_s: 120.0,
                    penalty_s: None,
                    allowed_modes: None,
                }],
                transfer_rules: None,
                demand_cells: vec![
                    DemandCell {
                        cell_id: "dc_b_core".to_string(),
                        x: 1000.0,
                        y: 0.0,
                        area_m2: 350_000.0,
                        residents_night: 1800.0,
                        jobs_day: 900.0,
                        activity_mix_residential: 0.55,
                        activity_mix_office: 0.22,
                        activity_mix_retail: 0.11,
                        activity_mix_recreation: 0.05,
                        activity_mix_industrial: 0.04,
                        activity_mix_education: 0.02,
                        activity_mix_health: 0.01,
                        centrality_score: 0.7,
                        data_quality_score: 0.9,
                        country_iso2: Some("GB".to_string()),
                    },
                    DemandCell {
                        cell_id: "dc_b_east".to_string(),
                        x: 1450.0,
                        y: 40.0,
                        area_m2: 300_000.0,
                        residents_night: 900.0,
                        jobs_day: 1400.0,
                        activity_mix_residential: 0.25,
                        activity_mix_office: 0.30,
                        activity_mix_retail: 0.20,
                        activity_mix_recreation: 0.10,
                        activity_mix_industrial: 0.08,
                        activity_mix_education: 0.04,
                        activity_mix_health: 0.03,
                        centrality_score: 0.75,
                        data_quality_score: 0.9,
                        country_iso2: Some("GB".to_string()),
                    },
                ],
                demand_meta: None,
            },
        }
    }

    fn test_output() -> SimulationOutput {
        SimulationOutput {
            meta: OutputMeta {
                results_version: "test".to_string(),
                scenario_name: "Builder Test".to_string(),
                seed: 7,
                time_period_hours: 1.0,
            },
            kpis: Kpis {
                total_trips_attempted: 100.0,
                total_trips_served: 90.0,
                share_trips_served: 0.9,
                total_trips: 90.0,
                mean_generalized_cost_s: 600.0,
                mean_in_vehicle_time_s: 240.0,
                mean_wait_time_s: 120.0,
                mean_walk_time_s: 80.0,
                mean_transfer_time_s: 0.0,
                mean_transfer_penalty_s: 0.0,
                mean_transfers: 0.0,
                mean_boardings: 1.0,
                total_boardings_attempted: 90.0,
                total_boardings_served: 88.0,
                total_boardings_denied: 2.0,
                share_boardings_served: 88.0 / 90.0,
                total_fare_revenue_base: 140.0,
                total_overflow_dropped: 1.0,
                share_demand_overflow_dropped: 1.0 / 90.0,
            },
            link_loads: vec![
                LinkLoad {
                    link_id: "l_ab".to_string(),
                    from_stop: "stop_a".to_string(),
                    to_stop: "stop_b".to_string(),
                    mode: "metro".to_string(),
                    passengers: 120.0,
                    capacity_per_hour: None,
                    capacity_in_period: 160.0,
                    load_to_capacity: 0.75,
                    crowding_penalty_s: 0.0,
                },
                LinkLoad {
                    link_id: "l_bc".to_string(),
                    from_stop: "stop_b".to_string(),
                    to_stop: "stop_c".to_string(),
                    mode: "metro".to_string(),
                    passengers: 80.0,
                    capacity_per_hour: None,
                    capacity_in_period: 160.0,
                    load_to_capacity: 0.5,
                    crowding_penalty_s: 0.0,
                },
            ],
            board_loads: vec![BoardLoad {
                service_id: "svc_forward".to_string(),
                stop_id: "stop_b".to_string(),
                arrivals: 45.0,
                served_from_arrivals: 40.0,
                served_from_queue: 3.0,
                denied_boardings: 2.0,
                queue_start: 4.0,
                queue_end: 6.0,
                headway_s: 240.0,
                vehicle_capacity: 650.0,
                departures_in_period: 15.0,
                departures_observed: 1,
                capacity_in_period: 9750.0,
                extra_wait_s: 15.0,
                time_bins: vec![BoardingTimeBin {
                    bin_index: 0,
                    arrivals: 45.0,
                    served: 43.0,
                    queue_end: 6.0,
                    departures: 1,
                    capacity: 650.0,
                }],
                time_to_next_departure_s_end: 120.0,
                alightings_served: 21.0,
                station_capacity_boarding_pph: 20_000.0,
                station_capacity_alighting_pph: 22_000.0,
                station_queue_capacity_pax: 3500.0,
                overflow_dropped: 1.0,
            }],
            stop_flows: vec![],
            passenger_cohorts: vec![],
            fare_flow: FareFlowSummary::default(),
            zone_demand_profiles: vec![],
            latent_od_demand: vec![],
            assigned_od_flows: vec![],
            mode_choice_results: vec![],
            stop_flow_states: vec![],
            vehicle_load_states: vec![],
            service_operation_states: vec![],
            stop_operation_states: vec![],
            transfer_operation_metrics: vec![],
            service_reliability_diagnostics: ServiceReliabilityDiagnostics::default(),
            synthetic_economy_config: None,
            zone_demand_layer: vec![],
            zone_economic_geography_layer: vec![],
            zone_demand_production_layer: vec![],
            zone_demand_attraction_layer: vec![],
            corridor_desire_lines: vec![],
            service_gap_layer: vec![],
            service_load_layer: vec![],
            planning_overlay_config: None,
            zone_planning_metrics: vec![],
            station_planning_metrics: vec![],
            corridor_planning_metrics: vec![],
            line_service_planning_metrics: vec![],
            network_financial_summary: NetworkFinancialSummary::default(),
            service_financial_metrics: vec![],
            corridor_financial_metrics: vec![],
            station_financial_context: vec![],
            zone_mode_share_metrics: vec![],
            corridor_mode_share_metrics: vec![],
            station_transit_capture_context: vec![],
            service_transit_capture_context: vec![],
            citywide_mode_share_summary: CitywideModeShareSummary::default(),
            build_preview_metrics: vec![],
            service_gap_rankings: interlinked_engine::sim::ServiceGapRankings::default(),
            planning_debug_summary: interlinked_engine::sim::PlanningDebugSummary::default(),
            demand_diagnostics: DemandDiagnostics::default(),
            active_temporal_slice: TemporalDemandSlice::default(),
            temporal_planning_snapshots: vec![],
            temporal_demand_diagnostics: TemporalDemandDiagnostics::default(),
            modal_demand_diagnostics: ModalDemandDiagnostics::default(),
            economic_diagnostics: EconomicDiagnostics::default(),
            diagnostics: Diagnostics {
                zones: 1,
                stops: 3,
                links: 4,
                services: 2,
                transfers: 1,
                access_edges: 0,
                egress_edges: 0,
                msa_iterations: 1,
                msa_final_max_rel_change: 0.0,
                sample_paths: vec![],
            },
        }
    }

    fn test_manifest(balance_base: f64) -> ProjectManifest {
        ProjectManifest {
            project_id: "p1".to_string(),
            name: "Builder Test".to_string(),
            created_at: "0".to_string(),
            updated_at: "0".to_string(),
            session_kind: SessionKind::Game,
            engine_schema_version: 1,
            ui_schema_version: 1,
            last_opened_run_id: None,
            recent_runs: vec![],
            clock_state: crate::SimulationClock {
                sim_datetime_utc: "2026-01-01T08:00:00Z".to_string(),
                tick_seconds: 0.0,
                running: false,
                speed: 1,
            },
            progress_metrics: Some(crate::GameProgressMetrics {
                budget: balance_base,
                currency: "GBP".to_string(),
                ridership: 0.0,
                coverage: 0.0,
                milestones: 0,
            }),
            start_location: None,
            economy: crate::EconomyManifest {
                currency: "GBP".to_string(),
                difficulty: "standard".to_string(),
                difficulty_profile: crate::DifficultyProfile {
                    profile_id: "standard".to_string(),
                    demand_mult: 1.0,
                    capex_mult: 1.0,
                    opex_mult: 1.0,
                    maintenance_mult: 1.0,
                    penalty_mult: 1.0,
                    ancillary_revenue_mult: 1.0,
                    unlock_cost_mult: 1.0,
                },
                economy_revision: 1,
                starting_budget_base: balance_base,
                current_balance_base: balance_base,
                cumulative_capex_base: 0.0,
                cumulative_opex_base: 0.0,
                cumulative_revenue_base: 0.0,
                cumulative_lost_demand_penalty_base: 0.0,
                fare_revenue_deferred_base: 0.0,
                fare_boardings_deferred_pax: 0.0,
                fare_policy: crate::default_fare_policy_manifest(),
                unlocked_countries: vec!["GB".to_string()],
                region_ledger: std::collections::BTreeMap::new(),
                maintenance_rate: crate::default_maintenance_rate(),
                ancillary_revenue_rate: crate::default_ancillary_revenue_rate(),
                quality_penalty_rates: crate::default_quality_penalty_rates(),
                monthly_financials: Vec::new(),
            },
            demand_surface: None,
            region_state: crate::RegionStateManifest::default(),
            simulation_scope: crate::default_simulation_scope_manifest(),
            runtime_scheduling: crate::default_runtime_scheduling_manifest(),
            pack_refs: vec![],
        }
    }

    #[test]
    fn compute_lines_uses_forward_service_for_station_order() {
        let scenario = test_scenario();
        let lines = compute_lines(&scenario);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.line_id, "line:test");
        assert_eq!(
            line.station_ids,
            vec![
                "stop_a".to_string(),
                "stop_b".to_string(),
                "stop_c".to_string()
            ]
        );
        assert_eq!(line.directions.len(), 2);
        assert_eq!(line.display_color.as_deref(), Some("#123456"));
        approx(
            *line
                .cumulative_time_s_by_stop_id
                .get("stop_c")
                .expect("stop_c travel time"),
            170.0,
            1e-6,
        );
    }

    #[test]
    fn station_inspection_reports_zero_based_position_and_live_metrics() {
        let scenario = test_scenario();
        let output = test_output();
        let inspection = inspect_station_from_scenario(&scenario, Some(&output), "stop_b")
            .expect("station inspection");
        assert_eq!(inspection.name, "Bravo");
        approx(inspection.boardings_attempted, 45.0, 1e-6);
        approx(inspection.boardings_served, 43.0, 1e-6);
        approx(inspection.denied_boardings, 2.0, 1e-6);
        approx(inspection.queue_end, 6.0, 1e-6);
        assert_eq!(inspection.served_lines.len(), 1);
        let line = &inspection.served_lines[0];
        assert_eq!(line.station_index, 1);
        assert_eq!(line.station_count, 3);
        assert_eq!(line.previous_station_name.as_deref(), Some("Alpha"));
        assert_eq!(line.next_station_name.as_deref(), Some("Charlie"));
        assert_eq!(line.journey_times.len(), 2);
        approx(line.journey_times[0].travel_time_s, 80.0, 1e-6);
        assert!(inspection.catchment_cells >= 1);
        assert!(inspection.catchment_residents > 0.0);
        assert!(inspection.catchment_jobs > 0.0);
        let catchment_mix_sum = inspection.catchment_mix_residential
            + inspection.catchment_mix_office
            + inspection.catchment_mix_retail
            + inspection.catchment_mix_recreation
            + inspection.catchment_mix_industrial
            + inspection.catchment_mix_education
            + inspection.catchment_mix_health;
        assert!((catchment_mix_sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn station_inspection_uses_nearest_cells_when_radius_is_empty() {
        let mut scenario = test_scenario();
        scenario.params.access_radius_m = 120.0;
        for c in &mut scenario.world.demand_cells {
            c.x += 40_000.0;
            c.y += 40_000.0;
        }
        let inspection =
            inspect_station_from_scenario(&scenario, None, "stop_b").expect("station inspection");
        assert!(inspection.catchment_cells > 0);
        assert!(inspection.catchment_mix_residential < 0.9);
        let non_residential = inspection.catchment_mix_office
            + inspection.catchment_mix_retail
            + inspection.catchment_mix_recreation
            + inspection.catchment_mix_industrial
            + inspection.catchment_mix_education
            + inspection.catchment_mix_health;
        assert!(non_residential > 0.1);
    }

    #[test]
    fn line_inspection_sums_line_loads_and_estimates_opex() {
        let scenario = test_scenario();
        let output = test_output();
        let cfg = interlinked_engine::platform::default_economy_config();
        let inspection =
            inspect_line_from_scenario(&scenario, Some(&output), "line:test", &cfg, Some(540))
                .expect("line inspection");
        assert_eq!(inspection.station_count, 3);
        assert_eq!(inspection.service_count, 2);
        approx(inspection.total_passengers, 200.0, 1e-6);
        assert!(!inspection.stations.is_empty());
        assert!(inspection.estimated_opex_per_hour_base > 0.0);
    }

    #[test]
    fn mutation_summary_charges_only_positive_delta() {
        let current = test_scenario();
        let mut expanded = current.clone();
        expanded.world.stops.push(Stop {
            id: "stop_d".to_string(),
            name: Some("Delta".to_string()),
            x: 3200.0,
            y: 0.0,
            country_iso2: Some("GB".to_string()),
            interchange_id: None,
            stop_type: Some("metro_station".to_string()),
            station_boarding_capacity_pph: None,
            station_alighting_capacity_pph: None,
            station_queue_capacity_pax: None,
        });
        expanded.world.links.push(Link {
            id: "l_cd".to_string(),
            from_stop: "stop_c".to_string(),
            to_stop: "stop_d".to_string(),
            distance_m: 1000.0,
            mode: "metro".to_string(),
            speed_mps: 20.0,
            geometry: None,
            line_id: Some("line:test".to_string()),
            mode_variant: None,
            capacity_per_hour: None,
        });
        let cfg = interlinked_engine::platform::default_economy_config();
        let expanded_summary = summarize_network_mutation(&current, &expanded, &cfg, None);
        assert!(expanded_summary.capex_delta_base > 0.0);

        let reduced_summary = summarize_network_mutation(&expanded, &current, &cfg, None);
        approx(reduced_summary.capex_delta_base, 0.0, 1e-6);
    }

    #[test]
    fn apply_build_budget_updates_balance_and_rejects_overspend() {
        let cfg = interlinked_engine::platform::default_economy_config();
        let mut manifest = test_manifest(1_000_000.0);
        let summary = NetworkMutationSummary {
            capex_delta_base: 250_000.0,
            infra_capex_delta_base: 250_000.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 250_000.0,
            construction_cost_delta_base: 250_000.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 250_000.0,
            projected_balance_after_apply_base: Some(750_000.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        apply_build_budget(&mut manifest, &cfg, &summary, None).expect("budget application");
        approx(manifest.economy.current_balance_base, 750_000.0, 1e-6);
        approx(manifest.economy.cumulative_capex_base, 250_000.0, 1e-6);

        let mut poor_manifest = test_manifest(100.0);
        let poor_summary = NetworkMutationSummary {
            capex_delta_base: 200.0,
            infra_capex_delta_base: 200.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 200.0,
            construction_cost_delta_base: 200.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 200.0,
            projected_balance_after_apply_base: Some(-100.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        let error = apply_build_budget(&mut poor_manifest, &cfg, &poor_summary, None)
            .expect_err("overspend should fail");
        assert!(error.contains("Insufficient funds"));
        approx(poor_manifest.economy.current_balance_base, 100.0, 1e-6);
    }

    #[test]
    fn apply_build_budget_ignores_zero_override_and_uses_summary_delta() {
        let cfg = interlinked_engine::platform::default_economy_config();
        let mut manifest = test_manifest(1_000_000.0);
        let summary = NetworkMutationSummary {
            capex_delta_base: 180_000.0,
            infra_capex_delta_base: 180_000.0,
            fleet_purchase_base: 0.0,
            fleet_upgrade_base: 0.0,
            fleet_transfer_fees_base: 0.0,
            fleet_salvage_refund_base: 0.0,
            net_capex_delta_base: 180_000.0,
            construction_cost_delta_base: 180_000.0,
            fleet_purchase_delta_base: 0.0,
            fleet_configuration_delta_base: 0.0,
            apply_total_delta_base: 180_000.0,
            projected_balance_after_apply_base: Some(820_000.0),
            projected_opex_per_hour_base: 0.0,
            projected_staff_opex_per_hour_base: 0.0,
            estimated_total_capex_base: 0.0,
            estimated_total_opex_per_hour_base: 0.0,
        };
        apply_build_budget(&mut manifest, &cfg, &summary, Some(0.0))
            .expect("zero override should not suppress capex");
        approx(manifest.economy.current_balance_base, 820_000.0, 1e-6);
        approx(manifest.economy.cumulative_capex_base, 180_000.0, 1e-6);
    }

    #[test]
    fn mutation_summary_charges_pending_order_commitment() {
        let current = test_scenario();
        let mut next = current.clone();
        for service in &mut next.world.services {
            service.rolling_stock_profile = Some(interlinked_engine::model::RollingStockProfile {
                package_id: Some("standard".to_string()),
                units_owned: Some(3),
                cars_per_unit: Some(8),
                speed_level: Some("balanced".to_string()),
                comfort_level: Some("standard".to_string()),
                pending_orders: vec![interlinked_engine::model::PurchaseOrder {
                    order_id: "po:test".to_string(),
                    units: 2,
                    status: Some("pending".to_string()),
                    unit_cost_base: Some(0.0),
                    total_cost_base: Some(1_750_000.0),
                    placed_at_tick_s: Some(0.0),
                    eta_at_tick_s: Some(21_600.0),
                }],
            });
        }
        let cfg = interlinked_engine::platform::default_economy_config();
        let summary = summarize_network_mutation(&current, &next, &cfg, None);
        approx(summary.fleet_purchase_base, 1_750_000.0, 1e-6);
        approx(
            summary.apply_total_delta_base,
            summary.net_capex_delta_base,
            1e-6,
        );
        assert!(summary.apply_total_delta_base >= 1_750_000.0);
    }

    #[test]
    fn settles_due_pending_orders_into_owned_units() {
        let mut scenario = test_scenario();
        for service in &mut scenario.world.services {
            service.stock_units_owned = Some(2);
            service.stock_units_assigned = Some(2);
            service.rolling_stock_profile = Some(interlinked_engine::model::RollingStockProfile {
                package_id: Some("standard".to_string()),
                units_owned: Some(2),
                cars_per_unit: Some(8),
                speed_level: Some("balanced".to_string()),
                comfort_level: Some("standard".to_string()),
                pending_orders: vec![interlinked_engine::model::PurchaseOrder {
                    order_id: "po:deliver".to_string(),
                    units: 3,
                    status: Some("pending".to_string()),
                    unit_cost_base: Some(500_000.0),
                    total_cost_base: Some(1_500_000.0),
                    placed_at_tick_s: Some(300.0),
                    eta_at_tick_s: Some(1200.0),
                }],
            });
        }

        let before_eta = settle_pending_purchase_orders(&mut scenario, 900.0);
        assert_eq!(before_eta, 0);
        assert_eq!(scenario.world.services[0].stock_units_owned, Some(2));
        assert_eq!(scenario.world.services[0].stock_units_assigned, Some(2));

        let delivered = settle_pending_purchase_orders(&mut scenario, 1200.0);
        assert_eq!(delivered, 3);
        for service in &scenario.world.services {
            assert_eq!(service.stock_units_owned, Some(5));
            assert_eq!(service.stock_units_assigned, Some(5));
            assert_eq!(
                service
                    .rolling_stock_profile
                    .as_ref()
                    .map(|profile| profile.pending_orders.len())
                    .unwrap_or_default(),
                0
            );
        }
    }
}
