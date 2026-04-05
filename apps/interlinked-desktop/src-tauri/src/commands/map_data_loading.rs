use tauri::{command, AppHandle};

use super::super::{
    canonicalize_region_id, counties_bounds, country_map_context_cache,
    empty_feature_collection_json, load_gb_county_boundaries, normalize_region_id,
    read_feature_collection_json, read_manifest, region_country_iso2, region_street_context_cache,
    world_context_from_countries_geojson, CountryMapContext, MapRuntimeConfig, RegionStreetContext,
};
use super::content_library::{
    basemap_file, counties_file, country_pack_dir, county_basemap_full_dir, county_basemap_mid_dir,
    county_roads_file, major_roads_file, primary_project_country_iso2, style_template_file,
    world_context_file,
};
use crate::map_assets::server::ensure_map_asset_server;

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
    let world_path =
        world_context_file(&app, &iso).ok_or_else(|| "world context asset missing".to_string())?;
    let vector_ready = world_path.exists()
        && counties_file(&app, &iso).is_some()
        && basemap_file(&app, &iso).is_some()
        && style_template_file(&app, &iso).is_some();
    let has_mid = county_basemap_mid_dir(&app, &iso).is_some()
        || country_pack_dir(&app, &iso)
            .map(|dir| dir.join("map").join("county_roads").exists())
            .unwrap_or(false);
    let has_full = county_basemap_full_dir(&app, &iso).is_some()
        || country_pack_dir(&app, &iso)
            .map(|dir| dir.join("map").join("county_roads").exists())
            .unwrap_or(false);
    let default_bounds = if iso == "GB" {
        load_gb_county_boundaries()
            .ok()
            .and_then(|catalog| counties_bounds(&catalog.counties))
    } else {
        None
    };
    let legacy_ready = world_path.exists() && (has_mid || has_full);
    let map_ready = vector_ready || legacy_ready;
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
        counties_url: counties_file(&app, &iso)
            .map(|_| format!("{}/map/{}/counties.geojson", server.base_url, iso)),
        major_roads_url: (!vector_ready)
            .then_some(())
            .and_then(|_| major_roads_file(&app, &iso))
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
        default_bounds,
        map_pack_version: Some(
            if vector_ready {
                "vector-mbtiles-v1"
            } else if county_basemap_full_dir(&app, &iso).is_some()
                || county_basemap_mid_dir(&app, &iso).is_some()
            {
                "geojson-basemap-v2"
            } else {
                "geojson-roads-v1"
            }
            .to_string(),
        ),
        map_ready,
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

    let world_path =
        world_context_file(&app, &iso).ok_or_else(|| "world context asset missing".to_string())?;
    let major_path = major_roads_file(&app, &iso);
    let mut world_context = read_feature_collection_json(&world_path)?;
    if world_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("countries.geojson"))
        .unwrap_or(false)
    {
        world_context = world_context_from_countries_geojson(world_context)?;
    }
    let major_roads = match major_path {
        Some(path) => {
            read_feature_collection_json(&path).unwrap_or_else(|_| empty_feature_collection_json())
        }
        None => empty_feature_collection_json(),
    };
    let default_bounds = if iso == "GB" {
        load_gb_county_boundaries()
            .ok()
            .and_then(|catalog| counties_bounds(&catalog.counties))
    } else {
        None
    };
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
    let county_id = normalized
        .split(':')
        .nth(2)
        .ok_or_else(|| "region county token missing".to_string())?;
    let roads = if normalized.starts_with("county:") {
        match county_roads_file(&app, &iso, county_id) {
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
