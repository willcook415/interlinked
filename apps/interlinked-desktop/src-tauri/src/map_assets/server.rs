use rusqlite::{params, Connection, OpenFlags};
use serde_json::Value as JsonValue;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use tauri::AppHandle;

use super::{MapAssetServer, MAP_ASSET_SERVER};
use crate::commands::content_library::{
    basemap_file, counties_file, county_basemap_full_file, county_basemap_mid_file,
    major_roads_file, style_template_file, world_context_file,
};
use crate::{read_json_file, world_context_from_countries_geojson};

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
    let style_path = style_template_file(app, country_iso2)
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
    Ok(content.into_bytes())
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
            let y = y_file
                .strip_suffix(".pbf")
                .unwrap_or(y_file)
                .parse::<u32>()
                .map_err(|_| "invalid tile y".to_string())?;
            let z = z.parse::<u32>().map_err(|_| "invalid tile z".to_string())?;
            let x = x.parse::<u32>().map_err(|_| "invalid tile x".to_string())?;
            let file = basemap_file(app, iso).ok_or_else(|| "basemap not found".to_string())?;
            read_mbtiles_tile(&file, z, x, y)
                .map(|bytes| (bytes, "application/vnd.mapbox-vector-tile"))
        }
        ["map", iso, "world_context.geojson"] => {
            let file = world_context_file(app, iso).ok_or_else(|| "asset not found".to_string())?;
            let value = if file
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("countries.geojson"))
                .unwrap_or(false)
            {
                world_context_from_countries_geojson(read_json_file::<JsonValue>(&file)?)?
            } else {
                read_json_file::<JsonValue>(&file)?
            };
            let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "counties.geojson"] => {
            let file = counties_file(app, iso).ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "major_roads.geojson"] => {
            let file = major_roads_file(app, iso).ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "county_basemap_mid", file_name] => {
            let county_id = file_name.strip_suffix(".geojson").unwrap_or(file_name);
            let file = county_basemap_mid_file(app, iso, county_id)
                .ok_or_else(|| "asset not found".to_string())?;
            let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok((bytes, "application/geo+json"))
        }
        ["map", iso, "county_basemap_full", file_name] => {
            let county_id = file_name.strip_suffix(".geojson").unwrap_or(file_name);
            let file = county_basemap_full_file(app, iso, county_id)
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
