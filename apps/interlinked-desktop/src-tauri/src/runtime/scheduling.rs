#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeCatchupPlan {
    pub(crate) steps_to_run: usize,
    pub(crate) backlog_steps: usize,
}

pub(crate) fn plan_runtime_catchup(
    accumulator_s: f64,
    fixed_step_s: f64,
    max_steps_per_cycle: usize,
) -> RuntimeCatchupPlan {
    if fixed_step_s <= 0.0 || !fixed_step_s.is_finite() {
        return RuntimeCatchupPlan::default();
    }
    let available_steps = (accumulator_s / fixed_step_s).floor().max(0.0) as usize;
    let capped_max_steps = max_steps_per_cycle.max(1);
    let steps_to_run = available_steps.min(capped_max_steps);
    RuntimeCatchupPlan {
        steps_to_run,
        backlog_steps: available_steps.saturating_sub(steps_to_run),
    }
}

pub(crate) fn effective_max_steps_per_cycle(
    configured_max_steps_per_cycle: u32,
    speed: u32,
) -> usize {
    let configured = configured_max_steps_per_cycle.clamp(1, 128) as usize;
    // Preserve backwards compatibility with existing manifests that pin max_steps_per_cycle to 1,
    // while ensuring higher runtime speed multipliers can be honored.
    // Also allow one additional bounded catch-up step so periodic strategic refresh spikes can
    // recover without prolonged backlog while still avoiding large burst execution.
    let speed_floor = speed.clamp(1, 8) as usize;
    let bounded_catchup_floor = speed_floor.saturating_add(1).min(8);
    configured.max(bounded_catchup_floor).min(128)
}

pub(crate) fn effective_strategic_refresh_interval_ticks(
    configured_interval_ticks: u32,
    speed: u32,
) -> u32 {
    let base = configured_interval_ticks.max(1);
    let speed_scale = speed.clamp(1, 8);
    base.saturating_mul(speed_scale).max(1)
}
