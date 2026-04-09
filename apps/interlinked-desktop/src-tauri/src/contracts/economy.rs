use crate::*;

use super::session::DifficultyProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyManifest {
    #[serde(default = "default_currency_code")]
    pub currency: String,
    #[serde(default = "default_difficulty_label")]
    pub difficulty: String,
    pub starting_budget_base: f64,
    pub current_balance_base: f64,
    pub cumulative_capex_base: f64,
    pub cumulative_opex_base: f64,
    #[serde(default)]
    pub cumulative_revenue_base: f64,
    #[serde(default)]
    pub cumulative_lost_demand_penalty_base: f64,
    #[serde(default)]
    pub difficulty_profile: DifficultyProfile,
    #[serde(default = "default_economy_revision")]
    pub economy_revision: u64,
    #[serde(default)]
    pub fare_revenue_deferred_base: f64,
    #[serde(default)]
    pub fare_boardings_deferred_pax: f64,
    #[serde(default = "default_fare_policy_manifest")]
    pub fare_policy: FarePolicyManifest,
    #[serde(default)]
    pub unlocked_countries: Vec<String>,
    #[serde(default)]
    pub region_ledger: BTreeMap<String, RegionEconomyLedger>,
    #[serde(default = "default_maintenance_rate")]
    pub maintenance_rate: f64,
    #[serde(default = "default_ancillary_revenue_rate")]
    pub ancillary_revenue_rate: f64,
    #[serde(default = "default_quality_penalty_rates")]
    pub quality_penalty_rates: QualityPenaltyRates,
    #[serde(default)]
    pub monthly_financials: Vec<MonthlyFinancialSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegionEconomyLedger {
    #[serde(default)]
    pub revenue_base: f64,
    #[serde(default)]
    pub opex_base: f64,
    #[serde(default)]
    pub capex_base: f64,
    #[serde(default)]
    pub penalties_base: f64,
    #[serde(default)]
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPenaltyRates {
    #[serde(default = "default_overcrowding_penalty_rate")]
    pub overcrowding_base_per_passenger: f64,
    #[serde(default = "default_reliability_penalty_rate")]
    pub reliability_base_per_passenger: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyFinancialSnapshot {
    pub month_index: u64,
    #[serde(default)]
    pub revenue_base: f64,
    #[serde(default)]
    pub opex_base: f64,
    #[serde(default)]
    pub capex_base: f64,
    #[serde(default)]
    pub penalties_base: f64,
    #[serde(default)]
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarePolicyManifest {
    #[serde(default = "default_fare_enabled_manifest")]
    pub enabled: bool,
    #[serde(default = "default_fare_mode_bus_base")]
    pub fare_mode_bus_base: f64,
    #[serde(default = "default_fare_mode_tram_base")]
    pub fare_mode_tram_base: f64,
    #[serde(default = "default_fare_mode_metro_base")]
    pub fare_mode_metro_base: f64,
    #[serde(default = "default_fare_mode_rail_base")]
    pub fare_mode_rail_base: f64,
    #[serde(default = "default_fare_mode_ferry_base")]
    pub fare_mode_ferry_base: f64,
    #[serde(default = "default_fare_mode_default_base")]
    pub fare_mode_default_base: f64,
    #[serde(default = "default_fare_transfer_window_s")]
    pub transfer_window_s: f64,
    #[serde(default = "default_fare_free_transfers_per_trip")]
    pub free_transfers_per_trip: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FarePolicyPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub fare_mode_bus_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_tram_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_metro_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_rail_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_ferry_base: Option<f64>,
    #[serde(default)]
    pub fare_mode_default_base: Option<f64>,
    #[serde(default)]
    pub transfer_window_s: Option<f64>,
    #[serde(default)]
    pub free_transfers_per_trip: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinancialDashboardRequest {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub line_id: Option<String>,
    #[serde(default)]
    pub region_id: Option<String>,
    #[serde(default)]
    pub granularity: Option<String>,
    #[serde(default)]
    pub periods: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialDashboardPoint {
    pub period_index: i64,
    pub label: String,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialModeBreakdownRow {
    pub mode: String,
    pub lines: usize,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialLineBreakdownRow {
    pub line_id: String,
    pub line_name: String,
    pub mode: String,
    pub region_id: Option<String>,
    pub estimated_capex_base: f64,
    pub estimated_opex_per_hour_base: f64,
    pub staff_opex_per_hour_base: f64,
    pub fleet_value_base: f64,
    pub units_owned: usize,
    pub units_pending: usize,
    pub units_assigned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialRegionBreakdownRow {
    pub region_id: String,
    pub revenue_base: f64,
    pub opex_base: f64,
    pub capex_base: f64,
    pub penalties_base: f64,
    pub net_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialDashboardResponse {
    pub currency: String,
    pub granularity: String,
    pub periods: usize,
    pub current_balance_base: f64,
    pub total_revenue_base: f64,
    pub total_opex_base: f64,
    pub total_capex_base: f64,
    pub total_penalties_base: f64,
    pub total_net_base: f64,
    pub points: Vec<FinancialDashboardPoint>,
    pub mode_breakdown: Vec<FinancialModeBreakdownRow>,
    pub line_breakdown: Vec<FinancialLineBreakdownRow>,
    pub region_breakdown: Vec<FinancialRegionBreakdownRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandLayerStats {
    pub cells: usize,
    pub residents_min: f64,
    pub residents_p50: f64,
    pub residents_max: f64,
    pub jobs_min: f64,
    pub jobs_p50: f64,
    pub jobs_max: f64,
    pub activity_min: f64,
    pub activity_p50: f64,
    pub activity_max: f64,
}
