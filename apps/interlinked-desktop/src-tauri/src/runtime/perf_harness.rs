use super::snapshots::{
    latest_runtime_fast_snapshot_for_project, latest_runtime_snapshot_for_project,
    publish_runtime_snapshots, publish_strategic_snapshot_for_tick,
};
use crate::*;
use interlinked_engine::model::{Link, Meta, Scenario, Service, Stop, World, Zone};
use interlinked_engine::platform::{ScenarioDocument, SimulationService};
use interlinked_engine::sim::types::StrategicPlannerTimingDiagnostics;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
struct RuntimePerfScenarioScale {
    name: &'static str,
    stop_count: usize,
    line_count: usize,
    cohorts_per_service: usize,
    queue_pax_per_cohort: f64,
    warmup_iterations: u64,
    iterations: u64,
}

#[derive(Debug, Default)]
struct RuntimePerfStats {
    tick_total_ms: Vec<f64>,
    prepare_ms: Vec<f64>,
    engine_step_ms: Vec<f64>,
    engine_fast_last_ms: Vec<f64>,
    runtime_ops_ms: Vec<f64>,
    economy_ms: Vec<f64>,
    publish_ms: Vec<f64>,
    latest_fast_read_ms: Vec<f64>,
    latest_combined_read_ms: Vec<f64>,
    size_sample_ms: Vec<f64>,
    fast_tick_total_ms: Vec<f64>,
    strategic_tick_total_ms: Vec<f64>,
}

#[derive(Debug)]
struct RuntimePerfReport {
    scenario_name: &'static str,
    iterations: u64,
    stop_count: usize,
    service_count: usize,
    link_count: usize,
    initial_queue_cohorts: usize,
    final_queue_cohorts: usize,
    final_onboard_cohorts: usize,
    train_count: usize,
    station_view_count: usize,
    line_ops_count: usize,
    snapshot_size_bytes: usize,
    fare_source: CounterProvenance,
    fare_source_label: String,
    lifecycle_clean: bool,
    max_queue_balance_error: f64,
    max_onboard_balance_error: f64,
    fast_tick_count: usize,
    strategic_refresh_count: usize,
    strategic_refresh_reason_counts: BTreeMap<String, usize>,
    adaptive_cap_counts: BTreeMap<usize, usize>,
    first_strategic_refresh_reason: Option<String>,
    first_strategic_planner_timing: Option<StrategicPlannerTimingDiagnostics>,
    stats: RuntimePerfStats,
}

impl RuntimePerfStats {
    fn push_snapshot(&mut self, snapshot: &RuntimeSnapshot) {
        self.tick_total_ms.push(snapshot.telemetry.tick_total_ms);
        self.prepare_ms.push(snapshot.telemetry.stage_prepare_ms);
        self.engine_step_ms.push(snapshot.telemetry.stage_step_ms);
        self.engine_fast_last_ms
            .push(snapshot.telemetry.engine_fast_last_ms);
        self.runtime_ops_ms
            .push(snapshot.telemetry.stage_runtime_ops_ms);
        self.economy_ms.push(snapshot.telemetry.stage_economy_ms);
        if snapshot.telemetry.engine_strategic_refresh_executed {
            self.strategic_tick_total_ms
                .push(snapshot.telemetry.tick_total_ms);
        } else {
            self.fast_tick_total_ms
                .push(snapshot.telemetry.tick_total_ms);
        }
    }
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn max(values: &[f64]) -> f64 {
    values
        .iter()
        .copied()
        .fold(0.0_f64, |acc, value| acc.max(value.max(0.0)))
}

fn unique_tmp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("interlinked_runtime_perf_{nanos}_{name}"))
}

fn synthetic_runtime_scenario(scale: RuntimePerfScenarioScale) -> Scenario {
    let mut stops = Vec::<Stop>::new();
    for idx in 0..scale.stop_count {
        stops.push(Stop {
            id: format!("stop_{idx:03}"),
            name: Some(format!("Stop {idx}")),
            x: idx as f64 * 850.0,
            y: (idx % 7) as f64 * 120.0,
            country_iso2: Some("GB".to_string()),
            interchange_id: None,
            stop_type: Some("metro_station".to_string()),
            station_boarding_capacity_pph: Some(36_000.0),
            station_alighting_capacity_pph: Some(36_000.0),
            station_queue_capacity_pax: Some(25_000.0),
        });
    }

    let mut links = Vec::<Link>::new();
    for idx in 1..scale.stop_count {
        let from = format!("stop_{:03}", idx - 1);
        let to = format!("stop_{idx:03}");
        links.push(Link {
            id: format!("link_fwd_{idx:03}"),
            from_stop: from.clone(),
            to_stop: to.clone(),
            distance_m: 850.0,
            mode: "metro".to_string(),
            speed_mps: 18.0,
            geometry: None,
            line_id: Some("synthetic_runtime".to_string()),
            mode_variant: None,
            capacity_per_hour: Some(36_000.0),
        });
        links.push(Link {
            id: format!("link_rev_{idx:03}"),
            from_stop: to,
            to_stop: from,
            distance_m: 850.0,
            mode: "metro".to_string(),
            speed_mps: 18.0,
            geometry: None,
            line_id: Some("synthetic_runtime".to_string()),
            mode_variant: None,
            capacity_per_hour: Some(36_000.0),
        });
    }

    let mut services = Vec::<Service>::new();
    let span = (scale.stop_count / scale.line_count.max(1)).max(3);
    for line_idx in 0..scale.line_count {
        let start = line_idx.saturating_mul(span / 2).min(scale.stop_count - 2);
        let end = (start + span).min(scale.stop_count);
        let mut sequence = (start..end)
            .map(|idx| format!("stop_{idx:03}"))
            .collect::<Vec<_>>();
        if sequence.len() < 3 {
            sequence = (0..scale.stop_count.min(3))
                .map(|idx| format!("stop_{idx:03}"))
                .collect();
        }
        let mut reverse_sequence = sequence.clone();
        reverse_sequence.reverse();
        let line_id = format!("line_{line_idx:03}");
        let unit_count = (scale.cohorts_per_service.max(2) as u32).clamp(2, 48);
        for (direction, stop_sequence) in [("fwd", sequence), ("rev", reverse_sequence)] {
            services.push(Service {
                id: format!("svc_{line_idx:03}_{direction}"),
                line_id: Some(line_id.clone()),
                name: Some(format!("Synthetic {line_idx} {direction}")),
                mode: "metro".to_string(),
                mode_variant: None,
                stop_sequence,
                direction: Some(direction.to_string()),
                direction_name: Some(direction.to_string()),
                display_color: Some("#3366cc".to_string()),
                service_enabled: Some(true),
                operating_tph: Some(12.0),
                stock_tier_id: Some("synthetic_metro".to_string()),
                stock_units_owned: Some(unit_count),
                stock_units_assigned: Some(unit_count),
                rolling_stock_profile: None,
                schedule_profile: None,
                headway_s: 300.0,
                dwell_s: 15.0,
                vehicle_capacity: 420.0,
                board_penalty_s: None,
            });
        }
    }

    let zones = (0..scale.stop_count)
        .map(|idx| Zone {
            id: format!("zone_{idx:03}"),
            x: stops[idx].x,
            y: stops[idx].y,
            population: 5_000.0,
            jobs: 4_000.0,
            country_iso2: Some("GB".to_string()),
        })
        .collect::<Vec<_>>();

    Scenario {
        meta: Meta {
            name: format!("runtime-perf-{}", scale.name),
            seed: 42,
            time_period_hours: 1.0,
            crs: Crs::Epsg3857,
        },
        params: default_params(),
        world: World {
            zones,
            stops,
            links,
            services,
            transfers: Vec::new(),
            transfer_rules: None,
            demand_cells: Vec::new(),
            demand_meta: None,
        },
    }
}

fn synthetic_project_manifest(scale: RuntimePerfScenarioScale) -> ProjectManifest {
    let mut fare_policy = default_fare_policy_manifest();
    fare_policy.enabled = true;
    let mut runtime_scheduling = default_runtime_scheduling_manifest();
    runtime_scheduling.snapshot_ring = 1;
    runtime_scheduling.strategic_refresh_interval_ticks = 10_000;
    runtime_scheduling.lightweight_tick_outputs = true;
    runtime_scheduling.runtime_ops_kernel_v1 = true;
    runtime_scheduling.ui_runtime_trains_v1 = true;
    runtime_scheduling.fare_recognition_v1 = true;

    ProjectManifest {
        project_id: format!("runtime-perf-{}", scale.name),
        name: format!("Runtime Perf {}", scale.name),
        created_at: "0".to_string(),
        updated_at: "0".to_string(),
        session_kind: SessionKind::Game,
        engine_schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
        ui_schema_version: 2,
        last_opened_run_id: None,
        recent_runs: Vec::new(),
        clock_state: default_clock_for(&SessionKind::Game),
        progress_metrics: Some(default_progress_metrics()),
        start_location: Some(StartLocation {
            country_iso2: "GB".to_string(),
            country_name: "United Kingdom".to_string(),
            city_id: 1,
            city_name: "Synthetic".to_string(),
            city_lon: 0.0,
            city_lat: 51.0,
            city_population: Some(1_000_000),
        }),
        economy: EconomyManifest {
            currency: default_currency_code(),
            difficulty: default_difficulty_label(),
            difficulty_profile: difficulty_profile_for_label("standard"),
            economy_revision: 1,
            starting_budget_base: 1_000_000_000.0,
            current_balance_base: 1_000_000_000.0,
            cumulative_capex_base: 0.0,
            cumulative_opex_base: 0.0,
            cumulative_revenue_base: 0.0,
            cumulative_lost_demand_penalty_base: 0.0,
            fare_revenue_deferred_base: 0.0,
            fare_boardings_deferred_pax: 0.0,
            fare_policy,
            unlocked_countries: vec!["GB".to_string()],
            region_ledger: BTreeMap::new(),
            maintenance_rate: default_maintenance_rate(),
            ancillary_revenue_rate: default_ancillary_revenue_rate(),
            quality_penalty_rates: default_quality_penalty_rates(),
            monthly_financials: Vec::new(),
        },
        demand_surface: Some(default_demand_surface_manifest()),
        region_state: RegionStateManifest {
            unlocked_region_ids: vec!["synthetic".to_string()],
            primary_focus_region_id: Some("synthetic".to_string()),
            active_region_ids: vec!["synthetic".to_string()],
        },
        simulation_scope: default_simulation_scope_manifest(),
        runtime_scheduling,
        pack_refs: Vec::new(),
    }
}

fn test_app_state_with_game(game: interlinked_engine::platform::GameState) -> AppState {
    AppState {
        game: Mutex::new(Some(game)),
        current_project: Mutex::new(None),
        runtime_tick: Mutex::new(None),
        runtime_loop: Mutex::new(None),
        runtime_snapshots: Mutex::new(VecDeque::new()),
        runtime_fast_snapshots: Mutex::new(VecDeque::new()),
        runtime_strategic_snapshots: Mutex::new(VecDeque::new()),
        runtime_strategic_demand_cache: Mutex::new(HashMap::new()),
        runtime_materialization: Mutex::new(None),
        runtime_ops: Mutex::new(None),
    }
}

fn seed_runtime_cohorts(
    gs: &mut interlinked_engine::platform::GameState,
    scale: RuntimePerfScenarioScale,
) -> usize {
    let mut seeded = 0usize;
    let scenario = gs.store.scenario();
    for service in &scenario.world.services {
        if service.stop_sequence.len() < 2 {
            continue;
        }
        let max_origins = service
            .stop_sequence
            .len()
            .saturating_sub(1)
            .min(scale.cohorts_per_service);
        for idx in 0..max_origins {
            let origin = service.stop_sequence[idx].clone();
            let destination_index = (idx + 1 + (seeded % 3)).min(service.stop_sequence.len() - 1);
            let destination = service.stop_sequence[destination_index].clone();
            let pax = scale.queue_pax_per_cohort + (idx % 5) as f64;
            gs.sim_state.queue_cohorts.insert(
                (service.id.clone(), origin.clone(), destination),
                pax.max(1.0),
            );
            gs.sim_state
                .queue
                .insert((service.id.clone(), origin.clone()), pax.max(1.0));
            gs.sim_state
                .time_to_next_departure_s
                .insert((service.id.clone(), origin), 0.0);
            seeded = seeded.saturating_add(1);
        }
    }
    seeded
}

fn run_perf_scenario(scale: RuntimePerfScenarioScale) -> RuntimePerfReport {
    let project_root = unique_tmp_path(scale.name);
    fs::create_dir_all(&project_root).expect("create perf project temp root");
    let scenario = synthetic_runtime_scenario(scale);
    let doc = ScenarioDocument {
        schema_version: ScenarioDocument::CURRENT_SCHEMA_VERSION,
        scenario,
    };
    let mut gs = SimulationService::init_game_state(&doc);
    let initial_queue_cohorts = seed_runtime_cohorts(&mut gs, scale);
    let state = test_app_state_with_game(gs);
    let mut manifest = synthetic_project_manifest(scale);
    manifest.clock_state.running = true;
    manifest.clock_state.speed = 1;

    let mut stats = RuntimePerfStats::default();
    let mut last_snapshot = None::<RuntimeSnapshot>;
    let mut snapshot_size_bytes = 0usize;
    let mut max_queue_balance_error = 0.0_f64;
    let mut max_onboard_balance_error = 0.0_f64;
    let mut strategic_refresh_reason_counts = BTreeMap::<String, usize>::new();
    let mut adaptive_cap_counts = BTreeMap::<usize, usize>::new();
    let mut first_strategic_refresh_reason = None::<String>;
    let mut first_strategic_planner_timing = None::<StrategicPlannerTimingDiagnostics>;

    let total_iterations = scale.warmup_iterations.saturating_add(scale.iterations);
    for tick_index in 1..=total_iterations {
        let snapshot = run_simulation_tick(
            &state,
            &project_root,
            &mut manifest,
            0.5,
            0.5,
            false,
            tick_index,
            tick_index,
            0,
            0,
            true,
            false,
        )
        .expect("perf scenario tick should run");

        if snapshot.telemetry.engine_strategic_refresh_executed
            && first_strategic_planner_timing.is_none()
        {
            first_strategic_refresh_reason = snapshot
                .telemetry
                .engine_strategic_refresh_reason
                .clone()
                .or_else(|| Some("unknown".to_string()));
            first_strategic_planner_timing = state
                .game
                .lock()
                .expect("game mutex should not be poisoned")
                .as_ref()
                .and_then(|gs| gs.last_output.as_ref())
                .map(|out| out.strategic_planner_timing.clone());
        }

        if tick_index <= scale.warmup_iterations {
            continue;
        }

        if let Some(diag) = snapshot.telemetry.lifecycle_diagnostics.as_ref() {
            max_queue_balance_error = max_queue_balance_error.max(diag.queue_balance_error.abs());
            max_onboard_balance_error =
                max_onboard_balance_error.max(diag.onboard_balance_error.abs());
        }
        stats.push_snapshot(&snapshot);
        let refresh_reason = if snapshot.telemetry.engine_strategic_refresh_executed {
            snapshot
                .telemetry
                .engine_strategic_refresh_reason
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "no_refresh".to_string()
        };
        *strategic_refresh_reason_counts
            .entry(refresh_reason)
            .or_insert(0) += 1;
        *adaptive_cap_counts
            .entry(snapshot.telemetry.adaptive_max_active_zones)
            .or_insert(0) += 1;

        let size_started = Instant::now();
        snapshot_size_bytes = serde_json::to_vec(&snapshot)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        stats
            .size_sample_ms
            .push(size_started.elapsed().as_secs_f64() * 1000.0);

        let publish_strategic = publish_strategic_snapshot_for_tick(&snapshot);
        let publish_started = Instant::now();
        publish_runtime_snapshots(
            &state,
            snapshot,
            manifest.runtime_scheduling.snapshot_ring,
            publish_strategic,
        )
        .expect("perf scenario snapshot publish should succeed");
        stats
            .publish_ms
            .push(publish_started.elapsed().as_secs_f64() * 1000.0);

        let fast_read_started = Instant::now();
        let latest_fast =
            latest_runtime_fast_snapshot_for_project(&state, &project_root.to_string_lossy())
                .expect("latest fast snapshot read should succeed");
        stats
            .latest_fast_read_ms
            .push(fast_read_started.elapsed().as_secs_f64() * 1000.0);
        assert!(latest_fast.is_some());

        let combined_read_started = Instant::now();
        let latest_combined =
            latest_runtime_snapshot_for_project(&state, &project_root.to_string_lossy())
                .expect("latest combined snapshot read should succeed");
        stats
            .latest_combined_read_ms
            .push(combined_read_started.elapsed().as_secs_f64() * 1000.0);
        last_snapshot = latest_combined;
    }

    let snapshot = last_snapshot.expect("perf scenario should publish at least one snapshot");
    let (final_queue_cohorts, final_onboard_cohorts) = {
        let guard = state
            .game
            .lock()
            .expect("game mutex should not be poisoned");
        let gs = guard.as_ref().expect("game state should exist");
        (
            gs.sim_state.queue_cohorts.len(),
            gs.sim_state.onboard_cohorts.len(),
        )
    };
    let fare_source = snapshot
        .telemetry
        .fare_source_diagnostic
        .as_ref()
        .map(|diag| diag.selected_provenance)
        .unwrap_or(CounterProvenance::DebugLegacy);
    let fare_source_label = snapshot
        .telemetry
        .fare_source_diagnostic
        .as_ref()
        .map(|diag| diag.selected_source_label.clone())
        .unwrap_or_else(|| "none".to_string());
    let lifecycle_clean = max_queue_balance_error <= 1e-6 && max_onboard_balance_error <= 1e-6;

    let report = RuntimePerfReport {
        scenario_name: scale.name,
        iterations: scale.iterations,
        stop_count: doc.scenario.world.stops.len(),
        service_count: doc.scenario.world.services.len(),
        link_count: doc.scenario.world.links.len(),
        initial_queue_cohorts,
        final_queue_cohorts,
        final_onboard_cohorts,
        train_count: snapshot.trains.len(),
        station_view_count: snapshot.stations.len(),
        line_ops_count: snapshot.line_ops.len(),
        snapshot_size_bytes,
        fare_source,
        fare_source_label,
        lifecycle_clean,
        max_queue_balance_error,
        max_onboard_balance_error,
        fast_tick_count: stats.fast_tick_total_ms.len(),
        strategic_refresh_count: stats.strategic_tick_total_ms.len(),
        strategic_refresh_reason_counts,
        adaptive_cap_counts,
        first_strategic_refresh_reason,
        first_strategic_planner_timing,
        stats,
    };

    let _ = fs::remove_dir_all(&project_root);
    report
}

fn print_report(report: &RuntimePerfReport) {
    eprintln!(
        "[runtime-perf] scenario={} iterations={} fast_ticks={} strategic_ticks={} stops={} services={} links={} trains={} stations={} line_ops={} queue_cohorts={}=>{} onboard_cohorts={} snapshot_bytes={} lifecycle_clean={} queue_err_max={:.6} onboard_err_max={:.6} fare_source={:?}:{}",
        report.scenario_name,
        report.iterations,
        report.fast_tick_count,
        report.strategic_refresh_count,
        report.stop_count,
        report.service_count,
        report.link_count,
        report.train_count,
        report.station_view_count,
        report.line_ops_count,
        report.initial_queue_cohorts,
        report.final_queue_cohorts,
        report.final_onboard_cohorts,
        report.snapshot_size_bytes,
        report.lifecycle_clean,
        report.max_queue_balance_error,
        report.max_onboard_balance_error,
        report.fare_source,
        report.fare_source_label,
    );
    eprintln!(
        "[runtime-perf] scenario={} avg_ms tick={:.3} fast_tick={:.3} strategic_tick={:.3} prepare={:.3} step={:.3} engine_fast={:.3} runtime_ops={:.3} economy={:.3} publish={:.3} latest_fast_read={:.3} latest_combined_read={:.3} size_sample={:.3}",
        report.scenario_name,
        avg(&report.stats.tick_total_ms),
        avg(&report.stats.fast_tick_total_ms),
        avg(&report.stats.strategic_tick_total_ms),
        avg(&report.stats.prepare_ms),
        avg(&report.stats.engine_step_ms),
        avg(&report.stats.engine_fast_last_ms),
        avg(&report.stats.runtime_ops_ms),
        avg(&report.stats.economy_ms),
        avg(&report.stats.publish_ms),
        avg(&report.stats.latest_fast_read_ms),
        avg(&report.stats.latest_combined_read_ms),
        avg(&report.stats.size_sample_ms),
    );
    eprintln!(
        "[runtime-perf] scenario={} max_ms tick={:.3} step={:.3} runtime_ops={:.3} publish={:.3} latest_combined_read={:.3}",
        report.scenario_name,
        max(&report.stats.tick_total_ms),
        max(&report.stats.engine_step_ms),
        max(&report.stats.runtime_ops_ms),
        max(&report.stats.publish_ms),
        max(&report.stats.latest_combined_read_ms),
    );
    eprintln!(
        "[runtime-perf] scenario={} strategic_refresh_reasons={:?} adaptive_caps={:?}",
        report.scenario_name, report.strategic_refresh_reason_counts, report.adaptive_cap_counts,
    );
    if let Some(timing) = report.first_strategic_planner_timing.as_ref() {
        eprintln!(
            "[runtime-perf] scenario={} first_strategic reason={} mode={} total_ms={:.3} materialize={:.3} validate_index={:.3} graph={:.3} latent={:.3} pending_od={:.3} mode_choice={:.3} assignment={:.3} lightweight_outputs={:.3} full_layers={:.3} operations={:.3} modal={:.3} phase3={:.3} economics={:.3} demand_diag={:.3} temporal={:.3}",
            report.scenario_name,
            report
                .first_strategic_refresh_reason
                .as_deref()
                .unwrap_or("unknown"),
            timing.output_mode,
            timing.total_ms,
            timing.materialize_effective_scenario_ms,
            timing.validate_index_ms,
            timing.graph_build_ms,
            timing.latent_demand_ms,
            timing.pending_od_ms,
            timing.mode_choice_ms,
            timing.assignment_ms,
            timing.lightweight_outputs_ms,
            timing.full_layers_ms,
            timing.operations_ms,
            timing.modal_outputs_ms,
            timing.phase3_outputs_ms,
            timing.economics_outputs_ms,
            timing.demand_diagnostics_ms,
            timing.temporal_bundle_ms,
        );
        eprintln!(
            "[runtime-perf] scenario={} first_strategic_counts zones={} stops={} links={} services={} demand_cells={} graph_nodes={} graph_edges={} active_latent={} mode_choice_rows={} assigned_od={} board_loads={} cohorts={} mode_paths_raw={} mode_paths_boardable={} assignment_paths_raw={} assignment_paths_boardable={} assignment_cache_iter={}/{} assignment_cache_kpi={}/{} attempted_pax={:.3}",
            report.scenario_name,
            timing.zones,
            timing.stops,
            timing.links,
            timing.services,
            timing.demand_cells,
            timing.graph_nodes,
            timing.graph_edges,
            timing.active_latent_rows,
            timing.mode_choice_rows,
            timing.assigned_od_rows,
            timing.board_load_rows,
            timing.passenger_cohort_rows,
            timing.mode_choice_candidate_paths_raw_total,
            timing.mode_choice_candidate_paths_boardable_total,
            timing.assignment_candidate_paths_raw_total,
            timing.assignment_candidate_paths_boardable_total,
            timing.assignment_iter_path_cache_hits,
            timing.assignment_iter_path_cache_misses,
            timing.assignment_kpi_path_cache_hits,
            timing.assignment_kpi_path_cache_misses,
            timing.assignment_attempted_pax_total,
        );
    }
}

#[test]
fn runtime_perf_scenario_builder_smoke() {
    let report = run_perf_scenario(RuntimePerfScenarioScale {
        name: "smoke",
        stop_count: 8,
        line_count: 2,
        cohorts_per_service: 3,
        queue_pax_per_cohort: 12.0,
        warmup_iterations: 1,
        iterations: 2,
    });

    assert_eq!(report.stop_count, 8);
    assert_eq!(report.service_count, 4);
    assert!(report.initial_queue_cohorts > 0);
    assert!(report.train_count > 0);
    assert!(report.snapshot_size_bytes > 0);
    assert!(
        report.max_onboard_balance_error <= 1e-6,
        "onboard conservation should remain clean in synthetic runtime perf scenario"
    );
    let counted_refreshes: usize = report
        .strategic_refresh_reason_counts
        .iter()
        .filter(|(reason, _)| reason.as_str() != "no_refresh")
        .map(|(_, count)| *count)
        .sum();
    assert_eq!(counted_refreshes, report.strategic_refresh_count);
    assert!(!report.adaptive_cap_counts.is_empty());
    let first_timing = report
        .first_strategic_planner_timing
        .as_ref()
        .expect("warmup strategic refresh should record planner timing");
    assert_eq!(first_timing.source_label, "engine_strategic_planner");
    assert!(first_timing.total_ms >= 0.0);
    assert!(first_timing.graph_build_ms >= 0.0);
    assert!(first_timing.assignment_ms >= 0.0);
    assert_ne!(report.fare_source, CounterProvenance::RuntimeProjection);
}

#[test]
#[ignore = "manual runtime perf harness; run with --ignored --nocapture for timings"]
fn runtime_perf_scenarios_report() {
    let scales = [
        RuntimePerfScenarioScale {
            name: "small",
            stop_count: 12,
            line_count: 3,
            cohorts_per_service: 4,
            queue_pax_per_cohort: 16.0,
            warmup_iterations: 4,
            iterations: 8,
        },
        RuntimePerfScenarioScale {
            name: "medium",
            stop_count: 48,
            line_count: 8,
            cohorts_per_service: 12,
            queue_pax_per_cohort: 24.0,
            warmup_iterations: 6,
            iterations: 8,
        },
        RuntimePerfScenarioScale {
            name: "stressish",
            stop_count: 96,
            line_count: 16,
            cohorts_per_service: 24,
            queue_pax_per_cohort: 32.0,
            warmup_iterations: 8,
            iterations: 6,
        },
    ];

    for scale in scales {
        let report = run_perf_scenario(scale);
        assert!(
            report.max_onboard_balance_error <= 1e-6,
            "onboard conservation must remain clean for {}",
            report.scenario_name
        );
        assert_ne!(
            report.fare_source,
            CounterProvenance::RuntimeProjection,
            "projection fare fallback should not be selected in perf scenario {}",
            report.scenario_name
        );
        print_report(&report);
    }
}
