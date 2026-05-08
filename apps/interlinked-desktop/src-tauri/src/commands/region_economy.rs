use super::super::*;
use crate::region::economy_service;
use tauri::{command, AppHandle};

#[command]
pub fn get_fare_policy(project_path: String) -> Result<FarePolicyManifest, String> {
    economy_service::get_fare_policy(project_path)
}

#[command]
pub fn set_fare_policy(
    state: tauri::State<AppState>,
    project_path: String,
    policy_patch: FarePolicyPatch,
) -> Result<FarePolicyManifest, String> {
    economy_service::set_fare_policy(state, project_path, policy_patch)
}

#[command]
pub fn expedite_fleet_delivery(
    state: tauri::State<AppState>,
    project_path: String,
    line_id: String,
    order_id: String,
) -> Result<FleetDeliveryExpediteResult, String> {
    economy_service::expedite_fleet_delivery(state, project_path, line_id, order_id)
}

#[command]
pub fn ensure_country_demand_surface(
    app: AppHandle,
    project_path: String,
    country_iso2: String,
) -> Result<DemandCoverageResult, String> {
    economy_service::ensure_country_demand_surface(app, project_path, country_iso2)
}

#[command]
pub fn list_demand_coverage(
    app: AppHandle,
    project_path: String,
) -> Result<Vec<DemandCoverageMeta>, String> {
    economy_service::list_demand_coverage(app, project_path)
}

#[command]
pub fn rebuild_demand_for_unlocked(
    app: AppHandle,
    project_path: String,
) -> Result<DemandRebuildResult, String> {
    economy_service::rebuild_demand_for_unlocked(app, project_path)
}

#[command]
pub fn get_financial_dashboard(
    app: AppHandle,
    project_path: String,
    request: FinancialDashboardRequest,
) -> Result<FinancialDashboardResponse, String> {
    economy_service::get_financial_dashboard(app, project_path, request)
}

#[command]
pub fn list_regions(app: AppHandle, project_path: String) -> Result<Vec<RegionStatus>, String> {
    economy_service::list_regions(app, project_path)
}

#[command]
pub fn unlock_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockResult, String> {
    economy_service::unlock_region(app, state, project_path, region_id)
}

#[command]
pub fn unlock_and_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockFocusResult, String> {
    economy_service::unlock_and_focus_region(app, state, project_path, region_id)
}

#[command]
pub fn set_primary_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<FocusResult, String> {
    economy_service::set_primary_focus_region(app, state, project_path, region_id)
}

#[command]
pub fn set_simulation_scope(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    scope: SimulationScopeUpdate,
) -> Result<ScopeState, String> {
    economy_service::set_simulation_scope(app, state, project_path, scope)
}

#[command]
pub fn get_demand_tile_source(
    project_path: String,
    layer: String,
) -> Result<DemandTileSourceMeta, String> {
    economy_service::get_demand_tile_source(project_path, layer)
}

#[command]
pub fn get_demand_layer_stats(project_path: String) -> Result<DemandLayerStats, String> {
    economy_service::get_demand_layer_stats(project_path)
}

#[command]
pub fn get_demand_overlay_payload(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    overlay_type: Option<String>,
) -> Result<DemandOverlayPayload, String> {
    economy_service::get_demand_overlay_payload(app, state, project_path, overlay_type)
}
