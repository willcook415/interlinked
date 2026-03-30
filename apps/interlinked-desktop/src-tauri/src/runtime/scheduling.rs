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
