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
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| x.len() == 2)
        .collect::<BTreeSet<_>>();
    if let Some(start) = manifest.start_location.as_ref() {
        let code = start.country_iso2.trim().to_ascii_uppercase();
        if code.len() == 2 {
            set.insert(code);
        }
    }
    set.into_iter().collect()
}

pub(crate) fn load_surface_wire(path: &Path) -> Result<DemandSurfaceCountryWire, String> {
    let mut surface: DemandSurfaceCountryWire = read_json_file(path)?;
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
        .map(|m| m.source != "surface_v4_region_scope")
        .unwrap_or(true);
    if changed || should_reset_meta {
        scenario.world.demand_meta = Some(DemandMeta {
            surface_version: "v4".to_string(),
            loaded_countries: vec![],
            source: "surface_v4_region_scope".to_string(),
        });
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
