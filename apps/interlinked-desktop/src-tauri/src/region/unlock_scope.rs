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
    if is_uk_country_iso2(country_iso2) {
        UK_EMPLOYMENT_BASELINE_RATIO
    } else {
        DEFAULT_EMPLOYMENT_BASELINE_RATIO
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
    let resolved = sync_country_region_state_with_overrides(
        manifest,
        catalog,
        country_iso2,
        RegionStateOverrides::default(),
    )?;
    Ok((resolved.unlocked_region_ids, resolved.active_region_ids))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RegionStateOverrides {
    pub(crate) ensure_unlocked_region_ids: Vec<String>,
    pub(crate) force_primary_focus_region_id: Option<String>,
    pub(crate) force_active_region_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct CountryRegionState {
    pub(crate) country_iso2: String,
    pub(crate) unlocked_region_ids: Vec<String>,
    pub(crate) primary_focus_region_id: String,
    pub(crate) active_region_ids: Vec<String>,
}

pub(crate) fn sync_country_region_state_with_overrides(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
    overrides: RegionStateOverrides,
) -> Result<CountryRegionState, String> {
    // Authoritative country-level region state reconciliation:
    // canonicalize -> validate against catalog -> apply overrides -> persist merged state.
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let valid = catalog
        .regions
        .iter()
        .map(|r| r.region_id.clone())
        .collect::<HashSet<_>>();
    let resolve_for_catalog = |id: &str| canonical_region_for_catalog(catalog, id);
    let mut unlocked_for_country = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| resolve_for_catalog(id))
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .filter(|id| valid.contains(id))
        .collect::<BTreeSet<_>>();

    for region_id in overrides
        .ensure_unlocked_region_ids
        .iter()
        .filter_map(|id| resolve_for_catalog(id))
    {
        if region_country_iso2(&region_id).as_deref() == Some(iso.as_str())
            && valid.contains(&region_id)
        {
            unlocked_for_country.insert(region_id);
        }
    }

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

    let mut primary = overrides
        .force_primary_focus_region_id
        .as_deref()
        .and_then(resolve_for_catalog)
        .filter(|rid| unlocked_for_country.contains(rid))
        .or_else(|| {
            manifest
                .region_state
                .primary_focus_region_id
                .as_deref()
                .and_then(resolve_for_catalog)
                .filter(|rid| unlocked_for_country.contains(rid))
        });
    if primary.is_none() {
        primary = unlocked_for_country.iter().next().cloned();
    }
    let primary = primary.ok_or_else(|| format!("failed to select primary region for {iso}"))?;

    let unlocked_set = unlocked_for_country.iter().cloned().collect::<HashSet<_>>();
    let mut active_for_country = overrides
        .force_active_region_ids
        .as_ref()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| resolve_for_catalog(id))
                .filter(|id| unlocked_set.contains(id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            manifest
                .region_state
                .active_region_ids
                .iter()
                .filter_map(|id| resolve_for_catalog(id))
                .filter(|id| unlocked_set.contains(id))
                .collect::<Vec<_>>()
        });
    if active_for_country.is_empty() || !active_for_country.contains(&primary) {
        active_for_country = default_active_regions_for_focus(catalog, &primary, &unlocked_set);
    }
    active_for_country = active_for_country
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
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
    manifest.region_state.primary_focus_region_id = Some(primary.clone());

    Ok(CountryRegionState {
        country_iso2: iso,
        unlocked_region_ids: unlocked_for_country.into_iter().collect(),
        primary_focus_region_id: primary,
        active_region_ids: active_for_country,
    })
}

pub(crate) fn materialize_country_surface_scoped(
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let catalog = build_region_catalog_for_surface(&iso, surface)?;
    materialize_country_surface_scoped_with_catalog(manifest, scenario, &iso, &catalog)
}

pub(crate) fn materialize_country_surface_scoped_with_app(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let catalog = build_region_catalog_for_surface_with_app(app, &iso, surface)?;
    materialize_country_surface_scoped_with_catalog(manifest, scenario, &iso, &catalog)
}

fn materialize_country_surface_scoped_with_catalog(
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    catalog: &SurfaceRegionCatalog,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
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
    let iso = canonical_country_iso2(iso).unwrap_or_else(|| iso.trim().to_ascii_uppercase());
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
    clear_surface_generated_persisted_demand(scenario);

    let mut out = Vec::<DemandCoverageResult>::new();
    let mut loaded_countries = Vec::<String>::new();
    let mut surface_version = None::<String>;
    for iso in unlocked_country_codes(manifest) {
        let Some(resolved_surface) =
            crate::commands::content_library::resolve_demand_surface_path(app, &iso)
        else {
            out.push(DemandCoverageResult {
                country_iso2: iso,
                installed: false,
                loaded: false,
                cells_loaded: 0,
                message: "Demand data not installed for country".to_string(),
            });
            continue;
        };
        let surface = load_surface_wire(&resolved_surface.path)?;
        let loaded_count =
            materialize_country_surface_scoped_with_app(app, manifest, scenario, &iso, &surface)?;
        loaded_countries.push(iso.clone());
        surface_version = Some(surface.surface_version.clone());
        upsert_pack_ref(manifest, &iso, &surface);
        out.push(DemandCoverageResult {
            country_iso2: iso,
            installed: true,
            loaded: true,
            cells_loaded: loaded_count,
            message: format!(
                "Loaded scoped region demand from {} ({})",
                resolved_surface.path.to_string_lossy(),
                resolved_surface.source.as_str()
            ),
        });
    }

    // Persisted gameplay demand authority is updated only here after full rematerialization.
    let (persisted_surface_version, persisted_loaded_countries) =
        write_surface_pipeline_demand_meta(scenario, surface_version, loaded_countries);
    sync_manifest_surface_pipeline_state(
        manifest,
        &persisted_surface_version,
        &persisted_loaded_countries,
    );
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
                region_kind: region.region_kind.clone(),
                region_token: region.region_token.clone(),
                h3_cell_id: region.h3_cell_id.clone(),
                name: region.name.clone(),
                admin_level: region.admin_level.clone(),
                nation: region.nation.clone(),
                source_code: region.source_code.clone(),
                adjacency_source: region.adjacency_source.clone(),
                geometry_source: region.geometry_source.clone(),
                unlocked: unlocked_regions.contains(&region.region_id),
                active: active_regions.contains(&region.region_id),
                adjacent_region_ids: region.adjacent_region_ids.clone(),
                unlock_cost_base: region_unlock_cost_base_for_manifest(manifest, &region),
                residents_smooth: region.residents_smooth,
                jobs_smooth: region.jobs_smooth,
                employment_estimate,
                cells_res8,
                geometry: region.geometry.clone(),
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
