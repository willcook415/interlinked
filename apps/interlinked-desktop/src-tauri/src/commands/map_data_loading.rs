use serde_json::Value as JsonValue;
use tauri::{command, AppHandle};

use super::super::{
    canonicalize_region_id, country_map_context_cache, empty_feature_collection_json,
    normalize_region_id, read_feature_collection_json, read_manifest, region_country_iso2,
    region_street_context_cache, world_context_from_countries_geojson, CountryMapContext,
    MapRuntimeConfig, RegionStreetContext,
};
use super::content_library::{primary_project_country_iso2, resolve_map_assets, MapRuntimeTier};
use crate::map_assets::server::ensure_map_asset_server;

fn extend_bounds_from_coords(value: &JsonValue, bounds: &mut Option<(f64, f64, f64, f64)>) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.len() == 2 {
        if let (Some(lon), Some(lat)) = (items[0].as_f64(), items[1].as_f64()) {
            if lon.is_finite() && lat.is_finite() {
                match bounds.as_mut() {
                    Some((min_lon, min_lat, max_lon, max_lat)) => {
                        *min_lon = (*min_lon).min(lon);
                        *min_lat = (*min_lat).min(lat);
                        *max_lon = (*max_lon).max(lon);
                        *max_lat = (*max_lat).max(lat);
                    }
                    None => {
                        *bounds = Some((lon, lat, lon, lat));
                    }
                }
            }
            return;
        }
    }
    for child in items {
        extend_bounds_from_coords(child, bounds);
    }
}

fn default_bounds_from_world_context(
    assets: &super::content_library::ResolvedMapAssets,
    iso: &str,
) -> Option<[[f64; 2]; 2]> {
    let world_path = assets.world_context.as_ref()?.path.clone();
    let mut world_context = read_feature_collection_json(&world_path).ok()?;
    if assets.world_context_requires_country_remap() {
        world_context = world_context_from_countries_geojson(world_context).ok()?;
    }
    let features = world_context.get("features")?.as_array()?;
    let target_iso = iso.trim().to_ascii_uppercase();
    let mut target_bounds: Option<(f64, f64, f64, f64)> = None;
    let mut fallback_bounds: Option<(f64, f64, f64, f64)> = None;

    for feature in features {
        let Some(geometry) = feature.get("geometry") else {
            continue;
        };
        let Some(coords) = geometry.get("coordinates") else {
            continue;
        };
        extend_bounds_from_coords(coords, &mut fallback_bounds);
        let feature_iso = feature
            .get("properties")
            .and_then(|props| props.get("country_iso2"))
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_ascii_uppercase());
        if feature_iso.as_deref() == Some(target_iso.as_str()) {
            extend_bounds_from_coords(coords, &mut target_bounds);
        }
    }

    let (min_lon, min_lat, max_lon, max_lat) = target_bounds.or(fallback_bounds)?;
    Some([[min_lon, min_lat], [max_lon, max_lat]])
}

#[command]
pub fn load_map_runtime_config(
    app: AppHandle,
    project_path: String,
) -> Result<MapRuntimeConfig, String> {
    let project_root = std::path::PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let iso = primary_project_country_iso2(&manifest)
        .ok_or_else(|| "project has no country context".to_string())?;
    let server = ensure_map_asset_server(&app)?;
    let assets = resolve_map_assets(&app, &iso);
    if assets.world_context.is_none() {
        return Err("world context asset missing".to_string());
    }
    let map_tier = assets.runtime_tier();
    let vector_ready = matches!(map_tier, MapRuntimeTier::VectorMbtilesV1);
    let has_mid = assets.has_mid_basemap();
    let has_full = assets.has_full_basemap();
    let has_county_roads = assets.has_county_roads();
    let default_bounds = default_bounds_from_world_context(&assets, &iso);
    Ok(MapRuntimeConfig {
        country_iso2: iso.clone(),
        style_url: vector_ready.then(|| {
            format!(
                "{}/map/{}/style.json",
                server.base_url,
                iso.to_ascii_lowercase()
            )
        }),
        world_context_url: format!("{}/map/{}/world_context.geojson", server.base_url, iso),
        counties_url: (!vector_ready)
            .then_some(())
            .and_then(|_| assets.counties.as_ref())
            .map(|_| format!("{}/map/{}/counties.geojson", server.base_url, iso)),
        major_roads_url: (!vector_ready)
            .then_some(())
            .and_then(|_| assets.major_roads.as_ref())
            .map(|_| format!("{}/map/{}/major_roads.geojson", server.base_url, iso)),
        county_basemap_mid_url_template: (!vector_ready && has_mid).then(|| {
            format!(
                "{}/map/{}/county_basemap_mid/{{county_id}}.geojson",
                server.base_url, iso
            )
        }),
        county_basemap_full_url_template: (!vector_ready && has_full).then(|| {
            format!(
                "{}/map/{}/county_basemap_full/{{county_id}}.geojson",
                server.base_url, iso
            )
        }),
        county_roads_url_template: (!vector_ready && has_county_roads).then(|| {
            format!(
                "{}/map/{}/county_roads/{{county_id}}.geojson",
                server.base_url, iso
            )
        }),
        default_bounds,
        map_pack_version: Some(map_tier.as_version().to_string()),
        map_ready: map_tier.map_ready(),
    })
}

#[command]
pub fn load_country_map_context(
    app: AppHandle,
    project_path: String,
) -> Result<CountryMapContext, String> {
    let project_root = std::path::PathBuf::from(project_path);
    let manifest = read_manifest(&project_root)?;
    let iso = primary_project_country_iso2(&manifest)
        .ok_or_else(|| "project has no country context".to_string())?;

    {
        let cache = country_map_context_cache()
            .lock()
            .map_err(|_| "country_map_context cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&iso) {
            return Ok(cached.clone());
        }
    }

    let assets = resolve_map_assets(&app, &iso);
    let world_path = assets
        .world_context
        .as_ref()
        .map(|entry| entry.path.clone())
        .ok_or_else(|| "world context asset missing".to_string())?;
    let major_path = assets.major_roads.as_ref().map(|entry| entry.path.clone());
    let mut world_context = read_feature_collection_json(&world_path)?;
    if assets.world_context_requires_country_remap() {
        world_context = world_context_from_countries_geojson(world_context)?;
    }
    let major_roads = match major_path {
        Some(path) => {
            read_feature_collection_json(&path).unwrap_or_else(|_| empty_feature_collection_json())
        }
        None => empty_feature_collection_json(),
    };
    let default_bounds = default_bounds_from_world_context(&assets, &iso);
    let context = CountryMapContext {
        country_iso2: iso.clone(),
        world_context,
        major_roads,
        default_bounds,
    };
    country_map_context_cache()
        .lock()
        .map_err(|_| "country_map_context cache poisoned".to_string())?
        .insert(iso, context.clone());
    Ok(context)
}

#[command]
pub fn load_region_street_context(
    app: AppHandle,
    _project_path: String,
    region_id: String,
) -> Result<RegionStreetContext, String> {
    let normalized =
        normalize_region_id(&region_id).ok_or_else(|| "invalid region_id".to_string())?;
    let normalized =
        canonicalize_region_id(&normalized).ok_or_else(|| "invalid region_id".to_string())?;
    {
        let cache = region_street_context_cache()
            .lock()
            .map_err(|_| "region_street_context cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&normalized) {
            return Ok(cached.clone());
        }
    }
    let iso =
        region_country_iso2(&normalized).ok_or_else(|| "region country missing".to_string())?;
    let assets = resolve_map_assets(&app, &iso);
    let roads = if normalized.starts_with("county:") {
        let Some(county_id) = normalized.split(':').nth(2) else {
            return Err("region county token missing".to_string());
        };
        match assets.county_roads_file(county_id) {
            Some(path) => read_feature_collection_json(&path)
                .unwrap_or_else(|_| empty_feature_collection_json()),
            None => empty_feature_collection_json(),
        }
    } else {
        empty_feature_collection_json()
    };
    let context = RegionStreetContext {
        region_id: normalized.clone(),
        country_iso2: iso,
        roads,
    };
    region_street_context_cache()
        .lock()
        .map_err(|_| "region_street_context cache poisoned".to_string())?
        .insert(normalized, context.clone());
    Ok(context)
}
