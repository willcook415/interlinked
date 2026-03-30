use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandTimeSlice {
    pub label: String,
    pub start_s: f64,
    pub end_s: f64,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandPurposeTimeSlice {
    pub label: String,
    pub start_s: f64,
    pub end_s: f64,
    pub home_work_mult: f64,
    pub home_education_mult: f64,
    pub home_retail_mult: f64,
    pub home_recreation_mult: f64,
    pub other_mult: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    // Generalized cost weights
    pub walk_weight: f64,
    pub wait_weight: f64,
    pub ivt_weight: f64,
    pub transfer_penalty_s: f64,

    // Access/egress
    pub access_walk_speed_mps: f64,
    pub access_radius_m: f64,

    // Gravity demand
    pub gravity_beta: f64,     // deterrence parameter (per second)
    pub trips_per_person: f64, // crude scaling: trips produced per person per period

    #[serde(default = "default_purpose_share_home_work")]
    pub purpose_share_home_work: f64,
    #[serde(default = "default_purpose_share_home_education")]
    pub purpose_share_home_education: f64,
    #[serde(default = "default_purpose_share_home_retail")]
    pub purpose_share_home_retail: f64,
    #[serde(default = "default_purpose_share_home_recreation")]
    pub purpose_share_home_recreation: f64,
    #[serde(default = "default_purpose_share_other")]
    pub purpose_share_other: f64,

    #[serde(default = "default_attraction_weight_office")]
    pub attraction_weight_office: f64,
    #[serde(default = "default_attraction_weight_retail")]
    pub attraction_weight_retail: f64,
    #[serde(default = "default_attraction_weight_recreation")]
    pub attraction_weight_recreation: f64,
    #[serde(default = "default_attraction_weight_industrial")]
    pub attraction_weight_industrial: f64,
    #[serde(default = "default_attraction_weight_education")]
    pub attraction_weight_education: f64,
    #[serde(default = "default_attraction_weight_health")]
    pub attraction_weight_health: f64,

    // --- NEW: route choice / assignment controls ---
    #[serde(default = "default_route_choice_k")]
    pub route_choice_k: usize, // number of k-shortest paths to consider per OD

    #[serde(default = "default_route_choice_theta")]
    pub route_choice_theta: f64, // logit dispersion (1/seconds): higher => more deterministic

    #[serde(default = "default_assignment_max_iters")]
    pub assignment_max_iters: usize,

    #[serde(default = "default_assignment_convergence_rel")]
    pub assignment_convergence_rel: f64, // stop when max rel change < this

    // --- NEW: capacity + queueing (milestone 2) ---
    #[serde(default = "default_capacity_enabled")]
    pub capacity_enabled: bool,

    #[serde(default = "default_queue_max_extra_wait_s")]
    pub queue_max_extra_wait_s: f64,

    // --- NEW: fare behavior + elasticity ---
    #[serde(default = "default_fare_enabled")]
    pub fare_enabled: bool,

    #[serde(default = "default_fare_value_of_time_base_per_hour")]
    pub fare_value_of_time_base_per_hour: f64,

    #[serde(default = "default_fare_elasticity")]
    pub fare_elasticity: f64,

    #[serde(default = "default_fare_reference_base")]
    pub fare_reference_base: f64,

    #[serde(default = "default_fare_transfer_window_s")]
    pub fare_transfer_window_s: f64,

    #[serde(default = "default_fare_free_transfers_per_trip")]
    pub fare_free_transfers_per_trip: u8,

    #[serde(default = "default_fare_overflow_retry_share")]
    pub fare_overflow_retry_share: f64,

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

    // --- NEW: station auto-capacity scaling ---
    #[serde(default = "default_station_capacity_scale_boarding")]
    pub station_capacity_scale_boarding: f64,
    #[serde(default = "default_station_capacity_scale_alighting")]
    pub station_capacity_scale_alighting: f64,
    #[serde(default = "default_station_queue_capacity_scale")]
    pub station_queue_capacity_scale: f64,

    // Optional: sample a specific OD instead of the first few encountered
    #[serde(default)]
    pub debug_sample_origin_zone: Option<String>,
    #[serde(default)]
    pub debug_sample_dest_zone: Option<String>,

    // Optional demand shaping profile over a 24-hour clock in seconds.
    // If empty, demand multiplier is 1.0 for all times.
    #[serde(default)]
    pub demand_profile: Vec<DemandTimeSlice>,

    #[serde(default)]
    pub demand_purpose_profile: Vec<DemandPurposeTimeSlice>,
}

fn default_route_choice_k() -> usize {
    3
}

fn default_route_choice_theta() -> f64 {
    0.002
}

fn default_assignment_max_iters() -> usize {
    8
}

fn default_assignment_convergence_rel() -> f64 {
    0.01
}

fn default_capacity_enabled() -> bool {
    true
}

fn default_queue_max_extra_wait_s() -> f64 {
    3600.0
}

fn default_fare_enabled() -> bool {
    false
}

fn default_fare_value_of_time_base_per_hour() -> f64 {
    12.0
}

fn default_fare_elasticity() -> f64 {
    0.35
}

fn default_fare_reference_base() -> f64 {
    2.5
}

fn default_fare_transfer_window_s() -> f64 {
    2700.0
}

fn default_fare_free_transfers_per_trip() -> u8 {
    1
}

fn default_fare_overflow_retry_share() -> f64 {
    0.15
}

fn default_fare_mode_bus_base() -> f64 {
    1.8
}

fn default_fare_mode_tram_base() -> f64 {
    2.3
}

fn default_fare_mode_metro_base() -> f64 {
    2.7
}

fn default_fare_mode_rail_base() -> f64 {
    3.6
}

fn default_fare_mode_ferry_base() -> f64 {
    3.0
}

fn default_fare_mode_default_base() -> f64 {
    2.5
}

fn default_station_capacity_scale_boarding() -> f64 {
    1.0
}

fn default_station_capacity_scale_alighting() -> f64 {
    1.0
}

fn default_station_queue_capacity_scale() -> f64 {
    1.0
}

fn default_purpose_share_home_work() -> f64 {
    0.52
}

fn default_purpose_share_home_education() -> f64 {
    0.12
}

fn default_purpose_share_home_retail() -> f64 {
    0.18
}

fn default_purpose_share_home_recreation() -> f64 {
    0.10
}

fn default_purpose_share_other() -> f64 {
    0.08
}

fn default_attraction_weight_office() -> f64 {
    1.0
}

fn default_attraction_weight_retail() -> f64 {
    0.9
}

fn default_attraction_weight_recreation() -> f64 {
    0.7
}

fn default_attraction_weight_industrial() -> f64 {
    1.1
}

fn default_attraction_weight_education() -> f64 {
    0.8
}

fn default_attraction_weight_health() -> f64 {
    0.75
}

pub(crate) fn default_vehicle_capacity() -> f64 {
    80.0
}

impl Params {
    pub fn fare_for_mode(&self, mode: &str) -> f64 {
        match mode.trim().to_ascii_lowercase().as_str() {
            "bus" => self.fare_mode_bus_base.max(0.0),
            "tram" => self.fare_mode_tram_base.max(0.0),
            "metro" => self.fare_mode_metro_base.max(0.0),
            "rail" => self.fare_mode_rail_base.max(0.0),
            "ferry" => self.fare_mode_ferry_base.max(0.0),
            _ => self.fare_mode_default_base.max(0.0),
        }
    }

    pub fn fare_value_of_time_per_s(&self) -> f64 {
        (self.fare_value_of_time_base_per_hour.max(0.0)) / 3600.0
    }

    pub fn overflow_retry_share_clamped(&self) -> f64 {
        self.fare_overflow_retry_share.clamp(0.0, 1.0)
    }

    pub fn demand_multiplier_at(&self, t_s: f64) -> f64 {
        if self.demand_profile.is_empty() {
            return 1.0;
        }

        let day_s = 86_400.0;
        let mut t = t_s % day_s;
        if t < 0.0 {
            t += day_s;
        }

        for slice in &self.demand_profile {
            if slice.multiplier <= 0.0 {
                continue;
            }

            if slice.start_s <= slice.end_s {
                if t >= slice.start_s && t < slice.end_s {
                    return slice.multiplier;
                }
            } else if t >= slice.start_s || t < slice.end_s {
                // Supports wrap-around slices, e.g. 22:00 -> 05:00.
                return slice.multiplier;
            }
        }

        1.0
    }

    pub fn purpose_multipliers_at(&self, t_s: f64) -> (f64, f64, f64, f64, f64) {
        if self.demand_purpose_profile.is_empty() {
            return (1.0, 1.0, 1.0, 1.0, 1.0);
        }

        let day_s = 86_400.0;
        let mut t = t_s % day_s;
        if t < 0.0 {
            t += day_s;
        }
        for slice in &self.demand_purpose_profile {
            if slice.start_s <= slice.end_s {
                if t >= slice.start_s && t < slice.end_s {
                    return (
                        slice.home_work_mult.max(0.0),
                        slice.home_education_mult.max(0.0),
                        slice.home_retail_mult.max(0.0),
                        slice.home_recreation_mult.max(0.0),
                        slice.other_mult.max(0.0),
                    );
                }
            } else if t >= slice.start_s || t < slice.end_s {
                return (
                    slice.home_work_mult.max(0.0),
                    slice.home_education_mult.max(0.0),
                    slice.home_retail_mult.max(0.0),
                    slice.home_recreation_mult.max(0.0),
                    slice.other_mult.max(0.0),
                );
            }
        }
        (1.0, 1.0, 1.0, 1.0, 1.0)
    }
}
