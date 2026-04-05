use super::super::*;
use tauri::{command, AppHandle};
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Clone, Deserialize)]
struct CatalogCountryWire {
    iso2: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogCityWire {
    geonameid: i64,
    name: String,
    lat: f64,
    lon: f64,
    population: u64,
}

fn repo_location_catalog_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("location_catalog")
}

fn repo_demand_surfaces_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("demand_surfaces")
}

fn repo_country_packs_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("data")
        .join("country_packs")
}

fn managed_country_pack_dir(app: &AppHandle, country_iso2: &str) -> Result<PathBuf, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    Ok(country_packs_root(app)?.join(&iso))
}

fn repo_country_pack_dir(country_iso2: &str) -> PathBuf {
    repo_country_packs_root().join(country_iso2.trim().to_ascii_uppercase())
}

pub(crate) fn country_pack_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let managed = managed_country_pack_dir(app, country_iso2).ok()?;
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_country_pack_dir(country_iso2);
    if repo.exists() {
        return Some(repo);
    }
    None
}

pub(crate) fn rollout_supported_countries() -> BTreeSet<String> {
    ["GB"]
        .iter()
        .map(|x| x.to_string())
        .collect::<BTreeSet<_>>()
}

fn location_catalog_file(app: &AppHandle, relative: &Path) -> Option<PathBuf> {
    let managed = location_catalog_root(app).ok()?.join(relative);
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_location_catalog_root().join(relative);
    if repo.exists() {
        return Some(repo);
    }
    None
}

pub(crate) fn demand_surface_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return None;
    }
    let managed_pack_surface = managed_country_pack_dir(app, &iso)
        .ok()?
        .join("surfaces")
        .join(format!("{iso}.surface.json"));
    if managed_pack_surface.exists() {
        return Some(managed_pack_surface);
    }
    let repo_pack_surface = repo_country_pack_dir(&iso)
        .join("surfaces")
        .join(format!("{iso}.surface.json"));
    if repo_pack_surface.exists() {
        return Some(repo_pack_surface);
    }
    let managed = demand_surfaces_root(app)
        .ok()?
        .join(format!("{iso}.surface.json"));
    if managed.exists() {
        return Some(managed);
    }
    let repo = repo_demand_surfaces_root().join(format!("{iso}.surface.json"));
    if repo.exists() {
        return Some(repo);
    }
    None
}

fn managed_demand_surface_file(app: &AppHandle, country_iso2: &str) -> Result<PathBuf, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    Ok(demand_surfaces_root(app)?.join(format!("{iso}.surface.json")))
}

pub(crate) fn world_context_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let canonical = repo_boundaries_root().join("countries.geojson");
    if canonical.exists() {
        return Some(canonical);
    }
    let repo = repo_boundaries_root().join("world_context.geojson");
    if repo.exists() {
        return Some(repo);
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir.join("map").join("world_context.geojson");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    None
}

pub(crate) fn counties_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir.join("map").join("counties.geojson");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    let repo = repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson");
    repo.exists().then_some(repo)
}

pub(crate) fn basemap_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir.join("map").join("gb_basemap.mbtiles");
    path.exists().then_some(path)
}

pub(crate) fn style_template_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    if let Some(pack_dir) = country_pack_dir(app, &iso) {
        let packaged = pack_dir
            .join("map")
            .join("style")
            .join("interlinked-light.json");
        if packaged.exists() {
            return Some(packaged);
        }
    }
    let repo = repo_map_style_root().join("interlinked-light.json");
    repo.exists().then_some(repo)
}

pub(crate) fn major_roads_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso != "GB" {
        return None;
    }
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir.join("map").join("gb_major_roads.geojson");
    path.exists().then_some(path)
}

pub(crate) fn county_roads_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let path = pack_dir
        .join("map")
        .join("county_roads")
        .join(format!("{}.geojson", county_id.trim()));
    path.exists().then_some(path)
}

pub(crate) fn county_basemap_mid_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let fallback = pack_dir
        .join("map")
        .join("county_basemap_mid")
        .join(format!("{}.geojson", county_id.trim()));
    if fallback.exists() {
        return Some(fallback);
    }
    county_roads_file(app, &iso, county_id)
}

pub(crate) fn county_basemap_full_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let fallback = pack_dir
        .join("map")
        .join("county_basemap_full")
        .join(format!("{}.geojson", county_id.trim()));
    if fallback.exists() {
        return Some(fallback);
    }
    county_roads_file(app, &iso, county_id)
}

pub(crate) fn county_basemap_mid_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let dir = pack_dir.join("map").join("county_basemap_mid");
    dir.exists().then_some(dir)
}

pub(crate) fn county_basemap_full_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let pack_dir = country_pack_dir(app, &iso)?;
    let dir = pack_dir.join("map").join("county_basemap_full");
    dir.exists().then_some(dir)
}

fn directory_size_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total = total.saturating_add(directory_size_bytes(&path));
        } else if let Ok(metadata) = entry.metadata() {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}

fn upsert_country_pack_entry(
    app: &AppHandle,
    country_iso2: &str,
    build_state: &str,
    surface_version: Option<String>,
    cells_count: usize,
    provenance: Option<String>,
) -> Result<(), String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let mut idx = read_country_pack_index(app)?;
    idx.version = 1;
    idx.packs.retain(|p| p.country_iso2 != iso);
    idx.packs.push(CountryPackEntry {
        country_iso2: iso,
        build_state: build_state.to_string(),
        surface_version,
        cells_count,
        last_updated_at: Some(now_string()),
        checksum: None,
        provenance,
    });
    idx.packs
        .sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    write_country_pack_index(app, &idx)
}

pub(crate) fn country_pack_status_for(
    app: &AppHandle,
    index: &CountryPackIndex,
    supported_rollout: &BTreeSet<String>,
    country_iso2: &str,
) -> CountryPackStatus {
    let iso = country_iso2.trim().to_ascii_uppercase();
    let idx_entry = index.packs.iter().find(|p| p.country_iso2 == iso);
    let surface_path = demand_surface_file(app, &iso);
    let mut surface_version = idx_entry.and_then(|p| p.surface_version.clone());
    let mut cells_count = idx_entry.map(|p| p.cells_count).unwrap_or(0);
    let mut last_updated_at = idx_entry.and_then(|p| p.last_updated_at.clone());
    let mut build_state = idx_entry
        .map(|p| p.build_state.clone())
        .unwrap_or_else(|| "missing".to_string());
    if let Some(path) = surface_path.as_ref() {
        if let Ok(surface) = load_surface_wire(path) {
            surface_version = Some(surface.surface_version.clone());
            cells_count = surface.cells_res8.len();
        }
        if build_state != "building" {
            build_state = "installed".to_string();
        }
        if last_updated_at.is_none() {
            last_updated_at = Some(now_string());
        }
    } else if build_state == "installed" {
        build_state = "missing".to_string();
    }

    let vector_map_ready = world_context_file(app, &iso).is_some()
        && counties_file(app, &iso).is_some()
        && basemap_file(app, &iso).is_some()
        && style_template_file(app, &iso).is_some();
    let legacy_map_ready = world_context_file(app, &iso).is_some()
        && (county_basemap_mid_dir(app, &iso).is_some()
            || county_basemap_full_dir(app, &iso).is_some()
            || country_pack_dir(app, &iso)
                .map(|dir| dir.join("map").join("county_roads").exists())
                .unwrap_or(false));
    let map_installed = vector_map_ready
        || legacy_map_ready
        || (world_context_file(app, &iso).is_some() && major_roads_file(app, &iso).is_some());
    let map_ready = vector_map_ready || legacy_map_ready;
    let map_size_bytes = country_pack_dir(app, &iso)
        .map(|dir| dir.join("map"))
        .filter(|dir| dir.exists())
        .map(|dir| directory_size_bytes(&dir));
    let demand_installed = surface_path.is_some();
    let fully_playable = demand_installed && map_ready;
    let map_pack_version = Some(
        if vector_map_ready {
            "vector-mbtiles-v1"
        } else if county_basemap_full_dir(app, &iso).is_some()
            || county_basemap_mid_dir(app, &iso).is_some()
        {
            "geojson-basemap-v2"
        } else if map_ready {
            "geojson-roads-v1"
        } else {
            "missing"
        }
        .to_string(),
    );
    let supported = supported_rollout.contains(&iso);
    let (eligible, reason) = if fully_playable && supported {
        (true, None)
    } else if !supported {
        (false, Some("Coming Soon".to_string()))
    } else if !map_ready {
        (false, Some("Map Pack Required".to_string()))
    } else if !demand_installed {
        (false, Some("Demand Pack Required".to_string()))
    } else {
        (false, Some("Install Required".to_string()))
    };

    CountryPackStatus {
        country_iso2: iso,
        build_state,
        surface_version,
        cells_count,
        last_updated_at,
        map_installed,
        map_ready,
        map_pack_version,
        map_size_bytes,
        demand_installed,
        fully_playable,
        eligible,
        reason,
    }
}

fn fallback_countries() -> Vec<CountryOption> {
    let mut rows = vec![
        ("AU", "Australia"),
        ("BE", "Belgium"),
        ("BR", "Brazil"),
        ("CA", "Canada"),
        ("CH", "Switzerland"),
        ("CN", "China"),
        ("DE", "Germany"),
        ("DK", "Denmark"),
        ("ES", "Spain"),
        ("FI", "Finland"),
        ("FR", "France"),
        ("GB", "Great Britain"),
        ("IE", "Ireland"),
        ("IN", "India"),
        ("IT", "Italy"),
        ("JP", "Japan"),
        ("KR", "South Korea"),
        ("MX", "Mexico"),
        ("NL", "Netherlands"),
        ("NO", "Norway"),
        ("NZ", "New Zealand"),
        ("PL", "Poland"),
        ("PT", "Portugal"),
        ("SE", "Sweden"),
        ("SG", "Singapore"),
        ("TR", "Turkey"),
        ("US", "United States"),
    ]
    .into_iter()
    .map(|(iso2, name)| CountryOption {
        iso2: iso2.to_string(),
        name: name.to_string(),
    })
    .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

fn fallback_cities(country_iso2: &str) -> Vec<CityOption> {
    let rows = match country_iso2 {
        "AU" => vec![
            (2147714, "Sydney", -33.8679, 151.2073, 5_230_000),
            (2172517, "Canberra", -35.2835, 149.1281, 381_000),
        ],
        "BE" => vec![
            (2800866, "Brussels", 50.8505, 4.3488, 1_200_000),
            (2797656, "Antwerp", 51.2199, 4.4035, 530_000),
        ],
        "BR" => vec![
            (3469058, "Brasilia", -15.7939, -47.8828, 3_100_000),
            (3448439, "Sao Paulo", -23.5505, -46.6333, 12_330_000),
        ],
        "CA" => vec![
            (6094817, "Ottawa", 45.4215, -75.6972, 1_070_000),
            (6077243, "Montreal", 45.5019, -73.5674, 1_780_000),
        ],
        "CH" => vec![
            (2661552, "Bern", 46.9481, 7.4474, 133_000),
            (2657896, "Zurich", 47.3769, 8.5417, 420_000),
        ],
        "CN" => vec![
            (1816670, "Beijing", 39.9042, 116.4074, 21_890_000),
            (1796236, "Shanghai", 31.2304, 121.4737, 24_870_000),
        ],
        "DE" => vec![
            (2950159, "Berlin", 52.52, 13.405, 3_850_000),
            (2867714, "Munich", 48.1374, 11.5755, 1_490_000),
        ],
        "DK" => vec![
            (2618425, "Copenhagen", 55.6761, 12.5683, 808_000),
            (2624652, "Aarhus", 56.1629, 10.2039, 285_000),
        ],
        "ES" => vec![
            (3117735, "Madrid", 40.4168, -3.7038, 3_280_000),
            (3128760, "Barcelona", 41.3851, 2.1734, 1_620_000),
        ],
        "FI" => vec![
            (658225, "Helsinki", 60.1699, 24.9384, 658_000),
            (634964, "Tampere", 61.4981, 23.761, 255_000),
        ],
        "FR" => vec![
            (2988507, "Paris", 48.8566, 2.3522, 2_160_000),
            (2996944, "Lyon", 45.764, 4.8357, 530_000),
        ],
        "GB" => vec![
            (2643743, "London", 51.5072, -0.1276, 9_000_000),
            (2643123, "Manchester", 53.4808, -2.2426, 570_000),
            (2644688, "Leeds", 53.8008, -1.5491, 536_000),
        ],
        "IE" => vec![
            (2964574, "Dublin", 53.3498, -6.2603, 593_000),
            (2965140, "Cork", 51.8985, -8.4756, 224_000),
        ],
        "IN" => vec![
            (1273294, "Delhi", 28.6139, 77.209, 16_787_000),
            (1275339, "Mumbai", 19.076, 72.8777, 12_442_000),
        ],
        "IT" => vec![
            (3169070, "Rome", 41.9028, 12.4964, 2_870_000),
            (3173435, "Milan", 45.4642, 9.19, 1_396_000),
        ],
        "JP" => vec![
            (1850147, "Tokyo", 35.6762, 139.6503, 13_960_000),
            (1853909, "Osaka", 34.6937, 135.5023, 2_750_000),
        ],
        "KR" => vec![
            (1835848, "Seoul", 37.5665, 126.978, 9_410_000),
            (1838519, "Busan", 35.1796, 129.0756, 3_350_000),
        ],
        "MX" => vec![
            (3530597, "Mexico City", 19.4326, -99.1332, 9_200_000),
            (3521081, "Guadalajara", 20.6597, -103.3496, 1_385_000),
        ],
        "NL" => vec![
            (2759794, "Amsterdam", 52.3676, 4.9041, 935_000),
            (2747891, "Rotterdam", 51.9244, 4.4777, 670_000),
        ],
        "NO" => vec![
            (3143244, "Oslo", 59.9139, 10.7522, 710_000),
            (3161732, "Bergen", 60.3913, 5.3221, 288_000),
        ],
        "NZ" => vec![
            (2179537, "Wellington", -41.2866, 174.7756, 216_000),
            (2193734, "Auckland", -36.8485, 174.7633, 1_530_000),
        ],
        "PL" => vec![
            (756135, "Warsaw", 52.2297, 21.0122, 1_860_000),
            (3094802, "Krakow", 50.0647, 19.945, 804_000),
        ],
        "PT" => vec![
            (2267057, "Lisbon", 38.7223, -9.1393, 545_000),
            (2735943, "Porto", 41.1579, -8.6291, 237_000),
        ],
        "SE" => vec![
            (2673730, "Stockholm", 59.3293, 18.0686, 984_000),
            (2711537, "Gothenburg", 57.7089, 11.9746, 603_000),
        ],
        "SG" => vec![(1880252, "Singapore", 1.2897, 103.8501, 5_918_000)],
        "TR" => vec![
            (745044, "Ankara", 39.9334, 32.8597, 5_750_000),
            (745042, "Istanbul", 41.0082, 28.9784, 15_700_000),
        ],
        "US" => vec![
            (4140963, "Washington", 38.8951, -77.0364, 702_000),
            (5128581, "New York", 40.7128, -74.006, 8_804_000),
            (5368361, "Los Angeles", 34.0522, -118.2437, 3_900_000),
        ],
        _ => vec![],
    };
    let capital_id = rows.first().map(|(geonameid, _, _, _, _)| *geonameid);
    let mut out = rows
        .into_iter()
        .map(|(geonameid, name, lat, lon, population)| CityOption {
            geonameid,
            name: name.to_string(),
            lat,
            lon,
            population,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| {
        let a_cap = capital_id == Some(a.geonameid);
        let b_cap = capital_id == Some(b.geonameid);
        b_cap
            .cmp(&a_cap)
            .then_with(|| b.population.cmp(&a.population))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn constrain_gb_start_cities(cities: Vec<CityOption>) -> Vec<CityOption> {
    let allowed_ids = BTreeMap::from([
        (2643743_i64, 0_usize), // London
        (2643123_i64, 1_usize), // Manchester
        (2644688_i64, 2_usize), // Leeds
    ]);
    let mut filtered = cities
        .into_iter()
        .filter(|city| allowed_ids.contains_key(&city.geonameid))
        .collect::<Vec<_>>();
    filtered.sort_by(|a, b| {
        let a_rank = allowed_ids.get(&a.geonameid).copied().unwrap_or(usize::MAX);
        let b_rank = allowed_ids.get(&b.geonameid).copied().unwrap_or(usize::MAX);
        a_rank
            .cmp(&b_rank)
            .then_with(|| b.population.cmp(&a.population))
            .then_with(|| a.name.cmp(&b.name))
    });
    filtered
}

pub(crate) fn list_cities_internal(
    app: &AppHandle,
    country_iso2: &str,
) -> Result<Vec<CityOption>, String> {
    let iso = country_iso2.trim().to_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let rel = Path::new("cities").join(format!("{iso}.json"));
    if let Some(path) = location_catalog_file(app, &rel) {
        let raw: Vec<CatalogCityWire> = read_json_file(&path)?;
        let capital_id = location_catalog_file(app, Path::new("capitals.json"))
            .and_then(|cap_path| read_json_file::<HashMap<String, i64>>(&cap_path).ok())
            .and_then(|caps| caps.get(&iso).copied());
        let mut out = raw
            .into_iter()
            .map(|c| CityOption {
                geonameid: c.geonameid,
                name: c.name,
                lat: c.lat,
                lon: c.lon,
                population: c.population,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| {
            let a_cap = capital_id == Some(a.geonameid);
            let b_cap = capital_id == Some(b.geonameid);
            b_cap
                .cmp(&a_cap)
                .then_with(|| b.population.cmp(&a.population))
                .then_with(|| a.name.cmp(&b.name))
        });
        if iso == "GB" {
            let constrained = constrain_gb_start_cities(out);
            if !constrained.is_empty() {
                return Ok(constrained);
            }
            let fallback = constrain_gb_start_cities(fallback_cities(&iso));
            if !fallback.is_empty() {
                return Ok(fallback);
            }
            return Err("no GB start cities available".to_string());
        }
        return Ok(out);
    }
    let fallback = fallback_cities(&iso);
    if fallback.is_empty() {
        return Err(format!("no cities available for country {iso}"));
    }
    if iso == "GB" {
        return Ok(constrain_gb_start_cities(fallback));
    }
    Ok(fallback)
}

#[command]
pub async fn pick_scenario_file(app: AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Scenario JSON", &["json"])
        .blocking_pick_file();
    let path = file
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().to_string());
    Ok(path)
}

#[command]
pub async fn pick_export_path(app: AppHandle, file_kind: String) -> Result<Option<String>, String> {
    let (label, ext) = match file_kind.to_ascii_lowercase().as_str() {
        "csv" => ("CSV Report", "csv"),
        "json" => ("JSON Report", "json"),
        _ => return Err("file_kind must be csv or json".to_string()),
    };
    let file = app
        .dialog()
        .file()
        .add_filter(label, &[ext])
        .blocking_save_file();
    let path = file
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().to_string());
    Ok(path)
}

#[command]
pub fn list_countries(app: AppHandle) -> Result<Vec<CountryOption>, String> {
    let path = location_catalog_file(&app, Path::new("countries.json"));
    if let Some(p) = path {
        let raw: Vec<CatalogCountryWire> = read_json_file(&p)?;
        let mut out = raw
            .into_iter()
            .map(|c| CountryOption {
                iso2: c.iso2,
                name: c.name,
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        return Ok(out);
    }
    Ok(fallback_countries())
}

#[command]
pub fn list_cities(app: AppHandle, country_iso2: String) -> Result<Vec<CityOption>, String> {
    list_cities_internal(&app, &country_iso2)
}

#[command]
pub fn list_country_pack_status(app: AppHandle) -> Result<Vec<CountryPackStatus>, String> {
    let countries = list_countries(app.clone())?;
    let index = read_country_pack_index(&app)?;
    let rollout = rollout_supported_countries();
    let mut out = countries
        .into_iter()
        .map(|country| country_pack_status_for(&app, &index, &rollout, &country.iso2))
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.country_iso2.cmp(&b.country_iso2));
    Ok(out)
}

#[command]
pub fn install_country_pack(app: AppHandle, country_iso2: String) -> Result<InstallResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let repo_pack_dir = repo_country_pack_dir(&iso);
    if repo_pack_dir.exists() {
        let destination = managed_country_pack_dir(&app, &iso)?;
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&repo_pack_dir, &destination)?;
        let surface_path = destination
            .join("surfaces")
            .join(format!("{iso}.surface.json"));
        let surface = load_surface_wire(&surface_path)?;
        upsert_country_pack_entry(
            &app,
            &iso,
            "installed",
            Some(surface.surface_version.clone()),
            surface.cells_res8.len(),
            Some("repo_pack_copy".to_string()),
        )?;
        country_map_context_cache()
            .lock()
            .map_err(|_| "country_map_context cache poisoned".to_string())?
            .remove(&iso);
        return Ok(InstallResult {
            country_iso2: iso,
            ok: true,
            message: format!(
                "Installed country pack to {}",
                destination.to_string_lossy()
            ),
        });
    }

    let source = repo_demand_surfaces_root().join(format!("{iso}.surface.json"));
    if !source.exists() {
        return Ok(InstallResult {
            country_iso2: iso,
            ok: false,
            message: "Country surface pack not found in local repository".to_string(),
        });
    }
    let surface = load_surface_wire(&source)?;
    let destination = managed_demand_surface_file(&app, &iso)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(&source, &destination).map_err(|e| e.to_string())?;
    upsert_country_pack_entry(
        &app,
        &iso,
        "installed",
        Some(surface.surface_version.clone()),
        surface.cells_res8.len(),
        Some("repo_copy".to_string()),
    )?;
    Ok(InstallResult {
        country_iso2: iso,
        ok: true,
        message: format!(
            "Installed country pack to {}",
            destination.to_string_lossy()
        ),
    })
}

#[command]
pub fn uninstall_country_pack(
    app: AppHandle,
    country_iso2: String,
) -> Result<UninstallResult, String> {
    let iso = country_iso2.trim().to_ascii_uppercase();
    if iso.len() != 2 {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    }
    let managed_pack_dir = managed_country_pack_dir(&app, &iso)?;
    if managed_pack_dir.exists() {
        fs::remove_dir_all(&managed_pack_dir).map_err(|e| e.to_string())?;
    }
    let managed = managed_demand_surface_file(&app, &iso)?;
    if managed.exists() {
        fs::remove_file(&managed).map_err(|e| e.to_string())?;
    }
    upsert_country_pack_entry(&app, &iso, "missing", None, 0, Some("removed".to_string()))?;
    country_map_context_cache()
        .lock()
        .map_err(|_| "country_map_context cache poisoned".to_string())?
        .remove(&iso);
    Ok(UninstallResult {
        country_iso2: iso,
        ok: true,
        message: "Country pack removed from managed storage".to_string(),
    })
}

pub(crate) fn primary_project_country_iso2(manifest: &ProjectManifest) -> Option<String> {
    manifest
        .start_location
        .as_ref()
        .map(|start| start.country_iso2.trim().to_ascii_uppercase())
        .filter(|iso| iso.len() == 2)
        .or_else(|| unlocked_country_codes(manifest).into_iter().next())
}
