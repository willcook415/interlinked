use crate::*;

pub(crate) fn normalize_loaded_countries(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn normalize_region_id(value: &str) -> Option<String> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, token] => {
            let tier = tier.trim().to_ascii_lowercase();
            let iso = iso.trim().to_ascii_uppercase();
            let token = token.trim();
            if (tier != "r6" && tier != "r7" && tier != "county")
                || iso.len() != 2
                || token.is_empty()
            {
                return None;
            }
            let token = token.to_ascii_lowercase();
            Some(format!("{tier}:{iso}:{token}"))
        }
        _ => None,
    }
}

pub(crate) fn canonicalize_region_id(value: &str) -> Option<String> {
    let mut normalized = normalize_region_id(value)?;
    if !normalized.starts_with("county:GB:") {
        return Some(normalized);
    }
    if let Ok(aliases) = load_gb_county_aliases() {
        for _ in 0..8 {
            let Some(mapped) = aliases.get(&normalized) else {
                break;
            };
            if mapped == &normalized {
                break;
            }
            let Some(next) = normalize_region_id(mapped) else {
                break;
            };
            normalized = next;
        }
    }
    Some(normalized)
}

pub(crate) fn canonicalize_region_ledger(ledger: &mut BTreeMap<String, RegionEconomyLedger>) {
    let mut merged = BTreeMap::<String, RegionEconomyLedger>::new();
    let old = std::mem::take(ledger);
    for (key, value) in old {
        let canonical = canonicalize_region_id(&key).unwrap_or(key);
        let entry = merged.entry(canonical).or_default();
        entry.revenue_base += value.revenue_base;
        entry.opex_base += value.opex_base;
        entry.capex_base += value.capex_base;
        entry.penalties_base += value.penalties_base;
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    *ledger = merged;
}

pub(crate) fn region_country_iso2(region_id: &str) -> Option<String> {
    let parts = region_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, _token]
            if (tier.to_ascii_lowercase().starts_with('r')
                || tier.eq_ignore_ascii_case("county"))
                && iso.len() == 2 =>
        {
            Some(iso.to_ascii_uppercase())
        }
        _ => None,
    }
}

pub(crate) fn region_id_from_res6(iso: &str, res6_cell_id: &str) -> String {
    format!("r6:{}:{}", iso.trim().to_ascii_uppercase(), res6_cell_id)
}

pub(crate) fn region_id_from_county(iso: &str, county_id: &str) -> String {
    format!(
        "county:{}:{}",
        iso.trim().to_ascii_uppercase(),
        county_id.trim()
    )
}
