use crate::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DemandSurfaceCountryWire {
    pub(crate) country_iso2: String,
    pub(crate) surface_version: String,
    pub(crate) source_provenance: JsonValue,
    pub(crate) cells_res6: Vec<DemandSurfaceCellWire>,
    pub(crate) cells_res7: Vec<DemandSurfaceCellWire>,
    pub(crate) cells_res8: Vec<DemandSurfaceCellWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DemandSurfaceCellWire {
    pub(crate) cell_id: String,
    pub(crate) h3_res: u8,
    pub(crate) lon: f64,
    pub(crate) lat: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) area_m2: f64,
    pub(crate) country_iso2: String,
    pub(crate) residents_raw: f64,
    pub(crate) jobs_raw: f64,
    pub(crate) residents_smooth: f64,
    pub(crate) jobs_smooth: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_residential: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_office: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_retail: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_recreation: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_industrial: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_education: f64,
    #[serde(default = "default_surface_activity_mix")]
    pub(crate) activity_mix_health: f64,
    pub(crate) quality: f64,
}

pub(crate) fn normalize_activity_mix(values: [f64; 7]) -> [f64; 7] {
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

pub(crate) fn default_surface_activity_mix() -> f64 {
    0.0
}

pub(crate) fn legacy_mix_from_residents_jobs(residents: f64, jobs: f64, area_m2: f64) -> [f64; 7] {
    let r = residents.max(0.0);
    let j = jobs.max(0.0);
    if r + j <= 1e-9 {
        return [0.70, 0.08, 0.08, 0.05, 0.05, 0.02, 0.02];
    }
    let total = r + j;
    let resident_balance = (r / total).clamp(0.0, 1.0);
    let employment_balance = (j / total).clamp(0.0, 1.0);
    let area_km2 = (area_m2.max(1.0) / 1_000_000.0).max(0.05);
    let density = total / area_km2;
    let urbanity = ((density + 1.0).log10() / 4.0).clamp(0.0, 1.0);

    normalize_activity_mix([
        0.52 + 0.30 * resident_balance - 0.25 * urbanity,
        0.05 + 0.22 * employment_balance + 0.10 * urbanity,
        0.05 + 0.09 * urbanity + 0.03 * employment_balance,
        0.04 + 0.07 * urbanity,
        0.04 + 0.12 * employment_balance + 0.04 * urbanity,
        0.03 + 0.04 * urbanity,
        0.03 + 0.03 * urbanity,
    ])
}

pub(crate) fn backfill_legacy_surface_mix(surface: &mut DemandSurfaceCountryWire) {
    let fill = |cells: &mut [DemandSurfaceCellWire]| {
        for c in cells {
            let current = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            let current_sum: f64 = current.iter().filter(|v| v.is_finite() && **v >= 0.0).sum();
            let normalized = if current_sum > 1e-9 {
                normalize_activity_mix(current)
            } else {
                legacy_mix_from_residents_jobs(c.residents_smooth, c.jobs_smooth, c.area_m2)
            };
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
    };
    fill(&mut surface.cells_res8);
    fill(&mut surface.cells_res7);
    fill(&mut surface.cells_res6);
}

pub(crate) fn unlocked_country_codes(manifest: &ProjectManifest) -> Vec<String> {
    let mut set = manifest
        .economy
        .unlocked_countries
        .iter()
        .filter_map(|x| canonical_country_iso2(x))
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        if let Some(code) = canonical_country_iso2(&start.country_iso2) {
            set.insert(code);
        }
    }
    set.into_iter().collect()
}

const UK_LAND_BACKFILL_SOURCE_CODE: &str = "uk_land_backfill_res6";

fn feature_country_iso2(props: Option<&serde_json::Map<String, JsonValue>>) -> Option<String> {
    let props = props?;
    let direct = props
        .get("country_iso2")
        .and_then(|value| value.as_str())
        .or_else(|| props.get("iso_a2").and_then(|value| value.as_str()))
        .or_else(|| props.get("ISO_A2").and_then(|value| value.as_str()))
        .or_else(|| props.get("iso2").and_then(|value| value.as_str()));
    direct.and_then(canonical_country_iso2)
}

fn feature_is_uk_landmask_candidate(
    props: Option<&serde_json::Map<String, JsonValue>>,
    default_to_true_when_missing: bool,
) -> bool {
    match feature_country_iso2(props) {
        Some(iso) => is_uk_country_iso2(&iso),
        None => default_to_true_when_missing,
    }
}

fn expected_hexes_res6_from_geojson(
    path: &Path,
    default_to_true_when_missing: bool,
) -> Result<BTreeSet<String>, String> {
    let value = read_json_file::<JsonValue>(path)?;
    let geojson = serde_json::from_value::<geojson::GeoJson>(value)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let geojson::GeoJson::FeatureCollection(feature_collection) = geojson else {
        return Ok(BTreeSet::new());
    };
    let config = h3o::geom::PolyfillConfig::new(Resolution::Six)
        .containment_mode(h3o::geom::ContainmentMode::Covers);
    let mut out = BTreeSet::<String>::new();
    use h3o::geom::ToCells;
    for feature in feature_collection.features {
        if !feature_is_uk_landmask_candidate(feature.properties.as_ref(), default_to_true_when_missing) {
            continue;
        }
        let Some(geometry) = feature.geometry else {
            continue;
        };
        let geo_geometry = geo::Geometry::try_from(&geometry.value)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let h3_geometry = h3o::geom::Geometry::from_degrees(geo_geometry)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        for cell in h3_geometry.to_cells(config) {
            if cell.resolution() == Resolution::Six {
                out.insert(cell.to_string().to_ascii_lowercase());
            }
        }
    }
    Ok(out)
}

fn world_context_value_from_path(path: &Path) -> Result<JsonValue, String> {
    let value = read_json_file::<JsonValue>(path)?;
    let has_country_iso2 = value
        .get("features")
        .and_then(|value| value.as_array())
        .map(|features| {
            features.iter().any(|feature| {
                feature
                    .get("properties")
                    .and_then(|props| props.as_object())
                    .and_then(|props| props.get("country_iso2"))
                    .and_then(|value| value.as_str())
                    .is_some()
            })
        })
        .unwrap_or(false);
    if has_country_iso2 {
        Ok(value)
    } else {
        world_context_from_countries_geojson(value)
    }
}

fn expected_uk_land_hexes_res6(path: &Path) -> Result<(BTreeSet<String>, Option<PathBuf>), String> {
    let mut candidate_paths = Vec::<PathBuf>::new();
    if let Some(pack_root) = path.parent().and_then(|parent| parent.parent()) {
        candidate_paths.push(pack_root.join("map").join("counties.geojson"));
        candidate_paths.push(pack_root.join("map").join("world_context.geojson"));
    }
    candidate_paths.push(repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson"));
    candidate_paths.push(repo_boundaries_root().join("countries.geojson"));
    candidate_paths.push(repo_boundaries_root().join("world_context.geojson"));

    let mut seen = HashSet::<PathBuf>::new();
    for candidate in candidate_paths {
        if !candidate.exists() || !seen.insert(candidate.clone()) {
            continue;
        }
        let file_name = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let out = if file_name == "world_context.geojson" || file_name == "countries.geojson" {
            let remapped = world_context_value_from_path(&candidate)?;
            let remapped_path = candidate.with_extension("remapped.tmp.json");
            let geojson = serde_json::from_value::<geojson::GeoJson>(remapped)
                .map_err(|error| format!("{}: {error}", candidate.display()))?;
            let geojson::GeoJson::FeatureCollection(feature_collection) = geojson else {
                continue;
            };
            let config = h3o::geom::PolyfillConfig::new(Resolution::Six)
                .containment_mode(h3o::geom::ContainmentMode::Covers);
            let mut out = BTreeSet::<String>::new();
            use h3o::geom::ToCells;
            for feature in feature_collection.features {
                if !feature_is_uk_landmask_candidate(feature.properties.as_ref(), false) {
                    continue;
                }
                let Some(geometry) = feature.geometry else {
                    continue;
                };
                let geo_geometry = geo::Geometry::try_from(&geometry.value)
                    .map_err(|error| format!("{}: {error}", remapped_path.display()))?;
                let h3_geometry = h3o::geom::Geometry::from_degrees(geo_geometry)
                    .map_err(|error| format!("{}: {error}", remapped_path.display()))?;
                for cell in h3_geometry.to_cells(config) {
                    if cell.resolution() == Resolution::Six {
                        out.insert(cell.to_string().to_ascii_lowercase());
                    }
                }
            }
            out
        } else {
            expected_hexes_res6_from_geojson(&candidate, true)?
        };
        if !out.is_empty() {
            return Ok((out, Some(candidate)));
        }
    }
    Ok((BTreeSet::new(), None))
}

fn nearest_template_cell(
    target: CellIndex,
    templates_by_id: &HashMap<String, DemandSurfaceCellWire>,
) -> Option<DemandSurfaceCellWire> {
    for radius in 1..=4 {
        for neighbor in target.grid_disk::<Vec<_>>(radius) {
            if neighbor == target || neighbor.resolution() != Resolution::Six {
                continue;
            }
            let key = neighbor.to_string().to_ascii_lowercase();
            if let Some(template) = templates_by_id.get(&key) {
                return Some(template.clone());
            }
        }
    }
    None
}

fn synthesize_backfill_res6_cell(
    cell: CellIndex,
    country_iso2: &str,
    template: Option<&DemandSurfaceCellWire>,
) -> DemandSurfaceCellWire {
    let center: h3o::LatLng = cell.into();
    let lon = center.lng();
    let lat = center.lat();
    let (x, y) = lonlat_to_web_mercator_m(lon, lat);
    let fallback_mix = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let template_mix = template
        .map(|cell| {
            [
                cell.activity_mix_residential,
                cell.activity_mix_office,
                cell.activity_mix_retail,
                cell.activity_mix_recreation,
                cell.activity_mix_industrial,
                cell.activity_mix_education,
                cell.activity_mix_health,
            ]
        })
        .unwrap_or(fallback_mix);
    let normalized_mix = normalize_activity_mix(template_mix);

    DemandSurfaceCellWire {
        cell_id: cell.to_string().to_ascii_lowercase(),
        h3_res: 6,
        lon,
        lat,
        x,
        y,
        area_m2: cell.area_m2(),
        country_iso2: country_iso2.to_string(),
        residents_raw: template.map(|cell| cell.residents_raw).unwrap_or(0.0),
        jobs_raw: template.map(|cell| cell.jobs_raw).unwrap_or(0.0),
        residents_smooth: template.map(|cell| cell.residents_smooth).unwrap_or(0.0),
        jobs_smooth: template.map(|cell| cell.jobs_smooth).unwrap_or(0.0),
        activity_mix_residential: normalized_mix[0],
        activity_mix_office: normalized_mix[1],
        activity_mix_retail: normalized_mix[2],
        activity_mix_recreation: normalized_mix[3],
        activity_mix_industrial: normalized_mix[4],
        activity_mix_education: normalized_mix[5],
        activity_mix_health: normalized_mix[6],
        quality: template.map(|cell| cell.quality).unwrap_or(0.0),
    }
}

fn apply_uk_land_hex_backfill(
    source_path: &Path,
    surface: &mut DemandSurfaceCountryWire,
) -> Result<(), String> {
    if !is_uk_country_iso2(&surface.country_iso2) {
        return Ok(());
    }

    let (expected_hexes, expected_source_path) = expected_uk_land_hexes_res6(source_path)?;
    if expected_hexes.is_empty() {
        return Ok(());
    }

    let templates_by_id = surface
        .cells_res6
        .iter()
        .filter_map(|cell| {
            let parsed = cell.cell_id.parse::<CellIndex>().ok()?;
            (parsed.resolution() == Resolution::Six)
                .then(|| (cell.cell_id.to_ascii_lowercase(), cell.clone()))
        })
        .collect::<HashMap<_, _>>();
    let actual_before = templates_by_id.keys().cloned().collect::<BTreeSet<_>>();

    let mut missing_hexes = expected_hexes
        .difference(&actual_before)
        .cloned()
        .collect::<Vec<_>>();
    missing_hexes.sort();

    let mut synthesized = Vec::<DemandSurfaceCellWire>::new();
    for missing_hex in &missing_hexes {
        let Ok(cell) = missing_hex.parse::<CellIndex>() else {
            continue;
        };
        if cell.resolution() != Resolution::Six {
            continue;
        }
        let template = nearest_template_cell(cell, &templates_by_id);
        synthesized.push(synthesize_backfill_res6_cell(
            cell,
            &surface.country_iso2,
            template.as_ref(),
        ));
    }
    surface.cells_res6.extend(synthesized);

    let actual_after = surface
        .cells_res6
        .iter()
        .filter_map(|cell| {
            let parsed = cell.cell_id.parse::<CellIndex>().ok()?;
            (parsed.resolution() == Resolution::Six).then(|| cell.cell_id.to_ascii_lowercase())
        })
        .collect::<BTreeSet<_>>();
    let missing_after = expected_hexes
        .difference(&actual_after)
        .cloned()
        .collect::<Vec<_>>();

    if !surface.source_provenance.is_object() {
        surface.source_provenance = serde_json::json!({});
    }
    if let Some(provenance) = surface.source_provenance.as_object_mut() {
        provenance.insert(
            UK_LAND_BACKFILL_SOURCE_CODE.to_string(),
            JsonValue::Array(
                missing_hexes
                    .iter()
                    .map(|hex_id| JsonValue::String(hex_id.clone()))
                    .collect(),
            ),
        );
        provenance.insert(
            "uk_land_coverage_audit".to_string(),
            serde_json::json!({
                "expected_land_hexes_res6": expected_hexes.len(),
                "actual_land_hexes_before_backfill_res6": actual_before.len(),
                "actual_land_hexes_after_backfill_res6": actual_after.len(),
                "missing_land_hexes_before_backfill_res6": missing_hexes.len(),
                "missing_land_hexes_after_backfill_res6": missing_after.len(),
                "expected_source_path": expected_source_path.map(|path| path.display().to_string()),
            }),
        );
    }

    Ok(())
}

pub(crate) fn load_surface_wire(path: &Path) -> Result<DemandSurfaceCountryWire, String> {
    let mut surface: DemandSurfaceCountryWire = read_json_file(path)?;
    if let Some(canonical_iso2) = canonical_country_iso2(&surface.country_iso2) {
        surface.country_iso2 = canonical_iso2.clone();
        for cell in &mut surface.cells_res8 {
            cell.country_iso2 = canonical_iso2.clone();
        }
        for cell in &mut surface.cells_res7 {
            cell.country_iso2 = canonical_iso2.clone();
        }
        for cell in &mut surface.cells_res6 {
            cell.country_iso2 = canonical_iso2.clone();
        }
    }
    if surface.surface_version.eq_ignore_ascii_case("v3") {
        backfill_legacy_surface_mix(&mut surface);
        surface.surface_version = "v4".to_string();
    } else if !surface.surface_version.eq_ignore_ascii_case("v4") {
        return Err(format!(
            "unsupported demand surface version '{}' in {} (expected v4 or v3)",
            surface.surface_version,
            path.display()
        ));
    }

    let validate_cells = |cells: &mut [DemandSurfaceCellWire], layer: &str| -> Result<(), String> {
        for c in cells {
            let values = [
                c.activity_mix_residential,
                c.activity_mix_office,
                c.activity_mix_retail,
                c.activity_mix_recreation,
                c.activity_mix_industrial,
                c.activity_mix_education,
                c.activity_mix_health,
            ];
            if values.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(format!(
                    "invalid activity mix in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let sum: f64 = values.iter().sum();
            if sum <= 1e-9 {
                return Err(format!(
                    "activity mix sum must be > 0 in {layer} cell {} from {}",
                    c.cell_id,
                    path.display()
                ));
            }
            let normalized = normalize_activity_mix(values);
            c.activity_mix_residential = normalized[0];
            c.activity_mix_office = normalized[1];
            c.activity_mix_retail = normalized[2];
            c.activity_mix_recreation = normalized[3];
            c.activity_mix_industrial = normalized[4];
            c.activity_mix_education = normalized[5];
            c.activity_mix_health = normalized[6];
        }
        Ok(())
    };

    validate_cells(&mut surface.cells_res8, "res8")?;
    validate_cells(&mut surface.cells_res7, "res7")?;
    validate_cells(&mut surface.cells_res6, "res6")?;
    apply_uk_land_hex_backfill(path, &mut surface)?;
    validate_cells(&mut surface.cells_res6, "res6")?;
    Ok(surface)
}

pub(crate) fn cell_id_is_legacy_generated(cell_id: &str) -> bool {
    cell_id.starts_with("df:") || cell_id.starts_with("dc:")
}

pub(crate) fn zone_id_is_legacy_generated(zone_id: &str) -> bool {
    zone_id.starts_with("z:df:") || zone_id.starts_with("z:dc:")
}

pub(crate) fn migrate_legacy_synthetic_demand(scenario: &mut Scenario) -> bool {
    let mut changed = false;
    let old_cells = scenario.world.demand_cells.len();
    scenario
        .world
        .demand_cells
        .retain(|c| !cell_id_is_legacy_generated(&c.cell_id));
    if scenario.world.demand_cells.len() != old_cells {
        changed = true;
    }

    let old_zones = scenario.world.zones.len();
    scenario.world.zones.retain(|z| {
        if zone_id_is_legacy_generated(&z.id) {
            return false;
        }
        if let Some(cell_id) = z.id.strip_prefix("z:") {
            return !cell_id_is_legacy_generated(cell_id);
        }
        true
    });
    if scenario.world.zones.len() != old_zones {
        changed = true;
    }

    let should_reset_meta = scenario
        .world
        .demand_meta
        .as_ref()
        .map(|m| m.source != DEMAND_PIPELINE_PERSISTED_META_SOURCE)
        .unwrap_or(true);
    if changed || should_reset_meta {
        scenario.world.demand_meta = Some(default_surface_pipeline_demand_meta());
        changed = true;
    }
    changed
}

pub(crate) fn is_surface_generated_cell_id(cell_id: &str) -> bool {
    cell_id.starts_with("ds:v3:") || cell_id.starts_with("ds:v4:") || cell_id.starts_with("ds:v4m:")
}

pub(crate) fn is_surface_generated_zone_id(zone_id: &str) -> bool {
    if zone_id.starts_with("ds:v3:")
        || zone_id.starts_with("ds:v4:")
        || zone_id.starts_with("ds:v4m:")
    {
        return true;
    }
    zone_id
        .strip_prefix("z:")
        .map(is_surface_generated_cell_id)
        .unwrap_or(false)
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

    fn res6_cell_set(surface: &DemandSurfaceCountryWire) -> BTreeSet<String> {
        surface
            .cells_res6
            .iter()
            .filter_map(|cell| {
                let parsed = cell.cell_id.parse::<CellIndex>().ok()?;
                (parsed.resolution() == Resolution::Six).then(|| cell.cell_id.to_ascii_lowercase())
            })
            .collect::<BTreeSet<_>>()
    }

    #[test]
    fn uk_surface_backfill_eliminates_expected_land_gaps() {
        let surface_path = uk_surface_path();
        let raw_surface =
            read_json_file::<DemandSurfaceCountryWire>(&surface_path).expect("raw UK surface");
        let (expected, source_path) =
            expected_uk_land_hexes_res6(&surface_path).expect("expected UK land hex set");
        assert!(
            !expected.is_empty(),
            "expected UK land set should not be empty (source: {:?})",
            source_path
        );
        assert!(
            source_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_ascii_lowercase().contains("counties.geojson"))
                .unwrap_or(false),
            "expected UK landmask should come from county coastline geometry first; got {:?}",
            source_path
        );

        let before_cells = res6_cell_set(&raw_surface);
        let before_missing = expected
            .difference(&before_cells)
            .cloned()
            .collect::<Vec<_>>();

        let repaired_surface = load_surface_wire(&surface_path).expect("repaired UK surface");
        let after_cells = res6_cell_set(&repaired_surface);
        let after_missing = expected
            .difference(&after_cells)
            .cloned()
            .collect::<Vec<_>>();

        println!(
            "uk_land_backfill source={:?} expected={} before_missing={} after_missing={}",
            source_path,
            expected.len(),
            before_missing.len(),
            after_missing.len()
        );

        assert!(
            before_missing.len() >= after_missing.len(),
            "backfill should not increase missing UK land hexes"
        );
        assert!(
            after_missing.is_empty(),
            "UK land hexes still missing after backfill: {}",
            after_missing.join(", ")
        );
    }

    #[test]
    fn uk_surface_includes_known_island_and_coastal_cells() {
        let surface_path = uk_surface_path();
        let surface = load_surface_wire(&surface_path).expect("repaired UK surface");
        let cells = res6_cell_set(&surface);
        let targets = [
            ("Isles of Scilly", -6.315, 49.915),
            ("Orkney (Kirkwall area)", -2.960, 58.984),
            ("Cornwall coast (Lizard area)", -5.206, 49.968),
        ];
        for (label, lon, lat) in targets {
            let ll = h3o::LatLng::new(lat, lon).expect("valid lon/lat");
            let cell = ll.to_cell(Resolution::Six).to_string().to_ascii_lowercase();
            assert!(
                cells.contains(&cell),
                "{label} expected res6 cell missing from UK surface: {cell}"
            );
        }
    }
}
