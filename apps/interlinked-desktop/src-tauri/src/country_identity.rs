use std::collections::BTreeSet;

pub(crate) const CANONICAL_UK_ISO2: &str = "UK";
pub(crate) const UK_COMPAT_GB_ISO2: &str = "GB";

pub(crate) fn normalize_country_iso2(value: &str) -> Option<String> {
    let iso = value.trim().to_ascii_uppercase();
    (iso.len() == 2).then_some(iso)
}

pub(crate) fn canonical_country_iso2(value: &str) -> Option<String> {
    let iso = normalize_country_iso2(value)?;
    if iso == UK_COMPAT_GB_ISO2 || iso == CANONICAL_UK_ISO2 {
        return Some(CANONICAL_UK_ISO2.to_string());
    }
    Some(iso)
}

pub(crate) fn is_uk_country_iso2(value: &str) -> bool {
    canonical_country_iso2(value)
        .map(|iso| iso == CANONICAL_UK_ISO2)
        .unwrap_or(false)
}

pub(crate) fn country_iso2_runtime_candidates(value: &str) -> Vec<String> {
    let Some(canonical) = canonical_country_iso2(value) else {
        return Vec::new();
    };
    if canonical == CANONICAL_UK_ISO2 {
        return vec![CANONICAL_UK_ISO2.to_string(), UK_COMPAT_GB_ISO2.to_string()];
    }
    vec![canonical]
}

pub(crate) fn canonicalize_country_codes(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| canonical_country_iso2(&value))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn display_country_name(country_iso2: &str) -> &'static str {
    if is_uk_country_iso2(country_iso2) {
        "United Kingdom"
    } else {
        ""
    }
}
