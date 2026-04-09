import type { CurrencyCode, DifficultyProfile } from "./session";

export type FarePolicyManifest = {
  enabled: boolean;
  fare_mode_bus_base: number;
  fare_mode_tram_base: number;
  fare_mode_metro_base: number;
  fare_mode_rail_base: number;
  fare_mode_ferry_base: number;
  fare_mode_default_base: number;
  transfer_window_s: number;
  free_transfers_per_trip: number;
};

export type EconomyManifest = {
  currency: CurrencyCode;
  difficulty: string;
  difficulty_profile?: DifficultyProfile;
  economy_revision?: number;
  starting_budget_base: number;
  current_balance_base: number;
  cumulative_capex_base: number;
  cumulative_opex_base: number;
  cumulative_revenue_base: number;
  cumulative_lost_demand_penalty_base: number;
  unlocked_countries: string[];
  fare_policy: FarePolicyManifest;
  region_ledger?: Record<
    string,
    {
      revenue_base: number;
      opex_base: number;
      capex_base: number;
      penalties_base: number;
      net_base: number;
    }
  >;
  maintenance_rate?: number;
  ancillary_revenue_rate?: number;
  quality_penalty_rates?: {
    overcrowding_base_per_passenger: number;
    reliability_base_per_passenger: number;
  };
  monthly_financials?: Array<{
    month_index: number;
    revenue_base: number;
    opex_base: number;
    capex_base: number;
    penalties_base: number;
    net_base: number;
  }>;
};

export type FinancialDashboardRequest = {
  mode?: string | null;
  line_id?: string | null;
  region_id?: string | null;
  granularity?: "day" | "week" | "month" | "year" | null;
  periods?: number | null;
};

export type FinancialDashboardPoint = {
  period_index: number;
  label: string;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialModeBreakdownRow = {
  mode: string;
  lines: number;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialLineBreakdownRow = {
  line_id: string;
  line_name: string;
  mode: string;
  region_id?: string | null;
  estimated_capex_base: number;
  estimated_opex_per_hour_base: number;
  staff_opex_per_hour_base: number;
  fleet_value_base: number;
  units_owned: number;
  units_pending: number;
  units_assigned: number;
};

export type FinancialRegionBreakdownRow = {
  region_id: string;
  revenue_base: number;
  opex_base: number;
  capex_base: number;
  penalties_base: number;
  net_base: number;
};

export type FinancialDashboardResponse = {
  currency: CurrencyCode | string;
  granularity: string;
  periods: number;
  current_balance_base: number;
  total_revenue_base: number;
  total_opex_base: number;
  total_capex_base: number;
  total_penalties_base: number;
  total_net_base: number;
  points: FinancialDashboardPoint[];
  mode_breakdown: FinancialModeBreakdownRow[];
  line_breakdown: FinancialLineBreakdownRow[];
  region_breakdown: FinancialRegionBreakdownRow[];
};
