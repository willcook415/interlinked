use crate::*;

#[derive(Debug, Clone)]
pub(crate) struct SurfaceRegionInfo {
    pub(crate) region_id: String,
    pub(crate) country_iso2: String,
    pub(crate) name: String,
    pub(crate) admin_level: String,
    pub(crate) nation: Option<String>,
    pub(crate) source_code: Option<String>,
    pub(crate) cell_id: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) area_m2: f64,
    pub(crate) residents_smooth: f64,
    pub(crate) jobs_smooth: f64,
    pub(crate) activity_mix_residential: f64,
    pub(crate) activity_mix_office: f64,
    pub(crate) activity_mix_retail: f64,
    pub(crate) activity_mix_recreation: f64,
    pub(crate) activity_mix_industrial: f64,
    pub(crate) activity_mix_education: f64,
    pub(crate) activity_mix_health: f64,
    pub(crate) adjacent_region_ids: Vec<String>,
    pub(crate) geometry: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub(crate) struct SurfaceRegionCatalog {
    pub(crate) regions: Vec<SurfaceRegionInfo>,
    pub(crate) by_id: HashMap<String, SurfaceRegionInfo>,
    pub(crate) cells_res8_by_region: HashMap<String, Vec<DemandSurfaceCellWire>>,
}

pub(crate) fn nearest_region_ids_by_xy(
    regions: &[SurfaceRegionInfo],
    x: f64,
    y: f64,
    limit: usize,
    exclude_region_id: Option<&str>,
) -> Vec<String> {
    let mut nearest = regions
        .iter()
        .filter(|r| {
            exclude_region_id
                .map(|id| id != r.region_id)
                .unwrap_or(true)
        })
        .map(|r| {
            let d2 = (r.x - x).powi(2) + (r.y - y).powi(2);
            (d2, r.region_id.clone())
        })
        .collect::<Vec<_>>();
    nearest.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    nearest.into_iter().take(limit).map(|(_, id)| id).collect()
}

pub(crate) fn build_surface_region_catalog(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> SurfaceRegionCatalog {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let mut regions = surface
        .cells_res6
        .iter()
        .map(|c| SurfaceRegionInfo {
            region_id: region_id_from_res6(&iso, &c.cell_id),
            country_iso2: iso.clone(),
            name: format!("{} {}", iso, &c.cell_id),
            admin_level: "h3_r6_proxy".to_string(),
            nation: None,
            source_code: None,
            cell_id: c.cell_id.clone(),
            x: c.x,
            y: c.y,
            area_m2: c.area_m2.max(0.0),
            residents_smooth: c.residents_smooth.max(0.0),
            jobs_smooth: c.jobs_smooth.max(0.0),
            activity_mix_residential: c.activity_mix_residential,
            activity_mix_office: c.activity_mix_office,
            activity_mix_retail: c.activity_mix_retail,
            activity_mix_recreation: c.activity_mix_recreation,
            activity_mix_industrial: c.activity_mix_industrial,
            activity_mix_education: c.activity_mix_education,
            activity_mix_health: c.activity_mix_health,
            adjacent_region_ids: vec![],
            geometry: None,
        })
        .collect::<Vec<_>>();

    let mut region_id_by_h3_res6 = HashMap::<CellIndex, String>::new();
    for region in &regions {
        if let Ok(cell) = region.cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                region_id_by_h3_res6.insert(cell, region.region_id.clone());
            }
        }
    }

    for i in 0..regions.len() {
        let region_id = regions[i].region_id.clone();
        let region_cell_id = regions[i].cell_id.clone();
        let region_x = regions[i].x;
        let region_y = regions[i].y;
        let mut adjacent_region_ids = Vec::<String>::new();

        if let Ok(cell) = region_cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                for neighbor in cell.grid_disk::<Vec<_>>(1) {
                    if neighbor == cell {
                        continue;
                    }
                    if let Some(neighbor_region_id) = region_id_by_h3_res6.get(&neighbor) {
                        if neighbor_region_id != &region_id
                            && !adjacent_region_ids.contains(neighbor_region_id)
                        {
                            adjacent_region_ids.push(neighbor_region_id.clone());
                        }
                    }
                }
            }
        }

        if adjacent_region_ids.is_empty() {
            adjacent_region_ids =
                nearest_region_ids_by_xy(&regions, region_x, region_y, 6, Some(region_id.as_str()));
        }
        regions[i].adjacent_region_ids = adjacent_region_ids;
    }

    for region in &mut regions {
        let normalized = normalize_activity_mix([
            region.activity_mix_residential,
            region.activity_mix_office,
            region.activity_mix_retail,
            region.activity_mix_recreation,
            region.activity_mix_industrial,
            region.activity_mix_education,
            region.activity_mix_health,
        ]);
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        let mut region_id = cell
            .cell_id
            .parse::<CellIndex>()
            .ok()
            .and_then(|idx| idx.parent(Resolution::Six))
            .and_then(|parent| region_id_by_h3_res6.get(&parent).cloned());
        if region_id.is_none() {
            region_id = nearest_region_ids_by_xy(&regions, cell.x, cell.y, 1, None)
                .into_iter()
                .next();
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    let by_id = regions
        .iter()
        .map(|r| (r.region_id.clone(), r.clone()))
        .collect::<HashMap<_, _>>();

    SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
    }
}

pub(crate) fn merge_surface_region_catalog_aliases(
    catalog: SurfaceRegionCatalog,
) -> SurfaceRegionCatalog {
    if catalog.regions.is_empty() {
        return catalog;
    }

    let mut canonical_by_region = HashMap::<String, String>::new();
    for region in &catalog.regions {
        let canonical =
            canonicalize_region_id(&region.region_id).unwrap_or_else(|| region.region_id.clone());
        canonical_by_region.insert(region.region_id.clone(), canonical);
    }

    let canonical_for = |region_id: &str, lookup: &HashMap<String, String>| {
        lookup
            .get(region_id)
            .cloned()
            .or_else(|| canonicalize_region_id(region_id))
            .unwrap_or_else(|| region_id.to_string())
    };

    let mut grouped = HashMap::<String, Vec<SurfaceRegionInfo>>::new();
    for mut region in catalog.regions {
        let canonical = canonical_for(&region.region_id, &canonical_by_region);
        region.adjacent_region_ids = region
            .adjacent_region_ids
            .iter()
            .map(|neighbor| canonical_for(neighbor, &canonical_by_region))
            .filter(|neighbor| neighbor != &canonical)
            .collect();
        region.region_id = canonical.clone();
        grouped.entry(canonical).or_default().push(region);
    }

    let mut merged_regions = Vec::<SurfaceRegionInfo>::new();
    for (canonical_region_id, mut group) in grouped {
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| {
            let a_score = a.area_m2.max(a.residents_smooth + a.jobs_smooth);
            let b_score = b.area_m2.max(b.residents_smooth + b.jobs_smooth);
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged = group[0].clone();
        merged.region_id = canonical_region_id.clone();
        let mut adjacency = merged.adjacent_region_ids.clone();

        let mut weighted_total = 0.0_f64;
        let mut weighted_x = 0.0_f64;
        let mut weighted_y = 0.0_f64;
        let mut mix_sums = [0.0_f64; 7];
        let mut total_residents = 0.0_f64;
        let mut total_jobs = 0.0_f64;
        let mut total_area = 0.0_f64;

        for region in group {
            let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
            weighted_total += weight;
            weighted_x += region.x * weight;
            weighted_y += region.y * weight;
            total_residents += region.residents_smooth.max(0.0);
            total_jobs += region.jobs_smooth.max(0.0);
            total_area += region.area_m2.max(0.0);
            mix_sums[0] += region.activity_mix_residential.max(0.0) * weight;
            mix_sums[1] += region.activity_mix_office.max(0.0) * weight;
            mix_sums[2] += region.activity_mix_retail.max(0.0) * weight;
            mix_sums[3] += region.activity_mix_recreation.max(0.0) * weight;
            mix_sums[4] += region.activity_mix_industrial.max(0.0) * weight;
            mix_sums[5] += region.activity_mix_education.max(0.0) * weight;
            mix_sums[6] += region.activity_mix_health.max(0.0) * weight;
            adjacency.extend(region.adjacent_region_ids.clone());
        }

        if weighted_total > 0.0 {
            merged.x = weighted_x / weighted_total;
            merged.y = weighted_y / weighted_total;
        }
        merged.residents_smooth = total_residents;
        merged.jobs_smooth = total_jobs;
        merged.area_m2 = total_area;
        let normalized_mix = normalize_activity_mix([
            mix_sums[0] / weighted_total.max(1e-9),
            mix_sums[1] / weighted_total.max(1e-9),
            mix_sums[2] / weighted_total.max(1e-9),
            mix_sums[3] / weighted_total.max(1e-9),
            mix_sums[4] / weighted_total.max(1e-9),
            mix_sums[5] / weighted_total.max(1e-9),
            mix_sums[6] / weighted_total.max(1e-9),
        ]);
        merged.activity_mix_residential = normalized_mix[0];
        merged.activity_mix_office = normalized_mix[1];
        merged.activity_mix_retail = normalized_mix[2];
        merged.activity_mix_recreation = normalized_mix[3];
        merged.activity_mix_industrial = normalized_mix[4];
        merged.activity_mix_education = normalized_mix[5];
        merged.activity_mix_health = normalized_mix[6];
        merged.adjacent_region_ids = adjacency;
        merged_regions.push(merged);
    }

    let valid_region_ids = merged_regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut merged_regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid) && rid != &region.region_id);
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }

    let mut merged_cells = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for (region_id, cells) in catalog.cells_res8_by_region {
        let canonical = canonical_for(&region_id, &canonical_by_region);
        merged_cells.entry(canonical).or_default().extend(cells);
    }

    merged_regions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    let by_id = merged_regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    SurfaceRegionCatalog {
        regions: merged_regions,
        by_id,
        cells_res8_by_region: merged_cells,
    }
}

pub(crate) fn build_region_catalog_for_surface(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso == "GB" {
        return build_gb_county_region_catalog(surface).map(merge_surface_region_catalog_aliases);
    }
    Ok(merge_surface_region_catalog_aliases(
        build_surface_region_catalog(&iso, surface),
    ))
}

pub(crate) fn build_gb_county_region_catalog(
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let county_catalog = load_gb_county_boundaries()?;
    let counties = county_catalog.counties;
    if counties.is_empty() {
        return Err("no GB counties available".to_string());
    }

    let mut regions = counties
        .iter()
        .map(|county| SurfaceRegionInfo {
            region_id: region_id_from_county("GB", &county.county_id),
            country_iso2: county.country_iso2.clone(),
            name: county.name.clone(),
            admin_level: "uk_county".to_string(),
            nation: Some(county.nation.clone()),
            source_code: Some(county.source_code.clone()),
            cell_id: county.county_id.clone(),
            x: 0.0,
            y: 0.0,
            area_m2: 0.0,
            residents_smooth: 0.0,
            jobs_smooth: 0.0,
            activity_mix_residential: 0.0,
            activity_mix_office: 0.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            adjacent_region_ids: vec![],
            geometry: Some(county.geometry_json.clone()),
        })
        .collect::<Vec<_>>();
    let county_index = counties
        .iter()
        .enumerate()
        .map(|(idx, county)| (county.county_id.clone(), idx))
        .collect::<HashMap<_, _>>();
    let adjacency_map = gb_county_adjacency_map(&counties);
    let mut res6_owner = HashMap::<String, usize>::new();

    for cell in &surface.cells_res6 {
        let county = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat));
        let Some(county) = county else { continue };
        let Some(&idx) = county_index.get(&county.county_id) else {
            continue;
        };
        res6_owner.insert(cell.cell_id.clone(), idx);
        let weight = (cell.residents_smooth + cell.jobs_smooth).max(1.0);
        let region = &mut regions[idx];
        region.area_m2 += cell.area_m2.max(0.0);
        region.residents_smooth += cell.residents_smooth.max(0.0);
        region.jobs_smooth += cell.jobs_smooth.max(0.0);
        region.x += cell.x * weight;
        region.y += cell.y * weight;
    }

    for region in &mut regions {
        let total_weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
        if total_weight > 0.0 {
            region.x /= total_weight;
            region.y /= total_weight;
        } else if let Some(county) = county_index
            .get(&region.cell_id)
            .and_then(|idx| counties.get(*idx))
        {
            let (x, y) = lonlat_to_web_mercator_m(county.bbox_center_lon, county.bbox_center_lat);
            region.x = x;
            region.y = y;
        }
    }

    for region in &mut regions {
        region.adjacent_region_ids = adjacency_map
            .get(&region.region_id)
            .cloned()
            .unwrap_or_default();
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        // Assign res8 cells by actual county geometry first.
        // Parent-res6 ownership can smear small counties and blur city-center detail.
        let mut region_id = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat))
            .and_then(|county| county_index.get(&county.county_id).copied())
            .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        if region_id.is_none() {
            region_id = cell
                .cell_id
                .parse::<CellIndex>()
                .ok()
                .and_then(|idx| idx.parent(Resolution::Six))
                .and_then(|parent| res6_owner.get(&parent.to_string()).copied())
                .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    for region in &mut regions {
        let normalized = if let Some(cells) = cells_res8_by_region.get(&region.region_id) {
            let mut w_sum = 0.0_f64;
            let mut r_sum = 0.0_f64;
            let mut o_sum = 0.0_f64;
            let mut rt_sum = 0.0_f64;
            let mut rc_sum = 0.0_f64;
            let mut i_sum = 0.0_f64;
            let mut e_sum = 0.0_f64;
            let mut h_sum = 0.0_f64;
            for c in cells {
                let w = (c.residents_smooth + c.jobs_smooth).max(1e-6);
                w_sum += w;
                r_sum += c.activity_mix_residential.max(0.0) * w;
                o_sum += c.activity_mix_office.max(0.0) * w;
                rt_sum += c.activity_mix_retail.max(0.0) * w;
                rc_sum += c.activity_mix_recreation.max(0.0) * w;
                i_sum += c.activity_mix_industrial.max(0.0) * w;
                e_sum += c.activity_mix_education.max(0.0) * w;
                h_sum += c.activity_mix_health.max(0.0) * w;
            }
            let denom = w_sum.max(1e-9);
            normalize_activity_mix([
                r_sum / denom,
                o_sum / denom,
                rt_sum / denom,
                rc_sum / denom,
                i_sum / denom,
                e_sum / denom,
                h_sum / denom,
            ])
        } else {
            normalize_activity_mix([
                region.activity_mix_residential,
                region.activity_mix_office,
                region.activity_mix_retail,
                region.activity_mix_recreation,
                region.activity_mix_industrial,
                region.activity_mix_education,
                region.activity_mix_health,
            ])
        };
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let valid_region_ids = regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid));
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }
    let by_id = regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    Ok(SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
    })
}

pub(crate) fn nearest_region_for_start(
    catalog: &SurfaceRegionCatalog,
    start: Option<&StartLocation>,
    country_iso2: &str,
) -> Option<String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let Some(s) = start.filter(|x| x.country_iso2.eq_ignore_ascii_case(&iso)) else {
        return catalog
            .regions
            .iter()
            .max_by(|a, b| {
                (a.residents_smooth + a.jobs_smooth)
                    .partial_cmp(&(b.residents_smooth + b.jobs_smooth))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.region_id.clone());
    };
    if iso == "GB" {
        if let Some(county_id) = preferred_home_county_id(s) {
            let region_id = region_id_from_county(&iso, county_id);
            if catalog.by_id.contains_key(&region_id) {
                return Some(region_id);
            }
        }
    }
    let (sx, sy) = lonlat_to_web_mercator_m(s.city_lon, s.city_lat);
    catalog
        .regions
        .iter()
        .min_by(|a, b| {
            let da = (a.x - sx).powi(2) + (a.y - sy).powi(2);
            let db = (b.x - sx).powi(2) + (b.y - sy).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|r| r.region_id.clone())
}

pub(crate) fn load_region_catalog_for_country(
    app: &AppHandle,
    country_iso2: &str,
) -> Result<Option<SurfaceRegionCatalog>, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Ok(None);
    }
    let Some(path) = demand_surface_file(app, &iso) else {
        return Ok(None);
    };
    let surface = load_surface_wire(&path)?;
    Ok(Some(build_region_catalog_for_surface(&iso, &surface)?))
}
