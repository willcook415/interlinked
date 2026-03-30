use crate::model::{Link, Scenario, Service, Stop, Transfer, TransferRule, Zone};

pub trait WorldStore {
    fn zones(&self) -> &[Zone];
}

pub trait NetworkStore {
    fn stops(&self) -> &[Stop];
    fn links(&self) -> &[Link];
    fn services(&self) -> &[Service];
    fn transfers(&self) -> &[Transfer];
    fn transfer_rules(&self) -> Option<&[TransferRule]>;
}

/// A convenient in-memory store that wraps a Scenario.
/// UI + tools can hold this and query through traits.
#[derive(Debug, Clone)]
pub struct ScenarioStore {
    scenario: Scenario,
}

impl ScenarioStore {
    pub fn new(scenario: Scenario) -> Self {
        Self { scenario }
    }

    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    pub(crate) fn scenario_mut(&mut self) -> &mut Scenario {
        &mut self.scenario
    }

    pub fn into_scenario(self) -> Scenario {
        self.scenario
    }
}

impl WorldStore for ScenarioStore {
    fn zones(&self) -> &[Zone] {
        &self.scenario.world.zones
    }
}

impl NetworkStore for ScenarioStore {
    fn stops(&self) -> &[Stop] {
        &self.scenario.world.stops
    }
    fn links(&self) -> &[Link] {
        &self.scenario.world.links
    }
    fn services(&self) -> &[Service] {
        &self.scenario.world.services
    }
    fn transfers(&self) -> &[Transfer] {
        &self.scenario.world.transfers
    }
    fn transfer_rules(&self) -> Option<&[TransferRule]> {
        self.scenario.world.transfer_rules.as_deref()
    }
}
