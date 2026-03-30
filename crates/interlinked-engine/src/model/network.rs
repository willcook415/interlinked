use crate::model::params::default_vehicle_capacity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PurchaseOrder {
    pub order_id: String,
    pub units: u32,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub unit_cost_base: Option<f64>,
    #[serde(default)]
    pub total_cost_base: Option<f64>,
    #[serde(default)]
    pub placed_at_tick_s: Option<f64>,
    #[serde(default)]
    pub eta_at_tick_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RollingStockProfile {
    #[serde(default)]
    pub package_id: Option<String>,
    #[serde(default)]
    pub units_owned: Option<u32>,
    #[serde(default)]
    pub cars_per_unit: Option<u32>,
    #[serde(default)]
    pub speed_level: Option<String>,
    #[serde(default)]
    pub comfort_level: Option<String>,
    #[serde(default)]
    pub pending_orders: Vec<PurchaseOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LineScheduleProfile {
    #[serde(default)]
    pub peak_start_minute: Option<u32>,
    #[serde(default)]
    pub peak_end_minute: Option<u32>,
    #[serde(default)]
    pub overnight_start_minute: Option<u32>,
    #[serde(default)]
    pub overnight_end_minute: Option<u32>,
    #[serde(default)]
    pub tph_peak: Option<f64>,
    #[serde(default)]
    pub tph_off_peak: Option<f64>,
    #[serde(default)]
    pub tph_overnight: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stop {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub country_iso2: Option<String>,

    // NEW: optional station/interchange identifier
    pub interchange_id: Option<String>,

    // NEW: optional platform/bay metadata (helps later)
    pub stop_type: Option<String>, // e.g. "bus_bay", "rail_platform", "tram_stop"

    #[serde(default)]
    pub station_boarding_capacity_pph: Option<f64>,
    #[serde(default)]
    pub station_alighting_capacity_pph: Option<f64>,
    #[serde(default)]
    pub station_queue_capacity_pax: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub from_stop: String,
    pub to_stop: String,
    pub distance_m: f64,
    pub mode: String,
    pub speed_mps: f64,

    /// Optional polyline geometry for drawing this link on a map.
    ///
    /// - Points are `[x, y]` in the same coordinate system as the stops.
    /// - If omitted, UIs can draw a straight line from stop A -> stop B.
    #[serde(default)]
    pub geometry: Option<Vec<[f64; 2]>>,

    #[serde(default)]
    pub line_id: Option<String>,

    #[serde(default)]
    pub mode_variant: Option<String>,

    // Optional capacity per hour, used later (v0 just records it)
    pub capacity_per_hour: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub mode_variant: Option<String>,
    pub stop_sequence: Vec<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub direction_name: Option<String>,
    #[serde(default)]
    pub display_color: Option<String>,
    #[serde(default)]
    pub service_enabled: Option<bool>,
    #[serde(default)]
    pub operating_tph: Option<f64>,
    #[serde(default)]
    pub stock_tier_id: Option<String>,
    #[serde(default)]
    pub stock_units_owned: Option<u32>,
    #[serde(default)]
    pub stock_units_assigned: Option<u32>,
    #[serde(default)]
    pub rolling_stock_profile: Option<RollingStockProfile>,
    #[serde(default)]
    pub schedule_profile: Option<LineScheduleProfile>,
    pub headway_s: f64,
    pub dwell_s: f64,

    // --- NEW: capacity (per vehicle) ---
    #[serde(default = "default_vehicle_capacity")]
    pub vehicle_capacity: f64,

    // Optional extra penalty when boarding this service (e.g. station access, security, platform time)
    pub board_penalty_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub from_stop: String,
    pub to_stop: String,
    pub time_s: f64,
    pub penalty_s: Option<f64>,
    pub allowed_modes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRule {
    pub from_mode: String,
    pub to_mode: String,

    // Time to interchange regardless of distance (stairs, ticket gates, finding platform)
    pub base_time_s: f64,

    // Extra “ugh” friction converted into time-equivalent seconds (reliability, inconvenience)
    pub penalty_s: f64,

    // Walking speed used when transferring between stops (m/s)
    pub walk_speed_mps: f64,

    // Optional: if stops are further than this, don't create an interchange edge
    pub max_distance_m: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::{Link, Service, Stop};

    #[test]
    fn stop_deserializes_without_builder_metadata() {
        let stop: Stop = serde_json::from_str(
            r#"{"id":"s1","x":12.0,"y":34.0,"country_iso2":"GB","interchange_id":null,"stop_type":"station"}"#,
        )
        .expect("legacy stop json");
        assert_eq!(stop.id, "s1");
        assert_eq!(stop.name, None);
    }

    #[test]
    fn link_deserializes_without_line_metadata() {
        let link: Link = serde_json::from_str(
            r#"{
                "id":"l1",
                "from_stop":"s1",
                "to_stop":"s2",
                "distance_m":1000.0,
                "mode":"metro",
                "speed_mps":16.0,
                "geometry":null,
                "capacity_per_hour":null
            }"#,
        )
        .expect("legacy link json");
        assert_eq!(link.id, "l1");
        assert_eq!(link.line_id, None);
        assert_eq!(link.mode_variant, None);
    }

    #[test]
    fn service_deserializes_without_display_metadata() {
        let service: Service = serde_json::from_str(
            r#"{
                "id":"svc1",
                "mode":"metro",
                "stop_sequence":["s1","s2"],
                "headway_s":300.0,
                "dwell_s":25.0,
                "vehicle_capacity":500.0,
                "board_penalty_s":null
            }"#,
        )
        .expect("legacy service json");
        assert_eq!(service.id, "svc1");
        assert_eq!(service.line_id, None);
        assert_eq!(service.name, None);
        assert_eq!(service.direction, None);
        assert_eq!(service.direction_name, None);
        assert_eq!(service.display_color, None);
        assert_eq!(service.service_enabled, None);
        assert_eq!(service.operating_tph, None);
        assert_eq!(service.stock_tier_id, None);
        assert_eq!(service.stock_units_owned, None);
        assert_eq!(service.stock_units_assigned, None);
        assert_eq!(service.rolling_stock_profile, None);
        assert_eq!(service.schedule_profile, None);
        assert_eq!(service.mode_variant, None);
    }
}
