use interlinked_engine::platform::EconomyConfig;
use serde::{Deserialize, Serialize};

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

pub(super) fn find_mode_preset<'a>(
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
