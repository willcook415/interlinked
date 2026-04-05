use super::*;
use crate::sim::types;

pub(super) struct TemporalBundleOutputs {
    pub(super) active_temporal_slice: TemporalDemandSlice,
    pub(super) temporal_planning_snapshots: Vec<TemporalPlanningSnapshot>,
    pub(super) temporal_demand_diagnostics: TemporalDemandDiagnostics,
    pub(super) modal_demand_diagnostics: ModalDemandDiagnostics,
    pub(super) economic_diagnostics: EconomicDiagnostics,
    pub(super) service_reliability_diagnostics: ServiceReliabilityDiagnostics,
}

#[derive(Debug, Clone)]
struct TemporalContextAggregate {
    context: TemporalDemandSlice,
    latent_od_demand: Vec<LatentOdDemand>,
    assigned_od_flows: Vec<AssignedOdFlow>,
    mode_total_latent: f64,
    mode_share_summary: CitywideModeShareSummary,
    zone_planning_metrics: Vec<ZonePlanningMetrics>,
    station_planning_metrics: Vec<StationPlanningMetrics>,
    corridor_planning_metrics: Vec<CorridorPlanningMetrics>,
    line_service_planning_metrics: Vec<LineOrServicePlanningMetrics>,
    service_gap_layer: Vec<ZoneServiceGapLayerData>,
    service_gap_rankings: ServiceGapRankings,
    network_financial_summary: types::NetworkFinancialSummary,
}

pub(super) fn build_temporal_bundle_outputs(
    s: &Scenario,
    settings: &SimulationSettings,
    include_temporal_bundle: bool,
    latent_build: &LatentDemandBuild,
    assigned_od_flows: &Vec<AssignedOdFlow>,
    modal_outputs: &ModalOutputs,
    phase3: &Phase3PlanningOutputs,
    service_gap_layer: &Vec<ZoneServiceGapLayerData>,
    economics_outputs: &EconomicsOutputs,
    operations_outputs: &OperationsOutputs,
) -> Result<TemporalBundleOutputs, String> {
    let active_temporal_slice = latent_build.active_context.clone();
    let (
        temporal_planning_snapshots,
        temporal_demand_diagnostics,
        temporal_mode_share_by_slice,
        temporal_mode_share_by_day,
    ) = if include_temporal_bundle {
        let mut contexts = vec![TemporalContextAggregate {
            context: active_temporal_slice.clone(),
            latent_od_demand: latent_build.all_latent.clone(),
            assigned_od_flows: assigned_od_flows.clone(),
            mode_total_latent: modal_outputs
                .mode_choice_results
                .iter()
                .map(|x| x.latent_passengers.max(0.0))
                .sum::<f64>(),
            mode_share_summary: modal_outputs.citywide_mode_share_summary.clone(),
            zone_planning_metrics: phase3.zone_planning_metrics.clone(),
            station_planning_metrics: phase3.station_planning_metrics.clone(),
            corridor_planning_metrics: phase3.corridor_planning_metrics.clone(),
            line_service_planning_metrics: phase3.line_service_planning_metrics.clone(),
            service_gap_layer: service_gap_layer.clone(),
            service_gap_rankings: phase3.service_gap_rankings.clone(),
            network_financial_summary: economics_outputs.network_financial_summary.clone(),
        }];

        for ctx in temporal_context_catalog(&active_temporal_slice) {
            if ctx.time_slice == active_temporal_slice.time_slice
                && ctx.service_day_type == active_temporal_slice.service_day_type
                && ctx.seasonal_profile == active_temporal_slice.seasonal_profile
            {
                continue;
            }
            let nested = super::run_simulation_internal(s, settings, None, Some(ctx.clone()), false)?;
            contexts.push(TemporalContextAggregate {
                context: nested.active_temporal_slice.clone(),
                latent_od_demand: nested.latent_od_demand,
                assigned_od_flows: nested.assigned_od_flows,
                mode_total_latent: nested
                    .mode_choice_results
                    .iter()
                    .map(|x| x.latent_passengers.max(0.0))
                    .sum::<f64>(),
                mode_share_summary: nested.citywide_mode_share_summary,
                zone_planning_metrics: nested.zone_planning_metrics,
                station_planning_metrics: nested.station_planning_metrics,
                corridor_planning_metrics: nested.corridor_planning_metrics,
                line_service_planning_metrics: nested.line_service_planning_metrics,
                service_gap_layer: nested.service_gap_layer,
                service_gap_rankings: nested.service_gap_rankings,
                network_financial_summary: nested.network_financial_summary,
            });
        }
        let (snapshots, temporal_diag) = build_temporal_outputs(&contexts);
        let (mode_by_slice, mode_by_day) = build_temporal_mode_summaries(&contexts);
        (snapshots, temporal_diag, mode_by_slice, mode_by_day)
    } else {
        (
            Vec::new(),
            TemporalDemandDiagnostics::default(),
            Vec::new(),
            Vec::new(),
        )
    };

    let mut modal_demand_diagnostics = modal_outputs.modal_demand_diagnostics.clone();
    let mut economic_diagnostics = economics_outputs.economic_diagnostics.clone();
    let mut service_reliability_diagnostics =
        operations_outputs.service_reliability_diagnostics.clone();
    if !temporal_mode_share_by_slice.is_empty() {
        modal_demand_diagnostics.mode_share_by_time_slice = temporal_mode_share_by_slice;
    }
    if !temporal_mode_share_by_day.is_empty() {
        modal_demand_diagnostics.mode_share_by_day_type = temporal_mode_share_by_day;
    }
    if include_temporal_bundle {
        let mut worst_reliability_by_time_slice = Vec::<TemporalRankingEntry>::new();
        let mut worst_dwell_pressure_stations_by_time_slice = Vec::<TemporalRankingEntry>::new();
        let mut worst_transfer_nodes_by_time_slice = Vec::<TemporalRankingEntry>::new();
        for ctx in &temporal_planning_snapshots {
            if let Some(worst_line) = ctx.line_service_planning_metrics.iter().min_by(|a, b| {
                a.reliability_score
                    .partial_cmp(&b.reliability_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                worst_reliability_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_line.service_id.clone(),
                    score: worst_line.reliability_score.max(0.0),
                    reason: format!(
                        "delay {:.1}s, irregularity {:.2}",
                        worst_line.average_delay_s.max(0.0),
                        worst_line.headway_irregularity.max(0.0)
                    ),
                });
            }
            if let Some(worst_station) = ctx.station_planning_metrics.iter().max_by(|a, b| {
                a.operational_pressure_score
                    .partial_cmp(&b.operational_pressure_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                worst_dwell_pressure_stations_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_station.stop_id.clone(),
                    score: worst_station.operational_pressure_score.max(0.0),
                    reason: format!(
                        "avg_dwell {:.1}s, transfer_success {:.2}",
                        worst_station.average_dwell_time_s.max(0.0),
                        worst_station.transfer_success_rate.clamp(0.0, 1.0)
                    ),
                });
            }
            if let Some(worst_transfer_station) = ctx
                .station_planning_metrics
                .iter()
                .filter(|x| x.transfer_success_rate > 0.0)
                .min_by(|a, b| {
                    a.transfer_success_rate
                        .partial_cmp(&b.transfer_success_rate)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                worst_transfer_nodes_by_time_slice.push(TemporalRankingEntry {
                    temporal_slice: ctx.temporal_slice.clone(),
                    id: worst_transfer_station.stop_id.clone(),
                    score: (1.0 - worst_transfer_station.transfer_success_rate).max(0.0),
                    reason: format!(
                        "transfer success {:.2}",
                        worst_transfer_station.transfer_success_rate.clamp(0.0, 1.0)
                    ),
                });
            }
        }
        service_reliability_diagnostics.worst_reliability_by_time_slice =
            worst_reliability_by_time_slice;
        service_reliability_diagnostics.worst_dwell_pressure_stations_by_time_slice =
            worst_dwell_pressure_stations_by_time_slice;
        service_reliability_diagnostics.worst_transfer_nodes_by_time_slice =
            worst_transfer_nodes_by_time_slice;

        let mut by_slice = Vec::<types::TemporalFinancialSummary>::new();
        for ctx in &temporal_planning_snapshots {
            by_slice.push(types::TemporalFinancialSummary {
                temporal_slice: ctx.temporal_slice.clone(),
                fare_revenue: ctx.network_financial_summary.metrics.fare_revenue.max(0.0),
                operating_cost: ctx
                    .network_financial_summary
                    .metrics
                    .operating_cost
                    .max(0.0),
                total_cost: ctx.network_financial_summary.metrics.total_cost.max(0.0),
                subsidy_required: ctx
                    .network_financial_summary
                    .metrics
                    .subsidy_required
                    .max(0.0),
                farebox_recovery_ratio: ctx
                    .network_financial_summary
                    .metrics
                    .farebox_recovery_ratio
                    .max(0.0),
            });
        }
        by_slice.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));
        economic_diagnostics.network_financial_by_time_slice = by_slice;

        let mut day_map = HashMap::<ServiceDayType, (f64, f64, f64, f64, f64)>::new();
        for ctx in &temporal_planning_snapshots {
            let weight = ctx
                .network_financial_summary
                .total_realised_transit_trips
                .max(0.0)
                .max(1.0);
            let entry = day_map
                .entry(ctx.temporal_slice.service_day_type)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 += weight;
            entry.1 += ctx.network_financial_summary.metrics.fare_revenue.max(0.0) * weight;
            entry.2 += ctx
                .network_financial_summary
                .metrics
                .operating_cost
                .max(0.0)
                * weight;
            entry.3 += ctx.network_financial_summary.metrics.total_cost.max(0.0) * weight;
            entry.4 += ctx
                .network_financial_summary
                .metrics
                .subsidy_required
                .max(0.0)
                * weight;
        }
        let mut by_day = day_map
            .into_iter()
            .map(|(day, agg)| {
                let denom = agg.0.max(1e-9);
                let fare = agg.1 / denom;
                let op_cost = agg.2 / denom;
                let total_cost = agg.3 / denom;
                let subsidy = agg.4 / denom;
                types::ServiceDayFinancialSummary {
                    service_day_type: day,
                    fare_revenue: fare.max(0.0),
                    operating_cost: op_cost.max(0.0),
                    total_cost: total_cost.max(0.0),
                    subsidy_required: subsidy.max(0.0),
                    farebox_recovery_ratio: if op_cost > 0.0 {
                        (fare / op_cost).max(0.0)
                    } else {
                        0.0
                    },
                }
            })
            .collect::<Vec<_>>();
        by_day.sort_by(|a, b| a.service_day_type.cmp(&b.service_day_type));
        economic_diagnostics.network_financial_by_day_type = by_day;
    }

    Ok(TemporalBundleOutputs {
        active_temporal_slice,
        temporal_planning_snapshots,
        temporal_demand_diagnostics,
        modal_demand_diagnostics,
        economic_diagnostics,
        service_reliability_diagnostics,
    })
}

const CANONICAL_DAY_TYPES: [ServiceDayType; 3] = [
    ServiceDayType::Weekday,
    ServiceDayType::Saturday,
    ServiceDayType::SundayHoliday,
];

fn temporal_context_catalog(active: &TemporalDemandSlice) -> Vec<TemporalDemandSlice> {
    let mut contexts = Vec::<TemporalDemandSlice>::new();
    let mut seen =
        std::collections::BTreeSet::<(ServiceDayType, DemandTimeSliceLabel, SeasonalProfile)>::new(
        );

    let mut push_ctx = |service_day_type: ServiceDayType,
                        time_slice: DemandTimeSliceLabel,
                        seasonal_profile: SeasonalProfile| {
        if seen.insert((service_day_type, time_slice, seasonal_profile)) {
            contexts.push(TemporalDemandSlice {
                service_day_type,
                time_slice,
                seasonal_profile,
                active_event_ids: Vec::new(),
            });
        }
    };

    // Always include the active context first.
    push_ctx(
        active.service_day_type,
        active.time_slice,
        active.seasonal_profile,
    );

    // Core weekly rhythm for the currently selected seasonal profile.
    for day in CANONICAL_DAY_TYPES {
        for window in DEMAND_SLICE_WINDOWS {
            push_ctx(day, window.label, active.seasonal_profile);
        }
    }

    // Explicit term-time education lenses.
    for slice in [
        DemandTimeSliceLabel::AmPeak,
        DemandTimeSliceLabel::Interpeak,
        DemandTimeSliceLabel::PmPeak,
    ] {
        push_ctx(ServiceDayType::Weekday, slice, SeasonalProfile::TermTime);
    }

    // Holiday weekend airport/tourism stress lenses.
    for day in [ServiceDayType::Saturday, ServiceDayType::SundayHoliday] {
        for slice in [
            DemandTimeSliceLabel::EarlyMorning,
            DemandTimeSliceLabel::Interpeak,
            DemandTimeSliceLabel::Evening,
        ] {
            push_ctx(day, slice, SeasonalProfile::HolidayPeriod);
        }
    }

    // Summer tourism uplift lenses.
    for day in [ServiceDayType::Saturday, ServiceDayType::SundayHoliday] {
        for slice in [
            DemandTimeSliceLabel::Interpeak,
            DemandTimeSliceLabel::Evening,
        ] {
            push_ctx(day, slice, SeasonalProfile::SummerPeak);
        }
    }

    contexts
}

fn temporal_context_label(ctx: &TemporalDemandSlice) -> String {
    format!(
        "{:?}/{:?}/{:?}",
        ctx.service_day_type, ctx.time_slice, ctx.seasonal_profile
    )
}

fn build_temporal_outputs(
    contexts: &[TemporalContextAggregate],
) -> (Vec<TemporalPlanningSnapshot>, TemporalDemandDiagnostics) {
    let mut snapshots = contexts
        .iter()
        .map(|ctx| TemporalPlanningSnapshot {
            temporal_slice: ctx.context.clone(),
            zone_planning_metrics: ctx.zone_planning_metrics.clone(),
            station_planning_metrics: ctx.station_planning_metrics.clone(),
            corridor_planning_metrics: ctx.corridor_planning_metrics.clone(),
            line_service_planning_metrics: ctx.line_service_planning_metrics.clone(),
            service_gap_rankings: ctx.service_gap_rankings.clone(),
            network_financial_summary: ctx.network_financial_summary.clone(),
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));

    let mut purpose_totals = Vec::<PurposeTemporalDemandTotals>::new();
    let mut station_pressure = Vec::<TemporalStationPressurePoint>::new();
    let mut service_pressure = Vec::<TemporalServicePressurePoint>::new();
    let mut corridor_pressure = Vec::<TemporalCorridorPressurePoint>::new();
    let mut service_gap_summaries = Vec::<TemporalServiceGapPoint>::new();

    let mut latent_to_realised_ratio_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut top_overloaded_stations_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut top_overloaded_services_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut strongest_corridors_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_waiting_by_station_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_denied_by_station_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_corridor_unserved_by_slice = Vec::<TemporalRankingEntry>::new();
    let mut peak_line_overload_by_slice = Vec::<TemporalRankingEntry>::new();

    let mut station_score_series = HashMap::<String, Vec<(TemporalDemandSlice, f64)>>::new();
    let mut service_score_series = HashMap::<String, Vec<(TemporalDemandSlice, f64)>>::new();

    for ctx in contexts {
        let label = temporal_context_label(&ctx.context);

        for purpose in TripPurpose::ALL {
            let latent = ctx
                .latent_od_demand
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.latent_passengers.max(0.0))
                .sum::<f64>();
            let realised = ctx
                .assigned_od_flows
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.assigned_passengers.max(0.0))
                .sum::<f64>();
            let unserved = ctx
                .assigned_od_flows
                .iter()
                .filter(|x| x.purpose == purpose)
                .map(|x| x.unserved_passengers.max(0.0))
                .sum::<f64>();
            purpose_totals.push(PurposeTemporalDemandTotals {
                temporal_slice: ctx.context.clone(),
                purpose,
                latent,
                realised,
                unserved,
            });
        }

        for st in &ctx.station_planning_metrics {
            station_pressure.push(TemporalStationPressurePoint {
                temporal_slice: ctx.context.clone(),
                stop_id: st.stop_id.clone(),
                waiting: st.waiting_now.max(0.0),
                denied: st.denied_total.max(0.0),
                boarded: st.boardings_total.max(0.0),
                alighted: st.alightings_total.max(0.0),
                load_pressure_score: st.load_pressure_score.max(0.0),
                overcrowding_risk_score: st.overcrowding_risk_score.max(0.0),
            });
            let station_score = st.overcrowding_risk_score
                + (st.denied_total / (st.boardings_total + 1.0))
                + 0.20 * (st.waiting_now / (st.boardings_total + 1.0));
            station_score_series
                .entry(st.stop_id.clone())
                .or_default()
                .push((ctx.context.clone(), station_score));
        }

        for svc in &ctx.line_service_planning_metrics {
            service_pressure.push(TemporalServicePressurePoint {
                temporal_slice: ctx.context.clone(),
                service_id: svc.service_id.clone(),
                line_id: svc.line_id.clone(),
                peak_load: svc.peak_load.max(0.0),
                average_load: svc.average_load.max(0.0),
                utilisation_score: svc.utilisation_score.max(0.0),
                overcrowded_segments: svc.overcrowded_segments,
            });
            let service_score = svc.utilisation_score + (svc.overcrowded_segments as f64) * 0.25;
            service_score_series
                .entry(svc.service_id.clone())
                .or_default()
                .push((ctx.context.clone(), service_score));
        }

        for corridor in &ctx.corridor_planning_metrics {
            corridor_pressure.push(TemporalCorridorPressurePoint {
                temporal_slice: ctx.context.clone(),
                origin_zone_id: corridor.origin_zone_id.clone(),
                destination_zone_id: corridor.destination_zone_id.clone(),
                purpose: corridor.dominant_purpose,
                latent_volume: corridor.latent_volume.max(0.0),
                realised_volume: corridor.realised_volume.max(0.0),
                unserved_volume: corridor.unserved_volume.max(0.0),
                served_ratio: corridor.served_ratio.clamp(0.0, 1.0),
            });
        }

        for gap in &ctx.service_gap_layer {
            service_gap_summaries.push(TemporalServiceGapPoint {
                temporal_slice: ctx.context.clone(),
                zone_id: gap.zone_id.clone(),
                total_unserved_demand: gap.total_unserved_demand.max(0.0),
                latent_vs_realised_ratio: gap.latent_vs_realised_ratio.max(0.0),
                accessibility_score: gap.accessibility_score,
                service_coverage_score: gap.service_coverage_score,
            });
        }

        let latent_total = ctx
            .latent_od_demand
            .iter()
            .map(|x| x.latent_passengers.max(0.0))
            .sum::<f64>();
        let realised_total = ctx
            .assigned_od_flows
            .iter()
            .map(|x| x.assigned_passengers.max(0.0))
            .sum::<f64>();
        let unserved_total = ctx
            .assigned_od_flows
            .iter()
            .map(|x| x.unserved_passengers.max(0.0))
            .sum::<f64>();
        let ratio = if realised_total > 0.0 {
            latent_total / realised_total
        } else if latent_total > 0.0 {
            999.0
        } else {
            1.0
        };
        latent_to_realised_ratio_by_slice.push(TemporalRankingEntry {
            temporal_slice: ctx.context.clone(),
            id: label.clone(),
            score: ratio,
            reason: format!(
                "latent {:.1}, realised {:.1}, unserved {:.1}",
                latent_total, realised_total, unserved_total
            ),
        });

        if let Some(top_station) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            (a.overcrowding_risk_score + a.denied_total / (a.boardings_total + 1.0))
                .partial_cmp(
                    &(b.overcrowding_risk_score + b.denied_total / (b.boardings_total + 1.0)),
                )
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            top_overloaded_stations_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: top_station.stop_id.clone(),
                score: top_station.overcrowding_risk_score
                    + top_station.denied_total / (top_station.boardings_total + 1.0),
                reason: format!(
                    "waiting {:.1}, denied {:.1}",
                    top_station.waiting_now, top_station.denied_total
                ),
            });
        }

        if let Some(top_service) = ctx.line_service_planning_metrics.iter().max_by(|a, b| {
            (a.utilisation_score + (a.overcrowded_segments as f64) * 0.25)
                .partial_cmp(&(b.utilisation_score + (b.overcrowded_segments as f64) * 0.25))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            top_overloaded_services_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: top_service.service_id.clone(),
                score: top_service.utilisation_score
                    + (top_service.overcrowded_segments as f64) * 0.25,
                reason: format!(
                    "utilisation {:.2}, overcrowded_segments {}",
                    top_service.utilisation_score, top_service.overcrowded_segments
                ),
            });
        }

        if let Some(top_corridor) = ctx.corridor_planning_metrics.iter().max_by(|a, b| {
            (a.unserved_volume + 0.3 * a.latent_volume)
                .partial_cmp(&(b.unserved_volume + 0.3 * b.latent_volume))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            strongest_corridors_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: format!(
                    "{}->{}",
                    top_corridor.origin_zone_id, top_corridor.destination_zone_id
                ),
                score: top_corridor.unserved_volume + 0.3 * top_corridor.latent_volume,
                reason: format!(
                    "purpose {:?}, latent {:.1}, unserved {:.1}",
                    top_corridor.dominant_purpose,
                    top_corridor.latent_volume,
                    top_corridor.unserved_volume
                ),
            });
        }

        if let Some(st_wait) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            a.waiting_now
                .partial_cmp(&b.waiting_now)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_waiting_by_station_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: st_wait.stop_id.clone(),
                score: st_wait.waiting_now.max(0.0),
                reason: format!("context {label}"),
            });
        }
        if let Some(st_denied) = ctx.station_planning_metrics.iter().max_by(|a, b| {
            a.denied_total
                .partial_cmp(&b.denied_total)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_denied_by_station_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: st_denied.stop_id.clone(),
                score: st_denied.denied_total.max(0.0),
                reason: format!("context {label}"),
            });
        }
        if let Some(c_unserved) = ctx.corridor_planning_metrics.iter().max_by(|a, b| {
            a.unserved_volume
                .partial_cmp(&b.unserved_volume)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_corridor_unserved_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: format!(
                    "{}->{}",
                    c_unserved.origin_zone_id, c_unserved.destination_zone_id
                ),
                score: c_unserved.unserved_volume.max(0.0),
                reason: format!("purpose {:?}", c_unserved.dominant_purpose),
            });
        }
        if let Some(s_overload) = ctx.line_service_planning_metrics.iter().max_by(|a, b| {
            (a.utilisation_score + (a.overcrowded_segments as f64) * 0.25)
                .partial_cmp(&(b.utilisation_score + (b.overcrowded_segments as f64) * 0.25))
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            peak_line_overload_by_slice.push(TemporalRankingEntry {
                temporal_slice: ctx.context.clone(),
                id: s_overload.service_id.clone(),
                score: s_overload.utilisation_score
                    + (s_overload.overcrowded_segments as f64) * 0.25,
                reason: format!("context {label}"),
            });
        }
    }

    let mut overload_flip_classifications = Vec::<TemporalRankingEntry>::new();
    for (stop_id, series) in station_score_series {
        if series.is_empty() {
            continue;
        }
        let max_entry = series
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let min_entry = series
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some((max_ctx, max_score)), Some((_min_ctx, min_score))) = (max_entry, min_entry) {
            if *max_score >= 1.0 && *max_score > (*min_score * 1.50 + 0.10) {
                overload_flip_classifications.push(TemporalRankingEntry {
                    temporal_slice: max_ctx.clone(),
                    id: format!("station:{stop_id}"),
                    score: *max_score,
                    reason: format!(
                        "pressure rises from {:.2} to {:.2} at {}",
                        min_score,
                        max_score,
                        temporal_context_label(max_ctx)
                    ),
                });
            }
        }
    }
    for (service_id, series) in service_score_series {
        if series.is_empty() {
            continue;
        }
        let max_entry = series
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let min_entry = series
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some((max_ctx, max_score)), Some((_min_ctx, min_score))) = (max_entry, min_entry) {
            if *max_score >= 0.95 && *max_score > (*min_score * 1.35 + 0.05) {
                overload_flip_classifications.push(TemporalRankingEntry {
                    temporal_slice: max_ctx.clone(),
                    id: format!("service:{service_id}"),
                    score: *max_score,
                    reason: format!(
                        "load pressure rises from {:.2} to {:.2} at {}",
                        min_score,
                        max_score,
                        temporal_context_label(max_ctx)
                    ),
                });
            }
        }
    }

    let diagnostics = TemporalDemandDiagnostics {
        purpose_totals,
        station_pressure,
        service_pressure,
        corridor_pressure,
        service_gap_summaries,
        latent_to_realised_ratio_by_slice,
        top_overloaded_stations_by_slice,
        top_overloaded_services_by_slice,
        strongest_corridors_by_slice,
        peak_waiting_by_station_by_slice,
        peak_denied_by_station_by_slice,
        peak_corridor_unserved_by_slice,
        peak_line_overload_by_slice,
        overload_flip_classifications,
    };

    (snapshots, diagnostics)
}

fn build_temporal_mode_summaries(
    contexts: &[TemporalContextAggregate],
) -> (
    Vec<TimeSliceModeShareSummary>,
    Vec<ServiceDayModeShareSummary>,
) {
    let mut by_time_slice = contexts
        .iter()
        .map(|ctx| TimeSliceModeShareSummary {
            temporal_slice: ctx.context.clone(),
            transit_share: ctx.mode_share_summary.transit_share.max(0.0),
            car_share: ctx.mode_share_summary.car_share.max(0.0),
            walk_share: ctx.mode_share_summary.walk_share.max(0.0),
            suppressed_share: ctx.mode_share_summary.suppressed_share.max(0.0),
        })
        .collect::<Vec<_>>();
    by_time_slice.sort_by(|a, b| a.temporal_slice.cmp(&b.temporal_slice));

    let mut agg_by_day = HashMap::<ServiceDayType, (f64, f64, f64, f64, f64)>::new();
    for ctx in contexts {
        let weight = ctx.mode_total_latent.max(0.0);
        let entry = agg_by_day
            .entry(ctx.context.service_day_type)
            .or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
        entry.0 += weight;
        entry.1 += ctx.mode_share_summary.transit_share.max(0.0) * weight;
        entry.2 += ctx.mode_share_summary.car_share.max(0.0) * weight;
        entry.3 += ctx.mode_share_summary.walk_share.max(0.0) * weight;
        entry.4 += ctx.mode_share_summary.suppressed_share.max(0.0) * weight;
    }
    let mut by_day_type = agg_by_day
        .into_iter()
        .map(|(day, agg)| {
            let denom = agg.0.max(1e-9);
            ServiceDayModeShareSummary {
                service_day_type: day,
                transit_share: (agg.1 / denom).clamp(0.0, 1.0),
                car_share: (agg.2 / denom).clamp(0.0, 1.0),
                walk_share: (agg.3 / denom).clamp(0.0, 1.0),
                suppressed_share: (agg.4 / denom).clamp(0.0, 1.0),
            }
        })
        .collect::<Vec<_>>();
    by_day_type.sort_by(|a, b| a.service_day_type.cmp(&b.service_day_type));

    (by_time_slice, by_day_type)
}

