use super::snapshots::{
    latest_runtime_fast_snapshot_for_project, latest_runtime_snapshot_for_project,
    publish_runtime_snapshots, publish_strategic_snapshot_for_tick,
};
use crate::*;
use interlinked_engine::model::{Link, Meta, Scenario, Service, Stop, World, Zone};
use interlinked_engine::platform::{ScenarioDocument, SimulationService};
use interlinked_engine::sim::types::StrategicPlannerTimingDiagnostics;
use interlinked_engine::{run_simulation_with_settings, SimulationSettings};
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

#[derive(Debug)]
struct MsaTopologyPerfReport {
    scenario_name: &'static str,
    stop_count: usize,
    service_count: usize,
    link_count: usize,
    requested_iterations: usize,
    assignment_iterations: usize,
    total_ms: f64,
    mode_choice_ms: f64,
    assignment_ms: f64,
    lightweight_outputs_ms: f64,
    full_route_searches: usize,
    structural_candidates: usize,
    candidate_evaluations: usize,
    potential_structure_reuse: usize,
    repeated_assignment_od: usize,
    topology_same: usize,
    topology_changed: usize,
    topology_unknown: usize,
    route_contexts: usize,
    route_cache_hits: usize,
    route_cache_misses: usize,
    route_search_total_ms: f64,
    route_graph_search_ms: f64,
    route_candidate_expansion_ms: f64,
    route_path_reconstruction_ms: f64,
    route_built_path_construction_ms: f64,
    route_path_dedupe_ms: f64,
    route_candidate_classification_ms: f64,
    route_cost_eval_ms: f64,
    route_cache_lookup_ms: f64,
    route_cache_insert_ms: f64,
    route_diagnostics_fingerprint_ms: f64,
    route_other_ms: f64,
    route_search_requests: usize,
    initial_dijkstra_calls: usize,
    expansion_dijkstra_calls: usize,
    expansion_attempts: usize,
    expansion_successes: usize,
    expansion_no_path: usize,
    expansion_duplicates: usize,
    expansion_heap_exhausted: usize,
    expansion_no_path_memo_hits: usize,
    expansion_no_path_memo_inserts: usize,
    expansion_skip_no_outgoing: usize,
    expansion_skip_spur_banned: usize,
    expansion_skip_target_banned: usize,
    early_exit_k_le_1: usize,
    dijkstra_relaxations: usize,
    graph_search_invocations: usize,
    built_paths: usize,
    candidate_paths: usize,
    rejected_candidates: usize,
    total_path_links_seen: usize,
    total_board_events_built: usize,
    total_alight_events_built: usize,
    max_candidates_per_od: usize,
    lifecycle_clean: bool,
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

fn synthetic_msa_topology_scenario() -> Scenario {
    let scale = RuntimePerfScenarioScale {
        name: "msa_multi_iter",
        stop_count: 36,
        line_count: 6,
        cohorts_per_service: 10,
        queue_pax_per_cohort: 32.0,
        warmup_iterations: 0,
        iterations: 1,
    };
    let mut scenario = synthetic_runtime_scenario(scale);
    scenario.params.assignment_max_iters = 4;
    scenario.params.assignment_convergence_rel = 0.0;
    scenario.params.route_choice_k = 3;
    scenario.params.trips_per_person = 2.5;
    scenario.params.queue_max_extra_wait_s = 1800.0;

    // Perf-only congestion setup: keep production defaults untouched, but make
    // graph costs sensitive enough for MSA iterations to have something to test.
    for link in &mut scenario.world.links {
        link.capacity_per_hour = Some(80.0);
    }
    for service in &mut scenario.world.services {
        service.vehicle_capacity = 42.0;
        service.stock_units_owned = Some(1);
        service.stock_units_assigned = Some(1);
        service.operating_tph = Some(4.0);
        service.headway_s = 900.0;
    }
    for zone in &mut scenario.world.zones {
        zone.population = 25_000.0;
        zone.jobs = 24_000.0;
    }

    scenario
}

fn run_msa_topology_perf_scenario() -> MsaTopologyPerfReport {
    let scenario = synthetic_msa_topology_scenario();
    let mut settings = SimulationSettings::from_params(&scenario.params);
    settings.msa_max_iters = 4;
    settings.convergence_rel = 0.0;
    settings.k_paths = 3;
    settings.lightweight_outputs = true;

    let output = run_simulation_with_settings(&scenario, &settings, None)
        .expect("MSA topology perf scenario should run");
    let timing = &output.strategic_planner_timing;
    let lifecycle = &output.lifecycle_conservation;

    MsaTopologyPerfReport {
        scenario_name: "msa_multi_iter",
        stop_count: scenario.world.stops.len(),
        service_count: scenario.world.services.len(),
        link_count: scenario.world.links.len(),
        requested_iterations: settings.msa_max_iters,
        assignment_iterations: output.diagnostics.msa_iterations,
        total_ms: timing.total_ms,
        mode_choice_ms: timing.mode_choice_ms,
        assignment_ms: timing.assignment_ms,
        lightweight_outputs_ms: timing.lightweight_outputs_ms,
        full_route_searches: timing.assignment_full_route_search_count,
        structural_candidates: timing.assignment_structural_candidate_count,
        candidate_evaluations: timing.assignment_candidate_evaluation_count,
        potential_structure_reuse: timing.assignment_potential_structure_reuse_count,
        repeated_assignment_od: timing.assignment_repeated_od_across_iterations,
        topology_same: timing.assignment_topology_same_count,
        topology_changed: timing.assignment_topology_changed_count,
        topology_unknown: timing.assignment_topology_unknown_count,
        route_contexts: timing.route_candidate_context_count,
        route_cache_hits: timing.route_candidate_cache_hits,
        route_cache_misses: timing.route_candidate_cache_misses,
        route_search_total_ms: timing.assignment_route_search_total_ms,
        route_graph_search_ms: timing.assignment_graph_search_ms,
        route_candidate_expansion_ms: timing.assignment_candidate_expansion_ms,
        route_path_reconstruction_ms: timing.assignment_path_reconstruction_ms,
        route_built_path_construction_ms: timing.assignment_built_path_construction_ms,
        route_path_dedupe_ms: timing.assignment_path_dedupe_ms,
        route_candidate_classification_ms: timing.assignment_candidate_classification_ms,
        route_cost_eval_ms: timing.assignment_cost_eval_ms,
        route_cache_lookup_ms: timing.assignment_route_cache_lookup_ms,
        route_cache_insert_ms: timing.assignment_cache_insert_ms,
        route_diagnostics_fingerprint_ms: timing.assignment_diagnostics_fingerprint_ms,
        route_other_ms: timing.assignment_other_route_search_ms,
        route_search_requests: timing.assignment_route_search_request_count,
        initial_dijkstra_calls: timing.assignment_initial_dijkstra_call_count,
        expansion_dijkstra_calls: timing.assignment_expansion_dijkstra_call_count,
        expansion_attempts: timing.assignment_expansion_attempt_count,
        expansion_successes: timing.assignment_expansion_success_count,
        expansion_no_path: timing.assignment_expansion_no_path_count,
        expansion_duplicates: timing.assignment_expansion_duplicate_count,
        expansion_heap_exhausted: timing.assignment_expansion_heap_exhausted_count,
        expansion_no_path_memo_hits: timing.assignment_expansion_no_path_memo_hit_count,
        expansion_no_path_memo_inserts: timing.assignment_expansion_no_path_memo_insert_count,
        expansion_skip_no_outgoing: timing.assignment_expansion_skip_no_outgoing_count,
        expansion_skip_spur_banned: timing.assignment_expansion_skip_spur_banned_count,
        expansion_skip_target_banned: timing.assignment_expansion_skip_target_banned_count,
        early_exit_k_le_1: timing.assignment_early_exit_k_le_1_count,
        dijkstra_relaxations: timing.assignment_dijkstra_relaxation_count,
        graph_search_invocations: timing.assignment_graph_search_invocation_count,
        built_paths: timing.assignment_built_path_count,
        candidate_paths: timing.assignment_route_candidate_path_count,
        rejected_candidates: timing.assignment_route_candidate_rejected_count,
        total_path_links_seen: timing.assignment_route_total_path_links_seen,
        total_board_events_built: timing.assignment_route_total_board_events_built,
        total_alight_events_built: timing.assignment_route_total_alight_events_built,
        max_candidates_per_od: timing.assignment_route_max_candidate_count_per_od,
        lifecycle_clean: lifecycle.queue_balance_error.abs() <= 1e-6
            && lifecycle.onboard_balance_error.abs() <= 1e-6,
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
            "[runtime-perf] scenario={} first_strategic_counts zones={} stops={} links={} services={} demand_cells={} graph_nodes={} graph_edges={} active_latent={} mode_choice_rows={} assigned_od={} board_loads={} cohorts={} mode_paths_raw={} mode_paths_boardable={} assignment_paths_raw={} assignment_paths_boardable={} mode_choice_cache={}/{} assignment_cache_iter={}/{} assignment_cache_kpi={}/{} route_contexts={} route_cache={}/{} assignment_iter_requests={} assignment_kpi_requests={} kpi_requery_avoided={} assignment_to_kpi_context_blocked={} assignment_full_route_searches={} structural_candidates={} candidate_evaluations={} potential_structure_reuse={} repeated_assignment_od={} topology_same={} topology_changed={} topology_unknown={} stage_unique_od_queries={} cross_stage_duplicate_od_estimate={} assignment_to_kpi_requery_estimate={} zero_boardable_unique_od={} zero_boardable_logs_suppressed={} attempted_pax={:.3}",
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
            timing.mode_choice_path_cache_hits,
            timing.mode_choice_path_cache_misses,
            timing.assignment_iter_path_cache_hits,
            timing.assignment_iter_path_cache_misses,
            timing.assignment_kpi_path_cache_hits,
            timing.assignment_kpi_path_cache_misses,
            timing.route_candidate_context_count,
            timing.route_candidate_cache_hits,
            timing.route_candidate_cache_misses,
            timing.assignment_iter_path_requests,
            timing.assignment_kpi_path_requests,
            timing.kpi_requery_avoided_count,
            timing.assignment_to_kpi_incompatible_context_count,
            timing.assignment_full_route_search_count,
            timing.assignment_structural_candidate_count,
            timing.assignment_candidate_evaluation_count,
            timing.assignment_potential_structure_reuse_count,
            timing.assignment_repeated_od_across_iterations,
            timing.assignment_topology_same_count,
            timing.assignment_topology_changed_count,
            timing.assignment_topology_unknown_count,
            timing.candidate_stage_unique_od_query_count,
            timing.candidate_cross_stage_duplicate_od_estimate,
            timing.assignment_to_kpi_requery_estimate,
            timing.mode_choice_zero_boardable_unique_od_count,
            timing.mode_choice_zero_boardable_log_suppressed_count,
            timing.assignment_attempted_pax_total,
        );
        eprintln!(
            "[runtime-perf] scenario={} first_strategic_route_search total_ms={:.3} graph_search_ms={:.3} candidate_expansion_ms={:.3} path_reconstruct_ms={:.3} built_path_ms={:.3} dedupe_ms={:.3} classify_ms={:.3} cost_eval_ms={:.3} cache_lookup_ms={:.3} cache_insert_ms={:.3} fingerprint_ms={:.3} other_ms={:.3}",
            report.scenario_name,
            timing.assignment_route_search_total_ms,
            timing.assignment_graph_search_ms,
            timing.assignment_candidate_expansion_ms,
            timing.assignment_path_reconstruction_ms,
            timing.assignment_built_path_construction_ms,
            timing.assignment_path_dedupe_ms,
            timing.assignment_candidate_classification_ms,
            timing.assignment_cost_eval_ms,
            timing.assignment_route_cache_lookup_ms,
            timing.assignment_cache_insert_ms,
            timing.assignment_diagnostics_fingerprint_ms,
            timing.assignment_other_route_search_ms,
        );
        eprintln!(
            "[runtime-perf] scenario={} first_strategic_route_counts requests={} initial_dijkstra={} expansion_dijkstra={} expansion_attempts={} expansion_successes={} expansion_no_path={} expansion_duplicates={} expansion_heap_exhausted={} memo_hits={} memo_inserts={} skip_no_outgoing={} skip_spur_banned={} skip_target_banned={} early_exit_k_le_1={} dijkstra_relaxations={} graph_searches={} built_paths={} candidate_paths={} rejected_candidates={} links_seen={} board_events={} alight_events={} max_candidates_per_od={}",
            report.scenario_name,
            timing.assignment_route_search_request_count,
            timing.assignment_initial_dijkstra_call_count,
            timing.assignment_expansion_dijkstra_call_count,
            timing.assignment_expansion_attempt_count,
            timing.assignment_expansion_success_count,
            timing.assignment_expansion_no_path_count,
            timing.assignment_expansion_duplicate_count,
            timing.assignment_expansion_heap_exhausted_count,
            timing.assignment_expansion_no_path_memo_hit_count,
            timing.assignment_expansion_no_path_memo_insert_count,
            timing.assignment_expansion_skip_no_outgoing_count,
            timing.assignment_expansion_skip_spur_banned_count,
            timing.assignment_expansion_skip_target_banned_count,
            timing.assignment_early_exit_k_le_1_count,
            timing.assignment_dijkstra_relaxation_count,
            timing.assignment_graph_search_invocation_count,
            timing.assignment_built_path_count,
            timing.assignment_route_candidate_path_count,
            timing.assignment_route_candidate_rejected_count,
            timing.assignment_route_total_path_links_seen,
            timing.assignment_route_total_board_events_built,
            timing.assignment_route_total_alight_events_built,
            timing.assignment_route_max_candidate_count_per_od,
        );
    }
}

fn print_msa_topology_report(report: &MsaTopologyPerfReport) {
    eprintln!(
        "[runtime-perf] scenario={} direct_planner=true stops={} services={} links={} requested_iters={} assignment_iters={} lifecycle_clean={} total_ms={:.3} mode_choice_ms={:.3} assignment_ms={:.3} lightweight_outputs_ms={:.3}",
        report.scenario_name,
        report.stop_count,
        report.service_count,
        report.link_count,
        report.requested_iterations,
        report.assignment_iterations,
        report.lifecycle_clean,
        report.total_ms,
        report.mode_choice_ms,
        report.assignment_ms,
        report.lightweight_outputs_ms,
    );
    eprintln!(
        "[runtime-perf] scenario={} msa_topology full_route_searches={} structural_candidates={} candidate_evaluations={} potential_structure_reuse={} repeated_assignment_od={} topology_same={} topology_changed={} topology_unknown={} route_contexts={} route_cache={}/{}",
        report.scenario_name,
        report.full_route_searches,
        report.structural_candidates,
        report.candidate_evaluations,
        report.potential_structure_reuse,
        report.repeated_assignment_od,
        report.topology_same,
        report.topology_changed,
        report.topology_unknown,
        report.route_contexts,
        report.route_cache_hits,
        report.route_cache_misses,
    );
    eprintln!(
        "[runtime-perf] scenario={} msa_route_search total_ms={:.3} graph_search_ms={:.3} candidate_expansion_ms={:.3} path_reconstruct_ms={:.3} built_path_ms={:.3} dedupe_ms={:.3} classify_ms={:.3} cost_eval_ms={:.3} cache_lookup_ms={:.3} cache_insert_ms={:.3} fingerprint_ms={:.3} other_ms={:.3}",
        report.scenario_name,
        report.route_search_total_ms,
        report.route_graph_search_ms,
        report.route_candidate_expansion_ms,
        report.route_path_reconstruction_ms,
        report.route_built_path_construction_ms,
        report.route_path_dedupe_ms,
        report.route_candidate_classification_ms,
        report.route_cost_eval_ms,
        report.route_cache_lookup_ms,
        report.route_cache_insert_ms,
        report.route_diagnostics_fingerprint_ms,
        report.route_other_ms,
    );
    eprintln!(
        "[runtime-perf] scenario={} msa_route_counts requests={} initial_dijkstra={} expansion_dijkstra={} expansion_attempts={} expansion_successes={} expansion_no_path={} expansion_duplicates={} expansion_heap_exhausted={} memo_hits={} memo_inserts={} skip_no_outgoing={} skip_spur_banned={} skip_target_banned={} early_exit_k_le_1={} dijkstra_relaxations={} graph_searches={} built_paths={} candidate_paths={} rejected_candidates={} links_seen={} board_events={} alight_events={} max_candidates_per_od={}",
        report.scenario_name,
        report.route_search_requests,
        report.initial_dijkstra_calls,
        report.expansion_dijkstra_calls,
        report.expansion_attempts,
        report.expansion_successes,
        report.expansion_no_path,
        report.expansion_duplicates,
        report.expansion_heap_exhausted,
        report.expansion_no_path_memo_hits,
        report.expansion_no_path_memo_inserts,
        report.expansion_skip_no_outgoing,
        report.expansion_skip_spur_banned,
        report.expansion_skip_target_banned,
        report.early_exit_k_le_1,
        report.dijkstra_relaxations,
        report.graph_search_invocations,
        report.built_paths,
        report.candidate_paths,
        report.rejected_candidates,
        report.total_path_links_seen,
        report.total_board_events_built,
        report.total_alight_events_built,
        report.max_candidates_per_od,
    );
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
    assert!(first_timing.mode_choice_path_cache_misses > 0);
    assert!(
        first_timing.assignment_iter_path_requests
            >= first_timing.assignment_iter_path_cache_misses
    );
    assert!(
        first_timing.assignment_kpi_path_requests >= first_timing.assignment_kpi_path_cache_misses
    );
    assert!(first_timing.route_candidate_context_count >= 2);
    assert_eq!(
        first_timing.kpi_requery_avoided_count,
        first_timing.assignment_kpi_path_cache_hits
    );
    assert!(
        first_timing.assignment_full_route_search_count
            >= first_timing.assignment_iter_path_cache_misses
    );
    assert!(
        first_timing.assignment_candidate_evaluation_count
            >= first_timing.assignment_potential_structure_reuse_count
    );
    assert!(first_timing.assignment_route_search_total_ms >= 0.0);
    assert!(
        first_timing.assignment_graph_search_invocation_count
            >= first_timing.assignment_iter_path_cache_misses
    );
    assert_eq!(
        first_timing.assignment_graph_search_invocation_count,
        first_timing
            .assignment_initial_dijkstra_call_count
            .saturating_add(first_timing.assignment_expansion_dijkstra_call_count)
    );
    assert!(first_timing.assignment_built_path_count > 0);
    assert!(
        first_timing.assignment_route_search_request_count
            >= first_timing.assignment_iter_path_requests
    );
    assert_ne!(report.fare_source, CounterProvenance::RuntimeProjection);

    let msa_report = run_msa_topology_perf_scenario();
    assert_eq!(msa_report.requested_iterations, 4);
    assert!(
        msa_report.assignment_iterations > 1,
        "MSA topology perf scenario should exercise repeated assignment iterations"
    );
    assert!(
        msa_report.candidate_evaluations >= msa_report.potential_structure_reuse,
        "potential structural reuse must be a subset of evaluated candidates"
    );
    assert!(msa_report.route_search_total_ms >= 0.0);
    assert!(
        msa_report.graph_search_invocations >= msa_report.full_route_searches,
        "each route-search miss should invoke at least one graph search"
    );
    assert_eq!(
        msa_report.graph_search_invocations,
        msa_report
            .initial_dijkstra_calls
            .saturating_add(msa_report.expansion_dijkstra_calls)
    );
    assert!(
        msa_report.expansion_attempts
            >= msa_report
                .expansion_successes
                .saturating_add(msa_report.expansion_duplicates)
    );
    assert!(msa_report.built_paths > 0);
    assert!(msa_report.lifecycle_clean);
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

    let msa_report = run_msa_topology_perf_scenario();
    assert!(
        msa_report.assignment_iterations > 1,
        "manual MSA topology scenario must exercise repeated assignment iterations"
    );
    print_msa_topology_report(&msa_report);
}
