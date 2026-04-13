use crate::*;

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

pub(crate) fn gb_county_adjacency_map(counties: &[CountyBoundary]) -> HashMap<String, Vec<String>> {
    let mut adjacency = counties
        .iter()
        .map(|county| {
            (
                region_id_from_county(CANONICAL_UK_ISO2, &county.county_id),
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
            let a_id = region_id_from_county(CANONICAL_UK_ISO2, &a.county_id);
            let b_id = region_id_from_county(CANONICAL_UK_ISO2, &b.county_id);
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
