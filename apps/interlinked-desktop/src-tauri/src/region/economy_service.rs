use crate::commands::build_mutation::world_xy_to_lonlat_safe;
use crate::commands::content_library::{primary_project_country_iso2, resolve_demand_surface_path};
use crate::*;
use geo::algorithm::area::Area;
use geo::BooleanOps;
use geo::{Coord, LineString, MultiPolygon, Polygon};
use h3o::CellIndex;
use interlinked_engine::model::{DemandCell, Scenario};
use std::any::Any;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;
use tauri::AppHandle;

fn perf_log(label: &str, started: Instant) {
    eprintln!("[perf] {label}: {}ms", started.elapsed().as_millis());
}

pub(crate) fn get_fare_policy(project_path: String) -> Result<FarePolicyManifest, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    sanitize_fare_policy(&mut manifest.economy.fare_policy);
    Ok(manifest.economy.fare_policy)
}

pub(crate) fn set_fare_policy(
    state: tauri::State<AppState>,
    project_path: String,
    policy_patch: FarePolicyPatch,
) -> Result<FarePolicyManifest, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    merge_fare_policy(&mut manifest.economy.fare_policy, &policy_patch);
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let project_path_string = project_root.to_string_lossy().to_string();
    let _ = enqueue_runtime_action_with_retry(
        state.inner(),
        &project_path_string,
        RuntimeAction::InvalidateMaterialization,
    )?;
    Ok(manifest.economy.fare_policy)
}

pub(crate) fn expedite_fleet_delivery(
    state: tauri::State<AppState>,
    project_path: String,
    line_id: String,
    order_id: String,
) -> Result<FleetDeliveryExpediteResult, String> {
    let normalized_line_id = line_id.trim();
    if normalized_line_id.is_empty() {
        return Err("line_id is required".to_string());
    }
    let normalized_order_id = order_id.trim();
    if normalized_order_id.is_empty() {
        return Err("order_id is required".to_string());
    }

    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let mut manifest = read_manifest(&project_root)?;
    let defaults = default_build_defaults(&economy_config());

    let service_indexes = doc
        .scenario
        .world
        .services
        .iter()
        .enumerate()
        .filter_map(|(index, service)| {
            if service_line_runtime_id(service) == normalized_line_id {
                Some(index)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let Some(first_service_index) = service_indexes.first().copied() else {
        return Err("line not found".to_string());
    };
    let sample = doc
        .scenario
        .world
        .services
        .get(first_service_index)
        .cloned()
        .ok_or_else(|| "line service could not be resolved".to_string())?;
    let sample_profile = sample.rolling_stock_profile.clone().unwrap_or_default();
    let current_units_owned = sample_profile
        .units_owned
        .unwrap_or_else(|| sample.stock_units_owned.unwrap_or(0));
    let mut pending_orders = sample_profile.pending_orders.clone();
    if pending_orders.is_empty() {
        return Err("line has no pending purchase orders".to_string());
    }
    let Some(order_position) = pending_orders.iter().position(|order| {
        order.order_id.trim() == normalized_order_id
            && order.units > 0
            && is_pending_purchase_order_status(order.status.as_deref())
    }) else {
        return Err("pending purchase order not found on the selected line".to_string());
    };

    let fallback_unit_cost_base = estimate_unit_purchase_cost_base_for_service(&sample, &defaults);
    let unit_cost_base =
        resolve_order_unit_cost_base(&pending_orders[order_position], fallback_unit_cost_base)
            .ok_or_else(|| "unable to resolve unit purchase cost for this order".to_string())?;
    let expedite_cost_base = (unit_cost_base * FLEET_EXPEDITE_MULTIPLIER)
        .max(unit_cost_base + FLEET_EXPEDITE_MIN_SURCHARGE_BASE);
    if !expedite_cost_base.is_finite() || expedite_cost_base <= 0.0 {
        return Err("failed to compute expedite cost".to_string());
    }
    if manifest.economy.current_balance_base + 1e-6 < expedite_cost_base {
        return Err(format!(
            "Insufficient funds: requires {:.0} base currency, available {:.0}.",
            expedite_cost_base.max(0.0).round(),
            manifest.economy.current_balance_base.max(0.0).round()
        ));
    }

    let mut remaining_order_units = 0_u32;
    if let Some(order) = pending_orders.get_mut(order_position) {
        let per_unit_for_totals =
            resolve_order_unit_cost_base(order, Some(unit_cost_base)).unwrap_or(unit_cost_base);
        if order.units <= 1 {
            pending_orders.remove(order_position);
        } else {
            order.units = order.units.saturating_sub(1);
            remaining_order_units = order.units;
            if per_unit_for_totals.is_finite() && per_unit_for_totals > 0.0 {
                order.unit_cost_base = Some(per_unit_for_totals);
                order.total_cost_base = Some(per_unit_for_totals * order.units as f64);
            }
        }
    }

    let next_units_owned = current_units_owned.saturating_add(1);
    for service_index in service_indexes {
        let Some(service) = doc.scenario.world.services.get_mut(service_index) else {
            continue;
        };
        let mut profile = service.rolling_stock_profile.clone().unwrap_or_default();
        profile.units_owned = Some(next_units_owned);
        profile.pending_orders = pending_orders.clone();
        service.stock_units_owned = Some(next_units_owned);
        service.stock_units_assigned = Some(next_units_owned);
        service.rolling_stock_profile = Some(profile);
    }

    manifest.economy.current_balance_base -= expedite_cost_base;
    manifest.economy.cumulative_capex_base += expedite_cost_base;
    update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, expedite_cost_base);
    record_monthly_financial_delta(&mut manifest, 0.0, 0.0, expedite_cost_base, 0.0);
    bump_economy_revision(&mut manifest);
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();

    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    write_manifest(&project_root, &manifest)?;

    if project_is_current(&state, &project_path)? {
        let mut guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_mut() {
            rehydrate_game_state_scenario(game_state, &doc.scenario);
        }
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path,
            RuntimeAction::InvalidateMaterialization,
        )?;
    }

    Ok(FleetDeliveryExpediteResult {
        line_id: normalized_line_id.to_string(),
        order_id: normalized_order_id.to_string(),
        delivered_units: 1,
        remaining_order_units,
        expedite_cost_base,
        balance_after_base: manifest.economy.current_balance_base,
    })
}

pub(crate) fn ensure_country_demand_surface(
    app: AppHandle,
    project_path: String,
    country_iso2: String,
) -> Result<DemandCoverageResult, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let result =
        ensure_country_surface_loaded(&app, &mut manifest, &mut doc.scenario, &country_iso2)?;
    if result.loaded {
        ScenarioService::save_to_path(
            scenario_path(&project_root).to_string_lossy().as_ref(),
            &doc,
        )
        .map_err(|e| e.to_string())?;
    }
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(result)
}

pub(crate) fn list_demand_coverage(
    app: AppHandle,
    project_path: String,
) -> Result<Vec<DemandCoverageMeta>, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let unlocked = unlocked_country_codes(&manifest);
    let mut out = Vec::<DemandCoverageMeta>::new();
    for iso in unlocked {
        let cells = doc
            .scenario
            .world
            .demand_cells
            .iter()
            .filter(|c| {
                c.country_iso2
                    .as_deref()
                    .and_then(canonical_country_iso2)
                    .map(|v| v.eq_ignore_ascii_case(&iso))
                    .unwrap_or(false)
            })
            .count();
        out.push(DemandCoverageMeta {
            country_iso2: iso.clone(),
            installed: resolve_demand_surface_path(&app, &iso).is_some(),
            loaded_in_scenario: cells > 0,
            cells,
            surface_version: manifest
                .demand_surface
                .as_ref()
                .map(|d| d.surface_version.clone()),
        });
    }
    out.sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    Ok(out)
}

pub(crate) fn rebuild_demand_for_unlocked(
    app: AppHandle,
    project_path: String,
) -> Result<DemandRebuildResult, String> {
    let project_root = PathBuf::from(project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let mut loaded = Vec::<String>::new();
    let mut missing = Vec::<String>::new();
    for iso in unlocked_country_codes(&manifest) {
        let status = ensure_country_surface_loaded(&app, &mut manifest, &mut doc.scenario, &iso)?;
        if status.loaded {
            loaded.push(iso);
        } else {
            missing.push(iso);
        }
    }
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    Ok(DemandRebuildResult {
        loaded_countries: loaded,
        missing_countries: missing,
        total_cells: doc.scenario.world.demand_cells.len(),
    })
}

pub(crate) fn get_financial_dashboard(
    app: AppHandle,
    project_path: String,
    request: FinancialDashboardRequest,
) -> Result<FinancialDashboardResponse, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let cfg = economy_config();
    let minute_of_day =
        ((manifest.clock_state.tick_seconds / 60.0).floor() as i64).rem_euclid(1440) as u32;

    let granularity = normalize_financial_granularity(request.granularity.as_deref());
    let default_periods = match granularity.as_str() {
        "day" => 30,
        "week" => 16,
        "year" => 8,
        _ => 12,
    };
    let periods = request.periods.unwrap_or(default_periods).clamp(1, 240);
    let mode_filter = request
        .mode
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "all");
    let line_filter = request
        .line_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));
    let region_filter = request
        .region_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"))
        .and_then(|value| canonicalize_region_id(&value).or(Some(value)));
    let filters_active = mode_filter.is_some() || line_filter.is_some() || region_filter.is_some();

    let project_country_iso2 = primary_project_country_iso2(&manifest).unwrap_or_default();
    let planning_regions = if project_country_iso2.is_empty() {
        None
    } else {
        load_region_catalog_for_country(&app, &project_country_iso2)
            .ok()
            .flatten()
            .map(|catalog| catalog.regions)
    };

    let mut line_ids = BTreeSet::<String>::new();
    for service in &doc.scenario.world.services {
        line_ids.insert(service_line_runtime_id(service));
    }

    let mut all_line_rows = Vec::<FinancialLineBreakdownRow>::new();
    for line_id in line_ids {
        let inspection = match inspect_line_from_scenario(
            &doc.scenario,
            None,
            &line_id,
            &cfg,
            Some(minute_of_day),
        ) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let line_name = if inspection.name.trim().is_empty() {
            line_id.clone()
        } else {
            inspection.name.clone()
        };
        let mode = inspection.mode.trim().to_ascii_lowercase();
        let mut region_id = None::<String>;
        if let Some(regions) = planning_regions.as_ref() {
            let mut counts = HashMap::<String, usize>::new();
            for station in &inspection.stations {
                let Some((lon, lat)) =
                    world_xy_to_lonlat_safe(&doc.scenario.meta.crs, station.x, station.y)
                else {
                    continue;
                };
                let (mx, my) = lonlat_to_web_mercator_m(lon, lat);
                let nearest = regions.iter().min_by(|a, b| {
                    let da = (a.x - mx).powi(2) + (a.y - my).powi(2);
                    let db = (b.x - mx).powi(2) + (b.y - my).powi(2);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });
                let Some(region) = nearest else { continue };
                let key = canonicalize_region_id(&region.region_id)
                    .unwrap_or_else(|| region.region_id.clone());
                *counts.entry(key).or_insert(0) += 1;
            }
            region_id = counts
                .into_iter()
                .max_by(|(left_id, left_count), (right_id, right_count)| {
                    left_count
                        .cmp(right_count)
                        .then_with(|| left_id.cmp(right_id))
                })
                .map(|(id, _)| id);
        }

        all_line_rows.push(FinancialLineBreakdownRow {
            line_id: line_id.clone(),
            line_name,
            mode,
            region_id,
            estimated_capex_base: inspection.estimated_capex_base.max(0.0),
            estimated_opex_per_hour_base: inspection.estimated_opex_per_hour_base.max(0.0),
            staff_opex_per_hour_base: inspection.cost_story.staff_opex_per_hour_base.max(0.0),
            fleet_value_base: inspection.cost_story.fleet_value_base.max(0.0),
            units_owned: inspection.fleet_state.units_owned,
            units_pending: inspection.fleet_state.units_pending,
            units_assigned: inspection.fleet_state.units_assigned,
        });
    }
    all_line_rows.sort_by(|a, b| {
        b.estimated_opex_per_hour_base
            .partial_cmp(&a.estimated_opex_per_hour_base)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.line_name.cmp(&b.line_name))
    });

    let filtered_line_rows = all_line_rows
        .iter()
        .filter(|row| {
            if let Some(mode) = mode_filter.as_ref() {
                if row.mode != *mode {
                    return false;
                }
            }
            if let Some(line_id) = line_filter.as_ref() {
                if row.line_id != *line_id {
                    return false;
                }
            }
            if let Some(region_id) = region_filter.as_ref() {
                if row.region_id.as_deref() != Some(region_id.as_str()) {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut mode_breakdown_map = BTreeMap::<String, FinancialModeBreakdownRow>::new();
    for line in &filtered_line_rows {
        let entry =
            mode_breakdown_map
                .entry(line.mode.clone())
                .or_insert(FinancialModeBreakdownRow {
                    mode: line.mode.clone(),
                    lines: 0,
                    revenue_base: 0.0,
                    opex_base: 0.0,
                    capex_base: 0.0,
                    penalties_base: 0.0,
                    net_base: 0.0,
                });
        entry.lines = entry.lines.saturating_add(1);
        entry.opex_base += line.estimated_opex_per_hour_base.max(0.0);
        entry.capex_base += line.estimated_capex_base.max(0.0);
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    let mut mode_breakdown = mode_breakdown_map.into_values().collect::<Vec<_>>();
    mode_breakdown.sort_by(|a, b| a.mode.cmp(&b.mode));

    let mut canonical_region_ledger = manifest.economy.region_ledger.clone();
    canonicalize_region_ledger(&mut canonical_region_ledger);
    let mut region_breakdown = canonical_region_ledger
        .into_iter()
        .map(|(region_id, row)| FinancialRegionBreakdownRow {
            region_id,
            revenue_base: row.revenue_base.max(0.0),
            opex_base: row.opex_base.max(0.0),
            capex_base: row.capex_base.max(0.0),
            penalties_base: row.penalties_base.max(0.0),
            net_base: row.net_base,
        })
        .collect::<Vec<_>>();
    if let Some(region_id) = region_filter.as_ref() {
        region_breakdown.retain(|row| row.region_id == *region_id);
    }
    region_breakdown.sort_by(|a, b| {
        b.net_base
            .partial_cmp(&a.net_base)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });

    let all_line_opex_total = all_line_rows
        .iter()
        .map(|row| row.estimated_opex_per_hour_base.max(0.0))
        .sum::<f64>();
    let filtered_line_opex_total = filtered_line_rows
        .iter()
        .map(|row| row.estimated_opex_per_hour_base.max(0.0))
        .sum::<f64>();
    let mut filter_scale = 1.0_f64;
    if mode_filter.is_some() || line_filter.is_some() {
        filter_scale *= if all_line_opex_total > 0.0 {
            (filtered_line_opex_total / all_line_opex_total).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    if region_filter.is_some() {
        let selected_region_abs = region_breakdown
            .iter()
            .map(|row| row.revenue_base + row.opex_base + row.capex_base + row.penalties_base)
            .sum::<f64>();
        let global_abs = manifest.economy.cumulative_revenue_base.max(0.0)
            + manifest.economy.cumulative_opex_base.max(0.0)
            + manifest.economy.cumulative_capex_base.max(0.0)
            + manifest
                .economy
                .cumulative_lost_demand_penalty_base
                .max(0.0);
        filter_scale *= if global_abs > 0.0 {
            (selected_region_abs / global_abs).clamp(0.0, 1.0)
        } else if selected_region_abs > 0.0 {
            1.0
        } else {
            0.0
        };
    }
    if !filters_active {
        filter_scale = 1.0;
    }

    let mut monthly_points = financial_points_from_monthly(&manifest.economy.monthly_financials);
    if monthly_points.is_empty() {
        monthly_points.push(FinancialDashboardPoint {
            period_index: 0,
            label: "Now".to_string(),
            revenue_base: manifest.economy.cumulative_revenue_base.max(0.0),
            opex_base: manifest.economy.cumulative_opex_base.max(0.0),
            capex_base: manifest.economy.cumulative_capex_base.max(0.0),
            penalties_base: manifest
                .economy
                .cumulative_lost_demand_penalty_base
                .max(0.0),
            net_base: manifest.economy.cumulative_revenue_base.max(0.0)
                - manifest.economy.cumulative_opex_base.max(0.0)
                - manifest.economy.cumulative_capex_base.max(0.0)
                - manifest
                    .economy
                    .cumulative_lost_demand_penalty_base
                    .max(0.0),
        });
    }
    let points = financial_points_for_granularity(
        &monthly_points,
        granularity.as_str(),
        periods,
        filter_scale,
    );

    let (total_revenue_base, total_opex_base, total_capex_base, total_penalties_base) =
        if region_filter.is_some() && !region_breakdown.is_empty() {
            (
                region_breakdown
                    .iter()
                    .map(|row| row.revenue_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.opex_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.capex_base)
                    .sum::<f64>(),
                region_breakdown
                    .iter()
                    .map(|row| row.penalties_base)
                    .sum::<f64>(),
            )
        } else {
            (
                manifest.economy.cumulative_revenue_base.max(0.0) * filter_scale,
                manifest.economy.cumulative_opex_base.max(0.0) * filter_scale,
                manifest.economy.cumulative_capex_base.max(0.0) * filter_scale,
                manifest
                    .economy
                    .cumulative_lost_demand_penalty_base
                    .max(0.0)
                    * filter_scale,
            )
        };
    let total_net_base =
        total_revenue_base - total_opex_base - total_capex_base - total_penalties_base;

    Ok(FinancialDashboardResponse {
        currency: manifest.economy.currency.clone(),
        granularity,
        periods: points.len(),
        current_balance_base: manifest.economy.current_balance_base * filter_scale.max(0.0),
        total_revenue_base,
        total_opex_base,
        total_capex_base,
        total_penalties_base,
        total_net_base,
        points,
        mode_breakdown,
        line_breakdown: filtered_line_rows,
        region_breakdown,
    })
}

pub(crate) fn list_regions(
    app: AppHandle,
    project_path: String,
) -> Result<Vec<RegionStatus>, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let rows = region_status_rows_for_manifest(&app, &manifest)?;
    perf_log("list_regions", started);
    Ok(rows)
}

fn apply_region_scope_runtime_mutation(
    state: &tauri::State<AppState>,
    project_path: &str,
    scenario: &Scenario,
) -> Result<(), String> {
    if !project_is_current(state, project_path)? {
        return Ok(());
    }
    {
        let mut guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_mut() {
            rehydrate_game_state_scenario(game_state, scenario);
        }
    }
    let _ = enqueue_runtime_action_with_retry(
        state.inner(),
        project_path,
        RuntimeAction::InvalidateMaterialization,
    )?;
    Ok(())
}

fn commit_region_scope_change(
    app: &AppHandle,
    state: &tauri::State<AppState>,
    project_path: &str,
    project_root: &Path,
    manifest: &mut ProjectManifest,
    doc: &mut ScenarioDocument,
) -> Result<usize, String> {
    let started = Instant::now();
    let rematerialize_started = Instant::now();
    rematerialize_unlocked_country_surfaces(app, manifest, &mut doc.scenario)?;
    perf_log(
        "commit_region_scope_change.rematerialize_unlocked_country_surfaces",
        rematerialize_started,
    );
    let materialized_cells = doc.scenario.world.demand_cells.len();
    if let Err(error) = ScenarioService::validate(&doc.scenario) {
        eprintln!(
            "[demand-materialization] commit_region_scope_change validation_failed project={} unlocked_regions={} active_regions={} focus_region={} demand_cells={} zones={} error={}",
            project_path,
            manifest.region_state.unlocked_region_ids.len(),
            manifest.region_state.active_region_ids.len(),
            manifest
                .region_state
                .primary_focus_region_id
                .as_deref()
                .unwrap_or("none"),
            doc.scenario.world.demand_cells.len(),
            doc.scenario.world.zones.len(),
            error
        );
        return Err(error.to_string());
    }
    let write_scenario_started = Instant::now();
    ScenarioService::save_to_path(scenario_path(project_root).to_string_lossy().as_ref(), doc)
        .map_err(|e| e.to_string())?;
    perf_log(
        "commit_region_scope_change.write_scenario",
        write_scenario_started,
    );
    let write_manifest_started = Instant::now();
    manifest.updated_at = now_string();
    write_manifest(project_root, manifest)?;
    perf_log(
        "commit_region_scope_change.write_manifest",
        write_manifest_started,
    );
    let runtime_mutation_started = Instant::now();
    apply_region_scope_runtime_mutation(state, project_path, &doc.scenario)?;
    perf_log(
        "commit_region_scope_change.apply_runtime_mutation",
        runtime_mutation_started,
    );
    perf_log("commit_region_scope_change.total", started);
    Ok(materialized_cells)
}

fn apply_region_unlock(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    iso: &str,
    normalized_region: &str,
    focus_after_unlock: bool,
) -> Result<f64, String> {
    let region = catalog
        .by_id
        .get(normalized_region)
        .ok_or_else(|| format!("UnknownRegion: unknown region_id: {normalized_region}"))?;
    let unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let country_unlocked = unlocked
        .iter()
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso))
        .cloned()
        .collect::<HashSet<_>>();
    if !unlocked.contains(normalized_region)
        && !country_unlocked.is_empty()
        && !region
            .adjacent_region_ids
            .iter()
            .any(|rid| country_unlocked.contains(rid))
    {
        return Err(
            "RegionNotAdjacent: region must be adjacent to an already unlocked region".to_string(),
        );
    }

    let charge = if unlocked.contains(normalized_region) {
        0.0
    } else {
        let population_by_region = calibrated_region_population_for_country(iso, &catalog.regions);
        let jobs_by_region = calibrated_region_jobs_for_country(iso, &catalog.regions);
        region_unlock_cost_base_for_manifest(
            manifest,
            region,
            population_by_region.get(normalized_region).copied(),
            jobs_by_region.get(normalized_region).copied(),
        )
    };
    if charge > 0.0 && manifest.economy.current_balance_base < charge {
        return Err(format!(
            "InsufficientFunds: need {:.0} base units, have {:.0}",
            charge, manifest.economy.current_balance_base
        ));
    }
    sync_country_region_state_with_overrides(
        manifest,
        catalog,
        iso,
        RegionStateOverrides {
            ensure_unlocked_region_ids: vec![normalized_region.to_string()],
            force_primary_focus_region_id: focus_after_unlock
                .then(|| normalized_region.to_string()),
            force_active_region_ids: None,
        },
    )?;

    if charge > 0.0 {
        manifest.economy.current_balance_base -= charge;
        manifest.economy.cumulative_capex_base += charge;
        update_region_ledger(&mut *manifest, 0.0, 0.0, 0.0, charge);
        record_monthly_financial_delta(&mut *manifest, 0.0, 0.0, charge, 0.0);
        bump_economy_revision(&mut *manifest);
    }

    let mut countries = unlocked_country_codes(manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.to_string());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    sync_progress_budget_from_economy(manifest);
    Ok(charge)
}

pub(crate) fn unlock_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockResult, String> {
    // Unlock-only compatibility path. Uses the same validation and charging flow as
    // unlock_and_focus_region, but does not force focus override on success.
    let normalized_region = canonicalize_region_id(&region_id)
        .ok_or_else(|| "InvalidRegionId: invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "InvalidRegionId: invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!(
            "CountryPackMissing: no installed demand pack for country {iso}"
        ));
    };

    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let project_iso = primary_project_country_iso2(&manifest).unwrap_or_default();
    if !project_iso.is_empty() && !project_iso.eq_ignore_ascii_case(&iso) {
        return Err(format!(
            "WrongCountryScope: region belongs to {iso}, project country is {project_iso}"
        ));
    }
    let charge = apply_region_unlock(&mut manifest, &catalog, &iso, &normalized_region, false)?;

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let _ = commit_region_scope_change(
        &app,
        &state,
        &project_path,
        &project_root,
        &mut manifest,
        &mut doc,
    )?;

    Ok(UnlockResult {
        region_id: normalized_region,
        charged_base: charge,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_regions: manifest.region_state.unlocked_region_ids.len(),
    })
}

pub(crate) fn unlock_and_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockFocusResult, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let normalized_region = canonicalize_region_id(&region_id)
        .ok_or_else(|| "InvalidRegionId: invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "InvalidRegionId: invalid region country code".to_string())?;
    let project_iso = primary_project_country_iso2(&manifest).unwrap_or_default();
    if !project_iso.is_empty() && !project_iso.eq_ignore_ascii_case(&iso) {
        return Err(format!(
            "WrongCountryScope: region belongs to {iso}, project country is {project_iso}"
        ));
    }
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!(
            "CountryPackMissing: no installed demand pack for country {iso}"
        ));
    };
    let charge = apply_region_unlock(&mut manifest, &catalog, &iso, &normalized_region, true)?;

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let materialized_cells = commit_region_scope_change(
        &app,
        &state,
        &project_path,
        &project_root,
        &mut manifest,
        &mut doc,
    )?;
    perf_log("unlock_and_focus_region.total", started);

    Ok(UnlockFocusResult {
        region_id: normalized_region.clone(),
        charged_base: charge,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_regions: manifest.region_state.unlocked_region_ids.len(),
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
        unlocked_region_ids: manifest.region_state.unlocked_region_ids.clone(),
        unlocked_countries: manifest.economy.unlocked_countries.clone(),
        scenario: ScenarioDocumentLite {
            schema_version: doc.schema_version,
            scenario: doc.scenario,
        },
    })
}

pub(crate) fn set_primary_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<FocusResult, String> {
    let started = Instant::now();
    let normalized_region = canonicalize_region_id(&region_id)
        .ok_or_else(|| "InvalidRegionId: invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "InvalidRegionId: invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!(
            "CountryPackMissing: no installed demand pack for country {iso}"
        ));
    };
    if !catalog.by_id.contains_key(&normalized_region) {
        return Err(format!(
            "UnknownRegion: unknown region_id: {normalized_region}"
        ));
    }

    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region) {
        return Err("RegionLocked: region must be unlocked before setting focus".to_string());
    }
    sync_country_region_state_with_overrides(
        &mut manifest,
        &catalog,
        &iso,
        RegionStateOverrides {
            ensure_unlocked_region_ids: vec![],
            force_primary_focus_region_id: Some(normalized_region.clone()),
            force_active_region_ids: None,
        },
    )?;

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let materialized_cells = commit_region_scope_change(
        &app,
        &state,
        &project_path,
        &project_root,
        &mut manifest,
        &mut doc,
    )?;
    perf_log("set_primary_focus_region.total", started);
    Ok(FocusResult {
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_region_ids: manifest.region_state.unlocked_region_ids.clone(),
        unlocked_countries: manifest.economy.unlocked_countries.clone(),
        scenario: ScenarioDocumentLite {
            schema_version: doc.schema_version,
            scenario: doc.scenario,
        },
    })
}

pub(crate) fn set_simulation_scope(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    scope: SimulationScopeUpdate,
) -> Result<ScopeState, String> {
    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let requested_active_region_ids = scope.active_region_ids.clone();
    if let Some(max_active) = scope.max_active_zones {
        manifest.simulation_scope.max_active_zones = max_active.clamp(120, 5000);
    }
    if let Some(mode) = scope.remote_regions_mode {
        manifest.simulation_scope.remote_regions_mode = normalize_scope(&mode);
    }
    if let Some(interval) = scope.remote_update_interval_ticks {
        manifest.simulation_scope.remote_update_interval_ticks = interval.max(1);
    }
    if let Some(v) = scope.focus_max_active_zones {
        manifest.simulation_scope.focus_max_active_zones = v.clamp(120, 6000);
    }
    if let Some(v) = scope.adjacent_max_active_zones {
        manifest.simulation_scope.adjacent_max_active_zones =
            v.clamp(40, manifest.simulation_scope.focus_max_active_zones);
    }
    if let Some(v) = scope.remote_max_active_zones {
        manifest.simulation_scope.remote_max_active_zones =
            v.clamp(20, manifest.simulation_scope.adjacent_max_active_zones);
    }
    if let Some(v) = scope.adjacent_update_interval_ticks {
        manifest.simulation_scope.adjacent_update_interval_ticks = v.max(1);
    }
    if let Some(active_ids) = requested_active_region_ids.as_ref() {
        manifest.region_state.active_region_ids = active_ids
            .into_iter()
            .filter_map(|id| canonicalize_region_id(&id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    for iso in unlocked_country_codes(&manifest) {
        let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
            continue;
        };
        let active_override = requested_active_region_ids.as_ref().map(|ids| {
            ids.iter()
                .filter_map(|id| canonicalize_region_id(id))
                .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
                .collect::<Vec<_>>()
        });
        sync_country_region_state_with_overrides(
            &mut manifest,
            &catalog,
            &iso,
            RegionStateOverrides {
                ensure_unlocked_region_ids: vec![],
                force_primary_focus_region_id: None,
                force_active_region_ids: active_override,
            },
        )?;
    }
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let materialized_cells = commit_region_scope_change(
        &app,
        &state,
        &project_path,
        &project_root,
        &mut manifest,
        &mut doc,
    )?;
    Ok(ScopeState {
        max_active_zones: manifest.simulation_scope.max_active_zones,
        remote_regions_mode: manifest.simulation_scope.remote_regions_mode.clone(),
        remote_update_interval_ticks: manifest.simulation_scope.remote_update_interval_ticks,
        focus_max_active_zones: manifest.simulation_scope.focus_max_active_zones,
        adjacent_max_active_zones: manifest.simulation_scope.adjacent_max_active_zones,
        remote_max_active_zones: manifest.simulation_scope.remote_max_active_zones,
        adjacent_update_interval_ticks: manifest.simulation_scope.adjacent_update_interval_ticks,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
    })
}

pub(crate) fn get_demand_tile_source(
    project_path: String,
    layer: String,
) -> Result<DemandTileSourceMeta, String> {
    let project_root = PathBuf::from(project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let countries = doc
        .scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.loaded_countries.clone())
        .unwrap_or_default();
    let has_cells = !doc.scenario.world.demand_cells.is_empty();
    Ok(DemandTileSourceMeta {
        layer: layer.clone(),
        source: if has_cells {
            DEMAND_TILE_SOURCE_PERSISTED_CELLS.to_string()
        } else {
            DEMAND_TILE_SOURCE_ZONES_FALLBACK.to_string()
        },
        countries_loaded: countries,
        cells: if has_cells {
            doc.scenario.world.demand_cells.len()
        } else {
            doc.scenario.world.zones.len()
        },
        mode: if layer.eq_ignore_ascii_case("population") || layer.eq_ignore_ascii_case("jobs") {
            "smoothed_raster_overlay".to_string()
        } else {
            "unknown".to_string()
        },
    })
}

pub(crate) fn get_demand_layer_stats(project_path: String) -> Result<DemandLayerStats, String> {
    let project_root = PathBuf::from(project_path);
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;

    let mut residents = Vec::<f64>::new();
    let mut jobs = Vec::<f64>::new();
    let mut activity = Vec::<f64>::new();
    if !doc.scenario.world.demand_cells.is_empty() {
        for c in &doc.scenario.world.demand_cells {
            residents.push(c.residents_night.max(0.0));
            jobs.push(c.jobs_day.max(0.0));
            activity.push(
                c.activity_mix_residential
                    .max(c.activity_mix_office)
                    .max(c.activity_mix_retail)
                    .max(c.activity_mix_recreation)
                    .max(c.activity_mix_industrial)
                    .max(c.activity_mix_education)
                    .max(c.activity_mix_health),
            );
        }
    } else {
        for z in &doc.scenario.world.zones {
            residents.push(z.population.max(0.0));
            jobs.push(z.jobs.max(0.0));
            let denom = (z.population + z.jobs).max(1e-6);
            activity.push((z.population / denom).max(z.jobs / denom));
        }
    }

    residents.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    jobs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    activity.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(DemandLayerStats {
        cells: residents.len(),
        residents_min: residents.first().copied().unwrap_or(0.0),
        residents_p50: percentile(&residents, 0.5),
        residents_max: residents.last().copied().unwrap_or(0.0),
        jobs_min: jobs.first().copied().unwrap_or(0.0),
        jobs_p50: percentile(&jobs, 0.5),
        jobs_max: jobs.last().copied().unwrap_or(0.0),
        activity_min: activity.first().copied().unwrap_or(0.0),
        activity_p50: percentile(&activity, 0.5),
        activity_max: activity.last().copied().unwrap_or(0.0),
    })
}

fn resolve_latest_planning_output_path(
    project_root: &PathBuf,
    manifest: &ProjectManifest,
) -> Option<(String, PathBuf)> {
    let mut candidates = Vec::<String>::new();
    if let Some(run_id) = manifest.last_opened_run_id.as_ref() {
        let run_id = run_id.trim();
        if !run_id.is_empty() {
            candidates.push(run_id.to_string());
        }
    }
    for run_id in &manifest.recent_runs {
        let run_id = run_id.trim();
        if run_id.is_empty() || candidates.iter().any(|existing| existing == run_id) {
            continue;
        }
        candidates.push(run_id.to_string());
    }
    for run_id in candidates {
        let output_path = runs_dir(project_root).join(&run_id).join("output.json");
        if output_path.exists() {
            return Some((run_id, output_path));
        }
    }
    None
}

fn demand_cell_intensity_score(cell: &DemandCell) -> f64 {
    let residents = cell.residents_night.max(0.0);
    let jobs = cell.jobs_day.max(0.0);
    let base = (residents + jobs).max(0.0);
    if base <= 0.0 {
        return 0.0;
    }
    let dominant_mix = cell
        .activity_mix_residential
        .max(cell.activity_mix_office)
        .max(cell.activity_mix_retail)
        .max(cell.activity_mix_recreation)
        .max(cell.activity_mix_industrial)
        .max(cell.activity_mix_education)
        .max(cell.activity_mix_health)
        .clamp(0.0, 1.0);
    let mixed_use_bonus = (1.0 - dominant_mix).clamp(0.0, 1.0) * 0.18;
    let civic_mix =
        (cell.activity_mix_education.max(0.0) + cell.activity_mix_health.max(0.0)).clamp(0.0, 1.0);
    base * (1.0 + mixed_use_bonus + civic_mix * 0.08)
}

#[derive(Debug, Clone, Default)]
struct DemandRegionLookup {
    unlocked_region_ids: HashSet<String>,
    region_name_by_id: HashMap<String, String>,
    region_lonlat_by_id: HashMap<String, (f64, f64)>,
    region_centers_xy: Vec<(String, f64, f64)>,
    region_id_by_zone_token: HashMap<String, String>,
    unlocked_geometry: Option<MultiPolygon<f64>>,
    unlocked_geometry_region_ids: Vec<String>,
    unlocked_geometry_missing_region_ids: Vec<String>,
    explicit_geometry_region_count: usize,
    h3_fallback_geometry_region_count: usize,
}

#[derive(Debug, Clone)]
struct StrategicOverlayLayers {
    source_kind: &'static str,
    run_id: Option<String>,
    service_gap_layer: Vec<interlinked_engine::sim::ZoneServiceGapLayerData>,
    corridor_desire_lines: Vec<interlinked_engine::sim::CorridorDesireLineData>,
}

#[derive(Debug, Clone, Default)]
struct DemandOverlayGeometryClipResult {
    display_geometry: Option<JsonValue>,
    rendered_geometry: Option<MultiPolygon<f64>>,
    clipped: bool,
    fully_inside: bool,
    invalid_display_geometry: bool,
    clip_failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemandOverlayRegionGeometrySource {
    Explicit,
    H3Fallback,
}

#[derive(Debug, Clone)]
struct DemandOverlayRegionGeometry {
    geometry: MultiPolygon<f64>,
    source: DemandOverlayRegionGeometrySource,
}

#[derive(Debug, Clone, Default)]
struct DemandOverlayCoverageDiagnostics {
    debug_enabled: bool,
    failed: bool,
    error: Option<String>,
    unlocked_union_area: f64,
    rendered_union_area: f64,
    uncovered_unlocked_area: f64,
    uncovered_ratio: f64,
    outside_rendered_area: f64,
    outside_ratio: f64,
    expected_intersecting_cell_count: usize,
    existing_intersecting_cell_count: usize,
    missing_intersecting_cell_count: usize,
    filtered_intersecting_cell_count: usize,
}

fn demand_overlay_debug_coverage_enabled() -> bool {
    std::env::var("INTERLINKED_DEBUG_DEMAND_COVERAGE")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn geometry_panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown geometry panic".to_string()
    }
}

fn safe_geometry_intersection(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
    label: &str,
) -> Result<MultiPolygon<f64>, String> {
    catch_unwind(AssertUnwindSafe(|| left.intersection(right)))
        .map_err(|payload| format!("{label}: {}", geometry_panic_message(payload)))
}

fn safe_geometry_difference(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
    label: &str,
) -> Result<MultiPolygon<f64>, String> {
    catch_unwind(AssertUnwindSafe(|| left.difference(right)))
        .map_err(|payload| format!("{label}: {}", geometry_panic_message(payload)))
}

fn safe_geometry_union(
    left: &MultiPolygon<f64>,
    right: &MultiPolygon<f64>,
    label: &str,
) -> Result<MultiPolygon<f64>, String> {
    catch_unwind(AssertUnwindSafe(|| left.union(right)))
        .map_err(|payload| format!("{label}: {}", geometry_panic_message(payload)))
}

fn geojson_value_to_geometry(value: &JsonValue, label: &str) -> Result<geo::Geometry<f64>, String> {
    let geometry = if let Ok(geometry) = serde_json::from_value::<geojson::Geometry>(value.clone())
    {
        geometry
    } else if let Ok(feature) = serde_json::from_value::<geojson::Feature>(value.clone()) {
        feature
            .geometry
            .ok_or_else(|| format!("{label} geometry feature missing geometry"))?
    } else {
        return Err(format!(
            "{label} geometry is not a parseable GeoJSON Geometry/Feature"
        ));
    };

    geo::Geometry::try_from(&geometry.value)
        .map_err(|error| format!("{label} geometry conversion failed: {error}"))
}

fn geometry_to_multipolygon(geometry: geo::Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match geometry {
        geo::Geometry::Polygon(polygon) => Some(MultiPolygon(vec![polygon])),
        geo::Geometry::MultiPolygon(multipolygon) => Some(multipolygon),
        geo::Geometry::GeometryCollection(collection) => {
            let mut out = None::<MultiPolygon<f64>>;
            for geometry in collection {
                let Some(part) = geometry_to_multipolygon(geometry) else {
                    continue;
                };
                out = Some(match out {
                    Some(existing) => {
                        safe_geometry_union(&existing, &part, "region geometry collection union")
                            .unwrap_or_else(|error| {
                                eprintln!("[demand-overlay-geometry] {error}");
                                let mut polygons = existing.0;
                                polygons.extend(part.0);
                                MultiPolygon(polygons)
                            })
                    }
                    None => part,
                });
            }
            out
        }
        _ => None,
    }
}

fn merge_unlocked_geometry(target: &mut Option<MultiPolygon<f64>>, incoming: MultiPolygon<f64>) {
    *target = Some(match target.take() {
        Some(existing) => {
            safe_geometry_union(&existing, &incoming, "merge unlocked overlay geometry")
                .unwrap_or_else(|error| {
                    eprintln!("[demand-overlay-geometry] {error}");
                    let mut polygons = existing.0;
                    polygons.extend(incoming.0);
                    MultiPolygon(polygons)
                })
        }
        None => incoming,
    });
}

fn h3_cell_boundary_polygon(cell: CellIndex) -> Option<Polygon<f64>> {
    let boundary = cell.boundary();
    let mut coords = boundary
        .iter()
        .map(|point| Coord {
            x: point.lng(),
            y: point.lat(),
        })
        .collect::<Vec<_>>();
    let first = coords.first().copied()?;
    if coords.last().copied() != Some(first) {
        coords.push(first);
    }
    if coords.len() < 4 {
        return None;
    }
    Some(Polygon::new(LineString::from(coords), vec![]))
}

fn h3_cell_id_multipolygon(cell_id: &str) -> Option<MultiPolygon<f64>> {
    let h3_cell_id = normalized_h3_cell_id(cell_id)?;
    let cell = h3_cell_id.parse::<CellIndex>().ok()?;
    Some(MultiPolygon(vec![h3_cell_boundary_polygon(cell)?]))
}

fn closed_ring_coordinates(line: &LineString<f64>) -> Option<Vec<Vec<f64>>> {
    let mut coords = line
        .points()
        .map(|point| vec![point.x(), point.y()])
        .collect::<Vec<_>>();
    if coords.len() < 3 {
        return None;
    }
    if coords.first() != coords.last() {
        let first = coords.first()?.clone();
        coords.push(first);
    }
    (coords.len() >= 4).then_some(coords)
}

fn polygon_to_geojson_coordinates(polygon: &Polygon<f64>) -> Option<Vec<Vec<Vec<f64>>>> {
    let exterior = closed_ring_coordinates(polygon.exterior())?;
    let mut rings = vec![exterior];
    for interior in polygon.interiors() {
        if let Some(ring) = closed_ring_coordinates(interior) {
            rings.push(ring);
        }
    }
    Some(rings)
}

fn multipolygon_to_geojson_geometry(multipolygon: &MultiPolygon<f64>) -> Option<JsonValue> {
    let mut polygons = multipolygon
        .0
        .iter()
        .filter(|polygon| polygon.unsigned_area() > 1e-18)
        .filter_map(polygon_to_geojson_coordinates)
        .collect::<Vec<_>>();
    if polygons.is_empty() {
        return None;
    }
    let geometry = if polygons.len() == 1 {
        geojson::Geometry::new(geojson::Value::Polygon(polygons.pop()?))
    } else {
        geojson::Geometry::new(geojson::Value::MultiPolygon(polygons))
    };
    serde_json::to_value(geometry).ok()
}

fn region_overlay_geometry_multipolygon(
    region: &SurfaceRegionInfo,
) -> Result<Option<DemandOverlayRegionGeometry>, String> {
    if let Some(geometry_value) = region.geometry.as_ref() {
        let label = format!("region {}", region.region_id);
        if let Some(geometry) =
            geometry_to_multipolygon(geojson_value_to_geometry(geometry_value, &label)?)
        {
            return Ok(Some(DemandOverlayRegionGeometry {
                geometry,
                source: DemandOverlayRegionGeometrySource::Explicit,
            }));
        }
    }

    // Some unlocked planning-region rows are H3-backed fallback regions rather
    // than authored polygons. They are still player-facing unlocked geometry,
    // so the overlay clip union must include their H3 region cell.
    for candidate in [
        region.h3_cell_id.as_deref(),
        Some(region.region_id.as_str()),
        Some(region.cell_id.as_str()),
        Some(region.region_token.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(geometry) = h3_cell_id_multipolygon(candidate) {
            return Ok(Some(DemandOverlayRegionGeometry {
                geometry,
                source: DemandOverlayRegionGeometrySource::H3Fallback,
            }));
        }
    }

    Ok(None)
}

fn h3_cell_multipolygon_intersects_geometry(cell: CellIndex, geometry: &MultiPolygon<f64>) -> bool {
    h3_cell_boundary_polygon(cell)
        .and_then(|polygon| {
            safe_geometry_intersection(
                &MultiPolygon(vec![polygon]),
                geometry,
                "H3 coverage candidate intersection",
            )
            .ok()
        })
        .map(|intersection| intersection.unsigned_area() > 1e-18)
        .unwrap_or(false)
}

fn include_neighboring_h3_cells_intersecting_geometry(
    geometry: &MultiPolygon<f64>,
    cells: &mut HashSet<CellIndex>,
) -> usize {
    let mut frontier = cells.iter().copied().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut added = 0usize;
    while cursor < frontier.len() {
        let seed = frontier[cursor];
        cursor += 1;
        for candidate in seed.grid_disk::<Vec<_>>(1) {
            if candidate.resolution() != h3o::Resolution::Eight || cells.contains(&candidate) {
                continue;
            }
            if h3_cell_multipolygon_intersects_geometry(candidate, geometry) {
                cells.insert(candidate);
                frontier.push(candidate);
                added += 1;
            }
        }
    }
    added
}

fn h3_res8_cells_intersecting_multipolygon(geometry: &MultiPolygon<f64>) -> HashSet<CellIndex> {
    let mut out = HashSet::<CellIndex>::new();
    if geometry.0.is_empty() || geometry.unsigned_area() <= 1e-18 {
        return out;
    }
    let geo_geometry = geo::Geometry::MultiPolygon(geometry.clone());
    if let Ok(h3_geometry) = h3o::geom::Geometry::from_degrees(geo_geometry) {
        let config = h3o::geom::PolyfillConfig::new(h3o::Resolution::Eight)
            .containment_mode(h3o::geom::ContainmentMode::Covers);
        use h3o::geom::ToCells;
        for cell in h3_geometry.to_cells(config) {
            if cell.resolution() == h3o::Resolution::Eight {
                out.insert(cell);
            }
        }
    }
    include_neighboring_h3_cells_intersecting_geometry(geometry, &mut out);
    out
}

fn demand_overlay_coverage_diagnostics(
    enabled: bool,
    unlocked_geometry: Option<&MultiPolygon<f64>>,
    rendered_geometry: Option<&MultiPolygon<f64>>,
    existing_h3_cells: &HashSet<CellIndex>,
    rendered_h3_cells: &HashSet<CellIndex>,
) -> DemandOverlayCoverageDiagnostics {
    if !enabled {
        let unlocked_union_area = unlocked_geometry
            .map(|geometry| geometry.unsigned_area().max(0.0))
            .unwrap_or(0.0);
        return DemandOverlayCoverageDiagnostics {
            debug_enabled: false,
            unlocked_union_area,
            ..DemandOverlayCoverageDiagnostics::default()
        };
    }
    let Some(unlocked_geometry) = unlocked_geometry else {
        return DemandOverlayCoverageDiagnostics {
            debug_enabled: true,
            ..DemandOverlayCoverageDiagnostics::default()
        };
    };
    let unlocked_union_area = unlocked_geometry.unsigned_area().max(0.0);
    if unlocked_union_area <= 1e-18 {
        return DemandOverlayCoverageDiagnostics {
            debug_enabled: true,
            ..DemandOverlayCoverageDiagnostics::default()
        };
    }

    let empty_rendered = MultiPolygon::<f64>(Vec::new());
    let rendered_geometry = rendered_geometry.unwrap_or(&empty_rendered);
    let rendered_union_area = rendered_geometry.unsigned_area().max(0.0);
    let uncovered_geometry = match safe_geometry_difference(
        unlocked_geometry,
        rendered_geometry,
        "demand overlay coverage unlocked-minus-rendered difference",
    ) {
        Ok(geometry) => geometry,
        Err(error) => {
            return DemandOverlayCoverageDiagnostics {
                debug_enabled: true,
                failed: true,
                error: Some(error),
                unlocked_union_area,
                rendered_union_area,
                ..DemandOverlayCoverageDiagnostics::default()
            };
        }
    };
    let uncovered_unlocked_area = uncovered_geometry.unsigned_area().max(0.0);
    let outside_rendered_area = match safe_geometry_difference(
        rendered_geometry,
        unlocked_geometry,
        "demand overlay coverage rendered-minus-unlocked difference",
    ) {
        Ok(geometry) => geometry.unsigned_area().max(0.0),
        Err(error) => {
            return DemandOverlayCoverageDiagnostics {
                debug_enabled: true,
                failed: true,
                error: Some(error),
                unlocked_union_area,
                rendered_union_area,
                uncovered_unlocked_area,
                uncovered_ratio: (uncovered_unlocked_area / unlocked_union_area).clamp(0.0, 1.0),
                ..DemandOverlayCoverageDiagnostics::default()
            };
        }
    };
    let expected_cells = h3_res8_cells_intersecting_multipolygon(unlocked_geometry);
    let uncovered_cells = if uncovered_unlocked_area > unlocked_union_area * 1e-9 {
        h3_res8_cells_intersecting_multipolygon(&uncovered_geometry)
    } else {
        HashSet::<CellIndex>::new()
    };
    let mut existing_intersecting_cell_count = 0usize;
    let mut missing_intersecting_cell_count = 0usize;
    let mut filtered_intersecting_cell_count = 0usize;
    for cell in &expected_cells {
        if existing_h3_cells.contains(cell) {
            existing_intersecting_cell_count += 1;
        }
    }
    for cell in &uncovered_cells {
        if existing_h3_cells.contains(cell) {
            if !rendered_h3_cells.contains(cell) {
                filtered_intersecting_cell_count += 1;
            }
        } else {
            missing_intersecting_cell_count += 1;
        }
    }

    DemandOverlayCoverageDiagnostics {
        debug_enabled: true,
        failed: false,
        error: None,
        unlocked_union_area,
        rendered_union_area,
        uncovered_unlocked_area,
        uncovered_ratio: (uncovered_unlocked_area / unlocked_union_area).clamp(0.0, 1.0),
        outside_rendered_area,
        outside_ratio: (outside_rendered_area / unlocked_union_area).max(0.0),
        expected_intersecting_cell_count: expected_cells.len(),
        existing_intersecting_cell_count,
        missing_intersecting_cell_count,
        filtered_intersecting_cell_count,
    }
}

/// Demand overlay display geometry contract:
/// H3 demand cells remain the unique internal substrate, but player-facing
/// overlay geometry is intersected and clipped against the merged unlocked
/// planning-region geometry. Boundary cells may render as partial polygons;
/// cells outside the unlocked geometry are omitted from the overlay payload.
fn clip_demand_overlay_cell_geometry(
    cell_id: &str,
    unlocked_geometry: Option<&MultiPolygon<f64>>,
) -> Option<DemandOverlayGeometryClipResult> {
    let Some(unlocked_geometry) = unlocked_geometry else {
        return Some(DemandOverlayGeometryClipResult::default());
    };
    let h3_cell_id = normalized_h3_cell_id(cell_id)?;
    let cell = h3_cell_id.parse::<CellIndex>().ok()?;
    let cell_multipolygon = h3_cell_id_multipolygon(&cell.to_string())?;
    let cell_area = cell_multipolygon.unsigned_area().max(1e-18);
    let clipped = match safe_geometry_intersection(
        &cell_multipolygon,
        unlocked_geometry,
        "demand overlay cell clip intersection",
    ) {
        Ok(clipped) => clipped,
        Err(error) => {
            eprintln!("[demand-overlay-geometry] {error}");
            return Some(DemandOverlayGeometryClipResult {
                clipped: true,
                invalid_display_geometry: true,
                clip_failed: true,
                ..DemandOverlayGeometryClipResult::default()
            });
        }
    };
    let clipped_area = clipped.unsigned_area();
    if clipped_area <= cell_area * 1e-9 {
        return None;
    }
    if clipped_area >= cell_area * (1.0 - 1e-7) {
        return Some(DemandOverlayGeometryClipResult {
            display_geometry: None,
            rendered_geometry: Some(cell_multipolygon),
            clipped: false,
            fully_inside: true,
            invalid_display_geometry: false,
            clip_failed: false,
        });
    }
    let display_geometry = multipolygon_to_geojson_geometry(&clipped);
    let invalid_display_geometry = display_geometry.is_none();
    Some(DemandOverlayGeometryClipResult {
        display_geometry,
        rendered_geometry: (!invalid_display_geometry).then_some(clipped),
        clipped: true,
        fully_inside: false,
        invalid_display_geometry,
        clip_failed: false,
    })
}

fn normalized_token(token: &str) -> Option<String> {
    let value = token.trim().to_ascii_lowercase();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn zone_lookup_tokens(zone_id: &str) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    let normalized = zone_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return out;
    }
    let mut push_token = |token: String| {
        if token.is_empty() || !seen.insert(token.clone()) {
            return;
        }
        out.push(token);
    };
    push_token(normalized.clone());
    if let Some(rest) = normalized.strip_prefix("z:") {
        push_token(rest.to_string());
    }
    for token in normalized.split(':').rev() {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        push_token(trimmed.to_string());
    }
    out
}

fn resolve_region_id_for_zone_id(zone_id: &str, lookup: &DemandRegionLookup) -> Option<String> {
    let normalized_zone = zone_id.trim().to_ascii_lowercase();
    if normalized_zone.is_empty() {
        return None;
    }
    if lookup.unlocked_region_ids.contains(&normalized_zone) {
        return Some(normalized_zone);
    }
    if let Some(canonical) = canonicalize_region_id(zone_id) {
        if lookup.unlocked_region_ids.contains(&canonical) {
            return Some(canonical);
        }
    }
    for token in zone_lookup_tokens(zone_id) {
        if let Some(region_id) = lookup.region_id_by_zone_token.get(&token) {
            return Some(region_id.clone());
        }
    }
    None
}

fn nearest_unlocked_region_for_world_point(
    scenario: &Scenario,
    lookup: &DemandRegionLookup,
    x: f64,
    y: f64,
) -> Option<String> {
    let (lon, lat) = world_xy_to_lonlat_safe(&scenario.meta.crs, x, y)?;
    let (mx, my) = lonlat_to_web_mercator_m(lon, lat);
    lookup
        .region_centers_xy
        .iter()
        .min_by(|left, right| {
            let dl = (left.1 - mx).powi(2) + (left.2 - my).powi(2);
            let dr = (right.1 - mx).powi(2) + (right.2 - my).powi(2);
            dl.partial_cmp(&dr).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|row| row.0.clone())
}

fn zone_world_xy_index(scenario: &Scenario) -> HashMap<String, (f64, f64)> {
    let mut out = HashMap::<String, (f64, f64)>::new();
    for cell in &scenario.world.demand_cells {
        let key = cell.cell_id.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        out.insert(key, (cell.x, cell.y));
    }
    for zone in &scenario.world.zones {
        let key = zone.id.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        out.entry(key).or_insert((zone.x, zone.y));
    }
    out
}

fn resolve_region_id_for_zone_with_fallback(
    zone_id: &str,
    lookup: &DemandRegionLookup,
    scenario: &Scenario,
    zone_xy_by_id: &HashMap<String, (f64, f64)>,
) -> Option<String> {
    if let Some(region_id) = resolve_region_id_for_zone_id(zone_id, lookup) {
        return Some(region_id);
    }
    let key = zone_id.trim().to_ascii_lowercase();
    let (x, y) = zone_xy_by_id.get(&key).copied()?;
    nearest_unlocked_region_for_world_point(scenario, lookup, x, y)
}

fn build_demand_region_lookup(
    app: &AppHandle,
    manifest: &ProjectManifest,
) -> Result<DemandRegionLookup, String> {
    let mut lookup = DemandRegionLookup::default();
    for iso in unlocked_country_codes(manifest) {
        let Some(catalog) = load_region_catalog_for_country(app, &iso)? else {
            continue;
        };
        let mut unlocked_catalog_regions = HashSet::<String>::new();
        for raw in &manifest.region_state.unlocked_region_ids {
            if let Some(canonical) = canonical_region_for_catalog(&catalog, raw) {
                unlocked_catalog_regions.insert(canonical);
                continue;
            }
            if let Some(canonical) = canonicalize_region_id(raw)
                .and_then(|value| canonical_region_for_catalog(&catalog, &value))
            {
                unlocked_catalog_regions.insert(canonical);
            }
        }
        for region in &catalog.regions {
            if !unlocked_catalog_regions.contains(&region.region_id) {
                continue;
            }
            lookup.unlocked_region_ids.insert(region.region_id.clone());
            lookup
                .region_name_by_id
                .entry(region.region_id.clone())
                .or_insert_with(|| region.name.clone());
            lookup
                .region_id_by_zone_token
                .entry(region.region_id.to_ascii_lowercase())
                .or_insert_with(|| region.region_id.clone());
            if let Some(region_geometry) = region_overlay_geometry_multipolygon(region)? {
                merge_unlocked_geometry(&mut lookup.unlocked_geometry, region_geometry.geometry);
                match region_geometry.source {
                    DemandOverlayRegionGeometrySource::Explicit => {
                        lookup.explicit_geometry_region_count += 1;
                    }
                    DemandOverlayRegionGeometrySource::H3Fallback => {
                        lookup.h3_fallback_geometry_region_count += 1;
                    }
                }
                lookup
                    .unlocked_geometry_region_ids
                    .push(region.region_id.clone());
            } else {
                lookup
                    .unlocked_geometry_missing_region_ids
                    .push(region.region_id.clone());
            }
            if let Some(token) = normalized_token(&region.cell_id) {
                lookup
                    .region_id_by_zone_token
                    .entry(token)
                    .or_insert_with(|| region.region_id.clone());
            }
            if let Some(token) = normalized_token(&region.region_token) {
                lookup
                    .region_id_by_zone_token
                    .entry(token)
                    .or_insert_with(|| region.region_id.clone());
            }
            if let Some(token) = region
                .h3_cell_id
                .as_ref()
                .and_then(|value| normalized_token(value))
            {
                lookup
                    .region_id_by_zone_token
                    .entry(token)
                    .or_insert_with(|| region.region_id.clone());
            }
            if !lookup.region_lonlat_by_id.contains_key(&region.region_id) {
                let (lon, lat) = web_mercator_m_to_lonlat(region.x, region.y);
                lookup
                    .region_lonlat_by_id
                    .insert(region.region_id.clone(), (lon, lat));
                lookup
                    .region_centers_xy
                    .push((region.region_id.clone(), region.x, region.y));
            }
        }

        for (region_id, cells) in &catalog.cells_res8_by_region {
            let Some(canonical_region_id) = canonical_region_for_catalog(&catalog, region_id)
                .or_else(|| {
                    let normalized = canonicalize_region_id(region_id)?;
                    if catalog.by_id.contains_key(&normalized) {
                        Some(normalized)
                    } else {
                        None
                    }
                })
            else {
                continue;
            };
            if !unlocked_catalog_regions.contains(&canonical_region_id) {
                continue;
            }
            for cell in cells {
                if let Some(token) = normalized_token(&cell.cell_id) {
                    lookup
                        .region_id_by_zone_token
                        .entry(token)
                        .or_insert_with(|| canonical_region_id.clone());
                }
            }
        }
    }
    Ok(lookup)
}

fn build_world_intensity_by_region(
    scenario: &Scenario,
    lookup: &DemandRegionLookup,
) -> (HashMap<String, f64>, usize) {
    let mut by_region = HashMap::<String, f64>::new();
    let mut mapped_samples = 0usize;

    if !scenario.world.demand_cells.is_empty() {
        for cell in &scenario.world.demand_cells {
            let Some(region_id) =
                resolve_region_id_for_zone_id(&cell.cell_id, lookup).or_else(|| {
                    nearest_unlocked_region_for_world_point(scenario, lookup, cell.x, cell.y)
                })
            else {
                continue;
            };
            let score = demand_cell_intensity_score(cell);
            *by_region.entry(region_id).or_insert(0.0) += score.max(0.0);
            mapped_samples += 1;
        }
        return (by_region, mapped_samples);
    }

    for zone in &scenario.world.zones {
        let Some(region_id) = resolve_region_id_for_zone_id(&zone.id, lookup)
            .or_else(|| nearest_unlocked_region_for_world_point(scenario, lookup, zone.x, zone.y))
        else {
            continue;
        };
        let score = (zone.population.max(0.0) + zone.jobs.max(0.0)).max(0.0);
        *by_region.entry(region_id).or_insert(0.0) += score;
        mapped_samples += 1;
    }
    (by_region, mapped_samples)
}

fn resolve_strategic_overlay_layers(
    state: &AppState,
    project_root: &PathBuf,
    manifest: &ProjectManifest,
    project_key: &str,
) -> Option<StrategicOverlayLayers> {
    if let Ok(cache) = state.runtime_strategic_demand_cache.lock() {
        if let Some(entry) = cache.get(project_key) {
            if !entry.service_gap_layer.is_empty() || !entry.corridor_desire_lines.is_empty() {
                return Some(StrategicOverlayLayers {
                    source_kind: "runtime",
                    run_id: None,
                    service_gap_layer: entry.service_gap_layer.clone(),
                    corridor_desire_lines: entry.corridor_desire_lines.clone(),
                });
            }
        }
    }

    let planning = resolve_latest_planning_output_path(project_root, manifest).and_then(
        |(run_id, output_path)| {
            read_json_file::<SimulationOutput>(&output_path)
                .ok()
                .map(|output| (run_id, output))
        },
    )?;
    Some(StrategicOverlayLayers {
        source_kind: "planning_run",
        run_id: Some(planning.0),
        service_gap_layer: planning.1.service_gap_layer,
        corridor_desire_lines: planning.1.corridor_desire_lines,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestedDemandOverlay {
    All,
    Intensity,
    ServiceGap,
    CorridorDesire,
    ResidentialAllocation,
    EmploymentAllocation,
    TotalAllocation,
    RawResidentialWeight,
    RawEmploymentWeight,
    FallbackCells,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemandCellOverlayMode {
    ResidentialAllocation,
    EmploymentAllocation,
    TotalAllocation,
    RawResidentialWeight,
    RawEmploymentWeight,
    FallbackCells,
}

fn parse_requested_demand_overlay(overlay_type: Option<String>) -> RequestedDemandOverlay {
    let Some(raw) = overlay_type else {
        return RequestedDemandOverlay::All;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "intensity" => RequestedDemandOverlay::Intensity,
        "service_gap" => RequestedDemandOverlay::ServiceGap,
        "corridor_desire" => RequestedDemandOverlay::CorridorDesire,
        "residential_allocation" => RequestedDemandOverlay::ResidentialAllocation,
        "employment_allocation" => RequestedDemandOverlay::EmploymentAllocation,
        "total_allocation" => RequestedDemandOverlay::TotalAllocation,
        "raw_residential_weight" => RequestedDemandOverlay::RawResidentialWeight,
        "raw_employment_weight" => RequestedDemandOverlay::RawEmploymentWeight,
        "fallback_cells" => RequestedDemandOverlay::FallbackCells,
        _ => RequestedDemandOverlay::All,
    }
}

fn requested_cell_overlay_mode(
    requested_overlay: RequestedDemandOverlay,
) -> Option<DemandCellOverlayMode> {
    match requested_overlay {
        RequestedDemandOverlay::ResidentialAllocation => {
            Some(DemandCellOverlayMode::ResidentialAllocation)
        }
        RequestedDemandOverlay::EmploymentAllocation => {
            Some(DemandCellOverlayMode::EmploymentAllocation)
        }
        RequestedDemandOverlay::TotalAllocation => Some(DemandCellOverlayMode::TotalAllocation),
        RequestedDemandOverlay::RawResidentialWeight => {
            Some(DemandCellOverlayMode::RawResidentialWeight)
        }
        RequestedDemandOverlay::RawEmploymentWeight => {
            Some(DemandCellOverlayMode::RawEmploymentWeight)
        }
        RequestedDemandOverlay::FallbackCells => Some(DemandCellOverlayMode::FallbackCells),
        _ => None,
    }
}

fn demand_cell_overlay_mode_label(mode: DemandCellOverlayMode) -> &'static str {
    match mode {
        DemandCellOverlayMode::ResidentialAllocation => "residential allocation",
        DemandCellOverlayMode::EmploymentAllocation => "employment allocation",
        DemandCellOverlayMode::TotalAllocation => "total allocation",
        DemandCellOverlayMode::RawResidentialWeight => "raw residential weight",
        DemandCellOverlayMode::RawEmploymentWeight => "raw employment weight",
        DemandCellOverlayMode::FallbackCells => "fallback cells",
    }
}

fn normalized_h3_cell_id(cell_id: &str) -> Option<String> {
    let trimmed = cell_id.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.parse::<CellIndex>().is_ok() {
        return Some(trimmed);
    }
    let suffix = trimmed.rsplit(':').next()?.trim();
    if suffix.parse::<CellIndex>().is_ok() {
        Some(suffix.to_string())
    } else {
        None
    }
}

fn demand_cell_lonlat(scenario: &Scenario, cell: &DemandCell) -> Option<(f64, f64)> {
    if let Some((lon, lat)) = world_xy_to_lonlat_safe(&scenario.meta.crs, cell.x, cell.y) {
        if lon.is_finite() && lat.is_finite() {
            return Some((lon, lat));
        }
    }
    let h3_cell_id = normalized_h3_cell_id(&cell.cell_id)?;
    let index = h3_cell_id.parse::<CellIndex>().ok()?;
    let center: h3o::LatLng = index.into();
    Some((center.lng(), center.lat()))
}

fn normalize_planning_region_id(
    raw_region_id: &str,
    lookup: &DemandRegionLookup,
) -> Option<String> {
    let trimmed = raw_region_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if lookup.unlocked_region_ids.contains(trimmed) {
        return Some(trimmed.to_string());
    }
    if let Some(canonical) = canonicalize_region_id(trimmed) {
        if lookup.unlocked_region_ids.contains(&canonical) {
            return Some(canonical);
        }
    }
    lookup
        .unlocked_region_ids
        .iter()
        .find(|value| value.eq_ignore_ascii_case(trimmed))
        .cloned()
}

fn resolve_demand_cell_region_id(
    cell: &DemandCell,
    scenario: &Scenario,
    lookup: &DemandRegionLookup,
) -> Option<String> {
    if let Some(diag_region_id) = cell
        .allocation_diagnostics
        .as_ref()
        .and_then(|diag| diag.planning_region_id.as_deref())
        .and_then(|raw| normalize_planning_region_id(raw, lookup))
    {
        return Some(diag_region_id);
    }

    if let Some(region_id) = resolve_region_id_for_zone_id(&cell.cell_id, lookup) {
        if lookup.unlocked_region_ids.contains(&region_id) {
            return Some(region_id);
        }
    }

    nearest_unlocked_region_for_world_point(scenario, lookup, cell.x, cell.y)
}

fn build_demand_cell_overlay_payload(
    scenario: &Scenario,
    lookup: &DemandRegionLookup,
    mode: DemandCellOverlayMode,
) -> DemandOverlayPayload {
    let total_cells = scenario.world.demand_cells.len();
    let mut mappable_cells = 0usize;
    let mut fallback_cells = 0usize;
    let mut overlay_cells_fully_inside = 0usize;
    let mut overlay_cells_clipped = 0usize;
    let mut overlay_cells_outside_unlocked = 0usize;
    let mut overlay_duplicate_cell_ids = 0usize;
    let mut overlay_invalid_clipped_geometry_count = 0usize;
    let mut overlay_clipped_geometry_failed_count = 0usize;
    let mut cell_rows = Vec::<DemandOverlayCellDatum>::new();
    let mut seen_cell_ids = HashSet::<String>::new();
    let mut existing_h3_cells = HashSet::<CellIndex>::new();
    let mut rendered_h3_cells = HashSet::<CellIndex>::new();
    let mut rendered_overlay_geometry = None::<MultiPolygon<f64>>;
    let coverage_debug_enabled = demand_overlay_debug_coverage_enabled();
    let mut coverage_debug_failed = false;
    let mut coverage_debug_error = None::<String>;

    for cell in &scenario.world.demand_cells {
        if let Some(h3_cell_id) = normalized_h3_cell_id(&cell.cell_id)
            .and_then(|value| value.parse::<CellIndex>().ok())
            .filter(|cell| cell.resolution() == h3o::Resolution::Eight)
        {
            existing_h3_cells.insert(h3_cell_id);
        }
    }

    for cell in &scenario.world.demand_cells {
        let normalized_cell_key = normalized_h3_cell_id(&cell.cell_id)
            .unwrap_or_else(|| cell.cell_id.trim().to_ascii_lowercase());
        if !normalized_cell_key.is_empty() && !seen_cell_ids.insert(normalized_cell_key) {
            overlay_duplicate_cell_ids += 1;
            continue;
        }
        let Some(geometry_clip) =
            clip_demand_overlay_cell_geometry(&cell.cell_id, lookup.unlocked_geometry.as_ref())
        else {
            overlay_cells_outside_unlocked += 1;
            continue;
        };
        if geometry_clip.invalid_display_geometry {
            overlay_invalid_clipped_geometry_count += 1;
            continue;
        }
        let Some(planning_region_id) = resolve_demand_cell_region_id(cell, scenario, lookup) else {
            continue;
        };
        let Some((lon, lat)) = demand_cell_lonlat(scenario, cell) else {
            continue;
        };
        if !lon.is_finite() || !lat.is_finite() {
            continue;
        }
        mappable_cells += 1;

        let diagnostics = cell.allocation_diagnostics.as_ref();
        let raw_weight_residential = diagnostics
            .and_then(|diag| diag.raw_weight_residential)
            .unwrap_or(0.0)
            .max(0.0);
        let raw_weight_employment = diagnostics
            .and_then(|diag| diag.raw_weight_employment)
            .unwrap_or(0.0)
            .max(0.0);
        let allocated_residential_mass = diagnostics
            .and_then(|diag| diag.allocated_residential_mass)
            .unwrap_or(0.0)
            .max(0.0);
        let allocated_employment_mass = diagnostics
            .and_then(|diag| diag.allocated_employment_mass)
            .unwrap_or(0.0)
            .max(0.0);
        let fallback_reason = diagnostics
            .and_then(|diag| diag.fallback_reason.as_ref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        if fallback_reason.is_some() {
            fallback_cells += 1;
        }
        if geometry_clip.clipped {
            overlay_cells_clipped += 1;
        } else if geometry_clip.fully_inside {
            overlay_cells_fully_inside += 1;
        }
        if geometry_clip.clip_failed {
            overlay_clipped_geometry_failed_count += 1;
        }
        if let Some(h3_cell_id) = normalized_h3_cell_id(&cell.cell_id)
            .and_then(|value| value.parse::<CellIndex>().ok())
            .filter(|cell| cell.resolution() == h3o::Resolution::Eight)
        {
            rendered_h3_cells.insert(h3_cell_id);
        }
        if coverage_debug_enabled {
            if let Some(rendered_geometry) = geometry_clip.rendered_geometry.as_ref() {
                // Exact rendered-union coverage diagnostics are useful for QA,
                // but geo boolean union can panic on pathological slivers. Keep
                // it out of the normal overlay path.
                rendered_overlay_geometry = match rendered_overlay_geometry.take() {
                    Some(existing) if !coverage_debug_failed => match safe_geometry_union(
                        &existing,
                        rendered_geometry,
                        "demand overlay rendered coverage union",
                    ) {
                        Ok(merged) => Some(merged),
                        Err(error) => {
                            coverage_debug_failed = true;
                            coverage_debug_error = Some(error);
                            Some(existing)
                        }
                    },
                    Some(existing) => Some(existing),
                    None => Some(rendered_geometry.clone()),
                };
            }
        }

        cell_rows.push(DemandOverlayCellDatum {
            cell_id: cell.cell_id.clone(),
            planning_region_id: Some(planning_region_id),
            display_geometry: geometry_clip.display_geometry,
            display_geometry_clipped: geometry_clip.clipped,
            lon,
            lat,
            area_m2: cell.area_m2.max(0.0),
            residents_night: cell.residents_night.max(0.0),
            jobs_day: cell.jobs_day.max(0.0),
            centrality_score: cell.centrality_score.max(0.0),
            data_quality_score: cell.data_quality_score.max(0.0),
            activity_mix_residential: cell.activity_mix_residential.max(0.0),
            activity_mix_office: cell.activity_mix_office.max(0.0),
            activity_mix_retail: cell.activity_mix_retail.max(0.0),
            activity_mix_recreation: cell.activity_mix_recreation.max(0.0),
            activity_mix_industrial: cell.activity_mix_industrial.max(0.0),
            activity_mix_education: cell.activity_mix_education.max(0.0),
            activity_mix_health: cell.activity_mix_health.max(0.0),
            raw_weight_residential,
            raw_weight_employment,
            allocated_residential_mass,
            allocated_employment_mass,
            fallback_reason,
        });
    }

    cell_rows.sort_by(|left, right| left.cell_id.cmp(&right.cell_id));

    let available = if matches!(mode, DemandCellOverlayMode::FallbackCells) {
        fallback_cells > 0
    } else {
        !cell_rows.is_empty()
    };

    let reason = if available {
        None
    } else if total_cells == 0 {
        Some("No demand cells are materialized in this scenario.".to_string())
    } else if mappable_cells == 0 {
        Some(
            "Demand cells exist but none map to unlocked planning regions with renderable geometry."
                .to_string(),
        )
    } else if matches!(mode, DemandCellOverlayMode::FallbackCells) {
        Some("No demand cells currently use allocation fallback behavior.".to_string())
    } else {
        Some(format!(
            "No mappable demand cells were available for {}.",
            demand_cell_overlay_mode_label(mode)
        ))
    };

    let mut by_region = HashMap::<String, usize>::new();
    for row in &cell_rows {
        if let Some(region_id) = row.planning_region_id.as_ref() {
            *by_region.entry(region_id.clone()).or_insert(0) += 1;
        }
    }
    let mut by_region_rows = by_region
        .into_iter()
        .map(|(region_id, count)| format!("{region_id}:{count}"))
        .collect::<Vec<_>>();
    by_region_rows.sort();
    let sample_cells = cell_rows
        .iter()
        .take(5)
        .map(|row| row.cell_id.clone())
        .collect::<Vec<_>>();
    let mut unlocked_geometry_region_ids = lookup.unlocked_geometry_region_ids.clone();
    unlocked_geometry_region_ids.sort();
    let mut unlocked_geometry_missing_region_ids =
        lookup.unlocked_geometry_missing_region_ids.clone();
    unlocked_geometry_missing_region_ids.sort();
    let coverage = if coverage_debug_failed {
        DemandOverlayCoverageDiagnostics {
            debug_enabled: true,
            failed: true,
            error: coverage_debug_error,
            unlocked_union_area: lookup
                .unlocked_geometry
                .as_ref()
                .map(|geometry| geometry.unsigned_area().max(0.0))
                .unwrap_or(0.0),
            ..DemandOverlayCoverageDiagnostics::default()
        }
    } else {
        demand_overlay_coverage_diagnostics(
            coverage_debug_enabled,
            lookup.unlocked_geometry.as_ref(),
            rendered_overlay_geometry.as_ref(),
            &existing_h3_cells,
            &rendered_h3_cells,
        )
    };
    eprintln!(
        "[demand-overlay-cell] mode={} coverage_debug_enabled={} coverage_debug_failed={} coverage_debug_error={} unlocked_regions={} geometry_regions={} explicit_geometry_regions={} h3_fallback_regions={} geometry_missing={} total_cells={} mappable_cells={} payload_rows={} fallback_cells={} unlocked_geometry={} fully_inside={} clipped={} outside_unlocked={} duplicate_cell_ids={} invalid_clipped={} clipped_failed={} unlocked_area={:.9} rendered_area={:.9} uncovered_area={:.9} uncovered_ratio={:.6} outside_area={:.9} outside_ratio={:.6} expected_cells={} existing_cells={} missing_cells={} filtered_cells={} geometry_region_ids={} missing_geometry_region_ids={} by_region={} sample_cells={}",
        demand_cell_overlay_mode_label(mode),
        coverage.debug_enabled,
        coverage.failed,
        coverage.error.as_deref().unwrap_or(""),
        lookup.unlocked_region_ids.len(),
        lookup.unlocked_geometry_region_ids.len(),
        lookup.explicit_geometry_region_count,
        lookup.h3_fallback_geometry_region_count,
        lookup.unlocked_geometry_missing_region_ids.len(),
        total_cells,
        mappable_cells,
        cell_rows.len(),
        fallback_cells,
        lookup.unlocked_geometry.is_some(),
        overlay_cells_fully_inside,
        overlay_cells_clipped,
        overlay_cells_outside_unlocked,
        overlay_duplicate_cell_ids,
        overlay_invalid_clipped_geometry_count,
        overlay_clipped_geometry_failed_count,
        coverage.unlocked_union_area,
        coverage.rendered_union_area,
        coverage.uncovered_unlocked_area,
        coverage.uncovered_ratio,
        coverage.outside_rendered_area,
        coverage.outside_ratio,
        coverage.expected_intersecting_cell_count,
        coverage.existing_intersecting_cell_count,
        coverage.missing_intersecting_cell_count,
        coverage.filtered_intersecting_cell_count,
        unlocked_geometry_region_ids.join("|"),
        unlocked_geometry_missing_region_ids.join("|"),
        by_region_rows.join(","),
        sample_cells.join("|"),
    );

    DemandOverlayPayload {
        available,
        reason,
        intensity_available: false,
        intensity_reason: None,
        service_gap_available: false,
        service_gap_reason: None,
        corridor_desire_available: false,
        corridor_desire_reason: None,
        run_id: None,
        cell_data_total: total_cells,
        cell_data_mappable: mappable_cells,
        cell_fallback_count: fallback_cells,
        overlay_unlocked_region_count: lookup.unlocked_region_ids.len(),
        overlay_unlocked_geometry_region_count: lookup.unlocked_geometry_region_ids.len(),
        overlay_unlocked_geometry_missing_region_count: lookup
            .unlocked_geometry_missing_region_ids
            .len(),
        overlay_unlocked_geometry_available: lookup.unlocked_geometry.is_some(),
        overlay_explicit_geometry_region_count: lookup.explicit_geometry_region_count,
        overlay_h3_fallback_region_count: lookup.h3_fallback_geometry_region_count,
        overlay_unlocked_union_area: coverage.unlocked_union_area,
        overlay_rendered_union_area: coverage.rendered_union_area,
        overlay_uncovered_unlocked_area: coverage.uncovered_unlocked_area,
        overlay_uncovered_ratio: coverage.uncovered_ratio,
        overlay_outside_rendered_area: coverage.outside_rendered_area,
        overlay_outside_ratio: coverage.outside_ratio,
        overlay_expected_intersecting_cell_count: coverage.expected_intersecting_cell_count,
        overlay_existing_intersecting_cell_count: coverage.existing_intersecting_cell_count,
        overlay_missing_intersecting_cell_count: coverage.missing_intersecting_cell_count,
        overlay_filtered_intersecting_cell_count: coverage.filtered_intersecting_cell_count,
        overlay_invalid_clipped_geometry_count,
        overlay_clipped_geometry_failed_count,
        overlay_coverage_debug_enabled: coverage.debug_enabled,
        overlay_coverage_debug_failed: coverage.failed,
        overlay_coverage_debug_error: coverage.error.clone(),
        overlay_cells_fully_inside,
        overlay_cells_clipped,
        overlay_cells_outside_unlocked,
        overlay_duplicate_cell_ids,
        cell_data: cell_rows,
        region_data: Vec::new(),
        corridor_data: Vec::new(),
    }
}

pub(crate) fn get_demand_overlay_payload(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    overlay_type: Option<String>,
) -> Result<DemandOverlayPayload, String> {
    let started = Instant::now();
    let requested_overlay = parse_requested_demand_overlay(overlay_type);
    let include_intensity = matches!(
        requested_overlay,
        RequestedDemandOverlay::All | RequestedDemandOverlay::Intensity
    );
    let include_service_gap = matches!(
        requested_overlay,
        RequestedDemandOverlay::All | RequestedDemandOverlay::ServiceGap
    );
    let include_corridor = matches!(
        requested_overlay,
        RequestedDemandOverlay::All | RequestedDemandOverlay::CorridorDesire
    );

    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let project_key = project_root.to_string_lossy().to_string();
    let runtime_loop_active =
        runtime_loop_matches_project(state.inner(), &project_key).unwrap_or(false);
    let lookup = build_demand_region_lookup(&app, &manifest)?;
    if let Some(cell_overlay_mode) = requested_cell_overlay_mode(requested_overlay) {
        let payload = build_demand_cell_overlay_payload(&doc.scenario, &lookup, cell_overlay_mode);
        eprintln!(
            "[rt-overlay] mode=cell project={} runtime_loop_active={} elapsed_ms={:.2} cell_data_total={} cell_data_mappable={} payload_rows={} fallback_cells={} region_rows={} corridor_rows={} available={} reason={}",
            project_key,
            runtime_loop_active,
            started.elapsed().as_secs_f64() * 1000.0,
            payload.cell_data_total,
            payload.cell_data_mappable,
            payload.cell_data.len(),
            payload.cell_fallback_count,
            payload.region_data.len(),
            payload.corridor_data.len(),
            payload.available,
            payload.reason.as_deref().unwrap_or("none"),
        );
        return Ok(payload);
    }
    let (intensity_by_region, mapped_intensity_samples) = if include_intensity {
        build_world_intensity_by_region(&doc.scenario, &lookup)
    } else {
        (HashMap::new(), 0usize)
    };

    let mut region_rows = Vec::<DemandOverlayRegionDatum>::new();
    for region_id in &lookup.unlocked_region_ids {
        let Some((lon, lat)) = lookup.region_lonlat_by_id.get(region_id).copied() else {
            continue;
        };
        region_rows.push(DemandOverlayRegionDatum {
            region_id: region_id.clone(),
            region_name: lookup
                .region_name_by_id
                .get(region_id)
                .cloned()
                .unwrap_or_else(|| region_id.clone()),
            lon,
            lat,
            intensity_score: if include_intensity {
                intensity_by_region
                    .get(region_id)
                    .copied()
                    .unwrap_or(0.0)
                    .max(0.0)
            } else {
                0.0
            },
            service_gap_score: 0.0,
            service_gap_ratio: 0.0,
        });
    }
    region_rows.sort_by(|a, b| a.region_id.cmp(&b.region_id));

    let intensity_available =
        include_intensity && !region_rows.is_empty() && mapped_intensity_samples > 0;
    let intensity_reason = if !include_intensity || intensity_available {
        None
    } else if region_rows.is_empty() {
        Some("No unlocked planning regions are available for demand intensity.".to_string())
    } else if !doc.scenario.world.demand_cells.is_empty() || !doc.scenario.world.zones.is_empty() {
        Some(
            "Demand substrate exists but could not be mapped to unlocked planning regions."
                .to_string(),
        )
    } else {
        Some("No demand substrate found in scenario world data.".to_string())
    };

    let zone_xy_by_id = if include_service_gap || include_corridor {
        zone_world_xy_index(&doc.scenario)
    } else {
        HashMap::new()
    };
    let strategic_layers = if include_service_gap || include_corridor {
        resolve_strategic_overlay_layers(state.inner(), &project_root, &manifest, &project_key)
    } else {
        None
    };
    let run_id = strategic_layers
        .as_ref()
        .and_then(|layers| layers.run_id.clone());

    let region_index_by_id = region_rows
        .iter()
        .enumerate()
        .map(|(idx, row)| (row.region_id.clone(), idx))
        .collect::<HashMap<_, _>>();

    let mut service_gap_available = !include_service_gap;
    let mut service_gap_reason = None::<String>;
    let mut service_gap_ratio_weight = HashMap::<String, f64>::new();
    if include_service_gap {
        if let Some(source) = strategic_layers.as_ref() {
            let mut mapped_rows = 0usize;
            for gap in &source.service_gap_layer {
                let Some(region_id) = resolve_region_id_for_zone_with_fallback(
                    &gap.zone_id,
                    &lookup,
                    &doc.scenario,
                    &zone_xy_by_id,
                ) else {
                    continue;
                };
                let Some(idx) = region_index_by_id.get(&region_id).copied() else {
                    continue;
                };
                let Some(row) = region_rows.get_mut(idx) else {
                    continue;
                };
                let score = gap.total_unserved_demand.max(0.0);
                let ratio = gap.latent_vs_realised_ratio.max(0.0);
                row.service_gap_score += score;
                row.service_gap_ratio += ratio * score.max(1.0);
                *service_gap_ratio_weight.entry(region_id).or_insert(0.0) += score.max(1.0);
                mapped_rows += 1;
            }
            for row in &mut region_rows {
                let weight = service_gap_ratio_weight
                    .get(&row.region_id)
                    .copied()
                    .unwrap_or(0.0);
                row.service_gap_ratio = if weight > 0.0 {
                    row.service_gap_ratio / weight
                } else {
                    0.0
                };
            }
            service_gap_available = mapped_rows > 0;
            if !service_gap_available {
                service_gap_reason = Some(format!(
                    "{} did not contain mappable service gap rows.",
                    if source.source_kind == "runtime" {
                        "Latest strategic runtime refresh"
                    } else {
                        "Latest planning run"
                    }
                ));
            }
        } else {
            service_gap_reason = Some(if runtime_loop_active {
                "Service gap will appear after the first strategic runtime refresh.".to_string()
            } else {
                "Run planning or start runtime simulation to compute service gap analysis."
                    .to_string()
            });
        }
    }

    let mut corridor_data = Vec::<DemandOverlayCorridorDatum>::new();
    if include_corridor {
        let mut corridor_by_pair = HashMap::<(String, String), DemandOverlayCorridorDatum>::new();
        if let Some(source) = strategic_layers.as_ref() {
            for corridor in &source.corridor_desire_lines {
                let Some(origin_region_id) = resolve_region_id_for_zone_with_fallback(
                    &corridor.origin_zone_id,
                    &lookup,
                    &doc.scenario,
                    &zone_xy_by_id,
                ) else {
                    continue;
                };
                let Some(destination_region_id) = resolve_region_id_for_zone_with_fallback(
                    &corridor.destination_zone_id,
                    &lookup,
                    &doc.scenario,
                    &zone_xy_by_id,
                ) else {
                    continue;
                };
                if origin_region_id == destination_region_id {
                    continue;
                }
                let (origin_region_id, destination_region_id) =
                    if origin_region_id <= destination_region_id {
                        (origin_region_id, destination_region_id)
                    } else {
                        (destination_region_id, origin_region_id)
                    };
                let Some((origin_lon, origin_lat)) =
                    lookup.region_lonlat_by_id.get(&origin_region_id).copied()
                else {
                    continue;
                };
                let Some((destination_lon, destination_lat)) = lookup
                    .region_lonlat_by_id
                    .get(&destination_region_id)
                    .copied()
                else {
                    continue;
                };
                let key = (origin_region_id.clone(), destination_region_id.clone());
                let row =
                    corridor_by_pair
                        .entry(key)
                        .or_insert_with(|| DemandOverlayCorridorDatum {
                            origin_region_id,
                            destination_region_id,
                            origin_lon,
                            origin_lat,
                            destination_lon,
                            destination_lat,
                            corridor_score: 0.0,
                            latent_passengers: 0.0,
                            realised_passengers: 0.0,
                            unserved_passengers: 0.0,
                            is_underserved: false,
                        });
                row.corridor_score += corridor.corridor_score.max(0.0);
                row.latent_passengers += corridor.latent_passengers.max(0.0);
                row.realised_passengers += corridor.realised_passengers.max(0.0);
                row.unserved_passengers += corridor.unserved_passengers.max(0.0);
                row.is_underserved = row.is_underserved || corridor.is_underserved;
            }
        }
        corridor_data = corridor_by_pair.into_values().collect::<Vec<_>>();
        corridor_data.sort_by(|a, b| {
            b.corridor_score
                .partial_cmp(&a.corridor_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.origin_region_id.cmp(&b.origin_region_id))
                .then_with(|| a.destination_region_id.cmp(&b.destination_region_id))
        });
    }

    let corridor_desire_available = include_corridor && !corridor_data.is_empty();
    let corridor_desire_reason = if !include_corridor || corridor_desire_available {
        None
    } else if let Some(source) = strategic_layers.as_ref() {
        Some(format!(
            "{} did not contain mappable corridor desire rows.",
            if source.source_kind == "runtime" {
                "Latest strategic runtime refresh"
            } else {
                "Latest planning run"
            }
        ))
    } else if runtime_loop_active {
        Some("Corridor desire will appear after the first strategic runtime refresh.".to_string())
    } else {
        Some(
            "Run planning or start runtime simulation to compute corridor desire analysis."
                .to_string(),
        )
    };

    if !include_corridor {
        corridor_data.clear();
    }

    let available = intensity_available || service_gap_available || corridor_desire_available;

    let payload = DemandOverlayPayload {
        available,
        reason: if available {
            None
        } else {
            Some("No demand overlay data is currently mappable for this save.".to_string())
        },
        intensity_available,
        intensity_reason,
        service_gap_available,
        service_gap_reason,
        corridor_desire_available,
        corridor_desire_reason,
        run_id,
        cell_data_total: 0,
        cell_data_mappable: 0,
        cell_fallback_count: 0,
        overlay_unlocked_region_count: lookup.unlocked_region_ids.len(),
        overlay_unlocked_geometry_region_count: lookup.unlocked_geometry_region_ids.len(),
        overlay_unlocked_geometry_missing_region_count: lookup
            .unlocked_geometry_missing_region_ids
            .len(),
        overlay_unlocked_geometry_available: lookup.unlocked_geometry.is_some(),
        overlay_explicit_geometry_region_count: lookup.explicit_geometry_region_count,
        overlay_h3_fallback_region_count: lookup.h3_fallback_geometry_region_count,
        overlay_unlocked_union_area: lookup
            .unlocked_geometry
            .as_ref()
            .map(|geometry| geometry.unsigned_area().max(0.0))
            .unwrap_or(0.0),
        overlay_rendered_union_area: 0.0,
        overlay_uncovered_unlocked_area: 0.0,
        overlay_uncovered_ratio: 0.0,
        overlay_outside_rendered_area: 0.0,
        overlay_outside_ratio: 0.0,
        overlay_expected_intersecting_cell_count: 0,
        overlay_existing_intersecting_cell_count: 0,
        overlay_missing_intersecting_cell_count: 0,
        overlay_filtered_intersecting_cell_count: 0,
        overlay_invalid_clipped_geometry_count: 0,
        overlay_clipped_geometry_failed_count: 0,
        overlay_coverage_debug_enabled: false,
        overlay_coverage_debug_failed: false,
        overlay_coverage_debug_error: None,
        overlay_cells_fully_inside: 0,
        overlay_cells_clipped: 0,
        overlay_cells_outside_unlocked: 0,
        overlay_duplicate_cell_ids: 0,
        cell_data: Vec::new(),
        region_data: region_rows,
        corridor_data,
    };
    eprintln!(
        "[rt-overlay] mode=regional project={} runtime_loop_active={} elapsed_ms={:.2} intensity_available={} service_gap_available={} corridor_desire_available={} region_rows={} corridor_rows={} run_id={} available={} reason={}",
        project_key,
        runtime_loop_active,
        started.elapsed().as_secs_f64() * 1000.0,
        payload.intensity_available,
        payload.service_gap_available,
        payload.corridor_desire_available,
        payload.region_data.len(),
        payload.corridor_data.len(),
        payload.run_id.as_deref().unwrap_or("none"),
        payload.available,
        payload.reason.as_deref().unwrap_or("none"),
    );
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use h3o::Resolution;

    fn rectangle(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                Coord { x: min_x, y: min_y },
                Coord { x: max_x, y: min_y },
                Coord { x: max_x, y: max_y },
                Coord { x: min_x, y: max_y },
                Coord { x: min_x, y: min_y },
            ]),
            vec![],
        )
    }

    fn test_cell() -> CellIndex {
        h3o::LatLng::new(53.4808, -2.2426)
            .expect("valid lon/lat")
            .to_cell(Resolution::Eight)
    }

    fn cell_bbox(cell: CellIndex) -> (f64, f64, f64, f64) {
        let polygon = h3_cell_boundary_polygon(cell).expect("cell polygon");
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for point in polygon.exterior().points() {
            min_x = min_x.min(point.x());
            min_y = min_y.min(point.y());
            max_x = max_x.max(point.x());
            max_y = max_y.max(point.y());
        }
        (min_x, min_y, max_x, max_y)
    }

    fn test_region_for_cell(region_id: &str, cell: CellIndex) -> SurfaceRegionInfo {
        let center: h3o::LatLng = cell.into();
        let (x, y) = lonlat_to_web_mercator_m(center.lng(), center.lat());
        SurfaceRegionInfo {
            region_id: region_id.to_string(),
            country_iso2: "UK".to_string(),
            region_kind: "planning_hex_unassigned".to_string(),
            region_token: cell.to_string(),
            h3_cell_id: Some(cell.to_string()),
            name: region_id.to_string(),
            admin_level: "planning_hex_res8_test".to_string(),
            nation: None,
            source_code: Some("test".to_string()),
            adjacency_source: "test".to_string(),
            geometry_source: "planning_surface_h3_fallback".to_string(),
            cell_id: cell.to_string(),
            x,
            y,
            area_m2: 1.0,
            residents_smooth: 1.0,
            jobs_smooth: 1.0,
            activity_mix_residential: 1.0,
            activity_mix_office: 0.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            adjacent_region_ids: vec![],
            geometry: None,
            canonical_hex_number: None,
            constituent_hex_numbers: vec![],
        }
    }

    fn display_geometry_multipolygon(value: &JsonValue) -> MultiPolygon<f64> {
        geometry_to_multipolygon(
            geojson_value_to_geometry(value, "test display geometry").expect("valid geometry"),
        )
        .expect("polygonal display geometry")
    }

    #[test]
    fn demand_overlay_geometry_clips_boundary_cell_to_unlocked_polygon() {
        let cell = test_cell();
        let (min_x, min_y, max_x, max_y) = cell_bbox(cell);
        let mid_x = (min_x + max_x) * 0.5;
        let unlocked = MultiPolygon(vec![rectangle(
            min_x - 0.001,
            min_y - 0.001,
            mid_x,
            max_y + 0.001,
        )]);

        let clipped = clip_demand_overlay_cell_geometry(&cell.to_string(), Some(&unlocked))
            .expect("intersecting boundary cell should be included");

        assert!(clipped.clipped);
        assert!(!clipped.fully_inside);
        assert!(
            clipped.display_geometry.is_some(),
            "boundary cells should carry clipped display geometry"
        );
        let rendered = display_geometry_multipolygon(
            clipped
                .display_geometry
                .as_ref()
                .expect("clipped display geometry"),
        );
        assert!(
            rendered.difference(&unlocked).unsigned_area() <= 1e-18,
            "clipped overlay display geometry must not extend outside the unlocked geometry"
        );
    }

    #[test]
    fn demand_overlay_geometry_omits_fully_outside_cell() {
        let cell = test_cell();
        let (_, _, max_x, max_y) = cell_bbox(cell);
        let unlocked = MultiPolygon(vec![rectangle(
            max_x + 0.01,
            max_y + 0.01,
            max_x + 0.02,
            max_y + 0.02,
        )]);

        assert!(clip_demand_overlay_cell_geometry(&cell.to_string(), Some(&unlocked)).is_none());
    }

    #[test]
    fn demand_overlay_geometry_keeps_full_cell_inside_unlocked_polygon() {
        let cell = test_cell();
        let (min_x, min_y, max_x, max_y) = cell_bbox(cell);
        let unlocked = MultiPolygon(vec![rectangle(
            min_x - 0.001,
            min_y - 0.001,
            max_x + 0.001,
            max_y + 0.001,
        )]);

        let result = clip_demand_overlay_cell_geometry(&cell.to_string(), Some(&unlocked))
            .expect("inside cell should be included");

        assert!(result.fully_inside);
        assert!(!result.clipped);
        assert!(
            result.display_geometry.is_none(),
            "inside cells can render from the H3 id without payload bloat"
        );
    }

    #[test]
    fn adjacent_unlocked_polygons_are_merged_before_overlay_clipping() {
        let cell = test_cell();
        let (min_x, min_y, max_x, max_y) = cell_bbox(cell);
        let mid_x = (min_x + max_x) * 0.5;
        let mut unlocked = None::<MultiPolygon<f64>>;
        merge_unlocked_geometry(
            &mut unlocked,
            MultiPolygon(vec![rectangle(
                min_x - 0.001,
                min_y - 0.001,
                mid_x,
                max_y + 0.001,
            )]),
        );
        merge_unlocked_geometry(
            &mut unlocked,
            MultiPolygon(vec![rectangle(
                mid_x,
                min_y - 0.001,
                max_x + 0.001,
                max_y + 0.001,
            )]),
        );

        let result = clip_demand_overlay_cell_geometry(&cell.to_string(), unlocked.as_ref())
            .expect("cell spanning adjacent unlocked regions should be included once");

        assert!(result.fully_inside);
        assert!(!result.clipped);
    }

    #[test]
    fn h3_backed_unlocked_regions_contribute_to_merged_overlay_geometry() {
        let first_cell = test_cell();
        let second_cell = h3o::LatLng::new(53.6208, -2.2426)
            .expect("valid lon/lat")
            .to_cell(Resolution::Eight);
        let (min_x, min_y, max_x, max_y) = cell_bbox(first_cell);
        let mut unlocked = None::<MultiPolygon<f64>>;
        merge_unlocked_geometry(
            &mut unlocked,
            MultiPolygon(vec![rectangle(
                min_x - 0.001,
                min_y - 0.001,
                max_x + 0.001,
                max_y + 0.001,
            )]),
        );
        let h3_backed_region = test_region_for_cell("r6:UK:test-north", second_cell);
        merge_unlocked_geometry(
            &mut unlocked,
            region_overlay_geometry_multipolygon(&h3_backed_region)
                .expect("valid region geometry")
                .expect("H3-backed region geometry fallback")
                .geometry,
        );

        let first = clip_demand_overlay_cell_geometry(&first_cell.to_string(), unlocked.as_ref())
            .expect("initial region cell should still be included");
        let second = clip_demand_overlay_cell_geometry(&second_cell.to_string(), unlocked.as_ref())
            .expect("newly unlocked H3-backed adjacent region cell should be included");

        assert!(first.fully_inside);
        assert!(second.fully_inside);
    }

    #[test]
    fn overlay_coverage_diagnostics_are_debug_gated() {
        let first_cell = test_cell();
        let second_cell = h3o::LatLng::new(53.6208, -2.2426)
            .expect("valid lon/lat")
            .to_cell(Resolution::Eight);
        let mut unlocked = None::<MultiPolygon<f64>>;
        let first_geometry = h3_cell_id_multipolygon(&first_cell.to_string()).expect("first cell");
        let second_geometry =
            h3_cell_id_multipolygon(&second_cell.to_string()).expect("second cell");
        merge_unlocked_geometry(&mut unlocked, first_geometry.clone());
        merge_unlocked_geometry(&mut unlocked, second_geometry);
        let unlocked = unlocked.expect("unlocked union");
        let mut existing = HashSet::<CellIndex>::new();
        let mut rendered_cells = HashSet::<CellIndex>::new();
        existing.insert(first_cell);
        rendered_cells.insert(first_cell);

        let coverage = demand_overlay_coverage_diagnostics(
            false,
            Some(&unlocked),
            Some(&first_geometry),
            &existing,
            &rendered_cells,
        );

        assert!(!coverage.debug_enabled);
        assert!(!coverage.failed);
        assert_eq!(coverage.uncovered_unlocked_area, 0.0);
        assert_eq!(coverage.missing_intersecting_cell_count, 0);
    }

    #[test]
    fn overlay_coverage_diagnostics_detect_missing_intersecting_cells() {
        let first_cell = test_cell();
        let second_cell = h3o::LatLng::new(53.6208, -2.2426)
            .expect("valid lon/lat")
            .to_cell(Resolution::Eight);
        let mut unlocked = None::<MultiPolygon<f64>>;
        let first_geometry = h3_cell_id_multipolygon(&first_cell.to_string()).expect("first cell");
        let second_geometry =
            h3_cell_id_multipolygon(&second_cell.to_string()).expect("second cell");
        merge_unlocked_geometry(&mut unlocked, first_geometry.clone());
        merge_unlocked_geometry(&mut unlocked, second_geometry.clone());
        let unlocked = unlocked.expect("unlocked union");
        let mut existing = HashSet::<CellIndex>::new();
        let mut rendered_cells = HashSet::<CellIndex>::new();
        existing.insert(first_cell);
        rendered_cells.insert(first_cell);

        let coverage = demand_overlay_coverage_diagnostics(
            true,
            Some(&unlocked),
            Some(&first_geometry),
            &existing,
            &rendered_cells,
        );

        assert!(coverage.uncovered_unlocked_area > 0.0);
        assert!(coverage.uncovered_ratio > 0.0);
        assert!(
            coverage.missing_intersecting_cell_count > 0,
            "coverage audit should identify H3 cells intersecting unlocked geometry but absent from world.demand_cells"
        );
    }

    #[test]
    fn overlay_coverage_diagnostics_accept_complete_adjacent_cell_coverage() {
        let first_cell = test_cell();
        let second_cell = h3o::LatLng::new(53.6208, -2.2426)
            .expect("valid lon/lat")
            .to_cell(Resolution::Eight);
        let mut unlocked = None::<MultiPolygon<f64>>;
        let first_geometry = h3_cell_id_multipolygon(&first_cell.to_string()).expect("first cell");
        let second_geometry =
            h3_cell_id_multipolygon(&second_cell.to_string()).expect("second cell");
        merge_unlocked_geometry(&mut unlocked, first_geometry.clone());
        merge_unlocked_geometry(&mut unlocked, second_geometry.clone());
        let unlocked = unlocked.expect("unlocked union");
        let mut rendered = None::<MultiPolygon<f64>>;
        merge_unlocked_geometry(&mut rendered, first_geometry);
        merge_unlocked_geometry(&mut rendered, second_geometry);
        let rendered = rendered.expect("rendered union");
        let mut existing = HashSet::<CellIndex>::new();
        let mut rendered_cells = HashSet::<CellIndex>::new();
        existing.insert(first_cell);
        existing.insert(second_cell);
        rendered_cells.insert(first_cell);
        rendered_cells.insert(second_cell);

        let coverage = demand_overlay_coverage_diagnostics(
            true,
            Some(&unlocked),
            Some(&rendered),
            &existing,
            &rendered_cells,
        );

        assert!(coverage.uncovered_ratio <= 1e-9);
        assert_eq!(coverage.missing_intersecting_cell_count, 0);
        assert_eq!(coverage.filtered_intersecting_cell_count, 0);
    }
}
