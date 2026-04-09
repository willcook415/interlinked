use crate::*;

pub(crate) use super::defaults::*;
pub(crate) use super::fare::*;
pub(crate) use super::materialization::*;
pub(crate) use super::models::*;
pub(crate) use super::train_kernel::*;
pub(crate) use super::views::*;

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
    gs.run_cfg.strategic_refresh_interval_steps = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
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
        provenance_warnings,
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
    let (accrued_fare_revenue_base, accrued_boardings_pax, mut completed_alightings_pax) =
        if fare_recognition_enabled {
            if manifest.session_kind == SessionKind::Game && trains_authoritative {
                let boarded = runtime_fare_events.boarded_pax.max(0.0);
                let completed = runtime_fare_events.completed_alightings_pax.max(0.0);
                let liability = runtime_fare_events.liability_accrued_base.max(0.0);
                (liability, boarded, completed)
            } else {
                fare_flow_for_economy(gs)
            }
        } else {
            let served = frame_lite.kpis.total_boardings_served.max(0.0);
            (
                frame_lite.kpis.total_fare_revenue_base.max(0.0),
                served,
                served,
            )
        };
    completed_alightings_pax = completed_alightings_pax.max(0.0);
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
        metrics.ridership += frame.kpis.total_trips_served.max(0.0);
    }
    sync_progress_budget_from_economy(manifest);
    let economy = SimulationAdvanceEconomy {
        current_balance_base: manifest.economy.current_balance_base,
        cumulative_revenue_base: manifest.economy.cumulative_revenue_base,
        cumulative_opex_base: manifest.economy.cumulative_opex_base,
        budget_display: manifest
            .progress_metrics
            .as_ref()
            .map(|m| m.budget)
            .unwrap_or(manifest.economy.current_balance_base),
    };
    telemetry.stage_economy_ms = econ_start.elapsed().as_secs_f64() * 1000.0;
    telemetry.strategic_refresh_due = strategic_refresh_due_hint;
    telemetry.strategic_refresh_interval_ticks = manifest
        .runtime_scheduling
        .strategic_refresh_interval_ticks
        .max(1);
    telemetry.runtime_views_materialized = emit_runtime_views && trains_authoritative;
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
    })
}
