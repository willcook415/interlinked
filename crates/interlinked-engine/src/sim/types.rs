use crate::model::Params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleOdPaths {
    pub origin_zone: String,
    pub dest_zone: String,
    pub trips: f64,
    pub k_paths_raw: usize,
    pub k_paths_after_dedupe: usize,
    pub paths: Vec<SamplePathOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplePathOption {
    pub share: f64,

    pub gc_s: f64,
    pub walk_s: f64,
    pub wait_s: f64,
    pub ivt_s: f64,

    pub transfer_count: f64,
    pub boardings: f64,

    pub link_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TripPurpose {
    Work,
    Education,
    Shopping,
    Leisure,
    Essential,
    Intercity,
}

impl TripPurpose {
    pub const ALL: [TripPurpose; 6] = [
        TripPurpose::Work,
        TripPurpose::Education,
        TripPurpose::Shopping,
        TripPurpose::Leisure,
        TripPurpose::Essential,
        TripPurpose::Intercity,
    ];
}

impl Default for TripPurpose {
    fn default() -> Self {
        TripPurpose::Work
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DemandTimeSliceLabel {
    EarlyMorning,
    AmPeak,
    Interpeak,
    PmPeak,
    Evening,
    LateNight,
}

impl Default for DemandTimeSliceLabel {
    fn default() -> Self {
        DemandTimeSliceLabel::Interpeak
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ServiceDayType {
    Weekday,
    Saturday,
    SundayHoliday,
}

impl Default for ServiceDayType {
    fn default() -> Self {
        ServiceDayType::Weekday
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SeasonalProfile {
    Neutral,
    SummerPeak,
    WinterPeak,
    TermTime,
    HolidayPeriod,
}

impl Default for SeasonalProfile {
    fn default() -> Self {
        SeasonalProfile::Neutral
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TemporalDemandSlice {
    pub service_day_type: ServiceDayType,
    pub time_slice: DemandTimeSliceLabel,
    pub seasonal_profile: SeasonalProfile,
    #[serde(default)]
    pub active_event_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TravelMode {
    Walk,
    Car,
    Bus,
    MetroTram,
    SuburbanRail,
    RegionalRail,
    HighSpeedRail,
    OtherTransit,
    NoTrip,
}

impl TravelMode {
    pub const ALL: [TravelMode; 9] = [
        TravelMode::Walk,
        TravelMode::Car,
        TravelMode::Bus,
        TravelMode::MetroTram,
        TravelMode::SuburbanRail,
        TravelMode::RegionalRail,
        TravelMode::HighSpeedRail,
        TravelMode::OtherTransit,
        TravelMode::NoTrip,
    ];
}

impl Default for TravelMode {
    fn default() -> Self {
        TravelMode::Car
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum OnTimeStatus {
    OnTime,
    SlightlyLate,
    Late,
    SevereDelay,
}

impl Default for OnTimeStatus {
    fn default() -> Self {
        OnTimeStatus::OnTime
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum OperationalIncidentType {
    None,
    MinorDelay,
    MajorDelay,
    Congestion,
    DwellOverrun,
    VehicleShortTurn,
    ServiceGap,
    TransferFailure,
}

impl Default for OperationalIncidentType {
    fn default() -> Self {
        OperationalIncidentType::None
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FareModel {
    FlatFare,
    DistanceBased,
    ZoneBased,
    ModeBased,
    TransferDiscount,
}

impl Default for FareModel {
    fn default() -> Self {
        FareModel::DistanceBased
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CommercialStrengthClassification {
    Strong,
    Viable,
    Marginal,
    Weak,
}

impl Default for CommercialStrengthClassification {
    fn default() -> Self {
        CommercialStrengthClassification::Marginal
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SocialNecessityClassification {
    Core,
    Important,
    Supportive,
    Low,
}

impl Default for SocialNecessityClassification {
    fn default() -> Self {
        SocialNecessityClassification::Supportive
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeFareSupplement {
    pub mode: TravelMode,
    pub additive_base: f64,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FareModelConfig {
    pub fare_model: FareModel,
    pub flat_fare_base: f64,
    pub distance_fare_base: f64,
    pub distance_fare_per_km: f64,
    pub zone_step_fare_base: f64,
    pub transfer_discount_rate: f64,
    pub transfer_discount_max_count: usize,
    #[serde(default)]
    pub mode_supplements: Vec<ModeFareSupplement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceCostProfile {
    pub mode_family: TravelMode,
    pub fixed_cost_per_period: f64,
    pub vehicle_hour_cost: f64,
    pub vehicle_km_cost: f64,
    pub crew_cost_proxy_per_vehicle_hour: f64,
    pub energy_cost_proxy_per_vehicle_km: f64,
    pub maintenance_cost_proxy_per_vehicle_km: f64,
    pub station_stop_call_cost: f64,
    pub peak_uplift_multiplier: f64,
    pub reliability_penalty_uplift: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InfrastructureCostProfile {
    pub mode_family: TravelMode,
    pub track_km_capex: f64,
    pub station_capex: f64,
    pub stop_capex: f64,
    pub complexity_multiplier: f64,
    pub annualized_maintenance_cost_per_km: f64,
    pub infrastructure_renewal_cost_per_km: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RollingStockCostProfile {
    pub mode_family: TravelMode,
    pub purchase_cost_per_vehicle: f64,
    pub lease_cost_per_period: f64,
    pub annualized_capital_cost_per_vehicle: f64,
    pub maintenance_cost_per_vehicle_period: f64,
    pub capacity_reference: f64,
    pub operating_efficiency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomicsPolicyConfig {
    pub capital_annualization_factor: f64,
    pub shared_infrastructure_allocation_weight: f64,
    pub commercial_strong_farebox_threshold: f64,
    pub commercial_viable_farebox_threshold: f64,
    pub commercial_marginal_farebox_threshold: f64,
    pub social_necessity_rural_threshold: f64,
    pub social_necessity_essential_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeUtilityCoefficients {
    pub utility_scale: f64,
    pub transit_gc_weight: f64,
    pub car_gc_weight: f64,
    pub walk_gc_weight: f64,
    pub transfer_aversion_s: f64,
    pub crowding_penalty_weight: f64,
    pub reliability_penalty_weight: f64,
    pub fare_sensitivity: f64,
    pub denied_boarding_penalty_s: f64,
    pub walk_max_distance_km: f64,
    pub walk_suppression_distance_km: f64,
    pub car_congestion_peak_factor: f64,
    pub car_congestion_weekend_factor: f64,
    pub car_speed_kph_core: f64,
    pub car_speed_kph_urban: f64,
    pub car_speed_kph_suburban: f64,
    pub car_speed_kph_rural: f64,
    pub car_operating_cost_base_per_km: f64,
    pub car_toll_proxy_base: f64,
    pub car_parking_penalty_core_s: f64,
    pub car_parking_penalty_major_city_s: f64,
    pub car_parking_penalty_town_s: f64,
    pub transit_reliability_base_s: f64,
    pub car_reliability_base_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperationsReliabilityConfig {
    pub base_dwell_station_s: f64,
    pub base_dwell_bus_stop_s: f64,
    pub boarding_dwell_s_per_pax: f64,
    pub alighting_dwell_s_per_pax: f64,
    pub crowding_dwell_multiplier: f64,
    pub interchange_dwell_multiplier: f64,
    pub runtime_delay_per_crowding_ratio: f64,
    pub runtime_delay_per_waiting_ratio: f64,
    pub delay_recovery_margin_s: f64,
    pub headway_irregularity_from_delay: f64,
    pub bunching_sensitivity_threshold: f64,
    pub transfer_base_window_s: f64,
    pub transfer_delay_impact: f64,
    pub transfer_crowding_impact: f64,
    pub reliability_penalty_coefficient_s: f64,
    pub irregularity_wait_penalty_weight: f64,
    pub stop_pressure_waiting_threshold: f64,
    pub stop_pressure_denied_threshold: f64,
    pub service_on_time_threshold_minor_s: f64,
    pub service_on_time_threshold_major_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeModeSensitivity {
    pub purpose: TripPurpose,
    pub value_of_time_weight: f64,
    pub cost_sensitivity: f64,
    pub transfer_aversion_multiplier: f64,
    pub crowding_aversion_multiplier: f64,
    pub transit_constant: f64,
    pub car_constant: f64,
    pub walk_constant: f64,
    pub suppression_constant: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettlementModeConstant {
    pub settlement_class: SettlementClass,
    pub transit_constant: f64,
    pub car_constant: f64,
    pub walk_constant: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArchetypeParkingPenalty {
    pub archetype: ZoneArchetype,
    pub parking_penalty_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransitSubmodePreference {
    pub purpose: TripPurpose,
    pub bus: f64,
    pub metro_tram: f64,
    pub suburban_rail: f64,
    pub regional_rail: f64,
    pub high_speed_rail: f64,
    pub other_transit: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SettlementClass {
    GlobalCityCore,
    MajorCity,
    RegionalCity,
    LargeTown,
    SmallTown,
    Village,
    Rural,
    SpecialNode,
}

impl Default for SettlementClass {
    fn default() -> Self {
        SettlementClass::SmallTown
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ZoneArchetype {
    Cbd,
    InnerResidential,
    OuterSuburb,
    TownCentre,
    IndustrialEstate,
    BusinessPark,
    UniversityDistrict,
    RetailLeisureDistrict,
    AirportZone,
    PortLogisticsZone,
    VillageCentre,
    RuralResidential,
    RuralAgricultural,
}

impl Default for ZoneArchetype {
    fn default() -> Self {
        ZoneArchetype::TownCentre
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SpecialAttractorType {
    Airport,
    Port,
    University,
    Stadium,
    Hospital,
    TourismLandmark,
    GovernmentCentre,
    LogisticsHub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeTraitConfig {
    pub archetype: ZoneArchetype,
    pub residential_weight: f64,
    pub employment_weight: f64,
    pub retail_weight: f64,
    pub leisure_weight: f64,
    pub education_weight: f64,
    pub industry_weight: f64,
    pub centrality_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementClassPurposeMultiplier {
    pub settlement_class: SettlementClass,
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorStrengthMultiplier {
    pub attractor_type: SpecialAttractorType,
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDayTypePurposeMultiplier {
    pub service_day_type: ServiceDayType,
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonalPurposeMultiplier {
    pub seasonal_profile: SeasonalProfile,
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDemandModifier {
    pub event_id: String,
    #[serde(default)]
    pub attractor_type: Option<SpecialAttractorType>,
    #[serde(default)]
    pub applies_day_types: Vec<ServiceDayType>,
    #[serde(default)]
    pub applies_time_slices: Vec<DemandTimeSliceLabel>,
    #[serde(default)]
    pub applies_seasonal_profiles: Vec<SeasonalProfile>,
    pub purpose_multipliers: PurposeTripRateModifiers,
    #[serde(default)]
    pub intensity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSlicePurposeMultiplier {
    pub time_slice: DemandTimeSliceLabel,
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticEconomyConfig {
    pub archetype_traits: Vec<ArchetypeTraitConfig>,
    pub settlement_class_multipliers: Vec<SettlementClassPurposeMultiplier>,
    pub purpose_trip_rates: PurposeTripRateModifiers,
    pub day_type_purpose_multipliers: Vec<ServiceDayTypePurposeMultiplier>,
    pub seasonal_purpose_multipliers: Vec<SeasonalPurposeMultiplier>,
    pub purpose_gc_decay_beta: PurposeTripRateModifiers,
    pub purpose_distance_decay_beta: PurposeTripRateModifiers,
    pub centrality_weight: f64,
    pub regional_importance_weight: f64,
    pub corridor_bonus_major_major: f64,
    pub corridor_bonus_commuter: f64,
    pub corridor_bonus_airport_core: f64,
    pub corridor_bonus_regional_link: f64,
    pub rural_baseline_trip_floor_per_person: f64,
    pub rural_essential_demand_floor_per_person: f64,
    pub time_slice_purpose_multipliers: Vec<TimeSlicePurposeMultiplier>,
    pub event_demand_modifiers: Vec<EventDemandModifier>,
    pub event_modifier_strength_scale: f64,
    pub attractor_strength_multipliers: Vec<AttractorStrengthMultiplier>,
    pub mode_utility_coefficients: ModeUtilityCoefficients,
    pub purpose_mode_sensitivities: Vec<PurposeModeSensitivity>,
    pub settlement_mode_constants: Vec<SettlementModeConstant>,
    pub archetype_parking_penalties: Vec<ArchetypeParkingPenalty>,
    pub transit_submode_preferences: Vec<TransitSubmodePreference>,
    #[serde(default)]
    pub operations_reliability_config: OperationsReliabilityConfig,
    #[serde(default)]
    pub fare_model_config: FareModelConfig,
    #[serde(default)]
    pub service_cost_profiles: Vec<ServiceCostProfile>,
    #[serde(default)]
    pub infrastructure_cost_profiles: Vec<InfrastructureCostProfile>,
    #[serde(default)]
    pub rolling_stock_cost_profiles: Vec<RollingStockCostProfile>,
    #[serde(default)]
    pub economics_policy_config: EconomicsPolicyConfig,
}

impl Default for SyntheticEconomyConfig {
    fn default() -> Self {
        Self {
            archetype_traits: vec![
                ArchetypeTraitConfig {
                    archetype: ZoneArchetype::Cbd,
                    residential_weight: 0.35,
                    employment_weight: 1.35,
                    retail_weight: 1.20,
                    leisure_weight: 1.05,
                    education_weight: 0.85,
                    industry_weight: 0.55,
                    centrality_weight: 1.20,
                },
                ArchetypeTraitConfig {
                    archetype: ZoneArchetype::OuterSuburb,
                    residential_weight: 1.15,
                    employment_weight: 0.55,
                    retail_weight: 0.70,
                    leisure_weight: 0.65,
                    education_weight: 0.80,
                    industry_weight: 0.45,
                    centrality_weight: 0.75,
                },
                ArchetypeTraitConfig {
                    archetype: ZoneArchetype::TownCentre,
                    residential_weight: 0.70,
                    employment_weight: 1.00,
                    retail_weight: 1.25,
                    leisure_weight: 1.00,
                    education_weight: 0.80,
                    industry_weight: 0.55,
                    centrality_weight: 1.00,
                },
                ArchetypeTraitConfig {
                    archetype: ZoneArchetype::RuralAgricultural,
                    residential_weight: 0.70,
                    employment_weight: 0.35,
                    retail_weight: 0.30,
                    leisure_weight: 0.50,
                    education_weight: 0.40,
                    industry_weight: 0.55,
                    centrality_weight: 0.35,
                },
            ],
            settlement_class_multipliers: vec![
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::GlobalCityCore,
                    work: 1.25,
                    education: 1.15,
                    shopping: 1.30,
                    leisure: 1.35,
                    essential: 1.05,
                    intercity: 1.60,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::MajorCity,
                    work: 1.18,
                    education: 1.10,
                    shopping: 1.20,
                    leisure: 1.20,
                    essential: 1.00,
                    intercity: 1.35,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::RegionalCity,
                    work: 1.08,
                    education: 1.05,
                    shopping: 1.05,
                    leisure: 1.00,
                    essential: 0.98,
                    intercity: 1.15,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::LargeTown,
                    work: 0.98,
                    education: 0.96,
                    shopping: 0.95,
                    leisure: 0.95,
                    essential: 0.95,
                    intercity: 0.88,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::SmallTown,
                    work: 0.90,
                    education: 0.88,
                    shopping: 0.90,
                    leisure: 0.88,
                    essential: 0.92,
                    intercity: 0.74,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::Village,
                    work: 0.72,
                    education: 0.74,
                    shopping: 0.70,
                    leisure: 0.72,
                    essential: 0.86,
                    intercity: 0.58,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::Rural,
                    work: 0.58,
                    education: 0.60,
                    shopping: 0.56,
                    leisure: 0.60,
                    essential: 0.82,
                    intercity: 0.48,
                },
                SettlementClassPurposeMultiplier {
                    settlement_class: SettlementClass::SpecialNode,
                    work: 1.10,
                    education: 1.02,
                    shopping: 1.08,
                    leisure: 1.12,
                    essential: 1.06,
                    intercity: 1.42,
                },
            ],
            purpose_trip_rates: PurposeTripRateModifiers {
                work: 1.00,
                education: 0.95,
                shopping: 0.92,
                leisure: 0.88,
                essential: 0.52,
                intercity: 0.35,
            },
            day_type_purpose_multipliers: vec![
                ServiceDayTypePurposeMultiplier {
                    service_day_type: ServiceDayType::Weekday,
                    work: 1.00,
                    education: 1.00,
                    shopping: 1.00,
                    leisure: 1.00,
                    essential: 1.00,
                    intercity: 1.00,
                },
                ServiceDayTypePurposeMultiplier {
                    service_day_type: ServiceDayType::Saturday,
                    work: 0.55,
                    education: 0.60,
                    shopping: 1.25,
                    leisure: 1.20,
                    essential: 0.95,
                    intercity: 1.05,
                },
                ServiceDayTypePurposeMultiplier {
                    service_day_type: ServiceDayType::SundayHoliday,
                    work: 0.35,
                    education: 0.35,
                    shopping: 1.05,
                    leisure: 1.18,
                    essential: 0.92,
                    intercity: 1.10,
                },
            ],
            seasonal_purpose_multipliers: vec![
                SeasonalPurposeMultiplier {
                    seasonal_profile: SeasonalProfile::Neutral,
                    work: 1.00,
                    education: 1.00,
                    shopping: 1.00,
                    leisure: 1.00,
                    essential: 1.00,
                    intercity: 1.00,
                },
                SeasonalPurposeMultiplier {
                    seasonal_profile: SeasonalProfile::SummerPeak,
                    work: 0.95,
                    education: 0.92,
                    shopping: 1.10,
                    leisure: 1.25,
                    essential: 1.00,
                    intercity: 1.18,
                },
                SeasonalPurposeMultiplier {
                    seasonal_profile: SeasonalProfile::WinterPeak,
                    work: 1.05,
                    education: 1.00,
                    shopping: 0.96,
                    leisure: 0.90,
                    essential: 1.08,
                    intercity: 0.96,
                },
                SeasonalPurposeMultiplier {
                    seasonal_profile: SeasonalProfile::TermTime,
                    work: 1.02,
                    education: 1.35,
                    shopping: 0.95,
                    leisure: 0.92,
                    essential: 1.00,
                    intercity: 0.98,
                },
                SeasonalPurposeMultiplier {
                    seasonal_profile: SeasonalProfile::HolidayPeriod,
                    work: 0.88,
                    education: 0.45,
                    shopping: 1.12,
                    leisure: 1.25,
                    essential: 1.00,
                    intercity: 1.28,
                },
            ],
            purpose_gc_decay_beta: PurposeTripRateModifiers {
                work: 0.00030,
                education: 0.00034,
                shopping: 0.00052,
                leisure: 0.00040,
                essential: 0.00062,
                intercity: 0.00012,
            },
            purpose_distance_decay_beta: PurposeTripRateModifiers {
                work: 0.030,
                education: 0.032,
                shopping: 0.060,
                leisure: 0.038,
                essential: 0.072,
                intercity: 0.012,
            },
            centrality_weight: 0.65,
            regional_importance_weight: 0.75,
            corridor_bonus_major_major: 1.55,
            corridor_bonus_commuter: 1.38,
            corridor_bonus_airport_core: 1.50,
            corridor_bonus_regional_link: 1.28,
            rural_baseline_trip_floor_per_person: 0.06,
            rural_essential_demand_floor_per_person: 0.08,
            time_slice_purpose_multipliers: vec![
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::EarlyMorning,
                    work: 0.60,
                    education: 0.55,
                    shopping: 0.35,
                    leisure: 0.30,
                    essential: 0.70,
                    intercity: 0.80,
                },
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::AmPeak,
                    work: 1.65,
                    education: 1.35,
                    shopping: 0.75,
                    leisure: 0.60,
                    essential: 0.95,
                    intercity: 0.85,
                },
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::Interpeak,
                    work: 0.85,
                    education: 0.95,
                    shopping: 1.05,
                    leisure: 1.00,
                    essential: 1.00,
                    intercity: 1.00,
                },
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::PmPeak,
                    work: 1.30,
                    education: 0.90,
                    shopping: 1.20,
                    leisure: 1.10,
                    essential: 1.00,
                    intercity: 1.15,
                },
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::Evening,
                    work: 0.40,
                    education: 0.35,
                    shopping: 1.05,
                    leisure: 1.35,
                    essential: 0.95,
                    intercity: 1.10,
                },
                TimeSlicePurposeMultiplier {
                    time_slice: DemandTimeSliceLabel::LateNight,
                    work: 0.12,
                    education: 0.18,
                    shopping: 0.22,
                    leisure: 0.68,
                    essential: 0.72,
                    intercity: 0.58,
                },
            ],
            event_demand_modifiers: vec![
                EventDemandModifier {
                    event_id: "university_term_uplift".to_string(),
                    attractor_type: Some(SpecialAttractorType::University),
                    applies_day_types: vec![ServiceDayType::Weekday],
                    applies_time_slices: vec![
                        DemandTimeSliceLabel::AmPeak,
                        DemandTimeSliceLabel::Interpeak,
                        DemandTimeSliceLabel::PmPeak,
                    ],
                    applies_seasonal_profiles: vec![SeasonalProfile::TermTime],
                    purpose_multipliers: PurposeTripRateModifiers {
                        work: 1.0,
                        education: 1.45,
                        shopping: 1.05,
                        leisure: 1.0,
                        essential: 1.0,
                        intercity: 1.0,
                    },
                    intensity: 1.0,
                },
                EventDemandModifier {
                    event_id: "airport_holiday_surge".to_string(),
                    attractor_type: Some(SpecialAttractorType::Airport),
                    applies_day_types: vec![
                        ServiceDayType::Saturday,
                        ServiceDayType::SundayHoliday,
                    ],
                    applies_time_slices: vec![
                        DemandTimeSliceLabel::EarlyMorning,
                        DemandTimeSliceLabel::Interpeak,
                        DemandTimeSliceLabel::Evening,
                    ],
                    applies_seasonal_profiles: vec![
                        SeasonalProfile::HolidayPeriod,
                        SeasonalProfile::SummerPeak,
                    ],
                    purpose_multipliers: PurposeTripRateModifiers {
                        work: 1.0,
                        education: 0.9,
                        shopping: 1.05,
                        leisure: 1.18,
                        essential: 1.05,
                        intercity: 1.50,
                    },
                    intensity: 1.0,
                },
                EventDemandModifier {
                    event_id: "stadium_evening_spike".to_string(),
                    attractor_type: Some(SpecialAttractorType::Stadium),
                    applies_day_types: vec![
                        ServiceDayType::Weekday,
                        ServiceDayType::Saturday,
                        ServiceDayType::SundayHoliday,
                    ],
                    applies_time_slices: vec![DemandTimeSliceLabel::Evening],
                    applies_seasonal_profiles: vec![
                        SeasonalProfile::Neutral,
                        SeasonalProfile::SummerPeak,
                        SeasonalProfile::HolidayPeriod,
                    ],
                    purpose_multipliers: PurposeTripRateModifiers {
                        work: 1.0,
                        education: 1.0,
                        shopping: 1.02,
                        leisure: 1.55,
                        essential: 1.0,
                        intercity: 1.05,
                    },
                    intensity: 1.0,
                },
                EventDemandModifier {
                    event_id: "tourism_seasonal_uplift".to_string(),
                    attractor_type: Some(SpecialAttractorType::TourismLandmark),
                    applies_day_types: vec![
                        ServiceDayType::Saturday,
                        ServiceDayType::SundayHoliday,
                    ],
                    applies_time_slices: vec![
                        DemandTimeSliceLabel::Interpeak,
                        DemandTimeSliceLabel::Evening,
                    ],
                    applies_seasonal_profiles: vec![
                        SeasonalProfile::SummerPeak,
                        SeasonalProfile::HolidayPeriod,
                    ],
                    purpose_multipliers: PurposeTripRateModifiers {
                        work: 0.95,
                        education: 0.85,
                        shopping: 1.10,
                        leisure: 1.45,
                        essential: 1.0,
                        intercity: 1.22,
                    },
                    intensity: 1.0,
                },
            ],
            event_modifier_strength_scale: 1.0,
            attractor_strength_multipliers: vec![
                AttractorStrengthMultiplier {
                    attractor_type: SpecialAttractorType::Airport,
                    work: 1.20,
                    education: 0.75,
                    shopping: 1.05,
                    leisure: 1.20,
                    essential: 1.08,
                    intercity: 1.90,
                },
                AttractorStrengthMultiplier {
                    attractor_type: SpecialAttractorType::University,
                    work: 0.95,
                    education: 1.85,
                    shopping: 1.12,
                    leisure: 1.05,
                    essential: 1.00,
                    intercity: 1.10,
                },
                AttractorStrengthMultiplier {
                    attractor_type: SpecialAttractorType::Hospital,
                    work: 1.05,
                    education: 0.90,
                    shopping: 0.92,
                    leisure: 0.85,
                    essential: 1.70,
                    intercity: 0.95,
                },
                AttractorStrengthMultiplier {
                    attractor_type: SpecialAttractorType::Port,
                    work: 1.25,
                    education: 0.80,
                    shopping: 0.85,
                    leisure: 0.88,
                    essential: 1.10,
                    intercity: 1.45,
                },
            ],
            mode_utility_coefficients: ModeUtilityCoefficients {
                utility_scale: 0.00085,
                transit_gc_weight: 1.00,
                car_gc_weight: 1.00,
                walk_gc_weight: 1.00,
                transfer_aversion_s: 280.0,
                crowding_penalty_weight: 1.10,
                reliability_penalty_weight: 1.00,
                fare_sensitivity: 0.85,
                denied_boarding_penalty_s: 540.0,
                walk_max_distance_km: 2.4,
                walk_suppression_distance_km: 4.0,
                car_congestion_peak_factor: 1.22,
                car_congestion_weekend_factor: 0.94,
                car_speed_kph_core: 20.0,
                car_speed_kph_urban: 30.0,
                car_speed_kph_suburban: 43.0,
                car_speed_kph_rural: 58.0,
                car_operating_cost_base_per_km: 0.26,
                car_toll_proxy_base: 0.35,
                car_parking_penalty_core_s: 960.0,
                car_parking_penalty_major_city_s: 560.0,
                car_parking_penalty_town_s: 220.0,
                transit_reliability_base_s: 180.0,
                car_reliability_base_s: 90.0,
            },
            purpose_mode_sensitivities: vec![
                PurposeModeSensitivity {
                    purpose: TripPurpose::Work,
                    value_of_time_weight: 1.18,
                    cost_sensitivity: 0.82,
                    transfer_aversion_multiplier: 1.28,
                    crowding_aversion_multiplier: 1.15,
                    transit_constant: 0.25,
                    car_constant: 0.10,
                    walk_constant: -1.15,
                    suppression_constant: -2.10,
                },
                PurposeModeSensitivity {
                    purpose: TripPurpose::Education,
                    value_of_time_weight: 0.92,
                    cost_sensitivity: 0.95,
                    transfer_aversion_multiplier: 1.10,
                    crowding_aversion_multiplier: 1.00,
                    transit_constant: 0.36,
                    car_constant: -0.08,
                    walk_constant: -0.65,
                    suppression_constant: -1.95,
                },
                PurposeModeSensitivity {
                    purpose: TripPurpose::Shopping,
                    value_of_time_weight: 0.80,
                    cost_sensitivity: 1.00,
                    transfer_aversion_multiplier: 0.95,
                    crowding_aversion_multiplier: 0.92,
                    transit_constant: 0.06,
                    car_constant: 0.18,
                    walk_constant: -0.10,
                    suppression_constant: -2.30,
                },
                PurposeModeSensitivity {
                    purpose: TripPurpose::Leisure,
                    value_of_time_weight: 0.72,
                    cost_sensitivity: 0.88,
                    transfer_aversion_multiplier: 0.82,
                    crowding_aversion_multiplier: 0.86,
                    transit_constant: 0.08,
                    car_constant: 0.14,
                    walk_constant: 0.04,
                    suppression_constant: -2.40,
                },
                PurposeModeSensitivity {
                    purpose: TripPurpose::Essential,
                    value_of_time_weight: 0.78,
                    cost_sensitivity: 0.76,
                    transfer_aversion_multiplier: 0.98,
                    crowding_aversion_multiplier: 0.94,
                    transit_constant: -0.02,
                    car_constant: 0.16,
                    walk_constant: -0.22,
                    suppression_constant: -2.95,
                },
                PurposeModeSensitivity {
                    purpose: TripPurpose::Intercity,
                    value_of_time_weight: 1.12,
                    cost_sensitivity: 0.84,
                    transfer_aversion_multiplier: 1.22,
                    crowding_aversion_multiplier: 1.04,
                    transit_constant: 0.18,
                    car_constant: 0.05,
                    walk_constant: -4.50,
                    suppression_constant: -2.55,
                },
            ],
            settlement_mode_constants: vec![
                SettlementModeConstant {
                    settlement_class: SettlementClass::GlobalCityCore,
                    transit_constant: 0.65,
                    car_constant: -0.30,
                    walk_constant: 0.35,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::MajorCity,
                    transit_constant: 0.52,
                    car_constant: -0.16,
                    walk_constant: 0.20,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::RegionalCity,
                    transit_constant: 0.30,
                    car_constant: -0.04,
                    walk_constant: 0.10,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::LargeTown,
                    transit_constant: 0.12,
                    car_constant: 0.10,
                    walk_constant: 0.02,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::SmallTown,
                    transit_constant: -0.02,
                    car_constant: 0.20,
                    walk_constant: -0.06,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::Village,
                    transit_constant: -0.14,
                    car_constant: 0.38,
                    walk_constant: -0.08,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::Rural,
                    transit_constant: -0.26,
                    car_constant: 0.50,
                    walk_constant: -0.10,
                },
                SettlementModeConstant {
                    settlement_class: SettlementClass::SpecialNode,
                    transit_constant: 0.24,
                    car_constant: 0.00,
                    walk_constant: 0.00,
                },
            ],
            archetype_parking_penalties: vec![
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::Cbd,
                    parking_penalty_s: 980.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::TownCentre,
                    parking_penalty_s: 620.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::RetailLeisureDistrict,
                    parking_penalty_s: 540.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::AirportZone,
                    parking_penalty_s: 700.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::BusinessPark,
                    parking_penalty_s: 300.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::IndustrialEstate,
                    parking_penalty_s: 220.0,
                },
                ArchetypeParkingPenalty {
                    archetype: ZoneArchetype::OuterSuburb,
                    parking_penalty_s: 180.0,
                },
            ],
            transit_submode_preferences: vec![
                TransitSubmodePreference {
                    purpose: TripPurpose::Work,
                    bus: 0.85,
                    metro_tram: 1.15,
                    suburban_rail: 1.18,
                    regional_rail: 1.05,
                    high_speed_rail: 0.92,
                    other_transit: 0.95,
                },
                TransitSubmodePreference {
                    purpose: TripPurpose::Education,
                    bus: 1.02,
                    metro_tram: 1.14,
                    suburban_rail: 1.02,
                    regional_rail: 0.96,
                    high_speed_rail: 0.86,
                    other_transit: 0.95,
                },
                TransitSubmodePreference {
                    purpose: TripPurpose::Shopping,
                    bus: 1.08,
                    metro_tram: 1.10,
                    suburban_rail: 0.98,
                    regional_rail: 0.94,
                    high_speed_rail: 0.82,
                    other_transit: 0.94,
                },
                TransitSubmodePreference {
                    purpose: TripPurpose::Leisure,
                    bus: 0.98,
                    metro_tram: 1.08,
                    suburban_rail: 1.00,
                    regional_rail: 1.02,
                    high_speed_rail: 1.12,
                    other_transit: 0.96,
                },
                TransitSubmodePreference {
                    purpose: TripPurpose::Essential,
                    bus: 1.15,
                    metro_tram: 0.94,
                    suburban_rail: 0.92,
                    regional_rail: 0.94,
                    high_speed_rail: 0.78,
                    other_transit: 0.92,
                },
                TransitSubmodePreference {
                    purpose: TripPurpose::Intercity,
                    bus: 0.62,
                    metro_tram: 0.86,
                    suburban_rail: 1.00,
                    regional_rail: 1.10,
                    high_speed_rail: 1.30,
                    other_transit: 0.90,
                },
            ],
            operations_reliability_config: OperationsReliabilityConfig {
                base_dwell_station_s: 24.0,
                base_dwell_bus_stop_s: 16.0,
                boarding_dwell_s_per_pax: 0.22,
                alighting_dwell_s_per_pax: 0.18,
                crowding_dwell_multiplier: 0.30,
                interchange_dwell_multiplier: 1.18,
                runtime_delay_per_crowding_ratio: 0.18,
                runtime_delay_per_waiting_ratio: 0.06,
                delay_recovery_margin_s: 55.0,
                headway_irregularity_from_delay: 0.12,
                bunching_sensitivity_threshold: 0.18,
                transfer_base_window_s: 210.0,
                transfer_delay_impact: 0.22,
                transfer_crowding_impact: 0.35,
                reliability_penalty_coefficient_s: 220.0,
                irregularity_wait_penalty_weight: 0.40,
                stop_pressure_waiting_threshold: 120.0,
                stop_pressure_denied_threshold: 45.0,
                service_on_time_threshold_minor_s: 90.0,
                service_on_time_threshold_major_s: 300.0,
            },
            fare_model_config: FareModelConfig {
                fare_model: FareModel::DistanceBased,
                flat_fare_base: 2.10,
                distance_fare_base: 1.20,
                distance_fare_per_km: 0.16,
                zone_step_fare_base: 0.85,
                transfer_discount_rate: 0.35,
                transfer_discount_max_count: 1,
                mode_supplements: vec![
                    ModeFareSupplement {
                        mode: TravelMode::Bus,
                        additive_base: 0.0,
                        multiplier: 1.0,
                    },
                    ModeFareSupplement {
                        mode: TravelMode::MetroTram,
                        additive_base: 0.15,
                        multiplier: 1.05,
                    },
                    ModeFareSupplement {
                        mode: TravelMode::SuburbanRail,
                        additive_base: 0.25,
                        multiplier: 1.08,
                    },
                    ModeFareSupplement {
                        mode: TravelMode::RegionalRail,
                        additive_base: 0.45,
                        multiplier: 1.12,
                    },
                    ModeFareSupplement {
                        mode: TravelMode::HighSpeedRail,
                        additive_base: 1.25,
                        multiplier: 1.25,
                    },
                ],
            },
            service_cost_profiles: vec![
                ServiceCostProfile {
                    mode_family: TravelMode::Bus,
                    fixed_cost_per_period: 55.0,
                    vehicle_hour_cost: 78.0,
                    vehicle_km_cost: 1.95,
                    crew_cost_proxy_per_vehicle_hour: 28.0,
                    energy_cost_proxy_per_vehicle_km: 0.62,
                    maintenance_cost_proxy_per_vehicle_km: 0.44,
                    station_stop_call_cost: 0.22,
                    peak_uplift_multiplier: 1.10,
                    reliability_penalty_uplift: 0.22,
                },
                ServiceCostProfile {
                    mode_family: TravelMode::MetroTram,
                    fixed_cost_per_period: 120.0,
                    vehicle_hour_cost: 115.0,
                    vehicle_km_cost: 2.45,
                    crew_cost_proxy_per_vehicle_hour: 32.0,
                    energy_cost_proxy_per_vehicle_km: 0.88,
                    maintenance_cost_proxy_per_vehicle_km: 0.62,
                    station_stop_call_cost: 0.34,
                    peak_uplift_multiplier: 1.14,
                    reliability_penalty_uplift: 0.24,
                },
                ServiceCostProfile {
                    mode_family: TravelMode::SuburbanRail,
                    fixed_cost_per_period: 165.0,
                    vehicle_hour_cost: 142.0,
                    vehicle_km_cost: 3.15,
                    crew_cost_proxy_per_vehicle_hour: 40.0,
                    energy_cost_proxy_per_vehicle_km: 1.08,
                    maintenance_cost_proxy_per_vehicle_km: 0.76,
                    station_stop_call_cost: 0.45,
                    peak_uplift_multiplier: 1.18,
                    reliability_penalty_uplift: 0.26,
                },
                ServiceCostProfile {
                    mode_family: TravelMode::RegionalRail,
                    fixed_cost_per_period: 210.0,
                    vehicle_hour_cost: 162.0,
                    vehicle_km_cost: 3.62,
                    crew_cost_proxy_per_vehicle_hour: 46.0,
                    energy_cost_proxy_per_vehicle_km: 1.28,
                    maintenance_cost_proxy_per_vehicle_km: 0.88,
                    station_stop_call_cost: 0.50,
                    peak_uplift_multiplier: 1.16,
                    reliability_penalty_uplift: 0.25,
                },
                ServiceCostProfile {
                    mode_family: TravelMode::HighSpeedRail,
                    fixed_cost_per_period: 320.0,
                    vehicle_hour_cost: 220.0,
                    vehicle_km_cost: 5.20,
                    crew_cost_proxy_per_vehicle_hour: 58.0,
                    energy_cost_proxy_per_vehicle_km: 1.90,
                    maintenance_cost_proxy_per_vehicle_km: 1.15,
                    station_stop_call_cost: 0.70,
                    peak_uplift_multiplier: 1.10,
                    reliability_penalty_uplift: 0.20,
                },
                ServiceCostProfile {
                    mode_family: TravelMode::OtherTransit,
                    fixed_cost_per_period: 95.0,
                    vehicle_hour_cost: 98.0,
                    vehicle_km_cost: 2.60,
                    crew_cost_proxy_per_vehicle_hour: 30.0,
                    energy_cost_proxy_per_vehicle_km: 0.80,
                    maintenance_cost_proxy_per_vehicle_km: 0.58,
                    station_stop_call_cost: 0.30,
                    peak_uplift_multiplier: 1.12,
                    reliability_penalty_uplift: 0.22,
                },
            ],
            infrastructure_cost_profiles: vec![
                InfrastructureCostProfile {
                    mode_family: TravelMode::Bus,
                    track_km_capex: 280_000.0,
                    station_capex: 240_000.0,
                    stop_capex: 70_000.0,
                    complexity_multiplier: 1.0,
                    annualized_maintenance_cost_per_km: 12_500.0,
                    infrastructure_renewal_cost_per_km: 6_500.0,
                },
                InfrastructureCostProfile {
                    mode_family: TravelMode::MetroTram,
                    track_km_capex: 11_500_000.0,
                    station_capex: 8_400_000.0,
                    stop_capex: 260_000.0,
                    complexity_multiplier: 1.25,
                    annualized_maintenance_cost_per_km: 175_000.0,
                    infrastructure_renewal_cost_per_km: 95_000.0,
                },
                InfrastructureCostProfile {
                    mode_family: TravelMode::SuburbanRail,
                    track_km_capex: 7_200_000.0,
                    station_capex: 5_900_000.0,
                    stop_capex: 220_000.0,
                    complexity_multiplier: 1.18,
                    annualized_maintenance_cost_per_km: 145_000.0,
                    infrastructure_renewal_cost_per_km: 82_000.0,
                },
                InfrastructureCostProfile {
                    mode_family: TravelMode::RegionalRail,
                    track_km_capex: 5_100_000.0,
                    station_capex: 4_700_000.0,
                    stop_capex: 170_000.0,
                    complexity_multiplier: 1.12,
                    annualized_maintenance_cost_per_km: 120_000.0,
                    infrastructure_renewal_cost_per_km: 70_000.0,
                },
                InfrastructureCostProfile {
                    mode_family: TravelMode::HighSpeedRail,
                    track_km_capex: 18_500_000.0,
                    station_capex: 14_500_000.0,
                    stop_capex: 450_000.0,
                    complexity_multiplier: 1.35,
                    annualized_maintenance_cost_per_km: 260_000.0,
                    infrastructure_renewal_cost_per_km: 140_000.0,
                },
                InfrastructureCostProfile {
                    mode_family: TravelMode::OtherTransit,
                    track_km_capex: 2_800_000.0,
                    station_capex: 1_900_000.0,
                    stop_capex: 140_000.0,
                    complexity_multiplier: 1.10,
                    annualized_maintenance_cost_per_km: 72_000.0,
                    infrastructure_renewal_cost_per_km: 38_000.0,
                },
            ],
            rolling_stock_cost_profiles: vec![
                RollingStockCostProfile {
                    mode_family: TravelMode::Bus,
                    purchase_cost_per_vehicle: 310_000.0,
                    lease_cost_per_period: 58.0,
                    annualized_capital_cost_per_vehicle: 48_000.0,
                    maintenance_cost_per_vehicle_period: 24.0,
                    capacity_reference: 52.0,
                    operating_efficiency: 1.00,
                },
                RollingStockCostProfile {
                    mode_family: TravelMode::MetroTram,
                    purchase_cost_per_vehicle: 2_100_000.0,
                    lease_cost_per_period: 280.0,
                    annualized_capital_cost_per_vehicle: 260_000.0,
                    maintenance_cost_per_vehicle_period: 74.0,
                    capacity_reference: 190.0,
                    operating_efficiency: 1.06,
                },
                RollingStockCostProfile {
                    mode_family: TravelMode::SuburbanRail,
                    purchase_cost_per_vehicle: 3_500_000.0,
                    lease_cost_per_period: 420.0,
                    annualized_capital_cost_per_vehicle: 390_000.0,
                    maintenance_cost_per_vehicle_period: 92.0,
                    capacity_reference: 280.0,
                    operating_efficiency: 1.08,
                },
                RollingStockCostProfile {
                    mode_family: TravelMode::RegionalRail,
                    purchase_cost_per_vehicle: 4_300_000.0,
                    lease_cost_per_period: 520.0,
                    annualized_capital_cost_per_vehicle: 470_000.0,
                    maintenance_cost_per_vehicle_period: 108.0,
                    capacity_reference: 320.0,
                    operating_efficiency: 1.04,
                },
                RollingStockCostProfile {
                    mode_family: TravelMode::HighSpeedRail,
                    purchase_cost_per_vehicle: 9_800_000.0,
                    lease_cost_per_period: 1_050.0,
                    annualized_capital_cost_per_vehicle: 1_120_000.0,
                    maintenance_cost_per_vehicle_period: 195.0,
                    capacity_reference: 420.0,
                    operating_efficiency: 1.14,
                },
                RollingStockCostProfile {
                    mode_family: TravelMode::OtherTransit,
                    purchase_cost_per_vehicle: 1_500_000.0,
                    lease_cost_per_period: 210.0,
                    annualized_capital_cost_per_vehicle: 165_000.0,
                    maintenance_cost_per_vehicle_period: 58.0,
                    capacity_reference: 150.0,
                    operating_efficiency: 1.02,
                },
            ],
            economics_policy_config: EconomicsPolicyConfig {
                capital_annualization_factor: 0.065,
                shared_infrastructure_allocation_weight: 0.60,
                commercial_strong_farebox_threshold: 1.25,
                commercial_viable_farebox_threshold: 0.95,
                commercial_marginal_farebox_threshold: 0.60,
                social_necessity_rural_threshold: 0.55,
                social_necessity_essential_threshold: 0.22,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeTripRateModifiers {
    pub work: f64,
    pub education: f64,
    pub shopping: f64,
    pub leisure: f64,
    pub essential: f64,
    pub intercity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneDemandProfile {
    pub zone_id: String,
    pub population: f64,
    pub jobs: f64,
    pub archetype: ZoneArchetype,
    pub settlement_class: SettlementClass,
    pub population_density: f64,
    pub employment_density: f64,
    pub retail_intensity: f64,
    pub leisure_intensity: f64,
    pub education_intensity: f64,
    pub industry_intensity: f64,
    pub centrality_score: f64,
    pub regional_importance: f64,
    pub tourism_score: f64,
    pub car_dependency: f64,
    pub transit_affinity: f64,
    #[serde(default)]
    pub nearest_service_centre_id: Option<String>,
    #[serde(default)]
    pub special_attractors: Vec<SpecialAttractorType>,
    pub trip_rate_modifiers: PurposeTripRateModifiers,
    pub work_attractiveness: f64,
    pub education_attractiveness: f64,
    pub shopping_attractiveness: f64,
    pub leisure_attractiveness: f64,
    pub essential_service_attractiveness: f64,
    pub intercity_importance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentOdDemand {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub time_slice: DemandTimeSliceLabel,
    #[serde(default)]
    pub service_day_type: Option<ServiceDayType>,
    #[serde(default)]
    pub seasonal_profile: Option<SeasonalProfile>,
    #[serde(default)]
    pub active_event_ids: Vec<String>,
    pub latent_passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeChoiceContext {
    pub purpose: TripPurpose,
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub service_day_type: ServiceDayType,
    pub time_slice: DemandTimeSliceLabel,
    pub seasonal_profile: SeasonalProfile,
    #[serde(default)]
    pub active_event_ids: Vec<String>,
    pub origin_settlement_class: SettlementClass,
    pub destination_settlement_class: SettlementClass,
    pub origin_archetype: ZoneArchetype,
    pub destination_archetype: ZoneArchetype,
    pub trip_distance_km: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeGeneralizedCostBreakdown {
    pub access_time_s: f64,
    pub wait_time_s: f64,
    pub in_vehicle_time_s: f64,
    pub transfer_penalty_s: f64,
    pub fare_cost_base: f64,
    pub parking_toll_proxy_base: f64,
    pub crowding_penalty_s: f64,
    pub reliability_penalty_s: f64,
    pub egress_time_s: f64,
    pub total_generalized_cost_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeGeneralizedCostByMode {
    pub mode: TravelMode,
    pub breakdown: ModeGeneralizedCostBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeShareValue {
    pub mode: TravelMode,
    pub share: f64,
    pub passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeChoiceResult {
    pub context: ModeChoiceContext,
    pub latent_passengers: f64,
    #[serde(default)]
    pub chosen_mode_shares: Vec<ModeShareValue>,
    #[serde(default)]
    pub generalized_costs_by_mode: Vec<ModeGeneralizedCostByMode>,
    pub transit_captured_passengers: f64,
    pub car_captured_passengers: f64,
    pub walk_captured_passengers: f64,
    pub suppressed_or_no_trip_passengers: f64,
    pub winning_mode: TravelMode,
    #[serde(default)]
    pub transit_submode_split: Vec<ModeShareValue>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssignedPathSummary {
    pub share: f64,
    pub attempted_passengers: f64,
    pub assigned_passengers: f64,
    pub link_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignedOdFlow {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub time_slice: DemandTimeSliceLabel,
    #[serde(default)]
    pub service_day_type: Option<ServiceDayType>,
    #[serde(default)]
    pub seasonal_profile: Option<SeasonalProfile>,
    #[serde(default)]
    pub active_event_ids: Vec<String>,
    pub assigned_passengers: f64,
    pub unserved_passengers: f64,
    #[serde(default)]
    pub suppressed_passengers: f64,
    #[serde(default)]
    pub chosen_paths: Vec<AssignedPathSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WaitingByDestination {
    pub destination_stop_id: String,
    pub waiting_passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopFlowState {
    pub stop_id: String,
    #[serde(default)]
    pub waiting_by_destination: Vec<WaitingByDestination>,
    pub total_waiting: f64,
    pub boarded_this_step: f64,
    pub alighted_this_step: f64,
    pub denied_this_step: f64,
    pub arrived_this_step: f64,
    #[serde(default)]
    pub departed_this_step: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleLoadState {
    pub vehicle_id: String,
    pub run_id: String,
    pub service_id: String,
    pub stop_id: String,
    pub current_load: f64,
    pub boardings_this_stop: f64,
    pub alightings_this_stop: f64,
    pub load_after_stop: f64,
    pub max_load_seen: f64,
    pub capacity: f64,
    pub crowding_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneDemandLayerData {
    pub zone_id: String,
    #[serde(default)]
    pub settlement_class: Option<SettlementClass>,
    #[serde(default)]
    pub archetype: Option<ZoneArchetype>,
    #[serde(default)]
    pub centrality_score: Option<f64>,
    #[serde(default)]
    pub regional_importance: Option<f64>,
    #[serde(default)]
    pub population_density: Option<f64>,
    #[serde(default)]
    pub employment_density: Option<f64>,
    #[serde(default)]
    pub retail_intensity: Option<f64>,
    #[serde(default)]
    pub leisure_intensity: Option<f64>,
    #[serde(default)]
    pub education_intensity: Option<f64>,
    #[serde(default)]
    pub industry_intensity: Option<f64>,
    #[serde(default)]
    pub work_attractiveness: Option<f64>,
    #[serde(default)]
    pub education_attractiveness: Option<f64>,
    #[serde(default)]
    pub shopping_attractiveness: Option<f64>,
    #[serde(default)]
    pub leisure_attractiveness: Option<f64>,
    #[serde(default)]
    pub essential_service_attractiveness: Option<f64>,
    #[serde(default)]
    pub intercity_importance: Option<f64>,
    #[serde(default)]
    pub special_attractors: Vec<SpecialAttractorType>,
    pub total_latent_demand_produced: f64,
    pub total_latent_demand_attracted: f64,
    pub total_realised_demand_produced: f64,
    pub total_unserved_demand_produced: f64,
    #[serde(default)]
    pub accessibility_score: Option<f64>,
    #[serde(default)]
    pub service_coverage_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeDemandValue {
    pub purpose: TripPurpose,
    pub latent: f64,
    pub realised: f64,
    pub unserved: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneDemandProductionLayerData {
    pub zone_id: String,
    #[serde(default)]
    pub by_purpose: Vec<PurposeDemandValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneDemandAttractionLayerData {
    pub zone_id: String,
    #[serde(default)]
    pub latent_by_purpose: Vec<PurposeDemandValue>,
    #[serde(default)]
    pub realised_by_purpose: Vec<PurposeDemandValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorridorDesireLineData {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub latent_passengers: f64,
    pub realised_passengers: f64,
    pub unserved_passengers: f64,
    pub corridor_score: f64,
    pub is_underserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneServiceGapLayerData {
    pub zone_id: String,
    pub total_unserved_demand: f64,
    #[serde(default)]
    pub unserved_by_purpose: Vec<PurposeDemandValue>,
    pub latent_vs_realised_ratio: f64,
    #[serde(default)]
    pub accessibility_score: Option<f64>,
    #[serde(default)]
    pub service_coverage_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneScoreEntry {
    pub zone_id: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceLoadLayerData {
    pub service_id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    pub passengers: f64,
    pub peak_load: f64,
    #[serde(default)]
    pub peak_load_stop_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneEconomicGeographyLayerData {
    pub zone_id: String,
    pub settlement_class: SettlementClass,
    pub archetype: ZoneArchetype,
    pub centrality_score: f64,
    pub regional_importance: f64,
    pub population_density: f64,
    pub employment_density: f64,
    pub retail_intensity: f64,
    pub leisure_intensity: f64,
    pub education_intensity: f64,
    pub industry_intensity: f64,
    pub work_attractiveness: f64,
    pub education_attractiveness: f64,
    pub shopping_attractiveness: f64,
    pub leisure_attractiveness: f64,
    pub essential_service_attractiveness: f64,
    pub intercity_importance: f64,
    #[serde(default)]
    pub special_attractors: Vec<SpecialAttractorType>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CorridorClassification {
    UrbanLocal,
    UrbanTrunkMetroSuitable,
    SuburbanCommuterRadial,
    RegionalConnector,
    Intercity,
    RuralEssentialConnector,
    AirportAccess,
    EducationConnector,
    Mixed,
}

impl Default for CorridorClassification {
    fn default() -> Self {
        CorridorClassification::Mixed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedServiceClass {
    MetroTrunk,
    SuburbanRail,
    IntercityRail,
    RegionalRail,
    TramOrBrt,
    FrequentBus,
    CoverageBus,
    AirportExpress,
    Mixed,
}

impl Default for RecommendedServiceClass {
    fn default() -> Self {
        RecommendedServiceClass::Mixed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ServiceRoleClassification {
    UrbanTrunk,
    Feeder,
    Intercity,
    CommuterRadial,
    LocalCoverage,
    RegionalConnector,
    AirportExpress,
    Mixed,
}

impl Default for ServiceRoleClassification {
    fn default() -> Self {
        ServiceRoleClassification::Mixed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum BuildPreviewType {
    Station,
    LineSegment,
    ServiceFrequencyIncrease,
}

impl Default for BuildPreviewType {
    fn default() -> Self {
        BuildPreviewType::Station
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneFlowReference {
    pub zone_id: String,
    pub passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StopFlowReference {
    pub stop_id: String,
    pub passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorridorReference {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeScoreValue {
    pub purpose: TripPurpose,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OdPatternMetric {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub passengers: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningOverlayConfig {
    pub station_catchment_radius_m: f64,
    pub station_nearby_zone_radius_m: f64,
    pub accessibility_jobs_gc_threshold_s: f64,
    pub accessibility_essential_gc_threshold_s: f64,
    pub accessibility_education_gc_threshold_s: f64,
    pub accessibility_retail_gc_threshold_s: f64,
    pub accessibility_intercity_gc_threshold_s: f64,
    pub service_gap_unserved_ratio_threshold: f64,
    pub corridor_metro_volume_threshold: f64,
    pub corridor_intercity_distance_km_threshold: f64,
    pub corridor_commuter_distance_km_max: f64,
    pub overcrowding_waiting_ratio_threshold: f64,
    pub overcrowding_crowding_ratio_threshold: f64,
    pub service_utilisation_high_threshold: f64,
    pub preview_intercept_weight_unserved: f64,
    pub preview_intercept_weight_latent: f64,
    pub preview_accessibility_delta_weight: f64,
}

impl Default for PlanningOverlayConfig {
    fn default() -> Self {
        Self {
            station_catchment_radius_m: 1200.0,
            station_nearby_zone_radius_m: 900.0,
            accessibility_jobs_gc_threshold_s: 3600.0,
            accessibility_essential_gc_threshold_s: 2800.0,
            accessibility_education_gc_threshold_s: 3200.0,
            accessibility_retail_gc_threshold_s: 3000.0,
            accessibility_intercity_gc_threshold_s: 9000.0,
            service_gap_unserved_ratio_threshold: 0.25,
            corridor_metro_volume_threshold: 220.0,
            corridor_intercity_distance_km_threshold: 35.0,
            corridor_commuter_distance_km_max: 70.0,
            overcrowding_waiting_ratio_threshold: 0.30,
            overcrowding_crowding_ratio_threshold: 0.85,
            service_utilisation_high_threshold: 0.72,
            preview_intercept_weight_unserved: 0.65,
            preview_intercept_weight_latent: 0.35,
            preview_accessibility_delta_weight: 0.40,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoneModeShareMetrics {
    pub zone_id: String,
    pub settlement_class: SettlementClass,
    pub archetype: ZoneArchetype,
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
    pub transit_captured_demand: f64,
    pub non_transit_demand: f64,
    #[serde(default)]
    pub mode_share_by_purpose: Vec<PurposeModeShareValue>,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeModeShareValue {
    pub purpose: TripPurpose,
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorridorModeShareMetrics {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub dominant_mode: TravelMode,
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
    pub strongest_purpose: TripPurpose,
    pub strongest_transit_submode: TravelMode,
    pub transit_captured_demand: f64,
    pub transit_capture_gap: f64,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StationTransitCaptureContext {
    pub stop_id: String,
    pub catchment_latent_demand: f64,
    pub transit_captured_demand: f64,
    pub uncaptured_competing_demand: f64,
    pub limiting_crowding_signal: f64,
    pub limiting_transfer_signal: f64,
    pub limiting_indirectness_signal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceTransitCaptureContext {
    pub service_id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    pub service_mode: TravelMode,
    pub latent_demand_exposed: f64,
    pub transit_captured_demand: f64,
    pub uncaptured_competing_demand: f64,
    pub utilisation_score: f64,
    pub crowding_lost_share_signal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CitywideModeShareSummary {
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
    #[serde(default)]
    pub by_purpose: Vec<PurposeModeShareValue>,
    #[serde(default)]
    pub by_time_slice: Vec<TimeSliceModeShareSummary>,
    #[serde(default)]
    pub by_day_type: Vec<ServiceDayModeShareSummary>,
    pub urban_transit_share: f64,
    pub rural_transit_share: f64,
    pub intercity_transit_share: f64,
    pub airport_access_transit_share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimeSliceModeShareSummary {
    pub temporal_slice: TemporalDemandSlice,
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceDayModeShareSummary {
    pub service_day_type: ServiceDayType,
    pub transit_share: f64,
    pub car_share: f64,
    pub walk_share: f64,
    pub suppressed_share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModalRankingEntry {
    pub id: String,
    pub score: f64,
    pub reason: String,
    pub temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModalDemandDiagnostics {
    #[serde(default)]
    pub mode_share_by_purpose: Vec<PurposeModeShareValue>,
    #[serde(default)]
    pub mode_share_by_zone: Vec<ZoneModeShareMetrics>,
    #[serde(default)]
    pub mode_share_by_corridor: Vec<CorridorModeShareMetrics>,
    #[serde(default)]
    pub mode_share_by_time_slice: Vec<TimeSliceModeShareSummary>,
    #[serde(default)]
    pub mode_share_by_day_type: Vec<ServiceDayModeShareSummary>,
    pub transit_capture_total: f64,
    pub transit_lost_total: f64,
    pub transit_lost_due_to_crowding: f64,
    pub transit_lost_due_to_fare: f64,
    pub transit_lost_due_to_indirectness: f64,
    pub transit_lost_due_to_reliability: f64,
    #[serde(default)]
    pub top_transit_capture_opportunity_corridors: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_car_dominated_transit_viable_corridors: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_overcrowded_corridors_losing_mode_share: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_rural_essential_low_demand_services: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_zones_by_transit_share: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_zones_losing_due_to_transfers: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_zones_losing_due_to_crowding: Vec<ModalRankingEntry>,
    #[serde(default)]
    pub top_zones_where_parking_penalty_supports_transit: Vec<ModalRankingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceOperationState {
    pub service_id: String,
    pub run_id: String,
    pub scheduled_departure_time_s: f64,
    pub expected_headway_s: f64,
    pub actual_departure_time_s: f64,
    pub cumulative_delay_s: f64,
    pub delay_at_last_stop_s: f64,
    pub dwell_time_last_stop_s: f64,
    pub runtime_last_segment_s: f64,
    pub bunching_gap_ahead_s: f64,
    pub bunching_gap_behind_s: f64,
    pub missed_departures: f64,
    pub skipped_capacity_opportunities: f64,
    pub reliability_score: f64,
    pub on_time_status: OnTimeStatus,
    pub incident_type: OperationalIncidentType,
    pub average_delay_s: f64,
    pub max_delay_s: f64,
    pub average_dwell_time_s: f64,
    pub max_dwell_time_s: f64,
    pub average_runtime_segment_s: f64,
    pub max_runtime_segment_s: f64,
    pub headway_irregularity: f64,
    pub scheduled_service_calls: f64,
    pub actual_service_calls: f64,
    pub average_headway_realised_s: f64,
    pub transfer_success_rate: f64,
    #[serde(default)]
    pub strongest_bottleneck_stop_id: Option<String>,
    #[serde(default)]
    pub delay_causes: Vec<String>,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StopOperationState {
    pub stop_id: String,
    pub scheduled_service_calls: f64,
    pub actual_service_calls: f64,
    pub average_headway_realised_s: f64,
    pub headway_irregularity: f64,
    pub average_dwell_time_s: f64,
    pub max_dwell_time_s: f64,
    pub average_wait_s: f64,
    pub platform_crowding_proxy: f64,
    pub denied_boarding_pressure: f64,
    pub transfer_success_rate: f64,
    pub operational_pressure_score: f64,
    pub incident_type: OperationalIncidentType,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransferOperationMetrics {
    pub interchange_stop_id: String,
    pub from_service_id: String,
    pub to_service_id: String,
    pub scheduled_transfer_window_s: f64,
    pub realised_transfer_window_s: f64,
    pub missed_transfer_count: f64,
    pub missed_transfer_rate: f64,
    pub average_transfer_wait_s: f64,
    pub delay_caused_transfer_failures: f64,
    pub interchange_pressure_score: f64,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperationalRankingEntry {
    pub id: String,
    pub score: f64,
    pub reason: String,
    pub temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceReliabilityDiagnostics {
    #[serde(default)]
    pub delay_by_service: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub dwell_inflation_causes: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub bunching_indicators: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub missed_transfers: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub realised_vs_scheduled_headway: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub operational_bottlenecks: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub reliability_linked_mode_choice_penalties: Vec<OperationalRankingEntry>,
    #[serde(default)]
    pub worst_reliability_by_time_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub worst_dwell_pressure_stations_by_time_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub worst_transfer_nodes_by_time_slice: Vec<TemporalRankingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinancialPerformanceMetrics {
    pub fare_revenue: f64,
    pub operating_cost: f64,
    pub infrastructure_cost_allocated: f64,
    pub rolling_stock_cost_allocated: f64,
    pub total_cost: f64,
    pub operating_surplus_deficit: f64,
    pub full_cost_surplus_deficit: f64,
    pub subsidy_required: f64,
    pub farebox_recovery_ratio: f64,
    pub cost_per_passenger: f64,
    pub cost_per_passenger_km: f64,
    pub revenue_per_passenger: f64,
    pub social_value_proxy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkFinancialSummary {
    pub metrics: FinancialPerformanceMetrics,
    pub total_realised_transit_trips: f64,
    pub total_passenger_km: f64,
    pub total_vehicle_km: f64,
    pub total_vehicle_hours: f64,
    pub total_infrastructure_annualized_cost: f64,
    pub total_rolling_stock_annualized_cost: f64,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceFinancialMetrics {
    pub service_id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    pub service_mode_family: TravelMode,
    pub ridership: f64,
    pub passenger_km: f64,
    pub vehicle_km: f64,
    pub vehicle_hours: f64,
    pub metrics: FinancialPerformanceMetrics,
    pub reliability_cost_uplift: f64,
    pub commercial_strength_classification: CommercialStrengthClassification,
    pub social_necessity_classification: SocialNecessityClassification,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorridorFinancialMetrics {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub demand_served: f64,
    pub passenger_km: f64,
    pub metrics: FinancialPerformanceMetrics,
    pub commercial_strength_classification: CommercialStrengthClassification,
    pub social_necessity_classification: SocialNecessityClassification,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StationFinancialContext {
    pub stop_id: String,
    pub boardings: f64,
    pub alightings: f64,
    pub associated_revenue: f64,
    pub operating_cost_burden_proxy: f64,
    pub capital_cost_burden_proxy: f64,
    pub strategic_value_proxy: f64,
    pub commercial_strength_classification: CommercialStrengthClassification,
    pub social_necessity_classification: SocialNecessityClassification,
    pub active_temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomicRankingEntry {
    pub id: String,
    pub score: f64,
    pub reason: String,
    pub temporal_slice: TemporalDemandSlice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalFinancialSummary {
    pub temporal_slice: TemporalDemandSlice,
    pub fare_revenue: f64,
    pub operating_cost: f64,
    pub total_cost: f64,
    pub subsidy_required: f64,
    pub farebox_recovery_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceDayFinancialSummary {
    pub service_day_type: ServiceDayType,
    pub fare_revenue: f64,
    pub operating_cost: f64,
    pub total_cost: f64,
    pub subsidy_required: f64,
    pub farebox_recovery_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomicDiagnostics {
    #[serde(default)]
    pub top_profitable_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_loss_making_high_ridership_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_subsidy_dependent_social_corridors: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_expensive_underperforming_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_reinvestment_worthy_corridors: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_socially_valuable_commercially_weak_links: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_revenue_generating_corridors: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub top_operating_cost_heavy_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub best_farebox_recovery_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub worst_full_cost_deficits_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub corridors_where_unreliability_hurts_finances: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub overloaded_highly_profitable_services: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub strongest_commercial_opportunities: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub strongest_social_necessity_corridors: Vec<EconomicRankingEntry>,
    #[serde(default)]
    pub network_financial_by_time_slice: Vec<TemporalFinancialSummary>,
    #[serde(default)]
    pub network_financial_by_day_type: Vec<ServiceDayFinancialSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZonePlanningMetrics {
    pub zone_id: String,
    pub settlement_class: SettlementClass,
    pub archetype: ZoneArchetype,
    pub population: f64,
    pub jobs: f64,
    pub centrality_score: f64,
    pub total_latent_produced: f64,
    pub total_latent_attracted: f64,
    pub total_realised_produced: f64,
    pub total_realised_attracted: f64,
    pub total_unserved_produced: f64,
    pub latent_to_realised_ratio: f64,
    pub access_to_jobs_score: f64,
    pub access_to_services_score: f64,
    pub access_to_education_score: f64,
    pub access_to_retail_leisure_score: f64,
    pub intercity_access_score: f64,
    pub composite_accessibility_score: f64,
    pub accessibility_score: f64,
    pub service_coverage_score: f64,
    #[serde(default)]
    pub dominant_trip_purpose: Option<TripPurpose>,
    #[serde(default)]
    pub top_destination_zones: Vec<ZoneFlowReference>,
    #[serde(default)]
    pub top_origin_zones: Vec<ZoneFlowReference>,
    pub strongest_corridor_score: f64,
    pub current_modeled_waiting_nearby: f64,
    pub current_boardings_nearby: f64,
    #[serde(default)]
    pub transit_capture_share: f64,
    #[serde(default)]
    pub car_capture_share: f64,
    #[serde(default)]
    pub walk_capture_share: f64,
    #[serde(default)]
    pub suppressed_share: f64,
    #[serde(default)]
    pub transit_captured_produced: f64,
    #[serde(default)]
    pub non_transit_captured_produced: f64,
    #[serde(default)]
    pub reliability_penalty_s: f64,
    #[serde(default)]
    pub operational_underservice_score: f64,
    #[serde(default)]
    pub transit_revenue_generated: f64,
    #[serde(default)]
    pub subsidy_need_proxy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StationPlanningMetrics {
    pub stop_id: String,
    pub catchment_population: f64,
    pub catchment_jobs: f64,
    pub catchment_education: f64,
    pub catchment_retail_leisure: f64,
    pub boardings_total: f64,
    pub alightings_total: f64,
    pub waiting_now: f64,
    pub denied_total: f64,
    pub arrivals_completed_total: f64,
    pub load_pressure_score: f64,
    pub overcrowding_risk_score: f64,
    pub service_frequency_proxy: f64,
    pub latent_demand_in_catchment: f64,
    pub realised_demand_in_catchment: f64,
    pub unserved_demand_in_catchment: f64,
    #[serde(default)]
    pub transit_captured_demand_in_catchment: f64,
    #[serde(default)]
    pub uncaptured_competing_demand_in_catchment: f64,
    #[serde(default)]
    pub transit_capture_share_in_catchment: f64,
    #[serde(default)]
    pub capture_limited_by_crowding: bool,
    #[serde(default)]
    pub average_dwell_time_s: f64,
    #[serde(default)]
    pub max_dwell_time_s: f64,
    #[serde(default)]
    pub platform_crowding_proxy: f64,
    #[serde(default)]
    pub transfer_success_rate: f64,
    #[serde(default)]
    pub operational_pressure_score: f64,
    #[serde(default)]
    pub headway_irregularity: f64,
    #[serde(default)]
    pub average_headway_realised_s: f64,
    #[serde(default)]
    pub associated_revenue: f64,
    #[serde(default)]
    pub operating_cost_burden_proxy: f64,
    #[serde(default)]
    pub capital_cost_burden_proxy: f64,
    #[serde(default)]
    pub strategic_value_proxy: f64,
    #[serde(default)]
    pub commercial_strength_classification: CommercialStrengthClassification,
    #[serde(default)]
    pub social_necessity_classification: SocialNecessityClassification,
    #[serde(default)]
    pub primary_trip_purposes_served: Vec<PurposeScoreValue>,
    #[serde(default)]
    pub top_destinations_from_station: Vec<StopFlowReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorridorPlanningMetrics {
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub dominant_purpose: TripPurpose,
    pub latent_volume: f64,
    pub realised_volume: f64,
    pub unserved_volume: f64,
    pub served_ratio: f64,
    #[serde(default)]
    pub average_generalized_cost_s: Option<f64>,
    #[serde(default)]
    pub directness_score: Option<f64>,
    pub corridor_classification: CorridorClassification,
    pub likely_mode_fit: RecommendedServiceClass,
    #[serde(default)]
    pub dominant_mode: TravelMode,
    #[serde(default)]
    pub transit_share: f64,
    #[serde(default)]
    pub car_share: f64,
    #[serde(default)]
    pub walk_share: f64,
    #[serde(default)]
    pub suppressed_share: f64,
    #[serde(default)]
    pub strongest_transit_submode: TravelMode,
    #[serde(default)]
    pub transit_capture_gap: f64,
    #[serde(default)]
    pub reliability_adjusted_service_quality: f64,
    #[serde(default)]
    pub recurring_bottleneck_score: f64,
    #[serde(default)]
    pub missed_transfer_sensitivity: f64,
    #[serde(default)]
    pub crowding_delay_pressure: f64,
    #[serde(default)]
    pub fare_revenue: f64,
    #[serde(default)]
    pub operating_cost_allocated: f64,
    #[serde(default)]
    pub total_cost_allocated: f64,
    #[serde(default)]
    pub subsidy_required: f64,
    #[serde(default)]
    pub farebox_recovery_ratio: f64,
    #[serde(default)]
    pub commercial_strength_classification: CommercialStrengthClassification,
    #[serde(default)]
    pub social_necessity_classification: SocialNecessityClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LineOrServicePlanningMetrics {
    pub service_id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    pub total_boardings: f64,
    pub passenger_km: f64,
    pub peak_load: f64,
    pub average_load: f64,
    #[serde(default)]
    pub max_load_point: Option<String>,
    pub overcrowded_segments: usize,
    #[serde(default)]
    pub strongest_origin_destination_patterns: Vec<OdPatternMetric>,
    pub role_classification: ServiceRoleClassification,
    pub utilisation_score: f64,
    #[serde(default)]
    pub service_mode_family: TravelMode,
    #[serde(default)]
    pub transit_captured_demand: f64,
    #[serde(default)]
    pub uncaptured_competing_demand_near_service: f64,
    #[serde(default)]
    pub crowding_lost_share_signal: f64,
    #[serde(default)]
    pub scheduled_headway_s: f64,
    #[serde(default)]
    pub realised_headway_s: f64,
    #[serde(default)]
    pub headway_irregularity: f64,
    #[serde(default)]
    pub average_delay_s: f64,
    #[serde(default)]
    pub max_delay_s: f64,
    #[serde(default)]
    pub average_dwell_s: f64,
    #[serde(default)]
    pub max_dwell_s: f64,
    #[serde(default)]
    pub bunching_risk_score: f64,
    #[serde(default)]
    pub reliability_score: f64,
    #[serde(default)]
    pub transfer_success_rate: f64,
    #[serde(default)]
    pub operational_pressure_score: f64,
    #[serde(default)]
    pub fare_revenue: f64,
    #[serde(default)]
    pub operating_cost: f64,
    #[serde(default)]
    pub infrastructure_cost_allocated: f64,
    #[serde(default)]
    pub rolling_stock_cost_allocated: f64,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub operating_surplus_deficit: f64,
    #[serde(default)]
    pub full_cost_surplus_deficit: f64,
    #[serde(default)]
    pub subsidy_required: f64,
    #[serde(default)]
    pub farebox_recovery_ratio: f64,
    #[serde(default)]
    pub cost_per_passenger: f64,
    #[serde(default)]
    pub cost_per_passenger_km: f64,
    #[serde(default)]
    pub revenue_per_passenger: f64,
    #[serde(default)]
    pub commercial_strength_classification: CommercialStrengthClassification,
    #[serde(default)]
    pub social_necessity_classification: SocialNecessityClassification,
    #[serde(default)]
    pub reliability_cost_pressure: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildPreviewMetrics {
    pub preview_id: String,
    pub preview_type: BuildPreviewType,
    #[serde(default)]
    pub affected_zones: Vec<String>,
    pub estimated_new_coverage_population: f64,
    pub estimated_new_coverage_jobs: f64,
    pub latent_demand_interceptable: f64,
    pub unserved_demand_addressable: f64,
    #[serde(default)]
    pub strongest_trip_purposes_unlocked: Vec<PurposeScoreValue>,
    #[serde(default)]
    pub strongest_corridors_touched: Vec<CorridorReference>,
    #[serde(default)]
    pub expected_nodes_affected: Vec<String>,
    pub accessibility_delta_proxy: f64,
    pub confidence: f64,
    pub explanation: String,
    #[serde(default)]
    pub estimated_revenue_uplift: f64,
    #[serde(default)]
    pub estimated_operating_cost_uplift: f64,
    #[serde(default)]
    pub estimated_capital_cost: f64,
    #[serde(default)]
    pub estimated_farebox_recovery: f64,
    #[serde(default)]
    pub likely_subsidy_requirement: f64,
    #[serde(default)]
    pub commercial_strength_classification: CommercialStrengthClassification,
    #[serde(default)]
    pub social_necessity_classification: SocialNecessityClassification,
    #[serde(default)]
    pub reinvestment_case_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StationScoreEntry {
    pub stop_id: String,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceScoreEntry {
    pub service_id: String,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceGapRankings {
    #[serde(default)]
    pub top_underserved_zones: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_underserved_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_overcrowded_stations: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_overcrowded_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_weak_access_rural_zones: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_high_potential_interventions: Vec<BuildPreviewMetrics>,
    #[serde(default)]
    pub top_transit_capture_opportunity_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_car_dominated_transit_viable_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_overcrowded_corridors_losing_mode_share: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_socially_important_low_demand_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_unreliable_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_dwell_pressure_stations: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_bunching_prone_lines: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_missed_transfer_interchanges: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_corridors_losing_capture_due_to_unreliability: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_operational_bottlenecks: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_profitable_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_loss_making_high_ridership_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_subsidy_dependent_social_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_expensive_underperforming_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_reinvestment_worthy_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_socially_valuable_commercially_weak_links: Vec<CorridorPlanningMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanningDebugSummary {
    #[serde(default)]
    pub top_underserved_zones_with_reasons: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_overcrowded_stations_with_causes: Vec<StationScoreEntry>,
    #[serde(default)]
    pub strongest_metro_suitable_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub strongest_intercity_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub strongest_rural_essential_gaps: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_candidate_interventions: Vec<BuildPreviewMetrics>,
    #[serde(default)]
    pub top_zones_by_transit_share: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_car_dominated_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub strongest_commuter_transit_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub strongest_intercity_rail_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub zones_losing_transit_due_to_transfers: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub zones_losing_transit_due_to_crowding: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub zones_where_parking_penalty_supports_transit: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_unreliable_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_dwell_pressure_stations: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_bunching_prone_lines: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_missed_transfer_interchanges: Vec<StationScoreEntry>,
    #[serde(default)]
    pub top_corridors_losing_capture_due_to_unreliability: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_nominally_frequent_but_poor_delivery_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub top_revenue_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub top_operating_cost_heavy_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub best_farebox_recovery_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub worst_full_cost_deficit_services: Vec<ServiceScoreEntry>,
    #[serde(default)]
    pub strongest_commercial_opportunities: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub strongest_social_necessity_corridors: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub corridors_where_unreliability_hurts_finances: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub overloaded_highly_profitable_services: Vec<ServiceScoreEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSliceDemandTotals {
    pub time_slice: DemandTimeSliceLabel,
    pub total_latent: f64,
    pub total_realised: f64,
    pub total_unserved: f64,
}

impl Default for TimeSliceDemandTotals {
    fn default() -> Self {
        Self {
            time_slice: DemandTimeSliceLabel::Interpeak,
            total_latent: 0.0,
            total_realised: 0.0,
            total_unserved: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StationFlowAggregate {
    pub stop_id: String,
    pub boarded: f64,
    pub alighted: f64,
    pub denied: f64,
    pub waiting: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceVehicleLoadAggregate {
    pub service_id: String,
    pub current_load: f64,
    pub max_load_seen: f64,
    pub capacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowConsistencyCheck {
    pub name: String,
    pub passed: bool,
    pub lhs: f64,
    pub rhs: f64,
    pub tolerance: f64,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemandDiagnostics {
    #[serde(default)]
    pub totals_by_time_slice: Vec<TimeSliceDemandTotals>,
    pub total_latent_demand: f64,
    pub total_realised_demand: f64,
    pub total_unserved_demand: f64,
    pub total_waiting_passengers_network: f64,
    #[serde(default)]
    pub boardings_alightings_by_station: Vec<StationFlowAggregate>,
    #[serde(default)]
    pub vehicle_loads_by_service: Vec<ServiceVehicleLoadAggregate>,
    #[serde(default)]
    pub top_od_pairs: Vec<AssignedOdFlow>,
    #[serde(default)]
    pub top_centrality_zones: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_work_attractors: Vec<ZoneScoreEntry>,
    #[serde(default)]
    pub top_intercity_pairs: Vec<CorridorDesireLineData>,
    #[serde(default)]
    pub strongest_commuter_corridors: Vec<CorridorDesireLineData>,
    #[serde(default)]
    pub strongest_rural_to_town_flows: Vec<CorridorDesireLineData>,
    #[serde(default)]
    pub strongest_anchor_flows: Vec<CorridorDesireLineData>,
    #[serde(default)]
    pub consistency_checks: Vec<FlowConsistencyCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PurposeTemporalDemandTotals {
    pub temporal_slice: TemporalDemandSlice,
    pub purpose: TripPurpose,
    pub latent: f64,
    pub realised: f64,
    pub unserved: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalStationPressurePoint {
    pub temporal_slice: TemporalDemandSlice,
    pub stop_id: String,
    pub waiting: f64,
    pub denied: f64,
    pub boarded: f64,
    pub alighted: f64,
    pub load_pressure_score: f64,
    pub overcrowding_risk_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalServicePressurePoint {
    pub temporal_slice: TemporalDemandSlice,
    pub service_id: String,
    #[serde(default)]
    pub line_id: Option<String>,
    pub peak_load: f64,
    pub average_load: f64,
    pub utilisation_score: f64,
    pub overcrowded_segments: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalCorridorPressurePoint {
    pub temporal_slice: TemporalDemandSlice,
    pub origin_zone_id: String,
    pub destination_zone_id: String,
    pub purpose: TripPurpose,
    pub latent_volume: f64,
    pub realised_volume: f64,
    pub unserved_volume: f64,
    pub served_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalServiceGapPoint {
    pub temporal_slice: TemporalDemandSlice,
    pub zone_id: String,
    pub total_unserved_demand: f64,
    pub latent_vs_realised_ratio: f64,
    #[serde(default)]
    pub accessibility_score: Option<f64>,
    #[serde(default)]
    pub service_coverage_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalRankingEntry {
    pub temporal_slice: TemporalDemandSlice,
    pub id: String,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalDemandDiagnostics {
    #[serde(default)]
    pub purpose_totals: Vec<PurposeTemporalDemandTotals>,
    #[serde(default)]
    pub station_pressure: Vec<TemporalStationPressurePoint>,
    #[serde(default)]
    pub service_pressure: Vec<TemporalServicePressurePoint>,
    #[serde(default)]
    pub corridor_pressure: Vec<TemporalCorridorPressurePoint>,
    #[serde(default)]
    pub service_gap_summaries: Vec<TemporalServiceGapPoint>,
    #[serde(default)]
    pub latent_to_realised_ratio_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub top_overloaded_stations_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub top_overloaded_services_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub strongest_corridors_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub peak_waiting_by_station_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub peak_denied_by_station_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub peak_corridor_unserved_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub peak_line_overload_by_slice: Vec<TemporalRankingEntry>,
    #[serde(default)]
    pub overload_flip_classifications: Vec<TemporalRankingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalPlanningSnapshot {
    pub temporal_slice: TemporalDemandSlice,
    #[serde(default)]
    pub zone_planning_metrics: Vec<ZonePlanningMetrics>,
    #[serde(default)]
    pub station_planning_metrics: Vec<StationPlanningMetrics>,
    #[serde(default)]
    pub corridor_planning_metrics: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub line_service_planning_metrics: Vec<LineOrServicePlanningMetrics>,
    #[serde(default)]
    pub service_gap_rankings: ServiceGapRankings,
    #[serde(default)]
    pub network_financial_summary: NetworkFinancialSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationOutput {
    pub meta: OutputMeta,
    pub kpis: Kpis,
    pub link_loads: Vec<LinkLoad>,
    pub board_loads: Vec<BoardLoad>,
    #[serde(default)]
    pub stop_flows: Vec<StopFlow>,
    #[serde(default)]
    pub passenger_cohorts: Vec<PassengerCohortFlow>,
    #[serde(default)]
    pub fare_flow: FareFlowSummary,
    #[serde(default)]
    pub zone_demand_profiles: Vec<ZoneDemandProfile>,
    #[serde(default)]
    pub latent_od_demand: Vec<LatentOdDemand>,
    #[serde(default)]
    pub assigned_od_flows: Vec<AssignedOdFlow>,
    #[serde(default)]
    pub mode_choice_results: Vec<ModeChoiceResult>,
    #[serde(default)]
    pub stop_flow_states: Vec<StopFlowState>,
    #[serde(default)]
    pub vehicle_load_states: Vec<VehicleLoadState>,
    #[serde(default)]
    pub service_operation_states: Vec<ServiceOperationState>,
    #[serde(default)]
    pub stop_operation_states: Vec<StopOperationState>,
    #[serde(default)]
    pub transfer_operation_metrics: Vec<TransferOperationMetrics>,
    #[serde(default)]
    pub service_reliability_diagnostics: ServiceReliabilityDiagnostics,
    #[serde(default)]
    pub synthetic_economy_config: Option<SyntheticEconomyConfig>,
    #[serde(default)]
    pub zone_demand_layer: Vec<ZoneDemandLayerData>,
    #[serde(default)]
    pub zone_economic_geography_layer: Vec<ZoneEconomicGeographyLayerData>,
    #[serde(default)]
    pub zone_demand_production_layer: Vec<ZoneDemandProductionLayerData>,
    #[serde(default)]
    pub zone_demand_attraction_layer: Vec<ZoneDemandAttractionLayerData>,
    #[serde(default)]
    pub corridor_desire_lines: Vec<CorridorDesireLineData>,
    #[serde(default)]
    pub service_gap_layer: Vec<ZoneServiceGapLayerData>,
    #[serde(default)]
    pub service_load_layer: Vec<ServiceLoadLayerData>,
    #[serde(default)]
    pub planning_overlay_config: Option<PlanningOverlayConfig>,
    #[serde(default)]
    pub zone_planning_metrics: Vec<ZonePlanningMetrics>,
    #[serde(default)]
    pub station_planning_metrics: Vec<StationPlanningMetrics>,
    #[serde(default)]
    pub corridor_planning_metrics: Vec<CorridorPlanningMetrics>,
    #[serde(default)]
    pub line_service_planning_metrics: Vec<LineOrServicePlanningMetrics>,
    #[serde(default)]
    pub network_financial_summary: NetworkFinancialSummary,
    #[serde(default)]
    pub service_financial_metrics: Vec<ServiceFinancialMetrics>,
    #[serde(default)]
    pub corridor_financial_metrics: Vec<CorridorFinancialMetrics>,
    #[serde(default)]
    pub station_financial_context: Vec<StationFinancialContext>,
    #[serde(default)]
    pub zone_mode_share_metrics: Vec<ZoneModeShareMetrics>,
    #[serde(default)]
    pub corridor_mode_share_metrics: Vec<CorridorModeShareMetrics>,
    #[serde(default)]
    pub station_transit_capture_context: Vec<StationTransitCaptureContext>,
    #[serde(default)]
    pub service_transit_capture_context: Vec<ServiceTransitCaptureContext>,
    #[serde(default)]
    pub citywide_mode_share_summary: CitywideModeShareSummary,
    #[serde(default)]
    pub build_preview_metrics: Vec<BuildPreviewMetrics>,
    #[serde(default)]
    pub service_gap_rankings: ServiceGapRankings,
    #[serde(default)]
    pub planning_debug_summary: PlanningDebugSummary,
    #[serde(default)]
    pub demand_diagnostics: DemandDiagnostics,
    #[serde(default)]
    pub active_temporal_slice: TemporalDemandSlice,
    #[serde(default)]
    pub temporal_planning_snapshots: Vec<TemporalPlanningSnapshot>,
    #[serde(default)]
    pub temporal_demand_diagnostics: TemporalDemandDiagnostics,
    #[serde(default)]
    pub modal_demand_diagnostics: ModalDemandDiagnostics,
    #[serde(default)]
    pub economic_diagnostics: EconomicDiagnostics,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMeta {
    pub results_version: String,
    pub scenario_name: String,
    pub seed: u64,
    pub time_period_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSettings {
    pub k_paths: usize,
    pub route_choice_theta: f64,
    pub msa_max_iters: usize,
    pub convergence_rel: f64,

    // Discrete time step for within-period boarding/queue dynamics (game bridge)
    pub time_bin_s: f64,
    #[serde(default)]
    pub lightweight_outputs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardingTimeBin {
    pub bin_index: usize,
    pub arrivals: f64,
    pub served: f64,
    pub queue_end: f64,
    pub departures: usize,
    pub capacity: f64,
}

impl SimulationSettings {
    pub fn from_params(p: &Params) -> Self {
        Self {
            k_paths: p.route_choice_k.max(1),
            route_choice_theta: p.route_choice_theta,
            msa_max_iters: p.assignment_max_iters.max(1),
            convergence_rel: p.assignment_convergence_rel.max(0.0),

            // 5-minute bins by default (good for peak modelling + game ticks later)
            time_bin_s: 300.0,
            lightweight_outputs: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kpis {
    // Trips (OD demand)
    pub total_trips_attempted: f64,
    pub total_trips_served: f64,
    pub share_trips_served: f64,

    pub total_trips: f64,

    pub mean_generalized_cost_s: f64,
    pub mean_in_vehicle_time_s: f64,
    pub mean_wait_time_s: f64,
    pub mean_walk_time_s: f64,

    pub mean_transfer_time_s: f64,
    pub mean_transfer_penalty_s: f64,
    pub mean_transfers: f64,

    pub mean_boardings: f64,

    // Boarding/capacity summary (service/stop board edges)
    pub total_boardings_attempted: f64,
    pub total_boardings_served: f64,
    pub total_boardings_denied: f64,
    pub share_boardings_served: f64,
    #[serde(default)]
    pub total_fare_revenue_base: f64,
    #[serde(default)]
    pub total_overflow_dropped: f64,
    #[serde(default)]
    pub share_demand_overflow_dropped: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkLoad {
    pub link_id: String,
    pub from_stop: String,
    pub to_stop: String,
    pub mode: String,

    pub passengers: f64,

    // --- NEW: crowding diagnostics ---
    pub capacity_per_hour: Option<f64>,
    pub capacity_in_period: f64, // capacity_per_hour * time_period_hours
    pub load_to_capacity: f64,   // passengers / capacity_in_period
    pub crowding_penalty_s: f64, // what your crowding function returns for this link
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardLoad {
    pub service_id: String,
    pub stop_id: String,

    pub arrivals: f64,
    pub served_from_arrivals: f64,
    pub served_from_queue: f64,
    pub denied_boardings: f64,
    pub queue_start: f64,
    pub queue_end: f64,

    pub headway_s: f64,
    pub vehicle_capacity: f64,
    pub departures_in_period: f64,
    #[serde(default)]
    pub departures_observed: usize,
    pub capacity_in_period: f64,

    pub extra_wait_s: f64,

    // NEW: time-sliced queue evolution
    pub time_bins: Vec<BoardingTimeBin>,
    pub time_to_next_departure_s_end: f64,
    #[serde(default)]
    pub alightings_served: f64,
    #[serde(default)]
    pub station_capacity_boarding_pph: f64,
    #[serde(default)]
    pub station_capacity_alighting_pph: f64,
    #[serde(default)]
    pub station_queue_capacity_pax: f64,
    #[serde(default)]
    pub overflow_dropped: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopFlow {
    pub stop_id: String,
    pub boardings_attempted: f64,
    pub boardings_served: f64,
    pub alightings_attempted: f64,
    pub alightings_served: f64,
    pub queue_start: f64,
    pub queue_end: f64,
    pub overflow_dropped: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassengerCohortFlow {
    pub service_id: String,
    pub board_stop_id: String,
    pub destination_stop_id: String,
    pub attempted_pax: f64,
    pub boarded_pax: f64,
    pub alighted_pax: f64,
    pub queue_end_pax: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FareFlowSummary {
    #[serde(default)]
    pub liability_accrued_base: f64,
    #[serde(default)]
    pub liability_accrued_pax: f64,
    #[serde(default)]
    pub completed_journeys_pax: f64,
    #[serde(default)]
    pub recognized_revenue_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostics {
    pub zones: usize,
    pub stops: usize,
    pub links: usize,
    pub services: usize,
    pub transfers: usize,
    pub access_edges: usize,
    pub egress_edges: usize,

    // --- NEW: assignment diagnostics ---
    pub msa_iterations: usize,
    pub msa_final_max_rel_change: f64,

    // --- NEW: route-choice debug samples ---
    #[serde(default)]
    pub sample_paths: Vec<SampleOdPaths>,
}
