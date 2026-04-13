use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use tauri::AppHandle;

use super::{MapAssetServer, MAP_ASSET_SERVER};
use crate::commands::content_library::resolve_map_assets;
use crate::{is_uk_country_iso2, read_json_file, world_context_from_countries_geojson};

fn extract_uk_multipolygon_from_counties(path: &Path) -> Result<Option<JsonValue>, String> {
    let value = read_json_file::<JsonValue>(path)?;
    let Some(features) = value.get("features").and_then(|value| value.as_array()) else {
        return Ok(None);
    };
    let mut polygons = Vec::<JsonValue>::new();
    for feature in features {
        let props = feature.get("properties").and_then(|value| value.as_object());
        let country_iso2 = props
            .and_then(|props| props.get("country_iso2"))
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_ascii_uppercase());
        if let Some(iso2) = country_iso2 {
            if !is_uk_country_iso2(&iso2) {
                continue;
            }
        }
        let Some(geometry) = feature.get("geometry").and_then(|value| value.as_object()) else {
            continue;
        };
        let geometry_type = geometry
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let Some(coords) = geometry.get("coordinates").and_then(|value| value.as_array()) else {
            continue;
        };
        match geometry_type {
            "Polygon" => polygons.push(JsonValue::Array(coords.clone())),
            "MultiPolygon" => {
                for polygon in coords {
                    if let Some(polygon_coords) = polygon.as_array() {
                        polygons.push(JsonValue::Array(polygon_coords.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    if polygons.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::json!({
        "type": "MultiPolygon",
        "coordinates": polygons,
    })))
}

fn inject_uk_world_geometry(
    mut world_context: JsonValue,
    counties_file: Option<&Path>,
) -> Result<JsonValue, String> {
    let Some(counties_path) = counties_file else {
        return Ok(world_context);
    };
    let Some(uk_geometry) = extract_uk_multipolygon_from_counties(counties_path)? else {
        return Ok(world_context);
    };
    let Some(features) = world_context
        .get_mut("features")
        .and_then(|value| value.as_array_mut())
    else {
        return Ok(world_context);
    };
    let mut replaced = false;
    for feature in features.iter_mut() {
        let country_iso2 = feature
            .get("properties")
            .and_then(|value| value.as_object())
            .and_then(|props| props.get("country_iso2"))
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_ascii_uppercase())
            .unwrap_or_default();
        if !is_uk_country_iso2(&country_iso2) {
            continue;
        }
        if let Some(geometry) = feature.get_mut("geometry") {
            *geometry = uk_geometry.clone();
            replaced = true;
        }
        if let Some(properties) = feature.get_mut("properties").and_then(|value| value.as_object_mut()) {
            properties.insert("country_iso2".to_string(), JsonValue::String("UK".to_string()));
            properties.entry("name".to_string()).or_insert(JsonValue::String("United Kingdom".to_string()));
        }
    }
    if !replaced {
        features.push(serde_json::json!({
            "type": "Feature",
            "geometry": uk_geometry,
            "properties": {
                "country_iso2": "UK",
                "name": "United Kingdom",
                "playable_now": true,
                "coming_soon": false
            }
        }));
    }
    Ok(world_context)
}

fn write_http_response(
    mut stream: TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .map_err(|e| e.to_string())?;
    if !head_only {
        stream.write_all(body).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_mbtiles_tile(path: &Path, z: u32, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let max_index = 2_u32
        .checked_pow(z)
        .ok_or_else(|| "tile zoom out of range".to_string())?;
    if x >= max_index || y >= max_index {
        return Err("tile coordinate out of range".to_string());
    }
    let tms_y = i64::from(max_index - 1 - y);
    let mut stmt = conn
        .prepare("SELECT tile_data FROM tiles WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params![i64::from(z), i64::from(x), tms_y])
        .map_err(|e| e.to_string())?;
    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Err("tile not found".to_string());
    };
    row.get::<usize, Vec<u8>>(0).map_err(|e| e.to_string())
}

fn build_runtime_style_json(app: &AppHandle, country_iso2: &str) -> Result<Vec<u8>, String> {
    let iso = country_iso2.trim().to_ascii_lowercase();
    let assets = resolve_map_assets(app, country_iso2);
    let style_path = assets
        .style_template
        .as_ref()
        .map(|entry| entry.path.clone())
        .ok_or_else(|| format!("style template missing for {country_iso2}"))?;
    let server = MAP_ASSET_SERVER
        .get()
        .cloned()
        .ok_or_else(|| "map asset server unavailable".to_string())?;
    let template =
        fs::read_to_string(&style_path).map_err(|e| format!("{}: {e}", style_path.display()))?;
    let content = template
        .replace("__BASE_URL__", &server.base_url)
        .replace("__COUNTRY_ISO2__", &iso);
    let mut style_json: JsonValue = serde_json::from_str(&content)
        .map_err(|e| format!("{}: style parse error: {e}", style_path.display()))?;

    if is_uk_country_iso2(country_iso2) {
        if let Some(sources) = style_json
            .get_mut("sources")
            .and_then(|v| v.as_object_mut())
        {
            for source in sources.values_mut() {
                let is_vector = source
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|v| v.eq_ignore_ascii_case("vector"))
                    .unwrap_or(false);
                if !is_vector {
                    continue;
                }
                source["bounds"] = serde_json::json!([-11.5, 49.3, 2.2, 61.2]);
            }
        }
        if let Some(layers) = style_json.get_mut("layers").and_then(|v| v.as_array_mut()) {
            for layer in layers {
                let id = layer.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                if id != "background" {
                    continue;
                }
                layer["paint"]["background-color"] = JsonValue::String("#c6ddf3".to_string());
            }
        }
    }

    serde_json::to_vec(&style_json).map_err(|e| e.to_string())
}

fn map_asset_response_bytes(
    app: &AppHandle,
    path: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    let path = path.trim();
    if path == "/health" {
        return Ok((br#"{"ok":true}"#.to_vec(), "application/json"));
    }
    let parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["map", iso, "style.json"] => {
            build_runtime_style_json(app, iso).map(|bytes| (bytes, "application/json"))
        }
        ["map", iso, "tiles", z, x, y_file] => {
            let assets = resolve_map_assets(app, iso);
            let y = y_file
                .strip_suffix(".pbf")
                .unwrap_or(y_file)
                .parse::<u32>()
                .map_err(|_| "invalid tile y".to_string())?;
            let z = z.parse::<u32>().map_err(|_| "invalid tile z".to_string())?;
            let x = x.parse::<u32>().map_err(|_| "invalid tile x".to_string())?;
            let file = assets
                .basemap_mbtiles
                .as_ref()
                .map(|entry| entry.path.clone())
                .ok_or_else(|| "basemap not found".to_string())?;
            read_mbtiles_tile(&file, z, x, y)
                .map(|bytes| (bytes, "application/vnd.mapbox-vector-tile"))
        }
        ["map", iso, "world_context.geojson"] => {
            let assets = resolve_map_assets(app, iso);
            let file = assets
                .world_context
                .as_ref()
                .map(|entry| entry.path.clone())
                .ok_or_else(|| "asset not found".to_string())?;
            let mut value = if assets.world_context_requires_country_remap() {
                world_context_from_countries_geojson(read_json_file::<JsonValue>(&file)?)?
            } else {
                read_json_file::<JsonValue>(&file)?
            };
            if is_uk_country_iso2(iso) {
                value = inject_uk_world_geometry(
                    value,
                    assets.counties.as_ref().map(|entry| entry.path.as_path()),
                )?;
            }
            let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "counties.geojson"] => {
            let assets = resolve_map_assets(app, iso);
            let file = assets
                .counties
                .as_ref()
                .map(|entry| entry.path.clone())
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "major_roads.geojson"] => {
            let assets = resolve_map_assets(app, iso);
            let file = assets
                .major_roads
                .as_ref()
                .map(|entry| entry.path.clone())
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "county_roads", file_name] => {
            let assets = resolve_map_assets(app, iso);
            let county_id = file_name.strip_suffix(".geojson").unwrap_or(file_name);
            let file = assets
                .county_roads_file(county_id)
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "county_basemap_mid", file_name] => {
            let assets = resolve_map_assets(app, iso);
            let county_id = file_name.strip_suffix(".geojson").unwrap_or(file_name);
            let file = assets
                .county_basemap_mid_file(county_id)
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "county_basemap_full", file_name] => {
            let assets = resolve_map_assets(app, iso);
            let county_id = file_name.strip_suffix(".geojson").unwrap_or(file_name);
            let file = assets
                .county_basemap_full_file(county_id)
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        _ => Err("asset not found".to_string()),
    }
}

fn handle_map_asset_request(stream: TcpStream, app: &AppHandle) -> Result<(), String> {
    let reader_stream = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;
    let parts = request_line.split_whitespace().collect::<Vec<_>>();
    let method = parts.first().copied().unwrap_or("");
    let request_path = parts
        .get(1)
        .map(|value| value.split('?').next().unwrap_or("/"))
        .unwrap_or("/");
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    match method {
        "GET" | "HEAD" => match map_asset_response_bytes(app, request_path) {
            Ok((body, content_type)) => {
                write_http_response(stream, 200, content_type, &body, method == "HEAD")
            }
            Err(_) => write_http_response(
                stream,
                404,
                "text/plain; charset=utf-8",
                b"not found",
                method == "HEAD",
            ),
        },
        _ => write_http_response(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"method not allowed",
            false,
        ),
    }
}

pub(crate) fn ensure_map_asset_server(app: &AppHandle) -> Result<MapAssetServer, String> {
    if let Some(existing) = MAP_ASSET_SERVER.get() {
        return Ok(existing.clone());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let server = MapAssetServer {
        base_url: format!("http://127.0.0.1:{}", addr.port()),
    };
    let app_handle = app.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let app_handle = app_handle.clone();
            thread::spawn(move || {
                let _ = handle_map_asset_request(stream, &app_handle);
            });
        }
    });
    let _ = MAP_ASSET_SERVER.set(server.clone());
    Ok(MAP_ASSET_SERVER.get().cloned().unwrap_or(server))
}
