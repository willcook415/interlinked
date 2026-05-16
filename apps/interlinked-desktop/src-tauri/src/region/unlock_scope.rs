use crate::*;
use geo::algorithm::intersects::Intersects;
use geo::{Coord, LineString, Polygon};
use h3o::{CellIndex, Resolution};
use interlinked_engine::model::DemandCellAllocationDiagnostics;
use std::time::Instant;

fn perf_log(label: &str, started: Instant) {
    eprintln!("[perf] {label}: {}ms", started.elapsed().as_millis());
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

const UK_UNLOCK_COST_BASE: f64 = 12_000_000.0;
const UK_UNLOCK_COST_POP_SCALE: f64 = 40_000.0;
const UK_UNLOCK_COST_JOBS_SCALE: f64 = 28_000.0;
const UK_UNLOCK_COST_LAND_SCALE: f64 = 220_000.0;
const UK_UNLOCK_COST_ROUND_TO_BASE_UNITS: f64 = 1_000.0;

fn round_unlock_cost(value: f64) -> f64 {
    let unit = UK_UNLOCK_COST_ROUND_TO_BASE_UNITS.max(1.0);
    (value / unit).round() * unit
}

fn region_unlock_cost_base_from_signals(population: f64, jobs: f64, area_m2: f64) -> f64 {
    let pop_term = population.max(0.0).sqrt();
    let jobs_term = jobs.max(0.0).sqrt();
    let land_km2 = (area_m2.max(0.0) / 1_000_000.0).max(0.0);
    let land_term = land_km2.sqrt();

    let raw = UK_UNLOCK_COST_BASE
        + pop_term * UK_UNLOCK_COST_POP_SCALE
        + jobs_term * UK_UNLOCK_COST_JOBS_SCALE
        + land_term * UK_UNLOCK_COST_LAND_SCALE;
    round_unlock_cost(raw.max(0.0))
}

pub(crate) fn region_unlock_cost_base(
    region: &SurfaceRegionInfo,
    population: Option<u64>,
    jobs: Option<u64>,
) -> f64 {
    let population_signal = population
        .map(|value| value as f64)
        .unwrap_or_else(|| region.residents_smooth.max(0.0));
    let jobs_signal = jobs
        .map(|value| value as f64)
        .unwrap_or_else(|| region.jobs_smooth.max(0.0));
    region_unlock_cost_base_from_signals(population_signal, jobs_signal, region.area_m2)
}

pub(crate) fn region_unlock_cost_base_for_manifest(
    manifest: &ProjectManifest,
    region: &SurfaceRegionInfo,
    population: Option<u64>,
    jobs: Option<u64>,
) -> f64 {
    let profile = resolved_difficulty_profile(manifest);
    let base = region_unlock_cost_base(region, population, jobs);
    round_unlock_cost(base * profile.unlock_cost_mult.max(0.0))
}

pub(crate) fn country_employment_baseline_ratio(country_iso2: &str) -> f64 {
    if is_uk_country_iso2(country_iso2) {
        UK_EMPLOYMENT_BASELINE_RATIO
    } else {
        DEFAULT_EMPLOYMENT_BASELINE_RATIO
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

const CELL_ALLOC_WEIGHT_EPS: f64 = 1e-9;
const CELL_ALLOC_INVARIANT_EPS: f64 = 1e-6;

#[derive(Debug, Clone)]
struct RegionAllocationCellInput {
    persisted_cell_id: String,
    x_mercator_m: f64,
    y_mercator_m: f64,
    area_m2: f64,
    residents_signal: f64,
    jobs_signal: f64,
    activity_mix: [f64; 7],
    centrality_score: f64,
    data_quality_score: f64,
    signal_source: String,
}

#[derive(Debug, Clone)]
struct RegionAllocationCellOutput {
    persisted_cell_id: String,
    x_mercator_m: f64,
    y_mercator_m: f64,
    area_m2: f64,
    activity_mix: [f64; 7],
    centrality_score: f64,
    data_quality_score: f64,
    raw_weight_residential: f64,
    raw_weight_employment: f64,
    raw_weight_education: f64,
    raw_weight_retail: f64,
    raw_weight_leisure: f64,
    raw_weight_tourism: f64,
    allocated_residential_mass: f64,
    allocated_employment_mass: f64,
    fallback_reason: Option<String>,
    signal_source: String,
}

#[derive(Debug, Clone)]
struct RegionAllocationOutcome {
    cells: Vec<RegionAllocationCellOutput>,
    fallback_residential: bool,
    fallback_employment: bool,
}

#[derive(Debug, Clone)]
struct RegionAllocationAuditRow {
    region_id: String,
    cells: usize,
    target_population: f64,
    allocated_population: f64,
    target_jobs: f64,
    allocated_jobs: f64,
    fallback_residential: bool,
    fallback_employment: bool,
    top_residential_cells: Vec<String>,
    top_employment_cells: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct RegionMaterializationCoverageRow {
    region_id: String,
    active_region: bool,
    surface_source_kind: String,
    source_surface_candidates_total: usize,
    surface_candidates_total: usize,
    substrate_res6_members_total: usize,
    membership_res8_total: usize,
    surface_intersection_cells_added: usize,
    boundary_excluded_outside_geometry: usize,
    boundary_included_from_geometry: usize,
    boundary_excluded_invalid_geometry: usize,
    direct_signal_cells_total: usize,
    fallback_signal_cells_total: usize,
    dropped_by_active_scope: usize,
    dropped_by_per_region_cap: usize,
    dropped_invalid_surface_xy: usize,
    dropped_zero_filtered: usize,
    dropped_dedup_collapse: usize,
    fallback_cells_added: usize,
    allocation_inputs_total: usize,
    materialized_cells_total: usize,
    unique_raw_residential_weight_count: usize,
    unique_raw_employment_weight_count: usize,
    unique_allocated_residential_mass_count: usize,
    unique_allocated_employment_mass_count: usize,
    source_sample_cell_ids: Vec<String>,
    direct_signal_sample_cell_ids: Vec<String>,
    fallback_signal_sample_cell_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct CountryMaterializationCoverageAudit {
    country_iso2: String,
    unlocked_region_count: usize,
    active_region_count: usize,
    regions_missing_catalog_mapping: usize,
    regions_without_surface_cells: usize,
    source_candidates_total: usize,
    candidate_cells_total: usize,
    substrate_res6_members_total: usize,
    membership_res8_total: usize,
    surface_intersection_cells_added: usize,
    boundary_excluded_outside_geometry: usize,
    boundary_included_from_geometry: usize,
    boundary_excluded_invalid_geometry: usize,
    direct_signal_cells_total: usize,
    fallback_signal_cells_total: usize,
    dropped_by_active_scope: usize,
    dropped_by_per_region_cap: usize,
    dropped_invalid_surface_xy: usize,
    dropped_zero_filtered: usize,
    dropped_dedup_collapse: usize,
    global_duplicate_cells_merged: usize,
    fallback_cells_added: usize,
    allocation_inputs_total: usize,
    materialized_cells_total: usize,
}

#[derive(Debug, Clone, Default)]
struct RegionSurfaceLatticeExpansion {
    source_kind: String,
    cells: Vec<ExpandedRegionSurfaceCell>,
    substrate_res6_members_total: usize,
    membership_res8_total: usize,
    surface_intersection_cells_added: usize,
    boundary_excluded_outside_geometry: usize,
    boundary_included_from_geometry: usize,
    boundary_excluded_invalid_geometry: usize,
    direct_signal_cells_total: usize,
    fallback_signal_cells_total: usize,
    source_sample_cell_ids: Vec<String>,
    direct_signal_sample_cell_ids: Vec<String>,
    fallback_signal_sample_cell_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct ExpandedRegionSurfaceCell {
    surface_cell: DemandSurfaceCellWire,
    signal_source: String,
    used_fallback: bool,
}

fn parse_res6_cell_from_region_id(region_id: &str) -> Option<CellIndex> {
    let suffix = region_id.rsplit(':').next()?.trim();
    let parsed = suffix.parse::<CellIndex>().ok()?;
    if parsed.resolution() == Resolution::Six {
        Some(parsed)
    } else {
        None
    }
}

fn substrate_res6_members_for_region(
    catalog: &SurfaceRegionCatalog,
    region_id: &str,
) -> Vec<CellIndex> {
    let canonical_region_id =
        canonical_region_for_catalog(catalog, region_id).unwrap_or_else(|| region_id.to_string());
    let mut out = Vec::<CellIndex>::new();

    if let Some(cell) = parse_res6_cell_from_region_id(&canonical_region_id) {
        out.push(cell);
    }
    for (legacy_region_id, mapped_region_id) in &catalog.legacy_region_aliases {
        if mapped_region_id != &canonical_region_id {
            continue;
        }
        if let Some(cell) = parse_res6_cell_from_region_id(legacy_region_id) {
            out.push(cell);
        }
    }

    out.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    out.dedup();
    out
}

fn region_activity_mix(region: &SurfaceRegionInfo) -> [f64; 7] {
    normalize_activity_mix([
        region.activity_mix_residential,
        region.activity_mix_office,
        region.activity_mix_retail,
        region.activity_mix_recreation,
        region.activity_mix_industrial,
        region.activity_mix_education,
        region.activity_mix_health,
    ])
}

#[derive(Debug, Clone, Copy)]
struct LandusePointSignal {
    mix: [f64; 7],
    intensity: f64,
    coverage: f64,
}

fn sample_landuse_signal_at_point(
    profile: &CountyLanduseProfile,
    x_m: f64,
    y_m: f64,
) -> Option<LandusePointSignal> {
    if profile.samples.is_empty() {
        return None;
    }

    let radius_m = 1_050.0_f64;
    let sigma = 420.0_f64;
    let radius2 = radius_m * radius_m;
    let mut mix_sum = [0.0_f64; 7];
    let mut weight_sum = 0.0_f64;
    let mut intensity_sum = 0.0_f64;
    let mut selected = 0usize;
    for sample in &profile.samples {
        let dx = sample.x_m - x_m;
        let dy = sample.y_m - y_m;
        let d2 = dx * dx + dy * dy;
        if d2 > radius2 {
            continue;
        }
        let gaussian = (-d2 / (2.0 * sigma * sigma)).exp();
        let weight = gaussian * sample.weight.max(0.05) * sample.intensity.max(0.15);
        if weight <= 0.0 || !weight.is_finite() {
            continue;
        }
        selected += 1;
        weight_sum += weight;
        intensity_sum += sample.intensity.max(0.0) * weight;
        for (idx, value) in sample.mix.iter().enumerate() {
            mix_sum[idx] += value.max(0.0) * weight;
        }
    }

    if selected == 0 {
        let mut nearest = profile
            .samples
            .iter()
            .map(|sample| {
                let dx = sample.x_m - x_m;
                let dy = sample.y_m - y_m;
                let d2 = dx * dx + dy * dy;
                (sample, d2)
            })
            .collect::<Vec<_>>();
        nearest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (sample, d2) in nearest.into_iter().take(10) {
            let distance = d2.sqrt();
            let inv = (1.0 / (1.0 + distance / 300.0)).clamp(0.02, 1.0);
            let weight = inv * sample.weight.max(0.05) * sample.intensity.max(0.15);
            if weight <= 0.0 || !weight.is_finite() {
                continue;
            }
            selected += 1;
            weight_sum += weight;
            intensity_sum += sample.intensity.max(0.0) * weight;
            for (idx, value) in sample.mix.iter().enumerate() {
                mix_sum[idx] += value.max(0.0) * weight;
            }
        }
    }

    if selected == 0 || weight_sum <= CELL_ALLOC_WEIGHT_EPS {
        return None;
    }
    let mut mix = [0.0_f64; 7];
    for (idx, slot) in mix.iter_mut().enumerate() {
        *slot = mix_sum[idx] / weight_sum;
    }
    Some(LandusePointSignal {
        mix: normalize_activity_mix(mix),
        intensity: (intensity_sum / weight_sum).clamp(0.1, 4.0),
        coverage: (selected as f64 / 8.0).clamp(0.0, 1.0),
    })
}

fn h3_cell_from_region_geometry_token(token: &str) -> Option<CellIndex> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(cell) = trimmed.parse::<CellIndex>() {
        return Some(cell);
    }
    trimmed.rsplit(':').next()?.trim().parse::<CellIndex>().ok()
}

fn canonical_h3_cell_for_region_geometry(region: &SurfaceRegionInfo) -> Option<CellIndex> {
    [
        region.h3_cell_id.as_deref(),
        Some(region.region_id.as_str()),
        Some(region.cell_id.as_str()),
        Some(region.region_token.as_str()),
    ]
    .into_iter()
    .flatten()
    .find_map(h3_cell_from_region_geometry_token)
}

fn h3_cell_boundary_geojson_value(cell: CellIndex) -> Option<serde_json::Value> {
    let polygon = h3_cell_boundary_polygon(cell)?;
    let coords = polygon
        .exterior()
        .points()
        .map(|point| vec![point.x(), point.y()])
        .collect::<Vec<_>>();
    serde_json::to_value(geojson::Geometry::new(geojson::Value::Polygon(vec![
        coords,
    ])))
    .ok()
}

fn canonical_region_geometry_value(region: &SurfaceRegionInfo) -> Option<serde_json::Value> {
    region.geometry.clone().or_else(|| {
        canonical_h3_cell_for_region_geometry(region).and_then(h3_cell_boundary_geojson_value)
    })
}

fn region_geo_geometry(region: &SurfaceRegionInfo) -> Result<Option<geo::Geometry<f64>>, String> {
    let Some(geometry_value) = canonical_region_geometry_value(region) else {
        return Ok(None);
    };
    let geometry =
        if let Ok(geometry) = serde_json::from_value::<geojson::Geometry>(geometry_value.clone()) {
            geometry
        } else if let Ok(feature) = serde_json::from_value::<geojson::Feature>(geometry_value) {
            feature.geometry.ok_or_else(|| {
                format!(
                    "region {} geometry feature missing geometry",
                    region.region_id
                )
            })?
        } else {
            return Err(format!(
                "region {} geometry is not a parseable GeoJSON Geometry/Feature",
                region.region_id
            ));
        };

    let geo_geometry = geo::Geometry::try_from(&geometry.value).map_err(|error| {
        format!(
            "region {} geometry conversion failed: {error}",
            region.region_id
        )
    })?;
    Ok(Some(geo_geometry))
}

fn region_geometry_res8_members(
    region: &SurfaceRegionInfo,
) -> Result<Option<HashSet<CellIndex>>, String> {
    let Some(geo_geometry) = region_geo_geometry(region)? else {
        return Ok(None);
    };
    let h3_geometry = h3o::geom::Geometry::from_degrees(geo_geometry.clone()).map_err(|error| {
        format!(
            "region {} geometry normalization failed: {error}",
            region.region_id
        )
    })?;
    let config = h3o::geom::PolyfillConfig::new(Resolution::Eight)
        .containment_mode(h3o::geom::ContainmentMode::Covers);
    use h3o::geom::ToCells;
    let mut out = HashSet::<CellIndex>::new();
    for cell in h3_geometry.to_cells(config) {
        if cell.resolution() == Resolution::Eight {
            out.insert(cell);
        }
    }
    include_intersecting_boundary_neighbors(&geo_geometry, &mut out);
    Ok(Some(out))
}

fn res8_surface_cell_index(cell: &DemandSurfaceCellWire) -> Option<CellIndex> {
    let trimmed = cell.cell_id.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.parse::<CellIndex>().is_ok() {
        trimmed.as_str()
    } else {
        trimmed.rsplit(':').next()?.trim()
    };
    let parsed = candidate.parse::<CellIndex>().ok()?;
    (parsed.resolution() == Resolution::Eight).then_some(parsed)
}

fn collect_country_surface_cells_by_res8(
    catalog: &SurfaceRegionCatalog,
) -> HashMap<CellIndex, DemandSurfaceCellWire> {
    let mut by_cell = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
    for cells in catalog.cells_res8_by_region.values() {
        for cell in cells {
            if let Some(index) = res8_surface_cell_index(cell) {
                by_cell.entry(index).or_insert_with(|| cell.clone());
            }
        }
    }
    by_cell
}

fn include_intersecting_country_surface_cells(
    geometry: &geo::Geometry<f64>,
    cells: &mut HashSet<CellIndex>,
    country_surface_cells: &HashMap<CellIndex, DemandSurfaceCellWire>,
) -> usize {
    let mut added = 0usize;
    for cell in country_surface_cells.keys().copied() {
        if cells.contains(&cell) || cell.resolution() != Resolution::Eight {
            continue;
        }
        if h3_cell_intersects_geometry(cell, geometry) {
            cells.insert(cell);
            added += 1;
        }
    }
    added
}

fn include_intersecting_candidate_cells<I>(
    geometry: &geo::Geometry<f64>,
    cells: &mut HashSet<CellIndex>,
    candidates: I,
) -> usize
where
    I: IntoIterator<Item = CellIndex>,
{
    let mut added = 0usize;
    for cell in candidates {
        if cells.contains(&cell) || cell.resolution() != Resolution::Eight {
            continue;
        }
        if h3_cell_intersects_geometry(cell, geometry) {
            cells.insert(cell);
            added += 1;
        }
    }
    added
}

fn h3_cell_boundary_polygon(cell: CellIndex) -> Option<Polygon<f64>> {
    let boundary = cell.boundary();
    let mut coords = boundary
        .iter()
        .map(|point| Coord {
            x: point.lng(),
            y: point.lat(),
        })
        .collect::<Vec<_>>();
    let first = coords.first().copied()?;
    if coords.last().copied() != Some(first) {
        coords.push(first);
    }
    if coords.len() < 4 {
        return None;
    }
    Some(Polygon::new(LineString::from(coords), vec![]))
}

fn h3_cell_intersects_geometry(cell: CellIndex, geometry: &geo::Geometry<f64>) -> bool {
    h3_cell_boundary_polygon(cell)
        .map(|polygon| geometry.intersects(&polygon))
        .unwrap_or(false)
}

fn include_intersecting_boundary_neighbors(
    geometry: &geo::Geometry<f64>,
    cells: &mut HashSet<CellIndex>,
) -> usize {
    let mut frontier = cells.iter().copied().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut added = 0usize;
    while cursor < frontier.len() {
        let seed = frontier[cursor];
        cursor += 1;
        let candidates = seed.grid_disk::<Vec<_>>(1);
        for candidate in candidates {
            if cells.contains(&candidate) || candidate.resolution() != Resolution::Eight {
                continue;
            }
            if h3_cell_intersects_geometry(candidate, geometry) {
                cells.insert(candidate);
                frontier.push(candidate);
                added += 1;
            }
        }
    }
    added
}

fn derive_surface_cell_for_membership(
    app: Option<&AppHandle>,
    iso: &str,
    region: &SurfaceRegionInfo,
    cell: CellIndex,
    county_catalog: Option<&CountyBoundaryCatalog>,
) -> ExpandedRegionSurfaceCell {
    let center: h3o::LatLng = cell.into();
    let lon = center.lng();
    let lat = center.lat();
    let (x, y) = lonlat_to_web_mercator_m(lon, lat);
    let area_m2 = cell.area_m2().max(0.0);
    let mut mix = region_activity_mix(region);
    let mut signal_source = "direct_region_geometry".to_string();
    let mut used_fallback = false;

    let radius_m = (region.area_m2.max(1.0) / std::f64::consts::PI)
        .sqrt()
        .max(700.0);
    let distance = ((x - region.x).powi(2) + (y - region.y).powi(2)).sqrt();
    let distance_ratio = (distance / (radius_m * 1.25)).clamp(0.0, 2.0);
    let centrality = (1.0 - 0.72 * distance_ratio).clamp(0.05, 1.0);

    let mut quality = (0.45 + 0.35 * centrality).clamp(0.15, 1.0);
    let mut intensity = 0.95_f64;

    if let (Some(app), Some(county_catalog)) = (app, county_catalog) {
        if let Some(county) = county_for_lon_lat(&county_catalog.counties, lon, lat)
            .or_else(|| nearest_county_for_lon_lat(&county_catalog.counties, lon, lat))
        {
            if let Ok(profile) = load_county_landuse_profile(app, iso, &county.county_id) {
                if let Some(landuse) = sample_landuse_signal_at_point(&profile, x, y) {
                    let blend = (0.35 + 0.55 * landuse.coverage).clamp(0.35, 0.9);
                    for idx in 0..mix.len() {
                        mix[idx] = mix[idx] * (1.0 - blend) + landuse.mix[idx] * blend;
                    }
                    mix = normalize_activity_mix(mix);
                    intensity = landuse.intensity;
                    quality = (0.28 + 0.27 * centrality + 0.45 * landuse.coverage).clamp(0.1, 1.0);
                    signal_source = "direct_landuse_profile".to_string();
                }
            }
        }
    }

    let jobs_pull = mix[1] * 1.00
        + mix[2] * 0.90
        + mix[4] * 0.95
        + mix[5] * 0.65
        + mix[6] * 0.60
        + mix[3] * 0.35
        + mix[0] * 0.15;
    let residents_pull = mix[0] * 1.05
        + mix[3] * 0.28
        + mix[2] * 0.20
        + mix[1] * 0.08
        + mix[4] * 0.05
        + mix[5] * 0.05
        + mix[6] * 0.05;
    let area_scale = (area_m2.sqrt() / 300.0).clamp(0.08, 12.0);
    let intensity_scale = intensity.clamp(0.2, 2.5);
    let mut residents = area_scale
        * residents_pull.max(0.02)
        * (0.58 + 0.42 * (1.0 - centrality))
        * intensity_scale;
    let mut jobs = area_scale * jobs_pull.max(0.02) * (0.42 + 0.58 * centrality) * intensity_scale;

    if !residents.is_finite() || residents < 0.0 {
        residents = 0.0;
    }
    if !jobs.is_finite() || jobs < 0.0 {
        jobs = 0.0;
    }
    if residents <= CELL_ALLOC_WEIGHT_EPS && jobs <= CELL_ALLOC_WEIGHT_EPS {
        used_fallback = true;
        signal_source = "fallback_region_defaults".to_string();
        residents = area_scale * (mix[0] * 0.75 + 0.08).max(0.01);
        jobs = area_scale * (mix[1] * 0.70 + mix[4] * 0.40 + 0.06).max(0.01);
        quality = quality.max(0.2);
    }

    ExpandedRegionSurfaceCell {
        surface_cell: DemandSurfaceCellWire {
            cell_id: cell.to_string().to_ascii_lowercase(),
            h3_res: 8,
            lon,
            lat,
            x,
            y,
            area_m2,
            country_iso2: iso.to_string(),
            residents_raw: residents,
            jobs_raw: jobs,
            residents_smooth: residents,
            jobs_smooth: jobs,
            activity_mix_residential: mix[0],
            activity_mix_office: mix[1],
            activity_mix_retail: mix[2],
            activity_mix_recreation: mix[3],
            activity_mix_industrial: mix[4],
            activity_mix_education: mix[5],
            activity_mix_health: mix[6],
            quality,
        },
        signal_source,
        used_fallback,
    }
}

fn expand_surface_cells_to_region_res8_lattice(
    app: Option<&AppHandle>,
    iso: &str,
    catalog: &SurfaceRegionCatalog,
    region: &SurfaceRegionInfo,
    source_surface_cells: &[DemandSurfaceCellWire],
    country_surface_cells_by_h3: &HashMap<CellIndex, DemandSurfaceCellWire>,
    county_catalog: Option<&CountyBoundaryCatalog>,
) -> RegionSurfaceLatticeExpansion {
    let substrate_members = substrate_res6_members_for_region(catalog, &region.region_id);
    let mut substrate_children = HashSet::<CellIndex>::new();
    for parent in &substrate_members {
        for child in parent.children(Resolution::Eight) {
            substrate_children.insert(child);
        }
    }
    let substrate_children_for_intersection = substrate_children.clone();

    let source_sample_cell_ids = source_surface_cells
        .iter()
        .map(|cell| cell.cell_id.clone())
        .take(3)
        .collect::<Vec<_>>();

    let mut source_kind = "source_res8_cells_fallback".to_string();
    let mut boundary_excluded_invalid_geometry = 0usize;
    let mut boundary_excluded_outside_geometry = 0usize;
    let mut boundary_included_from_geometry = 0usize;
    let mut surface_intersection_cells_added = 0usize;
    let mut membership_cells = match region_geometry_res8_members(region) {
        Ok(Some(geometry_members)) if !geometry_members.is_empty() => {
            source_kind = "region_geometry_polyfill_res8_intersects".to_string();
            if !substrate_children.is_empty() {
                boundary_excluded_outside_geometry =
                    substrate_children.difference(&geometry_members).count();
                boundary_included_from_geometry =
                    geometry_members.difference(&substrate_children).count();
            }
            geometry_members
        }
        Ok(Some(_)) => {
            source_kind = "substrate_res6_children_res8".to_string();
            substrate_children
        }
        Ok(None) => {
            if substrate_children.is_empty() {
                HashSet::<CellIndex>::new()
            } else {
                source_kind = "substrate_res6_children_res8".to_string();
                substrate_children
            }
        }
        Err(error) => {
            eprintln!(
                "[demand-materialization] region={} geometry_polyfill_error={}",
                region.region_id, error
            );
            source_kind = "substrate_res6_children_res8".to_string();
            boundary_excluded_invalid_geometry = substrate_children.len();
            substrate_children
        }
    };
    match region_geo_geometry(region) {
        Ok(Some(geometry)) => {
            let substrate_intersection_cells_added = include_intersecting_candidate_cells(
                &geometry,
                &mut membership_cells,
                substrate_children_for_intersection.iter().copied(),
            );
            if substrate_intersection_cells_added > 0 {
                source_kind.push_str("+substrate_intersections");
            }
            if !membership_cells.is_empty() {
                surface_intersection_cells_added = include_intersecting_country_surface_cells(
                    &geometry,
                    &mut membership_cells,
                    country_surface_cells_by_h3,
                );
                if surface_intersection_cells_added > 0 {
                    source_kind.push_str("+country_surface_intersections");
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "[demand-materialization] region={} surface_intersection_geometry_error={}",
                region.region_id, error
            );
        }
    }

    let mut cells = Vec::<ExpandedRegionSurfaceCell>::new();
    let mut direct_signal_sample_cell_ids = Vec::<String>::new();
    let mut fallback_signal_sample_cell_ids = Vec::<String>::new();
    let mut direct_signal_cells_total = 0usize;
    let mut fallback_signal_cells_total = 0usize;

    if membership_cells.is_empty() {
        for source in source_surface_cells {
            let fallback = !source.x.is_finite() || !source.y.is_finite();
            let signal_source = if fallback {
                "fallback_region_defaults".to_string()
            } else {
                "direct_source_cell".to_string()
            };
            if fallback {
                fallback_signal_cells_total += 1;
                if fallback_signal_sample_cell_ids.len() < 3 {
                    fallback_signal_sample_cell_ids.push(source.cell_id.clone());
                }
            } else {
                direct_signal_cells_total += 1;
                if direct_signal_sample_cell_ids.len() < 3 {
                    direct_signal_sample_cell_ids.push(source.cell_id.clone());
                }
            }
            cells.push(ExpandedRegionSurfaceCell {
                surface_cell: source.clone(),
                signal_source,
                used_fallback: fallback,
            });
        }
    } else {
        let mut ordered_members = membership_cells.into_iter().collect::<Vec<_>>();
        ordered_members.sort_by(|left, right| left.to_string().cmp(&right.to_string()));
        for cell in ordered_members {
            let expanded = country_surface_cells_by_h3
                .get(&cell)
                .map(|source| {
                    let fallback = !source.x.is_finite() || !source.y.is_finite();
                    ExpandedRegionSurfaceCell {
                        surface_cell: source.clone(),
                        signal_source: if fallback {
                            "fallback_region_defaults".to_string()
                        } else {
                            "direct_country_surface_intersection".to_string()
                        },
                        used_fallback: fallback,
                    }
                })
                .unwrap_or_else(|| {
                    derive_surface_cell_for_membership(app, iso, region, cell, county_catalog)
                });
            let cell_id = expanded.surface_cell.cell_id.clone();
            if expanded.used_fallback {
                fallback_signal_cells_total += 1;
                if fallback_signal_sample_cell_ids.len() < 3 {
                    fallback_signal_sample_cell_ids.push(cell_id);
                }
            } else {
                direct_signal_cells_total += 1;
                if direct_signal_sample_cell_ids.len() < 3 {
                    direct_signal_sample_cell_ids.push(cell_id);
                }
            }
            cells.push(expanded);
        }
    }

    RegionSurfaceLatticeExpansion {
        source_kind,
        membership_res8_total: cells.len(),
        cells,
        substrate_res6_members_total: substrate_members.len(),
        surface_intersection_cells_added,
        boundary_excluded_outside_geometry,
        boundary_included_from_geometry,
        boundary_excluded_invalid_geometry,
        direct_signal_cells_total,
        fallback_signal_cells_total,
        source_sample_cell_ids,
        direct_signal_sample_cell_ids,
        fallback_signal_sample_cell_ids,
    }
}

fn build_region_allocation_inputs_from_surface_cells(
    iso: &str,
    region: &SurfaceRegionInfo,
    surface_cells: &[ExpandedRegionSurfaceCell],
    active_region: bool,
) -> (
    Vec<RegionAllocationCellInput>,
    RegionMaterializationCoverageRow,
) {
    let mut coverage = RegionMaterializationCoverageRow {
        region_id: region.region_id.clone(),
        active_region,
        surface_candidates_total: surface_cells.len(),
        ..RegionMaterializationCoverageRow::default()
    };

    let mut allocation_inputs =
        Vec::<RegionAllocationCellInput>::with_capacity(surface_cells.len());
    for expanded in surface_cells {
        let cell = &expanded.surface_cell;
        // Persisted demand materialization keeps full unlocked-surface coverage.
        // Active-region scope/caps are runtime concerns and must not prune authoritative substrate cells.
        if !cell.x.is_finite() || !cell.y.is_finite() {
            coverage.dropped_invalid_surface_xy += 1;
            continue;
        }
        allocation_inputs.push(RegionAllocationCellInput {
            persisted_cell_id: format!("ds:v4:{}:{}", iso, cell.cell_id),
            x_mercator_m: cell.x,
            y_mercator_m: cell.y,
            area_m2: cell.area_m2.max(0.0),
            residents_signal: cell.residents_smooth.max(0.0),
            jobs_signal: cell.jobs_smooth.max(0.0),
            activity_mix: [
                cell.activity_mix_residential,
                cell.activity_mix_office,
                cell.activity_mix_retail,
                cell.activity_mix_recreation,
                cell.activity_mix_industrial,
                cell.activity_mix_education,
                cell.activity_mix_health,
            ],
            centrality_score: cell.quality.clamp(0.0, 1.0),
            data_quality_score: cell.quality.clamp(0.0, 1.0),
            signal_source: expanded.signal_source.clone(),
        });
    }

    if allocation_inputs.is_empty() {
        coverage.fallback_cells_added = 1;
        allocation_inputs.push(RegionAllocationCellInput {
            persisted_cell_id: format!("ds:v4m:{}:{}", iso, region.cell_id),
            x_mercator_m: region.x,
            y_mercator_m: region.y,
            area_m2: region.area_m2.max(0.0),
            residents_signal: region.residents_smooth.max(0.0),
            jobs_signal: region.jobs_smooth.max(0.0),
            activity_mix: [
                region.activity_mix_residential,
                region.activity_mix_office,
                region.activity_mix_retail,
                region.activity_mix_recreation,
                region.activity_mix_industrial,
                region.activity_mix_education,
                region.activity_mix_health,
            ],
            centrality_score: 0.45,
            data_quality_score: 0.65,
            signal_source: "fallback_region_defaults".to_string(),
        });
    }

    coverage.allocation_inputs_total = allocation_inputs.len();
    (allocation_inputs, coverage)
}

fn region_macro_totals_by_id(
    country_iso2: &str,
    regions: &[SurfaceRegionInfo],
) -> (
    std::collections::HashMap<String, f64>,
    std::collections::HashMap<String, f64>,
) {
    let calibrated_population = calibrated_region_population_for_country(country_iso2, regions);
    let calibrated_jobs = calibrated_region_jobs_for_country(country_iso2, regions);

    let mut population_by_region = std::collections::HashMap::<String, f64>::new();
    let mut jobs_by_region = std::collections::HashMap::<String, f64>::new();
    for region in regions {
        let population = calibrated_population
            .get(&region.region_id)
            .copied()
            .map(|value| value as f64)
            .unwrap_or_else(|| region.residents_smooth.max(0.0));
        let jobs = calibrated_jobs
            .get(&region.region_id)
            .copied()
            .map(|value| value as f64)
            .unwrap_or_else(|| region.jobs_smooth.max(0.0));
        population_by_region.insert(region.region_id.clone(), population.max(0.0));
        jobs_by_region.insert(region.region_id.clone(), jobs.max(0.0));
    }
    (population_by_region, jobs_by_region)
}

fn proportional_allocate(total: f64, weights: &[f64]) -> (Vec<f64>, bool) {
    if weights.is_empty() {
        return (Vec::new(), false);
    }
    if !total.is_finite() || total <= 0.0 {
        return (vec![0.0; weights.len()], false);
    }

    let cleaned = weights
        .iter()
        .map(|weight| {
            if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let weight_sum = cleaned.iter().sum::<f64>();
    if weight_sum > CELL_ALLOC_WEIGHT_EPS {
        let mut allocated = cleaned
            .iter()
            .map(|weight| total * (*weight / weight_sum))
            .collect::<Vec<_>>();
        let allocated_sum = allocated.iter().sum::<f64>();
        let residual = total - allocated_sum;
        if residual.abs() > CELL_ALLOC_WEIGHT_EPS {
            let mut max_idx = 0usize;
            let mut max_value = f64::NEG_INFINITY;
            for (idx, value) in allocated.iter().enumerate() {
                if *value > max_value {
                    max_value = *value;
                    max_idx = idx;
                }
            }
            allocated[max_idx] = (allocated[max_idx] + residual).max(0.0);
        }
        return (allocated, false);
    }

    let equal_share = total / (weights.len() as f64);
    let mut allocated = vec![equal_share; weights.len()];
    let allocated_sum = allocated.iter().sum::<f64>();
    let residual = total - allocated_sum;
    if residual.abs() > CELL_ALLOC_WEIGHT_EPS {
        allocated[0] = (allocated[0] + residual).max(0.0);
    }
    (allocated, true)
}

fn unique_nonnegative_value_count<I>(values: I) -> usize
where
    I: IntoIterator<Item = f64>,
{
    let mut unique = HashSet::<i64>::new();
    for value in values {
        if !value.is_finite() {
            continue;
        }
        let quantized = (value.max(0.0) * 1_000_000.0).round() as i64;
        unique.insert(quantized);
    }
    unique.len()
}

fn allocate_region_cell_masses(
    target_population: f64,
    target_jobs: f64,
    inputs: Vec<RegionAllocationCellInput>,
) -> RegionAllocationOutcome {
    if inputs.is_empty() {
        return RegionAllocationOutcome {
            cells: Vec::new(),
            fallback_residential: false,
            fallback_employment: false,
        };
    }

    let max_residents_signal = inputs
        .iter()
        .map(|cell| cell.residents_signal.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_jobs_signal = inputs
        .iter()
        .map(|cell| cell.jobs_signal.max(0.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let max_area_m2 = inputs
        .iter()
        .map(|cell| cell.area_m2.max(1.0))
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut outputs = inputs
        .into_iter()
        .map(|cell| {
            let raw_mix_sum = cell
                .activity_mix
                .iter()
                .map(|value| {
                    if value.is_finite() && *value > 0.0 {
                        *value
                    } else {
                        0.0
                    }
                })
                .sum::<f64>();
            let mix_is_degenerate = raw_mix_sum <= CELL_ALLOC_WEIGHT_EPS;
            let normalized_mix = normalize_activity_mix(cell.activity_mix);
            let area_norm = ((cell.area_m2.max(1.0) / max_area_m2).sqrt()).clamp(0.2, 1.0);
            let residents_signal_norm =
                (cell.residents_signal.max(0.0) / max_residents_signal).clamp(0.0, 1.0);
            let jobs_signal_norm = (cell.jobs_signal.max(0.0) / max_jobs_signal).clamp(0.0, 1.0);
            let centrality = cell.centrality_score.clamp(0.0, 1.0);
            let quality = cell.data_quality_score.clamp(0.0, 1.0);

            // Residential fit prioritizes home-oriented land use and existing resident signal.
            let raw_weight_residential = if mix_is_degenerate {
                0.0
            } else {
                (area_norm
                    * (0.50 * normalized_mix[0]
                        + 0.18 * residents_signal_norm
                        + 0.08 * normalized_mix[3]
                        + 0.08 * normalized_mix[2]
                        + 0.06 * normalized_mix[5]
                        + 0.05 * normalized_mix[6]
                        + 0.05 * (1.0 - normalized_mix[4]).clamp(0.0, 1.0))
                    * (0.80 + 0.20 * (1.0 - centrality))
                    * (0.85 + 0.15 * quality))
                    .max(0.0)
            };

            // Employment fit prioritizes office/industrial/commercial intensity and job signal.
            let raw_weight_employment = if mix_is_degenerate {
                0.0
            } else {
                (area_norm
                    * (0.34 * normalized_mix[1]
                        + 0.22 * normalized_mix[4]
                        + 0.16 * normalized_mix[2]
                        + 0.10 * normalized_mix[5]
                        + 0.08 * normalized_mix[6]
                        + 0.06 * jobs_signal_norm
                        + 0.04 * normalized_mix[3])
                    * (0.70 + 0.30 * centrality)
                    * (0.75 + 0.25 * quality))
                    .max(0.0)
            };

            let raw_weight_education = if mix_is_degenerate {
                0.0
            } else {
                (area_norm
                    * (0.62 * normalized_mix[5]
                        + 0.18 * normalized_mix[0]
                        + 0.10 * quality
                        + 0.10 * centrality))
                    .max(0.0)
            };
            let raw_weight_retail = if mix_is_degenerate {
                0.0
            } else {
                (area_norm
                    * (0.56 * normalized_mix[2]
                        + 0.16 * normalized_mix[1]
                        + 0.14 * normalized_mix[3]
                        + 0.14 * centrality))
                    .max(0.0)
            };
            let raw_weight_leisure = if mix_is_degenerate {
                0.0
            } else {
                (area_norm
                    * (0.58 * normalized_mix[3]
                        + 0.20 * normalized_mix[2]
                        + 0.12 * quality
                        + 0.10 * centrality))
                    .max(0.0)
            };
            let raw_weight_tourism =
                (0.52 * raw_weight_leisure + 0.33 * raw_weight_retail + 0.15 * quality).max(0.0);

            RegionAllocationCellOutput {
                persisted_cell_id: cell.persisted_cell_id,
                x_mercator_m: cell.x_mercator_m,
                y_mercator_m: cell.y_mercator_m,
                area_m2: cell.area_m2.max(0.0),
                activity_mix: normalized_mix,
                centrality_score: centrality,
                data_quality_score: quality,
                raw_weight_residential,
                raw_weight_employment,
                raw_weight_education,
                raw_weight_retail,
                raw_weight_leisure,
                raw_weight_tourism,
                allocated_residential_mass: 0.0,
                allocated_employment_mass: 0.0,
                fallback_reason: None,
                signal_source: cell.signal_source,
            }
        })
        .collect::<Vec<_>>();

    let residential_weights = outputs
        .iter()
        .map(|cell| cell.raw_weight_residential)
        .collect::<Vec<_>>();
    let employment_weights = outputs
        .iter()
        .map(|cell| cell.raw_weight_employment)
        .collect::<Vec<_>>();
    let (allocated_population, fallback_residential) =
        proportional_allocate(target_population.max(0.0), &residential_weights);
    let (allocated_jobs, fallback_employment) =
        proportional_allocate(target_jobs.max(0.0), &employment_weights);

    let fallback_reason = match (fallback_residential, fallback_employment) {
        (true, true) => Some("degenerate_residential_and_employment_weights".to_string()),
        (true, false) => Some("degenerate_residential_weights".to_string()),
        (false, true) => Some("degenerate_employment_weights".to_string()),
        (false, false) => None,
    };

    for (idx, cell) in outputs.iter_mut().enumerate() {
        cell.allocated_residential_mass = allocated_population.get(idx).copied().unwrap_or(0.0);
        cell.allocated_employment_mass = allocated_jobs.get(idx).copied().unwrap_or(0.0);
        cell.fallback_reason = fallback_reason.clone();
    }

    RegionAllocationOutcome {
        cells: outputs,
        fallback_residential,
        fallback_employment,
    }
}

fn build_region_allocation_audit_row(
    region_id: &str,
    target_population: f64,
    target_jobs: f64,
    outcome: &RegionAllocationOutcome,
) -> RegionAllocationAuditRow {
    let allocated_population = outcome
        .cells
        .iter()
        .map(|cell| cell.allocated_residential_mass.max(0.0))
        .sum::<f64>();
    let allocated_jobs = outcome
        .cells
        .iter()
        .map(|cell| cell.allocated_employment_mass.max(0.0))
        .sum::<f64>();

    let mut top_residential = outcome
        .cells
        .iter()
        .map(|cell| {
            (
                format!("{}@{}", cell.persisted_cell_id, cell.signal_source),
                cell.raw_weight_residential,
            )
        })
        .collect::<Vec<_>>();
    top_residential.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    top_residential.truncate(3);

    let mut top_employment = outcome
        .cells
        .iter()
        .map(|cell| {
            (
                format!("{}@{}", cell.persisted_cell_id, cell.signal_source),
                cell.raw_weight_employment,
            )
        })
        .collect::<Vec<_>>();
    top_employment.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    top_employment.truncate(3);

    RegionAllocationAuditRow {
        region_id: region_id.to_string(),
        cells: outcome.cells.len(),
        target_population,
        allocated_population,
        target_jobs,
        allocated_jobs,
        fallback_residential: outcome.fallback_residential,
        fallback_employment: outcome.fallback_employment,
        top_residential_cells: top_residential.into_iter().map(|row| row.0).collect(),
        top_employment_cells: top_employment.into_iter().map(|row| row.0).collect(),
    }
}

fn verify_region_allocation_invariants(row: &RegionAllocationAuditRow) -> Result<(), String> {
    let pop_tolerance = row.target_population.abs().max(1.0) * CELL_ALLOC_INVARIANT_EPS;
    let jobs_tolerance = row.target_jobs.abs().max(1.0) * CELL_ALLOC_INVARIANT_EPS;
    let pop_diff = (row.target_population - row.allocated_population).abs();
    let jobs_diff = (row.target_jobs - row.allocated_jobs).abs();

    if pop_diff > pop_tolerance {
        return Err(format!(
            "region {} population allocation mismatch: target={:.6} allocated={:.6} diff={:.6} tol={:.6}",
            row.region_id, row.target_population, row.allocated_population, pop_diff, pop_tolerance
        ));
    }
    if jobs_diff > jobs_tolerance {
        return Err(format!(
            "region {} jobs allocation mismatch: target={:.6} allocated={:.6} diff={:.6} tol={:.6}",
            row.region_id, row.target_jobs, row.allocated_jobs, jobs_diff, jobs_tolerance
        ));
    }
    Ok(())
}

pub(crate) fn sync_country_region_state(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let resolved = sync_country_region_state_with_overrides(
        manifest,
        catalog,
        country_iso2,
        RegionStateOverrides::default(),
    )?;
    Ok((resolved.unlocked_region_ids, resolved.active_region_ids))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RegionStateOverrides {
    pub(crate) ensure_unlocked_region_ids: Vec<String>,
    pub(crate) force_primary_focus_region_id: Option<String>,
    pub(crate) force_active_region_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct CountryRegionState {
    pub(crate) country_iso2: String,
    pub(crate) unlocked_region_ids: Vec<String>,
    pub(crate) primary_focus_region_id: String,
    pub(crate) active_region_ids: Vec<String>,
}

pub(crate) fn sync_country_region_state_with_overrides(
    manifest: &mut ProjectManifest,
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
    overrides: RegionStateOverrides,
) -> Result<CountryRegionState, String> {
    // Authoritative country-level region state reconciliation:
    // canonicalize -> validate against catalog -> apply overrides -> persist merged state.
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let valid = catalog
        .regions
        .iter()
        .map(|r| r.region_id.clone())
        .collect::<HashSet<_>>();
    let resolve_for_catalog = |id: &str| canonical_region_for_catalog(catalog, id);
    let mut unlocked_for_country = manifest
        .region_state
        .unlocked_region_ids
        .iter()
        .filter_map(|id| resolve_for_catalog(id))
        .filter(|id| region_country_iso2(id).as_deref() == Some(iso.as_str()))
        .filter(|id| valid.contains(id))
        .collect::<BTreeSet<_>>();

    for region_id in overrides
        .ensure_unlocked_region_ids
        .iter()
        .filter_map(|id| resolve_for_catalog(id))
    {
        if region_country_iso2(&region_id).as_deref() == Some(iso.as_str())
            && valid.contains(&region_id)
        {
            unlocked_for_country.insert(region_id);
        }
    }

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

    let mut primary = overrides
        .force_primary_focus_region_id
        .as_deref()
        .and_then(resolve_for_catalog)
        .filter(|rid| unlocked_for_country.contains(rid))
        .or_else(|| {
            manifest
                .region_state
                .primary_focus_region_id
                .as_deref()
                .and_then(resolve_for_catalog)
                .filter(|rid| unlocked_for_country.contains(rid))
        });
    if primary.is_none() {
        primary = unlocked_for_country.iter().next().cloned();
    }
    let primary = primary.ok_or_else(|| format!("failed to select primary region for {iso}"))?;

    let unlocked_set = unlocked_for_country.iter().cloned().collect::<HashSet<_>>();
    let mut active_for_country = overrides
        .force_active_region_ids
        .as_ref()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| resolve_for_catalog(id))
                .filter(|id| unlocked_set.contains(id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            manifest
                .region_state
                .active_region_ids
                .iter()
                .filter_map(|id| resolve_for_catalog(id))
                .filter(|id| unlocked_set.contains(id))
                .collect::<Vec<_>>()
        });
    if active_for_country.is_empty() || !active_for_country.contains(&primary) {
        active_for_country = default_active_regions_for_focus(catalog, &primary, &unlocked_set);
    }
    active_for_country = active_for_country
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
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
    manifest.region_state.primary_focus_region_id = Some(primary.clone());

    Ok(CountryRegionState {
        country_iso2: iso,
        unlocked_region_ids: unlocked_for_country.into_iter().collect(),
        primary_focus_region_id: primary,
        active_region_ids: active_for_country,
    })
}

pub(crate) fn materialize_country_surface_scoped(
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let catalog = build_region_catalog_for_surface(&iso, surface)?;
    materialize_country_surface_scoped_with_catalog(None, manifest, scenario, &iso, &catalog)
}

pub(crate) fn materialize_country_surface_scoped_with_app(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let catalog = if let Some(cached_or_built) = load_region_catalog_for_country(app, &iso)? {
        eprintln!(
            "[perf] materialize_country_surface_scoped_with_app.catalog_source iso={} source=load_region_catalog_for_country",
            iso
        );
        cached_or_built
    } else {
        // Keep legacy fallback semantics if no resolved catalog source exists.
        eprintln!(
            "[perf] materialize_country_surface_scoped_with_app.catalog_source iso={} source=direct_surface_build_fallback",
            iso
        );
        build_region_catalog_for_surface_with_app(app, &iso, surface)?
    };
    materialize_country_surface_scoped_with_catalog(Some(app), manifest, scenario, &iso, &catalog)
}

fn merge_allocation_diagnostics(
    existing: &mut Option<DemandCellAllocationDiagnostics>,
    incoming: Option<DemandCellAllocationDiagnostics>,
) {
    let Some(incoming) = incoming else {
        return;
    };
    let Some(existing_diag) = existing.as_mut() else {
        *existing = Some(incoming);
        return;
    };

    if existing_diag.planning_region_id != incoming.planning_region_id {
        existing_diag.planning_region_id = None;
        existing_diag.fallback_reason = Some("merged_overlapping_region_cells".to_string());
    }
    if existing_diag.fallback_reason.is_none() {
        existing_diag.fallback_reason = incoming.fallback_reason;
    }
}

fn append_or_merge_materialized_demand_cell(
    scenario: &mut Scenario,
    demand_index_by_id: &mut HashMap<String, usize>,
    zone_index_by_id: &mut HashMap<String, usize>,
    incoming_cell: DemandCell,
    incoming_zone: Zone,
) -> bool {
    let key = incoming_cell.cell_id.trim().to_ascii_lowercase();
    if key.is_empty() {
        return false;
    }

    if let Some(existing_index) = demand_index_by_id.get(&key).copied() {
        if let Some(existing) = scenario.world.demand_cells.get_mut(existing_index) {
            existing.area_m2 = existing.area_m2.max(incoming_cell.area_m2).max(0.0);
            existing.data_quality_score = existing
                .data_quality_score
                .max(incoming_cell.data_quality_score)
                .max(0.0);
            if existing.country_iso2.is_none() {
                existing.country_iso2 = incoming_cell.country_iso2.clone();
            }
            merge_allocation_diagnostics(
                &mut existing.allocation_diagnostics,
                incoming_cell.allocation_diagnostics,
            );
        }
        if let Some(zone_index) = zone_index_by_id.get(&key).copied() {
            if let Some(existing_zone) = scenario.world.zones.get_mut(zone_index) {
                if existing_zone.country_iso2.is_none() {
                    existing_zone.country_iso2 = incoming_zone.country_iso2;
                }
            }
        }
        return false;
    }

    let demand_index = scenario.world.demand_cells.len();
    scenario.world.demand_cells.push(incoming_cell);
    demand_index_by_id.insert(key.clone(), demand_index);

    if !zone_index_by_id.contains_key(&key) {
        let zone_index = scenario.world.zones.len();
        scenario.world.zones.push(incoming_zone);
        zone_index_by_id.insert(key, zone_index);
    }
    true
}

fn materialize_country_surface_scoped_with_catalog(
    app: Option<&AppHandle>,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
    catalog: &SurfaceRegionCatalog,
) -> Result<usize, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let (unlocked_regions, active_regions) = sync_country_region_state(manifest, &catalog, &iso)?;
    let active_set = active_regions.into_iter().collect::<HashSet<_>>();
    let (population_by_region, jobs_by_region) = region_macro_totals_by_id(&iso, &catalog.regions);

    let mut loaded_cells = 0usize;
    let mut materialized_cell_index_by_id = scenario
        .world
        .demand_cells
        .iter()
        .enumerate()
        .map(|(index, cell)| (cell.cell_id.trim().to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    let mut materialized_zone_index_by_id = scenario
        .world
        .zones
        .iter()
        .enumerate()
        .map(|(index, zone)| (zone.id.trim().to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    let mut allocation_audit_rows = Vec::<RegionAllocationAuditRow>::new();
    let mut coverage_rows = Vec::<RegionMaterializationCoverageRow>::new();
    let mut coverage_audit = CountryMaterializationCoverageAudit {
        country_iso2: iso.clone(),
        unlocked_region_count: unlocked_regions.len(),
        active_region_count: active_set.len(),
        ..CountryMaterializationCoverageAudit::default()
    };
    let county_catalog = if app.is_some() && is_uk_country_iso2(&iso) {
        match load_gb_county_boundaries() {
            Ok(catalog) => Some(catalog),
            Err(error) => {
                eprintln!(
                    "[demand-materialization] iso={} county_boundary_load_error={}",
                    iso, error
                );
                None
            }
        }
    } else {
        None
    };
    let country_surface_cells_by_h3 = collect_country_surface_cells_by_res8(catalog);
    for region_id in &unlocked_regions {
        let Some(region) = catalog.by_id.get(region_id) else {
            coverage_audit.regions_missing_catalog_mapping += 1;
            continue;
        };
        let target_population = population_by_region
            .get(region_id)
            .copied()
            .unwrap_or_else(|| region.residents_smooth.max(0.0));
        let target_jobs = jobs_by_region
            .get(region_id)
            .copied()
            .unwrap_or_else(|| region.jobs_smooth.max(0.0));

        let source_surface_cells = catalog
            .cells_res8_by_region
            .get(region_id)
            .cloned()
            .unwrap_or_default();
        if !catalog.cells_res8_by_region.contains_key(region_id) {
            coverage_audit.regions_missing_catalog_mapping += 1;
        }
        if source_surface_cells.is_empty() {
            coverage_audit.regions_without_surface_cells += 1;
        }
        let expanded_surface = expand_surface_cells_to_region_res8_lattice(
            app,
            &iso,
            catalog,
            region,
            &source_surface_cells,
            &country_surface_cells_by_h3,
            county_catalog.as_ref(),
        );
        let surface_cells = expanded_surface.cells;
        let region_is_active = active_set.contains(region_id);
        let (allocation_inputs, mut coverage_row) =
            build_region_allocation_inputs_from_surface_cells(
                &iso,
                region,
                &surface_cells,
                region_is_active,
            );
        coverage_row.surface_source_kind = expanded_surface.source_kind;
        coverage_row.source_surface_candidates_total = source_surface_cells.len();
        coverage_row.substrate_res6_members_total = expanded_surface.substrate_res6_members_total;
        coverage_row.membership_res8_total = expanded_surface.membership_res8_total;
        coverage_row.surface_intersection_cells_added =
            expanded_surface.surface_intersection_cells_added;
        coverage_row.boundary_excluded_outside_geometry =
            expanded_surface.boundary_excluded_outside_geometry;
        coverage_row.boundary_included_from_geometry =
            expanded_surface.boundary_included_from_geometry;
        coverage_row.boundary_excluded_invalid_geometry =
            expanded_surface.boundary_excluded_invalid_geometry;
        coverage_row.direct_signal_cells_total = expanded_surface.direct_signal_cells_total;
        coverage_row.fallback_signal_cells_total = expanded_surface.fallback_signal_cells_total;
        coverage_row.source_sample_cell_ids = expanded_surface.source_sample_cell_ids;
        coverage_row.direct_signal_sample_cell_ids = expanded_surface.direct_signal_sample_cell_ids;
        coverage_row.fallback_signal_sample_cell_ids =
            expanded_surface.fallback_signal_sample_cell_ids;

        let allocation_outcome =
            allocate_region_cell_masses(target_population, target_jobs, allocation_inputs);
        let audit_row = build_region_allocation_audit_row(
            region_id,
            target_population,
            target_jobs,
            &allocation_outcome,
        );
        verify_region_allocation_invariants(&audit_row)?;
        coverage_row.unique_raw_residential_weight_count = unique_nonnegative_value_count(
            allocation_outcome
                .cells
                .iter()
                .map(|cell| cell.raw_weight_residential),
        );
        coverage_row.unique_raw_employment_weight_count = unique_nonnegative_value_count(
            allocation_outcome
                .cells
                .iter()
                .map(|cell| cell.raw_weight_employment),
        );
        coverage_row.unique_allocated_residential_mass_count = unique_nonnegative_value_count(
            allocation_outcome
                .cells
                .iter()
                .map(|cell| cell.allocated_residential_mass),
        );
        coverage_row.unique_allocated_employment_mass_count = unique_nonnegative_value_count(
            allocation_outcome
                .cells
                .iter()
                .map(|cell| cell.allocated_employment_mass),
        );

        coverage_audit.source_candidates_total += coverage_row.source_surface_candidates_total;
        coverage_audit.candidate_cells_total += coverage_row.surface_candidates_total;
        coverage_audit.substrate_res6_members_total += coverage_row.substrate_res6_members_total;
        coverage_audit.membership_res8_total += coverage_row.membership_res8_total;
        coverage_audit.surface_intersection_cells_added +=
            coverage_row.surface_intersection_cells_added;
        coverage_audit.boundary_excluded_outside_geometry +=
            coverage_row.boundary_excluded_outside_geometry;
        coverage_audit.boundary_included_from_geometry +=
            coverage_row.boundary_included_from_geometry;
        coverage_audit.boundary_excluded_invalid_geometry +=
            coverage_row.boundary_excluded_invalid_geometry;
        coverage_audit.direct_signal_cells_total += coverage_row.direct_signal_cells_total;
        coverage_audit.fallback_signal_cells_total += coverage_row.fallback_signal_cells_total;
        coverage_audit.dropped_by_active_scope += coverage_row.dropped_by_active_scope;
        coverage_audit.dropped_by_per_region_cap += coverage_row.dropped_by_per_region_cap;
        coverage_audit.dropped_invalid_surface_xy += coverage_row.dropped_invalid_surface_xy;
        coverage_audit.dropped_zero_filtered += coverage_row.dropped_zero_filtered;
        coverage_audit.dropped_dedup_collapse += coverage_row.dropped_dedup_collapse;
        coverage_audit.fallback_cells_added += coverage_row.fallback_cells_added;
        coverage_audit.allocation_inputs_total += coverage_row.allocation_inputs_total;

        let mut materialized_region_cells = 0usize;
        for cell in allocation_outcome.cells {
            let (wx, wy) = web_mercator_m_to_world_xy(
                &scenario.meta.crs,
                cell.x_mercator_m,
                cell.y_mercator_m,
            );
            let residents = cell.allocated_residential_mass.max(0.0);
            let jobs = cell.allocated_employment_mass.max(0.0);
            let demand_cell = DemandCell {
                cell_id: cell.persisted_cell_id.clone(),
                x: wx,
                y: wy,
                area_m2: cell.area_m2.max(0.0),
                residents_night: residents,
                jobs_day: jobs,
                activity_mix_residential: cell.activity_mix[0],
                activity_mix_office: cell.activity_mix[1],
                activity_mix_retail: cell.activity_mix[2],
                activity_mix_recreation: cell.activity_mix[3],
                activity_mix_industrial: cell.activity_mix[4],
                activity_mix_education: cell.activity_mix[5],
                activity_mix_health: cell.activity_mix[6],
                centrality_score: cell.centrality_score,
                data_quality_score: cell.data_quality_score,
                country_iso2: Some(iso.clone()),
                allocation_diagnostics: Some(DemandCellAllocationDiagnostics {
                    planning_region_id: Some(region_id.clone()),
                    raw_weight_residential: Some(cell.raw_weight_residential),
                    raw_weight_employment: Some(cell.raw_weight_employment),
                    raw_weight_education: Some(cell.raw_weight_education),
                    raw_weight_retail: Some(cell.raw_weight_retail),
                    raw_weight_leisure: Some(cell.raw_weight_leisure),
                    raw_weight_tourism: Some(cell.raw_weight_tourism),
                    allocated_residential_mass: Some(residents),
                    allocated_employment_mass: Some(jobs),
                    fallback_reason: cell.fallback_reason.clone(),
                }),
            };
            let zone = Zone {
                id: cell.persisted_cell_id,
                x: wx,
                y: wy,
                population: residents,
                jobs,
                country_iso2: Some(iso.clone()),
            };
            let inserted = append_or_merge_materialized_demand_cell(
                scenario,
                &mut materialized_cell_index_by_id,
                &mut materialized_zone_index_by_id,
                demand_cell,
                zone,
            );
            if inserted {
                loaded_cells += 1;
                materialized_region_cells += 1;
            } else {
                coverage_audit.global_duplicate_cells_merged += 1;
            }
        }
        coverage_row.materialized_cells_total = materialized_region_cells;
        coverage_audit.materialized_cells_total += materialized_region_cells;
        coverage_rows.push(coverage_row);
        allocation_audit_rows.push(audit_row);
    }
    let fallback_regions = allocation_audit_rows
        .iter()
        .filter(|row| row.fallback_residential || row.fallback_employment)
        .count();
    eprintln!(
        "[demand-materialization] iso={} unlocked_regions={} active_regions={} country_surface_cells={} source_candidates_total={} candidate_cells_total={} substrate_res6_members_total={} membership_res8_total={} surface_intersection_cells_added={} boundary_excluded_outside_geometry={} boundary_included_from_geometry={} boundary_excluded_invalid_geometry={} direct_signal_cells_total={} fallback_signal_cells_total={} regions_missing_catalog_mapping={} regions_without_surface_cells={} dropped_active_scope={} dropped_per_region_cap={} dropped_invalid_surface_xy={} dropped_zero_filtered={} dropped_dedup_collapse={} global_duplicate_cells_merged={} fallback_cells_added={} allocation_inputs_total={} materialized_cells_total={}",
        coverage_audit.country_iso2,
        coverage_audit.unlocked_region_count,
        coverage_audit.active_region_count,
        country_surface_cells_by_h3.len(),
        coverage_audit.source_candidates_total,
        coverage_audit.candidate_cells_total,
        coverage_audit.substrate_res6_members_total,
        coverage_audit.membership_res8_total,
        coverage_audit.surface_intersection_cells_added,
        coverage_audit.boundary_excluded_outside_geometry,
        coverage_audit.boundary_included_from_geometry,
        coverage_audit.boundary_excluded_invalid_geometry,
        coverage_audit.direct_signal_cells_total,
        coverage_audit.fallback_signal_cells_total,
        coverage_audit.regions_missing_catalog_mapping,
        coverage_audit.regions_without_surface_cells,
        coverage_audit.dropped_by_active_scope,
        coverage_audit.dropped_by_per_region_cap,
        coverage_audit.dropped_invalid_surface_xy,
        coverage_audit.dropped_zero_filtered,
        coverage_audit.dropped_dedup_collapse,
        coverage_audit.global_duplicate_cells_merged,
        coverage_audit.fallback_cells_added,
        coverage_audit.allocation_inputs_total,
        coverage_audit.materialized_cells_total,
    );
    for row in &coverage_rows {
        eprintln!(
            "[demand-materialization] region={} active={} source_kind={} source_candidates={} candidate_cells={} substrate_res6_members={} membership_res8_total={} surface_intersection_cells_added={} boundary_excluded_outside_geometry={} boundary_included_from_geometry={} boundary_excluded_invalid_geometry={} direct_signal_cells_total={} fallback_signal_cells_total={} dropped_active_scope={} dropped_per_region_cap={} dropped_invalid_surface_xy={} dropped_zero_filtered={} dropped_dedup_collapse={} fallback_cells_added={} allocation_inputs={} materialized_cells={} unique_raw_res={} unique_raw_jobs={} unique_alloc_res={} unique_alloc_jobs={} source_sample={} direct_signal_sample={} fallback_signal_sample={}",
            row.region_id,
            row.active_region,
            row.surface_source_kind,
            row.source_surface_candidates_total,
            row.surface_candidates_total,
            row.substrate_res6_members_total,
            row.membership_res8_total,
            row.surface_intersection_cells_added,
            row.boundary_excluded_outside_geometry,
            row.boundary_included_from_geometry,
            row.boundary_excluded_invalid_geometry,
            row.direct_signal_cells_total,
            row.fallback_signal_cells_total,
            row.dropped_by_active_scope,
            row.dropped_by_per_region_cap,
            row.dropped_invalid_surface_xy,
            row.dropped_zero_filtered,
            row.dropped_dedup_collapse,
            row.fallback_cells_added,
            row.allocation_inputs_total,
            row.materialized_cells_total,
            row.unique_raw_residential_weight_count,
            row.unique_raw_employment_weight_count,
            row.unique_allocated_residential_mass_count,
            row.unique_allocated_employment_mass_count,
            row.source_sample_cell_ids.join("|"),
            row.direct_signal_sample_cell_ids.join("|"),
            row.fallback_signal_sample_cell_ids.join("|"),
        );
    }
    eprintln!(
        "[demand-allocation] iso={} regions={} fallback_regions={} loaded_cells={}",
        iso,
        allocation_audit_rows.len(),
        fallback_regions,
        loaded_cells
    );
    for row in &allocation_audit_rows {
        eprintln!(
            "[demand-allocation] region={} cells={} pop_target={:.3} pop_alloc={:.3} jobs_target={:.3} jobs_alloc={:.3} fallback_res={} fallback_jobs={} top_res={} top_jobs={}",
            row.region_id,
            row.cells,
            row.target_population,
            row.allocated_population,
            row.target_jobs,
            row.allocated_jobs,
            row.fallback_residential,
            row.fallback_employment,
            row.top_residential_cells.join("|"),
            row.top_employment_cells.join("|"),
        );
    }
    Ok(loaded_cells)
}

pub(crate) fn upsert_pack_ref(
    manifest: &mut ProjectManifest,
    iso: &str,
    surface: &DemandSurfaceCountryWire,
) {
    let iso = canonical_country_iso2(iso).unwrap_or_else(|| iso.trim().to_ascii_uppercase());
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
    let rematerialize_started = Instant::now();
    let cells_before_clear = scenario.world.demand_cells.len();
    clear_surface_generated_persisted_demand(scenario);
    let cells_after_clear = scenario.world.demand_cells.len();

    let mut out = Vec::<DemandCoverageResult>::new();
    let mut loaded_countries = Vec::<String>::new();
    let mut surface_version = None::<String>;
    let countries = unlocked_country_codes(manifest);
    eprintln!(
        "[demand-rematerialize] countries={} unlocked_regions={} active_regions={} focus_region={} cells_before_clear={} cells_after_clear={}",
        countries.join(","),
        manifest.region_state.unlocked_region_ids.join("|"),
        manifest.region_state.active_region_ids.join("|"),
        manifest
            .region_state
            .primary_focus_region_id
            .as_deref()
            .unwrap_or("none"),
        cells_before_clear,
        cells_after_clear,
    );
    for iso in countries {
        let country_started = Instant::now();
        let Some(resolved_surface) =
            crate::commands::content_library::resolve_demand_surface_path(app, &iso)
        else {
            eprintln!(
                "[demand-rematerialize] country={} surface_missing unlocked_regions={} active_regions={}",
                iso,
                manifest.region_state.unlocked_region_ids.join("|"),
                manifest.region_state.active_region_ids.join("|"),
            );
            out.push(DemandCoverageResult {
                country_iso2: iso,
                installed: false,
                loaded: false,
                cells_loaded: 0,
                message: "Demand data not installed for country".to_string(),
            });
            continue;
        };
        eprintln!(
            "[demand-rematerialize] country={} surface_path={} source={}",
            iso,
            resolved_surface.path.to_string_lossy(),
            resolved_surface.source.as_str(),
        );
        let load_surface_started = Instant::now();
        let surface = load_surface_wire(&resolved_surface.path)?;
        perf_log(
            &format!("rematerialize_unlocked_country_surfaces.load_surface_wire[{iso}]"),
            load_surface_started,
        );
        let materialize_started = Instant::now();
        let loaded_count =
            materialize_country_surface_scoped_with_app(app, manifest, scenario, &iso, &surface)?;
        perf_log(
            &format!("rematerialize_unlocked_country_surfaces.materialize_country_surface[{iso}]"),
            materialize_started,
        );
        loaded_countries.push(iso.clone());
        surface_version = Some(surface.surface_version.clone());
        upsert_pack_ref(manifest, &iso, &surface);
        out.push(DemandCoverageResult {
            country_iso2: iso.clone(),
            installed: true,
            loaded: true,
            cells_loaded: loaded_count,
            message: format!(
                "Loaded scoped region demand from {} ({})",
                resolved_surface.path.to_string_lossy(),
                resolved_surface.source.as_str()
            ),
        });
        perf_log(
            &format!("rematerialize_unlocked_country_surfaces.country_total[{iso}]"),
            country_started,
        );
    }

    // Persisted gameplay demand authority is updated only here after full rematerialization.
    let (persisted_surface_version, persisted_loaded_countries) =
        write_surface_pipeline_demand_meta(scenario, surface_version, loaded_countries);
    sync_manifest_surface_pipeline_state(
        manifest,
        &persisted_surface_version,
        &persisted_loaded_countries,
    );
    perf_log(
        "rematerialize_unlocked_country_surfaces.total",
        rematerialize_started,
    );
    Ok(out)
}

pub(crate) fn ensure_country_surface_loaded(
    app: &AppHandle,
    manifest: &mut ProjectManifest,
    scenario: &mut Scenario,
    country_iso2: &str,
) -> Result<DemandCoverageResult, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
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

const UK_MID_2024_POPULATION_TOTAL: u64 = 69_281_400;
const UK_WORKFORCE_JOBS_TOTAL: u64 = 36_600_000;
const UK_POP_WEIGHT_RESIDENTS: f64 = 0.94;
const UK_POP_WEIGHT_JOBS: f64 = 0.06;
const UK_POP_LOW_SIGNAL_THRESHOLD: f64 = 1_200.0;
const UK_POP_LOW_SIGNAL_EXPONENT: f64 = 3.0;
const UK_JOBS_WEIGHT_JOBS: f64 = 0.92;
const UK_JOBS_WEIGHT_RESIDENTS: f64 = 0.08;
const UK_JOBS_LOW_SIGNAL_THRESHOLD: f64 = 800.0;
const UK_JOBS_LOW_SIGNAL_EXPONENT: f64 = 3.0;

fn low_signal_taper(base_signal: f64, threshold: f64, exponent: f64) -> f64 {
    if base_signal <= 0.0 {
        return 0.0;
    }
    if threshold > 0.0 && base_signal < threshold {
        let ratio = (base_signal / threshold).clamp(0.0, 1.0);
        return threshold * ratio.powf(exponent.max(1.0));
    }
    base_signal
}

fn fixed_total_allocation_by_weight(
    weighted_ids: &[(String, f64)],
    total: u64,
) -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::<String, u64>::new();
    if weighted_ids.is_empty() {
        return out;
    }

    let cleaned = weighted_ids
        .iter()
        .map(|(id, weight)| {
            let cleaned_weight = if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            };
            (id.clone(), cleaned_weight)
        })
        .collect::<Vec<_>>();
    let total_weight = cleaned.iter().map(|(_, weight)| *weight).sum::<f64>();
    let fallback_share = 1.0 / (cleaned.len() as f64);
    let total_as_f64 = total as f64;

    let mut rows = cleaned
        .into_iter()
        .map(|(id, weight)| {
            let share = if total_weight > 0.0 {
                weight / total_weight
            } else {
                fallback_share
            };
            let raw = share * total_as_f64;
            let floor = raw.floor().max(0.0) as u64;
            (id, floor, raw - (floor as f64))
        })
        .collect::<Vec<_>>();

    let assigned = rows.iter().map(|(_, floor, _)| *floor).sum::<u64>();
    let remainder = total.saturating_sub(assigned);
    let mut order = (0..rows.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| {
        rows[*b]
            .2
            .partial_cmp(&rows[*a].2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| rows[*a].0.cmp(&rows[*b].0))
    });
    for idx in order.into_iter().take(remainder as usize) {
        rows[idx].1 = rows[idx].1.saturating_add(1);
    }

    for (id, value, _) in rows {
        out.insert(id, value);
    }
    out
}

pub(crate) fn calibrated_region_population_for_country(
    country_iso2: &str,
    regions: &[SurfaceRegionInfo],
) -> std::collections::HashMap<String, u64> {
    if !is_uk_country_iso2(country_iso2) {
        return std::collections::HashMap::new();
    }
    let weighted_ids = regions
        .iter()
        .map(|region| {
            let residents = region.residents_smooth.max(0.0);
            let jobs = region.jobs_smooth.max(0.0);
            let blended_signal = residents * UK_POP_WEIGHT_RESIDENTS + jobs * UK_POP_WEIGHT_JOBS;
            let weight = low_signal_taper(
                blended_signal,
                UK_POP_LOW_SIGNAL_THRESHOLD,
                UK_POP_LOW_SIGNAL_EXPONENT,
            );
            (region.region_id.clone(), weight)
        })
        .collect::<Vec<_>>();
    fixed_total_allocation_by_weight(&weighted_ids, UK_MID_2024_POPULATION_TOTAL)
}

pub(crate) fn calibrated_region_jobs_for_country(
    country_iso2: &str,
    regions: &[SurfaceRegionInfo],
) -> std::collections::HashMap<String, u64> {
    if !is_uk_country_iso2(country_iso2) {
        return std::collections::HashMap::new();
    }
    let weighted_ids = regions
        .iter()
        .map(|region| {
            let residents = region.residents_smooth.max(0.0);
            let jobs = region.jobs_smooth.max(0.0);
            let blended_signal = jobs * UK_JOBS_WEIGHT_JOBS + residents * UK_JOBS_WEIGHT_RESIDENTS;
            let weight = low_signal_taper(
                blended_signal,
                UK_JOBS_LOW_SIGNAL_THRESHOLD,
                UK_JOBS_LOW_SIGNAL_EXPONENT,
            );
            (region.region_id.clone(), weight)
        })
        .collect::<Vec<_>>();
    fixed_total_allocation_by_weight(&weighted_ids, UK_WORKFORCE_JOBS_TOTAL)
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
        let population_by_region =
            calibrated_region_population_for_country(iso.as_str(), &catalog.regions);
        let jobs_by_region = calibrated_region_jobs_for_country(iso.as_str(), &catalog.regions);
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
                region_kind: region.region_kind.clone(),
                region_token: region.region_token.clone(),
                h3_cell_id: region.h3_cell_id.clone(),
                name: region.name.clone(),
                admin_level: region.admin_level.clone(),
                nation: region.nation.clone(),
                source_code: region.source_code.clone(),
                adjacency_source: region.adjacency_source.clone(),
                geometry_source: region.geometry_source.clone(),
                unlocked: unlocked_regions.contains(&region.region_id),
                active: active_regions.contains(&region.region_id),
                adjacent_region_ids: region.adjacent_region_ids.clone(),
                unlock_cost_base: region_unlock_cost_base_for_manifest(
                    manifest,
                    &region,
                    population_by_region.get(&region.region_id).copied(),
                    jobs_by_region.get(&region.region_id).copied(),
                ),
                population: population_by_region.get(&region.region_id).copied(),
                jobs: jobs_by_region.get(&region.region_id).copied(),
                residents_smooth: region.residents_smooth,
                jobs_smooth: region.jobs_smooth,
                employment_estimate,
                cells_res8,
                // RegionStatus geometry is the player-facing planning boundary.
                // H3-backed regions without authored polygons get the same
                // canonical H3 boundary used by demand materialisation and
                // demand-overlay clipping, so visible boundaries and overlay
                // coverage do not drift apart.
                geometry: canonical_region_geometry_value(&region),
                canonical_hex_number: region.canonical_hex_number,
                constituent_hex_numbers: region.constituent_hex_numbers.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(left: f64, right: f64, tolerance: f64) {
        let diff = (left - right).abs();
        assert!(
            diff <= tolerance,
            "expected {left:.9} ~= {right:.9} (diff={diff:.9}, tol={tolerance:.9})"
        );
    }

    fn sample_input(
        cell_id: &str,
        area_m2: f64,
        residents_signal: f64,
        jobs_signal: f64,
        mix: [f64; 7],
        centrality_score: f64,
        data_quality_score: f64,
    ) -> RegionAllocationCellInput {
        RegionAllocationCellInput {
            persisted_cell_id: cell_id.to_string(),
            x_mercator_m: 0.0,
            y_mercator_m: 0.0,
            area_m2,
            residents_signal,
            jobs_signal,
            activity_mix: mix,
            centrality_score,
            data_quality_score,
            signal_source: "test_direct".to_string(),
        }
    }

    fn sample_region_info(region_id: &str) -> SurfaceRegionInfo {
        SurfaceRegionInfo {
            region_id: region_id.to_string(),
            country_iso2: "GB".to_string(),
            region_kind: "planning".to_string(),
            region_token: "sample".to_string(),
            h3_cell_id: None,
            name: "Sample Region".to_string(),
            admin_level: "planning".to_string(),
            nation: Some("GB".to_string()),
            source_code: Some("test".to_string()),
            adjacency_source: "test".to_string(),
            geometry_source: "test".to_string(),
            cell_id: "88194e2b89fffff".to_string(),
            x: 0.0,
            y: 0.0,
            area_m2: 1_000_000.0,
            residents_smooth: 10_000.0,
            jobs_smooth: 7_500.0,
            activity_mix_residential: 0.42,
            activity_mix_office: 0.24,
            activity_mix_retail: 0.12,
            activity_mix_recreation: 0.08,
            activity_mix_industrial: 0.08,
            activity_mix_education: 0.03,
            activity_mix_health: 0.03,
            adjacent_region_ids: Vec::new(),
            geometry: None,
            canonical_hex_number: None,
            constituent_hex_numbers: Vec::new(),
        }
    }

    fn sample_surface_cell(cell_id: &str, residents: f64, jobs: f64) -> DemandSurfaceCellWire {
        DemandSurfaceCellWire {
            cell_id: cell_id.to_string(),
            h3_res: 8,
            lon: -0.1,
            lat: 51.5,
            x: 0.0,
            y: 0.0,
            area_m2: 120_000.0,
            country_iso2: "GB".to_string(),
            residents_raw: residents.max(0.0),
            jobs_raw: jobs.max(0.0),
            residents_smooth: residents.max(0.0),
            jobs_smooth: jobs.max(0.0),
            activity_mix_residential: 0.45,
            activity_mix_office: 0.22,
            activity_mix_retail: 0.12,
            activity_mix_recreation: 0.08,
            activity_mix_industrial: 0.08,
            activity_mix_education: 0.03,
            activity_mix_health: 0.02,
            quality: 0.72,
        }
    }

    fn test_params() -> Params {
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

    fn empty_test_scenario() -> Scenario {
        Scenario {
            meta: Meta {
                name: "Demand Merge Test".to_string(),
                seed: 1,
                time_period_hours: 1.0,
                crs: Crs::Epsg3857,
            },
            params: test_params(),
            world: World {
                zones: vec![],
                stops: vec![],
                links: vec![],
                services: vec![],
                transfers: vec![],
                transfer_rules: None,
                demand_cells: vec![],
                demand_meta: None,
            },
        }
    }

    fn sample_demand_cell(cell_id: &str, region_id: &str, residents: f64, jobs: f64) -> DemandCell {
        DemandCell {
            cell_id: cell_id.to_string(),
            x: 0.0,
            y: 0.0,
            area_m2: 120_000.0,
            residents_night: residents,
            jobs_day: jobs,
            activity_mix_residential: 0.45,
            activity_mix_office: 0.22,
            activity_mix_retail: 0.12,
            activity_mix_recreation: 0.08,
            activity_mix_industrial: 0.08,
            activity_mix_education: 0.03,
            activity_mix_health: 0.02,
            centrality_score: 0.55,
            data_quality_score: 0.72,
            country_iso2: Some("UK".to_string()),
            allocation_diagnostics: Some(DemandCellAllocationDiagnostics {
                planning_region_id: Some(region_id.to_string()),
                raw_weight_residential: Some(residents),
                raw_weight_employment: Some(jobs),
                raw_weight_education: None,
                raw_weight_retail: None,
                raw_weight_leisure: None,
                raw_weight_tourism: None,
                allocated_residential_mass: Some(residents),
                allocated_employment_mass: Some(jobs),
                fallback_reason: None,
            }),
        }
    }

    fn sample_zone(zone_id: &str, residents: f64, jobs: f64) -> Zone {
        Zone {
            id: zone_id.to_string(),
            x: 0.0,
            y: 0.0,
            population: residents,
            jobs,
            country_iso2: Some("UK".to_string()),
        }
    }

    fn expanded_cells_from_surface(
        cells: Vec<DemandSurfaceCellWire>,
    ) -> Vec<ExpandedRegionSurfaceCell> {
        cells
            .into_iter()
            .map(|cell| ExpandedRegionSurfaceCell {
                surface_cell: cell,
                signal_source: "direct_surface".to_string(),
                used_fallback: false,
            })
            .collect()
    }

    fn sample_catalog_for_region(
        region: &SurfaceRegionInfo,
        parent_res6_region_id: &str,
        source_cells: Vec<DemandSurfaceCellWire>,
    ) -> SurfaceRegionCatalog {
        let mut by_id = HashMap::<String, SurfaceRegionInfo>::new();
        by_id.insert(region.region_id.clone(), region.clone());
        let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
        cells_res8_by_region.insert(region.region_id.clone(), source_cells);
        let mut legacy_region_aliases = HashMap::<String, String>::new();
        legacy_region_aliases.insert(parent_res6_region_id.to_string(), region.region_id.clone());
        SurfaceRegionCatalog {
            regions: vec![region.clone()],
            by_id,
            cells_res8_by_region,
            legacy_region_aliases,
        }
    }

    fn geometry_value_for_cell(cell: CellIndex) -> serde_json::Value {
        let polygon = h3_cell_boundary_polygon(cell).expect("cell boundary polygon");
        let coords = polygon
            .exterior()
            .points()
            .map(|point| vec![point.x(), point.y()])
            .collect::<Vec<_>>();
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [coords],
        })
    }

    fn sample_h3_backed_region(parent: CellIndex) -> SurfaceRegionInfo {
        let mut region = sample_region_info(&format!("r6:GB:{parent}"));
        region.region_kind = "planning_hex_unassigned".to_string();
        region.region_token = parent.to_string();
        region.h3_cell_id = Some(parent.to_string());
        region.source_code = Some("manual_region_unassigned_hex".to_string());
        region.geometry_source = "planning_surface_res6".to_string();
        region.cell_id = parent.to_string();
        region.geometry = None;
        region
    }

    #[test]
    fn h3_backed_region_without_authored_geometry_has_canonical_boundary() {
        let parent = "861951b2fffffff"
            .parse::<CellIndex>()
            .expect("res6 parent should parse");
        let region = sample_h3_backed_region(parent);

        let value = canonical_region_geometry_value(&region)
            .expect("H3-backed region should expose canonical display geometry");
        let parsed = region_geo_geometry(&region)
            .expect("canonical geometry should parse")
            .expect("H3 fallback geometry should be available");

        assert_eq!(
            value.get("type").and_then(|value| value.as_str()),
            Some("Polygon")
        );
        assert!(
            parsed.intersects(&h3_cell_boundary_polygon(parent).expect("parent polygon")),
            "canonical fallback should describe the same H3 planning boundary used by the overlay"
        );
    }

    #[test]
    fn h3_backed_region_res8_members_include_edge_intersecting_neighbors() {
        let parent = "861951b2fffffff"
            .parse::<CellIndex>()
            .expect("res6 parent should parse");
        let region = sample_h3_backed_region(parent);
        let direct_children = parent.children(Resolution::Eight).collect::<HashSet<_>>();

        let members = region_geometry_res8_members(&region)
            .expect("region geometry membership should compute")
            .expect("H3-backed region should use fallback geometry");

        assert!(
            members.len() > direct_children.len(),
            "the visible res6 planning boundary needs intersecting res8 edge cells beyond strict descendants"
        );
        assert!(
            members.iter().any(|cell| !direct_children.contains(cell)),
            "boundary sliver coverage should include non-descendant res8 cells that intersect the visible H3 region"
        );
    }

    #[test]
    fn allocation_preserves_region_totals() {
        let inputs = vec![
            sample_input(
                "c1",
                1_000_000.0,
                200.0,
                140.0,
                [0.72, 0.08, 0.06, 0.05, 0.02, 0.04, 0.03],
                0.35,
                0.80,
            ),
            sample_input(
                "c2",
                900_000.0,
                110.0,
                180.0,
                [0.35, 0.32, 0.14, 0.05, 0.08, 0.04, 0.02],
                0.58,
                0.76,
            ),
            sample_input(
                "c3",
                1_100_000.0,
                90.0,
                220.0,
                [0.18, 0.41, 0.17, 0.05, 0.12, 0.04, 0.03],
                0.70,
                0.82,
            ),
        ];

        let target_population = 12_500.0;
        let target_jobs = 8_200.0;
        let outcome = allocate_region_cell_masses(target_population, target_jobs, inputs);
        let audit = build_region_allocation_audit_row(
            "r6:TEST:alpha",
            target_population,
            target_jobs,
            &outcome,
        );

        verify_region_allocation_invariants(&audit)
            .expect("allocation totals should match targets");
        approx_eq(
            audit.allocated_population,
            target_population,
            1e-6 * target_population.max(1.0),
        );
        approx_eq(
            audit.allocated_jobs,
            target_jobs,
            1e-6 * target_jobs.max(1.0),
        );
    }

    #[test]
    fn allocation_differentiates_intra_region_cells() {
        let inputs = vec![
            sample_input(
                "res-heavy",
                1_200_000.0,
                250.0,
                25.0,
                [0.82, 0.04, 0.04, 0.04, 0.02, 0.03, 0.01],
                0.28,
                0.72,
            ),
            sample_input(
                "job-heavy",
                1_000_000.0,
                30.0,
                260.0,
                [0.10, 0.42, 0.18, 0.04, 0.18, 0.05, 0.03],
                0.78,
                0.85,
            ),
        ];

        let outcome = allocate_region_cell_masses(1_000.0, 1_000.0, inputs);
        assert_eq!(outcome.cells.len(), 2);
        let a = &outcome.cells[0];
        let b = &outcome.cells[1];

        assert!(
            a.allocated_residential_mass > b.allocated_residential_mass,
            "res-heavy cell should receive more residential mass"
        );
        assert!(
            b.allocated_employment_mass > a.allocated_employment_mass,
            "job-heavy cell should receive more employment mass"
        );
    }

    #[test]
    fn allocation_fallback_handles_degenerate_weights() {
        let inputs = vec![
            sample_input("zero-a", 0.0, 0.0, 0.0, [0.0; 7], 0.0, 0.0),
            sample_input("zero-b", 0.0, 0.0, 0.0, [0.0; 7], 0.0, 0.0),
        ];
        let target_population = 100.0;
        let target_jobs = 60.0;
        let outcome = allocate_region_cell_masses(target_population, target_jobs, inputs);
        let audit = build_region_allocation_audit_row(
            "r6:TEST:degenerate",
            target_population,
            target_jobs,
            &outcome,
        );

        assert!(outcome.fallback_residential);
        assert!(outcome.fallback_employment);
        assert_eq!(outcome.cells.len(), 2);
        for cell in &outcome.cells {
            assert_eq!(
                cell.fallback_reason.as_deref(),
                Some("degenerate_residential_and_employment_weights")
            );
        }
        verify_region_allocation_invariants(&audit)
            .expect("degenerate fallback should still preserve totals");
        approx_eq(outcome.cells[0].allocated_residential_mass, 50.0, 1e-6);
        approx_eq(outcome.cells[1].allocated_residential_mass, 50.0, 1e-6);
        approx_eq(outcome.cells[0].allocated_employment_mass, 30.0, 1e-6);
        approx_eq(outcome.cells[1].allocated_employment_mass, 30.0, 1e-6);
    }

    #[test]
    fn materialization_retains_broad_surface_cell_coverage() {
        let region = sample_region_info("r6:GB:london-core");
        let mut surface_cells = Vec::<DemandSurfaceCellWire>::new();
        for idx in 0..96 {
            let cell_id = format!("88a194e2b{:02x}fffff", idx);
            surface_cells.push(sample_surface_cell(&cell_id, 0.001, 0.001));
        }
        let expanded = expanded_cells_from_surface(surface_cells);

        let (inputs, coverage) =
            build_region_allocation_inputs_from_surface_cells("GB", &region, &expanded, false);
        assert_eq!(coverage.surface_candidates_total, 96);
        assert_eq!(coverage.dropped_by_active_scope, 0);
        assert_eq!(coverage.dropped_by_per_region_cap, 0);
        assert_eq!(coverage.dropped_zero_filtered, 0);
        assert_eq!(coverage.fallback_cells_added, 0);
        assert_eq!(
            inputs.len(),
            96,
            "all valid unlocked-surface cells should be retained"
        );
    }

    #[test]
    fn materialized_duplicate_cells_are_merged_into_unique_zones() {
        let mut scenario = empty_test_scenario();
        let mut demand_index_by_id = HashMap::<String, usize>::new();
        let mut zone_index_by_id = HashMap::<String, usize>::new();
        let cell_id = "ds:v4:UK:88194ad109fffff";

        assert!(append_or_merge_materialized_demand_cell(
            &mut scenario,
            &mut demand_index_by_id,
            &mut zone_index_by_id,
            sample_demand_cell(cell_id, "r6:UK:first", 10.0, 4.0),
            sample_zone(cell_id, 10.0, 4.0),
        ));
        assert!(!append_or_merge_materialized_demand_cell(
            &mut scenario,
            &mut demand_index_by_id,
            &mut zone_index_by_id,
            sample_demand_cell(cell_id, "r6:UK:second", 6.0, 9.0),
            sample_zone(cell_id, 6.0, 9.0),
        ));

        assert_eq!(scenario.world.demand_cells.len(), 1);
        assert_eq!(scenario.world.zones.len(), 1);
        assert_eq!(scenario.world.demand_cells[0].residents_night, 10.0);
        assert_eq!(scenario.world.demand_cells[0].jobs_day, 4.0);
        assert_eq!(scenario.world.zones[0].population, 10.0);
        assert_eq!(scenario.world.zones[0].jobs, 4.0);
        let diagnostics = scenario.world.demand_cells[0]
            .allocation_diagnostics
            .as_ref()
            .expect("merged demand cell should retain diagnostics");
        assert_eq!(diagnostics.planning_region_id, None);
        assert_eq!(
            diagnostics.fallback_reason.as_deref(),
            Some("merged_overlapping_region_cells")
        );
        assert_eq!(canonical_country_iso2("GB").as_deref(), Some("UK"));
    }

    #[test]
    fn country_surface_intersections_add_boundary_cells_not_owned_by_region() {
        let cell = "88194ad109fffff"
            .parse::<CellIndex>()
            .expect("res8 cell should parse");
        let geometry = geo::Geometry::Polygon(h3_cell_boundary_polygon(cell).expect("polygon"));
        let mut members = HashSet::<CellIndex>::new();
        let mut country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
        country_surface_cells.insert(cell, sample_surface_cell(&cell.to_string(), 42.0, 24.0));

        let added = include_intersecting_country_surface_cells(
            &geometry,
            &mut members,
            &country_surface_cells,
        );

        assert_eq!(added, 1);
        assert!(members.contains(&cell));
    }

    #[test]
    fn substrate_intersections_add_materialization_cells_missed_by_seed_set() {
        let cell = "88194ad109fffff"
            .parse::<CellIndex>()
            .expect("res8 cell should parse");
        let geometry = geo::Geometry::Polygon(h3_cell_boundary_polygon(cell).expect("polygon"));
        let mut members = HashSet::<CellIndex>::new();

        let added = include_intersecting_candidate_cells(&geometry, &mut members, [cell]);

        assert_eq!(added, 1);
        assert!(members.contains(&cell));
    }

    #[test]
    fn lattice_expansion_uses_authoritative_country_surface_values() {
        let cell = "88194ad109fffff"
            .parse::<CellIndex>()
            .expect("res8 cell should parse");
        let parent = cell
            .parent(Resolution::Six)
            .expect("res8 cell should have res6 parent");
        let mut region = sample_region_info("r6:GB:central-london");
        region.geometry = Some(geometry_value_for_cell(cell));
        let catalog = sample_catalog_for_region(&region, &format!("r6:GB:{parent}"), Vec::new());
        let mut country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
        country_surface_cells.insert(cell, sample_surface_cell(&cell.to_string(), 42.0, 24.0));
        let source_cells = Vec::<DemandSurfaceCellWire>::new();

        let expansion = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog,
            &region,
            &source_cells,
            &country_surface_cells,
            None,
        );
        let expanded = expansion
            .cells
            .iter()
            .find(|candidate| candidate.surface_cell.cell_id == cell.to_string())
            .expect("intersecting country-surface cell should be materialized");

        assert_eq!(
            expanded.signal_source,
            "direct_country_surface_intersection"
        );
        assert_eq!(expanded.surface_cell.residents_smooth, 42.0);
        assert_eq!(expanded.surface_cell.jobs_smooth, 24.0);
    }

    #[test]
    fn sparse_surface_cells_expand_to_parent_res8_lattice() {
        let region = sample_region_info("r6:GB:central-london");
        let parent_region_id = "r6:GB:86194ad17ffffff";
        let source_cells = vec![sample_surface_cell("88194ad109fffff", 12.0, 8.0)];
        let catalog = sample_catalog_for_region(&region, parent_region_id, source_cells.clone());
        let no_country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();

        let expansion = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog,
            &region,
            &source_cells,
            &no_country_surface_cells,
            None,
        );

        assert_eq!(expansion.substrate_res6_members_total, 1);
        assert_eq!(expansion.membership_res8_total, 49);
        assert_eq!(expansion.cells.len(), 49);
        assert!(
            expansion
                .cells
                .iter()
                .any(|cell| cell.surface_cell.cell_id == "88194ad109fffff"),
            "membership should include the source cell id when it is in region coverage"
        );
    }

    #[test]
    fn sparse_expansion_generates_non_uniform_direct_cell_signals() {
        let region = sample_region_info("r6:GB:central-london");
        let parent = "86194ad17ffffff"
            .parse::<CellIndex>()
            .expect("res6 parent should parse");
        let children = parent
            .children(Resolution::Eight)
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 2, "expected at least two res8 children");
        let seed_a = children[0].to_string().to_ascii_lowercase();
        let seed_b = children[1].to_string().to_ascii_lowercase();

        let source_cells = vec![
            sample_surface_cell(&seed_a, 20.0, 4.0),
            sample_surface_cell(&seed_b, 3.0, 18.0),
        ];
        let catalog =
            sample_catalog_for_region(&region, "r6:GB:86194ad17ffffff", source_cells.clone());
        let no_country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
        let expansion = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog,
            &region,
            &source_cells,
            &no_country_surface_cells,
            None,
        );

        let unique_residents = unique_nonnegative_value_count(
            expansion
                .cells
                .iter()
                .map(|cell| cell.surface_cell.residents_smooth),
        );
        let unique_jobs = unique_nonnegative_value_count(
            expansion
                .cells
                .iter()
                .map(|cell| cell.surface_cell.jobs_smooth),
        );
        assert!(
            unique_residents > 2,
            "direct per-cell residents signal should vary across expanded cells"
        );
        assert!(
            unique_jobs > 2,
            "direct per-cell jobs signal should vary across expanded cells"
        );
    }

    #[test]
    fn sparse_source_cells_are_not_privileged_weight_anchors() {
        let region = sample_region_info("r6:GB:central-london");
        let source_cell_id = "88194ad109fffff";
        let source_cells = vec![sample_surface_cell(
            source_cell_id,
            1_000_000.0,
            1_000_000.0,
        )];
        let catalog =
            sample_catalog_for_region(&region, "r6:GB:86194ad17ffffff", source_cells.clone());
        let no_country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
        let expansion = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog,
            &region,
            &source_cells,
            &no_country_surface_cells,
            None,
        );
        let source_cell = expansion
            .cells
            .iter()
            .find(|cell| cell.surface_cell.cell_id == source_cell_id)
            .expect("source cell should remain in expanded membership");
        assert!(
            source_cell.surface_cell.residents_smooth < 500.0
                && source_cell.surface_cell.jobs_smooth < 500.0,
            "expanded cell signals should come from direct per-cell derivation, not sparse source anchor values"
        );
    }

    #[test]
    fn boundary_membership_is_seed_independent() {
        let region = sample_region_info("r6:GB:central-london");
        let source_a = vec![sample_surface_cell("88194ad109fffff", 12.0, 8.0)];
        let source_b = vec![sample_surface_cell("88194ad125fffff", 4.0, 20.0)];
        let catalog_a =
            sample_catalog_for_region(&region, "r6:GB:86194ad17ffffff", source_a.clone());
        let catalog_b =
            sample_catalog_for_region(&region, "r6:GB:86194ad17ffffff", source_b.clone());
        let no_country_surface_cells = HashMap::<CellIndex, DemandSurfaceCellWire>::new();
        let expansion_a = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog_a,
            &region,
            &source_a,
            &no_country_surface_cells,
            None,
        );
        let expansion_b = expand_surface_cells_to_region_res8_lattice(
            None,
            "GB",
            &catalog_b,
            &region,
            &source_b,
            &no_country_surface_cells,
            None,
        );
        let set_a = expansion_a
            .cells
            .iter()
            .map(|cell| cell.surface_cell.cell_id.clone())
            .collect::<BTreeSet<_>>();
        let set_b = expansion_b
            .cells
            .iter()
            .map(|cell| cell.surface_cell.cell_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            set_a, set_b,
            "region membership should be mechanically determined and must not depend on sparse source seeds"
        );
    }

    #[test]
    fn live_central_london_sample_cells_imply_147_res8_lattice_cells() {
        let live_cell_ids = [
            "88194ad109fffff",
            "88194ad125fffff",
            "88194ad347fffff",
            "88195da487fffff",
            "88195da4d3fffff",
        ];
        let mut parent_res6 = HashSet::<String>::new();
        for cell_id in live_cell_ids {
            let cell = cell_id
                .parse::<CellIndex>()
                .expect("live demand cell id should parse as h3");
            let parent = cell
                .parent(Resolution::Six)
                .expect("res8 cell should have res6 parent");
            parent_res6.insert(parent.to_string().to_ascii_lowercase());
        }
        assert_eq!(parent_res6.len(), 3);
        let expected_res8_children = parent_res6
            .iter()
            .map(|parent| {
                parent
                    .parse::<CellIndex>()
                    .expect("res6 parent should parse")
                    .children(Resolution::Eight)
                    .count()
            })
            .sum::<usize>();
        assert_eq!(expected_res8_children, 147);
    }

    #[test]
    fn materialization_keeps_low_weight_cells_and_preserves_totals() {
        let region = sample_region_info("r6:GB:london-core");
        let mut surface_cells = Vec::<DemandSurfaceCellWire>::new();
        for idx in 0..64 {
            let cell_id = format!("88a194e2c{:02x}fffff", idx);
            let residents_signal = if idx % 9 == 0 { 0.00001 } else { 0.001 };
            let jobs_signal = if idx % 11 == 0 { 0.00002 } else { 0.001 };
            surface_cells.push(sample_surface_cell(&cell_id, residents_signal, jobs_signal));
        }
        let expanded = expanded_cells_from_surface(surface_cells);

        let (inputs, coverage) =
            build_region_allocation_inputs_from_surface_cells("GB", &region, &expanded, true);
        assert_eq!(inputs.len(), 64);
        assert_eq!(coverage.fallback_cells_added, 0);

        let outcome = allocate_region_cell_masses(8_000.0, 5_500.0, inputs);
        let audit =
            build_region_allocation_audit_row("r6:GB:london-core", 8_000.0, 5_500.0, &outcome);
        verify_region_allocation_invariants(&audit)
            .expect("wider retained cell set must still conserve region totals");
        assert_eq!(
            outcome.cells.len(),
            64,
            "low-signal cells should remain represented in authoritative demand materialization"
        );
    }
}
