use crate::*;
use crate::commands::build_mutation::world_xy_to_lonlat_safe;
use crate::commands::content_library::{demand_surface_file, primary_project_country_iso2};
use tauri::AppHandle;

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
                    .map(|v| v.eq_ignore_ascii_case(&iso))
                    .unwrap_or(false)
            })
            .count();
        out.push(DemandCoverageMeta {
            country_iso2: iso.clone(),
            installed: demand_surface_file(&app, &iso).is_some(),
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
    _app: AppHandle,
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
    let gb_counties = if project_country_iso2.eq_ignore_ascii_case("GB") {
        load_gb_county_boundaries()
            .ok()
            .map(|catalog| catalog.counties)
    } else {
        None
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
        if let Some(counties) = gb_counties.as_ref() {
            let mut counts = HashMap::<String, usize>::new();
            for station in &inspection.stations {
                let Some((lon, lat)) =
                    world_xy_to_lonlat_safe(&doc.scenario.meta.crs, station.x, station.y)
                else {
                    continue;
                };
                let county = county_for_lon_lat(counties, lon, lat)
                    .or_else(|| nearest_county_for_lon_lat(counties, lon, lat));
                let Some(county) = county else { continue };
                let key = canonicalize_region_id(&region_id_from_county("GB", &county.county_id))
                    .unwrap_or_else(|| region_id_from_county("GB", &county.county_id));
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

pub(crate) fn list_regions(app: AppHandle, project_path: String) -> Result<Vec<RegionStatus>, String> {
    let project_root = PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    region_status_rows_for_manifest(&app, &manifest)
}

pub(crate) fn unlock_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<UnlockResult, String> {
    let normalized_region =
        canonicalize_region_id(&region_id).ok_or_else(|| "invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!("no installed demand pack for country {iso}"));
    };
    let region = catalog
        .by_id
        .get(&normalized_region)
        .ok_or_else(|| format!("unknown region_id: {normalized_region}"))?;

    let project_root = PathBuf::from(&project_path);
    let mut manifest = read_manifest(&project_root)?;
    let mut unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let country_unlocked = unlocked
        .iter()
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .cloned()
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region)
        && !country_unlocked.is_empty()
        && !region
            .adjacent_region_ids
            .iter()
            .any(|rid| country_unlocked.contains(rid))
    {
        return Err("region must be adjacent to an already unlocked region".to_string());
    }

    let charge = if unlocked.contains(&normalized_region) {
        0.0
    } else {
        region_unlock_cost_base_for_manifest(&manifest, region)
    };
    if charge > 0.0 && manifest.economy.current_balance_base < charge {
        return Err(format!(
            "insufficient funds: need {:.0} base units, have {:.0}",
            charge, manifest.economy.current_balance_base
        ));
    }

    if charge > 0.0 {
        manifest.economy.current_balance_base -= charge;
        manifest.economy.cumulative_capex_base += charge;
        update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, charge);
        record_monthly_financial_delta(&mut manifest, 0.0, 0.0, charge, 0.0);
        bump_economy_revision(&mut manifest);
    }
    unlocked.insert(normalized_region.clone());
    manifest.region_state.unlocked_region_ids = unlocked.into_iter().collect();
    if manifest
        .region_state
        .primary_focus_region_id
        .as_deref()
        .and_then(canonicalize_region_id)
        .is_none()
    {
        manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    }

    let mut countries = unlocked_country_codes(&manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    sync_progress_budget_from_economy(&mut manifest);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;

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
    let region = catalog
        .by_id
        .get(&normalized_region)
        .ok_or_else(|| format!("UnknownRegion: unknown region_id: {normalized_region}"))?;

    let mut unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let country_unlocked = unlocked
        .iter()
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .cloned()
        .collect::<HashSet<_>>();
    if !unlocked.contains(&normalized_region)
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

    let charge = if unlocked.contains(&normalized_region) {
        0.0
    } else {
        region_unlock_cost_base_for_manifest(&manifest, region)
    };
    if charge > 0.0 && manifest.economy.current_balance_base < charge {
        return Err(format!(
            "InsufficientFunds: need {:.0} base units, have {:.0}",
            charge, manifest.economy.current_balance_base
        ));
    }

    if charge > 0.0 {
        manifest.economy.current_balance_base -= charge;
        manifest.economy.cumulative_capex_base += charge;
        update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, charge);
        record_monthly_financial_delta(&mut manifest, 0.0, 0.0, charge, 0.0);
        bump_economy_revision(&mut manifest);
    }
    unlocked.insert(normalized_region.clone());
    manifest.region_state.unlocked_region_ids = unlocked.iter().cloned().collect();
    manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    manifest.region_state.active_region_ids =
        default_active_regions_for_focus(&catalog, &normalized_region, &unlocked);

    let mut countries = unlocked_country_codes(&manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    sync_progress_budget_from_economy(&mut manifest);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;

    Ok(UnlockFocusResult {
        region_id: normalized_region.clone(),
        charged_base: charge,
        current_balance_base: manifest.economy.current_balance_base,
        unlocked_regions: manifest.region_state.unlocked_region_ids.len(),
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
    })
}

pub(crate) fn set_primary_focus_region(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    region_id: String,
) -> Result<FocusResult, String> {
    let normalized_region =
        canonicalize_region_id(&region_id).ok_or_else(|| "invalid region_id format".to_string())?;
    let iso = region_country_iso2(&normalized_region)
        .ok_or_else(|| "invalid region country code".to_string())?;
    let Some(catalog) = load_region_catalog_for_country(&app, &iso)? else {
        return Err(format!("no installed demand pack for country {iso}"));
    };
    if !catalog.by_id.contains_key(&normalized_region) {
        return Err(format!("unknown region_id: {normalized_region}"));
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
        return Err("region must be unlocked before setting focus".to_string());
    }
    manifest.region_state.primary_focus_region_id = Some(normalized_region.clone());
    manifest.region_state.active_region_ids =
        default_active_regions_for_focus(&catalog, &normalized_region, &unlocked);

    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let _ = open_session_internal(&app, &state, &project_root)?;
    Ok(FocusResult {
        primary_focus_region_id: normalized_region,
        active_region_ids: manifest.region_state.active_region_ids.clone(),
        materialized_cells,
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
    if let Some(active_ids) = scope.active_region_ids {
        manifest.region_state.active_region_ids = active_ids
            .into_iter()
            .filter_map(|id| canonicalize_region_id(&id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    let mut doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    rematerialize_unlocked_country_surfaces(&app, &mut manifest, &mut doc.scenario)?;
    let materialized_cells = doc.scenario.world.demand_cells.len();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &doc,
    )
    .map_err(|e| e.to_string())?;
    manifest.updated_at = now_string();
    write_manifest(&project_root, &manifest)?;
    let project_path_string = project_root.to_string_lossy().to_string();
    let _ = enqueue_runtime_action_with_retry(
        state.inner(),
        &project_path_string,
        RuntimeAction::InvalidateMaterialization,
    )?;
    let _ = open_session_internal(&app, &state, &project_root)?;
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
    Ok(DemandTileSourceMeta {
        layer: layer.clone(),
        source: "scenario.demand_cells".to_string(),
        countries_loaded: countries,
        cells: doc.scenario.world.demand_cells.len(),
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
