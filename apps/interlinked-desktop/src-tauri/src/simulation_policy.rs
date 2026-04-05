use crate::*;

pub(crate) fn default_difficulty_label() -> String {
    "standard".to_string()
}

pub(crate) fn default_profile_multiplier_missing() -> f64 {
    -1.0
}

pub(crate) fn default_economy_revision() -> u64 {
    0
}

pub(crate) fn difficulty_profile_for_label(label: &str) -> DifficultyProfile {
    let token = label.trim().to_ascii_lowercase();
    match token.as_str() {
        "easy" => DifficultyProfile {
            profile_id: "easy".to_string(),
            demand_mult: 0.85,
            capex_mult: 0.90,
            opex_mult: 0.92,
            maintenance_mult: 0.85,
            penalty_mult: 0.85,
            ancillary_revenue_mult: 1.08,
            unlock_cost_mult: 0.90,
        },
        "hard" => DifficultyProfile {
            profile_id: "hard".to_string(),
            demand_mult: 1.20,
            capex_mult: 1.15,
            opex_mult: 1.18,
            maintenance_mult: 1.25,
            penalty_mult: 1.25,
            ancillary_revenue_mult: 0.92,
            unlock_cost_mult: 1.15,
        },
        _ => DifficultyProfile {
            profile_id: "standard".to_string(),
            demand_mult: 1.0,
            capex_mult: 1.0,
            opex_mult: 1.0,
            maintenance_mult: 1.0,
            penalty_mult: 1.0,
            ancillary_revenue_mult: 1.0,
            unlock_cost_mult: 1.0,
        },
    }
}

pub(crate) fn difficulty_profile_for(difficulty: Difficulty) -> DifficultyProfile {
    match difficulty {
        Difficulty::Easy => difficulty_profile_for_label("easy"),
        Difficulty::Standard => difficulty_profile_for_label("standard"),
        Difficulty::Hard => difficulty_profile_for_label("hard"),
    }
}

pub(crate) fn sanitize_multiplier_or_fallback(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

pub(crate) fn sanitize_difficulty_profile(profile: &mut DifficultyProfile, difficulty_label: &str) {
    let fallback = difficulty_profile_for_label(difficulty_label);
    if profile.profile_id.trim().is_empty() {
        *profile = fallback;
        return;
    }
    profile.demand_mult =
        sanitize_multiplier_or_fallback(profile.demand_mult, fallback.demand_mult).clamp(0.2, 3.0);
    profile.capex_mult =
        sanitize_multiplier_or_fallback(profile.capex_mult, fallback.capex_mult).clamp(0.2, 3.0);
    profile.opex_mult =
        sanitize_multiplier_or_fallback(profile.opex_mult, fallback.opex_mult).clamp(0.2, 3.0);
    profile.maintenance_mult =
        sanitize_multiplier_or_fallback(profile.maintenance_mult, fallback.maintenance_mult)
            .clamp(0.2, 3.0);
    profile.penalty_mult =
        sanitize_multiplier_or_fallback(profile.penalty_mult, fallback.penalty_mult)
            .clamp(0.2, 3.0);
    profile.ancillary_revenue_mult = sanitize_multiplier_or_fallback(
        profile.ancillary_revenue_mult,
        fallback.ancillary_revenue_mult,
    )
    .clamp(0.2, 3.0);
    profile.unlock_cost_mult =
        sanitize_multiplier_or_fallback(profile.unlock_cost_mult, fallback.unlock_cost_mult)
            .clamp(0.2, 3.0);
}

pub(crate) fn resolved_difficulty_profile(manifest: &ProjectManifest) -> DifficultyProfile {
    let mut profile = manifest.economy.difficulty_profile.clone();
    sanitize_difficulty_profile(&mut profile, &manifest.economy.difficulty);
    profile
}

pub(crate) fn bump_economy_revision(manifest: &mut ProjectManifest) {
    manifest.economy.economy_revision = manifest.economy.economy_revision.saturating_add(1);
}

pub(crate) fn economy_config() -> EconomyConfig {
    default_economy_config()
}

pub(crate) fn difficulty_label(difficulty: Difficulty) -> String {
    match difficulty {
        Difficulty::Easy => "easy".to_string(),
        Difficulty::Standard => "standard".to_string(),
        Difficulty::Hard => "hard".to_string(),
    }
}

pub(crate) fn default_starting_budget_display(difficulty: Difficulty, currency: &str) -> f64 {
    let cfg = economy_config();
    let base = match difficulty {
        Difficulty::Easy => 3_000_000_000.0,
        Difficulty::Standard => 1_500_000_000.0,
        Difficulty::Hard => 750_000_000.0,
    };
    from_base_currency(base, currency, &cfg)
}

pub(crate) fn active_ledger_key(manifest: &ProjectManifest) -> String {
    manifest
        .region_state
        .primary_focus_region_id
        .as_ref()
        .and_then(|rid| canonicalize_region_id(rid))
        .unwrap_or_else(|| "global".to_string())
}

pub(crate) fn update_region_ledger(
    manifest: &mut ProjectManifest,
    delta_revenue_base: f64,
    delta_opex_base: f64,
    delta_penalty_base: f64,
    delta_capex_base: f64,
) {
    let key = active_ledger_key(manifest);
    let entry = manifest.economy.region_ledger.entry(key).or_default();
    entry.revenue_base += delta_revenue_base;
    entry.opex_base += delta_opex_base;
    entry.penalties_base += delta_penalty_base;
    entry.capex_base += delta_capex_base;
    entry.net_base = entry.revenue_base - entry.opex_base - entry.penalties_base - entry.capex_base;
}

pub(crate) fn sanitize_quality_penalty_rates(rates: &mut QualityPenaltyRates) {
    if !rates.overcrowding_base_per_passenger.is_finite() {
        rates.overcrowding_base_per_passenger = default_overcrowding_penalty_rate();
    }
    if !rates.reliability_base_per_passenger.is_finite() {
        rates.reliability_base_per_passenger = default_reliability_penalty_rate();
    }
    rates.overcrowding_base_per_passenger = rates.overcrowding_base_per_passenger.clamp(0.0, 100.0);
    rates.reliability_base_per_passenger = rates.reliability_base_per_passenger.clamp(0.0, 100.0);
}

pub(crate) fn sanitize_monthly_financials(monthly: &mut Vec<MonthlyFinancialSnapshot>) {
    monthly.retain(|entry| {
        entry.revenue_base.is_finite()
            && entry.opex_base.is_finite()
            && entry.capex_base.is_finite()
            && entry.penalties_base.is_finite()
            && entry.net_base.is_finite()
    });
    monthly.sort_by(|a, b| a.month_index.cmp(&b.month_index));
    if monthly.len() > ECONOMY_MONTHLY_FINANCIAL_CAP {
        let drain = monthly.len().saturating_sub(ECONOMY_MONTHLY_FINANCIAL_CAP);
        monthly.drain(0..drain);
    }
    for entry in monthly.iter_mut() {
        entry.revenue_base = sanitize_non_negative(entry.revenue_base);
        entry.opex_base = sanitize_non_negative(entry.opex_base);
        entry.capex_base = sanitize_non_negative(entry.capex_base);
        entry.penalties_base = sanitize_non_negative(entry.penalties_base);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
}

pub(crate) fn sanitize_economy_manifest(economy: &mut EconomyManifest) {
    economy.starting_budget_base = sanitize_non_negative(economy.starting_budget_base);
    if !economy.current_balance_base.is_finite() {
        economy.current_balance_base = 0.0;
    }
    economy.cumulative_capex_base = sanitize_non_negative(economy.cumulative_capex_base);
    economy.cumulative_opex_base = sanitize_non_negative(economy.cumulative_opex_base);
    economy.cumulative_revenue_base = sanitize_non_negative(economy.cumulative_revenue_base);
    economy.cumulative_lost_demand_penalty_base =
        sanitize_non_negative(economy.cumulative_lost_demand_penalty_base);
    economy.fare_revenue_deferred_base = sanitize_non_negative(economy.fare_revenue_deferred_base);
    economy.fare_boardings_deferred_pax =
        sanitize_non_negative(economy.fare_boardings_deferred_pax);
    if !economy.maintenance_rate.is_finite() {
        economy.maintenance_rate = default_maintenance_rate();
    }
    if !economy.ancillary_revenue_rate.is_finite() {
        economy.ancillary_revenue_rate = default_ancillary_revenue_rate();
    }
    economy.maintenance_rate = economy.maintenance_rate.clamp(0.0, 0.05);
    economy.ancillary_revenue_rate = economy.ancillary_revenue_rate.clamp(0.0, 0.75);
    sanitize_quality_penalty_rates(&mut economy.quality_penalty_rates);
    sanitize_difficulty_profile(&mut economy.difficulty_profile, &economy.difficulty);
    for entry in economy.region_ledger.values_mut() {
        if !entry.revenue_base.is_finite() {
            entry.revenue_base = 0.0;
        }
        if !entry.opex_base.is_finite() {
            entry.opex_base = 0.0;
        }
        if !entry.capex_base.is_finite() {
            entry.capex_base = 0.0;
        }
        if !entry.penalties_base.is_finite() {
            entry.penalties_base = 0.0;
        }
        if !entry.net_base.is_finite() {
            entry.net_base = 0.0;
        }
        entry.revenue_base = sanitize_non_negative(entry.revenue_base);
        entry.opex_base = sanitize_non_negative(entry.opex_base);
        entry.capex_base = sanitize_non_negative(entry.capex_base);
        entry.penalties_base = sanitize_non_negative(entry.penalties_base);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    sanitize_monthly_financials(&mut economy.monthly_financials);
    sanitize_fare_policy(&mut economy.fare_policy);
}

pub(crate) fn month_index_for_tick_seconds(tick_seconds: f64) -> u64 {
    if !tick_seconds.is_finite() || tick_seconds <= 0.0 {
        return 0;
    }
    (tick_seconds / ECONOMY_MONTH_SECONDS).floor().max(0.0) as u64
}

pub(crate) fn record_monthly_financial_delta(
    manifest: &mut ProjectManifest,
    revenue_base: f64,
    opex_base: f64,
    capex_base: f64,
    penalties_base: f64,
) {
    let revenue = sanitize_non_negative(revenue_base);
    let opex = sanitize_non_negative(opex_base);
    let capex = sanitize_non_negative(capex_base);
    let penalties = sanitize_non_negative(penalties_base);
    if revenue <= 0.0 && opex <= 0.0 && capex <= 0.0 && penalties <= 0.0 {
        return;
    }
    let month_index = month_index_for_tick_seconds(manifest.clock_state.tick_seconds);
    if let Some(entry) = manifest
        .economy
        .monthly_financials
        .iter_mut()
        .find(|entry| entry.month_index == month_index)
    {
        entry.revenue_base += revenue;
        entry.opex_base += opex;
        entry.capex_base += capex;
        entry.penalties_base += penalties;
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    } else {
        manifest
            .economy
            .monthly_financials
            .push(MonthlyFinancialSnapshot {
                month_index,
                revenue_base: revenue,
                opex_base: opex,
                capex_base: capex,
                penalties_base: penalties,
                net_base: revenue - opex - capex - penalties,
            });
    }
    sanitize_monthly_financials(&mut manifest.economy.monthly_financials);
}

pub(crate) fn normalize_financial_granularity(value: Option<&str>) -> String {
    let token = value
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "month".to_string());
    match token.as_str() {
        "day" | "week" | "month" | "year" => token,
        _ => "month".to_string(),
    }
}

pub(crate) fn financial_points_from_monthly(
    monthly: &[MonthlyFinancialSnapshot],
) -> Vec<FinancialDashboardPoint> {
    let mut rows = monthly.to_vec();
    rows.sort_by_key(|row| row.month_index);
    rows.into_iter()
        .map(|row| FinancialDashboardPoint {
            period_index: row.month_index as i64,
            label: format!("M{}", row.month_index.saturating_add(1)),
            revenue_base: row.revenue_base.max(0.0),
            opex_base: row.opex_base.max(0.0),
            capex_base: row.capex_base.max(0.0),
            penalties_base: row.penalties_base.max(0.0),
            net_base: row.net_base,
        })
        .collect()
}

pub(crate) fn distribute_month_points(
    points: &[FinancialDashboardPoint],
    slices_per_month: usize,
    prefix: &str,
) -> Vec<FinancialDashboardPoint> {
    if slices_per_month == 0 {
        return vec![];
    }
    let mut out = Vec::<FinancialDashboardPoint>::new();
    for point in points {
        for idx in 0..slices_per_month {
            let denominator = slices_per_month as f64;
            out.push(FinancialDashboardPoint {
                period_index: point.period_index * slices_per_month as i64 + idx as i64,
                label: format!("{}{}-{}", prefix, point.label, idx + 1),
                revenue_base: point.revenue_base / denominator,
                opex_base: point.opex_base / denominator,
                capex_base: point.capex_base / denominator,
                penalties_base: point.penalties_base / denominator,
                net_base: point.net_base / denominator,
            });
        }
    }
    out
}

pub(crate) fn aggregate_year_points(points: &[FinancialDashboardPoint]) -> Vec<FinancialDashboardPoint> {
    let mut grouped = BTreeMap::<i64, FinancialDashboardPoint>::new();
    for point in points {
        let year_index = (point.period_index / 12).max(0);
        let entry = grouped
            .entry(year_index)
            .or_insert(FinancialDashboardPoint {
                period_index: year_index,
                label: format!("Y{}", year_index + 1),
                revenue_base: 0.0,
                opex_base: 0.0,
                capex_base: 0.0,
                penalties_base: 0.0,
                net_base: 0.0,
            });
        entry.revenue_base += point.revenue_base.max(0.0);
        entry.opex_base += point.opex_base.max(0.0);
        entry.capex_base += point.capex_base.max(0.0);
        entry.penalties_base += point.penalties_base.max(0.0);
        entry.net_base += point.net_base;
    }
    grouped.into_values().collect()
}

pub(crate) fn financial_points_for_granularity(
    monthly_points: &[FinancialDashboardPoint],
    granularity: &str,
    periods: usize,
    scale: f64,
) -> Vec<FinancialDashboardPoint> {
    let scaled_monthly = monthly_points
        .iter()
        .map(|point| FinancialDashboardPoint {
            period_index: point.period_index,
            label: point.label.clone(),
            revenue_base: point.revenue_base * scale,
            opex_base: point.opex_base * scale,
            capex_base: point.capex_base * scale,
            penalties_base: point.penalties_base * scale,
            net_base: point.net_base * scale,
        })
        .collect::<Vec<_>>();
    let expanded = match granularity {
        "day" => distribute_month_points(&scaled_monthly, 30, "D"),
        "week" => distribute_month_points(&scaled_monthly, 4, "W"),
        "year" => aggregate_year_points(&scaled_monthly),
        _ => scaled_monthly,
    };
    if expanded.is_empty() {
        return vec![];
    }
    let keep = periods.max(1).min(expanded.len());
    expanded[expanded.len().saturating_sub(keep)..].to_vec()
}

pub(crate) fn apply_economy_realism_tick(
    manifest: &mut ProjectManifest,
    frame: &HistoryFrameLite,
    accrued_fare_revenue_base: f64,
    accrued_boardings_pax: f64,
    completed_alightings_for_revenue: f64,
    service_opex_per_hour: f64,
    staff_opex_per_hour: f64,
    dt_s: f64,
) -> (f64, f64, f64) {
    let difficulty_profile = resolved_difficulty_profile(manifest);
    let accrued_fare_revenue_base = sanitize_non_negative(accrued_fare_revenue_base);
    let accrued_boardings_pax = sanitize_non_negative(accrued_boardings_pax);
    manifest.economy.fare_revenue_deferred_base = sanitize_non_negative(
        manifest.economy.fare_revenue_deferred_base + accrued_fare_revenue_base,
    );
    manifest.economy.fare_boardings_deferred_pax =
        sanitize_non_negative(manifest.economy.fare_boardings_deferred_pax + accrued_boardings_pax);
    let completed_alightings = sanitize_non_negative(completed_alightings_for_revenue);
    let recognized_boardings =
        completed_alightings.min(manifest.economy.fare_boardings_deferred_pax.max(0.0));
    let recognition_ratio = if manifest.economy.fare_boardings_deferred_pax > 0.0 {
        (recognized_boardings / manifest.economy.fare_boardings_deferred_pax).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let fare_revenue_base =
        (manifest.economy.fare_revenue_deferred_base * recognition_ratio).max(0.0);
    manifest.economy.fare_boardings_deferred_pax =
        (manifest.economy.fare_boardings_deferred_pax - recognized_boardings).max(0.0);
    manifest.economy.fare_revenue_deferred_base =
        (manifest.economy.fare_revenue_deferred_base - fare_revenue_base).max(0.0);
    let ancillary_revenue_base = fare_revenue_base
        * manifest.economy.ancillary_revenue_rate.max(0.0)
        * difficulty_profile.ancillary_revenue_mult.max(0.0);
    let delta_revenue_base = fare_revenue_base + ancillary_revenue_base;
    let base_opex = (service_opex_per_hour + staff_opex_per_hour).max(0.0)
        * difficulty_profile.opex_mult.max(0.0)
        * (dt_s / 3600.0);
    let maintenance_reserve = manifest.economy.cumulative_capex_base.max(0.0)
        * manifest.economy.maintenance_rate.max(0.0)
        * difficulty_profile.maintenance_mult.max(0.0)
        * (dt_s / ECONOMY_MONTH_SECONDS);
    let delta_opex_base = base_opex + maintenance_reserve;
    let overcrowding_penalty_base = frame.kpis.total_overflow_dropped.max(0.0)
        * manifest
            .economy
            .quality_penalty_rates
            .overcrowding_base_per_passenger
            .max(0.0);
    let reliability_gap =
        (frame.kpis.total_boardings_attempted - frame.kpis.total_boardings_served).max(0.0);
    let reliability_penalty_base = reliability_gap
        * manifest
            .economy
            .quality_penalty_rates
            .reliability_base_per_passenger
            .max(0.0);
    let delta_penalty_base = (overcrowding_penalty_base + reliability_penalty_base)
        * difficulty_profile.penalty_mult.max(0.0);
    let delta_net_base = delta_revenue_base - delta_opex_base - delta_penalty_base;
    manifest.economy.current_balance_base += delta_net_base;
    manifest.economy.cumulative_revenue_base += delta_revenue_base;
    manifest.economy.cumulative_opex_base += delta_opex_base;
    manifest.economy.cumulative_lost_demand_penalty_base += delta_penalty_base;
    update_region_ledger(
        manifest,
        delta_revenue_base,
        delta_opex_base,
        delta_penalty_base,
        0.0,
    );
    record_monthly_financial_delta(
        manifest,
        delta_revenue_base,
        delta_opex_base,
        0.0,
        delta_penalty_base,
    );
    if delta_revenue_base.abs() > 1e-9
        || delta_opex_base.abs() > 1e-9
        || delta_penalty_base.abs() > 1e-9
    {
        bump_economy_revision(manifest);
    }
    (delta_revenue_base, delta_opex_base, delta_net_base)
}

pub(crate) fn parse_session_kind(value: Option<&str>) -> SessionKind {
    match value.unwrap_or("game").to_ascii_lowercase().as_str() {
        "scenario" => SessionKind::Scenario,
        "sandbox" => SessionKind::Game,
        _ => SessionKind::Game,
    }
}

pub(crate) fn default_clock_for(_kind: &SessionKind) -> SimulationClock {
    SimulationClock {
        sim_datetime_utc: DEFAULT_SIM_START_UTC.to_string(),
        tick_seconds: 0.0,
        running: false,
        speed: 1,
    }
}

pub(crate) fn normalize_speed(speed: u32) -> u32 {
    match speed {
        1 | 2 | 4 => speed,
        _ => 1,
    }
}

pub(crate) fn default_sim_speed() -> u32 {
    1
}

pub(crate) fn default_currency_code() -> String {
    "GBP".to_string()
}

pub(crate) fn normalize_currency(value: Option<&str>) -> String {
    normalize_currency_code(value.unwrap_or("GBP"))
}

pub(crate) fn parse_speed_value(value: Option<&JsonValue>) -> u32 {
    match value {
        Some(JsonValue::Number(n)) => n.as_u64().map(|v| v as u32).unwrap_or(1),
        Some(JsonValue::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "x1" => 1,
            "2" | "x2" => 2,
            "4" | "x4" => 4,
            _ => 1,
        },
        _ => 1,
    }
}

pub(crate) fn default_progress_metrics() -> GameProgressMetrics {
    GameProgressMetrics {
        budget: 0.0,
        currency: default_currency_code(),
        ridership: 0.0,
        coverage: 0.0,
        milestones: 0,
    }
}

pub(crate) fn default_fare_enabled_manifest() -> bool {
    true
}

pub(crate) fn default_fare_mode_bus_base() -> f64 {
    1.8
}

pub(crate) fn default_fare_mode_tram_base() -> f64 {
    2.3
}

pub(crate) fn default_fare_mode_metro_base() -> f64 {
    2.7
}

pub(crate) fn default_fare_mode_rail_base() -> f64 {
    3.6
}

pub(crate) fn default_fare_mode_ferry_base() -> f64 {
    3.0
}

pub(crate) fn default_fare_mode_default_base() -> f64 {
    2.5
}

pub(crate) fn default_fare_transfer_window_s() -> f64 {
    2700.0
}

pub(crate) fn default_fare_free_transfers_per_trip() -> u8 {
    1
}

pub(crate) fn default_fare_policy_manifest() -> FarePolicyManifest {
    FarePolicyManifest {
        enabled: default_fare_enabled_manifest(),
        fare_mode_bus_base: default_fare_mode_bus_base(),
        fare_mode_tram_base: default_fare_mode_tram_base(),
        fare_mode_metro_base: default_fare_mode_metro_base(),
        fare_mode_rail_base: default_fare_mode_rail_base(),
        fare_mode_ferry_base: default_fare_mode_ferry_base(),
        fare_mode_default_base: default_fare_mode_default_base(),
        transfer_window_s: default_fare_transfer_window_s(),
        free_transfers_per_trip: default_fare_free_transfers_per_trip(),
    }
}

pub(crate) fn default_maintenance_rate() -> f64 {
    // Monthly maintenance reserve as a fraction of cumulative capex.
    0.003
}

pub(crate) fn default_ancillary_revenue_rate() -> f64 {
    // Ancillary revenue as a fraction of fare revenue.
    0.06
}

pub(crate) fn default_overcrowding_penalty_rate() -> f64 {
    1.2
}

pub(crate) fn default_reliability_penalty_rate() -> f64 {
    0.4
}

pub(crate) fn default_quality_penalty_rates() -> QualityPenaltyRates {
    QualityPenaltyRates {
        overcrowding_base_per_passenger: default_overcrowding_penalty_rate(),
        reliability_base_per_passenger: default_reliability_penalty_rate(),
    }
}

pub(crate) fn default_economy_manifest() -> EconomyManifest {
    EconomyManifest {
        currency: default_currency_code(),
        difficulty: default_difficulty_label(),
        difficulty_profile: difficulty_profile_for_label("standard"),
        economy_revision: default_economy_revision(),
        starting_budget_base: 0.0,
        current_balance_base: 0.0,
        cumulative_capex_base: 0.0,
        cumulative_opex_base: 0.0,
        cumulative_revenue_base: 0.0,
        cumulative_lost_demand_penalty_base: 0.0,
        fare_revenue_deferred_base: 0.0,
        fare_boardings_deferred_pax: 0.0,
        fare_policy: default_fare_policy_manifest(),
        unlocked_countries: vec![],
        region_ledger: BTreeMap::new(),
        maintenance_rate: default_maintenance_rate(),
        ancillary_revenue_rate: default_ancillary_revenue_rate(),
        quality_penalty_rates: default_quality_penalty_rates(),
        monthly_financials: Vec::new(),
    }
}

pub(crate) fn sanitize_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn sanitize_fare_policy(policy: &mut FarePolicyManifest) {
    policy.fare_mode_bus_base = sanitize_non_negative(policy.fare_mode_bus_base);
    policy.fare_mode_tram_base = sanitize_non_negative(policy.fare_mode_tram_base);
    policy.fare_mode_metro_base = sanitize_non_negative(policy.fare_mode_metro_base);
    policy.fare_mode_rail_base = sanitize_non_negative(policy.fare_mode_rail_base);
    policy.fare_mode_ferry_base = sanitize_non_negative(policy.fare_mode_ferry_base);
    policy.fare_mode_default_base = sanitize_non_negative(policy.fare_mode_default_base);
    policy.transfer_window_s = sanitize_non_negative(policy.transfer_window_s);
    if policy.free_transfers_per_trip > 4 {
        policy.free_transfers_per_trip = 4;
    }
}

pub(crate) fn merge_fare_policy(policy: &mut FarePolicyManifest, patch: &FarePolicyPatch) {
    if let Some(v) = patch.enabled {
        policy.enabled = v;
    }
    if let Some(v) = patch.fare_mode_bus_base {
        policy.fare_mode_bus_base = v;
    }
    if let Some(v) = patch.fare_mode_tram_base {
        policy.fare_mode_tram_base = v;
    }
    if let Some(v) = patch.fare_mode_metro_base {
        policy.fare_mode_metro_base = v;
    }
    if let Some(v) = patch.fare_mode_rail_base {
        policy.fare_mode_rail_base = v;
    }
    if let Some(v) = patch.fare_mode_ferry_base {
        policy.fare_mode_ferry_base = v;
    }
    if let Some(v) = patch.fare_mode_default_base {
        policy.fare_mode_default_base = v;
    }
    if let Some(v) = patch.transfer_window_s {
        policy.transfer_window_s = v;
    }
    if let Some(v) = patch.free_transfers_per_trip {
        policy.free_transfers_per_trip = v;
    }
    sanitize_fare_policy(policy);
}

pub(crate) fn apply_fare_policy_to_params(params: &mut Params, policy: &FarePolicyManifest) {
    params.fare_enabled = policy.enabled;
    params.fare_mode_bus_base = policy.fare_mode_bus_base;
    params.fare_mode_tram_base = policy.fare_mode_tram_base;
    params.fare_mode_metro_base = policy.fare_mode_metro_base;
    params.fare_mode_rail_base = policy.fare_mode_rail_base;
    params.fare_mode_ferry_base = policy.fare_mode_ferry_base;
    params.fare_mode_default_base = policy.fare_mode_default_base;
    params.fare_transfer_window_s = policy.transfer_window_s;
    params.fare_free_transfers_per_trip = policy.free_transfers_per_trip;
}

pub(crate) fn apply_game_runtime_demand_tuning(params: &mut Params) {
    // Game mode should prioritize visible transit usage over pure walk-only shortest paths.
    // Keep this runtime-only so scenario/planning analysis remains neutral.
    params.walk_weight = params.walk_weight.max(4.0);
    params.trips_per_person = params.trips_per_person.max(3.0);
    params.gravity_beta = params.gravity_beta.min(0.00025);
}

pub(crate) fn apply_game_runtime_perf_budget(scenario: &mut Scenario, max_cells: usize) {
    if max_cells == 0 || scenario.world.demand_cells.len() <= max_cells {
        return;
    }

    let stop_points = scenario
        .world
        .stops
        .iter()
        .map(|stop| (stop.x, stop.y))
        .collect::<Vec<_>>();
    let proximity_scale_m = 2_500.0_f64;
    let mut ranked = scenario
        .world
        .demand_cells
        .iter()
        .enumerate()
        .map(|(idx, cell)| {
            let mass = (cell.residents_night.max(0.0) + cell.jobs_day.max(0.0)).max(1.0);
            let proximity = if stop_points.is_empty() {
                0.0
            } else {
                let mut best_d2 = f64::INFINITY;
                for (sx, sy) in &stop_points {
                    let dx = cell.x - *sx;
                    let dy = cell.y - *sy;
                    let d2 = dx * dx + dy * dy;
                    if d2 < best_d2 {
                        best_d2 = d2;
                    }
                }
                if best_d2.is_finite() {
                    let rel = best_d2.sqrt() / proximity_scale_m;
                    1.0 / (1.0 + rel * rel)
                } else {
                    0.0
                }
            };
            let score = mass * (1.0 + 2.5 * proximity);
            (idx, score, cell.cell_id.clone())
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
    });

    let keep_indices = ranked
        .into_iter()
        .take(max_cells)
        .map(|(idx, _, _)| idx)
        .collect::<HashSet<_>>();
    let mut trimmed = Vec::with_capacity(max_cells);
    for (idx, cell) in scenario.world.demand_cells.iter().enumerate() {
        if keep_indices.contains(&idx) {
            trimmed.push(cell.clone());
        }
    }
    scenario.world.demand_cells = trimmed;
}

pub(crate) fn default_demand_surface_manifest() -> DemandSurfaceManifest {
    DemandSurfaceManifest {
        surface_version: "v4".to_string(),
        loaded_countries: vec![],
        pack_version: None,
        last_rebuild_at: None,
    }
}

pub(crate) fn default_max_active_zones() -> usize {
    600
}

pub(crate) fn default_remote_regions_mode() -> String {
    "aggregate".to_string()
}

pub(crate) fn default_remote_update_interval_ticks() -> u32 {
    10
}

pub(crate) fn default_focus_max_active_zones() -> usize {
    480
}

pub(crate) fn default_adjacent_max_active_zones() -> usize {
    220
}

pub(crate) fn default_remote_max_active_zones() -> usize {
    80
}

pub(crate) fn default_adjacent_update_interval_ticks() -> u32 {
    4
}

pub(crate) fn default_simulation_scope_manifest() -> SimulationScopeManifest {
    SimulationScopeManifest {
        max_active_zones: default_max_active_zones(),
        remote_regions_mode: default_remote_regions_mode(),
        remote_update_interval_ticks: default_remote_update_interval_ticks(),
        focus_max_active_zones: default_focus_max_active_zones(),
        adjacent_max_active_zones: default_adjacent_max_active_zones(),
        remote_max_active_zones: default_remote_max_active_zones(),
        adjacent_update_interval_ticks: default_adjacent_update_interval_ticks(),
    }
}

pub(crate) fn normalize_scope(mode: &str) -> String {
    match mode.trim().to_ascii_lowercase().as_str() {
        "aggregate" => "aggregate".to_string(),
        _ => "aggregate".to_string(),
    }
}
