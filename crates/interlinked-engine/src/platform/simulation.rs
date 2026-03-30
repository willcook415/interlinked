use super::edits::apply_network_edits;
use super::kernels::{
    build_strategic_kernel_cache, run_fast_operational_step, scenario_topology_signature,
    should_run_strategic_refresh, update_fast_step_metrics, update_strategic_step_metrics,
    KernelPartitionState, StrategicRefreshReason,
};
use super::{GameState, GameStepOutput, GameStepRequest, ScenarioDocument, ScenarioStore};
use crate::sim::{init_sim_state, step_simulation, RunConfig};
use crate::sim::{
    run_simulation_with_settings_and_context_with_policy, DemandTimeSliceLabel, SeasonalProfile,
    ServiceDayType, SimulationOutput, SimulationSettings, TemporalBundlePolicy,
    TemporalDemandSlice,
};
use crate::sim::{HistoryConfig, SimHistory};
use std::hash::{Hash, Hasher};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PlanningRunOptions {
    pub settings_override: Option<SimulationSettings>,
    pub deterministic_mode: bool,
    pub deterministic_seed: Option<u64>,
    pub time_of_day_s: Option<f64>,
    pub service_day_type: Option<ServiceDayType>,
    pub seasonal_profile: Option<SeasonalProfile>,
    pub active_event_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SimulationScope {
    pub active_region_ids: Vec<String>,
    pub remote_regions_mode: String,
    pub max_active_zones: usize,
}

impl Default for SimulationScope {
    fn default() -> Self {
        Self {
            active_region_ids: vec![],
            remote_regions_mode: "aggregate".to_string(),
            max_active_zones: 600,
        }
    }
}

impl Default for PlanningRunOptions {
    fn default() -> Self {
        Self {
            settings_override: None,
            deterministic_mode: true,
            deterministic_seed: None,
            time_of_day_s: None,
            service_day_type: None,
            seasonal_profile: None,
            active_event_ids: None,
        }
    }
}

pub struct SimulationService;

impl SimulationService {
    /// Planning mode: deterministic batch run that yields KPIs/link loads/etc.
    pub fn run_planning(
        doc: &ScenarioDocument,
        opts: PlanningRunOptions,
    ) -> Result<SimulationOutput, String> {
        if !opts.deterministic_mode {
            return Err("deterministic_mode must be true (hard requirement)".to_string());
        }

        let mut s = doc.scenario.clone();
        let seed = opts.deterministic_seed.unwrap_or(s.meta.seed);
        s.meta.seed = seed;

        if let Some(clock_s) = opts.time_of_day_s {
            let mult = s.params.demand_multiplier_at(clock_s);
            s.params.trips_per_person *= mult;
            apply_purpose_multipliers_at(&mut s.params, clock_s);
        }

        let settings = match opts.settings_override {
            Some(x) => x,
            None => SimulationSettings::from_params(&s.params),
        };
        let temporal_context = Some(TemporalDemandSlice {
            service_day_type: opts.service_day_type.unwrap_or(ServiceDayType::Weekday),
            time_slice: opts
                .time_of_day_s
                .map(planning_time_slice_for_clock)
                .unwrap_or_default(),
            seasonal_profile: opts.seasonal_profile.unwrap_or(SeasonalProfile::Neutral),
            active_event_ids: opts.active_event_ids.unwrap_or_default(),
        });
        run_simulation_with_settings_and_context_with_policy(
            &s,
            &settings,
            None,
            temporal_context,
            TemporalBundlePolicy::AlwaysInclude,
        )
    }

    /// --------------------------
    /// Game-mode scaffolding
    /// --------------------------
    ///
    /// This is *not* a full passenger microsim yet — this is the clean API hook
    /// you’ll build into later (train movement, station crowding, etc).
    ///
    /// The point: UI/game loop can call step_game() repeatedly without touching sim.rs.
    pub fn init_game_state(doc: &ScenarioDocument) -> GameState {
        let run_cfg = RunConfig {
            step_kernel: Some(crate::sim::StepKernelConfig::fast()),
            deterministic_mode: true,
            deterministic_seed: Some(doc.scenario.meta.seed),
            ..Default::default()
        };
        let sim_state = init_sim_state(&doc.scenario, &run_cfg);
        let history = SimHistory::new(HistoryConfig::default());
        GameState {
            tick_s: 0.0,
            // Store the “authoritative” scenario/network here
            store: ScenarioStore::new(doc.scenario.clone()),
            // Quick metrics cache (optional)
            last_quick_kpis: None,
            last_output: None,
            run_cfg,
            sim_state,
            history,
            kernel_state: KernelPartitionState::default(),
        }
    }

    /// Tick the game simulation forward by dt seconds.
    /// Delegates to scoped stepping with the default scope.
    pub fn step_game(
        state: &mut GameState,
        dt_s: f64,
        req: GameStepRequest,
    ) -> Result<GameStepOutput, String> {
        Self::step_game_scoped(state, dt_s, req, &SimulationScope::default())
    }

    /// Scoped game step entrypoint.
    /// Current implementation relies on platform-layer pre-materialization and
    /// delegates to `step_game`, keeping engine behavior deterministic.
    pub fn step_game_scoped(
        state: &mut GameState,
        dt_s: f64,
        req: GameStepRequest,
        scope: &SimulationScope,
    ) -> Result<GameStepOutput, String> {
        if dt_s <= 0.0 {
            return Err("dt_s must be > 0".to_string());
        }

        if !req.edits.is_empty() {
            apply_network_edits(&mut state.store, &req.edits)?;
            state
                .kernel_state
                .mark_invalidated(StrategicRefreshReason::InvalidatedByEdit);
        }
        if req.force_strategic_refresh {
            state.kernel_state.explicit_refresh_requested = true;
        }

        let topology_signature = scenario_topology_signature(state.store.scenario());
        let scope_signature = simulation_scope_signature(scope);
        let strategic_interval_steps = state.run_cfg.strategic_refresh_interval_steps.max(1);
        state.kernel_state.perf.strategic_refresh_interval_steps = strategic_interval_steps;

        let refresh_reason = if state.run_cfg.enable_kernel_partitioning {
            should_run_strategic_refresh(
                &state.kernel_state,
                strategic_interval_steps,
                topology_signature,
                scope_signature,
            )
        } else {
            Some(StrategicRefreshReason::ExplicitForce)
        };

        let (out, next_sim_state, strategic_refresh_executed, strategic_refresh_reason) =
            if let Some(reason) = refresh_reason {
                let started = Instant::now();
                let (strategic_out, next) = step_simulation(
                    state.store.scenario(),
                    &state.run_cfg,
                    &state.sim_state,
                    dt_s,
                )?;
                let cache = build_strategic_kernel_cache(
                    &strategic_out,
                    topology_signature,
                    scope_signature,
                    next.t_s,
                );
                state.kernel_state.strategic_cache = Some(cache);
                state.kernel_state.invalidated = false;
                state.kernel_state.explicit_refresh_requested = false;
                state.kernel_state.last_scope_signature = scope_signature;
                state.kernel_state.last_topology_signature = topology_signature;
                update_strategic_step_metrics(
                    &mut state.kernel_state,
                    started,
                    reason,
                    strategic_interval_steps,
                );
                (strategic_out, next, true, Some(reason))
            } else {
                let started = Instant::now();
                let cache = state.kernel_state.strategic_cache.as_ref().ok_or_else(|| {
                    "missing strategic cache for fast operational step".to_string()
                })?;
                let (fast_out, next) = run_fast_operational_step(cache, &state.sim_state, dt_s)?;
                state.kernel_state.last_scope_signature = scope_signature;
                state.kernel_state.last_topology_signature = topology_signature;
                update_fast_step_metrics(&mut state.kernel_state, started);
                (fast_out, next, false, None)
            };

        state.sim_state = next_sim_state;
        state.tick_s = state.sim_state.t_s;
        state.last_quick_kpis = Some(out.kpis.clone());
        state.last_output = Some(out.clone());
        state.history.push(&out, &state.sim_state);

        Ok(GameStepOutput {
            tick_s: state.tick_s,
            quick_kpis: if req.recompute_quick_kpis {
                state.last_quick_kpis.clone()
            } else {
                None
            },
            strategic_refresh_executed,
            strategic_refresh_reason,
            kernel_perf: state.kernel_state.perf.clone(),
        })
    }
}

fn simulation_scope_signature(scope: &SimulationScope) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scope.active_region_ids.len().hash(&mut hasher);
    for region_id in &scope.active_region_ids {
        region_id.hash(&mut hasher);
    }
    scope.remote_regions_mode.hash(&mut hasher);
    scope.max_active_zones.hash(&mut hasher);
    hasher.finish()
}

fn apply_purpose_multipliers_at(params: &mut crate::model::Params, t_s: f64) {
    let (m_work, m_education, m_retail, m_recreation, m_other) = params.purpose_multipliers_at(t_s);
    let orig = [
        params.purpose_share_home_work.max(0.0),
        params.purpose_share_home_education.max(0.0),
        params.purpose_share_home_retail.max(0.0),
        params.purpose_share_home_recreation.max(0.0),
        params.purpose_share_other.max(0.0),
    ];
    let scaled = [
        orig[0] * m_work.max(0.0),
        orig[1] * m_education.max(0.0),
        orig[2] * m_retail.max(0.0),
        orig[3] * m_recreation.max(0.0),
        orig[4] * m_other.max(0.0),
    ];
    let scaled_sum: f64 = scaled.iter().sum();
    let final_values = if scaled_sum > 0.0 && scaled_sum.is_finite() {
        scaled
    } else {
        orig
    };

    params.purpose_share_home_work = final_values[0];
    params.purpose_share_home_education = final_values[1];
    params.purpose_share_home_retail = final_values[2];
    params.purpose_share_home_recreation = final_values[3];
    params.purpose_share_other = final_values[4];
}

fn planning_time_slice_for_clock(clock_s: f64) -> DemandTimeSliceLabel {
    let day_s = 86_400.0;
    let mut t = clock_s % day_s;
    if t < 0.0 {
        t += day_s;
    }
    if (4.0 * 3600.0..6.0 * 3600.0).contains(&t) {
        return DemandTimeSliceLabel::EarlyMorning;
    }
    if (6.0 * 3600.0..10.0 * 3600.0).contains(&t) {
        return DemandTimeSliceLabel::AmPeak;
    }
    if (10.0 * 3600.0..16.0 * 3600.0).contains(&t) {
        return DemandTimeSliceLabel::Interpeak;
    }
    if (16.0 * 3600.0..19.0 * 3600.0).contains(&t) {
        return DemandTimeSliceLabel::PmPeak;
    }
    if (19.0 * 3600.0..23.0 * 3600.0).contains(&t) {
        return DemandTimeSliceLabel::Evening;
    }
    DemandTimeSliceLabel::LateNight
}

pub fn history_last_frame(state: &GameState) -> Option<crate::sim::HistoryFrame> {
    state.history.last().cloned()
}

pub fn history_export(state: &GameState) -> crate::sim::SimHistory {
    state.history.clone()
}

pub fn history_clear(state: &mut GameState) {
    state.history.clear();
}

pub fn kernel_perf_metrics(state: &GameState) -> super::kernels::KernelPerfMetrics {
    state.kernel_state.perf.clone()
}
