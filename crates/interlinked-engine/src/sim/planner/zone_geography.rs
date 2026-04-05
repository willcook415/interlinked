use super::*;
use crate::sim::types;

pub(super) fn build_zone_demand_profiles(
    s: &Scenario,
    cfg: &SyntheticEconomyConfig,
) -> Vec<ZoneDemandProfile> {
    let by_cell = s
        .world
        .demand_cells
        .iter()
        .map(|c| (c.cell_id.as_str(), c))
        .collect::<HashMap<_, _>>();
    let max_pop = s
        .world
        .zones
        .iter()
        .map(|z| z.population.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_jobs = s
        .world
        .zones
        .iter()
        .map(|z| z.jobs.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_mass = s
        .world
        .zones
        .iter()
        .map(|z| z.population.max(0.0) + z.jobs.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut profiles = s
        .world
        .zones
        .iter()
        .map(|z| {
            let pop = z.population.max(0.0);
            let jobs = z.jobs.max(0.0);
            let mut activity_mix = [0.52, 0.18, 0.12, 0.08, 0.06, 0.03, 0.01];
            let mut centrality_score = (jobs / (pop + jobs + 1.0)).clamp(0.0, 1.0);
            let area_m2 = if let Some(cell) = by_cell.get(z.id.as_str()) {
                activity_mix = normalize_activity_mix([
                    cell.activity_mix_residential,
                    cell.activity_mix_office,
                    cell.activity_mix_retail,
                    cell.activity_mix_recreation,
                    cell.activity_mix_industrial,
                    cell.activity_mix_education,
                    cell.activity_mix_health,
                ]);
                centrality_score = cell.centrality_score.clamp(0.0, 1.0);
                cell.area_m2.max(1_000_000.0)
            } else {
                1_000_000.0
            };

            let area_km2 = (area_m2 / 1_000_000.0).max(0.05);
            let population_density = pop / area_km2;
            let employment_density = jobs / area_km2;
            let retail_intensity =
                (activity_mix[2] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let leisure_intensity =
                (activity_mix[3] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let education_intensity =
                (activity_mix[5] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let industry_intensity =
                (activity_mix[4] * (pop + jobs) / (max_mass + 1.0)).clamp(0.0, 1.5);
            let pop_norm = pop / (max_pop + 1.0);
            let jobs_norm = jobs / (max_jobs + 1.0);
            let centrality_term =
                (centrality_score * cfg.centrality_weight.max(0.05)).clamp(0.0, 2.0);
            let regional_importance =
                ((0.46 * jobs_norm + 0.26 * pop_norm + 0.28 * centrality_term)
                    * cfg.regional_importance_weight.max(0.05))
                .clamp(0.0, 2.0);
            let regional_term =
                (regional_importance * cfg.regional_importance_weight.max(0.05)).clamp(0.0, 2.0);
            let tourism_score =
                (0.45 * leisure_intensity + 0.25 * retail_intensity + 0.30 * centrality_term)
                    .clamp(0.0, 1.5);

            let archetype = classify_zone_archetype(
                z.id.as_str(),
                activity_mix,
                centrality_score,
                population_density,
                employment_density,
                pop,
                jobs,
            );
            let settlement_class =
                classify_settlement_class(pop + jobs, centrality_score, archetype);
            let special_attractors = infer_special_attractors(
                z.id.as_str(),
                archetype,
                settlement_class,
                activity_mix,
                centrality_score,
                tourism_score,
            );

            let trait_cfg = archetype_trait(archetype, cfg);
            let settlement_work =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Work, cfg);
            let settlement_education =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Education, cfg);
            let settlement_shopping =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Shopping, cfg);
            let settlement_leisure =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Leisure, cfg);
            let settlement_essential =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Essential, cfg);
            let settlement_intercity =
                settlement_purpose_multiplier(settlement_class, TripPurpose::Intercity, cfg);

            let base_transit = match settlement_class {
                SettlementClass::GlobalCityCore => 0.92,
                SettlementClass::MajorCity => 0.85,
                SettlementClass::RegionalCity => 0.74,
                SettlementClass::LargeTown => 0.63,
                SettlementClass::SmallTown => 0.53,
                SettlementClass::Village => 0.42,
                SettlementClass::Rural => 0.30,
                SettlementClass::SpecialNode => 0.68,
            };
            let transit_affinity =
                (base_transit + 0.18 * centrality_score + 0.08 * trait_cfg.centrality_weight)
                    .clamp(0.05, 0.99);
            let car_dependency = (1.0 - transit_affinity + 0.12).clamp(0.01, 0.99);

            let work_attractiveness = ((0.52 * jobs_norm
                + 0.24 * (employment_density / (employment_density + 8000.0))
                + 0.16 * centrality_term
                + 0.08 * regional_term)
                * settlement_work
                * (0.8 + trait_cfg.employment_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Work, cfg))
            .max(0.02);
            let education_attractiveness = ((0.42 * education_intensity
                + 0.24 * centrality_term
                + 0.16 * pop_norm
                + 0.18 * regional_term)
                * settlement_education
                * (0.7 + trait_cfg.education_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Education, cfg))
            .max(0.02);
            let shopping_attractiveness = ((0.46 * retail_intensity
                + 0.22 * centrality_term
                + 0.16 * pop_norm
                + 0.16 * regional_term)
                * settlement_shopping
                * (0.7 + trait_cfg.retail_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Shopping, cfg))
            .max(0.02);
            let leisure_attractiveness = ((0.38 * leisure_intensity
                + 0.30 * tourism_score
                + 0.14 * centrality_term
                + 0.18 * regional_term)
                * settlement_leisure
                * (0.65 + trait_cfg.leisure_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Leisure, cfg))
            .max(0.02);
            let essential_service_attractiveness = ((0.42 * pop_norm
                + 0.20 * education_intensity
                + 0.14 * centrality_term
                + 0.24 * regional_term)
                * settlement_essential
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Essential, cfg))
            .max(0.02);
            let intercity_importance = ((0.30 * jobs_norm
                + 0.30 * centrality_term
                + 0.40 * regional_term)
                * settlement_intercity
                * (0.7 + trait_cfg.centrality_weight)
                * zone_attractor_multiplier_raw(&special_attractors, TripPurpose::Intercity, cfg))
            .max(0.02);

            let trip_rate_modifiers = types::PurposeTripRateModifiers {
                work: (0.68
                    + 0.20 * transit_affinity
                    + 0.24 * settlement_work
                    + 0.12 * trait_cfg.residential_weight)
                    .clamp(0.15, 2.0),
                education: (0.64
                    + 0.20 * transit_affinity
                    + 0.24 * settlement_education
                    + 0.10 * education_intensity)
                    .clamp(0.15, 2.0),
                shopping: (0.58
                    + 0.18 * (1.0 - car_dependency)
                    + 0.24 * settlement_shopping
                    + 0.10 * retail_intensity)
                    .clamp(0.15, 2.0),
                leisure: (0.52
                    + 0.20 * tourism_score
                    + 0.18 * settlement_leisure
                    + 0.12 * leisure_intensity)
                    .clamp(0.15, 2.0),
                essential: (0.48 + 0.24 * settlement_essential + 0.22 * (1.0 - car_dependency))
                    .clamp(0.15, 2.0),
                intercity: (0.30 + 0.52 * intercity_importance + 0.18 * settlement_intercity)
                    .clamp(0.15, 2.0),
            };

            ZoneDemandProfile {
                zone_id: z.id.clone(),
                population: pop,
                jobs,
                archetype,
                settlement_class,
                population_density,
                employment_density,
                retail_intensity,
                leisure_intensity,
                education_intensity,
                industry_intensity,
                centrality_score,
                regional_importance,
                tourism_score,
                car_dependency,
                transit_affinity,
                nearest_service_centre_id: None,
                special_attractors,
                trip_rate_modifiers,
                work_attractiveness,
                education_attractiveness,
                shopping_attractiveness,
                leisure_attractiveness,
                essential_service_attractiveness,
                intercity_importance,
            }
        })
        .collect::<Vec<_>>();

    // Find nearest service centre for each zone (town centre, city core, or special node).
    let mut service_centres: Vec<usize> = Vec::new();
    for (idx, p) in profiles.iter().enumerate() {
        if is_service_centre(p) {
            service_centres.push(idx);
        }
    }
    if service_centres.is_empty() && !profiles.is_empty() {
        let mut best_idx = 0usize;
        let mut best_score = f64::NEG_INFINITY;
        for (idx, p) in profiles.iter().enumerate() {
            let score = p.essential_service_attractiveness + p.regional_importance;
            if score > best_score {
                best_score = score;
                best_idx = idx;
            }
        }
        service_centres.push(best_idx);
    }
    for i in 0..profiles.len() {
        let mut best: Option<(usize, f64)> = None;
        for j in &service_centres {
            let dist = euclid_m(
                (s.world.zones[i].x, s.world.zones[i].y),
                (s.world.zones[*j].x, s.world.zones[*j].y),
            );
            if let Some((_, best_dist)) = best {
                if dist < best_dist {
                    best = Some((*j, dist));
                }
            } else {
                best = Some((*j, dist));
            }
        }
        profiles[i].nearest_service_centre_id = best.map(|(idx, _)| profiles[idx].zone_id.clone());
    }

    profiles
}

pub(super) fn settlement_rank(class: SettlementClass) -> i32 {
    match class {
        SettlementClass::GlobalCityCore => 8,
        SettlementClass::MajorCity => 7,
        SettlementClass::RegionalCity => 6,
        SettlementClass::SpecialNode => 5,
        SettlementClass::LargeTown => 4,
        SettlementClass::SmallTown => 3,
        SettlementClass::Village => 2,
        SettlementClass::Rural => 1,
    }
}

fn classify_settlement_class(
    settlement_mass: f64,
    centrality_score: f64,
    archetype: ZoneArchetype,
) -> SettlementClass {
    if matches!(
        archetype,
        ZoneArchetype::AirportZone | ZoneArchetype::PortLogisticsZone
    ) && settlement_mass >= 6_000.0
    {
        return SettlementClass::SpecialNode;
    }
    if centrality_score >= 0.92 && settlement_mass >= 80_000.0 {
        return SettlementClass::GlobalCityCore;
    }
    if settlement_mass >= 45_000.0 || (centrality_score >= 0.84 && settlement_mass >= 30_000.0) {
        return SettlementClass::MajorCity;
    }
    if settlement_mass >= 20_000.0 || (centrality_score >= 0.70 && settlement_mass >= 12_000.0) {
        return SettlementClass::RegionalCity;
    }
    if settlement_mass >= 8_000.0 {
        return SettlementClass::LargeTown;
    }
    if settlement_mass >= 3_500.0 {
        return SettlementClass::SmallTown;
    }
    if settlement_mass >= 1_200.0 {
        return SettlementClass::Village;
    }
    SettlementClass::Rural
}

fn classify_zone_archetype(
    zone_id: &str,
    mix: [f64; 7],
    centrality_score: f64,
    population_density: f64,
    employment_density: f64,
    population: f64,
    jobs: f64,
) -> ZoneArchetype {
    let id = zone_id.to_ascii_lowercase();
    if id.contains("airport") || id.contains("airfield") {
        return ZoneArchetype::AirportZone;
    }
    if id.contains("port") || id.contains("harbour") || id.contains("harbor") {
        return ZoneArchetype::PortLogisticsZone;
    }
    if mix[5] >= 0.26 {
        return ZoneArchetype::UniversityDistrict;
    }
    if centrality_score >= 0.90 && mix[1] >= 0.28 && jobs >= population * 1.1 {
        return ZoneArchetype::Cbd;
    }
    if mix[4] >= 0.35 {
        return ZoneArchetype::IndustrialEstate;
    }
    if mix[1] >= 0.33 && centrality_score >= 0.55 {
        return ZoneArchetype::BusinessPark;
    }
    if (mix[2] + mix[3]) >= 0.40 && centrality_score >= 0.55 {
        return ZoneArchetype::RetailLeisureDistrict;
    }
    if mix[2] >= 0.22 && centrality_score >= 0.45 {
        return ZoneArchetype::TownCentre;
    }
    if population_density >= 5_500.0 && mix[0] >= 0.45 {
        return ZoneArchetype::InnerResidential;
    }
    if population_density >= 2_500.0 && mix[0] >= 0.42 {
        return ZoneArchetype::OuterSuburb;
    }
    if population_density >= 900.0 && mix[0] >= 0.40 {
        return ZoneArchetype::VillageCentre;
    }
    if mix[0] >= 0.40 || employment_density >= 350.0 {
        return ZoneArchetype::RuralResidential;
    }
    ZoneArchetype::RuralAgricultural
}

fn infer_special_attractors(
    zone_id: &str,
    archetype: ZoneArchetype,
    settlement_class: SettlementClass,
    mix: [f64; 7],
    centrality_score: f64,
    tourism_score: f64,
) -> Vec<SpecialAttractorType> {
    let id = zone_id.to_ascii_lowercase();
    let mut attractors: Vec<SpecialAttractorType> = Vec::new();
    if matches!(archetype, ZoneArchetype::AirportZone) {
        attractors.push(SpecialAttractorType::Airport);
    }
    if matches!(archetype, ZoneArchetype::PortLogisticsZone) {
        attractors.push(SpecialAttractorType::Port);
        attractors.push(SpecialAttractorType::LogisticsHub);
    }
    if matches!(archetype, ZoneArchetype::UniversityDistrict) || mix[5] >= 0.30 {
        attractors.push(SpecialAttractorType::University);
    }
    if mix[6] >= 0.16 || id.contains("hospital") || id.contains("medical") {
        attractors.push(SpecialAttractorType::Hospital);
    }
    if mix[3] >= 0.38 || id.contains("stadium") {
        attractors.push(SpecialAttractorType::Stadium);
    }
    if tourism_score >= 0.65 || id.contains("tour") {
        attractors.push(SpecialAttractorType::TourismLandmark);
    }
    if matches!(archetype, ZoneArchetype::Cbd)
        && settlement_rank(settlement_class) >= settlement_rank(SettlementClass::RegionalCity)
    {
        attractors.push(SpecialAttractorType::GovernmentCentre);
    }
    if mix[4] >= 0.30 && centrality_score >= 0.50 {
        attractors.push(SpecialAttractorType::LogisticsHub);
    }

    attractors.sort_unstable();
    attractors.dedup();
    attractors
}

fn archetype_trait(
    archetype: ZoneArchetype,
    cfg: &SyntheticEconomyConfig,
) -> types::ArchetypeTraitConfig {
    if let Some(found) = cfg
        .archetype_traits
        .iter()
        .find(|x| x.archetype == archetype)
    {
        return found.clone();
    }
    types::ArchetypeTraitConfig {
        archetype,
        residential_weight: 0.9,
        employment_weight: 0.9,
        retail_weight: 0.9,
        leisure_weight: 0.9,
        education_weight: 0.9,
        industry_weight: 0.9,
        centrality_weight: 0.9,
    }
}

pub(super) fn settlement_purpose_multiplier(
    class: SettlementClass,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let record = cfg
        .settlement_class_multipliers
        .iter()
        .find(|x| x.settlement_class == class);
    let m = if let Some(v) = record {
        match purpose {
            TripPurpose::Work => v.work,
            TripPurpose::Education => v.education,
            TripPurpose::Shopping => v.shopping,
            TripPurpose::Leisure => v.leisure,
            TripPurpose::Essential => v.essential,
            TripPurpose::Intercity => v.intercity,
        }
    } else {
        1.0
    };
    m.max(0.05)
}

fn zone_attractor_multiplier_raw(
    attractors: &[SpecialAttractorType],
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let mut mult = 1.0_f64;
    for attractor in attractors {
        if let Some(record) = cfg
            .attractor_strength_multipliers
            .iter()
            .find(|x| x.attractor_type == *attractor)
        {
            let m = match purpose {
                TripPurpose::Work => record.work,
                TripPurpose::Education => record.education,
                TripPurpose::Shopping => record.shopping,
                TripPurpose::Leisure => record.leisure,
                TripPurpose::Essential => record.essential,
                TripPurpose::Intercity => record.intercity,
            };
            mult *= m.max(0.05);
        }
    }
    mult.clamp(0.2, 8.0)
}

pub(super) fn zone_attractor_multiplier(
    zone: &ZoneDemandProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    zone_attractor_multiplier_raw(&zone.special_attractors, purpose, cfg)
}

fn is_service_centre(zone: &ZoneDemandProfile) -> bool {
    if matches!(
        zone.settlement_class,
        SettlementClass::GlobalCityCore
            | SettlementClass::MajorCity
            | SettlementClass::RegionalCity
            | SettlementClass::LargeTown
            | SettlementClass::SpecialNode
    ) {
        return true;
    }
    matches!(
        zone.archetype,
        ZoneArchetype::Cbd | ZoneArchetype::TownCentre
    ) || zone.special_attractors.iter().any(|a| {
        matches!(
            a,
            SpecialAttractorType::Hospital | SpecialAttractorType::GovernmentCentre
        )
    })
}

pub(super) fn euclid_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

pub(super) fn euclid_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    euclid_m(a, b) / 1000.0
}

fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
    let mut cleaned = [0.0; 7];
    let mut sum = 0.0_f64;
    for (idx, value) in values.iter().enumerate() {
        let v = if value.is_finite() && *value >= 0.0 {
            *value
        } else {
            0.0
        };
        cleaned[idx] = v;
        sum += v;
    }
    if sum > 0.0 {
        cleaned.map(|v| v / sum)
    } else {
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    }
}
