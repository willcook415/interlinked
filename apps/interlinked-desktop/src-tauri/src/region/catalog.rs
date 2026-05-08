use crate::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const UK_LAND_BACKFILL_SOURCE_CODE: &str = "uk_land_backfill_res6";
const UK_LAND_BACKFILL_NI_SOURCE_CODE: &str = "uk_land_backfill_res6_ni";

fn perf_log(label: &str, started: Instant) {
    eprintln!("[perf] {label}: {}ms", started.elapsed().as_millis());
}

#[derive(Debug, Clone)]
pub(crate) struct SurfaceRegionInfo {
    // Canonical runtime region descriptor shared by all country builders.
    // Strategic planning-region providers and any legacy compatibility providers
    // both serialize into this runtime shape.
    pub(crate) region_id: String,
    pub(crate) country_iso2: String,
    pub(crate) region_kind: String,
    pub(crate) region_token: String,
    pub(crate) h3_cell_id: Option<String>,
    pub(crate) name: String,
    pub(crate) admin_level: String,
    pub(crate) nation: Option<String>,
    pub(crate) source_code: Option<String>,
    pub(crate) adjacency_source: String,
    pub(crate) geometry_source: String,
    pub(crate) cell_id: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) area_m2: f64,
    pub(crate) residents_smooth: f64,
    pub(crate) jobs_smooth: f64,
    pub(crate) activity_mix_residential: f64,
    pub(crate) activity_mix_office: f64,
    pub(crate) activity_mix_retail: f64,
    pub(crate) activity_mix_recreation: f64,
    pub(crate) activity_mix_industrial: f64,
    pub(crate) activity_mix_education: f64,
    pub(crate) activity_mix_health: f64,
    pub(crate) adjacent_region_ids: Vec<String>,
    pub(crate) geometry: Option<JsonValue>,
    pub(crate) canonical_hex_number: Option<usize>,
    /// Parallel array to `geometry` polygons. If `geometry` is a MultiPolygon,
    /// this array contains the canonical hex number for each polygon in order.
    pub(crate) constituent_hex_numbers: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct SurfaceRegionCatalog {
    pub(crate) regions: Vec<SurfaceRegionInfo>,
    pub(crate) by_id: HashMap<String, SurfaceRegionInfo>,
    pub(crate) cells_res8_by_region: HashMap<String, Vec<DemandSurfaceCellWire>>,
    // Compatibility aliases from legacy substrate IDs to synthesized planning-region IDs.
    pub(crate) legacy_region_aliases: HashMap<String, String>,
}

pub(crate) fn nearest_region_ids_by_xy(
    regions: &[SurfaceRegionInfo],
    x: f64,
    y: f64,
    limit: usize,
    exclude_region_id: Option<&str>,
) -> Vec<String> {
    let mut nearest = regions
        .iter()
        .filter(|r| {
            exclude_region_id
                .map(|id| id != r.region_id)
                .unwrap_or(true)
        })
        .map(|r| {
            let d2 = (r.x - x).powi(2) + (r.y - y).powi(2);
            (d2, r.region_id.clone())
        })
        .collect::<Vec<_>>();
    nearest.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    nearest.into_iter().take(limit).map(|(_, id)| id).collect()
}

fn build_substrate_region_catalog(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> SurfaceRegionCatalog {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let uk_backfill_ids = surface
        .source_provenance
        .get(UK_LAND_BACKFILL_SOURCE_CODE)
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let uk_backfill_ids_ni = surface
        .source_provenance
        .get(UK_LAND_BACKFILL_NI_SOURCE_CODE)
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|value| value.as_str())
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut regions = surface
        .cells_res6
        .iter()
        .map(|c| {
            let cell_token = c.cell_id.to_ascii_lowercase();
            let source_code = if uk_backfill_ids_ni.contains(&cell_token) {
                Some(UK_LAND_BACKFILL_NI_SOURCE_CODE.to_string())
            } else if uk_backfill_ids.contains(&cell_token) {
                Some(UK_LAND_BACKFILL_SOURCE_CODE.to_string())
            } else {
                None
            };
            SurfaceRegionInfo {
                region_id: region_id_from_res6(&iso, &c.cell_id),
                country_iso2: iso.clone(),
                region_kind: "strategic_planning_region".to_string(),
                region_token: cell_token.clone(),
                h3_cell_id: Some(cell_token.clone()),
                name: format!("{} {}", iso, &c.cell_id),
                admin_level: "planning_r6".to_string(),
                nation: None,
                source_code,
                adjacency_source: "planning_res6_h3_disk_k1".to_string(),
                geometry_source: "planning_surface_res6".to_string(),
                cell_id: c.cell_id.clone(),
                x: c.x,
                y: c.y,
                area_m2: c.area_m2.max(0.0),
                residents_smooth: c.residents_smooth.max(0.0),
                jobs_smooth: c.jobs_smooth.max(0.0),
                activity_mix_residential: c.activity_mix_residential,
                activity_mix_office: c.activity_mix_office,
                activity_mix_retail: c.activity_mix_retail,
                activity_mix_recreation: c.activity_mix_recreation,
                activity_mix_industrial: c.activity_mix_industrial,
                activity_mix_education: c.activity_mix_education,
                activity_mix_health: c.activity_mix_health,
                adjacent_region_ids: vec![],
                geometry: None,
                canonical_hex_number: None,
                constituent_hex_numbers: vec![],
            }
        })
        .collect::<Vec<_>>();

    let mut region_id_by_h3_res6 = HashMap::<CellIndex, String>::new();
    for region in &regions {
        if let Ok(cell) = region.cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                region_id_by_h3_res6.insert(cell, region.region_id.clone());
            }
        }
    }

    for i in 0..regions.len() {
        let region_id = regions[i].region_id.clone();
        let region_cell_id = regions[i].cell_id.clone();
        let region_x = regions[i].x;
        let region_y = regions[i].y;
        let mut adjacent_region_ids = Vec::<String>::new();
        let mut adjacency_source = "planning_res6_h3_disk_k1".to_string();

        if let Ok(cell) = region_cell_id.parse::<CellIndex>() {
            if cell.resolution() == Resolution::Six {
                for neighbor in cell.grid_disk::<Vec<_>>(1) {
                    if neighbor == cell {
                        continue;
                    }
                    if let Some(neighbor_region_id) = region_id_by_h3_res6.get(&neighbor) {
                        if neighbor_region_id != &region_id
                            && !adjacent_region_ids.contains(neighbor_region_id)
                        {
                            adjacent_region_ids.push(neighbor_region_id.clone());
                        }
                    }
                }
            }
        }

        if adjacent_region_ids.is_empty() {
            adjacent_region_ids =
                nearest_region_ids_by_xy(&regions, region_x, region_y, 6, Some(region_id.as_str()));
            adjacency_source = "planning_nearest_by_centroid".to_string();
        }
        regions[i].adjacent_region_ids = adjacent_region_ids;
        regions[i].adjacency_source = adjacency_source;
    }

    for region in &mut regions {
        let normalized = normalize_activity_mix([
            region.activity_mix_residential,
            region.activity_mix_office,
            region.activity_mix_retail,
            region.activity_mix_recreation,
            region.activity_mix_industrial,
            region.activity_mix_education,
            region.activity_mix_health,
        ]);
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        let mut region_id = cell
            .cell_id
            .parse::<CellIndex>()
            .ok()
            .and_then(|idx| idx.parent(Resolution::Six))
            .and_then(|parent| region_id_by_h3_res6.get(&parent).cloned());
        if region_id.is_none() {
            region_id = nearest_region_ids_by_xy(&regions, cell.x, cell.y, 1, None)
                .into_iter()
                .next();
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    // Stamp each substrate region with its canonical hex number so the
    // numbering is authoritative and travels with the region through the
    // entire pipeline (manual region construction, merge, frontend).
    {
        let lookup = build_substrate_hex_number_lookup_from_regions(&regions);
        for region in &mut regions {
            region.canonical_hex_number = lookup.get(&region.region_id).copied();
        }
    }

    let by_id = regions
        .iter()
        .map(|r| (r.region_id.clone(), r.clone()))
        .collect::<HashMap<_, _>>();

    SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
        legacy_region_aliases: HashMap::new(),
    }
}

const UK_PLANNING_REGION_TARGET: usize = 128;
const UK_PLANNING_REGION_MIN: usize = 100;
const UK_PLANNING_REGION_MAX: usize = 140;

#[derive(Debug, Clone, Deserialize)]
struct ManualPlanningRegionsFile {
    #[serde(default)]
    schema_version: Option<u32>,
    #[serde(default)]
    country_iso2: Option<String>,
    #[serde(default)]
    regions: Vec<ManualPlanningRegionWire>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManualPlanningRegionWire {
    name: String,
    #[serde(default)]
    region_token: Option<String>,
    #[serde(default)]
    region_id: Option<String>,
    #[serde(default)]
    hex_numbers: Vec<usize>,
}

#[derive(Debug, Clone)]
struct ManualPlanningRegionDefinition {
    region_id: String,
    region_token: String,
    name: String,
    hex_numbers: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct PlanningNameAnchor {
    name: &'static str,
    lon: f64,
    lat: f64,
}

fn uk_planning_name_anchors() -> &'static [PlanningNameAnchor] {
    &[
        PlanningNameAnchor {
            name: "London",
            lon: -0.1276,
            lat: 51.5072,
        },
        PlanningNameAnchor {
            name: "Birmingham",
            lon: -1.8904,
            lat: 52.4862,
        },
        PlanningNameAnchor {
            name: "Manchester",
            lon: -2.2426,
            lat: 53.4808,
        },
        PlanningNameAnchor {
            name: "Leeds",
            lon: -1.5491,
            lat: 53.8008,
        },
        PlanningNameAnchor {
            name: "Liverpool",
            lon: -2.9916,
            lat: 53.4084,
        },
        PlanningNameAnchor {
            name: "Newcastle",
            lon: -1.6178,
            lat: 54.9783,
        },
        PlanningNameAnchor {
            name: "Sheffield",
            lon: -1.4701,
            lat: 53.3811,
        },
        PlanningNameAnchor {
            name: "Nottingham",
            lon: -1.1492,
            lat: 52.9548,
        },
        PlanningNameAnchor {
            name: "Leicester",
            lon: -1.1398,
            lat: 52.6369,
        },
        PlanningNameAnchor {
            name: "Bristol",
            lon: -2.5879,
            lat: 51.4545,
        },
        PlanningNameAnchor {
            name: "Cardiff",
            lon: -3.1791,
            lat: 51.4816,
        },
        PlanningNameAnchor {
            name: "Southampton",
            lon: -1.4044,
            lat: 50.9097,
        },
        PlanningNameAnchor {
            name: "Portsmouth",
            lon: -1.0872,
            lat: 50.8198,
        },
        PlanningNameAnchor {
            name: "Norwich",
            lon: 1.2974,
            lat: 52.6309,
        },
        PlanningNameAnchor {
            name: "Plymouth",
            lon: -4.1427,
            lat: 50.3755,
        },
        PlanningNameAnchor {
            name: "Swansea",
            lon: -3.9436,
            lat: 51.6214,
        },
        PlanningNameAnchor {
            name: "Hull",
            lon: -0.3274,
            lat: 53.7676,
        },
        PlanningNameAnchor {
            name: "Glasgow",
            lon: -4.2518,
            lat: 55.8642,
        },
        PlanningNameAnchor {
            name: "Edinburgh",
            lon: -3.1883,
            lat: 55.9533,
        },
        PlanningNameAnchor {
            name: "Aberdeen",
            lon: -2.0943,
            lat: 57.1497,
        },
        PlanningNameAnchor {
            name: "Dundee",
            lon: -2.9707,
            lat: 56.4620,
        },
        PlanningNameAnchor {
            name: "Inverness",
            lon: -4.2247,
            lat: 57.4778,
        },
        PlanningNameAnchor {
            name: "Belfast",
            lon: -5.9301,
            lat: 54.5973,
        },
    ]
}

fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

fn planning_region_target_count(country_iso2: &str, substrate_count: usize) -> usize {
    if substrate_count <= 1 {
        return substrate_count;
    }
    if is_uk_country_iso2(country_iso2) {
        let floor = UK_PLANNING_REGION_MIN.min(substrate_count);
        let ceil = UK_PLANNING_REGION_MAX.min(substrate_count);
        return UK_PLANNING_REGION_TARGET.min(ceil).max(floor);
    }
    ((substrate_count as f64) / 60.0).round().clamp(32.0, 180.0) as usize
}

fn slugify_manual_region_token(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_dash = false;
    for ch in value.chars() {
        let ch = ch.to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
            continue;
        }
        if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn normalize_manual_region_definitions(
    country_iso2: &str,
    file: ManualPlanningRegionsFile,
) -> Vec<ManualPlanningRegionDefinition> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let mut out = Vec::<ManualPlanningRegionDefinition>::new();
    let mut used_region_ids = HashSet::<String>::new();
    for (idx, row) in file.regions.into_iter().enumerate() {
        let name = row.name.trim();
        if name.is_empty() || row.hex_numbers.is_empty() {
            continue;
        }
        let default_token = format!("manual-{}", idx + 1);
        let token = row
            .region_token
            .as_deref()
            .map(slugify_manual_region_token)
            .filter(|token| !token.is_empty())
            .or_else(|| {
                let slug = slugify_manual_region_token(name);
                if slug.is_empty() {
                    None
                } else {
                    Some(slug)
                }
            })
            .unwrap_or(default_token);
        let region_id = row
            .region_id
            .as_deref()
            .and_then(normalize_region_id)
            .unwrap_or_else(|| format!("{}:{}:{}", RegionIdTier::H3Res6.as_tier_tag(), iso, token));
        if !used_region_ids.insert(region_id.clone()) {
            continue;
        }
        let normalized_numbers = row
            .hex_numbers
            .into_iter()
            .filter(|number| *number > 0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if normalized_numbers.is_empty() {
            continue;
        }
        let region_token = parse_region_id(&region_id)
            .map(|parsed| parsed.token)
            .unwrap_or(token);
        out.push(ManualPlanningRegionDefinition {
            region_id,
            region_token,
            name: name.to_string(),
            hex_numbers: normalized_numbers,
        });
    }
    out
}

fn manual_region_definition_paths_for_iso(country_iso2: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("country_packs");
    country_iso2_runtime_candidates(country_iso2)
        .into_iter()
        .flat_map(|iso| {
            [
                root.join(&iso).join("manual_regions.json"),
                root.join(&iso).join("regions").join("manual_regions.json"),
            ]
        })
        .collect()
}

fn manual_region_candidate_paths_with_app(app: &AppHandle, iso: &str) -> Vec<PathBuf> {
    let mut candidate_paths = Vec::<PathBuf>::new();
    if let Some(pack_dir) = crate::commands::content_library::country_pack_dir(app, iso) {
        candidate_paths.push(pack_dir.join("manual_regions.json"));
        candidate_paths.push(pack_dir.join("regions").join("manual_regions.json"));
    }
    candidate_paths.extend(manual_region_definition_paths_for_iso(iso));
    candidate_paths.dedup();
    candidate_paths
}

fn load_manual_region_definitions_for_country(
    app: &AppHandle,
    country_iso2: &str,
) -> Result<Vec<ManualPlanningRegionDefinition>, String> {
    // Canonical manual edit point for authored planning regions:
    // data/country_packs/<ISO2>/manual_regions.json
    // Runtime reads managed pack first, then repo pack fallback.
    let Some(iso) = canonical_country_iso2(country_iso2) else {
        return Ok(Vec::new());
    };
    let candidate_paths = manual_region_candidate_paths_with_app(app, &iso);
    for path in candidate_paths {
        if !path.exists() {
            continue;
        }
        let file = read_json_file::<ManualPlanningRegionsFile>(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(file_iso) = file.country_iso2.as_deref() {
            if let Some(file_iso) = canonical_country_iso2(file_iso) {
                if file_iso != iso {
                    continue;
                }
            }
        }
        let _ = file.schema_version;
        return Ok(normalize_manual_region_definitions(&iso, file));
    }
    Ok(Vec::new())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RegionCatalogMemoKey {
    iso: String,
    surface_path: String,
    surface_fingerprint: u64,
    manual_definitions_fingerprint: u64,
}

fn file_mtime_fingerprint(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    match std::fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(modified) => {
            1_u8.hash(&mut hasher);
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                duration.as_secs().hash(&mut hasher);
                duration.subsec_nanos().hash(&mut hasher);
            } else {
                0_u64.hash(&mut hasher);
                0_u32.hash(&mut hasher);
            }
        }
        Err(_) => {
            0_u8.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn manual_definition_fingerprint(app: &AppHandle, iso: &str) -> u64 {
    // Fingerprint every candidate path in precedence order so managed-vs-repo
    // source changes invalidate safely without altering runtime semantics.
    let mut hasher = DefaultHasher::new();
    for path in manual_region_candidate_paths_with_app(app, iso) {
        path.to_string_lossy().hash(&mut hasher);
        match std::fs::metadata(&path).and_then(|meta| meta.modified()) {
            Ok(modified) => {
                1_u8.hash(&mut hasher);
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    duration.as_secs().hash(&mut hasher);
                    duration.subsec_nanos().hash(&mut hasher);
                } else {
                    0_u64.hash(&mut hasher);
                    0_u32.hash(&mut hasher);
                }
            }
            Err(_) => {
                0_u8.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn substrate_group_key_for_region(region: &SurfaceRegionInfo) -> String {
    region
        .cell_id
        .parse::<CellIndex>()
        .ok()
        .and_then(|cell| cell.parent(Resolution::Four))
        .map(|parent| format!("p4:{parent}"))
        .unwrap_or_else(|| format!("seed:{}", region.region_id))
}

fn group_score(
    members: &HashSet<String>,
    substrate_by_id: &HashMap<String, SurfaceRegionInfo>,
) -> f64 {
    members
        .iter()
        .filter_map(|rid| substrate_by_id.get(rid))
        .map(|r| r.residents_smooth.max(0.0) + r.jobs_smooth.max(0.0))
        .sum::<f64>()
}

fn group_centroid_xy(
    members: &HashSet<String>,
    substrate_by_id: &HashMap<String, SurfaceRegionInfo>,
) -> (f64, f64) {
    let mut wx = 0.0_f64;
    let mut wy = 0.0_f64;
    let mut total = 0.0_f64;
    for region in members.iter().filter_map(|rid| substrate_by_id.get(rid)) {
        let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
        total += weight;
        wx += region.x * weight;
        wy += region.y * weight;
    }
    if total <= 1e-9 {
        return (0.0, 0.0);
    }
    (wx / total, wy / total)
}

fn build_group_adjacency(
    group_members: &HashMap<String, HashSet<String>>,
    group_by_region: &HashMap<String, String>,
    substrate_by_id: &HashMap<String, SurfaceRegionInfo>,
) -> HashMap<String, HashSet<String>> {
    let mut adjacency = group_members
        .keys()
        .map(|key| (key.clone(), HashSet::<String>::new()))
        .collect::<HashMap<_, _>>();
    for (group_key, members) in group_members {
        for region_id in members {
            let Some(region) = substrate_by_id.get(region_id) else {
                continue;
            };
            for neighbor_id in &region.adjacent_region_ids {
                let Some(neighbor_group) = group_by_region.get(neighbor_id) else {
                    continue;
                };
                if neighbor_group == group_key {
                    continue;
                }
                adjacency
                    .entry(group_key.clone())
                    .or_default()
                    .insert(neighbor_group.clone());
            }
        }
    }
    adjacency
}

fn merge_groups_to_target(
    target_count: usize,
    group_members: &mut HashMap<String, HashSet<String>>,
    group_by_region: &mut HashMap<String, String>,
    substrate_by_id: &HashMap<String, SurfaceRegionInfo>,
) {
    while group_members.len() > target_count.max(1) {
        let source_key = group_members
            .iter()
            .min_by(|(a_key, a_members), (b_key, b_members)| {
                let a_score = group_score(a_members, substrate_by_id);
                let b_score = group_score(b_members, substrate_by_id);
                cmp_f64(a_score, b_score).then_with(|| a_key.cmp(b_key))
            })
            .map(|(key, _)| key.clone());
        let Some(source_key) = source_key else {
            break;
        };
        let Some(source_members) = group_members.get(&source_key).cloned() else {
            break;
        };
        let (source_x, source_y) = group_centroid_xy(&source_members, substrate_by_id);
        let adjacency = build_group_adjacency(group_members, group_by_region, substrate_by_id);
        let mut candidates = adjacency
            .get(&source_key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates = group_members
                .keys()
                .filter(|key| **key != source_key)
                .cloned()
                .collect::<Vec<_>>();
        }
        let target_key = candidates.into_iter().min_by(|a, b| {
            let a_members = group_members.get(a);
            let b_members = group_members.get(b);
            let (ax, ay) = a_members
                .map(|members| group_centroid_xy(members, substrate_by_id))
                .unwrap_or((0.0, 0.0));
            let (bx, by) = b_members
                .map(|members| group_centroid_xy(members, substrate_by_id))
                .unwrap_or((0.0, 0.0));
            let da = (source_x - ax).powi(2) + (source_y - ay).powi(2);
            let db = (source_x - bx).powi(2) + (source_y - by).powi(2);
            let a_score = a_members
                .map(|members| group_score(members, substrate_by_id))
                .unwrap_or(0.0);
            let b_score = b_members
                .map(|members| group_score(members, substrate_by_id))
                .unwrap_or(0.0);
            cmp_f64(da, db)
                .then_with(|| cmp_f64(b_score, a_score))
                .then_with(|| a.cmp(b))
        });
        let Some(target_key) = target_key else {
            break;
        };
        let Some(moved_members) = group_members.remove(&source_key) else {
            break;
        };
        let entry = group_members.entry(target_key.clone()).or_default();
        for member in moved_members {
            entry.insert(member.clone());
            group_by_region.insert(member, target_key.clone());
        }
    }
}

fn h3_hex_ring_lonlat(cell: CellIndex) -> Option<Vec<Vec<f64>>> {
    let boundary = cell.boundary();
    if boundary.is_empty() {
        return None;
    }
    let mut ring = boundary
        .iter()
        .map(|point| vec![point.lng(), point.lat()])
        .collect::<Vec<_>>();
    if ring.len() < 3 {
        return None;
    }
    if let Some(first) = ring.first().cloned() {
        let needs_close = ring
            .last()
            .map(|last| (last[0] - first[0]).abs() > 1e-9 || (last[1] - first[1]).abs() > 1e-9);
        if needs_close.unwrap_or(true) {
            ring.push(first);
        }
    }
    Some(ring)
}

fn planning_region_geometry_from_cells(
    cells: &[(CellIndex, usize)],
) -> (Option<JsonValue>, Vec<usize>) {
    let mut polygons = Vec::new();
    let mut canonical_numbers = Vec::new();

    for (cell, canonical_number) in cells {
        if let Some(ring) = h3_hex_ring_lonlat(*cell) {
            polygons.push(vec![ring]);
            canonical_numbers.push(*canonical_number);
        }
    }

    if polygons.is_empty() {
        return (None, Vec::new());
    }
    (
        serde_json::to_value(GeoJsonGeometry::new(GeoJsonValue::MultiPolygon(polygons))).ok(),
        canonical_numbers,
    )
}

fn compass_suffix(
    anchor_lon: f64,
    anchor_lat: f64,
    region_lon: f64,
    region_lat: f64,
) -> &'static str {
    let dx = region_lon - anchor_lon;
    let dy = region_lat - anchor_lat;
    if dx.abs() < 0.18 && dy.abs() < 0.18 {
        return "Central";
    }
    if dx.abs() >= dy.abs() * 1.6 {
        return if dx > 0.0 { "East" } else { "West" };
    }
    if dy.abs() >= dx.abs() * 1.6 {
        return if dy > 0.0 { "North" } else { "South" };
    }
    match (dy > 0.0, dx > 0.0) {
        (true, true) => "Northeast",
        (true, false) => "Northwest",
        (false, true) => "Southeast",
        (false, false) => "Southwest",
    }
}

fn apply_planning_region_names(country_iso2: &str, regions: &mut [SurfaceRegionInfo]) {
    if regions.is_empty() {
        return;
    }
    if !is_uk_country_iso2(country_iso2) {
        for (idx, region) in regions.iter_mut().enumerate() {
            region.name = format!("Planning Region {}", idx + 1);
        }
        return;
    }
    let anchors = uk_planning_name_anchors();
    let mut grouped = HashMap::<&'static str, Vec<(usize, f64, f64, f64)>>::new();
    for (idx, region) in regions.iter().enumerate() {
        let (lon, lat) = web_mercator_m_to_lonlat(region.x, region.y);
        let nearest = anchors.iter().min_by(|a, b| {
            let da = (lon - a.lon).powi(2) + (lat - a.lat).powi(2);
            let db = (lon - b.lon).powi(2) + (lat - b.lat).powi(2);
            cmp_f64(da, db)
        });
        let Some(anchor) = nearest else {
            continue;
        };
        let d2 = (lon - anchor.lon).powi(2) + (lat - anchor.lat).powi(2);
        grouped
            .entry(anchor.name)
            .or_default()
            .push((idx, d2, lon, lat));
    }

    let mut used_names = HashSet::<String>::new();
    for anchor in anchors {
        let Some(entries) = grouped.get_mut(anchor.name) else {
            continue;
        };
        entries.sort_by(|a, b| cmp_f64(a.1, b.1).then_with(|| a.0.cmp(&b.0)));
        for (rank, (idx, _d2, lon, lat)) in entries.iter().enumerate() {
            let mut candidate = if rank == 0 {
                anchor.name.to_string()
            } else {
                format!(
                    "{} {}",
                    anchor.name,
                    compass_suffix(anchor.lon, anchor.lat, *lon, *lat)
                )
            };
            if !used_names.insert(candidate.clone()) {
                let mut suffix = 2usize;
                loop {
                    let amended = format!("{candidate} {suffix}");
                    if used_names.insert(amended.clone()) {
                        candidate = amended;
                        break;
                    }
                    suffix += 1;
                }
            }
            if let Some(region) = regions.get_mut(*idx) {
                region.name = candidate;
            }
        }
    }

    for (idx, region) in regions.iter_mut().enumerate() {
        if region.name.trim().is_empty() {
            region.name = format!("Planning Region {}", idx + 1);
        }
    }
}

fn synthesize_planning_region_catalog(
    country_iso2: &str,
    substrate: SurfaceRegionCatalog,
) -> SurfaceRegionCatalog {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    if substrate.regions.is_empty() {
        return substrate;
    }

    let mut group_members = HashMap::<String, HashSet<String>>::new();
    let mut group_by_region = HashMap::<String, String>::new();
    for region in &substrate.regions {
        let group_key = substrate_group_key_for_region(region);
        group_members
            .entry(group_key.clone())
            .or_default()
            .insert(region.region_id.clone());
        group_by_region.insert(region.region_id.clone(), group_key);
    }

    let target_count = planning_region_target_count(&iso, substrate.regions.len());
    merge_groups_to_target(
        target_count,
        &mut group_members,
        &mut group_by_region,
        &substrate.by_id,
    );

    let mut grouped = group_members
        .into_iter()
        .map(|(group_key, members)| {
            let mut member_ids = members.into_iter().collect::<Vec<_>>();
            member_ids.sort();
            (group_key, member_ids)
        })
        .collect::<Vec<_>>();
    grouped.sort_by(|(a_key, a_members), (b_key, b_members)| {
        let a_members = a_members.iter().cloned().collect::<HashSet<_>>();
        let b_members = b_members.iter().cloned().collect::<HashSet<_>>();
        let (ax, ay) = group_centroid_xy(&a_members, &substrate.by_id);
        let (bx, by) = group_centroid_xy(&b_members, &substrate.by_id);
        cmp_f64(by, ay)
            .then_with(|| cmp_f64(ax, bx))
            .then_with(|| a_key.cmp(b_key))
    });

    let mut macro_region_id_by_group = HashMap::<String, String>::new();
    let mut regions = Vec::<SurfaceRegionInfo>::new();
    for (idx, (group_key, member_ids)) in grouped.iter().enumerate() {
        let region_token = format!("pr{:03}", idx + 1);
        let region_id = format!(
            "{}:{}:{}",
            RegionIdTier::H3Res6.as_tier_tag(),
            iso,
            region_token
        );
        macro_region_id_by_group.insert(group_key.clone(), region_id.clone());

        let mut weighted_total = 0.0_f64;
        let mut weighted_x = 0.0_f64;
        let mut weighted_y = 0.0_f64;
        let mut total_residents = 0.0_f64;
        let mut total_jobs = 0.0_f64;
        let mut total_area = 0.0_f64;
        let mut mix_sums = [0.0_f64; 7];
        let mut member_cells = Vec::<(CellIndex, usize)>::new();

        for member_id in member_ids {
            let Some(region) = substrate.by_id.get(member_id) else {
                continue;
            };
            if let Ok(cell) = region.cell_id.parse::<CellIndex>() {
                if cell.resolution() == Resolution::Six {
                    let canonical = region.canonical_hex_number.unwrap_or(0);
                    member_cells.push((cell, canonical));
                }
            }
            let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
            weighted_total += weight;
            weighted_x += region.x * weight;
            weighted_y += region.y * weight;
            total_residents += region.residents_smooth.max(0.0);
            total_jobs += region.jobs_smooth.max(0.0);
            total_area += region.area_m2.max(0.0);
            mix_sums[0] += region.activity_mix_residential.max(0.0) * weight;
            mix_sums[1] += region.activity_mix_office.max(0.0) * weight;
            mix_sums[2] += region.activity_mix_retail.max(0.0) * weight;
            mix_sums[3] += region.activity_mix_recreation.max(0.0) * weight;
            mix_sums[4] += region.activity_mix_industrial.max(0.0) * weight;
            mix_sums[5] += region.activity_mix_education.max(0.0) * weight;
            mix_sums[6] += region.activity_mix_health.max(0.0) * weight;
        }
        let normalized_mix = normalize_activity_mix([
            mix_sums[0] / weighted_total.max(1e-9),
            mix_sums[1] / weighted_total.max(1e-9),
            mix_sums[2] / weighted_total.max(1e-9),
            mix_sums[3] / weighted_total.max(1e-9),
            mix_sums[4] / weighted_total.max(1e-9),
            mix_sums[5] / weighted_total.max(1e-9),
            mix_sums[6] / weighted_total.max(1e-9),
        ]);

        let (geometry, constituent_hex_numbers) =
            planning_region_geometry_from_cells(&member_cells);

        regions.push(SurfaceRegionInfo {
            region_id,
            country_iso2: iso.clone(),
            region_kind: "strategic_planning_region".to_string(),
            region_token: region_token.clone(),
            h3_cell_id: None,
            name: String::new(),
            admin_level: "planning_region_v2".to_string(),
            nation: None,
            source_code: Some("planning_region_synthesis_v2".to_string()),
            adjacency_source: "planning_region_res4_merged_adjacency".to_string(),
            geometry_source: "planning_region_res6_multipolygon".to_string(),
            cell_id: region_token,
            x: weighted_x / weighted_total.max(1e-9),
            y: weighted_y / weighted_total.max(1e-9),
            area_m2: total_area,
            residents_smooth: total_residents,
            jobs_smooth: total_jobs,
            activity_mix_residential: normalized_mix[0],
            activity_mix_office: normalized_mix[1],
            activity_mix_retail: normalized_mix[2],
            activity_mix_recreation: normalized_mix[3],
            activity_mix_industrial: normalized_mix[4],
            activity_mix_education: normalized_mix[5],
            activity_mix_health: normalized_mix[6],
            adjacent_region_ids: Vec::new(),
            geometry,
            canonical_hex_number: None,
            constituent_hex_numbers,
        });
    }

    let macro_adjacency_by_group = build_group_adjacency(
        &{
            grouped
                .iter()
                .map(|(group_key, members)| {
                    (
                        group_key.clone(),
                        members.iter().cloned().collect::<HashSet<_>>(),
                    )
                })
                .collect::<HashMap<_, _>>()
        },
        &group_by_region,
        &substrate.by_id,
    );
    let region_index_by_id = regions
        .iter()
        .enumerate()
        .map(|(idx, region)| (region.region_id.clone(), idx))
        .collect::<HashMap<_, _>>();
    for (group_key, neighbors) in macro_adjacency_by_group {
        let Some(region_id) = macro_region_id_by_group.get(&group_key).cloned() else {
            continue;
        };
        let Some(region_idx) = region_index_by_id.get(&region_id).copied() else {
            continue;
        };
        let mut adjacent = neighbors
            .iter()
            .filter_map(|neighbor_key| macro_region_id_by_group.get(neighbor_key))
            .filter(|neighbor_id| *neighbor_id != &region_id)
            .cloned()
            .collect::<Vec<_>>();
        adjacent.sort();
        adjacent.dedup();
        if adjacent.is_empty() {
            adjacent = nearest_region_ids_by_xy(
                &regions,
                regions[region_idx].x,
                regions[region_idx].y,
                6,
                Some(region_id.as_str()),
            );
        }
        regions[region_idx].adjacent_region_ids = adjacent;
    }

    apply_planning_region_names(&iso, &mut regions);

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for (substrate_region_id, cells) in substrate.cells_res8_by_region {
        let Some(group_key) = group_by_region.get(&substrate_region_id) else {
            continue;
        };
        let Some(macro_region_id) = macro_region_id_by_group.get(group_key) else {
            continue;
        };
        cells_res8_by_region
            .entry(macro_region_id.clone())
            .or_default()
            .extend(cells);
    }

    let mut legacy_region_aliases = HashMap::<String, String>::new();
    for (substrate_region_id, group_key) in group_by_region {
        let Some(macro_region_id) = macro_region_id_by_group.get(&group_key) else {
            continue;
        };
        legacy_region_aliases.insert(substrate_region_id, macro_region_id.clone());
    }

    regions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    let by_id = regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
        legacy_region_aliases,
    }
}

/// Core hex numbering logic factored into a helper that works on a slice of
/// `SurfaceRegionInfo` directly.  This is used both for the public
/// `build_substrate_hex_number_lookup` (catalog-level) and for stamping
/// canonical numbers onto substrate regions at construction time.
fn build_substrate_hex_number_lookup_from_regions(
    regions: &[SurfaceRegionInfo],
) -> HashMap<String, usize> {
    let mut primary_cells = Vec::<(String, String)>::new();
    let mut backfill_cells_legacy = Vec::<(String, String)>::new();
    let mut backfill_cells_extension = Vec::<(String, String)>::new();
    for region in regions {
        let row = (
            region.cell_id.to_ascii_lowercase(),
            region.region_id.clone(),
        );
        let source_code = region
            .source_code
            .as_deref()
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if source_code == UK_LAND_BACKFILL_SOURCE_CODE {
            backfill_cells_legacy.push(row);
        } else if source_code == UK_LAND_BACKFILL_NI_SOURCE_CODE
            || source_code.starts_with("uk_land_backfill_res6")
        {
            backfill_cells_extension.push(row);
        } else {
            primary_cells.push(row);
        }
    }
    primary_cells.sort_by(|a, b| a.0.cmp(&b.0));
    backfill_cells_legacy.sort_by(|a, b| a.0.cmp(&b.0));
    backfill_cells_extension.sort_by(|a, b| a.0.cmp(&b.0));

    let mut by_region_id = HashMap::<String, usize>::new();
    for (idx, (_cell_id, region_id)) in primary_cells
        .into_iter()
        .chain(backfill_cells_legacy.into_iter())
        .chain(backfill_cells_extension.into_iter())
        .enumerate()
    {
        by_region_id.insert(region_id, idx + 1);
    }
    by_region_id
}

fn build_substrate_hex_number_lookup(
    substrate: &SurfaceRegionCatalog,
) -> (HashMap<usize, String>, HashMap<String, usize>) {
    let by_region_id = build_substrate_hex_number_lookup_from_regions(&substrate.regions);
    let mut by_number = HashMap::<usize, String>::new();
    for (region_id, number) in &by_region_id {
        by_number.insert(*number, region_id.clone());
    }
    (by_number, by_region_id)
}

fn build_manual_planning_region_catalog(
    country_iso2: &str,
    substrate: &SurfaceRegionCatalog,
    synthesized: &SurfaceRegionCatalog,
    manual_definitions: &[ManualPlanningRegionDefinition],
) -> SurfaceRegionCatalog {
    if manual_definitions.is_empty() {
        return synthesized.clone();
    }
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let (hex_number_to_substrate_region, substrate_region_to_hex_number) =
        build_substrate_hex_number_lookup(substrate);

    let mut final_region_id_by_substrate = HashMap::<String, String>::new();
    let mut manual_region_by_id = HashMap::<String, ManualPlanningRegionDefinition>::new();

    for definition in manual_definitions {
        let mut assigned_any = false;
        for hex_number in &definition.hex_numbers {
            let Some(substrate_region_id) = hex_number_to_substrate_region.get(hex_number).cloned()
            else {
                eprintln!(
                    "[manual-region-validation] WARNING: hex_number {} in region '{}' ({}) does not resolve to any substrate region — skipped",
                    hex_number, definition.name, definition.region_id
                );
                continue;
            };
            if final_region_id_by_substrate.contains_key(&substrate_region_id) {
                eprintln!(
                    "[manual-region-validation] WARNING: hex_number {} (substrate {}) is already assigned to region '{}' — duplicate in region '{}' skipped",
                    hex_number, substrate_region_id,
                    final_region_id_by_substrate.get(&substrate_region_id).unwrap_or(&String::new()),
                    definition.name
                );
                continue;
            }
            final_region_id_by_substrate.insert(substrate_region_id, definition.region_id.clone());
            assigned_any = true;
        }
        if assigned_any {
            manual_region_by_id.insert(definition.region_id.clone(), definition.clone());
        }
    }

    if manual_region_by_id.is_empty() {
        return synthesized.clone();
    }

    for substrate_region_id in substrate.by_id.keys() {
        final_region_id_by_substrate
            .entry(substrate_region_id.clone())
            .or_insert_with(|| substrate_region_id.clone());
    }

    let mut group_members = HashMap::<String, HashSet<String>>::new();
    for (substrate_region_id, final_region_id) in &final_region_id_by_substrate {
        group_members
            .entry(final_region_id.clone())
            .or_default()
            .insert(substrate_region_id.clone());
    }

    let mut regions = Vec::<SurfaceRegionInfo>::new();
    for (final_region_id, members) in &group_members {
        let Some(example_member) = members.iter().next() else {
            continue;
        };
        let Some(example_region) = substrate.by_id.get(example_member) else {
            continue;
        };
        if let Some(definition) = manual_region_by_id.get(final_region_id) {
            let mut weighted_total = 0.0_f64;
            let mut weighted_x = 0.0_f64;
            let mut weighted_y = 0.0_f64;
            let mut total_residents = 0.0_f64;
            let mut total_jobs = 0.0_f64;
            let mut total_area = 0.0_f64;
            let mut mix_sums = [0.0_f64; 7];
            let mut member_cells = Vec::<(CellIndex, usize)>::new();

            for member_id in members {
                let Some(region) = substrate.by_id.get(member_id) else {
                    continue;
                };
                if let Ok(cell) = region.cell_id.parse::<CellIndex>() {
                    if cell.resolution() == Resolution::Six {
                        let canonical = substrate_region_to_hex_number
                            .get(member_id)
                            .copied()
                            .unwrap_or(0);
                        member_cells.push((cell, canonical));
                    }
                }
                let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
                weighted_total += weight;
                weighted_x += region.x * weight;
                weighted_y += region.y * weight;
                total_residents += region.residents_smooth.max(0.0);
                total_jobs += region.jobs_smooth.max(0.0);
                total_area += region.area_m2.max(0.0);
                mix_sums[0] += region.activity_mix_residential.max(0.0) * weight;
                mix_sums[1] += region.activity_mix_office.max(0.0) * weight;
                mix_sums[2] += region.activity_mix_retail.max(0.0) * weight;
                mix_sums[3] += region.activity_mix_recreation.max(0.0) * weight;
                mix_sums[4] += region.activity_mix_industrial.max(0.0) * weight;
                mix_sums[5] += region.activity_mix_education.max(0.0) * weight;
                mix_sums[6] += region.activity_mix_health.max(0.0) * weight;
            }
            let normalized_mix = normalize_activity_mix([
                mix_sums[0] / weighted_total.max(1e-9),
                mix_sums[1] / weighted_total.max(1e-9),
                mix_sums[2] / weighted_total.max(1e-9),
                mix_sums[3] / weighted_total.max(1e-9),
                mix_sums[4] / weighted_total.max(1e-9),
                mix_sums[5] / weighted_total.max(1e-9),
                mix_sums[6] / weighted_total.max(1e-9),
            ]);
            let (geometry, constituent_hex_numbers) =
                planning_region_geometry_from_cells(&member_cells);

            regions.push(SurfaceRegionInfo {
                region_id: definition.region_id.clone(),
                country_iso2: iso.clone(),
                region_kind: "strategic_planning_region".to_string(),
                region_token: definition.region_token.clone(),
                h3_cell_id: None,
                name: definition.name.clone(),
                admin_level: "planning_region_manual_v1".to_string(),
                nation: None,
                source_code: Some("manual_region_definition_v1".to_string()),
                adjacency_source: "manual_region_assignment_res6_adjacency".to_string(),
                geometry_source: "manual_region_res6_multipolygon".to_string(),
                cell_id: definition.region_token.clone(),
                x: weighted_x / weighted_total.max(1e-9),
                y: weighted_y / weighted_total.max(1e-9),
                area_m2: total_area,
                residents_smooth: total_residents,
                jobs_smooth: total_jobs,
                activity_mix_residential: normalized_mix[0],
                activity_mix_office: normalized_mix[1],
                activity_mix_retail: normalized_mix[2],
                activity_mix_recreation: normalized_mix[3],
                activity_mix_industrial: normalized_mix[4],
                activity_mix_education: normalized_mix[5],
                activity_mix_health: normalized_mix[6],
                adjacent_region_ids: Vec::new(),
                geometry,
                canonical_hex_number: None,
                constituent_hex_numbers,
            });
            continue;
        }

        let mut region = example_region.clone();
        let hex_number = substrate_region_to_hex_number
            .get(example_member)
            .copied()
            .unwrap_or_default();
        region.region_kind = "planning_hex_unassigned".to_string();
        region.name = format!("Hex #{hex_number}");
        region.admin_level = "planning_hex_res6".to_string();
        region.source_code = Some("manual_region_unassigned_hex".to_string());
        region.adjacency_source = "planning_res6_h3_disk_k1".to_string();
        region.geometry_source = "planning_surface_res6".to_string();
        region.adjacent_region_ids.clear();
        region.geometry = None;
        // Preserve the backend-authoritative hex number on unassigned hex regions
        // so the frontend can display the same number the backend uses.
        region.canonical_hex_number = Some(hex_number);
        region.constituent_hex_numbers = if hex_number > 0 {
            vec![hex_number]
        } else {
            vec![]
        };
        regions.push(region);
    }

    let adjacency = build_group_adjacency(
        &group_members,
        &final_region_id_by_substrate,
        &substrate.by_id,
    );
    let region_index_by_id = regions
        .iter()
        .enumerate()
        .map(|(idx, region)| (region.region_id.clone(), idx))
        .collect::<HashMap<_, _>>();
    for (region_id, neighbors) in adjacency {
        let Some(region_idx) = region_index_by_id.get(&region_id).copied() else {
            continue;
        };
        let mut adjacent = neighbors.into_iter().collect::<Vec<_>>();
        adjacent.sort();
        adjacent.dedup();
        if adjacent.is_empty() {
            adjacent = nearest_region_ids_by_xy(
                &regions,
                regions[region_idx].x,
                regions[region_idx].y,
                6,
                Some(region_id.as_str()),
            );
        }
        regions[region_idx].adjacent_region_ids = adjacent;
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for (substrate_region_id, cells) in &substrate.cells_res8_by_region {
        let final_region_id = final_region_id_by_substrate
            .get(substrate_region_id)
            .cloned()
            .unwrap_or_else(|| substrate_region_id.clone());
        cells_res8_by_region
            .entry(final_region_id)
            .or_default()
            .extend(cells.clone());
    }

    let mut synth_to_final_votes = HashMap::<String, HashMap<String, usize>>::new();
    for (substrate_region_id, synth_region_id) in &synthesized.legacy_region_aliases {
        let final_region_id = final_region_id_by_substrate
            .get(substrate_region_id)
            .cloned()
            .unwrap_or_else(|| substrate_region_id.clone());
        *synth_to_final_votes
            .entry(synth_region_id.clone())
            .or_default()
            .entry(final_region_id)
            .or_insert(0) += 1;
    }

    let mut legacy_region_aliases = HashMap::<String, String>::new();
    for (substrate_region_id, final_region_id) in &final_region_id_by_substrate {
        if substrate_region_id != final_region_id {
            legacy_region_aliases.insert(substrate_region_id.clone(), final_region_id.clone());
        }
    }
    for (synth_region_id, votes) in synth_to_final_votes {
        let Some((target_region_id, _)) = votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
        else {
            continue;
        };
        if synth_region_id != target_region_id {
            legacy_region_aliases.insert(synth_region_id, target_region_id);
        }
    }

    regions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    let by_id = regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
        legacy_region_aliases,
    }
}

pub(crate) fn build_surface_region_catalog(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> SurfaceRegionCatalog {
    // Two-layer model:
    // 1) res6 demand substrate remains the fine-grain simulation basis.
    // 2) synthesized planning regions are the player-facing progression geography.
    let substrate = build_substrate_region_catalog(country_iso2, surface);
    synthesize_planning_region_catalog(country_iso2, substrate)
}

fn build_surface_region_catalog_with_manual(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
    manual_definitions: &[ManualPlanningRegionDefinition],
) -> SurfaceRegionCatalog {
    let substrate = build_substrate_region_catalog(country_iso2, surface);
    let synthesized = synthesize_planning_region_catalog(country_iso2, substrate.clone());
    build_manual_planning_region_catalog(country_iso2, &substrate, &synthesized, manual_definitions)
}

pub(crate) fn canonical_region_for_catalog(
    catalog: &SurfaceRegionCatalog,
    region_id: &str,
) -> Option<String> {
    let normalized = canonicalize_region_id(region_id).unwrap_or_else(|| region_id.to_string());
    if catalog.by_id.contains_key(&normalized) {
        return Some(normalized);
    }
    catalog.legacy_region_aliases.get(&normalized).cloned()
}

fn region_from_start_lonlat(
    catalog: &SurfaceRegionCatalog,
    country_iso2: &str,
    lon: f64,
    lat: f64,
) -> Option<String> {
    let ll = h3o::LatLng::new(lat, lon).ok()?;
    let res6 = ll.to_cell(Resolution::Six).to_string();
    let legacy_region_id = region_id_from_res6(country_iso2, &res6);
    canonical_region_for_catalog(catalog, &legacy_region_id).or_else(|| {
        let (sx, sy) = lonlat_to_web_mercator_m(lon, lat);
        nearest_region_ids_by_xy(&catalog.regions, sx, sy, 1, None)
            .into_iter()
            .next()
    })
}

pub(crate) fn merge_surface_region_catalog_aliases(
    catalog: SurfaceRegionCatalog,
) -> SurfaceRegionCatalog {
    if catalog.regions.is_empty() {
        return catalog;
    }

    let mut canonical_by_region = HashMap::<String, String>::new();
    for region in &catalog.regions {
        let canonical =
            canonicalize_region_id(&region.region_id).unwrap_or_else(|| region.region_id.clone());
        canonical_by_region.insert(region.region_id.clone(), canonical);
    }

    let canonical_for = |region_id: &str, lookup: &HashMap<String, String>| {
        lookup
            .get(region_id)
            .cloned()
            .or_else(|| canonicalize_region_id(region_id))
            .unwrap_or_else(|| region_id.to_string())
    };

    let SurfaceRegionCatalog {
        regions: catalog_regions,
        by_id: _catalog_by_id,
        cells_res8_by_region: catalog_cells_res8_by_region,
        legacy_region_aliases: catalog_legacy_region_aliases,
    } = catalog;

    let mut grouped = HashMap::<String, Vec<SurfaceRegionInfo>>::new();
    for mut region in catalog_regions {
        let canonical = canonical_for(&region.region_id, &canonical_by_region);
        region.adjacent_region_ids = region
            .adjacent_region_ids
            .iter()
            .map(|neighbor| canonical_for(neighbor, &canonical_by_region))
            .filter(|neighbor| neighbor != &canonical)
            .collect();
        region.region_id = canonical.clone();
        grouped.entry(canonical).or_default().push(region);
    }

    let mut merged_regions = Vec::<SurfaceRegionInfo>::new();
    for (canonical_region_id, mut group) in grouped {
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| {
            let a_score = a.area_m2.max(a.residents_smooth + a.jobs_smooth);
            let b_score = b.area_m2.max(b.residents_smooth + b.jobs_smooth);
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut merged = group[0].clone();
        merged.region_id = canonical_region_id.clone();
        let mut adjacency = merged.adjacent_region_ids.clone();

        let mut weighted_total = 0.0_f64;
        let mut weighted_x = 0.0_f64;
        let mut weighted_y = 0.0_f64;
        let mut mix_sums = [0.0_f64; 7];
        let mut total_residents = 0.0_f64;
        let mut total_jobs = 0.0_f64;
        let mut total_area = 0.0_f64;

        for region in group {
            let weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
            weighted_total += weight;
            weighted_x += region.x * weight;
            weighted_y += region.y * weight;
            total_residents += region.residents_smooth.max(0.0);
            total_jobs += region.jobs_smooth.max(0.0);
            total_area += region.area_m2.max(0.0);
            mix_sums[0] += region.activity_mix_residential.max(0.0) * weight;
            mix_sums[1] += region.activity_mix_office.max(0.0) * weight;
            mix_sums[2] += region.activity_mix_retail.max(0.0) * weight;
            mix_sums[3] += region.activity_mix_recreation.max(0.0) * weight;
            mix_sums[4] += region.activity_mix_industrial.max(0.0) * weight;
            mix_sums[5] += region.activity_mix_education.max(0.0) * weight;
            mix_sums[6] += region.activity_mix_health.max(0.0) * weight;
            adjacency.extend(region.adjacent_region_ids.clone());
        }

        if weighted_total > 0.0 {
            merged.x = weighted_x / weighted_total;
            merged.y = weighted_y / weighted_total;
        }
        merged.residents_smooth = total_residents;
        merged.jobs_smooth = total_jobs;
        merged.area_m2 = total_area;
        let normalized_mix = normalize_activity_mix([
            mix_sums[0] / weighted_total.max(1e-9),
            mix_sums[1] / weighted_total.max(1e-9),
            mix_sums[2] / weighted_total.max(1e-9),
            mix_sums[3] / weighted_total.max(1e-9),
            mix_sums[4] / weighted_total.max(1e-9),
            mix_sums[5] / weighted_total.max(1e-9),
            mix_sums[6] / weighted_total.max(1e-9),
        ]);
        merged.activity_mix_residential = normalized_mix[0];
        merged.activity_mix_office = normalized_mix[1];
        merged.activity_mix_retail = normalized_mix[2];
        merged.activity_mix_recreation = normalized_mix[3];
        merged.activity_mix_industrial = normalized_mix[4];
        merged.activity_mix_education = normalized_mix[5];
        merged.activity_mix_health = normalized_mix[6];
        merged.adjacent_region_ids = adjacency;
        merged_regions.push(merged);
    }

    let valid_region_ids = merged_regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut merged_regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid) && rid != &region.region_id);
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }

    let mut merged_cells = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for (region_id, cells) in catalog_cells_res8_by_region {
        let canonical = canonical_for(&region_id, &canonical_by_region);
        merged_cells.entry(canonical).or_default().extend(cells);
    }

    let mut merged_aliases = HashMap::<String, String>::new();
    for (region_id, canonical_region_id) in &canonical_by_region {
        if region_id != canonical_region_id {
            merged_aliases.insert(region_id.clone(), canonical_region_id.clone());
        }
    }
    for (legacy_id, target_id) in catalog_legacy_region_aliases {
        let canonical_legacy = canonical_for(&legacy_id, &canonical_by_region);
        let canonical_target = canonical_for(&target_id, &canonical_by_region);
        if canonical_legacy != canonical_target {
            merged_aliases.insert(canonical_legacy, canonical_target);
        }
    }

    merged_regions.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.region_id.cmp(&b.region_id))
    });
    let by_id = merged_regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    SurfaceRegionCatalog {
        regions: merged_regions,
        by_id,
        cells_res8_by_region: merged_cells,
        legacy_region_aliases: merged_aliases,
    }
}

pub(crate) fn build_region_catalog_for_surface(
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    // Authoritative runtime provider: Interlinked strategic planning regions generated from
    // standardized demand surface cells (res6 regions with res8 assignments).
    // Legacy county builders remain compatibility-only and are not the default runtime model.
    Ok(merge_surface_region_catalog_aliases(
        build_surface_region_catalog(&iso, surface),
    ))
}

pub(crate) fn build_region_catalog_for_surface_with_app(
    app: &AppHandle,
    country_iso2: &str,
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let build_started = Instant::now();
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let manual_defs_started = Instant::now();
    let manual_definitions = load_manual_region_definitions_for_country(app, &iso)?;
    perf_log(
        "build_region_catalog_for_surface_with_app.load_manual_definitions",
        manual_defs_started,
    );
    let synth_started = Instant::now();
    let catalog = if manual_definitions.is_empty() {
        build_surface_region_catalog(&iso, surface)
    } else {
        build_surface_region_catalog_with_manual(&iso, surface, &manual_definitions)
    };
    perf_log(
        "build_region_catalog_for_surface_with_app.build_surface_catalog",
        synth_started,
    );
    let merge_started = Instant::now();
    let merged = merge_surface_region_catalog_aliases(catalog);
    perf_log(
        "build_region_catalog_for_surface_with_app.merge_aliases",
        merge_started,
    );
    perf_log(
        "build_region_catalog_for_surface_with_app.total",
        build_started,
    );
    Ok(merged)
}

pub(crate) fn build_gb_county_region_catalog(
    surface: &DemandSurfaceCountryWire,
) -> Result<SurfaceRegionCatalog, String> {
    let county_catalog = load_gb_county_boundaries()?;
    let counties = county_catalog.counties;
    if counties.is_empty() {
        return Err("no legacy UK county boundaries available".to_string());
    }

    let mut regions = counties
        .iter()
        .map(|county| SurfaceRegionInfo {
            region_id: region_id_from_county(CANONICAL_UK_ISO2, &county.county_id),
            country_iso2: canonical_country_iso2(&county.country_iso2)
                .unwrap_or_else(|| county.country_iso2.clone()),
            region_kind: "legacy_administrative_county".to_string(),
            region_token: county.county_id.clone(),
            h3_cell_id: None,
            name: county.name.clone(),
            admin_level: "legacy_uk_county".to_string(),
            nation: Some(county.nation.clone()),
            source_code: Some(county.source_code.clone()),
            adjacency_source: "legacy_county_boundary_touch".to_string(),
            geometry_source: "legacy_county_boundary_catalog".to_string(),
            cell_id: county.county_id.clone(),
            x: 0.0,
            y: 0.0,
            area_m2: 0.0,
            residents_smooth: 0.0,
            jobs_smooth: 0.0,
            activity_mix_residential: 0.0,
            activity_mix_office: 0.0,
            activity_mix_retail: 0.0,
            activity_mix_recreation: 0.0,
            activity_mix_industrial: 0.0,
            activity_mix_education: 0.0,
            activity_mix_health: 0.0,
            adjacent_region_ids: vec![],
            geometry: Some(county.geometry_json.clone()),
            canonical_hex_number: None,
            constituent_hex_numbers: vec![],
        })
        .collect::<Vec<_>>();
    let county_index = counties
        .iter()
        .enumerate()
        .map(|(idx, county)| (county.county_id.clone(), idx))
        .collect::<HashMap<_, _>>();
    let adjacency_map = gb_county_adjacency_map(&counties);
    let mut res6_owner = HashMap::<String, usize>::new();

    for cell in &surface.cells_res6 {
        let county = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat));
        let Some(county) = county else { continue };
        let Some(&idx) = county_index.get(&county.county_id) else {
            continue;
        };
        res6_owner.insert(cell.cell_id.clone(), idx);
        let weight = (cell.residents_smooth + cell.jobs_smooth).max(1.0);
        let region = &mut regions[idx];
        region.area_m2 += cell.area_m2.max(0.0);
        region.residents_smooth += cell.residents_smooth.max(0.0);
        region.jobs_smooth += cell.jobs_smooth.max(0.0);
        region.x += cell.x * weight;
        region.y += cell.y * weight;
    }

    for region in &mut regions {
        let total_weight = (region.residents_smooth + region.jobs_smooth).max(1.0);
        if total_weight > 0.0 {
            region.x /= total_weight;
            region.y /= total_weight;
        } else if let Some(county) = county_index
            .get(&region.cell_id)
            .and_then(|idx| counties.get(*idx))
        {
            let (x, y) = lonlat_to_web_mercator_m(county.bbox_center_lon, county.bbox_center_lat);
            region.x = x;
            region.y = y;
        }
    }

    for region in &mut regions {
        region.adjacent_region_ids = adjacency_map
            .get(&region.region_id)
            .cloned()
            .unwrap_or_default();
    }

    let mut cells_res8_by_region = HashMap::<String, Vec<DemandSurfaceCellWire>>::new();
    for cell in &surface.cells_res8 {
        // Assign res8 cells by actual county geometry first.
        // Parent-res6 ownership can smear small counties and blur city-center detail.
        let mut region_id = county_for_lon_lat(&counties, cell.lon, cell.lat)
            .or_else(|| nearest_county_for_lon_lat(&counties, cell.lon, cell.lat))
            .and_then(|county| county_index.get(&county.county_id).copied())
            .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        if region_id.is_none() {
            region_id = cell
                .cell_id
                .parse::<CellIndex>()
                .ok()
                .and_then(|idx| idx.parent(Resolution::Six))
                .and_then(|parent| res6_owner.get(&parent.to_string()).copied())
                .and_then(|idx| regions.get(idx).map(|region| region.region_id.clone()));
        }
        if let Some(region_id) = region_id {
            cells_res8_by_region
                .entry(region_id)
                .or_default()
                .push(cell.clone());
        }
    }

    for region in &mut regions {
        let normalized = if let Some(cells) = cells_res8_by_region.get(&region.region_id) {
            let mut w_sum = 0.0_f64;
            let mut r_sum = 0.0_f64;
            let mut o_sum = 0.0_f64;
            let mut rt_sum = 0.0_f64;
            let mut rc_sum = 0.0_f64;
            let mut i_sum = 0.0_f64;
            let mut e_sum = 0.0_f64;
            let mut h_sum = 0.0_f64;
            for c in cells {
                let w = (c.residents_smooth + c.jobs_smooth).max(1e-6);
                w_sum += w;
                r_sum += c.activity_mix_residential.max(0.0) * w;
                o_sum += c.activity_mix_office.max(0.0) * w;
                rt_sum += c.activity_mix_retail.max(0.0) * w;
                rc_sum += c.activity_mix_recreation.max(0.0) * w;
                i_sum += c.activity_mix_industrial.max(0.0) * w;
                e_sum += c.activity_mix_education.max(0.0) * w;
                h_sum += c.activity_mix_health.max(0.0) * w;
            }
            let denom = w_sum.max(1e-9);
            normalize_activity_mix([
                r_sum / denom,
                o_sum / denom,
                rt_sum / denom,
                rc_sum / denom,
                i_sum / denom,
                e_sum / denom,
                h_sum / denom,
            ])
        } else {
            normalize_activity_mix([
                region.activity_mix_residential,
                region.activity_mix_office,
                region.activity_mix_retail,
                region.activity_mix_recreation,
                region.activity_mix_industrial,
                region.activity_mix_education,
                region.activity_mix_health,
            ])
        };
        region.activity_mix_residential = normalized[0];
        region.activity_mix_office = normalized[1];
        region.activity_mix_retail = normalized[2];
        region.activity_mix_recreation = normalized[3];
        region.activity_mix_industrial = normalized[4];
        region.activity_mix_education = normalized[5];
        region.activity_mix_health = normalized[6];
    }

    let valid_region_ids = regions
        .iter()
        .map(|region| region.region_id.clone())
        .collect::<HashSet<_>>();
    for region in &mut regions {
        region
            .adjacent_region_ids
            .retain(|rid| valid_region_ids.contains(rid));
        region.adjacent_region_ids.sort();
        region.adjacent_region_ids.dedup();
    }
    let by_id = regions
        .iter()
        .map(|region| (region.region_id.clone(), region.clone()))
        .collect::<HashMap<_, _>>();
    Ok(SurfaceRegionCatalog {
        regions,
        by_id,
        cells_res8_by_region,
        legacy_region_aliases: HashMap::new(),
    })
}

pub(crate) fn nearest_region_for_start(
    catalog: &SurfaceRegionCatalog,
    start: Option<&StartLocation>,
    country_iso2: &str,
) -> Option<String> {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let Some(s) = start.filter(|x| x.country_iso2.eq_ignore_ascii_case(&iso)) else {
        return catalog
            .regions
            .iter()
            .max_by(|a, b| {
                (a.residents_smooth + a.jobs_smooth)
                    .partial_cmp(&(b.residents_smooth + b.jobs_smooth))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.region_id.clone());
    };
    region_from_start_lonlat(catalog, &iso, s.city_lon, s.city_lat)
}

pub(crate) fn load_region_catalog_for_country(
    app: &AppHandle,
    country_iso2: &str,
) -> Result<Option<SurfaceRegionCatalog>, String> {
    let load_started = Instant::now();
    static REGION_CATALOG_MEMO: OnceLock<
        Mutex<HashMap<RegionCatalogMemoKey, SurfaceRegionCatalog>>,
    > = OnceLock::new();
    let Some(iso) = canonical_country_iso2(country_iso2) else {
        return Ok(None);
    };
    let Some(resolved_surface) =
        crate::commands::content_library::resolve_demand_surface_path(app, &iso)
    else {
        return Ok(None);
    };
    let memo_key = RegionCatalogMemoKey {
        iso: iso.clone(),
        surface_path: resolved_surface.path.to_string_lossy().to_string(),
        surface_fingerprint: file_mtime_fingerprint(&resolved_surface.path),
        manual_definitions_fingerprint: manual_definition_fingerprint(app, &iso),
    };
    let cache = REGION_CATALOG_MEMO.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&memo_key) {
            eprintln!(
                "[perf] load_region_catalog_for_country.cache_hit iso={} source={}",
                iso,
                resolved_surface.source.as_str()
            );
            perf_log("load_region_catalog_for_country.cache_hit", load_started);
            return Ok(Some(cached.clone()));
        }
    }
    eprintln!(
        "[perf] load_region_catalog_for_country.cache_miss_rebuild iso={} source={}",
        iso,
        resolved_surface.source.as_str()
    );
    let load_surface_started = Instant::now();
    let surface = load_surface_wire(&resolved_surface.path)?;
    perf_log(
        "load_region_catalog_for_country.load_surface_wire",
        load_surface_started,
    );
    let build_catalog_started = Instant::now();
    let catalog = build_region_catalog_for_surface_with_app(app, &iso, &surface)?;
    perf_log(
        "load_region_catalog_for_country.build_region_catalog",
        build_catalog_started,
    );
    if let Ok(mut guard) = cache.lock() {
        guard.insert(memo_key, catalog.clone());
    }
    perf_log("load_region_catalog_for_country.cache_store", load_started);
    perf_log("load_region_catalog_for_country.total", load_started);
    Ok(Some(catalog))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uk_surface_path() -> PathBuf {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..");
        let uk = repo_root
            .join("data")
            .join("country_packs")
            .join("UK")
            .join("surfaces")
            .join("UK.surface.json");
        if uk.exists() {
            return uk;
        }
        repo_root
            .join("data")
            .join("country_packs")
            .join("GB")
            .join("surfaces")
            .join("GB.surface.json")
    }

    fn uk_regions_geojson_path() -> PathBuf {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..");
        let uk = repo_root
            .join("data")
            .join("country_packs")
            .join("UK")
            .join("regions.geojson");
        if uk.exists() {
            return uk;
        }
        repo_root
            .join("data")
            .join("country_packs")
            .join("GB")
            .join("regions.geojson")
    }

    #[test]
    fn uk_catalog_synthesizes_gameplay_scale_regions() {
        let surface = load_surface_wire(&uk_surface_path()).expect("UK surface should load");
        let catalog = build_region_catalog_for_surface(CANONICAL_UK_ISO2, &surface)
            .expect("catalog should build");
        println!("uk_synth_region_count={}", catalog.regions.len());
        println!(
            "uk_synth_region_sample={}",
            catalog
                .regions
                .first()
                .map(|region| region.name.clone())
                .unwrap_or_default()
        );

        assert!(
            (80..=180).contains(&catalog.regions.len()),
            "expected gameplay-scale region count, got {}",
            catalog.regions.len()
        );
        assert!(
            catalog
                .regions
                .iter()
                .all(|region| !region.name.trim().is_empty()),
            "all synthesized regions should have human-readable names"
        );
        assert!(
            catalog
                .regions
                .iter()
                .all(|region| !region.name.contains("8619")),
            "raw substrate-like names should not be exposed"
        );
        assert!(
            !catalog.legacy_region_aliases.is_empty(),
            "legacy substrate aliases should map into planning regions"
        );
    }

    #[test]
    fn uk_start_city_maps_to_planning_region() {
        let surface = load_surface_wire(&uk_surface_path()).expect("UK surface should load");
        let catalog = build_region_catalog_for_surface(CANONICAL_UK_ISO2, &surface)
            .expect("catalog should build");
        let start = StartLocation {
            country_iso2: CANONICAL_UK_ISO2.to_string(),
            country_name: "United Kingdom".to_string(),
            city_id: 2_643_743,
            city_name: "London".to_string(),
            city_lon: -0.1276,
            city_lat: 51.5072,
            city_population: Some(9_000_000),
        };
        let selected = nearest_region_for_start(&catalog, Some(&start), CANONICAL_UK_ISO2)
            .expect("start region");
        assert!(catalog.by_id.contains_key(&selected));
        assert!(
            selected.starts_with("r6:UK:pr"),
            "expected synthesized planning region id, got {selected}"
        );
    }

    #[test]
    fn uk_surface_res6_matches_regions_geojson_hex_inventory() {
        let surface = load_surface_wire(&uk_surface_path()).expect("UK surface should load");
        let surface_hexes = surface
            .cells_res6
            .iter()
            .map(|cell| cell.cell_id.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();

        let regions_geojson = read_json_file::<JsonValue>(&uk_regions_geojson_path())
            .expect("UK regions.geojson should load");
        let feature_rows = regions_geojson
            .get("features")
            .and_then(|rows| rows.as_array())
            .cloned()
            .unwrap_or_default();
        let mut geojson_hexes = BTreeSet::<String>::new();
        for feature in feature_rows {
            let region_id = feature
                .get("properties")
                .and_then(|properties| properties.get("region_id"))
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
                .unwrap_or_default();
            let Some(parsed) = parse_region_id(&region_id) else {
                continue;
            };
            if parsed.tier != RegionIdTier::H3Res6 {
                continue;
            }
            if !is_uk_country_iso2(&parsed.country_iso2) {
                continue;
            }
            geojson_hexes.insert(parsed.token);
        }

        let missing_from_surface = geojson_hexes
            .difference(&surface_hexes)
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            missing_from_surface.is_empty(),
            "regions.geojson includes hexes missing from surface: {}",
            missing_from_surface.join(", ")
        );
        // Runtime surface may include additional UK land-coverage backfill cells that
        // are intentionally absent from static regions.geojson snapshots.
    }

    #[test]
    fn uk_manual_regions_override_placeholder_regions() {
        let surface = load_surface_wire(&uk_surface_path()).expect("UK surface should load");
        let manual = vec![
            ManualPlanningRegionDefinition {
                region_id: "r6:UK:thanet".to_string(),
                region_token: "thanet".to_string(),
                name: "Thanet".to_string(),
                hex_numbers: vec![3351, 3352, 3356, 3361, 3357, 3359, 3354],
            },
            ManualPlanningRegionDefinition {
                region_id: "r6:UK:dover".to_string(),
                region_token: "dover".to_string(),
                name: "Dover".to_string(),
                hex_numbers: vec![3358, 3341, 3342, 3339, 3340, 3343, 3360],
            },
            ManualPlanningRegionDefinition {
                region_id: "r6:UK:canterbury".to_string(),
                region_token: "canterbury".to_string(),
                name: "Canterbury".to_string(),
                hex_numbers: vec![3355, 3338, 3641, 3334, 3335, 3332],
            },
        ];

        let substrate = build_substrate_region_catalog(CANONICAL_UK_ISO2, &surface);
        let synthesized = synthesize_planning_region_catalog(CANONICAL_UK_ISO2, substrate.clone());
        let catalog = merge_surface_region_catalog_aliases(build_manual_planning_region_catalog(
            CANONICAL_UK_ISO2,
            &substrate,
            &synthesized,
            &manual,
        ));

        assert!(catalog.by_id.contains_key("r6:UK:thanet"));
        assert!(catalog.by_id.contains_key("r6:UK:dover"));
        assert!(catalog.by_id.contains_key("r6:UK:canterbury"));
        assert_eq!(
            catalog.by_id.get("r6:UK:thanet").map(|r| r.name.as_str()),
            Some("Thanet")
        );
        assert_eq!(
            catalog.by_id.get("r6:UK:dover").map(|r| r.name.as_str()),
            Some("Dover")
        );
        assert_eq!(
            catalog
                .by_id
                .get("r6:UK:canterbury")
                .map(|r| r.name.as_str()),
            Some("Canterbury")
        );

        let (number_to_substrate, _) = build_substrate_hex_number_lookup(&substrate);
        let cell_3351 = number_to_substrate
            .get(&3351)
            .expect("hex #3351 should resolve")
            .clone();
        let canonical_for_3351 =
            canonical_region_for_catalog(&catalog, &cell_3351).expect("alias for #3351");
        assert_eq!(canonical_for_3351, "r6:UK:thanet");

        let cell_3358 = number_to_substrate
            .get(&3358)
            .expect("hex #3358 should resolve")
            .clone();
        let canonical_for_3358 =
            canonical_region_for_catalog(&catalog, &cell_3358).expect("alias for #3358");
        assert_eq!(canonical_for_3358, "r6:UK:dover");

        let cell_3332 = number_to_substrate
            .get(&3332)
            .expect("hex #3332 should resolve")
            .clone();
        let canonical_for_3332 =
            canonical_region_for_catalog(&catalog, &cell_3332).expect("alias for #3332");
        assert_eq!(canonical_for_3332, "r6:UK:canterbury");

        let unassigned = number_to_substrate
            .get(&3353)
            .expect("hex #3353 should resolve")
            .clone();
        let unassigned_region = catalog.by_id.get(&unassigned).expect("unassigned region");
        assert!(
            unassigned_region.name.starts_with("Hex #"),
            "expected unassigned hex fallback name, got {}",
            unassigned_region.name
        );
        assert_eq!(
            unassigned_region.source_code.as_deref().unwrap_or_default(),
            "manual_region_unassigned_hex"
        );
    }
}
