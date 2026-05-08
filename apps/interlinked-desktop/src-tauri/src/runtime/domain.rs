use super::scheduling::effective_strategic_refresh_interval_ticks;
use crate::*;

pub(crate) use super::defaults::*;
pub(crate) use super::fare::*;
pub(crate) use super::materialization::*;
pub(crate) use super::models::*;
pub(crate) use super::train_kernel::*;
pub(crate) use super::views::*;

const RT_TICK_LOG_INTERVAL: u64 = 20;
const RT_TICK_SLOW_TOTAL_MS: f64 = 120.0;
const RT_TICK_SLOW_STAGE_MS: f64 = 80.0;

fn economy_snapshot_from_manifest(manifest: &ProjectManifest) -> SimulationAdvanceEconomy {
    SimulationAdvanceEconomy {
        current_balance_base: manifest.economy.current_balance_base,
        cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
        cumulative_opex_base: manifest.economy.cumulative_opex_base,
        budget_display: manifest
            .progress_metrics
            .as_ref()
            .map(|m| m.budget)
            .unwrap_or(manifest.economy.current_balance_base),
    }
}

pub(crate) fn run_simulation_tick(
    state: &AppState,
    project_root: &Path,
    manifest: &mut ProjectManifest,
    dt_s: f64,
    fixed_step_s: f64,
    recompute_quick_kpis: bool,
    tick_index: u64,
    clock_revision: u64,
    queue_depth: usize,
    dropped_steps: u32,
    emit_runtime_views: bool,
    strategic_refresh_due_hint: bool,
) -> Result<RuntimeSnapshot, String> {
    let tick_start = Instant::now();
    let mut telemetry = RuntimePerfTelemetry {
        tick_index,
        dt_s,
        fixed_step_s,
        queue_depth,
        dropped_steps,
        ..RuntimePerfTelemetry::default()
    };
    let mut guard = state
        .game
        .lock()
        .map_err(|_| "game mutex poisoned".to_string())?;
    let gs = guard
        .as_mut()
        .ok_or_else(|| "game not initialised for current session".to_string())?;
    let prepare_start = Instant::now();
    if let Some(step_kernel) = gs.run_cfg.step_kernel.as_mut() {
        step_kernel.k_paths = 1;
        step_kernel.msa_max_iters = 1;
        step_kernel.convergence_rel = 1.0;
        step_kernel.route_choice_theta = 0.002;
    }
    gs.run_cfg.enable_kernel_partitioning = manifest.runtime_scheduling.runtime_ops_kernel_v1;
    gs.run_cfg.strategic_refresh_interval_steps = effective_strategic_refresh_interval_ticks(
        manifest.runtime_scheduling.strategic_refresh_interval_ticks,
        manifest.clock_state.speed,
    );
    if runtime_has_due_purchase_orders(gs.store.scenario(), manifest.clock_state.tick_seconds) {
        let mut scenario_with_orders = gs.store.scenario().clone();
        let delivered_orders = settle_pending_purchase_orders(
            &mut scenario_with_orders,
            manifest.clock_state.tick_seconds,
        );
        if delivered_orders > 0 {
            gs.store = ScenarioStore::new(scenario_with_orders);
        }
    }
    if gs.store.scenario().world.stops.is_empty() {
        let safe_dt_s = if dt_s.is_finite() { dt_s.max(0.0) } else { 0.0 };
        gs.tick_s = (gs.tick_s + safe_dt_s).max(0.0);
        gs.sim_state.t_s = gs.tick_s;
        manifest.clock_state.tick_seconds = gs.tick_s;
        if let Ok(mut guard) = state.runtime_ops.lock() {
            *guard = None;
        }
        sync_progress_budget_from_economy(manifest);
        telemetry.stage_prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.0;
        telemetry.adaptive_max_active_zones = 0;
        telemetry.stage_step_ms = 0.0;
        telemetry.stage_economy_ms = 0.0;
        telemetry.stage_runtime_ops_ms = 0.0;
        telemetry.strategic_refresh_due = strategic_refresh_due_hint;
        telemetry.strategic_refresh_interval_ticks = effective_strategic_refresh_interval_ticks(
            manifest.runtime_scheduling.strategic_refresh_interval_ticks,
            manifest.clock_state.speed,
        );
        telemetry.runtime_views_materialized = false;
        telemetry.tick_total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
        {
            let mut materialization = state
                .runtime_materialization
                .lock()
                .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
            if let Some(current) = materialization.as_mut() {
                if current.project_path == project_root.to_string_lossy() {
                    current.last_tick_ms = telemetry.tick_total_ms;
                }
            }
        }
        return Ok(RuntimeSnapshot {
            project_path: project_root.to_string_lossy().to_string(),
            clock_revision,
            clock: manifest.clock_state.clone(),
            economy: economy_snapshot_from_manifest(manifest),
            frame: None,
            delta_revenue_base: 0.0,
            delta_opex_base: 0.0,
            delta_net_base: 0.0,
            captured_at_epoch_ms: now_epoch_ms(),
            telemetry,
            trains: Vec::new(),
            stations: Vec::new(),
            line_ops: Vec::new(),
            provenance_warnings: Vec::new(),
            trains_authoritative: runtime_trains_authoritative_for_manifest(manifest),
            passenger_counter_provenance: CounterProvenance::RuntimeProjection,
            fare_counter_provenance: CounterProvenance::StrategicEstimate,
        });
    }
    let minute_of_day = clock_minute_of_day(&manifest.clock_state);
    let adaptive_cap = ensure_runtime_materialized_scenario(
        state,
        &project_root.to_string_lossy(),
        gs,
        manifest,
        minute_of_day,
        tick_index,
    )?;
    telemetry.adaptive_max_active_zones = adaptive_cap;
    telemetry.stage_prepare_ms = prepare_start.elapsed().as_secs_f64() * 1000.0;

    let scope = SimulationScope {
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        remote_regions_mode: manifest.simulation_scope.remote_regions_mode.clone(),
        max_active_zones: adaptive_cap,
    };
    let step_start = Instant::now();
    let req = interlinked_engine::platform::GameStepRequest {
        recompute_quick_kpis,
        edits: Vec::new(),
        force_strategic_refresh: false,
    };
    gs.run_cfg.lightweight_outputs = manifest.runtime_scheduling.lightweight_tick_outputs;
    let step_output = SimulationService::step_game_scoped(gs, dt_s, req, &scope)?;
    telemetry.stage_step_ms = step_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.engine_strategic_refresh_executed = step_output.strategic_refresh_executed;
    telemetry.engine_strategic_refresh_reason = step_output
        .strategic_refresh_reason
        .map(|reason| format!("{reason:?}"));
    telemetry.engine_fast_steps = step_output.kernel_perf.fast_steps;
    telemetry.engine_strategic_steps = step_output.kernel_perf.strategic_steps;
    telemetry.engine_fast_last_ms = step_output.kernel_perf.last_fast_ms;
    telemetry.engine_strategic_last_ms = step_output.kernel_perf.last_strategic_ms;
    telemetry.engine_fast_avg_ms = step_output.kernel_perf.avg_fast_ms();
    telemetry.engine_strategic_avg_ms = step_output.kernel_perf.avg_strategic_ms();
    telemetry.engine_steps_since_last_strategic =
        step_output.kernel_perf.steps_since_last_strategic;
    telemetry.engine_strategic_cache_hits = step_output.kernel_perf.strategic_cache_hits;
    telemetry.engine_strategic_cache_misses = step_output.kernel_perf.strategic_cache_misses;
    if let Some(output) = gs.last_output.as_ref() {
        telemetry.lifecycle_diagnostics = Some(RuntimeLifecycleDiagnostics::from_engine_summary(
            &output.lifecycle_conservation,
        ));
        let trace = &output.diagnostics.planner_passenger_trace;
        let planner_attempted = trace.assignment_attempted_pax_total.max(0.0);
        let planner_period_s = (output.meta.time_period_hours.max(0.0) * 3600.0).max(1e-9);
        let assignment_attempted_per_hour = planner_attempted * (3600.0 / planner_period_s);
        let cohort_attempted_per_hour =
            trace.passenger_cohort_attempted_pax_total.max(0.0) * (3600.0 / planner_period_s);
        if planner_attempted <= 0.0 || tick_index % 30 == 0 {
            eprintln!(
                "[pax-runtime] tick={} minute={} services={} demand_cells={} zones={} latent={:.6} mode_capture={:.6} mode_paths_raw={} mode_paths_boardable={} planner_period_s={:.3} assignment_attempted={:.6} assignment_attempted_per_hour={:.6} cohort_attempted={:.6} cohort_attempted_per_hour={:.6} first_zero_stage={} reason={}",
                tick_index,
                minute_of_day,
                gs.store.scenario().world.services.len(),
                trace.demand_cells_total,
                trace.zones_total,
                trace.latent_pax_total.max(0.0),
                trace.mode_choice_transit_captured_pax.max(0.0),
                trace.mode_choice_candidate_paths_raw_total,
                trace.mode_choice_candidate_paths_boardable_total,
                planner_period_s,
                trace.assignment_attempted_pax_total.max(0.0),
                assignment_attempted_per_hour.max(0.0),
                trace.passenger_cohort_attempted_pax_total.max(0.0),
                cohort_attempted_per_hour.max(0.0),
                trace.first_zero_stage.as_deref().unwrap_or("none"),
                trace.first_zero_reason.as_deref().unwrap_or("none"),
            );
        }
        if planner_attempted <= 0.0 {
            let service_mode_by_id = gs
                .store
                .scenario()
                .world
                .services
                .iter()
                .map(|service| (service.id.clone(), service.mode.clone()))
                .collect::<HashMap<_, _>>();
            for trace_row in trace
                .service_stop_traces
                .iter()
                .filter(|row| {
                    service_mode_by_id
                        .get(&row.service_id)
                        .map(|mode| mode.to_ascii_lowercase().contains("metro"))
                        .unwrap_or(false)
                })
                .take(8)
            {
                eprintln!(
                    "[pax-runtime-service] tick={} service={} stop={} raw_paths={} boardable_paths={} attempted={:.6} assigned={:.6} reason={}",
                    tick_index,
                    trace_row.service_id,
                    trace_row.stop_id,
                    trace_row.raw_candidate_paths,
                    trace_row.boardable_candidate_paths,
                    trace_row.planner_attempted_pax.max(0.0),
                    trace_row.planner_assigned_pax.max(0.0),
                    trace_row.reason_code.as_deref().unwrap_or("none"),
                );
            }
        }
    }
    if step_output.strategic_refresh_executed {
        if let Some(output) = gs.last_output.as_ref() {
            let cache_entry = RuntimeStrategicDemandCacheEntry {
                service_gap_layer: output.service_gap_layer.clone(),
                corridor_desire_lines: output.corridor_desire_lines.clone(),
            };
            if let Ok(mut cache) = state.runtime_strategic_demand_cache.lock() {
                cache.insert(project_root.to_string_lossy().to_string(), cache_entry);
            }
        }
    }

    let econ_start = Instant::now();
    let cfg = economy_config();
    let frame = interlinked_engine::platform::history_last_frame(gs)
        .ok_or_else(|| "no history frame available after step".to_string())?;
    let service_opex_per_hour = interlinked_engine::platform::estimate_service_opex_per_hour_base(
        gs.store.scenario(),
        &cfg,
    );
    let staff_opex_per_hour =
        builder_support::estimate_staff_opex_per_hour_base(gs.store.scenario(), &cfg);
    manifest.clock_state.tick_seconds = gs.tick_s;
    let frame_lite = HistoryFrameLite {
        t_s: frame.t_s,
        kpis: frame.kpis.clone(),
        queue_summary: frame.queue_summary.clone(),
        service_loads: if step_output.strategic_refresh_executed {
            build_live_service_loads(gs)
        } else {
            Vec::new()
        },
    };
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let trains_authoritative = runtime_trains_authoritative_for_manifest(manifest);
    let ops_start = Instant::now();
    let (
        runtime_trains,
        runtime_stations,
        runtime_line_ops,
        mut provenance_warnings,
        runtime_fare_events,
    ) = if trains_authoritative {
        let (trains, stations, line_ops, warnings, fare_events) = build_runtime_ops_views(
            state,
            &project_root.to_string_lossy(),
            gs.store.scenario(),
            gs.last_output.as_ref(),
            &manifest.economy.fare_policy,
            dt_s,
            topology_hash,
            emit_runtime_views,
        )?;
        (trains, stations, line_ops, warnings, fare_events)
    } else {
        if let Ok(mut guard) = state.runtime_ops.lock() {
            *guard = None;
        }
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RuntimeFareEvents::default(),
        )
    };
    telemetry.stage_runtime_ops_ms = ops_start.elapsed().as_secs_f64() * 1000.0;
    let fare_recognition_enabled = runtime_fare_recognition_enabled_for_manifest(manifest);
    // The engine fast kernel owns queue/onboard cohorts for fast operational
    // stepping. Completed onboard cohorts can feed authoritative fare only when
    // they can be priced by the current deterministic fare basis.
    let simulation_owned_lifecycle_events: Option<PassengerLifecycleEventBatch> =
        if !step_output.strategic_refresh_executed {
            gs.last_output.as_ref().and_then(|output| {
                authoritative_lifecycle_events_from_engine_fast_output(frame_lite.t_s, output)
            })
        } else {
            None
        };
    let strategic_lifecycle_events = if fare_recognition_enabled {
        strategic_lifecycle_events_for_economy(gs)
    } else {
        strategic_kpi_lifecycle_events_for_economy(&frame_lite)
    };
    let projection_fallback_enabled = fare_recognition_enabled
        && manifest.session_kind == SessionKind::Game
        && trains_authoritative;
    let projection_lifecycle_fallback = if projection_fallback_enabled {
        // Projection/animation-adjacent runtime ops are not authoritative economy truth.
        // Keep this final fallback explicit until simulation-owned passenger
        // lifecycle events include fare recognition.
        Some(collect_projection_lifecycle_events_fallback(
            frame_lite.t_s,
            runtime_fare_events,
        ))
    } else {
        None
    };
    let fare_event = select_fare_event_for_economy(
        simulation_owned_lifecycle_events.as_ref(),
        &strategic_lifecycle_events,
        projection_lifecycle_fallback.as_ref(),
        projection_fallback_enabled,
    );
    let fare_source_diagnostic = fare_source_selection_diagnostic(&fare_event);
    debug_assert_eq!(
        fare_source_diagnostic.selected_provenance,
        fare_event.provenance
    );
    debug_assert_eq!(
        fare_source_diagnostic.selected_source_label,
        fare_event.source_label
    );
    debug_assert!(
        fare_source_diagnostic.used_authoritative_sim
            || fare_source_diagnostic.used_strategic_estimate
            || fare_source_diagnostic.used_runtime_projection_fallback
            || fare_event.fare_delta_base <= 0.0
    );
    if fare_source_diagnostic.used_runtime_projection_fallback {
        provenance_warnings.push(
            "runtime_projection: economy fare recognition used desktop_projection_fare_events fallback"
                .to_string(),
        );
    }
    telemetry.fare_source_diagnostic = Some(runtime_fare_source_telemetry(&fare_event));
    debug_assert!(matches!(
        fare_event.kind,
        PassengerFareEventKind::FareRecognized
    ));
    let _fare_event_time_s = fare_event.simulation_time_s;
    let fare_counter_provenance = fare_event.provenance;
    let accrued_fare_revenue_base = fare_event.fare_delta_base.max(0.0);
    let accrued_boardings_pax = fare_event.passenger_count.max(0.0);
    let completed_alightings_pax = fare_event.completed_passenger_count.max(0.0);
    let (delta_revenue_base, delta_opex_base, delta_net_base) = apply_economy_realism_tick(
        manifest,
        &frame_lite,
        accrued_fare_revenue_base,
        accrued_boardings_pax,
        completed_alightings_pax,
        service_opex_per_hour,
        staff_opex_per_hour,
        dt_s,
    );
    if let Some(metrics) = manifest.progress_metrics.as_mut() {
        let served_this_tick = frame.kpis.total_trips_served.max(0.0);
        if served_this_tick.is_finite() && dt_s.is_finite() && dt_s > 1e-6 {
            let served_per_hour = (served_this_tick * (3600.0 / dt_s)).max(0.0);
            if served_per_hour.is_finite() {
                metrics.ridership = metrics.ridership.max(served_per_hour);
            }
        }
    }
    sync_progress_budget_from_economy(manifest);
    let economy = economy_snapshot_from_manifest(manifest);
    telemetry.stage_economy_ms = econ_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.strategic_refresh_due = strategic_refresh_due_hint;
    telemetry.strategic_refresh_interval_ticks = effective_strategic_refresh_interval_ticks(
        manifest.runtime_scheduling.strategic_refresh_interval_ticks,
        manifest.clock_state.speed,
    );
    telemetry.runtime_views_materialized = emit_runtime_views && trains_authoritative;
    telemetry.tick_total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
    let scenario_service_count = gs.store.scenario().world.services.len();
    let scenario_stop_count = gs.store.scenario().world.stops.len();
    let scenario_link_count = gs.store.scenario().world.links.len();
    let scenario_demand_cell_count = gs.store.scenario().world.demand_cells.len();
    let scenario_zone_count = gs.store.scenario().world.zones.len();
    let mut output_board_load_rows = 0usize;
    let mut output_cohort_rows = 0usize;
    let mut output_assigned_od_rows = 0usize;
    let mut output_mode_choice_rows = 0usize;
    if let Some(output) = gs.last_output.as_ref() {
        output_board_load_rows = output.board_loads.len();
        output_cohort_rows = output.passenger_cohorts.len();
        output_assigned_od_rows = output.assigned_od_flows.len();
        output_mode_choice_rows = output.mode_choice_results.len();
    }
    let dominant_stage = {
        let mut phases = [
            ("prepare", telemetry.stage_prepare_ms.max(0.0)),
            ("step", telemetry.stage_step_ms.max(0.0)),
            ("runtime_ops", telemetry.stage_runtime_ops_ms.max(0.0)),
            ("economy", telemetry.stage_economy_ms.max(0.0)),
        ];
        phases.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        phases.last().copied().unwrap_or(("none", 0.0))
    };
    let should_emit_tick_log = tick_index == 0
        || tick_index.is_multiple_of(RT_TICK_LOG_INTERVAL)
        || telemetry.tick_total_ms > RT_TICK_SLOW_TOTAL_MS
        || telemetry.stage_step_ms > RT_TICK_SLOW_STAGE_MS
        || telemetry.stage_prepare_ms > RT_TICK_SLOW_STAGE_MS
        || telemetry.stage_runtime_ops_ms > RT_TICK_SLOW_STAGE_MS
        || telemetry.stage_economy_ms > RT_TICK_SLOW_STAGE_MS;
    if should_emit_tick_log {
        eprintln!(
            "[rt-tick] project={} tick={} minute={} dt_s={:.3} fixed_step_s={:.3} total_ms={:.2} prepare_ms={:.2} step_ms={:.2} runtime_ops_ms={:.2} economy_ms={:.2} dominant_phase={} dominant_ms={:.2} services={} stops={} links={} demand_cells={} zones={} board_load_rows={} cohort_rows={} assigned_od_rows={} mode_choice_rows={} runtime_trains={} runtime_stations={} runtime_line_ops={} strategic_due_hint={} strategic_executed={} strategic_reason={} engine_fast_last_ms={:.2} engine_strategic_last_ms={:.2} engine_fast_steps={} engine_strategic_steps={} cache_hits={} cache_misses={} queue_depth={} dropped_steps={}",
            project_root.to_string_lossy(),
            tick_index,
            minute_of_day,
            dt_s.max(0.0),
            fixed_step_s.max(0.0),
            telemetry.tick_total_ms.max(0.0),
            telemetry.stage_prepare_ms.max(0.0),
            telemetry.stage_step_ms.max(0.0),
            telemetry.stage_runtime_ops_ms.max(0.0),
            telemetry.stage_economy_ms.max(0.0),
            dominant_stage.0,
            dominant_stage.1.max(0.0),
            scenario_service_count,
            scenario_stop_count,
            scenario_link_count,
            scenario_demand_cell_count,
            scenario_zone_count,
            output_board_load_rows,
            output_cohort_rows,
            output_assigned_od_rows,
            output_mode_choice_rows,
            runtime_trains.len(),
            runtime_stations.len(),
            runtime_line_ops.len(),
            strategic_refresh_due_hint,
            telemetry.engine_strategic_refresh_executed,
            telemetry
                .engine_strategic_refresh_reason
                .as_deref()
                .unwrap_or("none"),
            telemetry.engine_fast_last_ms.max(0.0),
            telemetry.engine_strategic_last_ms.max(0.0),
            telemetry.engine_fast_steps,
            telemetry.engine_strategic_steps,
            telemetry.engine_strategic_cache_hits,
            telemetry.engine_strategic_cache_misses,
            queue_depth,
            dropped_steps,
        );
    }
    {
        let mut materialization = state
            .runtime_materialization
            .lock()
            .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
        if let Some(current) = materialization.as_mut() {
            if current.project_path == project_root.to_string_lossy()
                && !telemetry.engine_strategic_refresh_executed
            {
                // Adaptive runtime scope should respond to steady fast/runtime cost.
                // Feeding strategic refresh wall time back into the cap can shrink the
                // scope and immediately force another strategic refresh.
                current.last_tick_ms = telemetry.tick_total_ms;
            }
        }
    }
    Ok(RuntimeSnapshot {
        project_path: project_root.to_string_lossy().to_string(),
        clock_revision,
        clock: manifest.clock_state.clone(),
        economy,
        frame: Some(frame_lite),
        delta_revenue_base,
        delta_opex_base,
        delta_net_base,
        captured_at_epoch_ms: now_epoch_ms(),
        telemetry,
        trains: runtime_trains,
        stations: runtime_stations,
        line_ops: runtime_line_ops,
        provenance_warnings,
        trains_authoritative,
        passenger_counter_provenance: CounterProvenance::RuntimeProjection,
        fare_counter_provenance,
    })
}
