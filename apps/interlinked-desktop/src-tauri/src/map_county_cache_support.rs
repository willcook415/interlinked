use crate::*;

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexFile {
    counties: Vec<UkCountyIndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexEntry {
    county_id: String,
    name: String,
    nation: String,
    country_iso2: String,
    source_code: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CountyBoundary {
    pub(crate) county_id: String,
    pub(crate) name: String,
    pub(crate) nation: String,
    pub(crate) country_iso2: String,
    pub(crate) source_code: String,
    pub(crate) geometry: MultiPolygon<f64>,
    pub(crate) geometry_json: JsonValue,
    pub(crate) bbox_center_lon: f64,
    pub(crate) bbox_center_lat: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct CountyBoundaryCatalog {
    pub(crate) counties: Vec<CountyBoundary>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GeoSegment {
    pub(crate) a_lon: f64,
    pub(crate) a_lat: f64,
    pub(crate) b_lon: f64,
    pub(crate) b_lat: f64,
    pub(crate) min_lon: f64,
    pub(crate) min_lat: f64,
    pub(crate) max_lon: f64,
    pub(crate) max_lat: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CountyModeConstraintData {
    pub(crate) road_segments: Vec<GeoSegment>,
    pub(crate) water_polygons: Vec<MultiPolygon<f64>>,
    pub(crate) water_segments: Vec<GeoSegment>,
}

#[derive(Debug, Clone)]
pub(crate) struct LanduseSample {
    pub(crate) x_m: f64,
    pub(crate) y_m: f64,
    pub(crate) weight: f64,
    pub(crate) intensity: f64,
    pub(crate) mix: [f64; 7],
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CountyLanduseProfile {
    pub(crate) samples: Vec<LanduseSample>,
}

#[derive(Debug, Clone, Deserialize)]
struct GbCountyAliasFile {
    aliases: HashMap<String, String>,
}

pub(crate) fn empty_feature_collection_json() -> JsonValue {
    serde_json::json!({
        "type": "FeatureCollection",
        "features": []
    })
}

pub(crate) fn read_feature_collection_json(path: &Path) -> Result<JsonValue, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value =
        serde_json::from_str::<JsonValue>(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    let is_feature_collection = value
        .get("type")
        .and_then(|v| v.as_str())
        .map(|v| v == "FeatureCollection")
        .unwrap_or(false);
    if !is_feature_collection {
        return Err(format!("{} is not a FeatureCollection", path.display()));
    }
    Ok(value)
}

pub(crate) fn country_map_context_cache() -> &'static Mutex<HashMap<String, CountryMapContext>> {
    COUNTRY_MAP_CONTEXT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn region_street_context_cache() -> &'static Mutex<HashMap<String, RegionStreetContext>>
{
    REGION_STREET_CONTEXT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn gb_county_landuse_cache() -> &'static Mutex<HashMap<String, CountyLanduseProfile>> {
    GB_COUNTY_LANDUSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn gb_county_mode_constraint_cache(
) -> &'static Mutex<HashMap<String, Arc<CountyModeConstraintData>>> {
    GB_COUNTY_MODE_CONSTRAINT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn simplify_county_geometry(geometry: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    geometry.simplify(&0.0018)
}

static GB_COUNTY_BOUNDARY_CACHE: OnceLock<Result<CountyBoundaryCatalog, String>> = OnceLock::new();
static GB_COUNTY_ALIAS_CACHE: OnceLock<Result<HashMap<String, String>, String>> = OnceLock::new();
static COUNTRY_MAP_CONTEXT_CACHE: OnceLock<Mutex<HashMap<String, CountryMapContext>>> =
    OnceLock::new();
static REGION_STREET_CONTEXT_CACHE: OnceLock<Mutex<HashMap<String, RegionStreetContext>>> =
    OnceLock::new();
static GB_COUNTY_LANDUSE_CACHE: OnceLock<Mutex<HashMap<String, CountyLanduseProfile>>> =
    OnceLock::new();
static GB_COUNTY_MODE_CONSTRAINT_CACHE: OnceLock<
    Mutex<HashMap<String, Arc<CountyModeConstraintData>>>,
> = OnceLock::new();

pub(crate) fn repo_boundaries_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("boundaries")
}

pub(crate) fn repo_map_style_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("map_assets")
        .join("style")
}

fn normalized_iso_a2(value: Option<&str>) -> Option<String> {
    let iso = value?.trim().to_ascii_uppercase();
    if iso.len() != 2 || iso == "-99" || !iso.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(iso)
}

fn world_context_iso2_from_props(props: &JsonValue) -> Option<String> {
    normalized_iso_a2(props.get("ISO_A2").and_then(|v| v.as_str()))
        .or_else(|| normalized_iso_a2(props.get("ISO_A2_EH").and_then(|v| v.as_str())))
}

pub(crate) fn world_context_from_countries_geojson(value: JsonValue) -> Result<JsonValue, String> {
    let Some(features) = value.get("features").and_then(|v| v.as_array()) else {
        return Err("countries.geojson must contain a features array".to_string());
    };
    let remapped = features
        .iter()
        .filter_map(|feature| {
            let geometry = feature.get("geometry")?.clone();
            let props = feature.get("properties")?;
            let iso = world_context_iso2_from_props(props)?;
            let name = props
                .get("NAME_EN")
                .and_then(|v| v.as_str())
                .or_else(|| props.get("ADMIN").and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            Some(serde_json::json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": {
                    "country_iso2": iso,
                    "name": name,
                    "playable_now": iso == "GB",
                    "coming_soon": iso != "GB"
                }
            }))
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "type": "FeatureCollection",
        "features": remapped
    }))
}

pub(crate) fn counties_bounds(counties: &[CountyBoundary]) -> Option<[[f64; 2]; 2]> {
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for county in counties {
        for polygon in &county.geometry.0 {
            for point in polygon.exterior().points() {
                min_lon = min_lon.min(point.x());
                min_lat = min_lat.min(point.y());
                max_lon = max_lon.max(point.x());
                max_lat = max_lat.max(point.y());
            }
        }
    }
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return None;
    }
    Some([[min_lon, min_lat], [max_lon, max_lat]])
}

fn point_eq(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

fn close_ring_coords(mut coords: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if coords.len() >= 2 && !point_eq(coords[0], *coords.last().unwrap_or(&(0.0, 0.0))) {
        coords.push(coords[0]);
    }
    coords
}

fn multipolygon_to_geojson_value(geometry: &MultiPolygon<f64>) -> JsonValue {
    let coordinates = geometry
        .0
        .iter()
        .map(|polygon| {
            let mut rings = Vec::<Vec<Vec<f64>>>::new();
            rings.push(
                polygon
                    .exterior()
                    .points()
                    .map(|point| vec![point.x(), point.y()])
                    .collect(),
            );
            for interior in polygon.interiors() {
                rings.push(
                    interior
                        .points()
                        .map(|point| vec![point.x(), point.y()])
                        .collect(),
                );
            }
            rings
        })
        .collect::<Vec<_>>();
    serde_json::to_value(GeoJsonGeometry::new(GeoJsonValue::MultiPolygon(
        coordinates,
    )))
    .unwrap_or(JsonValue::Null)
}

fn multipolygon_bbox_center(geometry: &MultiPolygon<f64>) -> Option<(f64, f64)> {
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for polygon in &geometry.0 {
        for point in polygon.exterior().points() {
            min_lon = min_lon.min(point.x());
            min_lat = min_lat.min(point.y());
            max_lon = max_lon.max(point.x());
            max_lat = max_lat.max(point.y());
        }
    }
    if !min_lon.is_finite() || !min_lat.is_finite() || !max_lon.is_finite() || !max_lat.is_finite()
    {
        return None;
    }
    Some(((min_lon + max_lon) * 0.5, (min_lat + max_lat) * 0.5))
}

pub(crate) fn geojson_coords_to_polygon(coords: &[Vec<Vec<f64>>]) -> Option<Polygon<f64>> {
    let exterior = coords.first()?;
    let exterior_ring = close_ring_coords(
        exterior
            .iter()
            .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
            .collect(),
    );
    let exterior_line = LineString::from(exterior_ring);
    let interiors = coords
        .iter()
        .skip(1)
        .filter_map(|ring| {
            let pts = close_ring_coords(
                ring.iter()
                    .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
                    .collect(),
            );
            (pts.len() >= 4).then_some(LineString::from(pts))
        })
        .collect::<Vec<_>>();
    Some(Polygon::new(exterior_line, interiors))
}

fn parse_uk_counties_canonical_geojson(
    index: &[UkCountyIndexEntry],
) -> Result<Vec<CountyBoundary>, String> {
    let path = repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson");
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let GeoJson::FeatureCollection(fc) = raw.parse::<GeoJson>().map_err(|e| e.to_string())? else {
        return Err(
            "gb_ceremonial_counties_canonical.geojson must be a FeatureCollection".to_string(),
        );
    };
    if fc.features.is_empty() {
        return Ok(vec![]);
    }
    let mut by_id = index
        .iter()
        .cloned()
        .map(|entry| (entry.county_id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::<CountyBoundary>::new();
    for feature in fc.features {
        let Some(props) = feature.properties.as_ref() else {
            continue;
        };
        let Some(county_id) = props
            .get("county_id")
            .and_then(|value| value.as_str())
            .map(|value| value.trim().to_string())
        else {
            continue;
        };
        let Some(entry) = by_id.remove(&county_id) else {
            continue;
        };
        let Some(geom) = feature.geometry else {
            continue;
        };
        let geometry = match &geom.value {
            GeoJsonValue::Polygon(coords) => {
                let Some(poly) = geojson_coords_to_polygon(coords) else {
                    continue;
                };
                MultiPolygon(vec![poly])
            }
            GeoJsonValue::MultiPolygon(multi) => {
                let polys = multi
                    .iter()
                    .filter_map(|coords| geojson_coords_to_polygon(coords))
                    .collect::<Vec<_>>();
                if polys.is_empty() {
                    continue;
                }
                MultiPolygon(polys)
            }
            _ => continue,
        };
        let geometry = simplify_county_geometry(&geometry);
        let Some((bbox_center_lon, bbox_center_lat)) = multipolygon_bbox_center(&geometry) else {
            continue;
        };
        out.push(CountyBoundary {
            county_id: entry.county_id,
            name: entry.name,
            nation: entry.nation,
            country_iso2: entry.country_iso2,
            source_code: entry.source_code,
            geometry_json: multipolygon_to_geojson_value(&geometry),
            geometry,
            bbox_center_lon,
            bbox_center_lat,
        });
    }
    Ok(out)
}

pub(crate) fn load_gb_county_boundaries() -> Result<CountyBoundaryCatalog, String> {
    let cached = GB_COUNTY_BOUNDARY_CACHE.get_or_init(|| {
        let index_path = repo_boundaries_root().join("gb_ceremonial_counties_index.json");
        let index_file: UkCountyIndexFile = read_json_file(&index_path)?;
        let counties = parse_uk_counties_canonical_geojson(&index_file.counties)?;
        if counties.is_empty() {
            return Err(format!(
                "GB county geometry missing in {}",
                repo_boundaries_root()
                    .join("gb_ceremonial_counties_canonical.geojson")
                    .display()
            ));
        }
        Ok(CountyBoundaryCatalog { counties })
    });
    cached.clone()
}

pub(crate) fn load_gb_county_aliases() -> Result<HashMap<String, String>, String> {
    let cached = GB_COUNTY_ALIAS_CACHE.get_or_init(|| {
        let alias_path = repo_boundaries_root().join("gb_ceremonial_county_aliases.json");
        if !alias_path.exists() {
            return Ok(HashMap::new());
        }
        let aliases: GbCountyAliasFile = read_json_file(&alias_path)?;
        Ok(aliases.aliases)
    });
    cached.clone()
}
