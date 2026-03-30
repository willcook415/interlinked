use crate::model::Scenario;
use crate::sim::{canonical_mode_from_tokens, lookup_mode_key_value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyConfig {
    pub base_currency: String,
    pub country_entry_fee_base: f64,
    pub station_capex_base: f64,
    pub link_capex_per_km_default_base: f64,
    pub service_opex_per_vehicle_hour_default_base: f64,
    pub mode_capex_per_km_base: HashMap<String, f64>,
    pub mode_opex_per_vehicle_hour_base: HashMap<String, f64>,
    pub fx_rates_to_base: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyState {
    pub currency: String,
    pub starting_budget_base: f64,
    pub current_balance_base: f64,
    pub cumulative_capex_base: f64,
    pub cumulative_opex_base: f64,
    pub unlocked_countries: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomySnapshot {
    pub currency: String,
    pub starting_budget: f64,
    pub current_balance: f64,
    pub cumulative_capex: f64,
    pub cumulative_opex: f64,
    pub unlocked_countries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyKpis {
    pub estimated_capex_base: f64,
    pub estimated_opex_per_hour_base: f64,
    pub country_entry_charges_base: f64,
    pub unlocked_countries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub stops: usize,
    pub links: usize,
    pub services: usize,
    pub total_link_km: f64,
}

pub fn default_economy_config() -> EconomyConfig {
    let mut capex = HashMap::<String, f64>::new();
    capex.insert("bus".to_string(), 0.0);
    capex.insert("tram".to_string(), 32_000_000.0);
    capex.insert("metro".to_string(), 185_000_000.0);
    capex.insert("suburban_rail".to_string(), 68_000_000.0);
    capex.insert("regional_rail".to_string(), 78_000_000.0);
    capex.insert("high_speed_rail".to_string(), 140_000_000.0);
    capex.insert("rail".to_string(), 78_000_000.0);
    capex.insert("ferry".to_string(), 4_500_000.0);
    capex.insert("other_transit".to_string(), 22_000_000.0);

    let mut opex = HashMap::<String, f64>::new();
    opex.insert("bus".to_string(), 125.0);
    opex.insert("tram".to_string(), 305.0);
    opex.insert("metro".to_string(), 650.0);
    opex.insert("suburban_rail".to_string(), 460.0);
    opex.insert("regional_rail".to_string(), 520.0);
    opex.insert("high_speed_rail".to_string(), 760.0);
    opex.insert("rail".to_string(), 520.0);
    opex.insert("ferry".to_string(), 420.0);
    opex.insert("other_transit".to_string(), 260.0);

    let mut fx = HashMap::<String, f64>::new();
    fx.insert("GBP".to_string(), 1.0);
    fx.insert("USD".to_string(), 0.79);
    fx.insert("EUR".to_string(), 0.86);

    EconomyConfig {
        base_currency: "GBP".to_string(),
        country_entry_fee_base: 250_000_000.0,
        station_capex_base: 18_000_000.0,
        link_capex_per_km_default_base: 22_000_000.0,
        service_opex_per_vehicle_hour_default_base: 260.0,
        mode_capex_per_km_base: capex,
        mode_opex_per_vehicle_hour_base: opex,
        fx_rates_to_base: fx,
    }
}

pub fn normalize_currency_code(value: &str) -> String {
    match value.trim().to_ascii_uppercase().as_str() {
        "USD" => "USD".to_string(),
        "EUR" => "EUR".to_string(),
        _ => "GBP".to_string(),
    }
}

pub fn to_base_currency(amount: f64, currency: &str, cfg: &EconomyConfig) -> f64 {
    let c = normalize_currency_code(currency);
    let rate = cfg.fx_rates_to_base.get(&c).copied().unwrap_or(1.0);
    amount * rate
}

pub fn from_base_currency(amount_base: f64, currency: &str, cfg: &EconomyConfig) -> f64 {
    let c = normalize_currency_code(currency);
    let rate = cfg.fx_rates_to_base.get(&c).copied().unwrap_or(1.0);
    if rate <= 0.0 {
        amount_base
    } else {
        amount_base / rate
    }
}

pub fn snapshot(state: &EconomyState, cfg: &EconomyConfig) -> EconomySnapshot {
    EconomySnapshot {
        currency: state.currency.clone(),
        starting_budget: from_base_currency(state.starting_budget_base, &state.currency, cfg),
        current_balance: from_base_currency(state.current_balance_base, &state.currency, cfg),
        cumulative_capex: from_base_currency(state.cumulative_capex_base, &state.currency, cfg),
        cumulative_opex: from_base_currency(state.cumulative_opex_base, &state.currency, cfg),
        unlocked_countries: state.unlocked_countries.iter().cloned().collect(),
    }
}

pub fn scenario_network_stats(s: &Scenario) -> NetworkStats {
    let total_link_km = s
        .world
        .links
        .iter()
        .map(|l| l.distance_m.max(0.0) / 1000.0)
        .sum();
    NetworkStats {
        stops: s.world.stops.len(),
        links: s.world.links.len(),
        services: s.world.services.len(),
        total_link_km,
    }
}

pub fn countries_in_scenario(s: &Scenario) -> BTreeSet<String> {
    let mut out = BTreeSet::<String>::new();
    for st in &s.world.stops {
        if let Some(c) = &st.country_iso2 {
            let code = c.trim().to_ascii_uppercase();
            if code.len() == 2 {
                out.insert(code);
            }
        }
    }
    for z in &s.world.zones {
        if let Some(c) = &z.country_iso2 {
            let code = c.trim().to_ascii_uppercase();
            if code.len() == 2 {
                out.insert(code);
            }
        }
    }
    out
}

pub fn estimate_network_capex_base(s: &Scenario, cfg: &EconomyConfig) -> f64 {
    let station = s.world.stops.len() as f64 * cfg.station_capex_base;
    let links: f64 = s
        .world
        .links
        .iter()
        .map(|l| {
            let canonical_mode = canonical_mode_from_tokens(
                &l.mode,
                l.mode_variant.as_deref(),
                (l.distance_m / 1000.0).max(0.0),
            );
            let rate = lookup_mode_key_value(&cfg.mode_capex_per_km_base, canonical_mode)
                .unwrap_or(cfg.link_capex_per_km_default_base);
            (l.distance_m.max(0.0) / 1000.0) * rate
        })
        .sum();
    station + links
}

pub fn estimate_service_opex_per_hour_base(s: &Scenario, cfg: &EconomyConfig) -> f64 {
    let mut by_stop = HashMap::<String, (f64, f64)>::new();
    for st in &s.world.stops {
        by_stop.insert(st.id.clone(), (st.x, st.y));
    }
    let mut by_pair = HashMap::<(String, String), f64>::new();
    for l in &s.world.links {
        by_pair.insert(
            (l.from_stop.clone(), l.to_stop.clone()),
            l.distance_m.max(0.0),
        );
    }

    s.world
        .services
        .iter()
        .map(|sv| {
            let mut route_distance_m = 0.0;
            for pair in sv.stop_sequence.windows(2) {
                if let [a, b] = pair {
                    if let Some(d) = by_pair.get(&(a.clone(), b.clone())) {
                        route_distance_m += *d;
                    } else if let (Some((ax, ay)), Some((bx, by))) =
                        (by_stop.get(a), by_stop.get(b))
                    {
                        let dx = ax - bx;
                        let dy = ay - by;
                        route_distance_m += (dx * dx + dy * dy).sqrt();
                    } else {
                        route_distance_m += 1000.0;
                    }
                }
            }
            let canonical_mode = canonical_mode_from_tokens(
                &sv.mode,
                sv.mode_variant.as_deref(),
                (route_distance_m / 1000.0).max(0.0),
            );
            let opex_per_vehicle_hr =
                lookup_mode_key_value(&cfg.mode_opex_per_vehicle_hour_base, canonical_mode)
                    .unwrap_or(cfg.service_opex_per_vehicle_hour_default_base);
            let speed_mps = 8.0;
            let in_vehicle_s = (route_distance_m / speed_mps).max(120.0);
            let round_trip_s = (in_vehicle_s * 2.0 + (sv.dwell_s.max(0.0) * 2.0)).max(300.0);
            let headway_s = sv.headway_s.max(60.0);
            let vehicles = (round_trip_s / headway_s).ceil().max(1.0);
            vehicles * opex_per_vehicle_hr
        })
        .sum()
}

pub fn estimate_country_entry_charge_base(
    unlocked: &BTreeSet<String>,
    scenario_countries: &BTreeSet<String>,
    cfg: &EconomyConfig,
) -> f64 {
    let new_count = scenario_countries
        .iter()
        .filter(|c| !unlocked.contains(*c))
        .count() as f64;
    new_count * cfg.country_entry_fee_base
}

pub fn planning_economy_kpis(
    s: &Scenario,
    unlocked: &BTreeSet<String>,
    cfg: &EconomyConfig,
) -> EconomyKpis {
    let scenario_countries = countries_in_scenario(s);
    EconomyKpis {
        estimated_capex_base: estimate_network_capex_base(s, cfg),
        estimated_opex_per_hour_base: estimate_service_opex_per_hour_base(s, cfg),
        country_entry_charges_base: estimate_country_entry_charge_base(
            unlocked,
            &scenario_countries,
            cfg,
        ),
        unlocked_countries: scenario_countries.union(unlocked).count(),
    }
}
