use super::ScenarioService;
use super::ScenarioStore;
use crate::model::{Link, Service, Stop, Transfer};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum NetworkEdit {
    AddStop(Stop),
    AddLink(Link),
    AddService(Service),
    AddTransfer(Transfer),
    // Later: remove, update, set headway, set fare, upgrade capacity, etc.
}

pub(crate) fn apply_network_edits(
    store: &mut ScenarioStore,
    edits: &[NetworkEdit],
) -> Result<(), String> {
    let world = &mut store.scenario_mut().world;

    for e in edits {
        match e {
            NetworkEdit::AddStop(x) => world.stops.push(x.clone()),
            NetworkEdit::AddLink(x) => world.links.push(x.clone()),
            NetworkEdit::AddService(x) => world.services.push(x.clone()),
            NetworkEdit::AddTransfer(x) => world.transfers.push(x.clone()),
        }
    }

    // Minimal sanity check after edits (fast).
    ScenarioService::validate(store.scenario()).map_err(|e| e.to_string())?;

    Ok(())
}
