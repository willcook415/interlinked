use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::planner::{run_simulation_with_settings_and_context_with_policy, TemporalBundlePolicy};
use super::types::{
    DemandTimeSliceLabel, SeasonalProfile, ServiceDayType, SimulationOutput, SimulationSettings,
    TemporalDemandSlice,
};
use crate::model::Scenario;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    pub horizon_s: f64,
    pub time_bin_s: f64,
    pub demand: DemandConfig,
    pub emit_time_series: bool,
    pub deterministic_mode: bool,
    pub deterministic_seed: Option<u64>,
    pub clock_start_s: f64,
    #[serde(default)]
    pub service_day_type: Option<ServiceDayType>,
    #[serde(default)]
    pub seasonal_profile: Option<SeasonalProfile>,
    #[serde(default)]
    pub active_event_ids: Option<Vec<String>>,

    /// Optional overrides to make step_simulation() cheap enough for UI/game loops.
    /// Planning runs should leave this as None.
    #[serde(default)]
    pub step_kernel: Option<StepKernelConfig>,
    #[serde(default)]
    pub lightweight_outputs: bool,
    /// Enables fast operational kernel stepping in platform game mode.
    #[serde(default = "default_enable_kernel_partitioning")]
    pub enable_kernel_partitioning: bool,
    /// Number of fast steps between forced strategic refreshes.
    #[serde(default = "default_strategic_refresh_interval_steps")]
    pub strategic_refresh_interval_steps: u32,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            horizon_s: 3600.0,
            time_bin_s: 300.0,
            demand: DemandConfig::default(),
            emit_time_series: true,
            deterministic_mode: true,
            deterministic_seed: None,
            clock_start_s: 8.0 * 3600.0,
            service_day_type: None,
            seasonal_profile: None,
            active_event_ids: None,
            step_kernel: None,
            lightweight_outputs: false,
            enable_kernel_partitioning: default_enable_kernel_partitioning(),
            strategic_refresh_interval_steps: default_strategic_refresh_interval_steps(),
        }
    }
}

fn default_enable_kernel_partitioning() -> bool {
    true
}

fn default_strategic_refresh_interval_steps() -> u32 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimState {
    pub t_s: f64,
    pub queue: HashMap<(String, String), f64>,
    #[serde(default)]
    pub queue_cohorts: HashMap<(String, String, String), f64>,
    pub time_to_next_departure_s: HashMap<(String, String), f64>,

    /// Additional OD demand (trips) to inject for the *next* simulation period.
    ///
    /// Keys are (origin_zone_id, dest_zone_id) and values are trips in this period.
    /// This is used by the stateful stepping layer to release demand events over time.
    ///
    /// IMPORTANT: This map is cleared after each step.
    #[serde(default)]
    pub pending_od_trips: HashMap<(String, String), f64>,
}

impl SimState {
    pub fn new() -> Self {
        Self {
            t_s: 0.0,
            queue: HashMap::new(),
            queue_cohorts: HashMap::new(),
            time_to_next_departure_s: HashMap::new(),
            pending_od_trips: HashMap::new(),
        }
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DemandEvent {
    // Inject additional trips at a given simulation time.
    // For now this is a scaffolding type; we will wire it into demand release next.
    ZoneToZone {
        t_s: f64,
        origin_zone: String,
        dest_zone: String,
        trips: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandConfig {
    // Baseline synthetic demand (gravity model) is always available.
    // If true, baseline demand is used. If false, only events drive demand.
    pub use_gravity_baseline: bool,

    // Optional external demand events (good for game: festivals, stadium events, disruptions)
    pub events: Vec<DemandEvent>,
}

impl Default for DemandConfig {
    fn default() -> Self {
        Self {
            use_gravity_baseline: true,
            events: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepKernelConfig {
    pub k_paths: usize,
    pub msa_max_iters: usize,
    pub convergence_rel: f64,
    pub route_choice_theta: f64,
}

impl StepKernelConfig {
    pub fn fast() -> Self {
        Self {
            k_paths: 1,
            msa_max_iters: 1,
            convergence_rel: 1.0,      // irrelevant with 1 iter, but keep sane
            route_choice_theta: 0.002, // keep your current default feel
        }
    }
}

pub fn init_sim_state(s: &Scenario, cfg: &RunConfig) -> SimState {
    let mut st = SimState::new();

    // Start clock from config.
    st.t_s = cfg.clock_start_s.max(0.0);

    // Initialize departure phase for each service’s first boarding stop.
    // (Future: expand to more stops when you model alight/board at intermediate stops.)
    for svc in &s.world.services {
        if let Some(first_stop) = svc.stop_sequence.first() {
            st.time_to_next_departure_s
                .insert((svc.id.clone(), first_stop.clone()), svc.headway_s.max(0.0));
        }
    }

    // Align time bin with config
    // (We don't store bins here yet; state advances via stepping later.)
    let _ = cfg;

    st
}

pub fn run_planning_stateful(
    s: &Scenario,
    cfg: &RunConfig,
    state_in: Option<&SimState>,
) -> Result<(SimulationOutput, SimState), String> {
    if !cfg.deterministic_mode {
        return Err("deterministic_mode must be true (hard requirement)".to_string());
    }

    // Use existing settings, but override time_bin_s from cfg
    let mut settings = SimulationSettings::from_params(&s.params);
    settings.time_bin_s = cfg.time_bin_s.max(1.0);
    settings.lightweight_outputs = cfg.lightweight_outputs;

    let mut s_run = s.clone();
    let seed = cfg.deterministic_seed.unwrap_or(s.meta.seed);
    s_run.meta.seed = seed;

    // Run the existing batch model (this remains your trusted kernel)
    let planning_clock_s = state_in
        .map(|st| st.t_s)
        .unwrap_or(cfg.clock_start_s.max(0.0));
    let temporal_context = Some(TemporalDemandSlice {
        service_day_type: cfg.service_day_type.unwrap_or(ServiceDayType::Weekday),
        time_slice: stateful_time_slice_for_clock(planning_clock_s),
        seasonal_profile: cfg.seasonal_profile.unwrap_or(SeasonalProfile::Neutral),
        active_event_ids: cfg.active_event_ids.clone().unwrap_or_default(),
    });
    let out = run_simulation_with_settings_and_context_with_policy(
        &s_run,
        &settings,
        state_in,
        temporal_context,
        TemporalBundlePolicy::AlwaysInclude,
    )?;

    // Build next state from output (queues persist)
    let mut st = state_in.cloned().unwrap_or_else(SimState::new);

    // Advance clock by horizon (planner runs typically simulate full horizon at once)
    st.t_s += cfg.horizon_s.max(0.0);

    // Persist end-of-horizon queues from board_loads
    st.queue.clear();
    for bl in &out.board_loads {
        st.queue.insert(
            (bl.service_id.clone(), bl.stop_id.clone()),
            bl.queue_end.max(0.0),
        );
    }
    st.queue_cohorts.clear();
    for cohort in &out.passenger_cohorts {
        let queue_end = cohort.queue_end_pax.max(0.0);
        if queue_end <= 0.0 {
            continue;
        }
        st.queue_cohorts.insert(
            (
                cohort.service_id.clone(),
                cohort.board_stop_id.clone(),
                cohort.destination_stop_id.clone(),
            ),
            queue_end,
        );
    }

    st.time_to_next_departure_s.clear();
    for bl in &out.board_loads {
        st.time_to_next_departure_s.insert(
            (bl.service_id.clone(), bl.stop_id.clone()),
            bl.time_to_next_departure_s_end.max(0.0),
        );
    }

    // Future-proof: keep time_to_next_departure_s as-is for now.
    // Next step we will actually advance phase using the discrete departure model.
    // (So trains don’t "reset" each run.)

    Ok((out, st))
}

/// Advance the simulation by one time step `dt_s` using the existing batch kernel.
///
/// What this does (exactly):
/// - Releases baseline gravity demand scaled to `dt_s`.
/// - Injects any DemandEvent::ZoneToZone events whose `t_s` lie in [state.t_s, state.t_s + dt_s).
/// - Runs the trusted batch model for a *period length of dt_s* (capacity/queues/headways are evaluated over dt).
/// - Persists end-of-step queues + departure phases back into SimState.
/// - Advances `state.t_s` by dt_s.
pub fn step_simulation(
    s: &Scenario,
    cfg: &RunConfig,
    state_in: &SimState,
    dt_s: f64,
) -> Result<(SimulationOutput, SimState), String> {
    if !cfg.deterministic_mode {
        return Err("deterministic_mode must be true (hard requirement)".to_string());
    }

    if dt_s <= 0.0 {
        return Err("dt_s must be > 0".to_string());
    }

    // ---- 1) Build a scenario copy whose period equals this step duration ----
    // The batch kernel interprets demand (trips_per_person) and capacity in terms of the scenario time period.
    // To run the kernel for dt_s, we set time_period_hours accordingly AND scale trips_per_person so that
    // demand per second stays consistent.
    let base_period_s = (s.meta.time_period_hours.max(0.0)) * 3600.0;
    if base_period_s <= 0.0 {
        return Err("scenario.meta.time_period_hours must be > 0".to_string());
    }

    let mut s_step = s.clone();
    let seed = cfg.deterministic_seed.unwrap_or(s.meta.seed);
    s_step.meta.seed = seed;
    s_step.meta.time_period_hours = dt_s / 3600.0;
    let base = s.params.trips_per_person * (dt_s / base_period_s);

    // ---- 2) Build an outgoing state that includes OD events released this step ----
    let mut st = state_in.clone();
    st.pending_od_trips.clear();

    let t0 = state_in.t_s;
    let t1 = t0 + dt_s;
    let t_mid = t0 + dt_s * 0.5;
    let factor = s.params.demand_multiplier_at(t_mid);
    s_step.params.trips_per_person = base * factor;
    apply_purpose_multipliers_at(&mut s_step.params, t_mid);
    for ev in &cfg.demand.events {
        match ev {
            DemandEvent::ZoneToZone {
                t_s,
                origin_zone,
                dest_zone,
                trips,
            } => {
                if *t_s >= t0 && *t_s < t1 && *trips > 0.0 {
                    *st.pending_od_trips
                        .entry((origin_zone.clone(), dest_zone.clone()))
                        .or_insert(0.0) += *trips;
                }
            }
        }
    }

    // ---- 3) Run the trusted batch kernel for this step ----
    let mut settings = SimulationSettings::from_params(&s_step.params);

    // Optional: fast stepping overrides (UI/game loop)
    if let Some(sk) = &cfg.step_kernel {
        settings.k_paths = sk.k_paths.max(1);
        settings.msa_max_iters = sk.msa_max_iters.max(1);
        settings.convergence_rel = sk.convergence_rel.max(0.0);
        settings.route_choice_theta = sk.route_choice_theta;
    }

    // Ensure bin size is sane for this step
    settings.time_bin_s = cfg.time_bin_s.max(1.0).min(dt_s.max(1.0));
    settings.lightweight_outputs = cfg.lightweight_outputs;

    let temporal_context = Some(TemporalDemandSlice {
        service_day_type: cfg.service_day_type.unwrap_or(ServiceDayType::Weekday),
        time_slice: stateful_time_slice_for_clock(t_mid),
        seasonal_profile: cfg.seasonal_profile.unwrap_or(SeasonalProfile::Neutral),
        active_event_ids: cfg.active_event_ids.clone().unwrap_or_default(),
    });
    let out = run_simulation_with_settings_and_context_with_policy(
        &s_step,
        &settings,
        Some(&st),
        temporal_context,
        TemporalBundlePolicy::NeverInclude,
    )?;

    // ---- 4) Persist next state ----
    let mut next = state_in.clone();
    next.t_s += dt_s;

    next.queue.clear();
    for bl in &out.board_loads {
        next.queue.insert(
            (bl.service_id.clone(), bl.stop_id.clone()),
            bl.queue_end.max(0.0),
        );
    }
    next.queue_cohorts.clear();
    for cohort in &out.passenger_cohorts {
        let queue_end = cohort.queue_end_pax.max(0.0);
        if queue_end <= 0.0 {
            continue;
        }
        next.queue_cohorts.insert(
            (
                cohort.service_id.clone(),
                cohort.board_stop_id.clone(),
                cohort.destination_stop_id.clone(),
            ),
            queue_end,
        );
    }

    next.time_to_next_departure_s.clear();
    for bl in &out.board_loads {
        next.time_to_next_departure_s.insert(
            (bl.service_id.clone(), bl.stop_id.clone()),
            bl.time_to_next_departure_s_end.max(0.0),
        );
    }

    next.pending_od_trips.clear();
    Ok((out, next))
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

fn stateful_time_slice_for_clock(clock_s: f64) -> DemandTimeSliceLabel {
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
