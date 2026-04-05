use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DemandSurfaceCountryWire {
    pub(crate) country_iso2: String,
    pub(crate) surface_version: String,
    pub(crate) source_provenance: JsonValue,
    pub(crate) cells_res6: Vec<DemandSurfaceCellWire>,
    pub(crate) cells_res7: Vec<DemandSurfaceCellWire>,
    pub(crate) cells_res8: Vec<DemandSurfaceCellWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DemandSurfaceCellWire {
    pub(crate) cell_id: String,
    pub(crate) h3_res: u8,
    pub(crate) lon: f64,
    pub(crate) lat: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) area_m2: f64,
    pub(crate) country_iso2: String,
    pub(crate) residents_raw: f64,
    pub(crate) jobs_raw: f64,
    pub(crate) residents_smooth: f64,
    pub(crate) jobs_smooth: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_residential: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_office: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_retail: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_recreation: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_industrial: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_education: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_health: f64,
    pub(crate) quality: f64,
}

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

pub(crate) fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
    let mut cleaned = values.map(|v| if v.is_finite() && v > 0.0 { v } else { 0.0 });
    let sum: f64 = cleaned.iter().sum();
    if sum <= 1e-9 {
        return [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    }
    for v in &mut cleaned {
        *v /= sum;
    }
    cleaned
}

pub(crate) fn default_surface_activity_mix() -> f64 {
    0.0
}

pub(crate) fn legacy_mix_from_residents_jobs(residents: f64, jobs: f64, area_m2: f64) -> [f64; 7] {
    let r = residents.max(0.0);
    let j = jobs.max(0.0);
    if r + j <= 1e-9 {
        return [0.70, 0.08, 0.08, 0.05, 0.05, 0.02, 0.02];
    }
    let total = r + j;
    let resident_balance = (r / total).clamp(0.0, 1.0);
    let employment_balance = (j / total).clamp(0.0, 1.0);
    let area_km2 = (area_m2.max(1.0) / 1_000_000.0).max(0.05);
    let density = total / area_km2;
    let urbanity = ((density + 1.0).log10() / 4.0).clamp(0.0, 1.0);

    normalize_activity_mix([
        0.52 + 0.30 * resident_balance - 0.25 * urbanity,
        0.05 + 0.22 * employment_balance + 0.10 * urbanity,
        0.05 + 0.09 * urbanity + 0.03 * employment_balance,
        0.04 + 0.07 * urbanity,
        0.04 + 0.12 * employment_balance + 0.04 * urbanity,
        0.03 + 0.04 * urbanity,
        0.03 + 0.03 * urbanity,
    ])
}

pub(crate) fn backfill_legacy_surface_mix(surface: &mut DemandSurfaceCountryWire) {
    let fill = |cells: &mut [DemandSurfaceCellWire]| {
        for c in cells {
            let current = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            let current_sum: f64 = current.iter().filter(|v| v.is_finite() && **v >= 0.0).sum();
            let normalized = if current_sum > 1e-9 {
                normalize_activity_mix(current)
            } else {
                legacy_mix_from_residents_jobs(c.residents_smooth, c.jobs_smooth, c.area_m2)
            };
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
    };
    fill(&mut surface.cells_res8);
    fill(&mut surface.cells_res7);
    fill(&mut surface.cells_res6);
}

pub(crate) fn unlocked_country_codes(manifest: &ProjectManifest) -> Vec<String> {
    let mut set = manifest
        .economy
        .unlocked_countries
        .iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            set.insert(code);
        }
    }
    set.into_iter().collect()
}

pub(crate) fn load_surface_wire(path: &Path) -> Result<DemandSurfaceCountryWire, String> {
    let mut surface: DemandSurfaceCountryWire = read_json_file(path)?;
    if surface.surface_version.eq_ignore_ascii_case("v3") {
        backfill_legacy_surface_mix(&mut surface);
        surface.surface_version = "v4".to_string();
    } else if !surface.surface_version.eq_ignore_ascii_case("v4") {
        return Err(format!(
            "unsupported demand surface version '{}' in {} (expected v4 or v3)",
            surface.surface_version,
            path.display()
        ));
    }

    let validate_cells = |cells: &mut [DemandSurfaceCellWire], layer: &str| -> Result<(), String> {
        for c in cells {
            let values = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(format!(
                    "invalid activity mix in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let sum: f64 = values.iter().sum();
            if sum <= 1e-9 {
                return Err(format!(
                    "activity mix sum must be > 0 in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let normalized = normalize_activity_mix(values);
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
        Ok(())
    };

    validate_cells(&mut surface.cells_res8, "res8")?;
    validate_cells(&mut surface.cells_res7, "res7")?;
    validate_cells(&mut surface.cells_res6, "res6")?;
    Ok(surface)
}

pub(crate) fn normalize_loaded_countries(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn normalize_region_id(value: &str) -> Option<String> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, token] => {
            let tier = tier.trim().to_ascii_lowercase();
            let iso = iso.trim().to_ascii_uppercase();
            let token = token.trim();
            if (tier != "r6" && tier != "r7" && tier != "county")
                || iso.len() != 2
                || token.is_empty()
            {
                return None;
            }
            let token = token.to_ascii_lowercase();
            Some(format!("{tier}:{iso}:{token}"))
        }
        _ => None,
    }
}

pub(crate) fn canonicalize_region_id(value: &str) -> Option<String> {
    let mut normalized = normalize_region_id(value)?;
    if !normalized.starts_with("county:GB:") {
        return Some(normalized);
    }
    if let Ok(aliases) = load_gb_county_aliases() {
        for _ in 0..8 {
            let Some(mapped) = aliases.get(&normalized) else {
                break;
            };
            if mapped == &normalized {
                break;
            }
            let Some(next) = normalize_region_id(mapped) else {
                break;
            };
            normalized = next;
        }
    }
    Some(normalized)
}

pub(crate) fn canonicalize_region_ledger(ledger: &mut BTreeMap<String, RegionEconomyLedger>) {
    let mut merged = BTreeMap::<String, RegionEconomyLedger>::new();
    let old = std::mem::take(ledger);
    for (key, value) in old {
        let canonical = canonicalize_region_id(&key).unwrap_or(key);
        let entry = merged.entry(canonical).or_default();
        entry.revenue_base += value.revenue_base;
        entry.opex_base += value.opex_base;
        entry.capex_base += value.capex_base;
        entry.penalties_base += value.penalties_base;
        entry.net_base =
            entry.revenue_base - entry.opex_base - entry.capex_base - entry.penalties_base;
    }
    *ledger = merged;
}

pub(crate) fn region_country_iso2(region_id: &str) -> Option<String> {
    let parts = region_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [tier, iso, _token]
            if (tier.to_ascii_lowercase().starts_with('r')
                || tier.eq_ignore_ascii_case("county"))
                && iso.len() == 2 =>
        {
            Some(iso.to_ascii_uppercase())
        }
        _ => None,
    }
}

pub(crate) fn cell_id_is_legacy_generated(cell_id: &str) -> bool {
    cell_id.starts_with("df:") || cell_id.starts_with("dc:")
}

pub(crate) fn zone_id_is_legacy_generated(zone_id: &str) -> bool {
    zone_id.starts_with("z:df:") || zone_id.starts_with("z:dc:")
}

pub(crate) fn migrate_legacy_synthetic_demand(scenario: &mut Scenario) -> bool {
    let mut changed = false;
    let old_cells = scenario.world.demand_cells.len();
    scenario
        .world
        .demand_cells
        .retain(|c| !cell_id_is_legacy_generated(&c.cell_id));
    if scenario.world.demand_cells.len() != old_cells {
        changed = true;
    }

    let old_zones = scenario.world.zones.len();
    scenario.world.zones.retain(|z| {
        if zone_id_is_legacy_generated(&z.id) {
            return false;
        }
        if let Some(cell_id) = z.id.strip_prefix("z:") {
            return !cell_id_is_legacy_generated(cell_id);
        }
        true
    });
    if scenario.world.zones.len() != old_zones {
        changed = true;
    }

    let should_reset_meta = scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.source != "surface_v4_region_scope")
        .unwrap_or(true);
    if changed || should_reset_meta {
        scenario.world.demand_meta = Some(DemandMeta {
            surface_version: "v4".to_string(),
            loaded_countries: vec![],
            source: "surface_v4_region_scope".to_string(),
        });
        changed = true;
    }
    changed
}

pub(crate) fn is_surface_generated_cell_id(cell_id: &str) -> bool {
    cell_id.starts_with("ds:v3:") || cell_id.starts_with("ds:v4:") || cell_id.starts_with("ds:v4m:")
}

pub(crate) fn is_surface_generated_zone_id(zone_id: &str) -> bool {
    if zone_id.starts_with("ds:v3:")
        || zone_id.starts_with("ds:v4:")
        || zone_id.starts_with("ds:v4m:")
    {
        return true;
    }
    zone_id
        .strip_prefix("z:")
        .map(is_surface_generated_cell_id)
        .unwrap_or(false)
}

pub(crate) fn region_id_from_res6(iso: &str, res6_cell_id: &str) -> String {
    format!("r6:{}:{}", iso.trim().to_ascii_uppercase(), res6_cell_id)
}

pub(crate) fn region_id_from_county(iso: &str, county_id: &str) -> String {
    format!(
        "county:{}:{}",
        iso.trim().to_ascii_uppercase(),
        county_id.trim()
    )
}

pub(crate) fn preferred_home_county_id(start: &StartLocation) -> Option<&'static str> {
    let city = start.city_name.trim().to_ascii_lowercase();
    match city.as_str() {
        "london" | "city of london" => Some("greater-london"),
        "manchester" => Some("greater-manchester"),
        "leeds" => Some("west-yorkshire"),
        _ => None,
    }
}

pub(crate) fn county_for_lon_lat(
    counties: &[CountyBoundary],
    lon: f64,
    lat: f64,
) -> Option<&CountyBoundary> {
    let point = Point::new(lon, lat);
    counties
        .iter()
        .find(|county| county.geometry.contains(&point))
}

pub(crate) fn nearest_county_for_lon_lat(
    counties: &[CountyBoundary],
    lon: f64,
    lat: f64,
) -> Option<&CountyBoundary> {
    counties.iter().min_by(|a, b| {
        let da = (a.bbox_center_lon - lon).powi(2) + (a.bbox_center_lat - lat).powi(2);
        let db = (b.bbox_center_lon - lon).powi(2) + (b.bbox_center_lat - lat).powi(2);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })
}

pub(crate) fn landuse_class_profile(class_name: &str) -> Option<([f64; 7], f64)> {
    let class_name = class_name.trim().to_ascii_lowercase();
    let (mix, intensity) = match class_name.as_str() {
        "residential" => ([0.78, 0.06, 0.05, 0.04, 0.01, 0.04, 0.02], 1.10),
        "commercial" => ([0.07, 0.56, 0.22, 0.03, 0.05, 0.04, 0.03], 2.55),
        "retail" => ([0.10, 0.22, 0.52, 0.08, 0.02, 0.03, 0.03], 2.30),
        "industrial" => ([0.03, 0.28, 0.10, 0.03, 0.48, 0.04, 0.04], 2.05),
        "park" | "meadow" | "grass" | "forest" | "natural" => {
            ([0.18, 0.02, 0.04, 0.70, 0.01, 0.03, 0.02], 0.45)
        }
        _ => return None,
    };
    Some((normalize_activity_mix(mix), intensity))
}

pub(crate) fn update_lonlat_bounds(point: &[JsonValue], bounds: &mut Option<(f64, f64, f64, f64)>) {
    if point.len() < 2 {
        return;
    }
    let Some(lon) = point[0].as_f64() else { return };
    let Some(lat) = point[1].as_f64() else { return };
    if !lon.is_finite() || !lat.is_finite() {
        return;
    }
    if let Some((min_lon, min_lat, max_lon, max_lat)) = bounds.as_mut() {
        *min_lon = (*min_lon).min(lon);
        *min_lat = (*min_lat).min(lat);
        *max_lon = (*max_lon).max(lon);
        *max_lat = (*max_lat).max(lat);
    } else {
        *bounds = Some((lon, lat, lon, lat));
    }
}

pub(crate) fn geometry_lonlat_bounds(geometry: &JsonValue) -> Option<(f64, f64, f64, f64)> {
    let gtype = geometry.get("type").and_then(|v| v.as_str())?;
    let coords = geometry.get("coordinates")?;
    let mut bounds = None::<(f64, f64, f64, f64)>;
    match gtype {
        "Polygon" => {
            let rings = coords.as_array()?;
            for ring in rings {
                let Some(points) = ring.as_array() else {
                    continue;
                };
                for point in points {
                    if let Some(pair) = point.as_array() {
                        update_lonlat_bounds(pair, &mut bounds);
                    }
                }
            }
        }
        "MultiPolygon" => {
            let polygons = coords.as_array()?;
            for polygon in polygons {
                let Some(rings) = polygon.as_array() else {
                    continue;
                };
                for ring in rings {
                    let Some(points) = ring.as_array() else {
                        continue;
                    };
                    for point in points {
                        if let Some(pair) = point.as_array() {
                            update_lonlat_bounds(pair, &mut bounds);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    bounds
}

pub(crate) fn parse_county_landuse_profile(path: &Path) -> Result<CountyLanduseProfile, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value =
        serde_json::from_str::<JsonValue>(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let features = value
        .get("features")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("{} is missing feature collection data", path.display()))?;
    let mut samples = Vec::<LanduseSample>::new();
    for feature in features {
        let Some(props) = feature.get("properties").and_then(|v| v.as_object()) else {
            continue;
        };
        let layer = props
            .get("feature_layer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if layer != "landuse" {
            continue;
        }
        let Some(class_name) = props.get("landuse_class").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((mix, intensity)) = landuse_class_profile(class_name) else {
            continue;
        };
        let Some(geometry) = feature.get("geometry") else {
            continue;
        };
        let Some((min_lon, min_lat, max_lon, max_lat)) = geometry_lonlat_bounds(geometry) else {
            continue;
        };
        let center_lon = (min_lon + max_lon) * 0.5;
        let center_lat = (min_lat + max_lat) * 0.5;
        if !center_lon.is_finite() || !center_lat.is_finite() {
            continue;
        }
        let (min_x, min_y) = lonlat_to_web_mercator_m(min_lon, min_lat);
        let (max_x, max_y) = lonlat_to_web_mercator_m(max_lon, max_lat);
        let bbox_area_m2 = ((max_x - min_x).abs() * (max_y - min_y).abs()).max(1.0);
        let size_weight = (bbox_area_m2.sqrt() / 120.0).clamp(0.35, 4.25);
        let (x_m, y_m) = lonlat_to_web_mercator_m(center_lon, center_lat);
        samples.push(LanduseSample {
            x_m,
            y_m,
            weight: size_weight,
            intensity,
            mix,
        });
    }
    Ok(CountyLanduseProfile { samples })
}

pub(crate) fn county_landuse_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let county = county_id.trim();
    if county.is_empty() {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    for dir_name in ["county_basemap_full", "county_basemap_mid"] {
        let path = pack_dir
            .join("map")
            .join(dir_name)
            .join(format!("{county}.geojson"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

pub(crate) fn load_county_landuse_profile(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Result<CountyLanduseProfile, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let county = county_id.trim().to_ascii_lowercase();
    if iso != "GB" || county.is_empty() {
        return Ok(CountyLanduseProfile::default());
    }
    let cache_key = format!("{iso}:{county}");
    {
        let cache = gb_county_landuse_cache()
            .lock()
            .map_err(|_| "county landuse cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    let profile = if let Some(path) = county_landuse_file(app, &iso, &county) {
        parse_county_landuse_profile(&path).unwrap_or_default()
    } else {
        CountyLanduseProfile::default()
    };
    gb_county_landuse_cache()
        .lock()
        .map_err(|_| "county landuse cache poisoned".to_string())?
        .insert(cache_key, profile.clone());
    Ok(profile)
}

pub(crate) fn estimate_legacy_demand_scale(scenario: &Scenario) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for cell in &scenario.world.demand_cells {
        let mass = cell.residents_night.max(0.0) + cell.jobs_day.max(0.0);
        if mass <= 0.0 || !mass.is_finite() {
            continue;
        }
        total += mass;
        count += 1;
    }
    if count == 0 {
        return 1.0;
    }
    let avg = total / count as f64;
    if !avg.is_finite() || avg <= 0.0 {
        return 1.0;
    }
    (120.0 / avg).clamp(1.0, 18.0).sqrt()
}

pub(crate) fn enrich_station_inspection_with_landuse(
    app: &AppHandle,
    scenario: &Scenario,
    inspection: &mut StationInspection,
) -> Result<(), String> {
    let (stop_mx, stop_my) =
        world_xy_to_web_mercator_m(&scenario.meta.crs, inspection.x, inspection.y);
    let (stop_lon, stop_lat) = web_mercator_m_to_lonlat(stop_mx, stop_my);
    if !stop_lon.is_finite() || !stop_lat.is_finite() {
        return Ok(());
    }
    let county_catalog = load_gb_county_boundaries()?;
    let Some(county) = county_for_lon_lat(&county_catalog.counties, stop_lon, stop_lat)
        .or_else(|| nearest_county_for_lon_lat(&county_catalog.counties, stop_lon, stop_lat))
    else {
        return Ok(());
    };
    let landuse = load_county_landuse_profile(app, "GB", &county.county_id)?;
    if landuse.samples.is_empty() {
        return Ok(());
    }

    let radius_m = inspection.catchment_radius_m.clamp(500.0, 2200.0);
    let sigma = (radius_m * 0.55).max(240.0);
    let radius2 = radius_m * radius_m;

    let mut selected = Vec::<(&LanduseSample, f64, f64)>::new();
    for sample in &landuse.samples {
        let dx = sample.x_m - stop_mx;
        let dy = sample.y_m - stop_my;
        let d2 = dx * dx + dy * dy;
        if d2 > radius2 {
            continue;
        }
        let gaussian = (-d2 / (2.0 * sigma * sigma)).exp();
        if gaussian <= 0.0 {
            continue;
        }
        let weight = gaussian * sample.weight * sample.intensity.max(0.15);
        if weight <= 0.0 {
            continue;
        }
        selected.push((sample, d2.sqrt(), weight));
    }
    if selected.is_empty() {
        let mut nearest = landuse
            .samples
            .iter()
            .map(|sample| {
                let dx = sample.x_m - stop_mx;
                let dy = sample.y_m - stop_my;
                let dist = (dx * dx + dy * dy).sqrt();
                (sample, dist)
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let scale = radius_m
            .max(nearest.first().map(|entry| entry.1).unwrap_or(1.0))
            .max(1.0);
        for (sample, dist) in nearest.into_iter().take(48) {
            let rel = dist / scale;
            let weight = (1.0 / (1.0 + rel * rel)) * sample.weight * sample.intensity.max(0.15);
            if weight <= 0.0 {
                continue;
            }
            selected.push((sample, dist, weight));
        }
    }
    if selected.is_empty() {
        return Ok(());
    }

    let mut mix_sum = [0.0_f64; 7];
    let mut weight_sum = 0.0_f64;
    let mut intensity_weighted = 0.0_f64;
    let mut max_dist = 0.0_f64;
    for (sample, dist, weight) in &selected {
        weight_sum += *weight;
        intensity_weighted += sample.intensity.max(0.0) * *weight;
        max_dist = max_dist.max(*dist);
        for (idx, value) in sample.mix.iter().enumerate() {
            mix_sum[idx] += value.max(0.0) * *weight;
        }
    }
    if weight_sum <= 0.0 {
        return Ok(());
    }
    let mut micro_mix = [0.0_f64; 7];
    for idx in 0..7 {
        micro_mix[idx] = mix_sum[idx] / weight_sum;
    }
    micro_mix = normalize_activity_mix(micro_mix);

    let mut base_mix = normalize_activity_mix([
        inspection.catchment_mix_residential,
        inspection.catchment_mix_office,
        inspection.catchment_mix_retail,
        inspection.catchment_mix_recreation,
        inspection.catchment_mix_industrial,
        inspection.catchment_mix_education,
        inspection.catchment_mix_health,
    ]);
    let blend = (weight_sum / (weight_sum + 22.0)).clamp(0.35, 0.9);
    for idx in 0..7 {
        base_mix[idx] = base_mix[idx] * (1.0 - blend) + micro_mix[idx] * blend;
    }
    base_mix = normalize_activity_mix(base_mix);

    let intensity_avg = (intensity_weighted / weight_sum).clamp(0.25, 3.4);
    let legacy_scale = estimate_legacy_demand_scale(scenario);
    let urban_scale = (0.55 + intensity_avg).clamp(0.65, 3.4);
    let total_scale = (legacy_scale * urban_scale).clamp(1.0, 12.0);
    let base_total = (inspection.catchment_residents + inspection.catchment_jobs).max(0.0);
    let total_estimate = if base_total > 0.0 {
        base_total * total_scale
    } else {
        weight_sum * 24.0
    };
    let jobs_pull = base_mix[1] * 1.0
        + base_mix[2] * 0.9
        + base_mix[4] * 0.95
        + base_mix[5] * 0.65
        + base_mix[6] * 0.6
        + base_mix[3] * 0.35
        + base_mix[0] * 0.15;
    let residents_pull = base_mix[0] * 1.05
        + base_mix[3] * 0.28
        + base_mix[2] * 0.2
        + base_mix[1] * 0.08
        + base_mix[4] * 0.05
        + base_mix[5] * 0.05
        + base_mix[6] * 0.05;
    let pull_sum = (jobs_pull + residents_pull).max(1e-6);
    let jobs_share = (jobs_pull / pull_sum).clamp(0.08, 0.92);
    let residents_share = 1.0 - jobs_share;
    let micro_jobs = (total_estimate * jobs_share).max(0.0);
    let micro_residents = (total_estimate * residents_share).max(0.0);

    inspection.catchment_mix_residential = base_mix[0];
    inspection.catchment_mix_office = base_mix[1];
    inspection.catchment_mix_retail = base_mix[2];
    inspection.catchment_mix_recreation = base_mix[3];
    inspection.catchment_mix_industrial = base_mix[4];
    inspection.catchment_mix_education = base_mix[5];
    inspection.catchment_mix_health = base_mix[6];
    inspection.catchment_residents =
        (inspection.catchment_residents * (1.0 - blend) + micro_residents * blend).max(0.0);
    inspection.catchment_jobs =
        (inspection.catchment_jobs * (1.0 - blend) + micro_jobs * blend).max(0.0);
    inspection.catchment_cells = inspection.catchment_cells.max(selected.len());
    inspection.catchment_radius_m = inspection.catchment_radius_m.max(max_dist).max(radius_m);
    Ok(())
}

pub(crate) fn gb_county_adjacency_map(counties: &[CountyBoundary]) -> HashMap<String, Vec<String>> {
    let mut adjacency = counties
        .iter()
        .map(|county| {
            (
                region_id_from_county("GB", &county.county_id),
                Vec::<String>::new(),
            )
        })
        .collect::<HashMap<_, _>>();
    for i in 0..counties.len() {
        for j in (i + 1)..counties.len() {
            let a = &counties[i];
            let b = &counties[j];
            if !a.geometry.intersects(&b.geometry) {
                continue;
            }
            let a_id = region_id_from_county("GB", &a.county_id);
            let b_id = region_id_from_county("GB", &b.county_id);
            adjacency
                .entry(a_id.clone())
                .or_default()
                .push(b_id.clone());
            adjacency.entry(b_id).or_default().push(a_id);
        }
    }
    for values in adjacency.values_mut() {
        values.sort();
        values.dedup();
    }
    adjacency
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
