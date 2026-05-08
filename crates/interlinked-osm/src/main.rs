use clap::{Parser, Subcommand};
use csv::StringRecord;
use geo::algorithm::contains::Contains;
use geo::algorithm::simplify::Simplify;
use geo::{LineString, MultiPolygon, Point, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use h3o::{CellIndex, LatLng, Resolution};
use interlinked_engine::model::{
    lonlat_to_web_mercator_m, web_mercator_m_to_world_xy, world_xy_to_web_mercator_m, Crs,
    DemandCell, DemandTimeSlice, Link, Meta, Params, Scenario, Service, Stop, World, Zone,
};
use interlinked_engine::{ScenarioDocument, ScenarioService};
use osmpbfreader::{Node, NodeId, OsmObj, OsmPbfReader, Tags, Way};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const CANONICAL_UK_ISO2: &str = "UK";
const UK_COMPAT_GB_ISO2: &str = "GB";
const DEFAULT_REGION_PROVIDER_MODEL: &str = "planning_surface_res6_v1";

fn canonical_country_iso2(value: &str) -> Option<String> {
    let iso = value.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return None;
    }
    if iso == CANONICAL_UK_ISO2 || iso == UK_COMPAT_GB_ISO2 {
        return Some(CANONICAL_UK_ISO2.to_string());
    }
    Some(iso)
}

fn is_uk_country_iso2(value: &str) -> bool {
    canonical_country_iso2(value)
        .map(|iso| iso == CANONICAL_UK_ISO2)
        .unwrap_or(false)
}

fn boundaries_query_iso2(value: &str) -> String {
    if is_uk_country_iso2(value) {
        UK_COMPAT_GB_ISO2.to_string()
    } else {
        value.trim().to_ascii_uppercase()
    }
}

#[derive(Parser)]
#[command(name = "interlinked-osm")]
#[command(about = "Interlinked data backbone tools", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    ImportPbf {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        out_root: Option<String>,
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        bbox: Option<Vec<f64>>,
        #[arg(long)]
        max_stops: Option<usize>,
        #[arg(long, default_value_t = 60.0)]
        snap_m: f64,
        #[arg(long, default_value_t = 600.0)]
        inferred_headway_s: f64,
        #[arg(long, default_value_t = true)]
        cleanup_topology: bool,
        #[arg(long, default_value_t = true)]
        infer_services: bool,
    },
    AttachCensus {
        #[arg(long)]
        scenario: String,
        #[arg(long)]
        csv: String,
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        profile_csv: Option<String>,
        #[arg(long, default_value_t = false)]
        replace_zones: bool,
    },
    NormalizeLsoa {
        #[arg(long)]
        population_csv: String,
        #[arg(long)]
        jobs_csv: String,
        #[arg(long)]
        centroids_csv: String,
        #[arg(long)]
        out: String,
        #[arg(long, default_value = "west_yorkshire")]
        region: String,
        #[arg(long, default_value = "epsg3857")]
        target_crs: String,
    },
    ImportGtfs {
        #[arg(long)]
        scenario: String,
        #[arg(long)]
        gtfs_dir: String,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = 80.0)]
        snap_m: f64,
        #[arg(long, default_value_t = 600.0)]
        default_headway_s: f64,
    },
    BuildLocationCatalog {
        #[arg(long)]
        country_info: String,
        #[arg(long)]
        cities: String,
        #[arg(long)]
        out: String,
    },
    BuildDemandFabric {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        out: String,
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        bbox: Option<Vec<f64>>,
        #[arg(long)]
        country_iso2: Option<String>,
        #[arg(long, default_value_t = 8)]
        h3_res: u8,
        #[arg(long, default_value = "epsg3857")]
        target_crs: String,
        #[arg(long)]
        population_raster_csv: Option<String>,
        #[arg(long)]
        built_raster_csv: Option<String>,
        #[arg(long)]
        country_boundaries_geojson: Option<String>,
        #[arg(long, default_value_t = false)]
        raster_only: bool,
    },
    ApplyDemandFabric {
        #[arg(long)]
        scenario: String,
        #[arg(long)]
        fabric: String,
        #[arg(long)]
        out: Option<String>,
        #[arg(long, default_value_t = true)]
        replace_zones: bool,
    },
    BuildDemandSurface {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        country_iso2: String,
        #[arg(long)]
        country_boundaries_geojson: String,
        #[arg(long)]
        population_raster_csv: String,
        #[arg(long)]
        built_raster_csv: String,
        #[arg(long)]
        out: String,
        #[arg(long, default_value_t = 8)]
        h3_res: u8,
        #[arg(long, default_value = "epsg3857")]
        target_crs: String,
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        bbox: Option<Vec<f64>>,
        #[arg(long, default_value_t = false)]
        raster_only: bool,
    },
    BuildDemandSurfacePack {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        countries: String,
        #[arg(long)]
        country_boundaries_geojson: String,
        #[arg(long)]
        population_raster_csv: String,
        #[arg(long)]
        built_raster_csv: String,
        #[arg(long)]
        out_dir: String,
        #[arg(long, default_value_t = 8)]
        h3_res: u8,
        #[arg(long, default_value = "epsg3857")]
        target_crs: String,
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        bbox: Option<Vec<f64>>,
        #[arg(long, default_value_t = false)]
        raster_only: bool,
    },
    BuildCountryPack {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        country_iso2: String,
        #[arg(long)]
        country_boundaries_geojson: String,
        #[arg(long)]
        population_raster_csv: String,
        #[arg(long)]
        built_raster_csv: String,
        #[arg(long)]
        out_dir: String,
        #[arg(long, default_value_t = 8)]
        h3_res: u8,
        #[arg(long, default_value = "epsg3857")]
        target_crs: String,
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        bbox: Option<Vec<f64>>,
        #[arg(long, default_value_t = false)]
        raster_only: bool,
    },
    #[command(name = "build-country-map-assets", alias = "build-gb-map-assets")]
    BuildCountryMapAssets {
        #[arg(long)]
        pbf: String,
        #[arg(long)]
        country_boundaries_geojson: String,
        #[arg(long)]
        out_dir: String,
    },
    ValidateCountryPack {
        #[arg(long)]
        pack_dir: String,
    },
}

#[derive(Debug, Clone)]
struct CandidateWay {
    way_id: i64,
    mode: String,
    node_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
struct GtfsStop {
    lon: f64,
    lat: f64,
}

#[derive(Debug, Clone)]
struct StopRef {
    id: String,
    mx: f64,
    my: f64,
    is_shape: bool,
}

#[derive(Debug, Clone)]
struct LsoaPopulationRow {
    lad_name: String,
    population: f64,
}

#[derive(Debug, Clone)]
struct NormalizedZoneRow {
    zone_id: String,
    x: f64,
    y: f64,
    population: f64,
    jobs: f64,
}

#[derive(Debug, Clone)]
struct LsoaNormalizationResult {
    rows: Vec<NormalizedZoneRow>,
    population_rows: usize,
    region_rows: usize,
    missing_jobs: usize,
    missing_centroids: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogCountry {
    iso2: String,
    name: String,
    capital_name: Option<String>,
    capital_geonameid: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogCity {
    geonameid: i64,
    name: String,
    ascii_name: String,
    lat: f64,
    lon: f64,
    population: u64,
    feature_code: String,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogMeta {
    schema_version: u32,
    generated_at_epoch_s: u64,
    country_count: usize,
    city_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandFabricMeta {
    schema_version: u32,
    generated_at_epoch_s: u64,
    source_pbf: String,
    h3_res: u8,
    target_crs: String,
    source_population_raster_csv: Option<String>,
    source_built_raster_csv: Option<String>,
    country_iso2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandFabricCell {
    cell_id: String,
    lon: f64,
    lat: f64,
    x: f64,
    y: f64,
    country_iso2: Option<String>,
    area_m2: f64,
    residents_night: f64,
    jobs_day: f64,
    activity_mix_residential: f64,
    activity_mix_office: f64,
    activity_mix_retail: f64,
    activity_mix_recreation: f64,
    activity_mix_industrial: f64,
    activity_mix_education: f64,
    activity_mix_health: f64,
    centrality_score: f64,
    data_quality_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandFabric {
    meta: DemandFabricMeta,
    cells: Vec<DemandFabricCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandSurfaceProvenance {
    generated_at_epoch_s: u64,
    source_pbf: String,
    country_boundaries_geojson: String,
    source_population_raster_csv: String,
    source_built_raster_csv: String,
    h3_base_res: u8,
    target_crs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandSurfaceCell {
    cell_id: String,
    h3_res: u8,
    lon: f64,
    lat: f64,
    x: f64,
    y: f64,
    area_m2: f64,
    country_iso2: String,
    residents_raw: f64,
    jobs_raw: f64,
    residents_smooth: f64,
    jobs_smooth: f64,
    #[serde(default)]
    activity_mix_residential: f64,
    #[serde(default)]
    activity_mix_office: f64,
    #[serde(default)]
    activity_mix_retail: f64,
    #[serde(default)]
    activity_mix_recreation: f64,
    #[serde(default)]
    activity_mix_industrial: f64,
    #[serde(default)]
    activity_mix_education: f64,
    #[serde(default)]
    activity_mix_health: f64,
    #[serde(default)]
    quality: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemandSurfaceCountry {
    country_iso2: String,
    surface_version: String,
    source_provenance: DemandSurfaceProvenance,
    cells_res6: Vec<DemandSurfaceCell>,
    cells_res7: Vec<DemandSurfaceCell>,
    cells_res8: Vec<DemandSurfaceCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CountryPackManifest {
    schema_version: u32,
    country_iso2: String,
    pack_version: String,
    generated_at_epoch_s: u64,
    surface_file: String,
    regions_file: String,
    // Authoritative runtime region provider contract (Package D).
    region_provider_model: String,
    #[serde(default)]
    compatibility_country_aliases: Vec<String>,
    region_count: usize,
    cells_res8: usize,
    source_provenance: DemandSurfaceProvenance,
    #[serde(default)]
    map_context_version: Option<String>,
    #[serde(default)]
    world_context_file: Option<String>,
    #[serde(default)]
    major_roads_file: Option<String>,
    #[serde(default)]
    county_roads_dir: Option<String>,
    #[serde(default)]
    county_basemap_mid_dir: Option<String>,
    #[serde(default)]
    county_basemap_full_dir: Option<String>,
    #[serde(default)]
    map_pack_version: Option<String>,
}

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
struct CountyBoundary {
    county_id: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    nation: String,
    #[allow(dead_code)]
    country_iso2: String,
    #[allow(dead_code)]
    source_code: String,
    geometry: MultiPolygon<f64>,
    bbox: [f64; 4],
}

#[derive(Debug, Clone)]
struct CountyBucketIndex {
    bucket_deg: f64,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}

impl CountyBucketIndex {
    fn new(counties: &[CountyBoundary], bucket_deg: f64) -> Self {
        let bucket_deg = bucket_deg.max(0.1);
        let inv = 1.0 / bucket_deg;
        let mut buckets = HashMap::<(i32, i32), Vec<usize>>::new();
        for (idx, county) in counties.iter().enumerate() {
            let min_x = (county.bbox[0] * inv).floor() as i32;
            let min_y = (county.bbox[1] * inv).floor() as i32;
            let max_x = (county.bbox[2] * inv).floor() as i32;
            let max_y = (county.bbox[3] * inv).floor() as i32;
            for bx in min_x..=max_x {
                for by in min_y..=max_y {
                    buckets.entry((bx, by)).or_default().push(idx);
                }
            }
        }
        Self {
            bucket_deg,
            buckets,
        }
    }

    fn candidate_indices_for_bbox(&self, bbox: [f64; 4]) -> Vec<usize> {
        let inv = 1.0 / self.bucket_deg.max(1e-9);
        let min_x = (bbox[0] * inv).floor() as i32;
        let min_y = (bbox[1] * inv).floor() as i32;
        let max_x = (bbox[2] * inv).floor() as i32;
        let max_y = (bbox[3] * inv).floor() as i32;
        let mut seen = HashSet::<usize>::new();
        let mut out = Vec::<usize>::new();
        for bx in min_x..=max_x {
            for by in min_y..=max_y {
                if let Some(indices) = self.buckets.get(&(bx, by)) {
                    for idx in indices {
                        if seen.insert(*idx) {
                            out.push(*idx);
                        }
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default)]
struct CellFeatureAgg {
    node_count: u64,
    stop_count: u64,
    amenity_count: u64,
    shop_count: u64,
    office_count: u64,
    industrial_count: u64,
    retail_count: u64,
    recreation_count: u64,
    leisure_count: u64,
    tourism_count: u64,
    education_count: u64,
    health_count: u64,
    residential_count: u64,
    highway_count: u64,
}

#[derive(Debug, Clone)]
struct RasterSample {
    lon: f64,
    lat: f64,
    value: f64,
}

#[derive(Debug, Clone)]
struct RasterBucketIndex {
    bucket_deg: f64,
    samples: Vec<RasterSample>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
}

impl RasterBucketIndex {
    fn new(samples: Vec<RasterSample>, bucket_deg: f64) -> Self {
        let mut buckets = HashMap::<(i32, i32), Vec<usize>>::new();
        for (idx, sample) in samples.iter().enumerate() {
            let key = raster_bucket_key(sample.lon, sample.lat, bucket_deg);
            buckets.entry(key).or_default().push(idx);
        }
        Self {
            bucket_deg,
            samples,
            buckets,
        }
    }

    fn nearest_value(&self, lon: f64, lat: f64) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let (bx, by) = raster_bucket_key(lon, lat, self.bucket_deg);
        let mut best: Option<(f64, f64)> = None;
        let max_ring = 12_i32;
        for ring in 0..=max_ring {
            let mut found_in_ring = false;
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    if let Some(indices) = self.buckets.get(&(bx + dx, by + dy)) {
                        found_in_ring = true;
                        for idx in indices {
                            let sample = &self.samples[*idx];
                            let dlon = sample.lon - lon;
                            let dlat = sample.lat - lat;
                            let d2 = dlon * dlon + dlat * dlat;
                            match best {
                                None => best = Some((d2, sample.value)),
                                Some((best_d2, _)) if d2 < best_d2 => {
                                    best = Some((d2, sample.value))
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if found_in_ring && best.is_some() {
                break;
            }
        }
        best.map(|(_, value)| value)
    }
}

fn raster_bucket_key(lon: f64, lat: f64, bucket_deg: f64) -> (i32, i32) {
    let inv = 1.0 / bucket_deg.max(1e-9);
    ((lon * inv).floor() as i32, (lat * inv).floor() as i32)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
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

fn geojson_coords_to_polygon(coords: &[Vec<Vec<f64>>]) -> Option<Polygon<f64>> {
    let exterior = coords.first()?;
    let exterior_ring = close_ring_coords(
        exterior
            .iter()
            .filter_map(|xy| (xy.len() >= 2).then_some((xy[0], xy[1])))
            .collect(),
    );
    if exterior_ring.len() < 4 {
        return None;
    }
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

fn multipolygon_bbox(geometry: &MultiPolygon<f64>) -> Option<[f64; 4]> {
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
    Some([min_lon, min_lat, max_lon, max_lat])
}

fn load_uk_counties_canonical() -> Result<Vec<CountyBoundary>, String> {
    let index_path = repo_root()
        .join("data")
        .join("boundaries")
        .join("uk_counties_index.json");
    let counties_path = repo_root()
        .join("data")
        .join("boundaries")
        .join("uk_counties_canonical.geojson");
    let raw =
        fs::read_to_string(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let index: UkCountyIndexFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let by_id = index
        .counties
        .iter()
        .cloned()
        .map(|entry| (entry.county_id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let counties_raw = fs::read_to_string(&counties_path)
        .map_err(|e| format!("{}: {e}", counties_path.display()))?;
    let GeoJson::FeatureCollection(fc) =
        counties_raw.parse::<GeoJson>().map_err(|e| e.to_string())?
    else {
        return Err("uk_counties_canonical.geojson must be a FeatureCollection".to_string());
    };
    let mut out = Vec::<CountyBoundary>::new();
    for feature in fc.features {
        let Some(props) = feature.properties.as_ref() else {
            continue;
        };
        let Some(county_id) = props.get("county_id").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(entry) = by_id.get(county_id) else {
            continue;
        };
        let Some(geom) = feature.geometry else {
            continue;
        };
        let geometry = match geom.value {
            Value::Polygon(coords) => {
                let Some(poly) = geojson_coords_to_polygon(&coords) else {
                    continue;
                };
                MultiPolygon(vec![poly])
            }
            Value::MultiPolygon(multi) => {
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
        }
        .simplify(&0.0018);
        let Some(bbox) = multipolygon_bbox(&geometry) else {
            continue;
        };
        out.push(CountyBoundary {
            county_id: entry.county_id.clone(),
            name: entry.name.clone(),
            nation: entry.nation.clone(),
            country_iso2: entry.country_iso2.clone(),
            source_code: entry.source_code.clone(),
            geometry,
            bbox,
        });
    }
    Ok(out)
}

fn bbox_intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

fn expand_bbox(bbox: [f64; 4], pad_deg: f64) -> [f64; 4] {
    [
        bbox[0] - pad_deg,
        bbox[1] - pad_deg,
        bbox[2] + pad_deg,
        bbox[3] + pad_deg,
    ]
}

fn line_bbox(coords: &[(f64, f64)]) -> Option<[f64; 4]> {
    if coords.is_empty() {
        return None;
    }
    let mut min_lon = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for &(lon, lat) in coords {
        min_lon = min_lon.min(lon);
        min_lat = min_lat.min(lat);
        max_lon = max_lon.max(lon);
        max_lat = max_lat.max(lat);
    }
    Some([min_lon, min_lat, max_lon, max_lat])
}

fn multipolygon_to_geojson_geometry(geometry: &MultiPolygon<f64>) -> Geometry {
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
                    .collect::<Vec<_>>(),
            );
            for interior in polygon.interiors() {
                rings.push(
                    interior
                        .points()
                        .map(|point| vec![point.x(), point.y()])
                        .collect::<Vec<_>>(),
                );
            }
            rings
        })
        .collect::<Vec<_>>();
    Geometry::new(Value::MultiPolygon(coordinates))
}

fn simplify_geojson_geometry(geometry: Geometry, tolerance: f64) -> Option<Geometry> {
    match geometry.value {
        Value::Polygon(coords) => {
            let polygon = geojson_coords_to_polygon(&coords)?;
            Some(multipolygon_to_geojson_geometry(
                &MultiPolygon(vec![polygon]).simplify(&tolerance),
            ))
        }
        Value::MultiPolygon(multi) => {
            let polygons = multi
                .iter()
                .filter_map(|coords| geojson_coords_to_polygon(coords))
                .collect::<Vec<_>>();
            if polygons.is_empty() {
                return None;
            }
            Some(multipolygon_to_geojson_geometry(
                &MultiPolygon(polygons).simplify(&tolerance),
            ))
        }
        _ => Some(geometry),
    }
}

fn road_class(tags: &Tags) -> Option<&'static str> {
    match tags.get("highway").map(|value| value.as_str()) {
        Some("motorway") => Some("motorway"),
        Some("motorway_link") => Some("motorway_link"),
        Some("trunk") => Some("trunk"),
        Some("trunk_link") => Some("trunk_link"),
        Some("primary") => Some("primary"),
        Some("primary_link") => Some("primary_link"),
        Some("secondary") => Some("secondary"),
        Some("secondary_link") => Some("secondary_link"),
        Some("tertiary") => Some("tertiary"),
        Some("tertiary_link") => Some("tertiary_link"),
        Some("unclassified") => Some("unclassified"),
        Some("residential") => Some("residential"),
        Some("living_street") => Some("living_street"),
        Some("service") => Some("service"),
        Some("pedestrian") => Some("pedestrian"),
        _ => None,
    }
}

fn is_major_road_class(road_class: &str) -> bool {
    matches!(
        road_class,
        "motorway" | "motorway_link" | "trunk" | "trunk_link" | "primary" | "primary_link"
    )
}

fn keep_full_basemap_road_class(road_class: &str) -> bool {
    matches!(
        road_class,
        "motorway"
            | "motorway_link"
            | "trunk"
            | "trunk_link"
            | "primary"
            | "primary_link"
            | "secondary"
            | "secondary_link"
            | "tertiary"
            | "tertiary_link"
            | "unclassified"
            | "residential"
            | "living_street"
            | "service"
            | "pedestrian"
    )
}

fn keep_mid_basemap_road_class(road_class: &str) -> bool {
    matches!(
        road_class,
        "motorway"
            | "motorway_link"
            | "trunk"
            | "trunk_link"
            | "primary"
            | "primary_link"
            | "secondary"
            | "secondary_link"
            | "tertiary"
            | "tertiary_link"
    )
}

fn rail_class(tags: &Tags) -> Option<&'static str> {
    match tags.get("railway").map(|value| value.as_str()) {
        Some("rail") => Some("rail"),
        Some("light_rail") => Some("light_rail"),
        Some("subway") => Some("subway"),
        Some("tram") => Some("tram"),
        _ => None,
    }
}

fn landuse_class(tags: &Tags) -> Option<&'static str> {
    match tags.get("landuse").map(|value| value.as_str()) {
        Some("residential") => Some("residential"),
        Some("industrial") => Some("industrial"),
        Some("commercial") => Some("commercial"),
        Some("retail") => Some("retail"),
        Some("forest") => Some("forest"),
        Some("grass") => Some("grass"),
        Some("meadow") => Some("meadow"),
        Some("recreation_ground") => Some("park"),
        _ => match tags.get("leisure").map(|value| value.as_str()) {
            Some("park") | Some("garden") | Some("golf_course") | Some("pitch") => Some("park"),
            _ => match tags.get("natural").map(|value| value.as_str()) {
                Some("wood") => Some("forest"),
                Some("heath") | Some("scrub") | Some("grassland") => Some("natural"),
                _ => None,
            },
        },
    }
}

fn water_class(tags: &Tags) -> Option<&'static str> {
    if matches!(
        tags.get("natural").map(|value| value.as_str()),
        Some("water") | Some("wetland")
    ) {
        return Some("water");
    }
    if matches!(
        tags.get("waterway").map(|value| value.as_str()),
        Some("riverbank") | Some("dock")
    ) {
        return Some("water");
    }
    if matches!(
        tags.get("landuse").map(|value| value.as_str()),
        Some("reservoir") | Some("basin")
    ) {
        return Some("water");
    }
    tags.get("water").map(|_| "water")
}

fn way_coords(way: &Way, nodes: &HashMap<NodeId, Node>) -> Vec<(f64, f64)> {
    way.nodes
        .iter()
        .filter_map(|node_id| nodes.get(node_id).map(|node| (node.lon(), node.lat())))
        .collect::<Vec<_>>()
}

fn closed_way_coords(way: &Way, nodes: &HashMap<NodeId, Node>) -> Vec<(f64, f64)> {
    let mut coords = way_coords(way, nodes);
    if coords.len() >= 3 && !point_eq(coords[0], *coords.last().unwrap_or(&coords[0])) {
        coords.push(coords[0]);
    }
    coords
}

fn point_in_county(geometry: &MultiPolygon<f64>, lon: f64, lat: f64) -> bool {
    geometry.contains(&Point::new(lon, lat))
}

fn segment_hits_county(a: (f64, f64), b: (f64, f64), county: &CountyBoundary) -> bool {
    let segment_bbox = [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)];
    if !bbox_intersects(expand_bbox(segment_bbox, 0.0012), county.bbox) {
        return false;
    }
    if point_in_county(&county.geometry, a.0, a.1) || point_in_county(&county.geometry, b.0, b.1) {
        return true;
    }
    let mid_lon = (a.0 + b.0) * 0.5;
    let mid_lat = (a.1 + b.1) * 0.5;
    point_in_county(&county.geometry, mid_lon, mid_lat)
}

fn extract_county_segments(coords: &[(f64, f64)], county: &CountyBoundary) -> Vec<Vec<(f64, f64)>> {
    if coords.len() < 2 {
        return Vec::new();
    }
    let inside = coords
        .iter()
        .map(|(lon, lat)| point_in_county(&county.geometry, *lon, *lat))
        .collect::<Vec<_>>();
    let mut out = Vec::<Vec<(f64, f64)>>::new();
    let mut current = Vec::<(f64, f64)>::new();
    for (idx, pair) in coords.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let hits = inside[idx] || inside[idx + 1] || segment_hits_county(a, b, county);
        if hits {
            if current
                .last()
                .map(|last| !point_eq(*last, a))
                .unwrap_or(true)
            {
                current.push(a);
            }
            if !point_eq(*current.last().unwrap_or(&b), b) {
                current.push(b);
            }
        } else if current.len() >= 2 {
            out.push(current.clone());
            current.clear();
        } else {
            current.clear();
        }
    }
    if current.len() >= 2 {
        out.push(current);
    }
    out
}

fn simplify_line_coords(coords: &[(f64, f64)], tolerance: f64) -> Vec<Vec<f64>> {
    if coords.len() < 2 {
        return vec![];
    }
    let simplified = LineString::from(coords.to_vec()).simplify(&tolerance);
    let points = simplified
        .points()
        .map(|point| vec![point.x(), point.y()])
        .collect::<Vec<_>>();
    if points.len() >= 2 {
        points
    } else {
        coords.iter().map(|(lon, lat)| vec![*lon, *lat]).collect()
    }
}

fn simplify_polygon_coords(coords: &[(f64, f64)], tolerance: f64) -> Vec<Vec<Vec<f64>>> {
    if coords.len() < 4 {
        return Vec::new();
    }
    let polygon = Polygon::new(LineString::from(coords.to_vec()), vec![]);
    let simplified = polygon.simplify(&tolerance);
    let exterior = simplified
        .exterior()
        .points()
        .map(|point| vec![point.x(), point.y()])
        .collect::<Vec<_>>();
    if exterior.len() < 4 {
        vec![coords.iter().map(|(lon, lat)| vec![*lon, *lat]).collect()]
    } else {
        vec![exterior]
    }
}

fn line_feature_with_props(
    coords: &[(f64, f64)],
    properties: JsonMap<String, JsonValue>,
    tolerance: f64,
) -> Option<Feature> {
    let coordinates = simplify_line_coords(coords, tolerance);
    if coordinates.len() < 2 {
        return None;
    }
    Some(Feature {
        bbox: None,
        geometry: Some(Geometry::new(Value::LineString(coordinates))),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    })
}

fn polygon_feature_with_props(
    coords: &[(f64, f64)],
    properties: JsonMap<String, JsonValue>,
    tolerance: f64,
) -> Option<Feature> {
    let coordinates = simplify_polygon_coords(coords, tolerance);
    if coordinates.is_empty() {
        return None;
    }
    Some(Feature {
        bbox: None,
        geometry: Some(Geometry::new(Value::Polygon(coordinates))),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    })
}

fn road_feature(coords: &[(f64, f64)], road_class: &str, tolerance: f64) -> Option<Feature> {
    let mut props = JsonMap::<String, JsonValue>::new();
    props.insert(
        "feature_layer".to_string(),
        JsonValue::String("road".to_string()),
    );
    props.insert(
        "road_class".to_string(),
        JsonValue::String(road_class.to_string()),
    );
    line_feature_with_props(coords, props, tolerance)
}

fn rail_feature(coords: &[(f64, f64)], rail_class: &str, tolerance: f64) -> Option<Feature> {
    let mut props = JsonMap::<String, JsonValue>::new();
    props.insert(
        "feature_layer".to_string(),
        JsonValue::String("rail".to_string()),
    );
    props.insert(
        "rail_class".to_string(),
        JsonValue::String(rail_class.to_string()),
    );
    line_feature_with_props(coords, props, tolerance)
}

fn polygon_context_feature(
    coords: &[(f64, f64)],
    feature_layer: &str,
    class_key: &str,
    class_value: &str,
    tolerance: f64,
) -> Option<Feature> {
    let mut props = JsonMap::<String, JsonValue>::new();
    props.insert(
        "feature_layer".to_string(),
        JsonValue::String(feature_layer.to_string()),
    );
    props.insert(
        class_key.to_string(),
        JsonValue::String(class_value.to_string()),
    );
    polygon_feature_with_props(coords, props, tolerance)
}

fn polygon_hits_county(coords: &[(f64, f64)], county: &CountyBoundary) -> bool {
    let Some(bbox) = line_bbox(coords) else {
        return false;
    };
    if !bbox_intersects(expand_bbox(bbox, 0.0015), county.bbox) {
        return false;
    }
    for (lon, lat) in coords {
        if point_in_county(&county.geometry, *lon, *lat) {
            return true;
        }
    }
    let center_lon = (bbox[0] + bbox[2]) * 0.5;
    let center_lat = (bbox[1] + bbox[3]) * 0.5;
    point_in_county(&county.geometry, center_lon, center_lat)
}

fn build_world_context_geojson(
    country_boundaries_geojson: &str,
    out_path: &Path,
) -> Result<(), String> {
    let normalize_iso = |value: Option<&str>| -> Option<String> {
        let iso = value?.trim().to_ascii_uppercase();
        if iso.len() != 2 || iso == "-99" || !iso.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return None;
        }
        Some(iso)
    };
    let raw = fs::read_to_string(country_boundaries_geojson)
        .map_err(|e| format!("{}: {e}", country_boundaries_geojson))?;
    let GeoJson::FeatureCollection(fc) = raw.parse::<GeoJson>().map_err(|e| e.to_string())? else {
        return Err("country_boundaries_geojson must be a FeatureCollection".to_string());
    };
    let features = fc
        .features
        .into_iter()
        .filter_map(|feature| {
            let geometry = feature.geometry?;
            let props = feature.properties?;
            let iso = normalize_iso(props.get("ISO_A2").and_then(|value| value.as_str())).or_else(
                || normalize_iso(props.get("ISO_A2_EH").and_then(|value| value.as_str())),
            )?;
            let canonical_iso = canonical_country_iso2(&iso).unwrap_or(iso);
            let name = props
                .get("NAME_EN")
                .and_then(|value| value.as_str())
                .or_else(|| props.get("ADMIN").and_then(|value| value.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            let simplified_geometry = simplify_geojson_geometry(geometry, 0.08)?;
            let mut mapped = JsonMap::<String, JsonValue>::new();
            mapped.insert(
                "country_iso2".to_string(),
                JsonValue::String(canonical_iso.clone()),
            );
            mapped.insert("name".to_string(), JsonValue::String(name));
            mapped.insert(
                "playable_now".to_string(),
                JsonValue::Bool(is_uk_country_iso2(&canonical_iso)),
            );
            mapped.insert(
                "coming_soon".to_string(),
                JsonValue::Bool(!is_uk_country_iso2(&canonical_iso)),
            );
            Some(Feature {
                bbox: None,
                geometry: Some(simplified_geometry),
                id: None,
                properties: Some(mapped),
                foreign_members: None,
            })
        })
        .collect::<Vec<_>>();
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        out_path,
        GeoJson::FeatureCollection(FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        })
        .to_string(),
    )
    .map_err(|e| format!("{}: {e}", out_path.display()))
}

fn build_country_map_artifacts(
    country_iso2: &str,
    pbf_path: &str,
    country_boundaries_geojson: &str,
    out_dir: &Path,
) -> Result<(), String> {
    let Some(canonical_iso) = canonical_country_iso2(country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    if !is_uk_country_iso2(&canonical_iso) {
        return Err(format!(
            "build-country-map-assets currently supports UK canonical map pack generation only (got {canonical_iso})"
        ));
    }
    let counties = load_uk_counties_canonical()?;
    let county_index = CountyBucketIndex::new(&counties, 0.5);
    let map_dir = out_dir.join("map");
    let county_roads_dir = map_dir.join("county_roads");
    let county_basemap_mid_dir = map_dir.join("county_basemap_mid");
    let county_basemap_full_dir = map_dir.join("county_basemap_full");
    fs::create_dir_all(&county_roads_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&county_basemap_mid_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&county_basemap_full_dir).map_err(|e| e.to_string())?;

    build_world_context_geojson(
        country_boundaries_geojson,
        &map_dir.join("world_context.geojson"),
    )?;

    let reader = File::open(pbf_path).map_err(|e| format!("{pbf_path}: {e}"))?;
    let mut pbf = OsmPbfReader::new(reader);
    let objects = pbf
        .get_objs_and_deps(|obj| {
            obj.way()
                .map(|way| {
                    road_class(&way.tags).is_some()
                        || rail_class(&way.tags).is_some()
                        || landuse_class(&way.tags).is_some()
                        || water_class(&way.tags).is_some()
                })
                .unwrap_or(false)
        })
        .map_err(|e| e.to_string())?;

    let mut nodes = HashMap::<NodeId, Node>::new();
    let mut ways = Vec::<Way>::new();
    for obj in objects.into_values() {
        match obj {
            OsmObj::Node(node) => {
                nodes.insert(node.id, node);
            }
            OsmObj::Way(way) => ways.push(way),
            _ => {}
        }
    }

    let mut major_features = Vec::<Feature>::new();
    let mut county_roads_features = counties
        .iter()
        .map(|county| (county.county_id.clone(), Vec::<Feature>::new()))
        .collect::<HashMap<_, _>>();
    let mut county_mid_features = counties
        .iter()
        .map(|county| (county.county_id.clone(), Vec::<Feature>::new()))
        .collect::<HashMap<_, _>>();
    let mut county_full_features = counties
        .iter()
        .map(|county| (county.county_id.clone(), Vec::<Feature>::new()))
        .collect::<HashMap<_, _>>();

    for way in &ways {
        let road = road_class(&way.tags);
        let rail = rail_class(&way.tags);
        let water = water_class(&way.tags);
        let landuse = landuse_class(&way.tags);
        if road.is_none() && rail.is_none() && water.is_none() && landuse.is_none() {
            continue;
        }
        if let Some(class) = road {
            let coords = way_coords(way, &nodes);
            if coords.len() < 2 {
                continue;
            }
            let Some(way_bbox) = line_bbox(&coords) else {
                continue;
            };
            if is_major_road_class(class) {
                if let Some(feature) = road_feature(&coords, class, 0.0018) {
                    major_features.push(feature);
                }
            }
            let candidate_counties =
                county_index.candidate_indices_for_bbox(expand_bbox(way_bbox, 0.002));
            let full_tolerance = match class {
                "motorway" | "motorway_link" | "trunk" | "trunk_link" => 0.00045,
                "primary" | "primary_link" => 0.00028,
                "secondary" | "secondary_link" => 0.00018,
                "tertiary" | "tertiary_link" => 0.00012,
                _ => 0.00008,
            };
            let mid_tolerance = match class {
                "motorway" | "motorway_link" | "trunk" | "trunk_link" => 0.0008,
                "primary" | "primary_link" => 0.00048,
                "secondary" | "secondary_link" => 0.00032,
                _ => 0.00024,
            };
            for county_idx in candidate_counties {
                let county = &counties[county_idx];
                if !bbox_intersects(expand_bbox(way_bbox, 0.002), county.bbox) {
                    continue;
                }
                let segments = extract_county_segments(&coords, county);
                if segments.is_empty() {
                    continue;
                }
                for segment in segments {
                    if keep_full_basemap_road_class(class) {
                        if let Some(feature) = road_feature(&segment, class, full_tolerance) {
                            if let Some(bucket) = county_roads_features.get_mut(&county.county_id) {
                                bucket.push(feature.clone());
                            }
                            if let Some(bucket) = county_full_features.get_mut(&county.county_id) {
                                bucket.push(feature);
                            }
                        }
                    }
                    if keep_mid_basemap_road_class(class) {
                        if let Some(feature) = road_feature(&segment, class, mid_tolerance) {
                            if let Some(bucket) = county_mid_features.get_mut(&county.county_id) {
                                bucket.push(feature);
                            }
                        }
                    }
                }
            }
            continue;
        }

        if let Some(class) = rail {
            let coords = way_coords(way, &nodes);
            if coords.len() < 2 {
                continue;
            }
            let Some(way_bbox) = line_bbox(&coords) else {
                continue;
            };
            let candidate_counties =
                county_index.candidate_indices_for_bbox(expand_bbox(way_bbox, 0.002));
            for county_idx in candidate_counties {
                let county = &counties[county_idx];
                if !bbox_intersects(expand_bbox(way_bbox, 0.002), county.bbox) {
                    continue;
                }
                let segments = extract_county_segments(&coords, county);
                if segments.is_empty() {
                    continue;
                }
                for segment in segments {
                    if let Some(feature) = rail_feature(&segment, class, 0.00018) {
                        if let Some(bucket) = county_full_features.get_mut(&county.county_id) {
                            bucket.push(feature);
                        }
                    }
                    if let Some(feature) = rail_feature(&segment, class, 0.00048) {
                        if let Some(bucket) = county_mid_features.get_mut(&county.county_id) {
                            bucket.push(feature);
                        }
                    }
                }
            }
            continue;
        }

        let polygon_coords = closed_way_coords(way, &nodes);
        if polygon_coords.len() < 4 {
            continue;
        }
        let Some(polygon_bbox) = line_bbox(&polygon_coords) else {
            continue;
        };
        let candidate_counties =
            county_index.candidate_indices_for_bbox(expand_bbox(polygon_bbox, 0.002));
        for county_idx in candidate_counties {
            let county = &counties[county_idx];
            if !polygon_hits_county(&polygon_coords, county) {
                continue;
            }
            if let Some(class) = water {
                if let Some(feature) =
                    polygon_context_feature(&polygon_coords, "water", "water_class", class, 0.0008)
                {
                    if let Some(bucket) = county_mid_features.get_mut(&county.county_id) {
                        bucket.push(feature.clone());
                    }
                    if let Some(bucket) = county_full_features.get_mut(&county.county_id) {
                        bucket.push(feature);
                    }
                }
                continue;
            }
            if let Some(class) = landuse {
                if let Some(feature) = polygon_context_feature(
                    &polygon_coords,
                    "landuse",
                    "landuse_class",
                    class,
                    0.0008,
                ) {
                    if let Some(bucket) = county_mid_features.get_mut(&county.county_id) {
                        bucket.push(feature.clone());
                    }
                    if let Some(bucket) = county_full_features.get_mut(&county.county_id) {
                        bucket.push(feature);
                    }
                }
            }
        }
    }

    let major_roads_geojson = GeoJson::FeatureCollection(FeatureCollection {
        bbox: None,
        features: major_features,
        foreign_members: None,
    })
    .to_string();
    fs::write(map_dir.join("major_roads.geojson"), &major_roads_geojson)
        .map_err(|e| e.to_string())?;
    // Compatibility output retained for legacy GB-prefixed loaders.
    fs::write(map_dir.join("gb_major_roads.geojson"), &major_roads_geojson)
        .map_err(|e| e.to_string())?;

    for county in &counties {
        let road_features = county_roads_features
            .remove(&county.county_id)
            .unwrap_or_default();
        let mid_features = county_mid_features
            .remove(&county.county_id)
            .unwrap_or_default();
        let full_features = county_full_features
            .remove(&county.county_id)
            .unwrap_or_default();
        fs::write(
            county_roads_dir.join(format!("{}.geojson", county.county_id)),
            GeoJson::FeatureCollection(FeatureCollection {
                bbox: None,
                features: road_features,
                foreign_members: None,
            })
            .to_string(),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            county_basemap_mid_dir.join(format!("{}.geojson", county.county_id)),
            GeoJson::FeatureCollection(FeatureCollection {
                bbox: None,
                features: mid_features,
                foreign_members: None,
            })
            .to_string(),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            county_basemap_full_dir.join(format!("{}.geojson", county.county_id)),
            GeoJson::FeatureCollection(FeatureCollection {
                bbox: None,
                features: full_features,
                foreign_members: None,
            })
            .to_string(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Legacy helper retained for test/compatibility callers that still use GB naming.
#[allow(dead_code)]
fn build_gb_map_artifacts(
    pbf_path: &str,
    country_boundaries_geojson: &str,
    out_dir: &Path,
) -> Result<(), String> {
    build_country_map_artifacts(
        CANONICAL_UK_ISO2,
        pbf_path,
        country_boundaries_geojson,
        out_dir,
    )
}

fn run_build_country_map_assets(
    country_iso2: &str,
    pbf_path: &str,
    country_boundaries_geojson: &str,
    out_dir: &str,
) -> Result<(), String> {
    let out_root = Path::new(out_dir);
    fs::create_dir_all(out_root).map_err(|e| e.to_string())?;
    build_country_map_artifacts(country_iso2, pbf_path, country_boundaries_geojson, out_root)?;
    println!(
        "Country map assets built: iso={} -> {}",
        canonical_country_iso2(country_iso2).unwrap_or_else(|| country_iso2.to_ascii_uppercase()),
        out_root.display()
    );
    Ok(())
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::ImportPbf {
            pbf,
            name,
            out_root,
            bbox,
            max_stops,
            snap_m,
            inferred_headway_s,
            cleanup_topology,
            infer_services,
        } => {
            let options = ImportPbfOptions {
                pbf_path: pbf,
                scenario_name: name,
                out_root,
                bbox,
                max_stops,
                snap_m,
                inferred_headway_s,
                cleanup_topology,
                infer_services,
            };
            run_import_pbf(options)
        }
        Commands::AttachCensus {
            scenario,
            csv,
            out,
            profile_csv,
            replace_zones,
        } => run_attach_census(
            &scenario,
            &csv,
            out.as_deref(),
            profile_csv.as_deref(),
            replace_zones,
        ),
        Commands::NormalizeLsoa {
            population_csv,
            jobs_csv,
            centroids_csv,
            out,
            region,
            target_crs,
        } => run_normalize_lsoa(
            &population_csv,
            &jobs_csv,
            &centroids_csv,
            &out,
            &region,
            &target_crs,
        ),
        Commands::ImportGtfs {
            scenario,
            gtfs_dir,
            out,
            snap_m,
            default_headway_s,
        } => run_import_gtfs(
            &scenario,
            &gtfs_dir,
            out.as_deref(),
            snap_m,
            default_headway_s,
        ),
        Commands::BuildLocationCatalog {
            country_info,
            cities,
            out,
        } => run_build_location_catalog(&country_info, &cities, &out),
        Commands::BuildDemandFabric {
            pbf,
            out,
            bbox,
            country_iso2,
            h3_res,
            target_crs,
            population_raster_csv,
            built_raster_csv,
            country_boundaries_geojson,
            raster_only,
        } => run_build_demand_fabric(BuildDemandFabricOptions {
            pbf_path: pbf,
            out_path: out,
            bbox,
            country_iso2,
            h3_res,
            target_crs,
            population_raster_csv,
            built_raster_csv,
            country_boundaries_geojson,
            raster_only,
        }),
        Commands::ApplyDemandFabric {
            scenario,
            fabric,
            out,
            replace_zones,
        } => run_apply_demand_fabric(&scenario, &fabric, out.as_deref(), replace_zones),
        Commands::BuildDemandSurface {
            pbf,
            country_iso2,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        } => run_build_demand_surface(BuildDemandSurfaceOptions {
            pbf_path: pbf,
            country_iso2,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out_path: out,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        }),
        Commands::BuildDemandSurfacePack {
            pbf,
            countries,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out_dir,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        } => run_build_demand_surface_pack(BuildDemandSurfacePackOptions {
            pbf_path: pbf,
            countries,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out_dir,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        }),
        Commands::BuildCountryPack {
            pbf,
            country_iso2,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out_dir,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        } => run_build_country_pack(BuildCountryPackOptions {
            pbf_path: pbf,
            country_iso2,
            country_boundaries_geojson,
            population_raster_csv,
            built_raster_csv,
            out_dir,
            h3_res,
            target_crs,
            bbox,
            raster_only,
        }),
        Commands::BuildCountryMapAssets {
            pbf,
            country_boundaries_geojson,
            out_dir,
        } => run_build_country_map_assets(
            CANONICAL_UK_ISO2,
            &pbf,
            &country_boundaries_geojson,
            &out_dir,
        ),
        Commands::ValidateCountryPack { pack_dir } => run_validate_country_pack(&pack_dir),
    }
}

fn run_build_location_catalog(
    country_info_path: &str,
    cities_path: &str,
    out_dir: &str,
) -> Result<(), String> {
    let countries = parse_country_info(country_info_path)?;
    let mut cities_by_country = parse_geonames_cities(cities_path)?;

    let mut country_list: Vec<CatalogCountry> = countries.values().cloned().collect();
    country_list.sort_by(|a, b| a.name.cmp(&b.name));

    for cities in cities_by_country.values_mut() {
        cities.sort_by(|a, b| {
            b.population
                .cmp(&a.population)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    let capitals: HashMap<String, i64> = country_list
        .iter()
        .filter_map(|c| c.capital_geonameid.map(|gid| (c.iso2.clone(), gid)))
        .collect();

    let out_path = Path::new(out_dir);
    let cities_path = out_path.join("cities");
    fs::create_dir_all(&cities_path).map_err(|e| e.to_string())?;

    let countries_json = serde_json::to_string_pretty(&country_list).map_err(|e| e.to_string())?;
    fs::write(out_path.join("countries.json"), countries_json).map_err(|e| e.to_string())?;

    let capitals_json = serde_json::to_string_pretty(&capitals).map_err(|e| e.to_string())?;
    fs::write(out_path.join("capitals.json"), capitals_json).map_err(|e| e.to_string())?;

    let mut city_count = 0usize;
    let mut iso_codes: Vec<String> = cities_by_country.keys().cloned().collect();
    iso_codes.sort();
    for iso in iso_codes {
        let rows = cities_by_country.remove(&iso).unwrap_or_default();
        city_count += rows.len();
        let payload = serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?;
        fs::write(cities_path.join(format!("{iso}.json")), payload).map_err(|e| e.to_string())?;
    }

    let meta = CatalogMeta {
        schema_version: 1,
        generated_at_epoch_s: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        country_count: country_list.len(),
        city_count,
    };
    fs::write(
        out_path.join("metadata.json"),
        serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    println!(
        "Location catalog built: countries={}, cities={} -> {}",
        meta.country_count,
        meta.city_count,
        out_path.display()
    );
    Ok(())
}

fn parse_iso2_from_feature_properties(
    props: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let keys = [
        "iso2",
        "ISO2",
        "ISO_A2",
        "iso_a2",
        "country_iso2",
        "COUNTRY",
    ];
    for key in keys {
        if let Some(code) = props
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| s.len() == 2 && s.chars().all(|ch| ch.is_ascii_alphabetic()))
        {
            return Some(code);
        }
    }
    None
}

fn close_ring(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if pts.len() < 3 {
        return pts;
    }
    if pts.first() != pts.last() {
        let first = pts[0];
        pts.push(first);
    }
    pts
}

fn to_polygon(coords: &[Vec<Vec<f64>>]) -> Option<Polygon<f64>> {
    if coords.is_empty() {
        return None;
    }
    let ext = close_ring(
        coords[0]
            .iter()
            .filter_map(|xy| {
                if xy.len() >= 2 {
                    Some((xy[0], xy[1]))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
    );
    if ext.len() < 4 {
        return None;
    }
    let exterior = LineString::from(ext);
    let mut interiors = Vec::<LineString<f64>>::new();
    for ring in coords.iter().skip(1) {
        let pts = close_ring(
            ring.iter()
                .filter_map(|xy| {
                    if xy.len() >= 2 {
                        Some((xy[0], xy[1]))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
        );
        if pts.len() >= 4 {
            interiors.push(LineString::from(pts));
        }
    }
    Some(Polygon::new(exterior, interiors))
}

fn parse_country_boundaries(path: &str) -> Result<HashMap<String, MultiPolygon<f64>>, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let gj = raw.parse::<GeoJson>().map_err(|e| e.to_string())?;
    let mut out = HashMap::<String, MultiPolygon<f64>>::new();
    let GeoJson::FeatureCollection(fc) = gj else {
        return Err("country_boundaries_geojson must be a FeatureCollection".to_string());
    };
    for feat in fc.features {
        let Some(props) = feat.properties else {
            continue;
        };
        let Some(iso) = parse_iso2_from_feature_properties(&props) else {
            continue;
        };
        let Some(geom) = feat.geometry else { continue };
        let polys = match geom.value {
            geojson::Value::Polygon(coords) => {
                let Some(poly) = to_polygon(&coords) else {
                    continue;
                };
                vec![poly]
            }
            geojson::Value::MultiPolygon(multi) => {
                let mut polys = Vec::<Polygon<f64>>::new();
                for coords in multi {
                    if let Some(poly) = to_polygon(&coords) {
                        polys.push(poly);
                    }
                }
                if polys.is_empty() {
                    continue;
                }
                polys
            }
            _ => continue,
        };
        out.insert(iso, MultiPolygon(polys));
    }
    if out.is_empty() {
        return Err(format!(
            "no valid country polygons found in {}",
            Path::new(path).display()
        ));
    }
    if let Some(poly) = out.get(UK_COMPAT_GB_ISO2).cloned() {
        out.entry(CANONICAL_UK_ISO2.to_string()).or_insert(poly);
    }
    Ok(out)
}

fn point_is_in_country(
    lon: f64,
    lat: f64,
    target_iso2: Option<&str>,
    boundaries: Option<&HashMap<String, MultiPolygon<f64>>>,
) -> bool {
    let (Some(iso), Some(map)) = (target_iso2, boundaries) else {
        return true;
    };
    let Some(poly) = map.get(iso) else {
        return false;
    };
    poly.contains(&Point::new(lon, lat))
}

fn build_demand_fabric_payload(options: &BuildDemandFabricOptions) -> Result<DemandFabric, String> {
    let pbf = Path::new(&options.pbf_path);
    if !pbf.exists() {
        return Err(format!("PBF not found: {}", pbf.display()));
    }
    let bbox = parse_bbox_or_world(options.bbox.as_deref())?;
    let resolution = Resolution::try_from(options.h3_res)
        .map_err(|_| format!("invalid h3 resolution: {}", options.h3_res))?;
    let country = options
        .country_iso2
        .as_deref()
        .and_then(canonical_country_iso2);
    let boundary_country = country.as_deref().map(boundaries_query_iso2);
    let boundaries = if let Some(path) = options.country_boundaries_geojson.as_deref() {
        Some(parse_country_boundaries(path)?)
    } else {
        None
    };
    if let (Some(iso), Some(map)) = (boundary_country.as_deref(), boundaries.as_ref()) {
        if !map.contains_key(iso) {
            return Err(format!("country '{iso}' not found in boundaries geojson"));
        }
    }

    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    let in_scope = |lon: f64, lat: f64| -> bool {
        if lon < min_lon || lon > max_lon || lat < min_lat || lat > max_lat {
            return false;
        }
        point_is_in_country(lon, lat, boundary_country.as_deref(), boundaries.as_ref())
    };

    let pop_samples_raw = if let Some(path) = options.population_raster_csv.as_deref() {
        read_raster_csv(path)?
            .into_iter()
            .filter(|s| in_scope(s.lon, s.lat))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let built_samples_raw = if let Some(path) = options.built_raster_csv.as_deref() {
        read_raster_csv(path)?
            .into_iter()
            .filter(|s| in_scope(s.lon, s.lat))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if options.raster_only && pop_samples_raw.is_empty() && built_samples_raw.is_empty() {
        return Err("raster-only mode requires population and/or built raster samples".to_string());
    }
    let pop_samples = if pop_samples_raw.is_empty() {
        None
    } else {
        Some(RasterBucketIndex::new(pop_samples_raw.clone(), 0.25))
    };
    let built_samples = if built_samples_raw.is_empty() {
        None
    } else {
        Some(RasterBucketIndex::new(built_samples_raw.clone(), 0.25))
    };

    let mut agg = HashMap::<String, CellFeatureAgg>::new();
    let mut centroids = HashMap::<String, (f64, f64, f64, f64)>::new(); // lon,lat,x,y
    let mut pop_by_cell = HashMap::<String, (f64, u32)>::new();
    let mut built_by_cell = HashMap::<String, (f64, u32)>::new();

    for sample in &pop_samples_raw {
        let ll = LatLng::new(sample.lat, sample.lon)
            .map_err(|e| format!("invalid raster lat/lon: {e}"))?;
        let cell = ll.to_cell(resolution);
        let cell_id = cell.to_string();
        agg.entry(cell_id.clone()).or_default().node_count += 1;
        let entry = pop_by_cell.entry(cell_id.clone()).or_insert((0.0, 0));
        entry.0 += sample.value.max(0.0);
        entry.1 += 1;
        centroids.entry(cell_id).or_insert_with(|| {
            let center = LatLng::from(cell);
            let center_lat = center.lat();
            let center_lon = center.lng();
            let (x, y) = lonlat_to_target_xy(center_lon, center_lat, &options.target_crs)
                .unwrap_or((0.0, 0.0));
            (center_lon, center_lat, x, y)
        });
    }
    for sample in &built_samples_raw {
        let ll = LatLng::new(sample.lat, sample.lon)
            .map_err(|e| format!("invalid raster lat/lon: {e}"))?;
        let cell = ll.to_cell(resolution);
        let cell_id = cell.to_string();
        agg.entry(cell_id.clone()).or_default().node_count += 1;
        let entry = built_by_cell.entry(cell_id.clone()).or_insert((0.0, 0));
        entry.0 += sample.value.max(0.0);
        entry.1 += 1;
        centroids.entry(cell_id).or_insert_with(|| {
            let center = LatLng::from(cell);
            let center_lat = center.lat();
            let center_lon = center.lng();
            let (x, y) = lonlat_to_target_xy(center_lon, center_lat, &options.target_crs)
                .unwrap_or((0.0, 0.0));
            (center_lon, center_lat, x, y)
        });
    }

    if !options.raster_only {
        let mut reader =
            OsmPbfReader::new(File::open(pbf).map_err(|e| format!("failed to open pbf: {e}"))?);
        for obj in reader.iter() {
            let obj = obj.map_err(|e| format!("pbf obj error: {e}"))?;
            let OsmObj::Node(node) = obj else { continue };
            let lon = node.lon();
            let lat = node.lat();
            if !in_scope(lon, lat) {
                continue;
            }
            let ll = LatLng::new(lat, lon).map_err(|e| format!("invalid lat/lon in pbf: {e}"))?;
            let cell = ll.to_cell(resolution);
            let cell_id = cell.to_string();
            let entry = agg.entry(cell_id.clone()).or_default();
            entry.node_count += 1;
            if classify_stop(&node.tags).is_some() {
                entry.stop_count += 1;
            }
            if node.tags.contains_key("amenity") {
                entry.amenity_count += 1;
            }
            if node.tags.contains_key("shop") {
                entry.shop_count += 1;
                entry.retail_count += 1;
            }
            if node.tags.get("office").is_some() {
                entry.office_count += 1;
            }
            if node.tags.get("landuse").map(|v| v.as_str()) == Some("industrial") {
                entry.industrial_count += 1;
            }
            if node.tags.get("landuse").map(|v| v.as_str()) == Some("residential") {
                entry.residential_count += 1;
            }
            if node.tags.contains_key("leisure") {
                entry.leisure_count += 1;
                entry.recreation_count += 1;
            }
            if node.tags.contains_key("tourism") {
                entry.tourism_count += 1;
                entry.recreation_count += 1;
            }
            if node.tags.get("amenity").map(|v| v.as_str()) == Some("school")
                || node.tags.get("amenity").map(|v| v.as_str()) == Some("university")
                || node.tags.get("amenity").map(|v| v.as_str()) == Some("college")
            {
                entry.education_count += 1;
            }
            if node.tags.get("amenity").map(|v| v.as_str()) == Some("hospital")
                || node.tags.get("amenity").map(|v| v.as_str()) == Some("clinic")
            {
                entry.health_count += 1;
            }
            if node.tags.contains_key("highway") {
                entry.highway_count += 1;
            }

            centroids.entry(cell_id).or_insert_with(|| {
                let center = LatLng::from(cell);
                let center_lat = center.lat();
                let center_lon = center.lng();
                let (x, y) = lonlat_to_target_xy(center_lon, center_lat, &options.target_crs)
                    .unwrap_or((0.0, 0.0));
                (center_lon, center_lat, x, y)
            });
        }
    }

    let mut cells = Vec::<DemandFabricCell>::new();
    let mut ids: Vec<String> = agg.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let a = agg.get(&id).expect("cell aggregate exists");
        let (lon, lat, x, y) = centroids.get(&id).copied().unwrap_or((0.0, 0.0, 0.0, 0.0));
        let pop_hint = pop_by_cell
            .get(&id)
            .and_then(|(sum, n)| {
                if *n > 0 {
                    Some(*sum / (*n as f64))
                } else {
                    None
                }
            })
            .or_else(|| {
                pop_samples
                    .as_ref()
                    .and_then(|idx| idx.nearest_value(lon, lat))
            })
            .unwrap_or(a.residential_count as f64 * 40.0);
        let built_hint = built_by_cell
            .get(&id)
            .and_then(|(sum, n)| {
                if *n > 0 {
                    Some(*sum / (*n as f64))
                } else {
                    None
                }
            })
            .or_else(|| {
                built_samples
                    .as_ref()
                    .and_then(|idx| idx.nearest_value(lon, lat))
            })
            .unwrap_or(a.node_count as f64 * 0.8);

        let score_residential =
            a.residential_count as f64 + 0.45 * a.node_count as f64 + 0.15 * built_hint;
        let score_office =
            a.office_count as f64 + 0.25 * a.amenity_count as f64 + 0.12 * a.highway_count as f64;
        let score_retail =
            a.retail_count as f64 + 0.55 * a.shop_count as f64 + 0.2 * a.amenity_count as f64;
        let score_recreation = a.recreation_count as f64
            + 0.35 * a.leisure_count as f64
            + 0.35 * a.tourism_count as f64;
        let score_industrial = a.industrial_count as f64 + 0.2 * a.highway_count as f64;
        let score_education = a.education_count as f64 + 0.08 * a.amenity_count as f64;
        let score_health = a.health_count as f64 + 0.06 * a.amenity_count as f64;

        let mix_sum = score_residential
            + score_office
            + score_retail
            + score_recreation
            + score_industrial
            + score_education
            + score_health;
        let denom = mix_sum.max(1e-6);

        let activity_mix_residential = score_residential / denom;
        let activity_mix_office = score_office / denom;
        let activity_mix_retail = score_retail / denom;
        let activity_mix_recreation = score_recreation / denom;
        let activity_mix_industrial = score_industrial / denom;
        let activity_mix_education = score_education / denom;
        let activity_mix_health = score_health / denom;

        let residents_night =
            (pop_hint.max(0.0) * (0.55 + 0.75 * activity_mix_residential)).max(0.0);
        let jobs_day = (built_hint.max(0.0)
            * (0.25
                + 1.25 * activity_mix_office
                + 1.05 * activity_mix_industrial
                + 0.8 * activity_mix_retail
                + 0.55 * activity_mix_education
                + 0.55 * activity_mix_health))
            .max(0.0);

        let centrality_score =
            ((a.stop_count as f64 * 1.5) + (a.highway_count as f64 * 0.2)).ln_1p();
        let quality_numer = (a.node_count as f64).ln_1p();
        let data_quality_score = (quality_numer / 8.0).clamp(0.0, 1.0);
        let area_m2 = id
            .parse::<CellIndex>()
            .ok()
            .map(|ci| ci.area_km2() * 1_000_000.0)
            .unwrap_or(0.0);

        cells.push(DemandFabricCell {
            cell_id: id,
            lon,
            lat,
            x,
            y,
            country_iso2: country.clone(),
            area_m2,
            residents_night,
            jobs_day,
            activity_mix_residential,
            activity_mix_office,
            activity_mix_retail,
            activity_mix_recreation,
            activity_mix_industrial,
            activity_mix_education,
            activity_mix_health,
            centrality_score,
            data_quality_score,
        });
    }

    Ok(DemandFabric {
        meta: DemandFabricMeta {
            schema_version: 1,
            generated_at_epoch_s: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            source_pbf: options.pbf_path.clone(),
            h3_res: options.h3_res,
            target_crs: options.target_crs.clone(),
            source_population_raster_csv: options.population_raster_csv.clone(),
            source_built_raster_csv: options.built_raster_csv.clone(),
            country_iso2: country.clone(),
        },
        cells,
    })
}

fn run_build_demand_fabric(options: BuildDemandFabricOptions) -> Result<(), String> {
    let payload = build_demand_fabric_payload(&options)?;
    if let Some(parent) = Path::new(&options.out_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &options.out_path,
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Demand fabric built: cells={} h3_res={} -> {}",
        payload.cells.len(),
        options.h3_res,
        options.out_path
    );
    Ok(())
}

fn smooth_series(
    values: &HashMap<String, f64>,
    parents: &HashMap<String, String>,
) -> HashMap<String, f64> {
    let mut parent_totals = HashMap::<String, (f64, u32)>::new();
    for (child, parent) in parents {
        let v = values.get(child).copied().unwrap_or(0.0);
        let e = parent_totals.entry(parent.clone()).or_insert((0.0, 0));
        e.0 += v;
        e.1 += 1;
    }
    values
        .iter()
        .map(|(child, raw)| {
            let parent = parents.get(child);
            let p_avg = parent
                .and_then(|p| parent_totals.get(p))
                .map(|(sum, n)| sum / (*n as f64).max(1.0))
                .unwrap_or(*raw);
            (child.clone(), (0.68 * raw + 0.32 * p_avg).max(0.0))
        })
        .collect()
}

fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
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

fn aggregate_surface_cells(
    base_cells: &[DemandSurfaceCell],
    target_res: u8,
    target_crs: &str,
    country_iso2: &str,
) -> Vec<DemandSurfaceCell> {
    #[derive(Debug, Clone, Default)]
    struct SurfaceAgg {
        residents_raw: f64,
        jobs_raw: f64,
        residents_smooth: f64,
        jobs_smooth: f64,
        child_count: u32,
        mix_weight_sum: f64,
        mix_residential_sum: f64,
        mix_office_sum: f64,
        mix_retail_sum: f64,
        mix_recreation_sum: f64,
        mix_industrial_sum: f64,
        mix_education_sum: f64,
        mix_health_sum: f64,
    }

    let mut acc = HashMap::<String, SurfaceAgg>::new();
    let Some(resolution) = Resolution::try_from(target_res).ok() else {
        return Vec::new();
    };
    for c in base_cells {
        let Ok(ci) = c.cell_id.parse::<CellIndex>() else {
            continue;
        };
        let Some(parent) = ci.parent(resolution) else {
            continue;
        };
        let parent_id = parent.to_string();
        let e = acc.entry(parent_id).or_default();
        e.residents_raw += c.residents_raw;
        e.jobs_raw += c.jobs_raw;
        e.residents_smooth += c.residents_smooth;
        e.jobs_smooth += c.jobs_smooth;
        e.child_count += 1;
        let mix_weight = (c.residents_smooth + c.jobs_smooth).max(1e-6);
        e.mix_weight_sum += mix_weight;
        e.mix_residential_sum += c.activity_mix_residential.max(0.0) * mix_weight;
        e.mix_office_sum += c.activity_mix_office.max(0.0) * mix_weight;
        e.mix_retail_sum += c.activity_mix_retail.max(0.0) * mix_weight;
        e.mix_recreation_sum += c.activity_mix_recreation.max(0.0) * mix_weight;
        e.mix_industrial_sum += c.activity_mix_industrial.max(0.0) * mix_weight;
        e.mix_education_sum += c.activity_mix_education.max(0.0) * mix_weight;
        e.mix_health_sum += c.activity_mix_health.max(0.0) * mix_weight;
    }
    let mut out = Vec::<DemandSurfaceCell>::new();
    for (cell_id, agg) in acc {
        let Ok(ci) = cell_id.parse::<CellIndex>() else {
            continue;
        };
        let center = LatLng::from(ci);
        let lon = center.lng();
        let lat = center.lat();
        let (x, y) = lonlat_to_target_xy(lon, lat, target_crs).unwrap_or((0.0, 0.0));
        let mix_denom = agg.mix_weight_sum.max(1e-9);
        let [activity_mix_residential, activity_mix_office, activity_mix_retail, activity_mix_recreation, activity_mix_industrial, activity_mix_education, activity_mix_health] =
            normalize_activity_mix([
                agg.mix_residential_sum / mix_denom,
                agg.mix_office_sum / mix_denom,
                agg.mix_retail_sum / mix_denom,
                agg.mix_recreation_sum / mix_denom,
                agg.mix_industrial_sum / mix_denom,
                agg.mix_education_sum / mix_denom,
                agg.mix_health_sum / mix_denom,
            ]);
        out.push(DemandSurfaceCell {
            cell_id,
            h3_res: target_res,
            lon,
            lat,
            x,
            y,
            area_m2: ci.area_km2() * 1_000_000.0,
            country_iso2: country_iso2.to_string(),
            residents_raw: agg.residents_raw.max(0.0),
            jobs_raw: agg.jobs_raw.max(0.0),
            residents_smooth: agg.residents_smooth.max(0.0),
            jobs_smooth: agg.jobs_smooth.max(0.0),
            activity_mix_residential,
            activity_mix_office,
            activity_mix_retail,
            activity_mix_recreation,
            activity_mix_industrial,
            activity_mix_education,
            activity_mix_health,
            quality: ((agg.child_count as f64).ln_1p() / 4.0).clamp(0.0, 1.0),
        });
    }
    out.sort_by(|a, b| a.cell_id.cmp(&b.cell_id));
    out
}

fn demand_surface_from_fabric(
    country_iso2: &str,
    boundaries_path: &str,
    population_raster_csv: &str,
    built_raster_csv: &str,
    fabric: &DemandFabric,
) -> DemandSurfaceCountry {
    let mut parent7 = HashMap::<String, String>::new();
    let mut parent6 = HashMap::<String, String>::new();
    let mut residents_raw = HashMap::<String, f64>::new();
    let mut jobs_raw = HashMap::<String, f64>::new();
    for c in &fabric.cells {
        residents_raw.insert(c.cell_id.clone(), c.residents_night.max(0.0));
        jobs_raw.insert(c.cell_id.clone(), c.jobs_day.max(0.0));
        if let Ok(ci) = c.cell_id.parse::<CellIndex>() {
            if let Some(p7) = ci.parent(Resolution::Seven) {
                parent7.insert(c.cell_id.clone(), p7.to_string());
            }
            if let Some(p6) = ci.parent(Resolution::Six) {
                parent6.insert(c.cell_id.clone(), p6.to_string());
            }
        }
    }
    let sm7_res = smooth_series(&residents_raw, &parent7);
    let sm7_jobs = smooth_series(&jobs_raw, &parent7);
    let sm6_res = smooth_series(&residents_raw, &parent6);
    let sm6_jobs = smooth_series(&jobs_raw, &parent6);

    let mut cells_res8 = Vec::<DemandSurfaceCell>::new();
    for c in &fabric.cells {
        let res_sm = 0.65
            * sm7_res
                .get(&c.cell_id)
                .copied()
                .unwrap_or(c.residents_night.max(0.0))
            + 0.35
                * sm6_res
                    .get(&c.cell_id)
                    .copied()
                    .unwrap_or(c.residents_night.max(0.0));
        let jobs_sm = 0.65
            * sm7_jobs
                .get(&c.cell_id)
                .copied()
                .unwrap_or(c.jobs_day.max(0.0))
            + 0.35
                * sm6_jobs
                    .get(&c.cell_id)
                    .copied()
                    .unwrap_or(c.jobs_day.max(0.0));
        let [activity_mix_residential, activity_mix_office, activity_mix_retail, activity_mix_recreation, activity_mix_industrial, activity_mix_education, activity_mix_health] =
            normalize_activity_mix([
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ]);
        cells_res8.push(DemandSurfaceCell {
            cell_id: c.cell_id.clone(),
            h3_res: 8,
            lon: c.lon,
            lat: c.lat,
            x: c.x,
            y: c.y,
            area_m2: c.area_m2.max(0.0),
            country_iso2: country_iso2.to_string(),
            residents_raw: c.residents_night.max(0.0),
            jobs_raw: c.jobs_day.max(0.0),
            residents_smooth: res_sm.max(0.0),
            jobs_smooth: jobs_sm.max(0.0),
            activity_mix_residential,
            activity_mix_office,
            activity_mix_retail,
            activity_mix_recreation,
            activity_mix_industrial,
            activity_mix_education,
            activity_mix_health,
            quality: c.data_quality_score.clamp(0.0, 1.0),
        });
    }
    cells_res8.sort_by(|a, b| a.cell_id.cmp(&b.cell_id));
    let cells_res7 = aggregate_surface_cells(&cells_res8, 7, &fabric.meta.target_crs, country_iso2);
    let cells_res6 = aggregate_surface_cells(&cells_res8, 6, &fabric.meta.target_crs, country_iso2);

    DemandSurfaceCountry {
        country_iso2: country_iso2.to_string(),
        surface_version: "v4".to_string(),
        source_provenance: DemandSurfaceProvenance {
            generated_at_epoch_s: fabric.meta.generated_at_epoch_s,
            source_pbf: fabric.meta.source_pbf.clone(),
            country_boundaries_geojson: boundaries_path.to_string(),
            source_population_raster_csv: population_raster_csv.to_string(),
            source_built_raster_csv: built_raster_csv.to_string(),
            h3_base_res: fabric.meta.h3_res,
            target_crs: fabric.meta.target_crs.clone(),
        },
        cells_res6,
        cells_res7,
        cells_res8,
    }
}

fn run_build_demand_surface(options: BuildDemandSurfaceOptions) -> Result<(), String> {
    let Some(iso) = canonical_country_iso2(&options.country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    let fabric = build_demand_fabric_payload(&BuildDemandFabricOptions {
        pbf_path: options.pbf_path.clone(),
        out_path: String::new(),
        bbox: options.bbox.clone(),
        country_iso2: Some(iso.clone()),
        h3_res: options.h3_res,
        target_crs: options.target_crs.clone(),
        population_raster_csv: Some(options.population_raster_csv.clone()),
        built_raster_csv: Some(options.built_raster_csv.clone()),
        country_boundaries_geojson: Some(options.country_boundaries_geojson.clone()),
        raster_only: options.raster_only,
    })?;
    let surface = demand_surface_from_fabric(
        &iso,
        &options.country_boundaries_geojson,
        &options.population_raster_csv,
        &options.built_raster_csv,
        &fabric,
    );
    if let Some(parent) = Path::new(&options.out_path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &options.out_path,
        serde_json::to_string_pretty(&surface).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Demand surface built: country={} res8={} res7={} res6={} -> {}",
        surface.country_iso2,
        surface.cells_res8.len(),
        surface.cells_res7.len(),
        surface.cells_res6.len(),
        options.out_path
    );
    Ok(())
}

fn run_build_demand_surface_pack(options: BuildDemandSurfacePackOptions) -> Result<(), String> {
    let boundaries = parse_country_boundaries(&options.country_boundaries_geojson)?;
    let mut countries = if options.countries.trim().eq_ignore_ascii_case("all") {
        boundaries.keys().cloned().collect::<Vec<_>>()
    } else {
        options
            .countries
            .split(',')
            .map(|c| c.trim().to_string())
            .collect::<Vec<_>>()
    };
    countries = countries
        .into_iter()
        .filter_map(|code| canonical_country_iso2(&code))
        .collect();
    countries.sort();
    countries.dedup();
    if countries.is_empty() {
        return Err("no countries provided for demand surface pack".to_string());
    }
    fs::create_dir_all(&options.out_dir).map_err(|e| e.to_string())?;
    let mut built = 0usize;
    for iso in countries {
        let boundary_iso = boundaries_query_iso2(&iso);
        if !boundaries.contains_key(&boundary_iso) {
            eprintln!("Skipping {} (not found in boundaries file)", iso);
            continue;
        }
        let out_path = Path::new(&options.out_dir).join(format!("{iso}.surface.json"));
        run_build_demand_surface(BuildDemandSurfaceOptions {
            pbf_path: options.pbf_path.clone(),
            country_iso2: iso,
            country_boundaries_geojson: options.country_boundaries_geojson.clone(),
            population_raster_csv: options.population_raster_csv.clone(),
            built_raster_csv: options.built_raster_csv.clone(),
            out_path: out_path.to_string_lossy().to_string(),
            h3_res: options.h3_res,
            target_crs: options.target_crs.clone(),
            bbox: options.bbox.clone(),
            raster_only: options.raster_only,
        })?;
        built += 1;
    }
    println!(
        "Demand surface pack built: {} countries -> {}",
        built, options.out_dir
    );
    Ok(())
}

fn run_build_country_pack(options: BuildCountryPackOptions) -> Result<(), String> {
    let Some(iso) = canonical_country_iso2(&options.country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };

    let fabric = build_demand_fabric_payload(&BuildDemandFabricOptions {
        pbf_path: options.pbf_path.clone(),
        out_path: String::new(),
        bbox: options.bbox.clone(),
        country_iso2: Some(iso.clone()),
        h3_res: options.h3_res,
        target_crs: options.target_crs.clone(),
        population_raster_csv: Some(options.population_raster_csv.clone()),
        built_raster_csv: Some(options.built_raster_csv.clone()),
        country_boundaries_geojson: Some(options.country_boundaries_geojson.clone()),
        raster_only: options.raster_only,
    })?;
    let surface = demand_surface_from_fabric(
        &iso,
        &options.country_boundaries_geojson,
        &options.population_raster_csv,
        &options.built_raster_csv,
        &fabric,
    );

    let pack_root = Path::new(&options.out_dir);
    let surfaces_dir = pack_root.join("surfaces");
    let region_macro_dir = pack_root.join("region_macro");
    let region_cells_dir = pack_root.join("region_cells");
    fs::create_dir_all(&surfaces_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&region_macro_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&region_cells_dir).map_err(|e| e.to_string())?;

    let surface_file = format!("{iso}.surface.json");
    let surface_path = surfaces_dir.join(&surface_file);
    fs::write(
        &surface_path,
        serde_json::to_string_pretty(&surface).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let mut region_adj = HashMap::<String, Vec<String>>::new();
    for (i, a) in surface.cells_res6.iter().enumerate() {
        let mut nearest = surface
            .cells_res6
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, b)| {
                let d2 = (a.x - b.x).powi(2) + (a.y - b.y).powi(2);
                (d2, b.cell_id.clone())
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        region_adj.insert(
            a.cell_id.clone(),
            nearest.into_iter().take(6).map(|(_, id)| id).collect(),
        );
    }

    let mut res8_by_region = HashMap::<String, Vec<&DemandSurfaceCell>>::new();
    for c in &surface.cells_res8 {
        let mut best: Option<(f64, String)> = None;
        for r in &surface.cells_res6 {
            let d2 = (c.x - r.x).powi(2) + (c.y - r.y).powi(2);
            match best {
                None => best = Some((d2, r.cell_id.clone())),
                Some((best_d2, _)) if d2 < best_d2 => best = Some((d2, r.cell_id.clone())),
                _ => {}
            }
        }
        if let Some((_, rid)) = best {
            res8_by_region.entry(rid).or_default().push(c);
        }
    }

    let region_features = surface
        .cells_res6
        .iter()
        .map(|r| {
            serde_json::json!({
                "type": "Feature",
                "properties": {
                    "region_id": format!("r6:{iso}:{}", r.cell_id),
                    "country_iso2": iso,
                    "admin_level": "h3_r6_proxy",
                    "adjacent_region_ids": region_adj.get(&r.cell_id).cloned().unwrap_or_default(),
                    "residents_smooth": r.residents_smooth,
                    "jobs_smooth": r.jobs_smooth
                },
                "geometry": {
                    "type": "Point",
                    "coordinates": [r.lon, r.lat]
                }
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        pack_root.join("regions.geojson"),
        serde_json::to_string_pretty(&serde_json::json!({
            "type": "FeatureCollection",
            "features": region_features
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    for r in &surface.cells_res6 {
        let rid = format!("r6:{iso}:{}", r.cell_id);
        let rid_file = rid.replace(':', "__");
        let adjacent = region_adj.get(&r.cell_id).cloned().unwrap_or_default();
        fs::write(
            region_macro_dir.join(format!("{rid_file}.json")),
            serde_json::to_string_pretty(&serde_json::json!({
                "region_id": rid,
                "country_iso2": iso,
                "residents_smooth": r.residents_smooth,
                "jobs_smooth": r.jobs_smooth,
                "activity_mix_residential": r.activity_mix_residential,
                "activity_mix_office": r.activity_mix_office,
                "activity_mix_retail": r.activity_mix_retail,
                "activity_mix_recreation": r.activity_mix_recreation,
                "activity_mix_industrial": r.activity_mix_industrial,
                "activity_mix_education": r.activity_mix_education,
                "activity_mix_health": r.activity_mix_health,
                "adjacent_region_ids": adjacent
            }))
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;

        let cells = res8_by_region
            .get(&r.cell_id)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| {
                serde_json::json!({
                    "cell_id": c.cell_id,
                    "x": c.x,
                    "y": c.y,
                    "residents_smooth": c.residents_smooth,
                    "jobs_smooth": c.jobs_smooth,
                    "activity_mix_residential": c.activity_mix_residential,
                    "activity_mix_office": c.activity_mix_office,
                    "activity_mix_retail": c.activity_mix_retail,
                    "activity_mix_recreation": c.activity_mix_recreation,
                    "activity_mix_industrial": c.activity_mix_industrial,
                    "activity_mix_education": c.activity_mix_education,
                    "activity_mix_health": c.activity_mix_health
                })
            })
            .collect::<Vec<_>>();
        fs::write(
            region_cells_dir.join(format!("{rid_file}.json")),
            serde_json::to_string_pretty(&serde_json::json!(cells)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }

    let mut world_context_file = None::<String>;
    let mut major_roads_file = None::<String>;
    let mut county_roads_dir_rel = None::<String>;
    let mut county_basemap_mid_dir_rel = None::<String>;
    let mut county_basemap_full_dir_rel = None::<String>;
    if is_uk_country_iso2(&iso) {
        build_country_map_artifacts(
            &iso,
            &options.pbf_path,
            &options.country_boundaries_geojson,
            pack_root,
        )?;
        world_context_file = Some("map/world_context.geojson".to_string());
        major_roads_file = Some("map/major_roads.geojson".to_string());
        county_roads_dir_rel = Some("map/county_roads".to_string());
        county_basemap_mid_dir_rel = Some("map/county_basemap_mid".to_string());
        county_basemap_full_dir_rel = Some("map/county_basemap_full".to_string());
    }

    let manifest = CountryPackManifest {
        schema_version: 2,
        country_iso2: iso.clone(),
        pack_version: "v4".to_string(),
        generated_at_epoch_s: fabric.meta.generated_at_epoch_s,
        surface_file: format!("surfaces/{surface_file}"),
        regions_file: "regions.geojson".to_string(),
        region_provider_model: DEFAULT_REGION_PROVIDER_MODEL.to_string(),
        compatibility_country_aliases: if is_uk_country_iso2(&iso) {
            vec![UK_COMPAT_GB_ISO2.to_string()]
        } else {
            Vec::new()
        },
        region_count: surface.cells_res6.len(),
        cells_res8: surface.cells_res8.len(),
        source_provenance: surface.source_provenance.clone(),
        map_context_version: world_context_file.as_ref().map(|_| "v1".to_string()),
        world_context_file,
        major_roads_file,
        county_roads_dir: county_roads_dir_rel,
        county_basemap_mid_dir: county_basemap_mid_dir_rel,
        county_basemap_full_dir: county_basemap_full_dir_rel,
        map_pack_version: Some("geojson-basemap-v2".to_string()),
    };
    fs::write(
        pack_root.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    println!(
        "Country pack built: iso={} regions={} cells_res8={} -> {}",
        iso,
        manifest.region_count,
        manifest.cells_res8,
        pack_root.display()
    );
    Ok(())
}

fn run_validate_country_pack(pack_dir: &str) -> Result<(), String> {
    let root = Path::new(pack_dir);
    if !root.exists() {
        return Err(format!("pack_dir does not exist: {}", root.display()));
    }
    let manifest_path = root.join("manifest.json");
    if !manifest_path.exists() {
        return Err("manifest.json missing".to_string());
    }
    let manifest_raw = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: CountryPackManifest =
        serde_json::from_str(&manifest_raw).map_err(|e| e.to_string())?;
    let Some(canonical_manifest_iso2) = canonical_country_iso2(&manifest.country_iso2) else {
        return Err("manifest.country_iso2 must be two-letter ISO code".to_string());
    };
    if manifest.region_provider_model.trim().is_empty()
        || !manifest
            .region_provider_model
            .starts_with("planning_surface_")
    {
        return Err("manifest.region_provider_model must start with planning_surface_".to_string());
    }
    let surface_path = root.join(&manifest.surface_file);
    if !surface_path.exists() {
        return Err(format!("surface file missing: {}", surface_path.display()));
    }
    let surface_raw = fs::read_to_string(&surface_path).map_err(|e| e.to_string())?;
    let surface: DemandSurfaceCountry =
        serde_json::from_str(&surface_raw).map_err(|e| e.to_string())?;
    let Some(canonical_surface_iso2) = canonical_country_iso2(&surface.country_iso2) else {
        return Err("surface country_iso2 must be two-letter ISO code".to_string());
    };
    if canonical_surface_iso2 != canonical_manifest_iso2 {
        return Err(format!(
            "surface country mismatch: manifest={} surface={}",
            canonical_manifest_iso2, canonical_surface_iso2
        ));
    }
    if !root.join(&manifest.regions_file).exists() {
        return Err(format!(
            "regions file missing: {}",
            root.join(&manifest.regions_file).display()
        ));
    }
    if !root.join("region_macro").exists() || !root.join("region_cells").exists() {
        return Err("region_macro and/or region_cells directory missing".to_string());
    }
    if let Some(world_context_file) = &manifest.world_context_file {
        if !root.join(world_context_file).exists() {
            return Err(format!(
                "world context file missing: {}",
                root.join(world_context_file).display()
            ));
        }
    }
    if let Some(major_roads_file) = &manifest.major_roads_file {
        if !root.join(major_roads_file).exists() {
            return Err(format!(
                "major roads file missing: {}",
                root.join(major_roads_file).display()
            ));
        }
    }
    if let Some(county_roads_dir) = &manifest.county_roads_dir {
        if !root.join(county_roads_dir).exists() {
            return Err(format!(
                "county roads dir missing: {}",
                root.join(county_roads_dir).display()
            ));
        }
    }
    if let Some(county_basemap_mid_dir) = &manifest.county_basemap_mid_dir {
        if !root.join(county_basemap_mid_dir).exists() {
            return Err(format!(
                "county basemap mid dir missing: {}",
                root.join(county_basemap_mid_dir).display()
            ));
        }
    }
    if let Some(county_basemap_full_dir) = &manifest.county_basemap_full_dir {
        if !root.join(county_basemap_full_dir).exists() {
            return Err(format!(
                "county basemap full dir missing: {}",
                root.join(county_basemap_full_dir).display()
            ));
        }
    }
    println!(
        "Country pack valid: iso={} regions={} cells_res8={} at {}",
        manifest.country_iso2,
        manifest.region_count,
        manifest.cells_res8,
        root.display()
    );
    Ok(())
}

fn run_apply_demand_fabric(
    scenario_path: &str,
    fabric_path: &str,
    out_path: Option<&str>,
    replace_zones: bool,
) -> Result<(), String> {
    let mut doc = ScenarioService::load_from_path(scenario_path).map_err(|e| e.to_string())?;
    let raw = fs::read_to_string(fabric_path).map_err(|e| e.to_string())?;
    let fabric: DemandFabric = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if fabric.cells.is_empty() {
        return Err("demand fabric has no cells".to_string());
    }

    let demand_cells: Vec<DemandCell> = fabric
        .cells
        .iter()
        .map(|c| DemandCell {
            cell_id: c.cell_id.clone(),
            x: c.x,
            y: c.y,
            area_m2: c.area_m2,
            residents_night: c.residents_night.max(0.0),
            jobs_day: c.jobs_day.max(0.0),
            activity_mix_residential: c.activity_mix_residential.max(0.0),
            activity_mix_office: c.activity_mix_office.max(0.0),
            activity_mix_retail: c.activity_mix_retail.max(0.0),
            activity_mix_recreation: c.activity_mix_recreation.max(0.0),
            activity_mix_industrial: c.activity_mix_industrial.max(0.0),
            activity_mix_education: c.activity_mix_education.max(0.0),
            activity_mix_health: c.activity_mix_health.max(0.0),
            centrality_score: c.centrality_score.max(0.0),
            data_quality_score: c.data_quality_score.clamp(0.0, 1.0),
            country_iso2: c.country_iso2.clone(),
            allocation_diagnostics: None,
        })
        .collect();
    let zones: Vec<Zone> = demand_cells
        .iter()
        .map(|c| Zone {
            id: c.cell_id.clone(),
            x: c.x,
            y: c.y,
            population: c.residents_night.max(0.0),
            jobs: c.jobs_day.max(0.0),
            country_iso2: c.country_iso2.clone(),
        })
        .collect();
    doc.scenario.world.demand_cells = demand_cells;
    if replace_zones {
        doc.scenario.world.zones = zones;
    }

    ScenarioService::validate(&doc.scenario).map_err(|e| e.to_string())?;
    let out = out_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sibling_output(scenario_path, "scenario.demand.json"));
    ScenarioService::save_to_path(&out, &doc).map_err(|e| e.to_string())?;
    println!(
        "Demand fabric applied: cells={}, zones_replaced={} -> {}",
        doc.scenario.world.demand_cells.len(),
        replace_zones,
        out
    );
    Ok(())
}

fn read_raster_csv(path: &str) -> Result<Vec<RasterSample>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let lon_idx = col_idx(&headers, "lon")
        .or_else(|| col_idx(&headers, "longitude"))
        .ok_or_else(|| "raster csv missing lon/longitude column".to_string())?;
    let lat_idx = col_idx(&headers, "lat")
        .or_else(|| col_idx(&headers, "latitude"))
        .ok_or_else(|| "raster csv missing lat/latitude column".to_string())?;
    let val_idx = col_idx(&headers, "value")
        .or_else(|| col_idx(&headers, "v"))
        .ok_or_else(|| "raster csv missing value column".to_string())?;
    let mut out = Vec::<RasterSample>::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let lon = rec
            .get(lon_idx)
            .unwrap_or("")
            .trim()
            .parse::<f64>()
            .unwrap_or(f64::NAN);
        let lat = rec
            .get(lat_idx)
            .unwrap_or("")
            .trim()
            .parse::<f64>()
            .unwrap_or(f64::NAN);
        let value = rec
            .get(val_idx)
            .unwrap_or("")
            .trim()
            .parse::<f64>()
            .unwrap_or(f64::NAN);
        if lon.is_finite() && lat.is_finite() && value.is_finite() {
            out.push(RasterSample { lon, lat, value });
        }
    }
    Ok(out)
}

struct ImportPbfOptions {
    pbf_path: String,
    scenario_name: String,
    out_root: Option<String>,
    bbox: Option<Vec<f64>>,
    max_stops: Option<usize>,
    snap_m: f64,
    inferred_headway_s: f64,
    cleanup_topology: bool,
    infer_services: bool,
}

struct BuildDemandFabricOptions {
    pbf_path: String,
    out_path: String,
    bbox: Option<Vec<f64>>,
    country_iso2: Option<String>,
    h3_res: u8,
    target_crs: String,
    population_raster_csv: Option<String>,
    built_raster_csv: Option<String>,
    country_boundaries_geojson: Option<String>,
    raster_only: bool,
}

struct BuildDemandSurfaceOptions {
    pbf_path: String,
    country_iso2: String,
    country_boundaries_geojson: String,
    population_raster_csv: String,
    built_raster_csv: String,
    out_path: String,
    h3_res: u8,
    target_crs: String,
    bbox: Option<Vec<f64>>,
    raster_only: bool,
}

struct BuildDemandSurfacePackOptions {
    pbf_path: String,
    countries: String,
    country_boundaries_geojson: String,
    population_raster_csv: String,
    built_raster_csv: String,
    out_dir: String,
    h3_res: u8,
    target_crs: String,
    bbox: Option<Vec<f64>>,
    raster_only: bool,
}

struct BuildCountryPackOptions {
    pbf_path: String,
    country_iso2: String,
    country_boundaries_geojson: String,
    population_raster_csv: String,
    built_raster_csv: String,
    out_dir: String,
    h3_res: u8,
    target_crs: String,
    bbox: Option<Vec<f64>>,
    raster_only: bool,
}

fn run_import_pbf(options: ImportPbfOptions) -> Result<(), String> {
    let pbf = Path::new(&options.pbf_path);
    if !pbf.exists() {
        return Err(format!("PBF not found: {}", pbf.display()));
    }
    let bbox = parse_bbox_or_world(options.bbox.as_deref())?;

    let mut ways = Vec::<CandidateWay>::new();
    let mut needed_nodes = HashSet::<i64>::new();
    scan_candidate_ways(pbf, &mut ways, &mut needed_nodes)?;

    let mut pt_stops = Vec::<Stop>::new();
    let mut node_xy = HashMap::<i64, (f64, f64)>::new();
    scan_nodes(
        pbf,
        &needed_nodes,
        bbox,
        options.max_stops,
        &mut pt_stops,
        &mut node_xy,
    )?;

    let raw_pt = pt_stops.len();
    pt_stops = snap_nearby_stops(pt_stops, options.snap_m);
    let snapped_pt = pt_stops.len();

    let mut shape_stops = build_shape_stops(&ways, &node_xy);
    let mut links = build_corridor_links(&ways, &node_xy, bbox);

    if options.cleanup_topology {
        links = dedupe_links(links);
        let keep = keep_largest_component(&links);
        links.retain(|l| keep.contains(&l.from_stop) && keep.contains(&l.to_stop));
        shape_stops.retain(|s| keep.contains(&s.id));
    }

    let shape_count = shape_stops.len();
    let mut stops = pt_stops;
    stops.extend(shape_stops);

    let zones = build_grid_zones(bbox, &stops, 30);
    let services = if options.infer_services {
        infer_services_from_links(&links, options.inferred_headway_s)
    } else {
        build_link_shuttle_services(&links)
    };

    let scenario = Scenario {
        meta: Meta {
            name: options.scenario_name.clone(),
            seed: 42,
            time_period_hours: 1.0,
            crs: Crs::Epsg3857,
        },
        params: default_params_fallback(),
        world: World {
            stops,
            links,
            zones,
            services,
            transfers: vec![],
            transfer_rules: None,
            demand_cells: vec![],
            demand_meta: None,
        },
    };

    ScenarioService::validate(&scenario)
        .map_err(|e| format!("scenario validation failed:\n{e}"))?;
    let out_root = options.out_root.as_deref().unwrap_or("data/osm_import");
    let out_dir = Path::new(out_root).join(&options.scenario_name);
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;
    let out_path = out_dir.join("scenario.json");
    let doc = ScenarioDocument::new_current(scenario);
    ScenarioService::save_to_path(out_path.to_string_lossy().as_ref(), &doc)
        .map_err(|e| e.to_string())?;

    println!(
        "OSM v2 import complete: PT stops raw={} snapped={}, shape_nodes={}, links={}, services={}",
        raw_pt,
        snapped_pt,
        shape_count,
        doc.scenario.world.links.len(),
        doc.scenario.world.services.len()
    );
    println!("Wrote {}", out_path.display());
    Ok(())
}

fn run_normalize_lsoa(
    population_csv_path: &str,
    jobs_csv_path: &str,
    centroids_csv_path: &str,
    out_path: &str,
    region: &str,
    target_crs: &str,
) -> Result<(), String> {
    let population = read_lsoa_population(population_csv_path)?;
    let jobs = read_lsoa_jobs(jobs_csv_path)?;
    let centroids = read_lsoa_centroids(centroids_csv_path)?;
    let result = normalize_lsoa_rows(&population, &jobs, &centroids, region, target_crs)?;
    write_normalized_zones_csv(out_path, &result.rows)?;
    println!(
        "LSOA normalization complete: population_rows={}, region_rows={}, written_rows={}, missing_jobs={}, missing_centroids={}",
        result.population_rows,
        result.region_rows,
        result.rows.len(),
        result.missing_jobs,
        result.missing_centroids
    );
    println!("Wrote {}", out_path);
    Ok(())
}

fn run_attach_census(
    scenario_path: &str,
    csv_path: &str,
    out_path: Option<&str>,
    profile_csv_path: Option<&str>,
    replace_zones: bool,
) -> Result<(), String> {
    let mut doc = ScenarioService::load_from_path(scenario_path).map_err(|e| e.to_string())?;
    let mut updated = 0usize;
    if replace_zones {
        let zones = read_normalized_zones_csv(csv_path)?;
        updated = zones.len();
        doc.scenario.world.zones = zones;
    } else {
        let mut zone_index = HashMap::<String, usize>::new();
        for (i, z) in doc.scenario.world.zones.iter().enumerate() {
            zone_index.insert(z.id.clone(), i);
        }
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_path(csv_path)
            .map_err(|e| e.to_string())?;
        let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
        for rec in rdr.records() {
            let rec = rec.map_err(|e| e.to_string())?;
            if let Some(zi) =
                resolve_zone_row(&headers, &rec, &doc.scenario.world.zones, &zone_index)?
            {
                doc.scenario.world.zones[zi].population =
                    parse_required_f64(&headers, &rec, "population")?.max(0.0);
                doc.scenario.world.zones[zi].jobs =
                    parse_required_f64(&headers, &rec, "jobs")?.max(0.0);
                updated += 1;
            }
        }
    }

    if let Some(profile_path) = profile_csv_path {
        doc.scenario.params.demand_profile = read_demand_profile(profile_path)?;
    }
    ScenarioService::validate(&doc.scenario).map_err(|e| e.to_string())?;

    let out = out_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sibling_output(scenario_path, "scenario.census.json"));
    ScenarioService::save_to_path(&out, &doc).map_err(|e| e.to_string())?;
    if replace_zones {
        println!(
            "Census attached (replace mode): replaced_zones={}, demand_slices={}",
            updated,
            doc.scenario.params.demand_profile.len()
        );
    } else {
        println!(
            "Census attached: updated_zones={}, demand_slices={}",
            updated,
            doc.scenario.params.demand_profile.len()
        );
    }
    println!("Wrote {}", out);
    Ok(())
}

fn run_import_gtfs(
    scenario_path: &str,
    gtfs_dir: &str,
    out_path: Option<&str>,
    snap_m: f64,
    default_headway_s: f64,
) -> Result<(), String> {
    let mut doc = ScenarioService::load_from_path(scenario_path).map_err(|e| e.to_string())?;
    let stops_file = Path::new(gtfs_dir).join("stops.txt");
    let routes_file = Path::new(gtfs_dir).join("routes.txt");
    let trips_file = Path::new(gtfs_dir).join("trips.txt");
    let stop_times_file = Path::new(gtfs_dir).join("stop_times.txt");
    if !stops_file.exists()
        || !routes_file.exists()
        || !trips_file.exists()
        || !stop_times_file.exists()
    {
        return Err(
            "gtfs_dir must contain stops.txt, routes.txt, trips.txt, stop_times.txt".to_string(),
        );
    }

    let gtfs_stops = read_gtfs_stops(&stops_file)?;
    let route_modes = read_gtfs_route_modes(&routes_file)?;
    let trip_routes = read_gtfs_trip_routes(&trips_file)?;
    let trip_stop_sequences = read_gtfs_trip_stop_sequences(&stop_times_file)?;
    let mut refs = build_stop_refs(&doc.scenario);

    let mut added_stops = 0usize;
    let mut added_links = 0usize;
    let mut added_services = 0usize;
    let mut deduped_services = 0usize;
    for (trip_id, stop_ids) in trip_stop_sequences {
        let Some(route_id) = trip_routes.get(&trip_id) else {
            continue;
        };
        let mode = route_modes
            .get(route_id)
            .cloned()
            .unwrap_or_else(|| "bus".to_string());
        let mut seq = Vec::<String>::new();
        for sid in stop_ids {
            let Some(gs) = gtfs_stops.get(&sid) else {
                continue;
            };
            let (mx, my) = lonlat_to_web_mercator_m(gs.lon, gs.lat);
            let mapped = map_or_create_stop(&mut doc.scenario, &mut refs, &sid, mx, my, snap_m);
            if mapped.starts_with("gtfs:stop:") {
                added_stops += 1;
            }
            if seq.last() != Some(&mapped) {
                seq.push(mapped);
            }
        }
        if seq.len() < 2 {
            continue;
        }
        ensure_links_for_sequence(&mut doc.scenario, &seq, &mode, &mut added_links);
        let duplicate = doc
            .scenario
            .world
            .services
            .iter()
            .any(|s| s.mode == mode && s.stop_sequence == seq);
        if duplicate {
            deduped_services += 1;
            continue;
        }
        doc.scenario.world.services.push(Service {
            id: format!("gtfs:trip:{trip_id}"),
            mode: mode.clone(),
            line_id: None,
            name: None,
            direction: None,
            direction_name: None,
            display_color: None,
            service_enabled: None,
            operating_tph: None,
            stock_tier_id: None,
            stock_units_owned: None,
            stock_units_assigned: None,
            rolling_stock_profile: None,
            schedule_profile: None,
            mode_variant: None,
            stop_sequence: seq,
            headway_s: default_headway_s.max(60.0),
            dwell_s: 20.0,
            vehicle_capacity: default_vehicle_capacity_for_mode(&mode),
            board_penalty_s: Some(default_board_penalty_for_mode(&mode)),
        });
        added_services += 1;
    }

    ScenarioService::validate(&doc.scenario).map_err(|e| e.to_string())?;
    let out = out_path
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sibling_output(scenario_path, "scenario.gtfs.json"));
    ScenarioService::save_to_path(&out, &doc).map_err(|e| e.to_string())?;
    println!(
        "GTFS merge complete: added_stops={}, added_links={}, added_services={}, deduped_services={}",
        added_stops, added_links, added_services, deduped_services
    );
    println!("Wrote {}", out);
    Ok(())
}

fn parse_bbox_or_world(bbox: Option<&[f64]>) -> Result<(f64, f64, f64, f64), String> {
    match bbox {
        Some(b) if b.len() == 4 => Ok((b[0], b[1], b[2], b[3])),
        Some(_) => Err("bbox must be 4 numbers: min_lon min_lat max_lon max_lat".to_string()),
        None => Ok((-180.0, -90.0, 180.0, 90.0)),
    }
}

fn scan_candidate_ways(
    pbf_path: &Path,
    ways: &mut Vec<CandidateWay>,
    needed_node_ids: &mut HashSet<i64>,
) -> Result<(), String> {
    let mut reader =
        OsmPbfReader::new(File::open(pbf_path).map_err(|e| format!("failed to open pbf: {e}"))?);
    for obj in reader.iter() {
        let obj = obj.map_err(|e| format!("pbf obj error: {e}"))?;
        let OsmObj::Way(way) = obj else { continue };
        let Some(mode) = classify_way(&way.tags) else {
            continue;
        };

        let node_ids: Vec<i64> = way.nodes.iter().map(|nid| nid.0).collect();
        if node_ids.len() < 2 {
            continue;
        }
        for nid in &node_ids {
            needed_node_ids.insert(*nid);
        }
        ways.push(CandidateWay {
            way_id: way.id.0,
            mode,
            node_ids,
        });
    }
    Ok(())
}

fn scan_nodes(
    pbf_path: &Path,
    needed_node_ids: &HashSet<i64>,
    bbox: (f64, f64, f64, f64),
    max_stops: Option<usize>,
    pt_stops: &mut Vec<Stop>,
    node_xy: &mut HashMap<i64, (f64, f64)>,
) -> Result<(), String> {
    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    let mut reader =
        OsmPbfReader::new(File::open(pbf_path).map_err(|e| format!("failed to open pbf: {e}"))?);
    for obj in reader.iter() {
        let obj = obj.map_err(|e| format!("pbf obj error: {e}"))?;
        let OsmObj::Node(node) = obj else { continue };

        let nid = node.id.0;
        let lon = node.lon();
        let lat = node.lat();
        if needed_node_ids.contains(&nid) {
            let (x, y) = lonlat_to_web_mercator_m(lon, lat);
            node_xy.insert(nid, (x, y));
        }
        if lon < min_lon || lon > max_lon || lat < min_lat || lat > max_lat {
            continue;
        }
        if let Some(stop_type) = classify_stop(&node.tags) {
            let (x, y) = lonlat_to_web_mercator_m(lon, lat);
            pt_stops.push(Stop {
                id: format!("osm:n{}", node.id.0),
                x,
                y,
                name: None,
                interchange_id: None,
                stop_type: Some(stop_type),
                country_iso2: None,
                station_boarding_capacity_pph: None,
                station_alighting_capacity_pph: None,
                station_queue_capacity_pax: None,
            });
            if let Some(max) = max_stops {
                if pt_stops.len() >= max {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn build_shape_stops(ways: &[CandidateWay], node_xy: &HashMap<i64, (f64, f64)>) -> Vec<Stop> {
    let mut ids = HashSet::<i64>::new();
    for w in ways {
        for nid in &w.node_ids {
            ids.insert(*nid);
        }
    }
    let mut out = Vec::<Stop>::with_capacity(ids.len());
    for nid in ids {
        if let Some((x, y)) = node_xy.get(&nid).copied() {
            out.push(Stop {
                id: format!("osm:shape:n{nid}"),
                x,
                y,
                name: None,
                interchange_id: None,
                stop_type: Some("shape".to_string()),
                country_iso2: None,
                station_boarding_capacity_pph: None,
                station_alighting_capacity_pph: None,
                station_queue_capacity_pax: None,
            });
        }
    }
    out
}

fn build_corridor_links(
    ways: &[CandidateWay],
    node_xy: &HashMap<i64, (f64, f64)>,
    bbox: (f64, f64, f64, f64),
) -> Vec<Link> {
    let mut links = Vec::<Link>::new();
    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    for w in ways {
        let speed_mps = speed_for_mode(&w.mode);
        for (i, pair) in w.node_ids.windows(2).enumerate() {
            let a = pair[0];
            let b = pair[1];
            let Some((x1, y1)) = node_xy.get(&a).copied() else {
                continue;
            };
            let Some((x2, y2)) = node_xy.get(&b).copied() else {
                continue;
            };

            let (lon1, lat1) = interlinked_engine::model::web_mercator_m_to_lonlat(x1, y1);
            let (lon2, lat2) = interlinked_engine::model::web_mercator_m_to_lonlat(x2, y2);
            let in_bbox =
                (lon1 >= min_lon && lon1 <= max_lon && lat1 >= min_lat && lat1 <= max_lat)
                    || (lon2 >= min_lon && lon2 <= max_lon && lat2 >= min_lat && lat2 <= max_lat);
            if !in_bbox {
                continue;
            }

            let dx = x2 - x1;
            let dy = y2 - y1;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 10.0 {
                continue;
            }

            links.push(Link {
                id: format!("osm:w{}:{i}", w.way_id),
                from_stop: format!("osm:shape:n{a}"),
                to_stop: format!("osm:shape:n{b}"),
                distance_m: dist,
                mode: w.mode.clone(),
                line_id: None,
                mode_variant: None,
                speed_mps,
                geometry: Some(vec![[x1, y1], [x2, y2]]),
                capacity_per_hour: None,
            });
        }
    }
    links
}

fn snap_nearby_stops(stops: Vec<Stop>, snap_m: f64) -> Vec<Stop> {
    let mut clusters: Vec<Stop> = Vec::new();
    for s in stops {
        let mut merged = false;
        for c in &mut clusters {
            let dx = c.x - s.x;
            let dy = c.y - s.y;
            if (dx * dx + dy * dy).sqrt() <= snap_m {
                c.x = (c.x + s.x) * 0.5;
                c.y = (c.y + s.y) * 0.5;
                merged = true;
                break;
            }
        }
        if !merged {
            clusters.push(s);
        }
    }
    clusters
}

fn dedupe_links(links: Vec<Link>) -> Vec<Link> {
    let mut best = HashMap::<(String, String, String), Link>::new();
    for l in links {
        if l.from_stop == l.to_stop {
            continue;
        }
        let key = (l.from_stop.clone(), l.to_stop.clone(), l.mode.clone());
        match best.get(&key) {
            None => {
                best.insert(key, l);
            }
            Some(prev) => {
                if l.distance_m < prev.distance_m {
                    best.insert(key, l);
                }
            }
        }
    }
    best.into_values().collect()
}

fn keep_largest_component(links: &[Link]) -> HashSet<String> {
    let mut adj = HashMap::<String, Vec<String>>::new();
    for l in links {
        adj.entry(l.from_stop.clone())
            .or_default()
            .push(l.to_stop.clone());
        adj.entry(l.to_stop.clone())
            .or_default()
            .push(l.from_stop.clone());
    }
    let mut seen = HashSet::<String>::new();
    let mut best = HashSet::<String>::new();
    for node in adj.keys() {
        if seen.contains(node) {
            continue;
        }
        let mut q = VecDeque::new();
        let mut comp = HashSet::<String>::new();
        q.push_back(node.clone());
        seen.insert(node.clone());
        while let Some(u) = q.pop_front() {
            comp.insert(u.clone());
            if let Some(nei) = adj.get(&u) {
                for v in nei {
                    if seen.insert(v.clone()) {
                        q.push_back(v.clone());
                    }
                }
            }
        }
        if comp.len() > best.len() {
            best = comp;
        }
    }
    best
}

fn build_grid_zones(bbox: (f64, f64, f64, f64), stops: &[Stop], grid_n: usize) -> Vec<Zone> {
    let (min_lon, min_lat, max_lon, max_lat) = bbox;
    let (minx, miny) = lonlat_to_web_mercator_m(min_lon, min_lat);
    let (maxx, maxy) = lonlat_to_web_mercator_m(max_lon, max_lat);
    let dx = (maxx - minx) / grid_n as f64;
    let dy = (maxy - miny) / grid_n as f64;

    let mut zones = Vec::<Zone>::with_capacity(grid_n * grid_n);
    for iy in 0..grid_n {
        for ix in 0..grid_n {
            zones.push(Zone {
                id: format!("z:{ix}:{iy}"),
                x: minx + (ix as f64 + 0.5) * dx,
                y: miny + (iy as f64 + 0.5) * dy,
                population: 1.0,
                jobs: 1.0,
                country_iso2: None,
            });
        }
    }

    let mut zone_counts = vec![0usize; zones.len()];
    for s in stops {
        if s.stop_type.as_deref() == Some("shape") {
            continue;
        }
        let mut ix = ((s.x - minx) / dx).floor() as isize;
        let mut iy = ((s.y - miny) / dy).floor() as isize;
        if ix < 0 {
            ix = 0;
        }
        if iy < 0 {
            iy = 0;
        }
        if ix as usize >= grid_n {
            ix = (grid_n - 1) as isize;
        }
        if iy as usize >= grid_n {
            iy = (grid_n - 1) as isize;
        }
        let idx = iy as usize * grid_n + ix as usize;
        zone_counts[idx] += 1;
    }
    let cx = (minx + maxx) * 0.5;
    let cy = (miny + maxy) * 0.5;
    let sx = ((maxx - minx) * 0.5).abs().max(1.0);
    let sy = ((maxy - miny) * 0.5).abs().max(1.0);
    for (z, c) in zones.iter_mut().zip(zone_counts) {
        let stop_weight = c as f64;
        let nx = (z.x - cx) / sx;
        let ny = (z.y - cy) / sy;
        let r = (nx * nx + ny * ny).sqrt().min(2.0);
        let angle = ny.atan2(nx);

        let cbd = (-2.6 * r * r).exp();
        let suburban = (-((r - 0.85).powi(2)) / 0.32).exp();
        let corridor = (1.0 + 0.25 * (2.0 * angle).cos() + 0.15 * (3.0 * angle).sin()).max(0.2);

        let pop_raw = 1.0 + 1.4 * stop_weight + 2.4 * suburban + 0.9 * (1.0 - cbd) + 0.4 * corridor;
        let jobs_raw = 1.0 + 1.8 * stop_weight + 3.1 * cbd + 0.8 * corridor + 0.3 * suburban;
        z.population = pop_raw.max(1.0);
        z.jobs = jobs_raw.max(1.0);
    }
    zones
}

fn infer_services_from_links(links: &[Link], headway_s: f64) -> Vec<Service> {
    let mut by_mode = HashMap::<String, Vec<&Link>>::new();
    for l in links {
        by_mode.entry(l.mode.clone()).or_default().push(l);
    }
    let mut services = Vec::<Service>::new();
    for (mode, lset) in by_mode {
        let mut out_deg = HashMap::<String, usize>::new();
        let mut in_deg = HashMap::<String, usize>::new();
        let mut nexts = HashMap::<String, Vec<String>>::new();
        for l in &lset {
            *out_deg.entry(l.from_stop.clone()).or_insert(0) += 1;
            *in_deg.entry(l.to_stop.clone()).or_insert(0) += 1;
            nexts
                .entry(l.from_stop.clone())
                .or_default()
                .push(l.to_stop.clone());
        }
        let mut used = HashSet::<(String, String)>::new();
        for (from, tos) in &nexts {
            for to in tos {
                let key = (from.clone(), to.clone());
                if used.contains(&key) {
                    continue;
                }
                let mut seq = vec![from.clone(), to.clone()];
                used.insert(key);
                let mut tail = to.clone();
                while let Some(next_list) = nexts.get(&tail) {
                    if next_list.len() != 1 {
                        break;
                    }
                    if in_deg.get(&tail).copied().unwrap_or(0) != 1 {
                        break;
                    }
                    let n2 = next_list[0].clone();
                    let k2 = (tail.clone(), n2.clone());
                    if used.contains(&k2) {
                        break;
                    }
                    seq.push(n2.clone());
                    used.insert(k2);
                    tail = n2;
                }
                services.push(Service {
                    id: format!("svc:{mode}:{}", services.len()),
                    mode: mode.clone(),
                    line_id: None,
                    name: None,
                    direction: None,
                    direction_name: None,
                    display_color: None,
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    mode_variant: None,
                    stop_sequence: seq,
                    headway_s: headway_s.max(60.0),
                    dwell_s: 15.0,
                    vehicle_capacity: default_vehicle_capacity_for_mode(&mode),
                    board_penalty_s: Some(default_board_penalty_for_mode(&mode)),
                });
            }
        }
    }
    services
}

fn build_link_shuttle_services(links: &[Link]) -> Vec<Service> {
    links
        .iter()
        .map(|l| Service {
            id: format!("svc:{}", l.id),
            mode: l.mode.clone(),
            line_id: None,
            name: None,
            direction: None,
            direction_name: None,
            display_color: None,
            service_enabled: None,
            operating_tph: None,
            stock_tier_id: None,
            stock_units_owned: None,
            stock_units_assigned: None,
            rolling_stock_profile: None,
            schedule_profile: None,
            mode_variant: None,
            stop_sequence: vec![l.from_stop.clone(), l.to_stop.clone()],
            headway_s: 600.0,
            dwell_s: 15.0,
            vehicle_capacity: default_vehicle_capacity_for_mode(&l.mode),
            board_penalty_s: Some(default_board_penalty_for_mode(&l.mode)),
        })
        .collect()
}

fn sibling_output(input: &str, filename: &str) -> String {
    let p = Path::new(input);
    p.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(filename)
        .to_string_lossy()
        .to_string()
}

fn normalize_key(input: &str) -> String {
    input.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn is_lsoa_code(code: &str) -> bool {
    let code = code.trim();
    if code.len() != 9 {
        return false;
    }
    let mut chars = code.chars();
    let Some(prefix) = chars.next() else {
        return false;
    };
    if prefix != 'E' && prefix != 'W' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_digit())
}

fn parse_numeric_cell(raw: &str, field_name: &str) -> Result<f64, String> {
    let v = raw.trim();
    if v.is_empty() || v == "*" {
        return Ok(0.0);
    }
    let cleaned = v.replace(',', "");
    cleaned
        .parse::<f64>()
        .map_err(|e| format!("bad numeric value '{v}' in field '{field_name}': {e}"))
}

fn parse_required_numeric_cell(raw: &str, field_name: &str) -> Result<f64, String> {
    let v = raw.trim();
    if v.is_empty() || v == "*" {
        return Err(format!("missing numeric value in field '{field_name}'"));
    }
    parse_numeric_cell(v, field_name)
}

fn is_west_yorkshire_lad(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.contains("leeds")
        || n.contains("bradford")
        || n.contains("calderdale")
        || n.contains("kirklees")
        || n.contains("wakefield")
}

fn read_lsoa_population(path: &str) -> Result<HashMap<String, LsoaPopulationRow>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let mut code_idx: Option<usize> = None;
    let mut lad_idx: Option<usize> = None;
    let mut total_idx: Option<usize> = None;
    let mut map = HashMap::<String, LsoaPopulationRow>::new();

    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        if code_idx.is_none() {
            let c = col_idx(&rec, "LSOA 2021 Code");
            let l = col_idx(&rec, "LAD 2023 Name");
            let t = col_idx(&rec, "Total");
            if c.is_some() && l.is_some() && t.is_some() {
                code_idx = c;
                lad_idx = l;
                total_idx = t;
            }
            continue;
        }

        let ci = code_idx.expect("code idx set after header detection");
        let li = lad_idx.expect("lad idx set after header detection");
        let ti = total_idx.expect("total idx set after header detection");
        let code = rec.get(ci).unwrap_or("").trim();
        if !is_lsoa_code(code) {
            continue;
        }
        if map.contains_key(code) {
            return Err(format!("duplicate population row for zone_id '{code}'"));
        }
        let lad_name = rec.get(li).unwrap_or("").trim();
        if lad_name.is_empty() {
            return Err(format!("missing LAD 2023 Name for zone_id '{code}'"));
        }
        let population = parse_required_numeric_cell(rec.get(ti).unwrap_or(""), "Total")?;
        map.insert(
            code.to_string(),
            LsoaPopulationRow {
                lad_name: lad_name.to_string(),
                population: population.max(0.0),
            },
        );
    }

    if code_idx.is_none() {
        return Err(
            "population CSV missing required columns: 'LSOA 2021 Code', 'LAD 2023 Name', 'Total'"
                .to_string(),
        );
    }
    if map.is_empty() {
        return Err("population CSV contained no valid LSOA rows".to_string());
    }
    Ok(map)
}

fn read_lsoa_jobs(path: &str) -> Result<HashMap<String, f64>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let mut mnemonic_idx: Option<usize> = None;
    let mut sector_cols: Option<Vec<(char, usize)>> = None;
    let mut map = HashMap::<String, f64>::new();

    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        if mnemonic_idx.is_none() {
            if let Some(idx) = col_idx(&rec, "mnemonic") {
                let mut cols = HashMap::<char, usize>::new();
                for (i, h) in rec.iter().enumerate() {
                    let t = h.trim();
                    if t.len() >= 4 {
                        let mut chars = t.chars();
                        let ch = chars.next().unwrap_or_default();
                        if ('A'..='U').contains(&ch)
                            && t.strip_prefix(ch)
                                .map(|rest| rest.starts_with(" :"))
                                .unwrap_or(false)
                        {
                            cols.entry(ch).or_insert(i);
                        }
                    }
                }
                let mut ordered = Vec::<(char, usize)>::new();
                for letter in 'A'..='U' {
                    let Some(i) = cols.get(&letter).copied() else {
                        return Err(format!(
                            "jobs CSV missing top-level sector column '{letter} : ...'"
                        ));
                    };
                    ordered.push((letter, i));
                }
                mnemonic_idx = Some(idx);
                sector_cols = Some(ordered);
            }
            continue;
        }

        let mi = mnemonic_idx.expect("mnemonic idx set after header detection");
        let code = rec.get(mi).unwrap_or("").trim();
        if !is_lsoa_code(code) {
            continue;
        }
        if map.contains_key(code) {
            return Err(format!("duplicate jobs row for zone_id '{code}'"));
        }
        let mut total = 0.0;
        for (letter, idx) in sector_cols.as_ref().expect("sector columns set") {
            let raw = rec.get(*idx).unwrap_or("");
            total += parse_numeric_cell(raw, &format!("{letter} sector"))?;
        }
        map.insert(code.to_string(), total.max(0.0));
    }

    if mnemonic_idx.is_none() {
        return Err("jobs CSV missing required 'mnemonic' header row".to_string());
    }
    if map.is_empty() {
        return Err("jobs CSV contained no valid LSOA rows".to_string());
    }
    Ok(map)
}

fn read_lsoa_centroids(path: &str) -> Result<HashMap<String, (f64, f64)>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let code_idx = col_idx(&headers, "LSOA21CD")
        .ok_or_else(|| "centroids CSV missing column 'LSOA21CD'".to_string())?;
    let lon_idx =
        col_idx(&headers, "lon").ok_or_else(|| "centroids CSV missing column 'lon'".to_string())?;
    let lat_idx =
        col_idx(&headers, "lat").ok_or_else(|| "centroids CSV missing column 'lat'".to_string())?;

    let mut map = HashMap::<String, (f64, f64)>::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let code = rec.get(code_idx).unwrap_or("").trim();
        if !is_lsoa_code(code) {
            continue;
        }
        if map.contains_key(code) {
            return Err(format!("duplicate centroid row for zone_id '{code}'"));
        }
        let lon = parse_required_numeric_cell(rec.get(lon_idx).unwrap_or(""), "lon")?;
        let lat = parse_required_numeric_cell(rec.get(lat_idx).unwrap_or(""), "lat")?;
        map.insert(code.to_string(), (lon, lat));
    }
    if map.is_empty() {
        return Err("centroids CSV contained no valid LSOA rows".to_string());
    }
    Ok(map)
}

fn lonlat_to_target_xy(lon: f64, lat: f64, target_crs: &str) -> Result<(f64, f64), String> {
    match normalize_key(target_crs).as_str() {
        "epsg3857" => Ok(lonlat_to_web_mercator_m(lon, lat)),
        "wgs84" => Ok((lon, lat)),
        other => Err(format!(
            "unsupported target_crs '{other}' (supported: epsg3857, wgs84)"
        )),
    }
}

fn normalize_lsoa_rows(
    population: &HashMap<String, LsoaPopulationRow>,
    jobs: &HashMap<String, f64>,
    centroids: &HashMap<String, (f64, f64)>,
    region: &str,
    target_crs: &str,
) -> Result<LsoaNormalizationResult, String> {
    if normalize_key(region).as_str() != "west_yorkshire" {
        return Err(format!(
            "unsupported region '{}' (currently supported: west_yorkshire)",
            region
        ));
    }

    let mut rows = Vec::<NormalizedZoneRow>::new();
    let mut region_rows = 0usize;
    let mut missing_jobs = 0usize;
    let mut missing_centroids = 0usize;

    for (code, pop) in population {
        if !is_west_yorkshire_lad(&pop.lad_name) {
            continue;
        }
        region_rows += 1;
        let Some(jobs_value) = jobs.get(code).copied() else {
            missing_jobs += 1;
            continue;
        };
        let Some((lon, lat)) = centroids.get(code).copied() else {
            missing_centroids += 1;
            continue;
        };
        let (x, y) = lonlat_to_target_xy(lon, lat, target_crs)?;
        if !x.is_finite() || !y.is_finite() {
            return Err(format!(
                "non-finite transformed coordinate for zone_id '{code}'"
            ));
        }
        rows.push(NormalizedZoneRow {
            zone_id: code.clone(),
            x,
            y,
            population: pop.population.max(0.0),
            jobs: jobs_value.max(0.0),
        });
    }
    rows.sort_by(|a, b| a.zone_id.cmp(&b.zone_id));
    if rows.is_empty() {
        return Err("normalization produced zero rows after filtering/join".to_string());
    }

    Ok(LsoaNormalizationResult {
        rows,
        population_rows: population.len(),
        region_rows,
        missing_jobs,
        missing_centroids,
    })
}

fn write_normalized_zones_csv(path: &str, rows: &[NormalizedZoneRow]) -> Result<(), String> {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    wtr.write_record(["zone_id", "x", "y", "population", "jobs"])
        .map_err(|e| e.to_string())?;
    for row in rows {
        let x = row.x.to_string();
        let y = row.y.to_string();
        let population = row.population.to_string();
        let jobs = row.jobs.to_string();
        wtr.write_record([
            row.zone_id.as_str(),
            x.as_str(),
            y.as_str(),
            population.as_str(),
            jobs.as_str(),
        ])
        .map_err(|e| e.to_string())?;
    }
    wtr.flush().map_err(|e| e.to_string())
}

fn read_normalized_zones_csv(path: &str) -> Result<Vec<Zone>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let zone_idx =
        col_idx(&headers, "zone_id").ok_or_else(|| "missing column 'zone_id'".to_string())?;
    let x_idx = col_idx(&headers, "x").ok_or_else(|| "missing column 'x'".to_string())?;
    let y_idx = col_idx(&headers, "y").ok_or_else(|| "missing column 'y'".to_string())?;
    let pop_idx =
        col_idx(&headers, "population").ok_or_else(|| "missing column 'population'".to_string())?;
    let jobs_idx = col_idx(&headers, "jobs").ok_or_else(|| "missing column 'jobs'".to_string())?;

    let mut out = Vec::<Zone>::new();
    let mut seen = HashSet::<String>::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let zone_id = rec.get(zone_idx).unwrap_or("").trim();
        if zone_id.is_empty() {
            continue;
        }
        if !seen.insert(zone_id.to_string()) {
            return Err(format!(
                "duplicate zone_id '{zone_id}' in normalized census CSV"
            ));
        }
        let x = parse_required_numeric_cell(rec.get(x_idx).unwrap_or(""), "x")?;
        let y = parse_required_numeric_cell(rec.get(y_idx).unwrap_or(""), "y")?;
        let population =
            parse_required_numeric_cell(rec.get(pop_idx).unwrap_or(""), "population")?.max(0.0);
        let jobs = parse_required_numeric_cell(rec.get(jobs_idx).unwrap_or(""), "jobs")?.max(0.0);
        if !x.is_finite() || !y.is_finite() {
            return Err(format!("non-finite coordinate for zone_id '{zone_id}'"));
        }
        out.push(Zone {
            id: zone_id.to_string(),
            x,
            y,
            population,
            jobs,
            country_iso2: None,
        });
    }
    if out.is_empty() {
        return Err("normalized census CSV contained no rows".to_string());
    }
    Ok(out)
}

fn read_demand_profile(path: &str) -> Result<Vec<DemandTimeSlice>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let mut out = Vec::<DemandTimeSlice>::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        out.push(DemandTimeSlice {
            label: parse_required_string(&headers, &rec, "label")?,
            start_s: parse_required_f64(&headers, &rec, "start_s")?,
            end_s: parse_required_f64(&headers, &rec, "end_s")?,
            multiplier: parse_required_f64(&headers, &rec, "multiplier")?,
        });
    }
    Ok(out)
}

fn resolve_zone_row(
    headers: &StringRecord,
    rec: &StringRecord,
    zones: &[Zone],
    zone_index: &HashMap<String, usize>,
) -> Result<Option<usize>, String> {
    if let Some(i) = col_idx(headers, "zone_id") {
        let zid = rec.get(i).unwrap_or("").trim();
        if !zid.is_empty() {
            return Ok(zone_index.get(zid).copied());
        }
    }
    let x = parse_optional_f64(headers, rec, "x")?;
    let y = parse_optional_f64(headers, rec, "y")?;
    let (Some(x), Some(y)) = (x, y) else {
        return Ok(None);
    };
    let mut best: Option<(usize, f64)> = None;
    for (i, z) in zones.iter().enumerate() {
        let dx = z.x - x;
        let dy = z.y - y;
        let d2 = dx * dx + dy * dy;
        match best {
            None => best = Some((i, d2)),
            Some((_, bd2)) if d2 < bd2 => best = Some((i, d2)),
            _ => {}
        }
    }
    Ok(best.map(|(i, _)| i))
}

fn read_gtfs_stops(path: &Path) -> Result<HashMap<String, GtfsStop>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let mut map = HashMap::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let id = parse_required_string(&headers, &rec, "stop_id")?;
        let lon = parse_required_f64(&headers, &rec, "stop_lon")?;
        let lat = parse_required_f64(&headers, &rec, "stop_lat")?;
        map.insert(id, GtfsStop { lon, lat });
    }
    Ok(map)
}

fn read_gtfs_route_modes(path: &Path) -> Result<HashMap<String, String>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let mut map = HashMap::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let route_id = parse_required_string(&headers, &rec, "route_id")?;
        let route_type = parse_required_string(&headers, &rec, "route_type")?;
        map.insert(route_id, map_route_type_to_mode(&route_type));
    }
    Ok(map)
}

fn read_gtfs_trip_routes(path: &Path) -> Result<HashMap<String, String>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let mut map = HashMap::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let trip_id = parse_required_string(&headers, &rec, "trip_id")?;
        let route_id = parse_required_string(&headers, &rec, "route_id")?;
        map.insert(trip_id, route_id);
    }
    Ok(map)
}

fn read_gtfs_trip_stop_sequences(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let mut rows = HashMap::<String, Vec<(u32, String)>>::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let trip_id = parse_required_string(&headers, &rec, "trip_id")?;
        let stop_id = parse_required_string(&headers, &rec, "stop_id")?;
        let seq = parse_required_f64(&headers, &rec, "stop_sequence")? as u32;
        rows.entry(trip_id).or_default().push((seq, stop_id));
    }
    let mut out = HashMap::<String, Vec<String>>::new();
    for (trip, mut recs) in rows {
        recs.sort_by_key(|(s, _)| *s);
        out.insert(trip, recs.into_iter().map(|(_, sid)| sid).collect());
    }
    Ok(out)
}

fn build_stop_refs(s: &Scenario) -> Vec<StopRef> {
    s.world
        .stops
        .iter()
        .map(|st| {
            let (mx, my) = world_xy_to_web_mercator_m(&s.meta.crs, st.x, st.y);
            StopRef {
                id: st.id.clone(),
                mx,
                my,
                is_shape: st.stop_type.as_deref() == Some("shape"),
            }
        })
        .collect()
}

fn map_or_create_stop(
    scenario: &mut Scenario,
    refs: &mut Vec<StopRef>,
    gtfs_stop_id: &str,
    mx: f64,
    my: f64,
    snap_m: f64,
) -> String {
    let mut best: Option<(usize, f64)> = None;
    for (i, s) in refs.iter().enumerate() {
        if s.is_shape {
            continue;
        }
        let dx = s.mx - mx;
        let dy = s.my - my;
        let d2 = dx * dx + dy * dy;
        match best {
            None => best = Some((i, d2)),
            Some((_, bd2)) if d2 < bd2 => best = Some((i, d2)),
            _ => {}
        }
    }
    if let Some((i, d2)) = best {
        if d2.sqrt() <= snap_m {
            return refs[i].id.clone();
        }
    }
    let id = format!("gtfs:stop:{gtfs_stop_id}");
    if scenario.world.stops.iter().any(|s| s.id == id) {
        return id;
    }
    let (wx, wy) = web_mercator_m_to_world_xy(&scenario.meta.crs, mx, my);
    scenario.world.stops.push(Stop {
        id: id.clone(),
        x: wx,
        y: wy,
        name: None,
        interchange_id: None,
        stop_type: Some("gtfs_stop".to_string()),
        country_iso2: None,
        station_boarding_capacity_pph: None,
        station_alighting_capacity_pph: None,
        station_queue_capacity_pax: None,
    });
    refs.push(StopRef {
        id: id.clone(),
        mx,
        my,
        is_shape: false,
    });
    id
}

fn ensure_links_for_sequence(
    s: &mut Scenario,
    seq: &[String],
    mode: &str,
    added_links: &mut usize,
) {
    let mut stop_xy = HashMap::<&str, (f64, f64)>::new();
    for st in &s.world.stops {
        let (mx, my) = world_xy_to_web_mercator_m(&s.meta.crs, st.x, st.y);
        stop_xy.insert(st.id.as_str(), (mx, my));
    }

    for w in seq.windows(2) {
        let from = &w[0];
        let to = &w[1];
        if s.world
            .links
            .iter()
            .any(|l| &l.from_stop == from && &l.to_stop == to && l.mode == mode)
        {
            continue;
        }
        let Some((x1, y1)) = stop_xy.get(from.as_str()).copied() else {
            continue;
        };
        let Some((x2, y2)) = stop_xy.get(to.as_str()).copied() else {
            continue;
        };
        let dx = x2 - x1;
        let dy = y2 - y1;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist <= 1.0 {
            continue;
        }
        s.world.links.push(Link {
            id: format!("gtfs:lnk:{mode}:{from}:{to}"),
            from_stop: from.clone(),
            to_stop: to.clone(),
            distance_m: dist,
            mode: mode.to_string(),
            line_id: None,
            mode_variant: None,
            speed_mps: speed_for_mode(mode),
            geometry: Some(vec![[x1, y1], [x2, y2]]),
            capacity_per_hour: None,
        });
        *added_links += 1;
    }
}

fn parse_country_info(path: &str) -> Result<HashMap<String, CatalogCountry>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open country info: {e}"))?;
    let reader = BufReader::new(file);
    let mut out = HashMap::<String, CatalogCountry>::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 17 {
            continue;
        }
        let iso2 = cols[0].trim().to_uppercase();
        if iso2.len() != 2 {
            continue;
        }
        let name = cols[4].trim().to_string();
        if name.is_empty() {
            continue;
        }
        let capital_name = if cols[5].trim().is_empty() {
            None
        } else {
            Some(cols[5].trim().to_string())
        };
        let capital_geonameid = cols[16].trim().parse::<i64>().ok();
        out.insert(
            iso2.clone(),
            CatalogCountry {
                iso2,
                name,
                capital_name,
                capital_geonameid,
            },
        );
    }
    if out.is_empty() {
        return Err("no countries parsed from countryInfo source".to_string());
    }
    Ok(out)
}

fn parse_geonames_cities(path: &str) -> Result<HashMap<String, Vec<CatalogCity>>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open cities source: {e}"))?;
    let reader = BufReader::new(file);
    let mut out = HashMap::<String, Vec<CatalogCity>>::new();
    let mut seen = HashSet::<i64>::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 15 {
            continue;
        }
        let geonameid = match cols[0].trim().parse::<i64>() {
            Ok(v) if v > 0 => v,
            _ => continue,
        };
        if !seen.insert(geonameid) {
            continue;
        }
        let feature_class = cols[6].trim();
        if feature_class != "P" {
            continue;
        }
        let country_code = cols[8].trim().to_uppercase();
        if country_code.len() != 2 {
            continue;
        }
        let lat = match cols[4].trim().parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => continue,
        };
        let lon = match cols[5].trim().parse::<f64>() {
            Ok(v) if v.is_finite() => v,
            _ => continue,
        };
        let population = cols[14].trim().parse::<u64>().unwrap_or(0);
        let name = cols[1].trim().to_string();
        let ascii_name = cols[2].trim().to_string();
        if name.is_empty() {
            continue;
        }

        out.entry(country_code).or_default().push(CatalogCity {
            geonameid,
            name,
            ascii_name,
            lat,
            lon,
            population,
            feature_code: cols[7].trim().to_string(),
        });
    }
    if out.is_empty() {
        return Err("no city rows parsed from GeoNames source".to_string());
    }
    Ok(out)
}

fn col_idx(headers: &StringRecord, name: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

fn parse_required_string(
    headers: &StringRecord,
    rec: &StringRecord,
    col: &str,
) -> Result<String, String> {
    let i = col_idx(headers, col).ok_or_else(|| format!("missing column '{col}'"))?;
    let v = rec.get(i).unwrap_or("").trim();
    if v.is_empty() {
        return Err(format!("column '{col}' is empty"));
    }
    Ok(v.to_string())
}

fn parse_required_f64(
    headers: &StringRecord,
    rec: &StringRecord,
    col: &str,
) -> Result<f64, String> {
    let v = parse_required_string(headers, rec, col)?;
    v.parse::<f64>()
        .map_err(|e| format!("bad f64 in column '{col}': {e}"))
}

fn parse_optional_f64(
    headers: &StringRecord,
    rec: &StringRecord,
    col: &str,
) -> Result<Option<f64>, String> {
    let Some(i) = col_idx(headers, col) else {
        return Ok(None);
    };
    let raw = rec.get(i).unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed = raw
        .parse::<f64>()
        .map_err(|e| format!("bad f64 in column '{col}': {e}"))?;
    Ok(Some(parsed))
}

fn classify_way(tags: &Tags) -> Option<String> {
    if matches_tag(tags, "railway", &["rail", "light_rail", "subway"]) {
        return Some("rail".to_string());
    }
    if matches_tag(tags, "railway", &["tram"]) {
        return Some("tram".to_string());
    }
    None
}

fn classify_stop(tags: &Tags) -> Option<String> {
    if matches_tag(tags, "railway", &["station", "halt"]) {
        return Some("rail_station".to_string());
    }
    if matches_tag(tags, "railway", &["tram_stop"]) {
        return Some("tram_stop".to_string());
    }
    if matches_tag(tags, "highway", &["bus_stop"]) {
        return Some("bus_stop".to_string());
    }
    if matches_tag(tags, "public_transport", &["platform", "stop_position"]) {
        return Some("pt_platform".to_string());
    }
    if matches_tag(tags, "amenity", &["ferry_terminal"]) {
        return Some("ferry_terminal".to_string());
    }
    if matches_tag(tags, "aerialway", &["station"]) {
        return Some("aerialway_station".to_string());
    }
    None
}

fn matches_tag(tags: &Tags, key: &str, values: &[&str]) -> bool {
    tags.get(key)
        .map(|v| values.iter().any(|x| *x == v.as_str()))
        .unwrap_or(false)
}

fn map_route_type_to_mode(route_type: &str) -> String {
    match route_type.trim() {
        "0" => "tram".to_string(),
        "1" => "metro".to_string(),
        "2" => "rail".to_string(),
        "3" => "bus".to_string(),
        "4" => "ferry".to_string(),
        "5" => "cable_car".to_string(),
        "6" => "aerialway".to_string(),
        "7" => "funicular".to_string(),
        "11" => "trolleybus".to_string(),
        "12" => "monorail".to_string(),
        _ => "bus".to_string(),
    }
}

fn speed_for_mode(mode: &str) -> f64 {
    match mode {
        "rail" => 20.0,
        "metro" => 16.0,
        "tram" => 12.0,
        "ferry" => 8.0,
        "cable_car" | "aerialway" | "funicular" => 6.0,
        "high_speed_rail" => 40.0,
        _ => 10.0,
    }
}

fn default_vehicle_capacity_for_mode(mode: &str) -> f64 {
    match mode {
        "rail" => 220.0,
        "metro" => 180.0,
        "tram" => 90.0,
        "ferry" => 250.0,
        "high_speed_rail" => 400.0,
        "cable_car" | "aerialway" => 35.0,
        _ => 65.0,
    }
}

fn default_board_penalty_for_mode(mode: &str) -> f64 {
    match mode {
        "high_speed_rail" => 180.0,
        "rail" | "metro" => 60.0,
        "ferry" => 120.0,
        _ => 20.0,
    }
}

fn default_params_fallback() -> Params {
    Params {
        walk_weight: 1.0,
        wait_weight: 2.0,
        ivt_weight: 1.0,
        transfer_penalty_s: 300.0,
        access_walk_speed_mps: 1.4,
        access_radius_m: 1200.0,
        gravity_beta: 0.0003,
        trips_per_person: 1.0,
        purpose_share_home_work: 0.52,
        purpose_share_home_education: 0.12,
        purpose_share_home_retail: 0.18,
        purpose_share_home_recreation: 0.10,
        purpose_share_other: 0.08,
        attraction_weight_office: 1.0,
        attraction_weight_retail: 0.9,
        attraction_weight_recreation: 0.7,
        attraction_weight_industrial: 1.1,
        attraction_weight_education: 0.8,
        attraction_weight_health: 0.75,
        route_choice_k: 3,
        route_choice_theta: 0.002,
        assignment_max_iters: 8,
        assignment_convergence_rel: 0.01,
        capacity_enabled: true,
        queue_max_extra_wait_s: 3600.0,
        fare_enabled: true,
        fare_value_of_time_base_per_hour: 12.0,
        fare_elasticity: 0.35,
        fare_reference_base: 2.5,
        fare_transfer_window_s: 2700.0,
        fare_free_transfers_per_trip: 1,
        fare_overflow_retry_share: 0.15,
        fare_mode_bus_base: 1.8,
        fare_mode_tram_base: 2.3,
        fare_mode_metro_base: 2.7,
        fare_mode_rail_base: 3.6,
        fare_mode_ferry_base: 3.0,
        fare_mode_default_base: 2.5,
        station_capacity_scale_boarding: 1.0,
        station_capacity_scale_alighting: 1.0,
        station_queue_capacity_scale: 1.0,
        debug_sample_origin_zone: None,
        debug_sample_dest_zone: None,
        demand_profile: vec![],
        demand_purpose_profile: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("interlinked_osm_{nanos}_{name}"))
            .to_string_lossy()
            .to_string()
    }

    fn write_tmp(name: &str, content: &str) -> String {
        let p = unique_path(name);
        fs::write(&p, content).expect("failed to write temp file");
        p
    }

    #[test]
    fn normalize_handles_header_offset_and_filters_region() {
        let pop_csv = write_tmp(
            "pop.csv",
            "meta\nnote\nskip\nLAD 2023 Code,LAD 2023 Name,LSOA 2021 Code,LSOA 2021 Name,Total\nE08000035,Leeds,E01000001,Leeds A,\"1,500\"\nE08000036,Wakefield,E01000002,Wakefield A,900\nE06000001,Hartlepool,E01000003,Hartlepool A,700\n",
        );
        let jobs_csv = write_tmp(
            "jobs.csv",
            "meta\nnote\n2021 super output area - lower layer,mnemonic,A : Agriculture, forestry and fishing,B : Mining and quarrying,C : Manufacturing,D : Electricity, gas, steam and air conditioning supply,E : Water supply; sewerage, waste management and remediation activities,F : Construction,G : Wholesale and retail trade; repair of motor vehicles and motorcycles,H : Transportation and storage,I : Accommodation and food service activities,J : Information and communication,K : Financial and insurance activities,L : Real estate activities,M : Professional, scientific and technical activities,N : Administrative and support service activities,O : Public administration and defence; compulsory social security,P : Education,Q : Human health and social work activities,R : Arts, entertainment and recreation,S : Other service activities,T : Activities of households as employers;undifferentiated goods-and services-producing activities of households for own use,U : Activities of extraterritorial organisations and bodies\nLeeds A,E01000001,10,0,0,0,0,0,5,0,0,0,0,0,0,0,0,0,0,0,0,0,0\nWakefield A,E01000002,20,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0\nHartlepool A,E01000003,30,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0\n",
        );
        let centroids_csv = write_tmp(
            "centroids.csv",
            "LSOA21CD,lon,lat\nE01000001,-1.5,53.8\nE01000002,-1.6,53.7\nE01000003,-1.2,54.0\n",
        );

        let pop = read_lsoa_population(&pop_csv).expect("population parse should succeed");
        let jobs = read_lsoa_jobs(&jobs_csv).expect("jobs parse should succeed");
        let centroids =
            read_lsoa_centroids(&centroids_csv).expect("centroids parse should succeed");
        let out = normalize_lsoa_rows(&pop, &jobs, &centroids, "west_yorkshire", "epsg3857")
            .expect("normalization should succeed");

        assert_eq!(out.population_rows, 3);
        assert_eq!(out.region_rows, 2);
        assert_eq!(out.rows.len(), 2);
        assert_eq!(out.rows[0].zone_id, "E01000001");
        assert_eq!(out.rows[1].zone_id, "E01000002");
        assert!(out.rows[0].x.is_finite() && out.rows[0].y.is_finite());
    }

    #[test]
    fn population_parser_rejects_missing_columns() {
        let pop_csv = write_tmp(
            "bad_pop.csv",
            "header\nLSOA 2021 Code,LAD 2023 Name\nE01000001,Leeds\n",
        );
        let err = read_lsoa_population(&pop_csv).expect_err("parser should fail");
        assert!(err.contains("missing required columns"));
    }

    #[test]
    fn normalized_reader_rejects_duplicate_zone_ids() {
        let norm_csv = write_tmp(
            "dup_norm.csv",
            "zone_id,x,y,population,jobs\nE01000001,1,2,100,50\nE01000001,3,4,120,70\n",
        );
        let err = read_normalized_zones_csv(&norm_csv).expect_err("duplicate rows must fail");
        assert!(err.contains("duplicate zone_id"));
    }

    #[test]
    fn lonlat_conversion_to_epsg3857_is_finite() {
        let (x, y) =
            lonlat_to_target_xy(-1.5491, 53.8008, "epsg3857").expect("conversion should succeed");
        assert!(x.is_finite() && y.is_finite());
    }

    #[test]
    fn replace_zones_path_preserves_network() {
        let zones_csv = write_tmp(
            "zones.csv",
            "zone_id,x,y,population,jobs\nE01000001,-172400.0,7118300.0,1500,320\nE01000002,-173500.0,7119100.0,900,120\n",
        );
        let mut scenario = Scenario {
            meta: Meta {
                name: "test".to_string(),
                seed: 42,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: default_params_fallback(),
            world: World {
                zones: vec![Zone {
                    id: "z:0:0".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 1.0,
                    jobs: 1.0,
                    country_iso2: None,
                }],
                stops: vec![
                    Stop {
                        id: "s1".to_string(),
                        x: 0.0,
                        y: 0.0,
                        name: None,
                        interchange_id: None,
                        stop_type: Some("bus_stop".to_string()),
                        country_iso2: None,
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "s2".to_string(),
                        x: 100.0,
                        y: 0.0,
                        name: None,
                        interchange_id: None,
                        stop_type: Some("bus_stop".to_string()),
                        country_iso2: None,
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                ],
                links: vec![Link {
                    id: "l1".to_string(),
                    from_stop: "s1".to_string(),
                    to_stop: "s2".to_string(),
                    distance_m: 100.0,
                    mode: "bus".to_string(),
                    line_id: None,
                    mode_variant: None,
                    speed_mps: 10.0,
                    geometry: None,
                    capacity_per_hour: None,
                }],
                services: vec![Service {
                    id: "svc1".to_string(),
                    mode: "bus".to_string(),
                    line_id: None,
                    name: None,
                    direction: None,
                    direction_name: None,
                    display_color: None,
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    mode_variant: None,
                    stop_sequence: vec!["s1".to_string(), "s2".to_string()],
                    headway_s: 600.0,
                    dwell_s: 15.0,
                    vehicle_capacity: 65.0,
                    board_penalty_s: Some(20.0),
                }],
                transfers: vec![],
                transfer_rules: None,
                demand_cells: vec![],
                demand_meta: None,
            },
        };

        let old_stops = scenario.world.stops.len();
        let old_links = scenario.world.links.len();
        let old_services = scenario.world.services.len();
        scenario.world.zones = read_normalized_zones_csv(&zones_csv).expect("zones should parse");

        assert_eq!(scenario.world.zones.len(), 2);
        assert_eq!(scenario.world.stops.len(), old_stops);
        assert_eq!(scenario.world.links.len(), old_links);
        assert_eq!(scenario.world.services.len(), old_services);
        ScenarioService::validate(&scenario).expect("scenario should validate after zone replace");
    }

    #[test]
    fn geonames_catalog_parser_reads_countries_and_cities() {
        let country_info = write_tmp(
            "country_info.txt",
            "#iso\tiso3\tnum\tfips\tname\tcapital\tarea\tpop\tcontinent\ttld\tcur\tcurname\tphone\tpostal\tregex\tlangs\tgeonameid\tneigh\tfips_eq\nGB\tGBR\t826\tUK\tUnited Kingdom\tLondon\t0\t0\tEU\t.uk\tGBP\tPound\t44\t\t\ten-GB\t2635167\tIE\t\nDE\tDEU\t276\tGM\tGermany\tBerlin\t0\t0\tEU\t.de\tEUR\tEuro\t49\t\t\tde\t2921044\t\t\n",
        );
        let cities = write_tmp(
            "allCountries.txt",
            "2643743\tLondon\tLondon\t\t51.50853\t-0.12574\tP\tPPLC\tGB\t\t\t\t\t\t9000000\t0\t0\tEurope/London\t2020-01-01\n5128581\tNew York City\tNew York City\t\t40.71427\t-74.00597\tP\tPPLA\tUS\t\t\t\t\t\t8175133\t0\t0\tAmerica/New_York\t2020-01-01\n2950159\tBerlin\tBerlin\t\t52.52437\t13.41053\tP\tPPLC\tDE\t\t\t\t\t\t3426354\t0\t0\tEurope/Berlin\t2020-01-01\n",
        );
        let countries = parse_country_info(&country_info).expect("countries should parse");
        let cities_by_country = parse_geonames_cities(&cities).expect("cities should parse");
        assert_eq!(
            countries.get("GB").expect("GB exists").name,
            "United Kingdom"
        );
        assert_eq!(cities_by_country.get("DE").expect("DE cities").len(), 1);
        assert_eq!(
            cities_by_country.get("GB").expect("GB cities")[0].name,
            "London"
        );
    }

    #[test]
    fn apply_demand_fabric_replaces_zones_and_populates_cells() {
        let scenario_path = unique_path("scenario_input.json");
        let out_path = unique_path("scenario_demand.json");
        let fabric_path = unique_path("fabric.json");

        let scenario = Scenario {
            meta: Meta {
                name: "fabric-test".to_string(),
                seed: 42,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: default_params_fallback(),
            world: World {
                zones: vec![Zone {
                    id: "z0".to_string(),
                    x: 0.0,
                    y: 0.0,
                    population: 10.0,
                    jobs: 5.0,
                    country_iso2: None,
                }],
                stops: vec![
                    Stop {
                        id: "s1".to_string(),
                        x: 0.0,
                        y: 0.0,
                        name: None,
                        interchange_id: None,
                        stop_type: Some("bus_stop".to_string()),
                        country_iso2: None,
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                    Stop {
                        id: "s2".to_string(),
                        x: 100.0,
                        y: 0.0,
                        name: None,
                        interchange_id: None,
                        stop_type: Some("bus_stop".to_string()),
                        country_iso2: None,
                        station_boarding_capacity_pph: None,
                        station_alighting_capacity_pph: None,
                        station_queue_capacity_pax: None,
                    },
                ],
                links: vec![Link {
                    id: "l1".to_string(),
                    from_stop: "s1".to_string(),
                    to_stop: "s2".to_string(),
                    distance_m: 100.0,
                    mode: "bus".to_string(),
                    line_id: None,
                    mode_variant: None,
                    speed_mps: 10.0,
                    geometry: None,
                    capacity_per_hour: None,
                }],
                services: vec![Service {
                    id: "sv1".to_string(),
                    mode: "bus".to_string(),
                    line_id: None,
                    name: None,
                    direction: None,
                    direction_name: None,
                    display_color: None,
                    service_enabled: None,
                    operating_tph: None,
                    stock_tier_id: None,
                    stock_units_owned: None,
                    stock_units_assigned: None,
                    rolling_stock_profile: None,
                    schedule_profile: None,
                    mode_variant: None,
                    stop_sequence: vec!["s1".to_string(), "s2".to_string()],
                    headway_s: 600.0,
                    dwell_s: 20.0,
                    vehicle_capacity: 65.0,
                    board_penalty_s: Some(20.0),
                }],
                transfers: vec![],
                transfer_rules: None,
                demand_cells: vec![],
                demand_meta: None,
            },
        };
        let doc = ScenarioDocument::new_current(scenario);
        ScenarioService::save_to_path(&scenario_path, &doc).expect("scenario should save");

        let fabric = DemandFabric {
            meta: DemandFabricMeta {
                schema_version: 1,
                generated_at_epoch_s: 0,
                source_pbf: "test".to_string(),
                h3_res: 8,
                target_crs: "epsg3857".to_string(),
                source_population_raster_csv: None,
                source_built_raster_csv: None,
                country_iso2: Some("GB".to_string()),
            },
            cells: vec![DemandFabricCell {
                cell_id: "88195da49bfffff".to_string(),
                lon: -1.5491,
                lat: 53.8008,
                x: -172445.0,
                y: 7119540.0,
                country_iso2: Some("GB".to_string()),
                area_m2: 700_000.0,
                residents_night: 1200.0,
                jobs_day: 800.0,
                activity_mix_residential: 0.4,
                activity_mix_office: 0.2,
                activity_mix_retail: 0.15,
                activity_mix_recreation: 0.1,
                activity_mix_industrial: 0.08,
                activity_mix_education: 0.04,
                activity_mix_health: 0.03,
                centrality_score: 0.5,
                data_quality_score: 0.9,
            }],
        };
        fs::write(
            &fabric_path,
            serde_json::to_string_pretty(&fabric).expect("fabric json should serialize"),
        )
        .expect("fabric should write");

        run_apply_demand_fabric(&scenario_path, &fabric_path, Some(&out_path), true)
            .expect("apply demand fabric should succeed");
        let out_doc =
            ScenarioService::load_from_path(&out_path).expect("output scenario should load");
        assert_eq!(out_doc.scenario.world.zones.len(), 1);
        assert_eq!(out_doc.scenario.world.demand_cells.len(), 1);
        assert_eq!(out_doc.scenario.world.zones[0].population, 1200.0);
        assert_eq!(out_doc.scenario.world.zones[0].jobs, 800.0);
    }

    #[test]
    fn demand_surface_from_fabric_preserves_res8_mix_and_v4_roundtrip() {
        let fabric = DemandFabric {
            meta: DemandFabricMeta {
                schema_version: 1,
                generated_at_epoch_s: 123,
                source_pbf: "test.osm.pbf".to_string(),
                h3_res: 8,
                target_crs: "epsg3857".to_string(),
                source_population_raster_csv: None,
                source_built_raster_csv: None,
                country_iso2: Some("GB".to_string()),
            },
            cells: vec![DemandFabricCell {
                cell_id: "88195da49bfffff".to_string(),
                lon: -1.5491,
                lat: 53.8008,
                x: -172445.0,
                y: 7119540.0,
                country_iso2: Some("GB".to_string()),
                area_m2: 700_000.0,
                residents_night: 1000.0,
                jobs_day: 600.0,
                activity_mix_residential: 0.42,
                activity_mix_office: 0.18,
                activity_mix_retail: 0.15,
                activity_mix_recreation: 0.10,
                activity_mix_industrial: 0.08,
                activity_mix_education: 0.04,
                activity_mix_health: 0.03,
                centrality_score: 0.4,
                data_quality_score: 0.8,
            }],
        };

        let surface =
            demand_surface_from_fabric("GB", "boundaries.geojson", "pop.csv", "built.csv", &fabric);
        assert_eq!(surface.surface_version, "v4");
        assert_eq!(surface.cells_res8.len(), 1);
        let c = &surface.cells_res8[0];
        assert!((c.activity_mix_residential - 0.42).abs() < 1e-9);
        assert!((c.activity_mix_office - 0.18).abs() < 1e-9);
        assert!((c.activity_mix_retail - 0.15).abs() < 1e-9);

        let json = serde_json::to_string(&surface).expect("surface should serialize");
        let roundtrip: DemandSurfaceCountry =
            serde_json::from_str(&json).expect("surface should deserialize");
        let rc = &roundtrip.cells_res8[0];
        let sum = rc.activity_mix_residential
            + rc.activity_mix_office
            + rc.activity_mix_retail
            + rc.activity_mix_recreation
            + rc.activity_mix_industrial
            + rc.activity_mix_education
            + rc.activity_mix_health;
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_surface_cells_weights_mix_by_density() {
        let dense = DemandSurfaceCell {
            cell_id: "88195da49bfffff".to_string(),
            h3_res: 8,
            lon: -1.5491,
            lat: 53.8008,
            x: -172445.0,
            y: 7119540.0,
            area_m2: 700_000.0,
            country_iso2: "GB".to_string(),
            residents_raw: 900.0,
            jobs_raw: 100.0,
            residents_smooth: 900.0,
            jobs_smooth: 100.0,
            activity_mix_residential: 1.0,
            activity_mix_office: 0.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            quality: 0.9,
        };
        let sparse = DemandSurfaceCell {
            cell_id: "88195da49bfffff".to_string(),
            h3_res: 8,
            lon: -1.5491,
            lat: 53.8008,
            x: -172445.0,
            y: 7119540.0,
            area_m2: 700_000.0,
            country_iso2: "GB".to_string(),
            residents_raw: 50.0,
            jobs_raw: 50.0,
            residents_smooth: 50.0,
            jobs_smooth: 50.0,
            activity_mix_residential: 0.0,
            activity_mix_office: 1.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            quality: 0.6,
        };

        let out = aggregate_surface_cells(&[dense, sparse], 7, "epsg3857", "GB");
        assert_eq!(out.len(), 1);
        let c = &out[0];
        assert!((c.activity_mix_residential - 0.909090909).abs() < 1e-6);
        assert!((c.activity_mix_office - 0.090909091).abs() < 1e-6);
        let sum = c.activity_mix_residential
            + c.activity_mix_office
            + c.activity_mix_retail
            + c.activity_mix_recreation
            + c.activity_mix_industrial
            + c.activity_mix_education
            + c.activity_mix_health;
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
