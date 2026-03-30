use crate::sim::{compare_outputs, SimulationDelta, SimulationOutput};

pub struct ComparisonService;

impl ComparisonService {
    pub fn diff(base: &SimulationOutput, compare: &SimulationOutput) -> SimulationDelta {
        compare_outputs(base, compare)
    }
}
