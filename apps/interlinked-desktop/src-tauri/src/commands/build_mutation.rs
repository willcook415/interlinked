use super::super::*;
use super::content_library::{
    county_basemap_full_file, county_basemap_mid_file, county_roads_file,
    primary_project_country_iso2,
};
use crate::builder_support::{StationRuntimeDiagnostics, StationRuntimeServiceDiagnostics};
use crate::contracts::CounterProvenance;
use std::time::Instant;
use tauri::{command, AppHandle};

fn build_perf_log(label: &str, started: Instant) {
    eprintln!("[build-perf] {label}: {}ms", started.elapsed().as_millis());
}

#[command]
pub fn load_build_defaults() -> Result<BuildDefaults, String> {
    let started = Instant::now();
    let defaults = default_build_defaults(&economy_config());
    build_perf_log("command.load_build_defaults", started);
    Ok(defaults)
}

fn county_iso_and_id_from_region_id(region_id: &str) -> Option<(String, String)> {
    let mut parts = region_id.split(':');
    let tier = parts.next()?.trim();
    let iso = parts.next()?.trim().to_ascii_uppercase();
    let county_id = parts.next()?.trim().to_ascii_lowercase();
    if !tier.eq_ignore_ascii_case("county") || iso.len() != 2 || county_id.is_empty() {
        return None;
    }
    Some((iso, county_id))
}

pub(crate) fn world_xy_to_lonlat_safe(crs: &Crs, x: f64, y: f64) -> Option<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let (mx, my) = world_xy_to_web_mercator_m(crs, x, y);
    let (lon, lat) = web_mercator_m_to_lonlat(mx, my);
    if lon.is_finite() && lat.is_finite() {
        Some((lon, lat))
    } else {
        None
    }
}

fn link_path_lonlat(
    link: &Link,
    stop_xy_by_id: &HashMap<String, (f64, f64)>,
    crs: &Crs,
) -> Vec<(f64, f64)> {
    let mut out = Vec::<(f64, f64)>::new();
    if let Some(geometry) = link.geometry.as_ref() {
        for point in geometry {
            if point.len() < 2 {
                continue;
            }
            if let Some((lon, lat)) = world_xy_to_lonlat_safe(crs, point[0], point[1]) {
                if out.last().copied() != Some((lon, lat)) {
                    out.push((lon, lat));
                }
            }
        }
    }
    if out.len() >= 2 {
        return out;
    }
    let endpoint_ids = [link.from_stop.as_str(), link.to_stop.as_str()];
    for stop_id in endpoint_ids {
        let Some((x, y)) = stop_xy_by_id.get(stop_id).copied() else {
            continue;
        };
        if let Some((lon, lat)) = world_xy_to_lonlat_safe(crs, x, y) {
            if out.last().copied() != Some((lon, lat)) {
                out.push((lon, lat));
            }
        }
    }
    out
}

fn path_hits_county(path: &[(f64, f64)], county: &CountyBoundary) -> bool {
    if path.is_empty() {
        return false;
    }
    for (lon, lat) in path {
        if county.geometry.contains(&Point::new(*lon, *lat)) {
            return true;
        }
    }
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let line = Line::new(Coord { x: a.0, y: a.1 }, Coord { x: b.0, y: b.1 });
        if county.geometry.intersects(&line) {
            return true;
        }
    }
    false
}

fn is_drivable_road_class(road_class: &str) -> bool {
    let class = road_class.trim().to_ascii_lowercase();
    !matches!(
        class.as_str(),
        "pedestrian" | "footway" | "cycleway" | "path" | "bridleway" | "steps" | "track"
    )
}

pub(crate) fn geo_segment_from_points(a: (f64, f64), b: (f64, f64)) -> Option<GeoSegment> {
    if !a.0.is_finite()
        || !a.1.is_finite()
        || !b.0.is_finite()
        || !b.1.is_finite()
        || ((a.0 - b.0).abs() < 1e-12 && (a.1 - b.1).abs() < 1e-12)
    {
        return None;
    }
    Some(GeoSegment {
        a_lon: a.0,
        a_lat: a.1,
        b_lon: b.0,
        b_lat: b.1,
        min_lon: a.0.min(b.0),
        min_lat: a.1.min(b.1),
        max_lon: a.0.max(b.0),
        max_lat: a.1.max(b.1),
    })
}

fn collect_linestring_segments(coords: &[Vec<f64>], out: &mut Vec<GeoSegment>) {
    for pair in coords.windows(2) {
        if pair[0].len() < 2 || pair[1].len() < 2 {
            continue;
        }
        if let Some(seg) =
            geo_segment_from_points((pair[0][0], pair[0][1]), (pair[1][0], pair[1][1]))
        {
            out.push(seg);
        }
    }
}

fn collect_polygon_ring_segments(ring: &[Vec<f64>], out: &mut Vec<GeoSegment>) {
    let mut points = ring
        .iter()
        .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return;
    }
    if points.first() != points.last() {
        if let Some(first) = points.first().copied() {
            points.push(first);
        }
    }
    for pair in points.windows(2) {
        if let Some(seg) = geo_segment_from_points(pair[0], pair[1]) {
            out.push(seg);
        }
    }
}

fn parse_county_mode_constraints_geojson(
    path: &Path,
    include_roads: bool,
    include_water: bool,
) -> Result<CountyModeConstraintData, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let geojson = raw
        .parse::<GeoJson>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let GeoJson::FeatureCollection(fc) = geojson else {
        return Err(format!("{} is not a FeatureCollection", path.display()));
    };
    let mut out = CountyModeConstraintData::default();
    for feature in fc.features {
        let Some(geometry) = feature.geometry else {
            continue;
        };
        let layer = feature
            .properties
            .as_ref()
            .and_then(|props| props.get("feature_layer"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if include_roads && layer == "road" {
            let road_class = feature
                .properties
                .as_ref()
                .and_then(|props| props.get("road_class"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !is_drivable_road_class(road_class) {
                continue;
            }
            match geometry.value {
                GeoJsonValue::LineString(coords) => {
                    collect_linestring_segments(&coords, &mut out.road_segments)
                }
                GeoJsonValue::MultiLineString(lines) => {
                    for coords in lines {
                        collect_linestring_segments(&coords, &mut out.road_segments);
                    }
                }
                _ => {}
            }
        } else if include_water && layer == "water" {
            match geometry.value {
                GeoJsonValue::Polygon(coords) => {
                    for ring in &coords {
                        collect_polygon_ring_segments(ring, &mut out.water_segments);
                    }
                    if let Some(poly) = geojson_coords_to_polygon(&coords) {
                        out.water_polygons.push(MultiPolygon(vec![poly]));
                    }
                }
                GeoJsonValue::MultiPolygon(multi) => {
                    for coords in multi {
                        for ring in &coords {
                            collect_polygon_ring_segments(ring, &mut out.water_segments);
                        }
                        if let Some(poly) = geojson_coords_to_polygon(&coords) {
                            out.water_polygons.push(MultiPolygon(vec![poly]));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

fn load_county_mode_constraints(
    app: &AppHandle,
    county_id: &str,
) -> Result<Arc<CountyModeConstraintData>, String> {
    let county = county_id.trim().to_ascii_lowercase();
    if county.is_empty() {
        return Ok(Arc::new(CountyModeConstraintData::default()));
    }
    {
        let cache = gb_county_mode_constraint_cache()
            .lock()
            .map_err(|_| "county mode constraint cache poisoned".to_string())?;
        if let Some(cached) = cache.get(&county) {
            return Ok(cached.clone());
        }
    }

    let roads_path = county_roads_file(app, CANONICAL_UK_ISO2, &county)
        .or_else(|| county_basemap_full_file(app, CANONICAL_UK_ISO2, &county))
        .or_else(|| county_basemap_mid_file(app, CANONICAL_UK_ISO2, &county));
    let water_path = county_basemap_full_file(app, CANONICAL_UK_ISO2, &county)
        .or_else(|| county_basemap_mid_file(app, CANONICAL_UK_ISO2, &county));

    let mut merged = CountyModeConstraintData::default();
    if let Some(path) = roads_path.as_ref() {
        let include_water = water_path
            .as_ref()
            .map(|candidate| candidate == path)
            .unwrap_or(false);
        let parsed = parse_county_mode_constraints_geojson(path, true, include_water)?;
        merged.road_segments.extend(parsed.road_segments);
        merged.water_polygons.extend(parsed.water_polygons);
        merged.water_segments.extend(parsed.water_segments);
    }
    if let Some(path) = water_path.as_ref() {
        let already_loaded = roads_path
            .as_ref()
            .map(|candidate| candidate == path)
            .unwrap_or(false);
        if !already_loaded {
            let parsed = parse_county_mode_constraints_geojson(path, false, true)?;
            merged.water_polygons.extend(parsed.water_polygons);
            merged.water_segments.extend(parsed.water_segments);
        }
    }

    let shared = Arc::new(merged);
    gb_county_mode_constraint_cache()
        .lock()
        .map_err(|_| "county mode constraint cache poisoned".to_string())?
        .insert(county, shared.clone());
    Ok(shared)
}

fn lonlat_distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let avg_lat_rad = ((a.1 + b.1) * 0.5 * std::f64::consts::PI) / 180.0;
    let dx = (b.0 - a.0) * 111_320.0 * avg_lat_rad.cos().abs().max(0.2);
    let dy = (b.1 - a.1) * 110_540.0;
    (dx * dx + dy * dy).sqrt()
}

fn sample_path_points(path: &[(f64, f64)], step_m: f64) -> Vec<(f64, f64)> {
    if path.is_empty() {
        return vec![];
    }
    let mut out = vec![path[0]];
    let step = step_m.max(25.0);
    for segment in path.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let len = lonlat_distance_m(a, b);
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        let samples = (len / step).ceil().max(1.0) as usize;
        for idx in 1..=samples {
            let t = idx as f64 / samples as f64;
            let point = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if out.last().copied() != Some(point) {
                out.push(point);
            }
        }
    }
    out
}

fn point_to_segment_distance_m(lon: f64, lat: f64, seg: &GeoSegment) -> f64 {
    let lon_scale = 111_320.0 * lat.to_radians().cos().abs().max(0.2);
    let lat_scale = 110_540.0;
    let ax = seg.a_lon * lon_scale;
    let ay = seg.a_lat * lat_scale;
    let bx = seg.b_lon * lon_scale;
    let by = seg.b_lat * lat_scale;
    let px = lon * lon_scale;
    let py = lat * lat_scale;
    let abx = bx - ax;
    let aby = by - ay;
    let apx = px - ax;
    let apy = py - ay;
    let ab2 = abx * abx + aby * aby;
    if ab2 <= 1e-9 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0);
    let cx = ax + abx * t;
    let cy = ay + aby * t;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn point_near_any_segment_m(lon: f64, lat: f64, segments: &[GeoSegment], threshold_m: f64) -> bool {
    if segments.is_empty() {
        return false;
    }
    let lon_tol = threshold_m / (111_320.0 * lat.to_radians().cos().abs().max(0.2));
    let lat_tol = threshold_m / 110_540.0;
    for seg in segments {
        if lon < seg.min_lon - lon_tol
            || lon > seg.max_lon + lon_tol
            || lat < seg.min_lat - lat_tol
            || lat > seg.max_lat + lat_tol
        {
            continue;
        }
        if point_to_segment_distance_m(lon, lat, seg) <= threshold_m {
            return true;
        }
    }
    false
}

fn county_ids_for_path(path: &[(f64, f64)], counties: &[CountyBoundary]) -> HashSet<String> {
    let mut out = HashSet::<String>::new();
    for county in counties {
        if path_hits_county(path, county) {
            out.insert(county.county_id.clone());
        }
    }
    if out.is_empty() {
        if let Some((lon, lat)) = path.first().copied() {
            if let Some(county) = county_for_lon_lat(counties, lon, lat)
                .or_else(|| nearest_county_for_lon_lat(counties, lon, lat))
            {
                out.insert(county.county_id.clone());
            }
        }
    }
    out
}

const BUS_ROAD_SNAP_MAX_M: f64 = 120.0;
const FERRY_WATER_PATH_TOLERANCE_M: f64 = 120.0;
const FERRY_WATER_TERMINAL_TOLERANCE_M: f64 = 350.0;

fn point_near_roads_in_layers(
    lon: f64,
    lat: f64,
    layers: &[Arc<CountyModeConstraintData>],
) -> bool {
    for layer in layers {
        if point_near_any_segment_m(lon, lat, &layer.road_segments, BUS_ROAD_SNAP_MAX_M) {
            return true;
        }
    }
    false
}

fn point_in_water_layers(lon: f64, lat: f64, layers: &[Arc<CountyModeConstraintData>]) -> bool {
    let point = Point::new(lon, lat);
    for layer in layers {
        for polygon in &layer.water_polygons {
            if polygon.contains(&point) {
                return true;
            }
        }
    }
    false
}

fn point_near_water_layers(
    lon: f64,
    lat: f64,
    layers: &[Arc<CountyModeConstraintData>],
    threshold_m: f64,
) -> bool {
    for layer in layers {
        if point_near_any_segment_m(lon, lat, &layer.water_segments, threshold_m) {
            return true;
        }
    }
    false
}

pub(crate) fn bus_path_matches_roads(
    path: &[(f64, f64)],
    layers: &[Arc<CountyModeConstraintData>],
) -> bool {
    let has_data = layers.iter().any(|layer| !layer.road_segments.is_empty());
    if !has_data {
        return true;
    }
    for (lon, lat) in sample_path_points(path, 120.0) {
        if !point_near_roads_in_layers(lon, lat, layers) {
            return false;
        }
    }
    true
}

pub(crate) fn ferry_path_matches_water(
    path: &[(f64, f64)],
    layers: &[Arc<CountyModeConstraintData>],
) -> bool {
    let has_data = layers
        .iter()
        .any(|layer| !layer.water_polygons.is_empty() || !layer.water_segments.is_empty());
    if !has_data {
        return true;
    }
    let Some(start) = path.first().copied() else {
        return false;
    };
    let Some(end) = path.last().copied() else {
        return false;
    };
    if !point_in_water_layers(start.0, start.1, layers)
        && !point_near_water_layers(start.0, start.1, layers, FERRY_WATER_TERMINAL_TOLERANCE_M)
    {
        return false;
    }
    if !point_in_water_layers(end.0, end.1, layers)
        && !point_near_water_layers(end.0, end.1, layers, FERRY_WATER_TERMINAL_TOLERANCE_M)
    {
        return false;
    }

    let samples = sample_path_points(path, 150.0);
    let mut has_open_water = false;
    for (idx, (lon, lat)) in samples.iter().copied().enumerate() {
        let endpoint = idx == 0 || idx + 1 == samples.len();
        if point_in_water_layers(lon, lat, layers) {
            has_open_water = true;
            continue;
        }
        let threshold_m = if endpoint {
            FERRY_WATER_TERMINAL_TOLERANCE_M
        } else {
            FERRY_WATER_PATH_TOLERANCE_M
        };
        if !point_near_water_layers(lon, lat, layers, threshold_m) {
            return false;
        }
    }
    has_open_water || samples.len() <= 2
}

fn stop_type_requires_road(stop_type: Option<&str>) -> bool {
    stop_type
        .map(|value| value.trim().to_ascii_lowercase().contains("bus"))
        .unwrap_or(false)
}

fn stop_type_requires_water(stop_type: Option<&str>) -> bool {
    stop_type
        .map(|value| value.trim().to_ascii_lowercase().contains("ferry"))
        .unwrap_or(false)
}

fn default_mutation_path_validation_meta() -> MutationPathValidationMeta {
    MutationPathValidationMeta {
        path_validation_mode: "proximity".to_string(),
        road_snap_tolerance_m: BUS_ROAD_SNAP_MAX_M,
        water_path_tolerance_m: FERRY_WATER_PATH_TOLERANCE_M,
        water_terminal_tolerance_m: FERRY_WATER_TERMINAL_TOLERANCE_M,
        ..MutationPathValidationMeta::default()
    }
}

fn validate_mutation_respects_unlocked_uk_regions(
    app: &AppHandle,
    current: &Scenario,
    next: &Scenario,
    manifest: &ProjectManifest,
) -> Result<MutationPathValidationMeta, String> {
    let mut validation_meta = default_mutation_path_validation_meta();
    if manifest.session_kind != SessionKind::Game {
        return Ok(validation_meta);
    }
    let country_iso2 = primary_project_country_iso2(manifest).unwrap_or_default();
    if !is_uk_country_iso2(&country_iso2) {
        return Ok(validation_meta);
    }

    let catalog = load_gb_county_boundaries()?;
    let counties = catalog.counties;
    if counties.is_empty() {
        return Ok(validation_meta);
    }

    let unlocked_county_ids = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| canonicalize_region_id(id))
        .filter_map(|id| county_iso_and_id_from_region_id(&id))
        .filter(|(iso, _)| is_uk_country_iso2(iso))
        .map(|(_, county_id)| county_id)
        .collect::<HashSet<_>>();
    if unlocked_county_ids.is_empty() {
        return Ok(validation_meta);
    }

    let locked_counties = counties
        .iter()
        .filter(|county| !unlocked_county_ids.contains(&county.county_id))
        .collect::<Vec<_>>();
    let locked_county_ids = locked_counties
        .iter()
        .map(|county| county.county_id.clone())
        .collect::<HashSet<_>>();

    let current_stop_by_id = current
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), stop))
        .collect::<HashMap<_, _>>();
    let current_link_by_id = current
        .world
        .links
        .iter()
        .map(|link| (link.id.clone(), link))
        .collect::<HashMap<_, _>>();
    let next_stop_xy_by_id = next
        .world
        .stops
        .iter()
        .map(|stop| (stop.id.clone(), (stop.x, stop.y)))
        .collect::<HashMap<_, _>>();
    let mut blocked_counties = BTreeSet::<String>::new();
    let mut road_invalid = BTreeSet::<String>::new();
    let mut water_invalid = BTreeSet::<String>::new();

    for stop in &next.world.stops {
        let changed = current_stop_by_id
            .get(&stop.id)
            .map(|prev| (prev.x - stop.x).abs() > 1e-6 || (prev.y - stop.y).abs() > 1e-6)
            .unwrap_or(true);
        if !changed {
            continue;
        }
        let Some((lon, lat)) = world_xy_to_lonlat_safe(&next.meta.crs, stop.x, stop.y) else {
            continue;
        };
        validation_meta.changed_stops_checked =
            validation_meta.changed_stops_checked.saturating_add(1);
        let county = county_for_lon_lat(&counties, lon, lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, lon, lat));
        let Some(county) = county else {
            continue;
        };
        if locked_county_ids.contains(&county.county_id) {
            blocked_counties.insert(county.name.clone());
        }
        let layers = load_county_mode_constraints(app, &county.county_id)?;
        let stop_label = stop
            .name
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| stop.id.clone());
        if stop_type_requires_road(stop.stop_type.as_deref()) {
            validation_meta.road_stops_checked =
                validation_meta.road_stops_checked.saturating_add(1);
            if !layers.road_segments.is_empty()
                && !point_near_any_segment_m(lon, lat, &layers.road_segments, BUS_ROAD_SNAP_MAX_M)
            {
                road_invalid.insert(format!("stop:{stop_label}"));
                validation_meta.road_stops_invalid =
                    validation_meta.road_stops_invalid.saturating_add(1);
            }
        }
        if stop_type_requires_water(stop.stop_type.as_deref()) {
            validation_meta.water_stops_checked =
                validation_meta.water_stops_checked.saturating_add(1);
            if (!layers.water_polygons.is_empty() || !layers.water_segments.is_empty())
                && !point_in_water_layers(lon, lat, std::slice::from_ref(&layers))
                && !point_near_any_segment_m(
                    lon,
                    lat,
                    &layers.water_segments,
                    FERRY_WATER_TERMINAL_TOLERANCE_M,
                )
            {
                water_invalid.insert(format!("stop:{stop_label}"));
                validation_meta.water_stops_invalid =
                    validation_meta.water_stops_invalid.saturating_add(1);
            }
        }
    }

    for link in &next.world.links {
        let changed = current_link_by_id
            .get(&link.id)
            .map(|prev| {
                prev.from_stop != link.from_stop
                    || prev.to_stop != link.to_stop
                    || prev.geometry != link.geometry
                    || prev.mode != link.mode
            })
            .unwrap_or(true);
        if !changed {
            continue;
        }
        let path = link_path_lonlat(link, &next_stop_xy_by_id, &next.meta.crs);
        if path.len() < 2 {
            continue;
        }
        validation_meta.changed_links_checked =
            validation_meta.changed_links_checked.saturating_add(1);
        for county in &locked_counties {
            if path_hits_county(&path, county) {
                blocked_counties.insert(county.name.clone());
            }
        }
        let county_ids = county_ids_for_path(&path, &counties);
        let mut layers = Vec::<Arc<CountyModeConstraintData>>::new();
        for county_id in county_ids {
            layers.push(load_county_mode_constraints(app, &county_id)?);
        }
        let mode = link.mode.trim().to_ascii_lowercase();
        if mode == "bus" {
            validation_meta.bus_links_checked = validation_meta.bus_links_checked.saturating_add(1);
            if !bus_path_matches_roads(&path, &layers) {
                road_invalid.insert(format!("link:{}", link.id));
                validation_meta.bus_links_invalid =
                    validation_meta.bus_links_invalid.saturating_add(1);
            }
        }
        if mode == "ferry" {
            validation_meta.ferry_links_checked =
                validation_meta.ferry_links_checked.saturating_add(1);
            if !ferry_path_matches_water(&path, &layers) {
                water_invalid.insert(format!("link:{}", link.id));
                validation_meta.ferry_links_invalid =
                    validation_meta.ferry_links_invalid.saturating_add(1);
            }
        }
    }

    let mut errors = Vec::<String>::new();
    if !blocked_counties.is_empty() {
        let counties_list = blocked_counties.into_iter().collect::<Vec<_>>();
        validation_meta.locked_county_hits = counties_list.len();
        errors.push(format!(
            "LockedCountyViolation: unlock before building in: {}",
            counties_list.join(", ")
        ));
    }
    if !road_invalid.is_empty() {
        let offenders = road_invalid.into_iter().collect::<Vec<_>>();
        errors.push(format!(
            "ModePathInvalidRoad: bus geometry must follow drivable roads ({})",
            offenders.join(", ")
        ));
    }
    if !water_invalid.is_empty() {
        let offenders = water_invalid.into_iter().collect::<Vec<_>>();
        errors.push(format!(
            "ModePathInvalidWater: ferry geometry must remain on water with shoreline terminals ({})",
            offenders.join(", ")
        ));
    }
    if errors.is_empty() {
        Ok(validation_meta)
    } else {
        Err(errors.join(" | "))
    }
}

fn apply_difficulty_to_mutation_summary(
    summary: &mut NetworkMutationSummary,
    profile: &DifficultyProfile,
) {
    let capex_mult = profile.capex_mult.max(0.0);
    let opex_mult = profile.opex_mult.max(0.0);
    let previous_apply_total = summary.apply_total_delta_base;
    summary.capex_delta_base *= capex_mult;
    summary.infra_capex_delta_base *= capex_mult;
    summary.fleet_purchase_base *= capex_mult;
    summary.fleet_upgrade_base *= capex_mult;
    summary.fleet_transfer_fees_base *= capex_mult;
    summary.fleet_salvage_refund_base *= capex_mult;
    summary.net_capex_delta_base *= capex_mult;
    summary.construction_cost_delta_base *= capex_mult;
    summary.fleet_purchase_delta_base *= capex_mult;
    summary.fleet_configuration_delta_base *= capex_mult;
    summary.apply_total_delta_base *= capex_mult;
    summary.estimated_total_capex_base *= capex_mult;
    summary.projected_opex_per_hour_base *= opex_mult;
    summary.projected_staff_opex_per_hour_base *= opex_mult;
    summary.estimated_total_opex_per_hour_base *= opex_mult;
    if let Some(balance_after_apply) = summary.projected_balance_after_apply_base {
        let implied_balance_before = balance_after_apply + previous_apply_total;
        summary.projected_balance_after_apply_base =
            Some(implied_balance_before - summary.apply_total_delta_base);
    }
}

#[command]
pub fn preview_network_mutation(
    app: AppHandle,
    project_path: String,
    scenario_document: ScenarioDocumentLite,
) -> Result<NetworkMutationPreviewResult, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;

    let load_current_started = Instant::now();
    let current_doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    build_perf_log(
        "command.preview_network_mutation.load_current_doc",
        load_current_started,
    );
    let migrate_next_started = Instant::now();
    let next_doc = ScenarioService::migrate_to_current(ScenarioDocument {
        schema_version: scenario_document.schema_version,
        scenario: scenario_document.scenario,
    })
    .map_err(|e| e.to_string())?;
    build_perf_log(
        "command.preview_network_mutation.migrate_next_doc",
        migrate_next_started,
    );
    let validate_started = Instant::now();
    ScenarioService::validate(&next_doc.scenario).map_err(|e| e.to_string())?;
    build_perf_log(
        "command.preview_network_mutation.validate_next_doc",
        validate_started,
    );
    let read_manifest_started = Instant::now();
    let manifest = read_manifest(&project_root)?;
    build_perf_log(
        "command.preview_network_mutation.read_manifest",
        read_manifest_started,
    );
    let path_validation_started = Instant::now();
    let path_validation = validate_mutation_respects_unlocked_uk_regions(
        &app,
        &current_doc.scenario,
        &next_doc.scenario,
        &manifest,
    )?;
    build_perf_log(
        "command.preview_network_mutation.validate_paths",
        path_validation_started,
    );
    let cfg = economy_config();
    let summarize_started = Instant::now();
    let summary = summarize_network_mutation(
        &current_doc.scenario,
        &next_doc.scenario,
        &cfg,
        Some(manifest.economy.current_balance_base),
    );
    build_perf_log(
        "command.preview_network_mutation.summarize",
        summarize_started,
    );
    let mut summary = summary;
    let profile = resolved_difficulty_profile(&manifest);
    let apply_difficulty_started = Instant::now();
    apply_difficulty_to_mutation_summary(&mut summary, &profile);
    build_perf_log(
        "command.preview_network_mutation.apply_difficulty",
        apply_difficulty_started,
    );
    let breakdown_started = Instant::now();
    let cost_breakdown = mutation_cost_breakdown(&summary);
    build_perf_log(
        "command.preview_network_mutation.cost_breakdown",
        breakdown_started,
    );
    build_perf_log("command.preview_network_mutation.total", started);
    Ok(NetworkMutationPreviewResult {
        summary,
        cost_breakdown,
        path_validation,
    })
}

#[command]
pub fn apply_network_mutation(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    scenario_document: ScenarioDocumentLite,
    capex_override_base: Option<f64>,
) -> Result<NetworkMutationResult, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    ensure_project_dirs(&project_root)?;

    let load_current_started = Instant::now();
    let current_doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    build_perf_log(
        "command.apply_network_mutation.load_current_doc",
        load_current_started,
    );
    let migrate_next_started = Instant::now();
    let mut next_doc = ScenarioService::migrate_to_current(ScenarioDocument {
        schema_version: scenario_document.schema_version,
        scenario: scenario_document.scenario,
    })
    .map_err(|e| e.to_string())?;
    build_perf_log(
        "command.apply_network_mutation.migrate_next_doc",
        migrate_next_started,
    );
    let validate_started = Instant::now();
    ScenarioService::validate(&next_doc.scenario).map_err(|e| e.to_string())?;
    build_perf_log(
        "command.apply_network_mutation.validate_next_doc",
        validate_started,
    );

    let read_manifest_started = Instant::now();
    let mut manifest = read_manifest(&project_root)?;
    build_perf_log(
        "command.apply_network_mutation.read_manifest",
        read_manifest_started,
    );
    let path_validation_started = Instant::now();
    let path_validation = validate_mutation_respects_unlocked_uk_regions(
        &app,
        &current_doc.scenario,
        &next_doc.scenario,
        &manifest,
    )?;
    build_perf_log(
        "command.apply_network_mutation.validate_paths",
        path_validation_started,
    );
    let cfg = economy_config();
    let summarize_started = Instant::now();
    let summary = summarize_network_mutation(
        &current_doc.scenario,
        &next_doc.scenario,
        &cfg,
        Some(manifest.economy.current_balance_base),
    );
    build_perf_log(
        "command.apply_network_mutation.summarize",
        summarize_started,
    );
    let mut summary = summary;
    let profile = resolved_difficulty_profile(&manifest);
    let apply_difficulty_started = Instant::now();
    apply_difficulty_to_mutation_summary(&mut summary, &profile);
    build_perf_log(
        "command.apply_network_mutation.apply_difficulty",
        apply_difficulty_started,
    );
    let breakdown_started = Instant::now();
    let cost_breakdown = mutation_cost_breakdown(&summary);
    build_perf_log(
        "command.apply_network_mutation.cost_breakdown",
        breakdown_started,
    );
    let capex_override_scaled = capex_override_base
        .filter(|value| value.is_finite())
        .map(|value| value * profile.capex_mult.max(0.0));
    let apply_budget_started = Instant::now();
    apply_build_budget(&mut manifest, &cfg, &summary, capex_override_scaled)?;
    build_perf_log(
        "command.apply_network_mutation.apply_budget",
        apply_budget_started,
    );
    let applied_total_delta_base = capex_override_scaled
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(summary.apply_total_delta_base);
    if applied_total_delta_base.is_finite() {
        if applied_total_delta_base >= 0.0 {
            update_region_ledger(&mut manifest, 0.0, 0.0, 0.0, applied_total_delta_base);
            record_monthly_financial_delta(&mut manifest, 0.0, 0.0, applied_total_delta_base, 0.0);
        } else {
            let refund_base = -applied_total_delta_base;
            manifest.economy.cumulative_revenue_base += refund_base;
            update_region_ledger(&mut manifest, refund_base, 0.0, 0.0, 0.0);
            record_monthly_financial_delta(&mut manifest, refund_base, 0.0, 0.0, 0.0);
        }
        bump_economy_revision(&mut manifest);
    }
    sync_progress_budget_from_economy(&mut manifest);
    manifest.updated_at = now_string();

    let settle_orders_started = Instant::now();
    let _ =
        settle_pending_purchase_orders(&mut next_doc.scenario, manifest.clock_state.tick_seconds);
    build_perf_log(
        "command.apply_network_mutation.settle_purchase_orders",
        settle_orders_started,
    );

    let write_scenario_started = Instant::now();
    ScenarioService::save_to_path(
        scenario_path(&project_root).to_string_lossy().as_ref(),
        &next_doc,
    )
    .map_err(|e| e.to_string())?;
    build_perf_log(
        "command.apply_network_mutation.write_scenario",
        write_scenario_started,
    );
    let write_manifest_started = Instant::now();
    write_manifest(&project_root, &manifest)?;
    build_perf_log(
        "command.apply_network_mutation.write_manifest",
        write_manifest_started,
    );

    if project_is_current(&state, &project_path)? {
        let rehydrate_started = Instant::now();
        let mut guard = state
            .game
            .lock()
            .map_err(|_| "game mutex poisoned".to_string())?;
        if let Some(game_state) = guard.as_mut() {
            rehydrate_game_state_scenario(game_state, &next_doc.scenario);
        }
        build_perf_log(
            "command.apply_network_mutation.rehydrate_game_state",
            rehydrate_started,
        );
        let enqueue_runtime_action_started = Instant::now();
        let _ = enqueue_runtime_action_with_retry(
            state.inner(),
            &project_path,
            RuntimeAction::InvalidateMaterialization,
        )?;
        build_perf_log(
            "command.apply_network_mutation.enqueue_runtime_invalidate",
            enqueue_runtime_action_started,
        );
    }
    build_perf_log("command.apply_network_mutation.total", started);
    Ok(NetworkMutationResult {
        scenario: ScenarioDocumentLite {
            schema_version: next_doc.schema_version,
            scenario: next_doc.scenario,
        },
        manifest,
        summary,
        cost_breakdown,
        path_validation,
    })
}

#[command]
pub fn inspect_station(
    app: AppHandle,
    state: tauri::State<AppState>,
    project_path: String,
    stop_id: String,
) -> Result<StationInspection, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    let load_scenario_started = Instant::now();
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let load_scenario_ms = load_scenario_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log(
        "command.inspect_station.load_scenario",
        load_scenario_started,
    );
    let output_started = Instant::now();
    let output = inspection_output_for_project(&state, &project_path, &doc.scenario).ok();
    let resolve_output_ms = output_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_station.resolve_output", output_started);
    let inspect_started = Instant::now();
    let mut inspection = inspect_station_from_scenario(&doc.scenario, output.as_ref(), &stop_id)?;
    let compute_ms = inspect_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_station.compute", inspect_started);
    let runtime_overlay_started = Instant::now();
    let latest_snapshot = latest_runtime_snapshot_for_project(state.inner(), &project_path)
        .ok()
        .flatten();
    if let Some(snapshot) = latest_snapshot.as_ref() {
        if let Some(runtime_station) = snapshot.stations.iter().find(|s| s.stop_id == stop_id) {
            inspection.station_load_current_pax = runtime_station.current_inside_pax.max(0.0);
            inspection.station_queue_capacity_pax = runtime_station.capacity_pax.max(0.0);
            inspection.passengers_declined_last_hour = runtime_station.declined_last_hour.max(0.0);
            inspection.station_entries_per_hour = runtime_station.entries_per_hour.max(0.0);
            inspection.station_exits_per_hour = runtime_station.exits_per_hour.max(0.0);
            inspection.average_wait_to_board_s = runtime_station.avg_wait_to_board_s.max(0.0);
            inspection.queue_end = runtime_station.current_inside_pax.max(0.0);
        }
    }
    let runtime_overlay_ms = runtime_overlay_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log(
        "command.inspect_station.merge_runtime_overlay",
        runtime_overlay_started,
    );
    let diagnostics_started = Instant::now();
    let mut line_id_by_service = HashMap::<String, String>::new();
    for service in &doc.scenario.world.services {
        line_id_by_service.insert(service.id.clone(), service_line_runtime_id(service));
    }
    let mut planner_trace = interlinked_engine::sim::PlannerPassengerTrace::default();
    let mut planner_service_stop_trace_by_key =
        HashMap::<(String, String), interlinked_engine::sim::PlannerServiceStopTrace>::new();
    let mut planner_mode_capture_by_service = HashMap::<String, f64>::new();

    let mut service_debug_rows = HashMap::<String, StationRuntimeServiceDiagnostics>::new();
    let mut planner_attempted_total_pax = 0.0_f64;
    let mut planner_cohort_rows = 0usize;
    if let Some(sim_output) = output.as_ref() {
        planner_trace = sim_output.diagnostics.planner_passenger_trace.clone();
        for trace in &planner_trace.service_stop_traces {
            planner_service_stop_trace_by_key.insert(
                (trace.service_id.clone(), trace.stop_id.clone()),
                trace.clone(),
            );
        }
        for service_capture in &sim_output.service_transit_capture_context {
            planner_mode_capture_by_service.insert(
                service_capture.service_id.clone(),
                service_capture.transit_captured_demand.max(0.0),
            );
        }
        for cohort in &sim_output.passenger_cohorts {
            if cohort.board_stop_id != stop_id {
                continue;
            }
            let attempted = cohort.attempted_pax.max(0.0);
            planner_attempted_total_pax += attempted;
            planner_cohort_rows = planner_cohort_rows.saturating_add(1);
            let row = service_debug_rows
                .entry(cohort.service_id.clone())
                .or_insert_with(|| StationRuntimeServiceDiagnostics {
                    service_id: cohort.service_id.clone(),
                    line_id: line_id_by_service
                        .get(&cohort.service_id)
                        .cloned()
                        .unwrap_or_default(),
                    ..StationRuntimeServiceDiagnostics::default()
                });
            row.planner_attempted_pax += attempted;
        }
        for load in &sim_output.board_loads {
            if load.stop_id != stop_id {
                continue;
            }
            let row = service_debug_rows
                .entry(load.service_id.clone())
                .or_insert_with(|| StationRuntimeServiceDiagnostics {
                    service_id: load.service_id.clone(),
                    line_id: line_id_by_service
                        .get(&load.service_id)
                        .cloned()
                        .unwrap_or_default(),
                    ..StationRuntimeServiceDiagnostics::default()
                });
            row.planner_board_load_arrivals_pax += load.arrivals.max(0.0);
        }
    }

    let mut runtime_queue_total_pax = 0.0_f64;
    let mut runtime_attempted_total_pax = 0.0_f64;
    let mut has_live_runtime_ops = false;
    if let Ok(guard) = state.runtime_ops.lock() {
        if let Some(ops) = guard
            .as_ref()
            .filter(|runtime| runtime.project_path == project_path)
        {
            has_live_runtime_ops = true;
            for ((service_id, board_stop_id, _destination_stop_id), queued) in &ops.queue_cohorts {
                if board_stop_id != &stop_id {
                    continue;
                }
                let queued = queued.max(0.0);
                runtime_queue_total_pax += queued;
                let row = service_debug_rows
                    .entry(service_id.clone())
                    .or_insert_with(|| StationRuntimeServiceDiagnostics {
                        service_id: service_id.clone(),
                        line_id: line_id_by_service
                            .get(service_id)
                            .cloned()
                            .unwrap_or_default(),
                        ..StationRuntimeServiceDiagnostics::default()
                    });
                row.runtime_queue_pax += queued;
            }
            for ((service_id, board_stop_id), ingest) in &ops.last_queue_ingest_by_service_stop {
                if board_stop_id != &stop_id {
                    continue;
                }
                let row = service_debug_rows
                    .entry(service_id.clone())
                    .or_insert_with(|| StationRuntimeServiceDiagnostics {
                        service_id: service_id.clone(),
                        line_id: line_id_by_service
                            .get(service_id)
                            .cloned()
                            .unwrap_or_default(),
                        ..StationRuntimeServiceDiagnostics::default()
                    });
                row.runtime_attempted_pax += ingest.attempted_pax.max(0.0);
                row.runtime_ingested_pax += ingest.ingested_pax.max(0.0);
                row.runtime_dropped_not_dispatchable_pax +=
                    ingest.dropped_not_dispatchable_pax.max(0.0);
                row.runtime_dropped_invalid_stop_pax += ingest.dropped_invalid_stop_pax.max(0.0);
                row.dispatchable = ops.dispatch_service_ids.contains(service_id);
                runtime_attempted_total_pax += ingest.attempted_pax.max(0.0);
            }
            for ((service_id, board_stop_id), boarding) in &ops.last_boarding_by_service_stop {
                if board_stop_id != &stop_id {
                    continue;
                }
                let row = service_debug_rows
                    .entry(service_id.clone())
                    .or_insert_with(|| StationRuntimeServiceDiagnostics {
                        service_id: service_id.clone(),
                        line_id: line_id_by_service
                            .get(service_id)
                            .cloned()
                            .unwrap_or_default(),
                        ..StationRuntimeServiceDiagnostics::default()
                    });
                row.runtime_boarding_attempted_pax += boarding.attempted_pax.max(0.0);
                row.runtime_boarded_pax += boarding.boarded_pax.max(0.0);
                row.runtime_left_behind_pax += boarding.left_behind_pax.max(0.0);
            }
        }
    }

    for row in service_debug_rows.values_mut() {
        if row.line_id.is_empty() {
            row.line_id = line_id_by_service
                .get(&row.service_id)
                .cloned()
                .unwrap_or_default();
        }
        if let Some(trace) =
            planner_service_stop_trace_by_key.get(&(row.service_id.clone(), stop_id.clone()))
        {
            row.planner_candidate_paths_raw = trace.raw_candidate_paths;
            row.planner_candidate_paths_boardable = trace.boardable_candidate_paths;
            row.planner_rejected_no_board_or_alight_paths = trace.rejected_no_board_or_alight_paths;
            row.planner_rejected_unpaired_board_alight_paths =
                trace.rejected_unpaired_board_alight_paths;
            row.planner_assigned_pax = trace.planner_assigned_pax.max(0.0);
            row.planner_reason_code = trace.reason_code.clone();
        }
        row.planner_mode_transit_captured_pax = planner_mode_capture_by_service
            .get(&row.service_id)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        let attempted_basis = if has_live_runtime_ops {
            row.runtime_attempted_pax.max(0.0)
        } else {
            row.planner_attempted_pax.max(0.0)
        };
        row.diagnostic_note = if attempted_basis > 0.0 && row.runtime_ingested_pax <= 0.0 {
            if row.runtime_dropped_not_dispatchable_pax > 0.0 {
                Some("runtime_ingest_drop:not_dispatchable".to_string())
            } else if row.runtime_dropped_invalid_stop_pax > 0.0 {
                Some("runtime_ingest_drop:invalid_stop_key".to_string())
            } else {
                Some("runtime_ingest_zero:unknown".to_string())
            }
        } else if row.runtime_queue_pax > 0.0 && row.runtime_boarding_attempted_pax <= 0.0 {
            Some("runtime_boarding_zero:no_departure_or_key_mismatch".to_string())
        } else if row.runtime_boarding_attempted_pax > 0.0
            && row.runtime_boarded_pax <= 0.0
            && row.runtime_left_behind_pax > 0.0
        {
            Some("runtime_boarding_zero:capacity_or_direction_block".to_string())
        } else {
            None
        };
    }

    let snapshot_current_inside_pax = latest_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.stations.iter().find(|s| s.stop_id == stop_id))
        .map(|station| station.current_inside_pax.max(0.0))
        .unwrap_or(0.0);
    let mut services = service_debug_rows.into_values().collect::<Vec<_>>();
    services.sort_by(|a, b| a.service_id.cmp(&b.service_id));
    let attempted_total_for_trace = if has_live_runtime_ops {
        runtime_attempted_total_pax.max(0.0)
    } else {
        planner_attempted_total_pax.max(0.0)
    };
    let planner_stage_hint = planner_trace
        .first_zero_stage
        .as_deref()
        .unwrap_or("unknown_stage");
    let planner_reason_hint = planner_trace
        .first_zero_reason
        .as_deref()
        .unwrap_or("unknown_reason");
    let first_zero_or_mismatch = if attempted_total_for_trace <= 0.0 {
        if has_live_runtime_ops {
            Some(format!(
                "A:runtime_attempted_zero|planner_stage={planner_stage_hint}|reason={planner_reason_hint}"
            ))
        } else {
            Some(format!(
                "A:planner_attempted_zero|planner_stage={planner_stage_hint}|reason={planner_reason_hint}"
            ))
        }
    } else if runtime_queue_total_pax <= 0.0 {
        if services
            .iter()
            .any(|row| row.runtime_dropped_not_dispatchable_pax > 0.0)
        {
            Some("B:runtime_ingest_drop_not_dispatchable".to_string())
        } else if services
            .iter()
            .any(|row| row.runtime_dropped_invalid_stop_pax > 0.0)
        {
            Some("B:runtime_ingest_drop_invalid_stop_key".to_string())
        } else {
            Some("B:runtime_queue_zero_after_ingest".to_string())
        }
    } else if snapshot_current_inside_pax <= 0.0 {
        Some("C:snapshot_station_current_inside_zero".to_string())
    } else if services.iter().any(|row| {
        row.runtime_boarding_attempted_pax > 0.0
            && row.runtime_boarded_pax <= 0.0
            && row.runtime_left_behind_pax > 0.0
    }) {
        Some("E:boarding_attempted_but_zero_boarded".to_string())
    } else {
        None
    };
    eprintln!(
        "[pax-inspect] stop={} planner_stage={} planner_reason={} planner_attempted={:.2} runtime_attempted={:.2} runtime_queue={:.2} snapshot_inside={:.2} mode_raw_paths={} mode_boardable_paths={} assignment_attempted={:.2}",
        stop_id,
        planner_trace
            .first_zero_stage
            .as_deref()
            .unwrap_or("none"),
        planner_trace
            .first_zero_reason
            .as_deref()
            .unwrap_or("none"),
        planner_attempted_total_pax.max(0.0),
        runtime_attempted_total_pax.max(0.0),
        runtime_queue_total_pax.max(0.0),
        snapshot_current_inside_pax.max(0.0),
        planner_trace.mode_choice_candidate_paths_raw_total,
        planner_trace.mode_choice_candidate_paths_boardable_total,
        planner_trace.assignment_attempted_pax_total.max(0.0),
    );

    inspection.runtime_diagnostics = Some(StationRuntimeDiagnostics {
        counter_provenance: CounterProvenance::DebugLegacy,
        tick_index: latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.telemetry.tick_index)
            .unwrap_or(0),
        planner_attempted_total_pax: planner_attempted_total_pax.max(0.0),
        runtime_attempted_total_pax: runtime_attempted_total_pax.max(0.0),
        planner_cohort_rows,
        runtime_queue_total_pax: runtime_queue_total_pax.max(0.0),
        snapshot_current_inside_pax: snapshot_current_inside_pax.max(0.0),
        planner_demand_cells_total: planner_trace.demand_cells_total,
        planner_demand_cells_nonzero_activity: planner_trace.demand_cells_nonzero_activity,
        planner_zones_total: planner_trace.zones_total,
        planner_zones_nonzero_activity: planner_trace.zones_nonzero_activity,
        planner_latent_rows_total: planner_trace.latent_rows_total,
        planner_latent_total_pax: planner_trace.latent_pax_total.max(0.0),
        planner_mode_choice_rows_total: planner_trace.mode_choice_rows_total,
        planner_mode_choice_rows_with_transit_capture: planner_trace
            .mode_choice_rows_with_transit_capture,
        planner_mode_choice_transit_captured_pax: planner_trace
            .mode_choice_transit_captured_pax
            .max(0.0),
        planner_mode_choice_candidate_paths_raw_total: planner_trace
            .mode_choice_candidate_paths_raw_total,
        planner_mode_choice_candidate_paths_boardable_total: planner_trace
            .mode_choice_candidate_paths_boardable_total,
        planner_mode_choice_rejected_no_board_or_alight_total: planner_trace
            .mode_choice_rejected_no_board_or_alight_total,
        planner_mode_choice_rejected_unpaired_board_alight_total: planner_trace
            .mode_choice_rejected_unpaired_board_alight_total,
        planner_assignment_od_rows_with_transit_latent: planner_trace
            .assignment_od_rows_with_transit_latent,
        planner_assignment_od_rows_with_attempted: planner_trace.assignment_od_rows_with_attempted,
        planner_assignment_attempted_total_pax: planner_trace
            .assignment_attempted_pax_total
            .max(0.0),
        planner_assignment_candidate_paths_raw_total: planner_trace
            .assignment_candidate_paths_raw_total,
        planner_assignment_candidate_paths_boardable_total: planner_trace
            .assignment_candidate_paths_boardable_total,
        planner_assignment_rejected_no_board_or_alight_total: planner_trace
            .assignment_rejected_no_board_or_alight_total,
        planner_assignment_rejected_unpaired_board_alight_total: planner_trace
            .assignment_rejected_unpaired_board_alight_total,
        planner_first_zero_stage: planner_trace.first_zero_stage.clone(),
        planner_first_zero_reason: planner_trace.first_zero_reason.clone(),
        first_zero_or_mismatch,
        services,
    });
    let diagnostics_ms = diagnostics_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log(
        "command.inspect_station.runtime_diagnostics",
        diagnostics_started,
    );
    let landuse_started = Instant::now();
    let _ = enrich_station_inspection_with_landuse(&app, &doc.scenario, &mut inspection);
    let landuse_ms = landuse_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_station.enrich_landuse", landuse_started);
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_station.total", started);
    let runtime_status = runtime_loop_status_for_project(state.inner(), &project_path).ok();
    let snapshot_tick_index = latest_snapshot
        .as_ref()
        .map(|snapshot| snapshot.telemetry.tick_index)
        .unwrap_or(0);
    let output_board_load_rows = output
        .as_ref()
        .map(|value| value.board_loads.len())
        .unwrap_or(0);
    let output_cohort_rows = output
        .as_ref()
        .map(|value| value.passenger_cohorts.len())
        .unwrap_or(0);
    let output_mode_choice_rows = output
        .as_ref()
        .map(|value| value.mode_choice_results.len())
        .unwrap_or(0);
    eprintln!(
        "[rt-inspect] kind=station project={} stop_id={} total_ms={:.2} load_scenario_ms={:.2} resolve_output_ms={:.2} compute_ms={:.2} runtime_overlay_ms={:.2} diagnostics_ms={:.2} enrich_landuse_ms={:.2} output_available={} board_load_rows={} cohort_rows={} mode_choice_rows={} service_rows={} planner_attempted_total={:.3} runtime_attempted_total={:.3} runtime_queue_total={:.3} snapshot_inside={:.3} snapshot_tick_index={} runtime_running={} runtime_speed={} runtime_queue_depth={}",
        project_path,
        stop_id,
        total_ms.max(0.0),
        load_scenario_ms.max(0.0),
        resolve_output_ms.max(0.0),
        compute_ms.max(0.0),
        runtime_overlay_ms.max(0.0),
        diagnostics_ms.max(0.0),
        landuse_ms.max(0.0),
        output.is_some(),
        output_board_load_rows,
        output_cohort_rows,
        output_mode_choice_rows,
        inspection
            .runtime_diagnostics
            .as_ref()
            .map(|diag| diag.services.len())
            .unwrap_or(0),
        inspection
            .runtime_diagnostics
            .as_ref()
            .map(|diag| diag.planner_attempted_total_pax.max(0.0))
            .unwrap_or(0.0),
        inspection
            .runtime_diagnostics
            .as_ref()
            .map(|diag| diag.runtime_attempted_total_pax.max(0.0))
            .unwrap_or(0.0),
        inspection
            .runtime_diagnostics
            .as_ref()
            .map(|diag| diag.runtime_queue_total_pax.max(0.0))
            .unwrap_or(0.0),
        inspection
            .runtime_diagnostics
            .as_ref()
            .map(|diag| diag.snapshot_current_inside_pax.max(0.0))
            .unwrap_or(0.0),
        snapshot_tick_index,
        runtime_status
            .as_ref()
            .map(|status| status.running)
            .unwrap_or(false),
        runtime_status
            .as_ref()
            .map(|status| status.speed)
            .unwrap_or(0),
        runtime_status
            .as_ref()
            .map(|status| status.queue_depth)
            .unwrap_or(0),
    );
    Ok(inspection)
}

#[command]
pub fn inspect_line(
    state: tauri::State<AppState>,
    project_path: String,
    line_id: String,
) -> Result<LineInspection, String> {
    let started = Instant::now();
    let project_root = PathBuf::from(&project_path);
    let load_scenario_started = Instant::now();
    let doc =
        ScenarioService::load_from_path(scenario_path(&project_root).to_string_lossy().as_ref())
            .map_err(|e| e.to_string())?;
    let load_scenario_ms = load_scenario_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_line.load_scenario", load_scenario_started);
    let output_started = Instant::now();
    let output = inspection_output_for_project(&state, &project_path, &doc.scenario).ok();
    let resolve_output_ms = output_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_line.resolve_output", output_started);
    let read_manifest_started = Instant::now();
    let manifest = read_manifest(&project_root)?;
    let read_manifest_ms = read_manifest_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_line.read_manifest", read_manifest_started);
    let minute_of_day = clock_minute_of_day(&manifest.clock_state);
    let inspect_started = Instant::now();
    let mut inspection = inspect_line_from_scenario(
        &doc.scenario,
        output.as_ref(),
        &line_id,
        &economy_config(),
        Some(minute_of_day),
    )?;
    let compute_ms = inspect_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_line.compute", inspect_started);
    let runtime_overlay_started = Instant::now();
    let latest_snapshot = latest_runtime_snapshot_for_project(state.inner(), &project_path)
        .ok()
        .flatten();
    if let Some(snapshot) = latest_snapshot.as_ref() {
        if let Some(runtime_line) = snapshot
            .line_ops
            .iter()
            .find(|line| line.line_id == line_id)
        {
            inspection.boardings_attempted = runtime_line.boardings_attempted_per_hour.max(0.0);
            inspection.boardings_served = runtime_line.boarded_per_hour.max(0.0);
            inspection.alightings_served = runtime_line.alighted_per_hour.max(0.0);
            inspection.denied_boardings = runtime_line.denied_boardings_per_hour.max(0.0);
            inspection.queue_end = runtime_line.queue_end_pax.max(0.0);
            inspection.avg_wait_s = Some(runtime_line.mean_wait_s.max(0.0));
            inspection.operations_now.avg_wait_s = Some(runtime_line.mean_wait_s.max(0.0));
        }
    }
    let runtime_overlay_ms = runtime_overlay_started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log(
        "command.inspect_line.merge_runtime_overlay",
        runtime_overlay_started,
    );
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    build_perf_log("command.inspect_line.total", started);
    let runtime_status = runtime_loop_status_for_project(state.inner(), &project_path).ok();
    let snapshot_tick_index = latest_snapshot
        .as_ref()
        .map(|snapshot| snapshot.telemetry.tick_index)
        .unwrap_or(0);
    let output_board_load_rows = output
        .as_ref()
        .map(|value| value.board_loads.len())
        .unwrap_or(0);
    let output_cohort_rows = output
        .as_ref()
        .map(|value| value.passenger_cohorts.len())
        .unwrap_or(0);
    let output_mode_choice_rows = output
        .as_ref()
        .map(|value| value.mode_choice_results.len())
        .unwrap_or(0);
    eprintln!(
        "[rt-inspect] kind=line project={} line_id={} total_ms={:.2} load_scenario_ms={:.2} resolve_output_ms={:.2} read_manifest_ms={:.2} compute_ms={:.2} runtime_overlay_ms={:.2} output_available={} board_load_rows={} cohort_rows={} mode_choice_rows={} service_enabled={} effective_tph={:.3} queue_end={:.3} runtime_running={} runtime_speed={} runtime_queue_depth={} snapshot_tick_index={}",
        project_path,
        line_id,
        total_ms.max(0.0),
        load_scenario_ms.max(0.0),
        resolve_output_ms.max(0.0),
        read_manifest_ms.max(0.0),
        compute_ms.max(0.0),
        runtime_overlay_ms.max(0.0),
        output.is_some(),
        output_board_load_rows,
        output_cohort_rows,
        output_mode_choice_rows,
        inspection.service_enabled,
        inspection.effective_tph.max(0.0),
        inspection.queue_end.max(0.0),
        runtime_status
            .as_ref()
            .map(|status| status.running)
            .unwrap_or(false),
        runtime_status
            .as_ref()
            .map(|status| status.speed)
            .unwrap_or(0),
        runtime_status
            .as_ref()
            .map(|status| status.queue_depth)
            .unwrap_or(0),
        snapshot_tick_index,
    );
    Ok(inspection)
}
