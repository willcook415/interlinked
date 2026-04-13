use crate::*;

// Demand pipeline authority contract (runtime/game path):
// 1) source demand input: country demand surface file chosen by content_library resolver.
// 2) persisted gameplay substrate: scenario.world.demand_cells + scenario.world.zones + demand_meta.
// 3) planner-effective substrate: planner derives effective zones from demand_cells when present.
// 4) runtime-transient shaping: perf budget trims only in-memory runtime clone, never persisted.
pub(crate) const DEMAND_PIPELINE_PERSISTED_META_SOURCE: &str = "surface_v4_region_scope";
pub(crate) const DEMAND_TILE_SOURCE_PERSISTED_CELLS: &str = "scenario.world.demand_cells.persisted";
pub(crate) const DEMAND_TILE_SOURCE_ZONES_FALLBACK: &str = "scenario.world.zones.fallback";

pub(crate) fn default_surface_pipeline_demand_meta() -> DemandMeta {
    DemandMeta {
        surface_version: "v4".to_string(),
        loaded_countries: vec![],
        source: DEMAND_PIPELINE_PERSISTED_META_SOURCE.to_string(),
    }
}

pub(crate) fn clear_surface_generated_persisted_demand(scenario: &mut Scenario) {
    scenario
        .world
        .demand_cells
        .retain(|cell| !is_surface_generated_cell_id(&cell.cell_id));
    scenario
        .world
        .zones
        .retain(|zone| !is_surface_generated_zone_id(&zone.id));
}

pub(crate) fn write_surface_pipeline_demand_meta(
    scenario: &mut Scenario,
    surface_version: Option<String>,
    loaded_countries: Vec<String>,
) -> (String, Vec<String>) {
    let loaded = normalize_loaded_countries(loaded_countries);
    let version = surface_version
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            scenario
                .world
                .demand_meta
                .as_ref()
                .map(|meta| meta.surface_version.clone())
        })
        .unwrap_or_else(|| "v4".to_string());
    scenario.world.demand_meta = Some(DemandMeta {
        surface_version: version.clone(),
        loaded_countries: loaded.clone(),
        source: DEMAND_PIPELINE_PERSISTED_META_SOURCE.to_string(),
    });
    (version, loaded)
}

pub(crate) fn sync_manifest_surface_pipeline_state(
    manifest: &mut ProjectManifest,
    surface_version: &str,
    loaded_countries: &[String],
) {
    let mut ds = manifest
        .demand_surface
        .clone()
        .unwrap_or_else(default_demand_surface_manifest);
    ds.surface_version = surface_version.to_string();
    ds.loaded_countries = loaded_countries.to_vec();
    ds.last_rebuild_at = Some(now_string());
    manifest.demand_surface = Some(ds);
}
