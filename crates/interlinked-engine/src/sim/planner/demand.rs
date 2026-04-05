use super::*;

pub(super) fn build_latent_demand_foundation(
    s: &Scenario,
    base_graph: &super::graph::Graph,
    clock_s: f64,
    temporal_context: Option<TemporalDemandSlice>,
    coverage_mode: LatentDemandCoverage,
) -> Result<LatentDemandBuild, String> {
    let economy_cfg = SyntheticEconomyConfig::default();
    let zone_count = s.world.zones.len();
    let purpose_shares = normalized_purpose_shares(&s.params);
    let purpose_shares6 = [
        (purpose_shares[0].max(0.0)) * economy_cfg.purpose_trip_rates.work.max(0.0),
        (purpose_shares[1].max(0.0)) * economy_cfg.purpose_trip_rates.education.max(0.0),
        (purpose_shares[2].max(0.0)) * economy_cfg.purpose_trip_rates.shopping.max(0.0),
        (purpose_shares[3].max(0.0)) * economy_cfg.purpose_trip_rates.leisure.max(0.0),
        (purpose_shares[4] * 0.65).max(0.0) * economy_cfg.purpose_trip_rates.essential.max(0.0),
        (purpose_shares[4] * 0.35).max(0.0) * economy_cfg.purpose_trip_rates.intercity.max(0.0),
    ];
    let zone_profiles = build_zone_demand_profiles(s, &economy_cfg);
    let mut active_context = temporal_context.unwrap_or(TemporalDemandSlice {
        service_day_type: ServiceDayType::Weekday,
        time_slice: demand_time_slice_at(clock_s),
        seasonal_profile: SeasonalProfile::Neutral,
        active_event_ids: Vec::new(),
    });
    if active_context.active_event_ids.is_empty() {
        active_context.active_event_ids =
            resolve_active_event_ids(&active_context, &economy_cfg, &zone_profiles);
    } else {
        active_context.active_event_ids =
            normalize_active_event_ids(&active_context.active_event_ids, &economy_cfg);
    }

    let mut contexts = match coverage_mode {
        LatentDemandCoverage::SingleContext => vec![active_context.clone()],
        LatentDemandCoverage::CanonicalSlicesForDay => DEMAND_SLICE_WINDOWS
            .iter()
            .map(|window| TemporalDemandSlice {
                service_day_type: active_context.service_day_type,
                time_slice: window.label,
                seasonal_profile: active_context.seasonal_profile,
                active_event_ids: Vec::new(),
            })
            .collect::<Vec<_>>(),
    };
    for ctx in &mut contexts {
        if ctx.active_event_ids.is_empty() {
            ctx.active_event_ids = resolve_active_event_ids(ctx, &economy_cfg, &zone_profiles);
        } else {
            ctx.active_event_ids = normalize_active_event_ids(&ctx.active_event_ids, &economy_cfg);
        }
    }

    let zone_coords = s.world.zones.iter().map(|z| (z.x, z.y)).collect::<Vec<_>>();

    // Use base generalized costs (without iterated crowding) as one component of impedance.
    // When no path exists in current network, we still generate latent demand from spatial
    // impedance so assignment can correctly report that demand as unserved.
    let mut dist_by_origin = Vec::with_capacity(zone_count);
    for oi in 0..zone_count {
        let origin_node = base_graph.svc_index.zone_nodes_start + oi;
        dist_by_origin.push(dijkstra(base_graph, origin_node));
    }

    let mut all_latent: Vec<LatentOdDemand> = Vec::new();
    let mut active_latent: Vec<ActiveLatentOd> = Vec::new();
    for context in &contexts {
        let is_active_context = context.time_slice == active_context.time_slice
            && context.service_day_type == active_context.service_day_type
            && context.seasonal_profile == active_context.seasonal_profile;
        let active_events =
            active_event_modifiers_for_context(context, &economy_cfg, &zone_profiles);
        for oi in 0..zone_count {
            let origin_zone = &s.world.zones[oi];
            let profile = &zone_profiles[oi];
            let produced_total =
                origin_zone.population.max(0.0) * s.params.trips_per_person.max(0.0);
            if produced_total <= 0.0 {
                continue;
            }

            for (purpose_idx, purpose) in TripPurpose::ALL.iter().enumerate() {
                let purpose_share = purpose_shares6[purpose_idx].max(0.0);
                if purpose_share <= 0.0 {
                    continue;
                }
                let mut produced = produced_total
                    * purpose_share
                    * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                    * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                    * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg)
                    * purpose_trip_modifier(&profile.trip_rate_modifiers, *purpose);
                produced *= origin_production_factor(profile, *purpose, &economy_cfg).max(0.0);

                // Keep rural/village zones alive with a persistent essential floor.
                if matches!(
                    profile.settlement_class,
                    SettlementClass::Village | SettlementClass::Rural
                ) && *purpose == TripPurpose::Essential
                {
                    let floor = profile.population.max(0.0)
                        * economy_cfg.rural_essential_demand_floor_per_person.max(0.0)
                        * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                        * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                        * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg);
                    produced = produced.max(floor);
                } else if matches!(
                    profile.settlement_class,
                    SettlementClass::Village | SettlementClass::Rural
                ) {
                    let floor = profile.population.max(0.0)
                        * economy_cfg.rural_baseline_trip_floor_per_person.max(0.0)
                        * 0.25
                        * time_slice_multiplier(context.time_slice, *purpose, &economy_cfg)
                        * day_type_multiplier(context.service_day_type, *purpose, &economy_cfg)
                        * seasonal_multiplier(context.seasonal_profile, *purpose, &economy_cfg);
                    produced = produced.max(floor);
                }

                if produced <= 0.0 {
                    continue;
                }

                let mut weights: Vec<(usize, f64, f64, Vec<String>)> = Vec::new();
                let mut base_wsum = 0.0_f64;
                let mut weighted_event_wsum = 0.0_f64;
                let mut wsum = 0.0_f64;
                for dj in 0..zone_count {
                    if oi == dj {
                        continue;
                    }
                    let attraction =
                        purpose_attraction(dj, *purpose, &zone_profiles, &economy_cfg).max(0.0);
                    if attraction <= 0.0 {
                        continue;
                    }
                    let dest_node = base_graph.svc_index.zone_nodes_start + zone_count + dj;
                    let gc_s = dist_by_origin[oi][dest_node].dist;
                    let dist_km = euclid_km(zone_coords[oi], zone_coords[dj]).max(0.01);
                    let gc_effective_s = if gc_s.is_finite() {
                        gc_s.max(1.0)
                    } else {
                        // Synthetic fallback if no viable path exists yet.
                        (dist_km / 40.0) * 3600.0 + 600.0
                    };
                    let impedance = purpose_impedance(
                        *purpose,
                        gc_effective_s,
                        dist_km,
                        s.params.gravity_beta.max(0.0),
                        &economy_cfg,
                    );
                    let corridor_mult =
                        corridor_bonus(oi, dj, *purpose, &zone_profiles, &economy_cfg, dist_km);
                    let service_center_mult =
                        rural_service_center_bias(profile, &zone_profiles[dj], *purpose);
                    let base_w = attraction * impedance * corridor_mult * service_center_mult;
                    if base_w <= 0.0 {
                        continue;
                    }
                    let (event_mult, event_ids) = pair_event_multiplier(
                        *purpose,
                        profile,
                        &zone_profiles[dj],
                        &active_events,
                        &economy_cfg,
                    );
                    let w = base_w * event_mult;
                    if w > 0.0 {
                        base_wsum += base_w;
                        weighted_event_wsum += base_w * event_mult;
                        wsum += w;
                        weights.push((dj, w, event_mult, event_ids));
                    }
                }
                if wsum <= 0.0 {
                    continue;
                }

                let produced_event_adjusted = if base_wsum > 0.0 {
                    let avg_event_mult = (weighted_event_wsum / base_wsum).clamp(0.25, 4.0);
                    produced * avg_event_mult
                } else {
                    produced
                };

                for (dj, w, _event_mult, event_ids) in weights {
                    let latent = (produced_event_adjusted * (w / wsum)).max(0.0);
                    if latent <= 0.0 {
                        continue;
                    }
                    let row = LatentOdDemand {
                        origin_zone_id: origin_zone.id.clone(),
                        destination_zone_id: s.world.zones[dj].id.clone(),
                        purpose: *purpose,
                        time_slice: context.time_slice,
                        service_day_type: Some(context.service_day_type),
                        seasonal_profile: Some(context.seasonal_profile),
                        active_event_ids: event_ids.clone(),
                        latent_passengers: latent,
                    };
                    if is_active_context {
                        active_latent.push(ActiveLatentOd {
                            origin_idx: oi,
                            destination_idx: dj,
                            origin_zone_id: row.origin_zone_id.clone(),
                            destination_zone_id: row.destination_zone_id.clone(),
                            purpose: row.purpose,
                            time_slice: row.time_slice,
                            service_day_type: context.service_day_type,
                            seasonal_profile: context.seasonal_profile,
                            active_event_ids: event_ids,
                            latent_passengers: row.latent_passengers,
                        });
                    }
                    all_latent.push(row);
                }
            }
        }
    }

    Ok(LatentDemandBuild {
        economy_config: economy_cfg,
        zone_profiles,
        all_latent,
        active_latent,
        active_context,
    })
}

fn demand_time_slice_at(clock_s: f64) -> DemandTimeSliceLabel {
    let day_s = 86_400.0;
    let mut t = clock_s % day_s;
    if t < 0.0 {
        t += day_s;
    }
    for window in DEMAND_SLICE_WINDOWS {
        if window.start_s <= window.end_s {
            if t >= window.start_s && t < window.end_s {
                return window.label;
            }
        } else if t >= window.start_s || t < window.end_s {
            return window.label;
        }
    }
    DemandTimeSliceLabel::Interpeak
}

fn normalize_active_event_ids(
    active_event_ids: &[String],
    cfg: &SyntheticEconomyConfig,
) -> Vec<String> {
    let mut dedup = std::collections::BTreeSet::<String>::new();
    for id in active_event_ids {
        if cfg.event_demand_modifiers.iter().any(|x| x.event_id == *id) {
            dedup.insert(id.clone());
        }
    }
    dedup.into_iter().collect::<Vec<_>>()
}

fn resolve_active_event_ids(
    context: &TemporalDemandSlice,
    cfg: &SyntheticEconomyConfig,
    zone_profiles: &[ZoneDemandProfile],
) -> Vec<String> {
    let explicit = normalize_active_event_ids(&context.active_event_ids, cfg);
    let use_explicit = !explicit.is_empty();

    let mut ids = Vec::<String>::new();
    for event in &cfg.event_demand_modifiers {
        if use_explicit {
            if !explicit.iter().any(|id| id == &event.event_id) {
                continue;
            }
        } else {
            if !event.applies_day_types.is_empty()
                && !event.applies_day_types.contains(&context.service_day_type)
            {
                continue;
            }
            if !event.applies_time_slices.is_empty()
                && !event.applies_time_slices.contains(&context.time_slice)
            {
                continue;
            }
            if !event.applies_seasonal_profiles.is_empty()
                && !event
                    .applies_seasonal_profiles
                    .contains(&context.seasonal_profile)
            {
                continue;
            }
        }

        if let Some(attractor) = event.attractor_type {
            let exists = zone_profiles
                .iter()
                .any(|z| z.special_attractors.contains(&attractor));
            if !exists {
                continue;
            }
        }

        ids.push(event.event_id.clone());
    }
    ids
}

fn active_event_modifiers_for_context<'a>(
    context: &'a TemporalDemandSlice,
    cfg: &'a SyntheticEconomyConfig,
    zone_profiles: &'a [ZoneDemandProfile],
) -> Vec<&'a EventDemandModifier> {
    let active_ids = resolve_active_event_ids(context, cfg, zone_profiles);
    cfg.event_demand_modifiers
        .iter()
        .filter(|event| active_ids.iter().any(|id| id == &event.event_id))
        .collect::<Vec<_>>()
}

fn pair_event_multiplier(
    purpose: TripPurpose,
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    active_events: &[&EventDemandModifier],
    cfg: &SyntheticEconomyConfig,
) -> (f64, Vec<String>) {
    let mut mult = 1.0_f64;
    let mut applied_ids = Vec::<String>::new();

    for event in active_events {
        let applies_to_pair = if let Some(attractor) = event.attractor_type {
            origin.special_attractors.contains(&attractor)
                || destination.special_attractors.contains(&attractor)
        } else {
            true
        };
        if !applies_to_pair {
            continue;
        }

        let base = match purpose {
            TripPurpose::Work => event.purpose_multipliers.work,
            TripPurpose::Education => event.purpose_multipliers.education,
            TripPurpose::Shopping => event.purpose_multipliers.shopping,
            TripPurpose::Leisure => event.purpose_multipliers.leisure,
            TripPurpose::Essential => event.purpose_multipliers.essential,
            TripPurpose::Intercity => event.purpose_multipliers.intercity,
        }
        .max(0.05);
        let scaled = 1.0
            + ((base - 1.0)
                * event.intensity.max(0.0)
                * cfg.event_modifier_strength_scale.max(0.0));
        mult *= scaled.clamp(0.1, 6.0);
        applied_ids.push(event.event_id.clone());
    }

    (mult.clamp(0.1, 8.0), applied_ids)
}

fn day_type_multiplier(
    service_day_type: ServiceDayType,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .day_type_purpose_multipliers
        .iter()
        .find(|x| x.service_day_type == service_day_type)
    {
        let m = match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        };
        return m.max(0.0);
    }

    match (service_day_type, purpose) {
        (ServiceDayType::Weekday, TripPurpose::Work) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Education) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Shopping) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Leisure) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Essential) => 1.0,
        (ServiceDayType::Weekday, TripPurpose::Intercity) => 1.0,
        (ServiceDayType::Saturday, TripPurpose::Work) => 0.55,
        (ServiceDayType::Saturday, TripPurpose::Education) => 0.60,
        (ServiceDayType::Saturday, TripPurpose::Shopping) => 1.25,
        (ServiceDayType::Saturday, TripPurpose::Leisure) => 1.20,
        (ServiceDayType::Saturday, TripPurpose::Essential) => 0.95,
        (ServiceDayType::Saturday, TripPurpose::Intercity) => 1.05,
        (ServiceDayType::SundayHoliday, TripPurpose::Work) => 0.35,
        (ServiceDayType::SundayHoliday, TripPurpose::Education) => 0.35,
        (ServiceDayType::SundayHoliday, TripPurpose::Shopping) => 1.05,
        (ServiceDayType::SundayHoliday, TripPurpose::Leisure) => 1.18,
        (ServiceDayType::SundayHoliday, TripPurpose::Essential) => 0.92,
        (ServiceDayType::SundayHoliday, TripPurpose::Intercity) => 1.10,
    }
}

fn seasonal_multiplier(
    seasonal_profile: SeasonalProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .seasonal_purpose_multipliers
        .iter()
        .find(|x| x.seasonal_profile == seasonal_profile)
    {
        let m = match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        };
        return m.max(0.0);
    }

    match (seasonal_profile, purpose) {
        (SeasonalProfile::Neutral, _) => 1.0,
        (SeasonalProfile::SummerPeak, TripPurpose::Work) => 0.95,
        (SeasonalProfile::SummerPeak, TripPurpose::Education) => 0.92,
        (SeasonalProfile::SummerPeak, TripPurpose::Shopping) => 1.10,
        (SeasonalProfile::SummerPeak, TripPurpose::Leisure) => 1.25,
        (SeasonalProfile::SummerPeak, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::SummerPeak, TripPurpose::Intercity) => 1.18,
        (SeasonalProfile::WinterPeak, TripPurpose::Work) => 1.05,
        (SeasonalProfile::WinterPeak, TripPurpose::Education) => 1.00,
        (SeasonalProfile::WinterPeak, TripPurpose::Shopping) => 0.96,
        (SeasonalProfile::WinterPeak, TripPurpose::Leisure) => 0.90,
        (SeasonalProfile::WinterPeak, TripPurpose::Essential) => 1.08,
        (SeasonalProfile::WinterPeak, TripPurpose::Intercity) => 0.96,
        (SeasonalProfile::TermTime, TripPurpose::Work) => 1.02,
        (SeasonalProfile::TermTime, TripPurpose::Education) => 1.35,
        (SeasonalProfile::TermTime, TripPurpose::Shopping) => 0.95,
        (SeasonalProfile::TermTime, TripPurpose::Leisure) => 0.92,
        (SeasonalProfile::TermTime, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::TermTime, TripPurpose::Intercity) => 0.98,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Work) => 0.88,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Education) => 0.45,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Shopping) => 1.12,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Leisure) => 1.25,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Essential) => 1.00,
        (SeasonalProfile::HolidayPeriod, TripPurpose::Intercity) => 1.28,
    }
}

fn time_slice_multiplier(
    slice: DemandTimeSliceLabel,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    if let Some(record) = cfg
        .time_slice_purpose_multipliers
        .iter()
        .find(|x| x.time_slice == slice)
    {
        return match purpose {
            TripPurpose::Work => record.work,
            TripPurpose::Education => record.education,
            TripPurpose::Shopping => record.shopping,
            TripPurpose::Leisure => record.leisure,
            TripPurpose::Essential => record.essential,
            TripPurpose::Intercity => record.intercity,
        }
        .max(0.0);
    }

    // Fallback keeps the generator robust if config entries are partially omitted.
    match (slice, purpose) {
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Work) => 0.60,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Education) => 0.55,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Shopping) => 0.35,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Leisure) => 0.30,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Essential) => 0.70,
        (DemandTimeSliceLabel::EarlyMorning, TripPurpose::Intercity) => 0.80,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Work) => 1.65,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Education) => 1.35,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Shopping) => 0.75,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Leisure) => 0.60,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Essential) => 0.95,
        (DemandTimeSliceLabel::AmPeak, TripPurpose::Intercity) => 0.85,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Work) => 0.85,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Education) => 0.95,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Shopping) => 1.05,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Leisure) => 1.00,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Essential) => 1.00,
        (DemandTimeSliceLabel::Interpeak, TripPurpose::Intercity) => 1.00,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Work) => 1.30,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Education) => 0.90,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Shopping) => 1.20,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Leisure) => 1.10,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Essential) => 1.00,
        (DemandTimeSliceLabel::PmPeak, TripPurpose::Intercity) => 1.15,
        (DemandTimeSliceLabel::Evening, TripPurpose::Work) => 0.40,
        (DemandTimeSliceLabel::Evening, TripPurpose::Education) => 0.35,
        (DemandTimeSliceLabel::Evening, TripPurpose::Shopping) => 1.05,
        (DemandTimeSliceLabel::Evening, TripPurpose::Leisure) => 1.35,
        (DemandTimeSliceLabel::Evening, TripPurpose::Essential) => 0.95,
        (DemandTimeSliceLabel::Evening, TripPurpose::Intercity) => 1.10,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Work) => 0.12,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Education) => 0.18,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Shopping) => 0.22,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Leisure) => 0.68,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Essential) => 0.72,
        (DemandTimeSliceLabel::LateNight, TripPurpose::Intercity) => 0.58,
    }
}

fn purpose_trip_modifier(
    modifiers: &super::types::PurposeTripRateModifiers,
    purpose: TripPurpose,
) -> f64 {
    let raw = match purpose {
        TripPurpose::Work => modifiers.work,
        TripPurpose::Education => modifiers.education,
        TripPurpose::Shopping => modifiers.shopping,
        TripPurpose::Leisure => modifiers.leisure,
        TripPurpose::Essential => modifiers.essential,
        TripPurpose::Intercity => modifiers.intercity,
    };
    raw.max(0.1)
}

fn purpose_impedance(
    purpose: TripPurpose,
    gc_s: f64,
    dist_km: f64,
    gravity_beta: f64,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let (purpose_gc_beta, purpose_dist_beta) = match purpose {
        TripPurpose::Work => (
            cfg.purpose_gc_decay_beta.work.max(0.0),
            cfg.purpose_distance_decay_beta.work.max(0.0),
        ),
        TripPurpose::Education => (
            cfg.purpose_gc_decay_beta.education.max(0.0),
            cfg.purpose_distance_decay_beta.education.max(0.0),
        ),
        TripPurpose::Shopping => (
            cfg.purpose_gc_decay_beta.shopping.max(0.0),
            cfg.purpose_distance_decay_beta.shopping.max(0.0),
        ),
        TripPurpose::Leisure => (
            cfg.purpose_gc_decay_beta.leisure.max(0.0),
            cfg.purpose_distance_decay_beta.leisure.max(0.0),
        ),
        TripPurpose::Essential => (
            cfg.purpose_gc_decay_beta.essential.max(0.0),
            cfg.purpose_distance_decay_beta.essential.max(0.0),
        ),
        TripPurpose::Intercity => (
            cfg.purpose_gc_decay_beta.intercity.max(0.0),
            cfg.purpose_distance_decay_beta.intercity.max(0.0),
        ),
    };
    let beta_gc = (gravity_beta + purpose_gc_beta).max(0.0);
    let gc_term = (-beta_gc * gc_s.max(0.0)).exp();
    let dist_term = (-purpose_dist_beta * dist_km.max(0.0)).exp();
    (gc_term * dist_term).max(0.0)
}

fn purpose_attraction(
    zone_idx: usize,
    purpose: TripPurpose,
    zone_profiles: &[ZoneDemandProfile],
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let z = &zone_profiles[zone_idx];
    let base = match purpose {
        TripPurpose::Work => z.work_attractiveness,
        TripPurpose::Education => z.education_attractiveness,
        TripPurpose::Shopping => z.shopping_attractiveness,
        TripPurpose::Leisure => z.leisure_attractiveness,
        TripPurpose::Essential => z.essential_service_attractiveness,
        TripPurpose::Intercity => z.intercity_importance,
    }
    .max(0.0);
    let settlement_mult = settlement_purpose_multiplier(z.settlement_class, purpose, cfg);
    let anchor_mult = zone_attractor_multiplier(z, purpose, cfg);
    (base * settlement_mult * anchor_mult).max(0.0)
}

fn origin_production_factor(
    origin: &ZoneDemandProfile,
    purpose: TripPurpose,
    cfg: &SyntheticEconomyConfig,
) -> f64 {
    let residential_bias = match origin.archetype {
        ZoneArchetype::InnerResidential => 1.15,
        ZoneArchetype::OuterSuburb => 1.25,
        ZoneArchetype::VillageCentre => 1.10,
        ZoneArchetype::RuralResidential => 1.08,
        ZoneArchetype::RuralAgricultural => 0.95,
        ZoneArchetype::Cbd => 0.65,
        _ => 1.0,
    };
    let purpose_factor = match purpose {
        TripPurpose::Work => 0.75 + 0.25 * residential_bias + 0.30 * origin.transit_affinity,
        TripPurpose::Education => {
            0.70 + 0.25 * residential_bias + 0.20 * origin.education_intensity
        }
        TripPurpose::Shopping => {
            0.62 + 0.20 * residential_bias + 0.10 * (1.0 - origin.car_dependency)
        }
        TripPurpose::Leisure => 0.58 + 0.20 * residential_bias + 0.22 * origin.tourism_score,
        TripPurpose::Essential => {
            0.48 + 0.18 * residential_bias + 0.20 * (1.0 - origin.car_dependency)
        }
        TripPurpose::Intercity => {
            0.35 + 0.20 * origin.regional_importance + 0.15 * origin.centrality_score
        }
    };
    let settlement_mult = settlement_purpose_multiplier(origin.settlement_class, purpose, cfg);
    let transit_factor = (0.35 + origin.transit_affinity).clamp(0.2, 1.35);
    let car_penalty = (1.10 - 0.45 * origin.car_dependency).clamp(0.45, 1.10);
    (purpose_factor * settlement_mult * transit_factor * car_penalty).max(0.05)
}

fn corridor_bonus(
    oi: usize,
    dj: usize,
    purpose: TripPurpose,
    profiles: &[ZoneDemandProfile],
    cfg: &SyntheticEconomyConfig,
    dist_km: f64,
) -> f64 {
    let o = &profiles[oi];
    let d = &profiles[dj];
    let mut bonus = 1.0_f64;
    let o_rank = settlement_rank(o.settlement_class);
    let d_rank = settlement_rank(d.settlement_class);

    let both_major = o_rank >= settlement_rank(SettlementClass::MajorCity)
        && d_rank >= settlement_rank(SettlementClass::MajorCity);
    if both_major && matches!(purpose, TripPurpose::Work | TripPurpose::Intercity) {
        bonus *= cfg.corridor_bonus_major_major.max(1.0);
    }

    let commuter_origin = matches!(
        o.archetype,
        ZoneArchetype::OuterSuburb
            | ZoneArchetype::InnerResidential
            | ZoneArchetype::VillageCentre
            | ZoneArchetype::RuralResidential
    ) || matches!(
        o.settlement_class,
        SettlementClass::LargeTown
            | SettlementClass::SmallTown
            | SettlementClass::Village
            | SettlementClass::Rural
    );
    let commuter_dest = d_rank >= settlement_rank(SettlementClass::RegionalCity);
    if purpose == TripPurpose::Work && commuter_origin && commuter_dest && dist_km <= 90.0 {
        bonus *= cfg.corridor_bonus_commuter.max(1.0);
    }

    let airport_core = (o
        .special_attractors
        .contains(&SpecialAttractorType::Airport)
        && (d.archetype == ZoneArchetype::Cbd
            || d_rank >= settlement_rank(SettlementClass::MajorCity)))
        || (d
            .special_attractors
            .contains(&SpecialAttractorType::Airport)
            && (o.archetype == ZoneArchetype::Cbd
                || o_rank >= settlement_rank(SettlementClass::MajorCity)));
    if airport_core
        && matches!(
            purpose,
            TripPurpose::Intercity | TripPurpose::Work | TripPurpose::Leisure
        )
    {
        bonus *= cfg.corridor_bonus_airport_core.max(1.0);
    }

    let regional_link = matches!(
        o.settlement_class,
        SettlementClass::SmallTown | SettlementClass::Village | SettlementClass::Rural
    ) && d_rank >= settlement_rank(SettlementClass::RegionalCity);
    if regional_link
        && matches!(
            purpose,
            TripPurpose::Essential | TripPurpose::Intercity | TripPurpose::Work
        )
    {
        bonus *= cfg.corridor_bonus_regional_link.max(1.0);
    }

    bonus.clamp(0.2, 5.0)
}

fn rural_service_center_bias(
    origin: &ZoneDemandProfile,
    destination: &ZoneDemandProfile,
    purpose: TripPurpose,
) -> f64 {
    if !matches!(
        origin.settlement_class,
        SettlementClass::Village | SettlementClass::Rural
    ) {
        return 1.0;
    }
    if purpose == TripPurpose::Essential {
        if origin.nearest_service_centre_id.as_deref() == Some(destination.zone_id.as_str()) {
            return 1.35;
        }
        return 0.48;
    }
    if matches!(purpose, TripPurpose::Shopping | TripPurpose::Education)
        && origin.nearest_service_centre_id.as_deref() == Some(destination.zone_id.as_str())
    {
        return 1.20;
    }
    1.0
}
