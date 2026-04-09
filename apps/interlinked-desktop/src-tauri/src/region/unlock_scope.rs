use crate::*;

pub(crate) fn default_active_regions_for_focus(
    catalog: &SurfaceRegionCatalog,
    focus_region_id: &str,
    unlocked: &HashSet<String>,
) -> Vec<String> {
    let mut out = vec![focus_region_id.to_string()];
    let Some(focus) = catalog.by_id.get(focus_region_id) else {
        return out;
    };
    for rid in &focus.adjacent_region_ids {
        if unlocked.contains(rid) && !out.contains(rid) {
            out.push(rid.clone());
        }
        if out.len() >= 4 {
            break;
        }
    }
    out
}

pub(crate) fn region_unlock_cost_base(region: &SurfaceRegionInfo) -> f64 {
    let scale = (region.residents_smooth + region.jobs_smooth)
        .max(1.0)
        .sqrt();
    20_000_000.0 + scale * 22_000.0
}

pub(crate) fn region_unlock_cost_base_for_manifest(
    manifest: &ProjectManifest,
    region: &SurfaceRegionInfo,
) -> f64 {
    let profile = resolved_difficulty_profile(manifest);
    region_unlock_cost_base(region) * profile.unlock_cost_mult.max(0.0)
}

pub(crate) fn country_employment_baseline_ratio(country_iso2: &str) -> f64 {
    match country_iso2.trim().to_ascii_uppercase().as_str() {
        "GB" => UK_EMPLOYMENT_BASELINE_RATIO,
        _ => DEFAULT_EMPLOYMENT_BASELINE_RATIO,
    }
}

pub(crate) fn region_employment_raw_score(region: &SurfaceRegionInfo) -> f64 {
    let residents = region.residents_smooth.max(0.0);
    if residents <= 0.0 {
        return 0.0;
    }
    let weighted_mix = 0.32 * region.activity_mix_residential.max(0.0)
        + 1.42 * region.activity_mix_office.max(0.0)
        + 0.96 * region.activity_mix_retail.max(0.0)
        + 0.66 * region.activity_mix_recreation.max(0.0)
        + 1.24 * region.activity_mix_industrial.max(0.0)
        + 0.98 * region.activity_mix_education.max(0.0)
        + 1.04 * region.activity_mix_health.max(0.0);
    residents * weighted_mix.max(0.06)
}

pub(crate) fn sync_country_region_state(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let valid = catalog
        .regions
        .iter()
        .map(|r| r.region_id.clone())
        .collect::<HashSet<_>>();
    let mut unlocked_for_country = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .filter(|id| valid.contains(id))
        .collect::<BTreeSet<_>>();

    if unlocked_for_country.is_empty() {
        if let Some(seed_region) =
            nearest_region_for_start(catalog, manifest.start_location.as_ref(), &iso)
        {
            unlocked_for_country.insert(seed_region);
        }
    }
    if unlocked_for_country.is_empty() {
        return Err(format!("no regions available for country {iso}"));
    }

    let mut primary = manifest
        .region_state
        .primary_focus_region_id
        .as_deref()
        .and_then(canonicalize_region_id)
        .filter(|rid| unlocked_for_country.contains(rid));
    if primary.is_none() {
        primary = unlocked_for_country.iter().next().cloned();
    }
    let primary = primary.ok_or_else(|| format!("failed to select primary region for {iso}"))?;

    let unlocked_set = unlocked_for_country.iter().cloned().collect::<HashSet<_>>();
    let mut active_for_country = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| unlocked_set.contains(id))
        .collect::<Vec<_>>();
    if active_for_country.is_empty() || !active_for_country.contains(&primary) {
        active_for_country = default_active_regions_for_focus(catalog, &primary, &unlocked_set);
    }
    active_for_country.truncate(8);

    let mut merged_unlocked = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() != Some(iso.as_str()))
        .collect::<BTreeSet<_>>();
    for rid in &unlocked_for_country {
        merged_unlocked.insert(rid.clone());
    }
    manifest.region_state.unlocked_region_ids = merged_unlocked.into_iter().collect();

    let mut merged_active = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter(|id| region_country_iso2(id).as_deref() != Some(iso.as_str()))
        .collect::<BTreeSet<_>>();
    for rid in &active_for_country {
        merged_active.insert(rid.clone());
    }
    manifest.region_state.active_region_ids = merged_active.into_iter().collect();
    manifest.region_state.primary_focus_region_id = Some(primary);

    Ok((
        unlocked_for_country.into_iter().collect::<Vec<_>>(),
        active_for_country,
    ))
}

pub(crate) fn materialize_country_surface_scoped(
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let catalog = build_region_catalog_for_surface(&iso, surface)?;
    let (unlocked_regions, active_regions) = sync_country_region_state(manifest, &catalog, &iso)?;

    let max_active_zones = manifest.simulation_scope.max_active_zones.clamp(
        manifest.simulation_scope.remote_max_active_zones.max(20),
        manifest
            .simulation_scope
            .focus_max_active_zones
            .clamp(120, 6000),
    );
    let active_count = active_regions.len().max(1);
    let per_region_cap = (max_active_zones / active_count).clamp(40, 400);
    let active_set = active_regions.into_iter().collect::<HashSet<_>>();

    let mut loaded_cells = 0usize;
    for region_id in &unlocked_regions {
        if active_set.contains(region_id) {
            let mut cells = catalog
                .cells_res8_by_region
                .get(region_id)
                .cloned()
                .unwrap_or_default();
            cells.sort_by(|a, b| {
                (b.residents_smooth + b.jobs_smooth)
                    .partial_cmp(&(a.residents_smooth + a.jobs_smooth))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            cells.truncate(per_region_cap);
            for c in cells {
                let residents = c.residents_smooth.max(0.0);
                let jobs = c.jobs_smooth.max(0.0);
                let (wx, wy) = web_mercator_m_to_world_xy(&scenario.meta.crs, c.x, c.y);
                let [residential, office, retail, recreation, industrial, education, health] =
                    normalize_activity_mix([
                        c.activity_mix_residential,
                        c.activity_mix_office,
                        c.activity_mix_retail,
                        c.activity_mix_recreation,
                        c.activity_mix_industrial,
                        c.activity_mix_education,
                        c.activity_mix_health,
                    ]);
                let cid = format!("ds:v4:{}:{}", iso, c.cell_id);
                scenario.world.demand_cells.push(DemandCell {
                    cell_id: cid.clone(),
                    x: wx,
                    y: wy,
                    area_m2: c.area_m2.max(0.0),
                    residents_night: residents,
                    jobs_day: jobs,
                    activity_mix_residential: residential,
                    activity_mix_office: office,
                    activity_mix_retail: retail,
                    activity_mix_recreation: recreation,
                    activity_mix_industrial: industrial,
                    activity_mix_education: education,
                    activity_mix_health: health,
                    centrality_score: c.quality.clamp(0.0, 1.0),
                    data_quality_score: c.quality.clamp(0.0, 1.0),
                    country_iso2: Some(iso.clone()),
                });
                scenario.world.zones.push(Zone {
                    id: cid,
                    x: wx,
                    y: wy,
                    population: residents,
                    jobs,
                    country_iso2: Some(iso.clone()),
                });
                loaded_cells += 1;
            }
            continue;
        }

        if let Some(region) = catalog.by_id.get(region_id) {
            let residents = region.residents_smooth.max(0.0);
            let jobs = region.jobs_smooth.max(0.0);
            let (wx, wy) = web_mercator_m_to_world_xy(&scenario.meta.crs, region.x, region.y);
            let [residential, office, retail, recreation, industrial, education, health] =
                normalize_activity_mix([
                    region.activity_mix_residential,
                    region.activity_mix_office,
                    region.activity_mix_retail,
                    region.activity_mix_recreation,
                    region.activity_mix_industrial,
                    region.activity_mix_education,
                    region.activity_mix_health,
                ]);
            let cid = format!("ds:v4m:{}:{}", iso, region.cell_id);
            scenario.world.demand_cells.push(DemandCell {
                cell_id: cid.clone(),
                x: wx,
                y: wy,
                area_m2: region.area_m2.max(0.0),
                residents_night: residents,
                jobs_day: jobs,
                activity_mix_residential: residential,
                activity_mix_office: office,
                activity_mix_retail: retail,
                activity_mix_recreation: recreation,
                activity_mix_industrial: industrial,
                activity_mix_education: education,
                activity_mix_health: health,
                centrality_score: 0.45,
                data_quality_score: 0.65,
                country_iso2: Some(iso.clone()),
            });
            scenario.world.zones.push(Zone {
                id: cid,
                x: wx,
                y: wy,
                population: residents,
                jobs,
                country_iso2: Some(iso.clone()),
            });
            loaded_cells += 1;
        }
    }
    Ok(loaded_cells)
}

pub(crate) fn upsert_pack_ref(
    manifest: &mut ProjectManifest,
    iso: &str,
    surface: &DemandSurfaceCountryWire,
) {
    let iso = iso.trim().to_ascii_uppercase();
    manifest.pack_refs.retain(|p| p.country_iso2 != iso);
    manifest.pack_refs.push(CountryPackRef {
        country_iso2: iso,
        surface_version: Some(surface.surface_version.clone()),
        checksum: None,
    });
    manifest
        .pack_refs
        .sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
}

pub(crate) fn rematerialize_unlocked_country_surfaces(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
) -> Result<Vec<DemandCoverageResult>, String> {
    scenario
        .world
        .demand_cells
        .retain(|c| !is_surface_generated_cell_id(&c.cell_id));
    scenario
        .world
        .zones
        .retain(|z| !is_surface_generated_zone_id(&z.id));

    let mut out = Vec::<DemandCoverageResult>::new();
    let mut loaded_countries = Vec::<String>::new();
    let mut surface_version = None::<String>;
    for iso in unlocked_country_codes(manifest) {
        let Some(path) = demand_surface_file(app, &iso) else {
            out.push(DemandCoverageResult {
                country_iso2: iso,
                installed: false,
                loaded: false,
                cells_loaded: 0,
                message: "Demand data not installed for country".to_string(),
            });
            continue;
        };
        let surface = load_surface_wire(&path)?;
        let loaded_count = materialize_country_surface_scoped(manifest, scenario, &iso, &surface)?;
        loaded_countries.push(iso.clone());
        surface_version = Some(surface.surface_version.clone());
        upsert_pack_ref(manifest, &iso, &surface);
        out.push(DemandCoverageResult {
            country_iso2: iso,
            installed: true,
            loaded: true,
            cells_loaded: loaded_count,
            message: format!(
                "Loaded scoped region demand from {}",
                path.to_string_lossy()
            ),
        });
    }

    loaded_countries = normalize_loaded_countries(loaded_countries);
    scenario.world.demand_meta = Some(DemandMeta {
        surface_version: surface_version.unwrap_or_else(|| "v4".to_string()),
        loaded_countries: loaded_countries.clone(),
        source: "surface_v4_region_scope".to_string(),
    });
    let mut ds = manifest
        .demand_surface
        .clone()
        .unwrap_or_else(default_demand_surface_manifest);
    if let Some(version) = scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.surface_version.clone())
    {
        ds.surface_version = version;
    }
    ds.loaded_countries = loaded_countries;
    ds.last_rebuild_at = Some(now_string());
    manifest.demand_surface = Some(ds);
    Ok(out)
}

pub(crate) fn ensure_country_surface_loaded(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
) -> Result<DemandCoverageResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two letters".to_string());
    }
    let mut countries = unlocked_country_codes(manifest)
        .into_iter()
        .collect::<BTreeSet<_>>();
    countries.insert(iso.clone());
    manifest.economy.unlocked_countries = countries.into_iter().collect();
    let all = rematerialize_unlocked_country_surfaces(app, manifest, scenario)?;
    Ok(all
        .into_iter()
        .find(|c| c.country_iso2.eq_ignore_ascii_case(&iso))
        .unwrap_or(DemandCoverageResult {
            country_iso2: iso,
            installed: false,
            loaded: false,
            cells_loaded: 0,
            message: "Demand data not installed for country".to_string(),
        }))
}

pub(crate) fn region_status_rows_for_manifest(
    app: &AppHandle,
    manifest: &ProjectManifest,
) -> Result<Vec<RegionStatus>, String> {
    let unlocked_regions = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();
    let active_regions = manifest
        .region_state
        .active_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .collect::<HashSet<_>>();

    let mut rows = Vec::<RegionStatus>::new();
    for iso in unlocked_country_codes(manifest) {
        let Some(catalog) = load_region_catalog_for_country(app, &iso)? else {
            continue;
        };
        let residents_total = catalog
            .regions
            .iter()
            .map(|region| region.residents_smooth.max(0.0))
            .sum::<f64>();
        let raw_employment_total = catalog
            .regions
            .iter()
            .map(region_employment_raw_score)
            .sum::<f64>();
        let target_employment_total =
            residents_total * country_employment_baseline_ratio(iso.as_str());
        let employment_scale = if raw_employment_total > 0.0 {
            target_employment_total / raw_employment_total
        } else {
            0.0
        };
        for region in catalog.regions {
            let cells_res8 = catalog
                .cells_res8_by_region
                .get(&region.region_id)
                .map(|v| v.len())
                .unwrap_or(0);
            let employment_estimate =
                (region_employment_raw_score(&region) * employment_scale).max(0.0);
            rows.push(RegionStatus {
                region_id: region.region_id.clone(),
                country_iso2: region.country_iso2.clone(),
                name: region.name.clone(),
                admin_level: region.admin_level.clone(),
                nation: region.nation.clone(),
                source_code: region.source_code.clone(),
                unlocked: unlocked_regions.contains(&region.region_id),
                active: active_regions.contains(&region.region_id),
                adjacent_region_ids: region.adjacent_region_ids.clone(),
                unlock_cost_base: region_unlock_cost_base_for_manifest(manifest, &region),
                residents_smooth: region.residents_smooth,
                jobs_smooth: region.jobs_smooth,
                employment_estimate,
                cells_res8,
                geometry: if region.country_iso2.eq_ignore_ascii_case("GB")
                    && counties_file(app, "GB").is_some()
                {
                    None
                } else {
                    region.geometry.clone()
                },
            });
        }
    }
    rows.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    Ok(rows)
}
