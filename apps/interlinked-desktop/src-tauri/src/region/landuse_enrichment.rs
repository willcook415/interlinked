use crate::commands::content_library::resolve_map_assets;
use crate::*;
use serde_json::value::RawValue;

#[derive(Deserialize)]
struct CountyLanduseFeatureCollection<'a> {
    #[serde(borrow)]
    features: Vec<CountyLanduseFeature<'a>>,
}

#[derive(Default, Deserialize)]
struct CountyLanduseFeatureProps<'a> {
    #[serde(default, borrow)]
    feature_layer: Option<&'a str>,
    #[serde(default, borrow)]
    landuse_class: Option<&'a str>,
}

#[derive(Deserialize)]
struct CountyLanduseFeature<'a> {
    #[serde(default, borrow)]
    properties: CountyLanduseFeatureProps<'a>,
    #[serde(default, borrow)]
    geometry: Option<&'a RawValue>,
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

pub(crate) fn parse_county_landuse_profile(path: &Path) -> Result<CountyLanduseProfile, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let feature_collection = serde_json::from_str::<CountyLanduseFeatureCollection<'_>>(&raw)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut samples = Vec::<LanduseSample>::new();
    for feature in feature_collection.features {
        let layer = feature
            .properties
            .feature_layer
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if layer != "landuse" {
            continue;
        }
        let Some(class_name) = feature.properties.landuse_class else {
            continue;
        };
        let Some((mix, intensity)) = landuse_class_profile(class_name) else {
            continue;
        };
        let Some(geometry_raw) = feature.geometry else {
            continue;
        };
        let Ok(geometry) = serde_json::from_str::<JsonValue>(geometry_raw.get()) else {
            continue;
        };
        let Some((min_lon, min_lat, max_lon, max_lat)) = geometry_lonlat_bounds(&geometry) else {
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
    let assets = resolve_map_assets(app, &iso);
    for dir in [
        assets.county_basemap_full_dir.as_ref(),
        assets.county_basemap_mid_dir.as_ref(),
    ] {
        let Some(dir) = dir else { continue };
        let path = dir.path.join(format!("{county}.geojson"));
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
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let county = county_id.trim().to_ascii_lowercase();
    if !is_uk_country_iso2(&iso) || county.is_empty() {
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
    let landuse = load_county_landuse_profile(app, CANONICAL_UK_ISO2, &county.county_id)?;
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
