use super::types::TravelMode;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CanonicalTransitMode {
    Bus,
    Tram,
    Metro,
    SuburbanRail,
    RegionalRail,
    HighSpeedRail,
    Ferry,
    OtherTransit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FareModeBucket {
    Bus,
    Tram,
    Metro,
    Rail,
    Ferry,
    Default,
}

const BUS_KEYS: &[&str] = &["bus"];
const TRAM_KEYS: &[&str] = &["tram"];
const METRO_KEYS: &[&str] = &["metro"];
const SUBURBAN_RAIL_KEYS: &[&str] = &["suburban_rail", "commuter_rail", "rail"];
const REGIONAL_RAIL_KEYS: &[&str] = &["regional_rail", "rail"];
const HIGH_SPEED_RAIL_KEYS: &[&str] = &["high_speed_rail", "rail"];
const FERRY_KEYS: &[&str] = &["ferry"];
const OTHER_TRANSIT_KEYS: &[&str] = &["other_transit"];

impl CanonicalTransitMode {
    pub fn travel_mode_family(self) -> TravelMode {
        match self {
            CanonicalTransitMode::Bus => TravelMode::Bus,
            CanonicalTransitMode::Tram | CanonicalTransitMode::Metro => TravelMode::MetroTram,
            CanonicalTransitMode::SuburbanRail => TravelMode::SuburbanRail,
            CanonicalTransitMode::RegionalRail => TravelMode::RegionalRail,
            CanonicalTransitMode::HighSpeedRail => TravelMode::HighSpeedRail,
            CanonicalTransitMode::Ferry | CanonicalTransitMode::OtherTransit => {
                TravelMode::OtherTransit
            }
        }
    }

    pub fn display_mode_class(self) -> &'static str {
        match self {
            CanonicalTransitMode::Bus => "bus",
            CanonicalTransitMode::Tram => "tram",
            CanonicalTransitMode::Metro => "metro",
            CanonicalTransitMode::SuburbanRail => "commuter_rail",
            CanonicalTransitMode::RegionalRail => "rail",
            CanonicalTransitMode::HighSpeedRail => "high_speed_rail",
            CanonicalTransitMode::Ferry => "ferry",
            CanonicalTransitMode::OtherTransit => "unknown",
        }
    }

    pub fn economy_lookup_keys(self) -> &'static [&'static str] {
        match self {
            CanonicalTransitMode::Bus => BUS_KEYS,
            CanonicalTransitMode::Tram => TRAM_KEYS,
            CanonicalTransitMode::Metro => METRO_KEYS,
            CanonicalTransitMode::SuburbanRail => SUBURBAN_RAIL_KEYS,
            CanonicalTransitMode::RegionalRail => REGIONAL_RAIL_KEYS,
            CanonicalTransitMode::HighSpeedRail => HIGH_SPEED_RAIL_KEYS,
            CanonicalTransitMode::Ferry => FERRY_KEYS,
            CanonicalTransitMode::OtherTransit => OTHER_TRANSIT_KEYS,
        }
    }

    pub fn fare_bucket(self) -> FareModeBucket {
        match self {
            CanonicalTransitMode::Bus => FareModeBucket::Bus,
            CanonicalTransitMode::Tram => FareModeBucket::Tram,
            CanonicalTransitMode::Metro => FareModeBucket::Metro,
            CanonicalTransitMode::SuburbanRail
            | CanonicalTransitMode::RegionalRail
            | CanonicalTransitMode::HighSpeedRail => FareModeBucket::Rail,
            CanonicalTransitMode::Ferry => FareModeBucket::Ferry,
            CanonicalTransitMode::OtherTransit => FareModeBucket::Default,
        }
    }

    pub fn is_rural_essential_candidate(self) -> bool {
        matches!(
            self,
            CanonicalTransitMode::Bus
                | CanonicalTransitMode::SuburbanRail
                | CanonicalTransitMode::RegionalRail
                | CanonicalTransitMode::Ferry
        )
    }
}

pub fn normalize_mode_token(mode: &str) -> String {
    mode.trim().to_ascii_lowercase()
}

pub fn normalize_variant_token(mode_variant: Option<&str>) -> Option<String> {
    mode_variant
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub fn canonical_mode_from_mode_only(mode: &str, trip_distance_km: f64) -> CanonicalTransitMode {
    canonical_mode_from_tokens(mode, None, trip_distance_km)
}

pub fn canonical_mode_from_tokens(
    mode: &str,
    mode_variant: Option<&str>,
    trip_distance_km: f64,
) -> CanonicalTransitMode {
    let mode_token = normalize_mode_token(mode);
    let variant_token = normalize_variant_token(mode_variant);

    if token_has_any(&mode_token, &["bus", "coach", "brt"]) {
        return CanonicalTransitMode::Bus;
    }
    if token_has_any(&mode_token, &["tram", "streetcar", "light_rail"]) {
        return CanonicalTransitMode::Tram;
    }
    if token_has_any(&mode_token, &["metro", "subway", "underground"]) {
        return CanonicalTransitMode::Metro;
    }
    if token_has_any(&mode_token, &["ferry", "boat", "waterbus"]) {
        return CanonicalTransitMode::Ferry;
    }
    if token_has_any(&mode_token, &["rail", "train"]) {
        return classify_rail_mode(variant_token.as_deref(), trip_distance_km);
    }

    if let Some(variant) = variant_token.as_deref() {
        if token_has_any(variant, &["bus", "coach", "brt"]) {
            return CanonicalTransitMode::Bus;
        }
        if token_has_any(variant, &["tram", "streetcar", "light_rail"]) {
            return CanonicalTransitMode::Tram;
        }
        if token_has_any(variant, &["metro", "subway", "underground"]) {
            return CanonicalTransitMode::Metro;
        }
        if token_has_any(variant, &["ferry", "boat", "waterbus"]) {
            return CanonicalTransitMode::Ferry;
        }
        if token_has_any(variant, &["rail", "train"]) {
            return classify_rail_mode(Some(variant), trip_distance_km);
        }
    }

    CanonicalTransitMode::OtherTransit
}

pub fn travel_mode_family_from_tokens(
    mode: &str,
    mode_variant: Option<&str>,
    trip_distance_km: f64,
) -> TravelMode {
    canonical_mode_from_tokens(mode, mode_variant, trip_distance_km).travel_mode_family()
}

pub fn fare_mode_bucket_from_tokens(
    mode: &str,
    mode_variant: Option<&str>,
    trip_distance_km: f64,
) -> FareModeBucket {
    canonical_mode_from_tokens(mode, mode_variant, trip_distance_km).fare_bucket()
}

pub fn lookup_mode_key_value<T: Copy>(
    values: &HashMap<String, T>,
    canonical_mode: CanonicalTransitMode,
) -> Option<T> {
    for key in canonical_mode.economy_lookup_keys() {
        if let Some(value) = values.get(*key).copied() {
            return Some(value);
        }
    }
    values.iter().find_map(|(key, value)| {
        canonical_mode
            .economy_lookup_keys()
            .iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
            .then_some(*value)
    })
}

fn classify_rail_mode(mode_variant: Option<&str>, trip_distance_km: f64) -> CanonicalTransitMode {
    if let Some(variant) = mode_variant {
        if token_has_any(variant, &["high_speed", "highspeed", "hsr", "bullet"]) {
            return CanonicalTransitMode::HighSpeedRail;
        }
        if token_has_any(variant, &["regional", "intercity"]) {
            return CanonicalTransitMode::RegionalRail;
        }
        if token_has_any(variant, &["commuter", "suburban"]) {
            return CanonicalTransitMode::SuburbanRail;
        }
    }

    if trip_distance_km >= 180.0 {
        CanonicalTransitMode::HighSpeedRail
    } else if trip_distance_km >= 48.0 {
        CanonicalTransitMode::RegionalRail
    } else {
        CanonicalTransitMode::SuburbanRail
    }
}

fn token_has_any(token: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| token.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_mode_prefers_variant_for_rail() {
        assert_eq!(
            canonical_mode_from_tokens("rail", Some("commuter_rail"), 220.0),
            CanonicalTransitMode::SuburbanRail
        );
        assert_eq!(
            canonical_mode_from_tokens("rail", Some("regional_rail"), 10.0),
            CanonicalTransitMode::RegionalRail
        );
        assert_eq!(
            canonical_mode_from_tokens("rail", Some("high_speed_rail"), 25.0),
            CanonicalTransitMode::HighSpeedRail
        );
    }

    #[test]
    fn canonical_mode_falls_back_to_distance_for_rail() {
        assert_eq!(
            canonical_mode_from_mode_only("rail", 12.0),
            CanonicalTransitMode::SuburbanRail
        );
        assert_eq!(
            canonical_mode_from_mode_only("rail", 72.0),
            CanonicalTransitMode::RegionalRail
        );
        assert_eq!(
            canonical_mode_from_mode_only("rail", 280.0),
            CanonicalTransitMode::HighSpeedRail
        );
    }

    #[test]
    fn canonical_mode_handles_non_rail_variants() {
        assert_eq!(
            canonical_mode_from_tokens("metro", None, 0.0),
            CanonicalTransitMode::Metro
        );
        assert_eq!(
            canonical_mode_from_tokens("tram", None, 0.0),
            CanonicalTransitMode::Tram
        );
        assert_eq!(
            canonical_mode_from_tokens("bus", None, 0.0),
            CanonicalTransitMode::Bus
        );
        assert_eq!(
            canonical_mode_from_tokens("ferry", None, 0.0),
            CanonicalTransitMode::Ferry
        );
    }

    #[test]
    fn lookup_mode_value_uses_specific_then_fallback_keys() {
        let mut values = HashMap::<String, f64>::new();
        values.insert("rail".to_string(), 11.0);
        values.insert("regional_rail".to_string(), 17.0);
        values.insert("high_speed_rail".to_string(), 21.0);

        assert_eq!(
            lookup_mode_key_value(&values, CanonicalTransitMode::SuburbanRail),
            Some(11.0)
        );
        assert_eq!(
            lookup_mode_key_value(&values, CanonicalTransitMode::RegionalRail),
            Some(17.0)
        );
        assert_eq!(
            lookup_mode_key_value(&values, CanonicalTransitMode::HighSpeedRail),
            Some(21.0)
        );
    }

    #[test]
    fn rural_candidate_modes_are_explicit() {
        assert!(CanonicalTransitMode::Bus.is_rural_essential_candidate());
        assert!(CanonicalTransitMode::SuburbanRail.is_rural_essential_candidate());
        assert!(CanonicalTransitMode::RegionalRail.is_rural_essential_candidate());
        assert!(CanonicalTransitMode::Ferry.is_rural_essential_candidate());
        assert!(!CanonicalTransitMode::Metro.is_rural_essential_candidate());
    }
}
