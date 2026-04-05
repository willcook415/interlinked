use std::collections::HashMap;

use super::contracts::{purpose_from_index, purpose_index};
use super::{
    euclid_km, euclid_m, AssignedOdFlow, CitywideModeShareSummary, CorridorModeShareMetrics,
    ModalDemandDiagnostics, ModalOutputs, ModalRankingEntry, ModeChoiceResult,
    PlanningOverlayConfig, PurposeModeShareValue, Scenario, ServiceDayModeShareSummary,
    ServiceLoadLayerData, ServiceTransitCaptureContext, SettlementClass, SpecialAttractorType,
    StationTransitCaptureContext, StopFlowState, TemporalDemandSlice, TimeSliceModeShareSummary,
    TravelMode, TripPurpose, VehicleLoadState, ZoneDemandProfile, ZoneModeShareMetrics,
};
use crate::sim::modes::{canonical_mode_from_tokens, CanonicalTransitMode};

pub(super) fn build_modal_outputs(
    s: &Scenario,
    zone_profiles: &[ZoneDemandProfile],
    mode_choice_results: &[ModeChoiceResult],
    assigned_od_flows: &[AssignedOdFlow],
    stop_flow_states: &[StopFlowState],
    vehicle_load_states: &[VehicleLoadState],
    service_load_layer: &[ServiceLoadLayerData],
    active_context: &TemporalDemandSlice,
) -> ModalOutputs {
    #[derive(Debug, Clone, Default)]
    struct ZoneModeAgg {
        latent: f64,
        transit: f64,
        car: f64,
        walk: f64,
        suppressed: f64,
        by_purpose: [(f64, f64, f64, f64, f64); 6],
        transfer_penalty_s: f64,
        crowding_penalty_s: f64,
        parking_proxy_base: f64,
    }

    #[derive(Debug, Clone, Default)]
    struct CorridorModeAgg {
        latent: f64,
        transit: f64,
        car: f64,
        walk: f64,
        suppressed: f64,
        by_purpose_latent: [f64; 6],
        submode_pax: HashMap<TravelMode, f64>,
    }

    let mut out = ModalOutputs::default();
    out.mode_choice_results = mode_choice_results.to_vec();
    if zone_profiles.is_empty() || mode_choice_results.is_empty() {
        return out;
    }

    let profile_by_id = zone_profiles
        .iter()
        .map(|p| (p.zone_id.clone(), p))
        .collect::<HashMap<_, _>>();

    let mut zone_agg = HashMap::<String, ZoneModeAgg>::new();
    let mut corridor_agg = HashMap::<(String, String), CorridorModeAgg>::new();

    let mut total_latent = 0.0_f64;
    let mut total_transit = 0.0_f64;
    let mut total_car = 0.0_f64;
    let mut total_walk = 0.0_f64;
    let mut total_suppressed = 0.0_f64;
    let mut purpose_totals = [(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); 6];
    let mut urban_latent = 0.0_f64;
    let mut urban_transit = 0.0_f64;
    let mut rural_latent = 0.0_f64;
    let mut rural_transit = 0.0_f64;
    let mut intercity_latent = 0.0_f64;
    let mut intercity_transit = 0.0_f64;
    let mut airport_latent = 0.0_f64;
    let mut airport_transit = 0.0_f64;

    let mut lost_due_crowding = 0.0_f64;
    let mut lost_due_fare = 0.0_f64;
    let mut lost_due_indirectness = 0.0_f64;
    let mut lost_due_reliability = 0.0_f64;

    for result in mode_choice_results {
        let latent = result.latent_passengers.max(0.0);
        if latent <= 0.0 {
            continue;
        }
        let purpose_idx = purpose_index(result.context.purpose);
        let transit = result.transit_captured_passengers.max(0.0);
        let car = result.car_captured_passengers.max(0.0);
        let walk = result.walk_captured_passengers.max(0.0);
        let suppressed = result.suppressed_or_no_trip_passengers.max(0.0);
        let lost = (latent - transit).max(0.0);

        total_latent += latent;
        total_transit += transit;
        total_car += car;
        total_walk += walk;
        total_suppressed += suppressed;
        purpose_totals[purpose_idx].0 += latent;
        purpose_totals[purpose_idx].1 += transit;
        purpose_totals[purpose_idx].2 += car;
        purpose_totals[purpose_idx].3 += walk;
        purpose_totals[purpose_idx].4 += suppressed;

        if matches!(
            result.context.origin_settlement_class,
            SettlementClass::GlobalCityCore
                | SettlementClass::MajorCity
                | SettlementClass::RegionalCity
                | SettlementClass::LargeTown
        ) {
            urban_latent += latent;
            urban_transit += transit;
        }
        if matches!(
            result.context.origin_settlement_class,
            SettlementClass::Village | SettlementClass::Rural
        ) {
            rural_latent += latent;
            rural_transit += transit;
        }
        if result.context.purpose == TripPurpose::Intercity {
            intercity_latent += latent;
            intercity_transit += transit;
        }
        let airport_pair = profile_by_id
            .get(&result.context.origin_zone_id)
            .map(|p| {
                p.special_attractors
                    .contains(&SpecialAttractorType::Airport)
            })
            .unwrap_or(false)
            || profile_by_id
                .get(&result.context.destination_zone_id)
                .map(|p| {
                    p.special_attractors
                        .contains(&SpecialAttractorType::Airport)
                })
                .unwrap_or(false);
        if airport_pair {
            airport_latent += latent;
            airport_transit += transit;
        }

        let transfer_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.transfer_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let crowding_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.crowding_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let fare_cost_base = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.fare_cost_base.max(0.0))
            .unwrap_or(0.0);
        let reliability_penalty_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.reliability_penalty_s.max(0.0))
            .unwrap_or(0.0);
        let transit_gc_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::OtherTransit)
            .map(|x| x.breakdown.total_generalized_cost_s.max(0.0))
            .unwrap_or(0.0);
        let car_gc_s = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::Car)
            .map(|x| x.breakdown.total_generalized_cost_s.max(0.0))
            .unwrap_or(transit_gc_s);
        let indirectness_signal = (transit_gc_s - car_gc_s).max(0.0);

        let reason_sum =
            crowding_penalty_s + fare_cost_base + indirectness_signal + reliability_penalty_s;
        if reason_sum > 0.0 && lost > 0.0 {
            lost_due_crowding += lost * (crowding_penalty_s / reason_sum);
            lost_due_fare += lost * (fare_cost_base / reason_sum);
            lost_due_indirectness += lost * (indirectness_signal / reason_sum);
            lost_due_reliability += lost * (reliability_penalty_s / reason_sum);
        }

        let zentry = zone_agg
            .entry(result.context.origin_zone_id.clone())
            .or_default();
        zentry.latent += latent;
        zentry.transit += transit;
        zentry.car += car;
        zentry.walk += walk;
        zentry.suppressed += suppressed;
        zentry.by_purpose[purpose_idx].0 += latent;
        zentry.by_purpose[purpose_idx].1 += transit;
        zentry.by_purpose[purpose_idx].2 += car;
        zentry.by_purpose[purpose_idx].3 += walk;
        zentry.by_purpose[purpose_idx].4 += suppressed;
        zentry.transfer_penalty_s += transfer_penalty_s * latent;
        zentry.crowding_penalty_s += crowding_penalty_s * latent;
        let parking_proxy_base = result
            .generalized_costs_by_mode
            .iter()
            .find(|g| g.mode == TravelMode::Car)
            .map(|x| x.breakdown.parking_toll_proxy_base.max(0.0))
            .unwrap_or(0.0);
        zentry.parking_proxy_base += parking_proxy_base * latent;

        let ckey = (
            result.context.origin_zone_id.clone(),
            result.context.destination_zone_id.clone(),
        );
        let centry = corridor_agg.entry(ckey).or_default();
        centry.latent += latent;
        centry.transit += transit;
        centry.car += car;
        centry.walk += walk;
        centry.suppressed += suppressed;
        centry.by_purpose_latent[purpose_idx] += latent;
        for sub in &result.transit_submode_split {
            *centry.submode_pax.entry(sub.mode).or_insert(0.0) += sub.passengers.max(0.0);
        }
    }

    let mut zone_mode_share_metrics = zone_profiles
        .iter()
        .map(|profile| {
            let agg = zone_agg.get(&profile.zone_id).cloned().unwrap_or_default();
            let denom = agg.latent.max(1e-9);
            let mut mode_share_by_purpose = Vec::<PurposeModeShareValue>::new();
            for (idx, vals) in agg.by_purpose.iter().enumerate() {
                if vals.0 <= 0.0 {
                    continue;
                }
                mode_share_by_purpose.push(PurposeModeShareValue {
                    purpose: purpose_from_index(idx),
                    transit_share: (vals.1 / vals.0).clamp(0.0, 1.0),
                    car_share: (vals.2 / vals.0).clamp(0.0, 1.0),
                    walk_share: (vals.3 / vals.0).clamp(0.0, 1.0),
                    suppressed_share: (vals.4 / vals.0).clamp(0.0, 1.0),
                });
            }
            mode_share_by_purpose.sort_by(|a, b| {
                b.transit_share
                    .partial_cmp(&a.transit_share)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ZoneModeShareMetrics {
                zone_id: profile.zone_id.clone(),
                settlement_class: profile.settlement_class,
                archetype: profile.archetype,
                transit_share: (agg.transit / denom).clamp(0.0, 1.0),
                car_share: (agg.car / denom).clamp(0.0, 1.0),
                walk_share: (agg.walk / denom).clamp(0.0, 1.0),
                suppressed_share: (agg.suppressed / denom).clamp(0.0, 1.0),
                transit_captured_demand: agg.transit.max(0.0),
                non_transit_demand: (agg.car + agg.walk + agg.suppressed).max(0.0),
                mode_share_by_purpose,
                active_temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    zone_mode_share_metrics.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));

    let mut corridor_mode_share_metrics = corridor_agg
        .iter()
        .map(|((oz, dz), agg)| {
            let denom = agg.latent.max(1e-9);
            let transit_share = (agg.transit / denom).clamp(0.0, 1.0);
            let car_share = (agg.car / denom).clamp(0.0, 1.0);
            let walk_share = (agg.walk / denom).clamp(0.0, 1.0);
            let suppressed_share = (agg.suppressed / denom).clamp(0.0, 1.0);
            let mut dominant_mode = TravelMode::Car;
            let mut dominant_share = car_share;
            if transit_share > dominant_share {
                dominant_share = transit_share;
                dominant_mode = TravelMode::OtherTransit;
            }
            if walk_share > dominant_share {
                dominant_share = walk_share;
                dominant_mode = TravelMode::Walk;
            }
            if suppressed_share > dominant_share {
                dominant_mode = TravelMode::NoTrip;
            }
            let strongest_purpose = agg
                .by_purpose_latent
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| purpose_from_index(idx))
                .unwrap_or(TripPurpose::Work);
            let strongest_transit_submode = agg
                .submode_pax
                .iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(mode, _)| *mode)
                .unwrap_or(TravelMode::OtherTransit);
            CorridorModeShareMetrics {
                origin_zone_id: oz.clone(),
                destination_zone_id: dz.clone(),
                dominant_mode,
                transit_share,
                car_share,
                walk_share,
                suppressed_share,
                strongest_purpose,
                strongest_transit_submode,
                transit_captured_demand: agg.transit.max(0.0),
                transit_capture_gap: (agg.latent - agg.transit).max(0.0),
                active_temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    corridor_mode_share_metrics.sort_by(|a, b| {
        b.transit_capture_gap
            .partial_cmp(&a.transit_capture_gap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut nearest_zone_by_stop = HashMap::<String, String>::new();
    for stop in &s.world.stops {
        let mut best: Option<(String, f64)> = None;
        for z in &s.world.zones {
            let d = euclid_m((stop.x, stop.y), (z.x, z.y));
            match &best {
                Some((_, bd)) if d >= *bd => {}
                _ => best = Some((z.id.clone(), d)),
            }
        }
        if let Some((zid, _)) = best {
            nearest_zone_by_stop.insert(stop.id.clone(), zid);
        }
    }

    let planning_cfg = PlanningOverlayConfig::default();
    let mut station_transit_capture_context = Vec::<StationTransitCaptureContext>::new();
    for stop in &s.world.stops {
        let mut catchment_latent = 0.0_f64;
        let mut catchment_transit = 0.0_f64;
        let mut catchment_uncaptured = 0.0_f64;
        let mut weighted_transfer = 0.0_f64;
        let mut weighted_indirectness = 0.0_f64;

        for z in &s.world.zones {
            let d = euclid_m((stop.x, stop.y), (z.x, z.y));
            if d > planning_cfg.station_catchment_radius_m {
                continue;
            }
            if let Some(agg) = zone_agg.get(&z.id) {
                catchment_latent += agg.latent.max(0.0);
                catchment_transit += agg.transit.max(0.0);
                catchment_uncaptured += (agg.car + agg.walk + agg.suppressed).max(0.0);
                weighted_transfer += agg.transfer_penalty_s.max(0.0);
                weighted_indirectness += agg.crowding_penalty_s.max(0.0);
            }
        }

        let flow = stop_flow_states.iter().find(|x| x.stop_id == stop.id);
        let waiting = flow.map(|x| x.total_waiting.max(0.0)).unwrap_or(0.0);
        let denied = flow.map(|x| x.denied_this_step.max(0.0)).unwrap_or(0.0);
        let boarded = flow.map(|x| x.boarded_this_step.max(0.0)).unwrap_or(0.0);
        let crowding_signal = (waiting + denied) / (boarded + 1.0);
        let capture_share = if catchment_latent > 0.0 {
            (catchment_transit / catchment_latent).clamp(0.0, 1.0)
        } else {
            0.0
        };
        station_transit_capture_context.push(StationTransitCaptureContext {
            stop_id: stop.id.clone(),
            catchment_latent_demand: catchment_latent.max(0.0),
            transit_captured_demand: catchment_transit.max(0.0),
            uncaptured_competing_demand: catchment_uncaptured.max(0.0),
            limiting_crowding_signal: crowding_signal.max(0.0),
            limiting_transfer_signal: if catchment_latent > 0.0 {
                (weighted_transfer / catchment_latent).max(0.0)
            } else {
                0.0
            },
            limiting_indirectness_signal: if catchment_latent > 0.0 {
                (weighted_indirectness / catchment_latent).max(0.0)
            } else {
                0.0
            },
        });
        if capture_share <= 0.0 {
            continue;
        }
    }
    station_transit_capture_context.sort_by(|a, b| a.stop_id.cmp(&b.stop_id));

    let mut service_transit_capture_context = Vec::<ServiceTransitCaptureContext>::new();
    let mut canonical_mode_by_service = HashMap::<String, CanonicalTransitMode>::new();
    for svc in &s.world.services {
        let endpoint_dist_km =
            if let (Some(a), Some(b)) = (svc.stop_sequence.first(), svc.stop_sequence.last()) {
                let aa = s.world.stops.iter().find(|x| x.id == *a);
                let bb = s.world.stops.iter().find(|x| x.id == *b);
                if let (Some(aa), Some(bb)) = (aa, bb) {
                    euclid_km((aa.x, aa.y), (bb.x, bb.y))
                } else {
                    0.0
                }
            } else {
                0.0
            };
        let canonical_mode =
            canonical_mode_from_tokens(&svc.mode, svc.mode_variant.as_deref(), endpoint_dist_km);
        let mode = canonical_mode.travel_mode_family();
        canonical_mode_by_service.insert(svc.id.clone(), canonical_mode);
        let mut affected_zone_ids = std::collections::HashSet::<String>::new();
        for stop_id in &svc.stop_sequence {
            if let Some(zid) = nearest_zone_by_stop.get(stop_id) {
                affected_zone_ids.insert(zid.clone());
            }
        }
        let mut latent_exposed = 0.0_f64;
        let mut transit_captured = 0.0_f64;
        let mut uncaptured = 0.0_f64;
        for zid in &affected_zone_ids {
            if let Some(agg) = zone_agg.get(zid) {
                latent_exposed += agg.latent.max(0.0);
                transit_captured += agg.transit.max(0.0);
                uncaptured += (agg.car + agg.walk + agg.suppressed).max(0.0);
            }
        }
        let utilisation_score = service_load_layer
            .iter()
            .find(|x| x.service_id == svc.id)
            .map(|x| x.peak_load.max(0.0) / (svc.vehicle_capacity.max(1.0)))
            .unwrap_or(0.0);
        let max_crowding = vehicle_load_states
            .iter()
            .filter(|x| x.service_id == svc.id)
            .map(|x| x.crowding_ratio.max(0.0))
            .fold(0.0_f64, f64::max);
        service_transit_capture_context.push(ServiceTransitCaptureContext {
            service_id: svc.id.clone(),
            line_id: svc.line_id.clone(),
            service_mode: mode,
            latent_demand_exposed: latent_exposed.max(0.0),
            transit_captured_demand: transit_captured.max(0.0),
            uncaptured_competing_demand: uncaptured.max(0.0),
            utilisation_score: utilisation_score.max(0.0),
            crowding_lost_share_signal: (max_crowding - 0.75).max(0.0)
                * (uncaptured / (latent_exposed + 1.0)).max(0.0),
        });
    }
    service_transit_capture_context.sort_by(|a, b| a.service_id.cmp(&b.service_id));

    let by_purpose = purpose_totals
        .iter()
        .enumerate()
        .filter(|(_, vals)| vals.0 > 0.0)
        .map(|(idx, vals)| PurposeModeShareValue {
            purpose: purpose_from_index(idx),
            transit_share: (vals.1 / vals.0).clamp(0.0, 1.0),
            car_share: (vals.2 / vals.0).clamp(0.0, 1.0),
            walk_share: (vals.3 / vals.0).clamp(0.0, 1.0),
            suppressed_share: (vals.4 / vals.0).clamp(0.0, 1.0),
        })
        .collect::<Vec<_>>();
    let citywide_mode_share_summary = CitywideModeShareSummary {
        transit_share: if total_latent > 0.0 {
            (total_transit / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        car_share: if total_latent > 0.0 {
            (total_car / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        walk_share: if total_latent > 0.0 {
            (total_walk / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        suppressed_share: if total_latent > 0.0 {
            (total_suppressed / total_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        by_purpose: by_purpose.clone(),
        by_time_slice: vec![TimeSliceModeShareSummary {
            temporal_slice: active_context.clone(),
            transit_share: if total_latent > 0.0 {
                (total_transit / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            car_share: if total_latent > 0.0 {
                (total_car / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            walk_share: if total_latent > 0.0 {
                (total_walk / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            suppressed_share: if total_latent > 0.0 {
                (total_suppressed / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }],
        by_day_type: vec![ServiceDayModeShareSummary {
            service_day_type: active_context.service_day_type,
            transit_share: if total_latent > 0.0 {
                (total_transit / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            car_share: if total_latent > 0.0 {
                (total_car / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            walk_share: if total_latent > 0.0 {
                (total_walk / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
            suppressed_share: if total_latent > 0.0 {
                (total_suppressed / total_latent).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }],
        urban_transit_share: if urban_latent > 0.0 {
            (urban_transit / urban_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        rural_transit_share: if rural_latent > 0.0 {
            (rural_transit / rural_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        intercity_transit_share: if intercity_latent > 0.0 {
            (intercity_transit / intercity_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
        airport_access_transit_share: if airport_latent > 0.0 {
            (airport_transit / airport_latent).clamp(0.0, 1.0)
        } else {
            0.0
        },
    };

    let mut top_capture_opportunity = corridor_mode_share_metrics
        .iter()
        .map(|c| ModalRankingEntry {
            id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
            score: c.transit_capture_gap.max(0.0) * (0.6 + c.car_share.max(0.0)),
            reason: format!(
                "latent gap {:.1}, transit_share {:.2}, car_share {:.2}",
                c.transit_capture_gap, c.transit_share, c.car_share
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_capture_opportunity.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_capture_opportunity.truncate(12);

    let mut top_car_dominated = corridor_mode_share_metrics
        .iter()
        .filter(|c| c.car_share > c.transit_share && c.transit_share >= 0.08)
        .map(|c| ModalRankingEntry {
            id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
            score: c.car_share.max(0.0) * c.transit_capture_gap.max(0.0),
            reason: format!(
                "car {:.2} > transit {:.2}, gap {:.1}",
                c.car_share, c.transit_share, c.transit_capture_gap
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_car_dominated.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_car_dominated.truncate(12);

    let mut top_overcrowded_losing = corridor_mode_share_metrics
        .iter()
        .map(|c| {
            let crowding_signal = assigned_od_flows
                .iter()
                .find(|f| {
                    f.origin_zone_id == c.origin_zone_id
                        && f.destination_zone_id == c.destination_zone_id
                        && f.time_slice == active_context.time_slice
                })
                .map(|f| f.unserved_passengers.max(0.0))
                .unwrap_or(0.0);
            ModalRankingEntry {
                id: format!("{}->{}", c.origin_zone_id, c.destination_zone_id),
                score: crowding_signal * (1.0 + c.car_share.max(0.0)),
                reason: format!(
                    "unserved {:.1} with car_share {:.2}",
                    crowding_signal, c.car_share
                ),
                temporal_slice: active_context.clone(),
            }
        })
        .collect::<Vec<_>>();
    top_overcrowded_losing.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_overcrowded_losing.truncate(12);

    let mut top_rural_services = service_transit_capture_context
        .iter()
        .filter(|svc| {
            canonical_mode_by_service
                .get(&svc.service_id)
                .copied()
                .map(|mode| mode.is_rural_essential_candidate())
                .unwrap_or(matches!(
                    svc.service_mode,
                    TravelMode::Bus | TravelMode::SuburbanRail | TravelMode::RegionalRail
                ))
        })
        .map(|svc| ModalRankingEntry {
            id: svc.service_id.clone(),
            score: (svc.latent_demand_exposed - svc.transit_captured_demand).max(0.0)
                + (1.0 - svc.utilisation_score.min(1.0)) * 20.0,
            reason: format!(
                "latent {:.1}, captured {:.1}, utilisation {:.2}",
                svc.latent_demand_exposed, svc.transit_captured_demand, svc.utilisation_score
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_rural_services.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_rural_services.truncate(12);

    let mut top_zone_transit = zone_mode_share_metrics
        .iter()
        .map(|z| ModalRankingEntry {
            id: z.zone_id.clone(),
            score: z.transit_share.max(0.0),
            reason: format!(
                "transit_share {:.2}, car_share {:.2}",
                z.transit_share, z.car_share
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_transit.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_transit.truncate(12);

    let mut top_zone_transfer_loss = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.transfer_penalty_s / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "transfer penalty {:.1}s per latent pax",
                if agg.latent > 0.0 {
                    agg.transfer_penalty_s / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_transfer_loss.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_transfer_loss.truncate(12);

    let mut top_zone_crowding_loss = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.crowding_penalty_s / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "crowding penalty {:.1}s per latent pax",
                if agg.latent > 0.0 {
                    agg.crowding_penalty_s / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_crowding_loss.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_crowding_loss.truncate(12);

    let mut top_zone_parking = zone_agg
        .iter()
        .map(|(zone_id, agg)| ModalRankingEntry {
            id: zone_id.clone(),
            score: if agg.latent > 0.0 {
                (agg.parking_proxy_base / agg.latent).max(0.0)
            } else {
                0.0
            },
            reason: format!(
                "parking/toll proxy {:.2} base per latent pax",
                if agg.latent > 0.0 {
                    agg.parking_proxy_base / agg.latent
                } else {
                    0.0
                }
            ),
            temporal_slice: active_context.clone(),
        })
        .collect::<Vec<_>>();
    top_zone_parking.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_zone_parking.truncate(12);

    out.zone_mode_share_metrics = zone_mode_share_metrics.clone();
    out.corridor_mode_share_metrics = corridor_mode_share_metrics.clone();
    out.station_transit_capture_context = station_transit_capture_context.clone();
    out.service_transit_capture_context = service_transit_capture_context.clone();
    out.citywide_mode_share_summary = citywide_mode_share_summary.clone();
    out.modal_demand_diagnostics = ModalDemandDiagnostics {
        mode_share_by_purpose: by_purpose,
        mode_share_by_zone: zone_mode_share_metrics,
        mode_share_by_corridor: corridor_mode_share_metrics,
        mode_share_by_time_slice: citywide_mode_share_summary.by_time_slice.clone(),
        mode_share_by_day_type: citywide_mode_share_summary.by_day_type.clone(),
        transit_capture_total: total_transit.max(0.0),
        transit_lost_total: (total_latent - total_transit).max(0.0),
        transit_lost_due_to_crowding: lost_due_crowding.max(0.0),
        transit_lost_due_to_fare: lost_due_fare.max(0.0),
        transit_lost_due_to_indirectness: lost_due_indirectness.max(0.0),
        transit_lost_due_to_reliability: lost_due_reliability.max(0.0),
        top_transit_capture_opportunity_corridors: top_capture_opportunity,
        top_car_dominated_transit_viable_corridors: top_car_dominated,
        top_overcrowded_corridors_losing_mode_share: top_overcrowded_losing,
        top_rural_essential_low_demand_services: top_rural_services,
        top_zones_by_transit_share: top_zone_transit,
        top_zones_losing_due_to_transfers: top_zone_transfer_loss,
        top_zones_losing_due_to_crowding: top_zone_crowding_loss,
        top_zones_where_parking_penalty_supports_transit: top_zone_parking,
    };

    out
}
