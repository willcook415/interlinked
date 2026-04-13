use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionIdTier {
    County,
    H3Res6,
    H3Res7,
}

impl RegionIdTier {
    pub(crate) fn as_tier_tag(self) -> &'static str {
        match self {
            Self::County => "county",
            Self::H3Res6 => "r6",
            Self::H3Res7 => "r7",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedRegionId {
    pub(crate) tier: RegionIdTier,
    pub(crate) country_iso2: String,
    pub(crate) token: String,
}

pub(crate) fn parse_region_id(value: &str) -> Option<ParsedRegionId> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    let [tier_raw, iso_raw, token_raw] = parts.as_slice() else {
        return None;
    };
    let tier = match tier_raw.trim().to_ascii_lowercase().as_str() {
        "county" => RegionIdTier::County,
        "r6" => RegionIdTier::H3Res6,
        "r7" => RegionIdTier::H3Res7,
        _ => return None,
    };
    let country_iso2 = canonical_country_iso2(iso_raw)?;
    let token = token_raw.trim().to_ascii_lowercase();
    if token.is_empty() {
        return None;
    }
    Some(ParsedRegionId {
        tier,
        country_iso2,
        token,
    })
}

pub(crate) fn normalize_loaded_countries(values: Vec<String>) -> Vec<String> {
    canonicalize_country_codes(values)
}

pub(crate) fn normalize_region_id(value: &str) -> Option<String> {
    let parsed = parse_region_id(value)?;
    Some(format!(
        "{}:{}:{}",
        parsed.tier.as_tier_tag(),
        parsed.country_iso2,
        parsed.token
    ))
}

pub(crate) fn canonicalize_region_id(value: &str) -> Option<String> {
    let mut normalized = normalize_region_id(value)?;
    let parsed = parse_region_id(&normalized)?;
    if parsed.tier != RegionIdTier::County || !is_uk_country_iso2(&parsed.country_iso2) {
        return Some(normalized);
    }
    // Compatibility aliases are currently defined by legacy GB county identifiers.
    // Runtime canonical identity remains UK; alias lookups remap GB -> UK.
    // Canonical runtime region identity remains country-agnostic and tier-based.
    if let Ok(aliases) = load_gb_county_aliases() {
        for _ in 0..8 {
            let alias_lookup = normalized.replacen(":UK:", ":GB:", 1);
            let Some(mapped) = aliases
                .get(&alias_lookup)
                .or_else(|| aliases.get(&normalized))
            else {
                break;
            };
            if mapped == &normalized {
                break;
            }
            let Some(next) = normalize_region_id(mapped) else {
                break;
            };
            if next == normalized {
                break;
            }
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
    parse_region_id(region_id).map(|parsed| parsed.country_iso2)
}

pub(crate) fn region_id_tier(region_id: &str) -> Option<RegionIdTier> {
    parse_region_id(region_id).map(|parsed| parsed.tier)
}

pub(crate) fn canonicalize_region_state_manifest(state: &mut RegionStateManifest) {
    state.unlocked_region_ids = state
        .unlocked_region_ids
        .iter()
        .filter_map(|x| canonicalize_region_id(x))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    state.active_region_ids = state
        .active_region_ids
        .iter()
        .filter_map(|x| canonicalize_region_id(x))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    state.primary_focus_region_id = state
        .primary_focus_region_id
        .as_deref()
        .and_then(canonicalize_region_id);
}

pub(crate) fn region_id_from_res6(iso: &str, res6_cell_id: &str) -> String {
    format!(
        "{}:{}:{}",
        RegionIdTier::H3Res6.as_tier_tag(),
        iso.trim().to_ascii_uppercase(),
        res6_cell_id.trim().to_ascii_lowercase()
    )
}

pub(crate) fn region_id_from_county(iso: &str, county_id: &str) -> String {
    format!(
        "{}:{}:{}",
        RegionIdTier::County.as_tier_tag(),
        iso.trim().to_ascii_uppercase(),
        county_id.trim().to_ascii_lowercase()
    )
}
