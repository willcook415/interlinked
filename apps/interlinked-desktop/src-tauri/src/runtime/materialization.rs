use crate::*;
use std::time::Instant;

use super::fare::engine_fare_policy_context_from_scenario_manifest;
use super::models::RuntimeMaterializationState;
use interlinked_engine::sim::EngineFarePolicyContext;

const RT_MAT_REUSE_LOG_INTERVAL: u64 = 120;

pub(crate) fn adaptive_runtime_zone_cap(
    manifest: &ProjectManifest,
    scenario: &Scenario,
    previous_cap: usize,
    previous_tick_ms: f64,
) -> usize {
    let focus_cap = manifest
        .simulation_scope
        .focus_max_active_zones
        .clamp(120, 6000);
    let floor_cap = manifest
        .simulation_scope
        .remote_max_active_zones
        .clamp(20, focus_cap);
    let base_cap = manifest
        .simulation_scope
        .max_active_zones
        .clamp(floor_cap, focus_cap);
    let stop_count = scenario.world.stops.len();
    let network_cap = if stop_count <= 25 {
        48
    } else if stop_count <= 80 {
        96
    } else if stop_count <= 200 {
        160
    } else if stop_count <= 500 {
        240
    } else if stop_count <= 1_200 {
        320
    } else {
        420
    };
    let mut cap = previous_cap
        .clamp(floor_cap, focus_cap)
        .min(base_cap)
        .min(network_cap);
    let target_ms = manifest.runtime_scheduling.target_tick_ms.clamp(4.0, 250.0);
    if previous_tick_ms > 0.0 {
        if previous_tick_ms > target_ms * 1.35 {
            cap = ((cap as f64) * 0.82).round() as usize;
        } else if previous_tick_ms < target_ms * 0.70 {
            cap = ((cap as f64) * 1.08).round() as usize;
        }
    }
    cap.clamp(floor_cap, focus_cap)
}

pub(crate) fn ensure_runtime_materialized_scenario(
    state: &AppState,
    project_path: &str,
    gs: &mut interlinked_engine::platform::GameState,
    manifest: &ProjectManifest,
    minute_of_day: u32,
    tick_index: u64,
) -> Result<usize, String> {
    let total_started = Instant::now();
    let topology_hash = scenario_topology_hash(gs.store.scenario());
    let active_scope_hash = scope_hash(manifest);
    let active_fare_hash = fare_hash(&manifest.economy.fare_policy);
    let existing = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?
        .clone();
    let mut materialization = existing.unwrap_or(RuntimeMaterializationState {
        project_path: project_path.to_string(),
        topology_hash: 0,
        scope_hash: 0,
        fare_hash: 0,
        minute_of_day,
        last_materialized_tick: 0,
        adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
        candidate_adaptive_max_active_zones: manifest
            .simulation_scope
            .max_active_zones
            .clamp(120, 5000),
        last_tick_ms: 0.0,
        fare_policy_context: EngineFarePolicyContext::default(),
    });
    if materialization.project_path != project_path {
        materialization = RuntimeMaterializationState {
            project_path: project_path.to_string(),
            topology_hash: 0,
            scope_hash: 0,
            fare_hash: 0,
            minute_of_day,
            last_materialized_tick: 0,
            adaptive_max_active_zones: manifest.simulation_scope.max_active_zones.clamp(120, 5000),
            candidate_adaptive_max_active_zones: manifest
                .simulation_scope
                .max_active_zones
                .clamp(120, 5000),
            last_tick_ms: 0.0,
            fare_policy_context: EngineFarePolicyContext::default(),
        };
    }
    let adaptive_cap = adaptive_runtime_zone_cap(
        manifest,
        gs.store.scenario(),
        materialization.candidate_adaptive_max_active_zones,
        materialization.last_tick_ms,
    );
    let remote_interval = manifest
        .simulation_scope
        .remote_update_interval_ticks
        .max(1) as u64;
    let cap_delta = adaptive_cap.abs_diff(materialization.adaptive_max_active_zones);
    let cap_rebalance_due = cap_delta >= 32
        && tick_index.saturating_sub(materialization.last_materialized_tick) >= remote_interval;
    let reason_topology = topology_hash != materialization.topology_hash;
    let reason_scope = active_scope_hash != materialization.scope_hash;
    let reason_fare = active_fare_hash != materialization.fare_hash;
    let reason_minute = minute_of_day != materialization.minute_of_day;
    let reason_cap = cap_rebalance_due;
    let needs_materialization =
        reason_topology || reason_scope || reason_fare || reason_minute || reason_cap;
    let active_cap = if needs_materialization {
        adaptive_cap
    } else {
        materialization.adaptive_max_active_zones
    };
    if needs_materialization {
        let materialize_started = Instant::now();
        // Runtime materialization mutates only the in-memory GameState scenario.
        // Persisted demand authority remains in scenario/current.scenario.json.
        let cfg = economy_config();
        let mut materialized = gs.store.scenario().clone();
        let before_cells_total = materialized.world.demand_cells.len();
        let before_cells_nonzero = materialized
            .world
            .demand_cells
            .iter()
            .filter(|cell| cell.residents_night > 0.0 || cell.jobs_day > 0.0)
            .count();
        let before_zones_total = materialized.world.zones.len();
        let before_zones_nonzero = materialized
            .world
            .zones
            .iter()
            .filter(|zone| zone.population > 0.0 || zone.jobs > 0.0)
            .count();
        strip_auto_reverse_runtime_artifacts(&mut materialized);
        apply_game_runtime_demand_tuning(&mut materialized.params);
        apply_fare_policy_to_params(&mut materialized.params, &manifest.economy.fare_policy);
        synthesize_auto_reverse_runtime_services(&mut materialized);
        materialize_line_operations_for_minute(&mut materialized, &cfg, minute_of_day);
        apply_runtime_transient_demand_perf_budget(&mut materialized, adaptive_cap);
        materialization.fare_policy_context = engine_fare_policy_context_from_scenario_manifest(
            &materialized,
            &manifest.economy.fare_policy,
        );
        let after_cells_total = materialized.world.demand_cells.len();
        let after_cells_nonzero = materialized
            .world
            .demand_cells
            .iter()
            .filter(|cell| cell.residents_night > 0.0 || cell.jobs_day > 0.0)
            .count();
        let after_zones_total = materialized.world.zones.len();
        let after_zones_nonzero = materialized
            .world
            .zones
            .iter()
            .filter(|zone| zone.population > 0.0 || zone.jobs > 0.0)
            .count();
        let materialize_ms = materialize_started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "[rt-mat] refresh project={} tick={} minute={} reasons=topology:{}|scope:{}|fare:{}|minute:{}|cap:{} active_regions={} remote_interval={} cap={}=>{} cap_delta={} materialize_ms={:.2} total_ms={:.2} stops={} services={} demand_cells={}=>{} nonzero_cells={}=>{} zones={}=>{} nonzero_zones={}=>{}",
            project_path,
            tick_index,
            minute_of_day,
            reason_topology,
            reason_scope,
            reason_fare,
            reason_minute,
            reason_cap,
            manifest.region_state.active_region_ids.len(),
            remote_interval,
            materialization.adaptive_max_active_zones,
            adaptive_cap,
            cap_delta,
            materialize_ms.max(0.0),
            total_started.elapsed().as_secs_f64() * 1000.0,
            materialized.world.stops.len(),
            materialized.world.services.len(),
            before_cells_total,
            after_cells_total,
            before_cells_nonzero,
            after_cells_nonzero,
            before_zones_total,
            after_zones_total,
            before_zones_nonzero,
            after_zones_nonzero,
        );
        gs.store = ScenarioStore::new(materialized);
        materialization.topology_hash = topology_hash;
        materialization.scope_hash = active_scope_hash;
        materialization.fare_hash = active_fare_hash;
        materialization.minute_of_day = minute_of_day;
        materialization.last_materialized_tick = tick_index;
        materialization.adaptive_max_active_zones = adaptive_cap;
        materialization.candidate_adaptive_max_active_zones = adaptive_cap;
    } else {
        if tick_index.is_multiple_of(RT_MAT_REUSE_LOG_INTERVAL) || cap_delta >= 8 {
            eprintln!(
                "[rt-mat] reuse project={} tick={} minute={} active_regions={} remote_interval={} cap={}=>{} cap_delta={} last_materialized_tick={} elapsed_since_materialized={} total_ms={:.2}",
                project_path,
                tick_index,
                minute_of_day,
                manifest.region_state.active_region_ids.len(),
                remote_interval,
                materialization.adaptive_max_active_zones,
                adaptive_cap,
                cap_delta,
                materialization.last_materialized_tick,
                tick_index.saturating_sub(materialization.last_materialized_tick),
                total_started.elapsed().as_secs_f64() * 1000.0,
            );
        }
        // Keep the engine scope pinned to the materialized cap until a real
        // cap rebalance occurs. The candidate may drift with fast-tick feedback,
        // but returning it early changes the strategic scope signature without
        // rematerializing demand inputs.
        materialization.candidate_adaptive_max_active_zones = adaptive_cap;
    }
    let mut guard = state
        .runtime_materialization
        .lock()
        .map_err(|_| "runtime_materialization mutex poisoned".to_string())?;
    gs.run_cfg.fare_policy_context = materialization.fare_policy_context.clone();
    *guard = Some(materialization);
    Ok(active_cap)
}

pub(crate) fn runtime_has_due_purchase_orders(scenario: &Scenario, now_tick_s: f64) -> bool {
    if !now_tick_s.is_finite() || now_tick_s < 0.0 {
        return false;
    }
    if cfg!(debug_assertions) {
        return scenario.world.services.iter().any(|service| {
            service
                .rolling_stock_profile
                .as_ref()
                .map(|profile| profile.pending_orders.iter().any(|order| order.units > 0))
                .unwrap_or(false)
        });
    }
    scenario.world.services.iter().any(|service| {
        service
            .rolling_stock_profile
            .as_ref()
            .map(|profile| {
                profile.pending_orders.iter().any(|order| {
                    order
                        .eta_at_tick_s
                        .map(|eta| eta.is_finite() && eta <= now_tick_s + 1e-6)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}
