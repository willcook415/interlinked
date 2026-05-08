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
    let Some(iso) = normalize_country_iso2(country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    Ok(country_packs_root(app)?.join(&iso))
}

fn repo_country_pack_dir(country_iso2: &str) -> PathBuf {
    repo_country_packs_root().join(
        normalize_country_iso2(country_iso2)
            .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase()),
    )
}

fn hard_link_or_copy(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    match fs::hard_link(src, dst) {
        Ok(_) => Ok(()),
        Err(_) => {
            fs::copy(src, dst)
                .map_err(|e| format!("{} -> {}: {e}", src.display(), dst.display()))?;
            Ok(())
        }
    }
}

fn canonical_map_pack_version(pack_dir: &Path) -> Option<String> {
    let map_dir = pack_dir.join("map");
    let has_world = map_dir.join("world_context.geojson").exists();
    if !has_world {
        return None;
    }
    if map_dir.join("basemap.mbtiles").exists() || map_dir.join("gb_basemap.mbtiles").exists() {
        return Some("vector-mbtiles-v1".to_string());
    }
    if map_dir.join("county_basemap_mid").exists() || map_dir.join("county_basemap_full").exists() {
        return Some("geojson-basemap-v2".to_string());
    }
    if map_dir.join("county_roads").exists() {
        return Some("geojson-roads-v1".to_string());
    }
    None
}

// Canonical flagship pack normalization (Package E):
// - canonical country identity on disk uses UK
// - canonical surface/map filenames are normalized
// - GB naming is retained as compatibility-only where needed
fn canonicalize_pack_layout(pack_dir: &Path, canonical_iso2: &str) -> Result<(), String> {
    let canonical_iso2 = canonical_iso2.trim().to_ascii_uppercase();
    let surfaces_dir = pack_dir.join("surfaces");
    if surfaces_dir.exists() {
        let canonical_surface = surfaces_dir.join(format!("{canonical_iso2}.surface.json"));
        if !canonical_surface.exists() {
            let mut source_surface = country_iso2_runtime_candidates(&canonical_iso2)
                .into_iter()
                .map(|iso| surfaces_dir.join(format!("{iso}.surface.json")))
                .find(|path| path.exists());
            if source_surface.is_none() {
                source_surface = fs::read_dir(&surfaces_dir)
                    .ok()
                    .into_iter()
                    .flat_map(|entries| entries.flatten())
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .map(|name| name.to_ascii_lowercase().ends_with(".surface.json"))
                            .unwrap_or(false)
                    });
            }
            if let Some(source) = source_surface {
                hard_link_or_copy(&source, &canonical_surface)?;
            }
        }
    }

    let map_dir = pack_dir.join("map");
    if map_dir.exists() {
        let canonical_basemap = map_dir.join("basemap.mbtiles");
        let legacy_basemap = map_dir.join("gb_basemap.mbtiles");
        if !canonical_basemap.exists() && legacy_basemap.exists() {
            hard_link_or_copy(&legacy_basemap, &canonical_basemap)?;
        }
        let canonical_major_roads = map_dir.join("major_roads.geojson");
        let legacy_major_roads = map_dir.join("gb_major_roads.geojson");
        if !canonical_major_roads.exists() && legacy_major_roads.exists() {
            hard_link_or_copy(&legacy_major_roads, &canonical_major_roads)?;
        }
    }

    let manifest_path = pack_dir.join("manifest.json");
    let mut manifest = if manifest_path.exists() {
        read_json_file::<JsonValue>(&manifest_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !manifest.is_object() {
        manifest = serde_json::json!({});
    }
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| "manifest root must be object".to_string())?;
    let schema_version = object
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .max(2);
    object.insert(
        "schema_version".to_string(),
        JsonValue::from(schema_version),
    );
    object.insert(
        "country_iso2".to_string(),
        JsonValue::String(canonical_iso2.clone()),
    );
    object.insert(
        "surface_file".to_string(),
        JsonValue::String(format!("surfaces/{canonical_iso2}.surface.json")),
    );
    if object
        .get("regions_file")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        object.insert(
            "regions_file".to_string(),
            JsonValue::String("regions.geojson".to_string()),
        );
    }
    object.insert(
        "region_provider_model".to_string(),
        JsonValue::String(DEFAULT_REGION_PROVIDER_MODEL.to_string()),
    );
    if canonical_iso2 == CANONICAL_UK_ISO2 {
        object.insert(
            "compatibility_country_aliases".to_string(),
            serde_json::json!([UK_COMPAT_GB_ISO2]),
        );
    }
    if map_dir.join("world_context.geojson").exists() {
        object.insert(
            "world_context_file".to_string(),
            JsonValue::String("map/world_context.geojson".to_string()),
        );
    }
    if map_dir.join("major_roads.geojson").exists() {
        object.insert(
            "major_roads_file".to_string(),
            JsonValue::String("map/major_roads.geojson".to_string()),
        );
    } else if map_dir.join("gb_major_roads.geojson").exists() {
        object.insert(
            "major_roads_file".to_string(),
            JsonValue::String("map/gb_major_roads.geojson".to_string()),
        );
    }
    if let Some(map_pack_version) = canonical_map_pack_version(pack_dir) {
        object.insert(
            "map_pack_version".to_string(),
            JsonValue::String(map_pack_version),
        );
    }
    write_json_file(&manifest_path, &manifest)
}

fn migrate_managed_uk_pack_alias(app: &AppHandle) -> Result<(), String> {
    let canonical_dir = managed_country_pack_dir(app, CANONICAL_UK_ISO2)?;
    let legacy_dir = managed_country_pack_dir(app, UK_COMPAT_GB_ISO2)?;

    if !canonical_dir.exists() && legacy_dir.exists() {
        match fs::rename(&legacy_dir, &canonical_dir) {
            Ok(_) => {}
            Err(_) => {
                copy_dir_recursive(&legacy_dir, &canonical_dir)?;
                fs::remove_dir_all(&legacy_dir).map_err(|e| e.to_string())?;
            }
        }
    }
    if canonical_dir.exists() {
        canonicalize_pack_layout(&canonical_dir, CANONICAL_UK_ISO2)?;
    }
    Ok(())
}

pub(crate) fn country_pack_dir(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    if is_uk_country_iso2(country_iso2) {
        let _ = migrate_managed_uk_pack_alias(app);
    }
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        let managed = managed_country_pack_dir(app, &candidate_iso).ok()?;
        if managed.exists() {
            return Some(managed);
        }
    }
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        let repo = repo_country_pack_dir(&candidate_iso);
        if repo.exists() {
            return Some(repo);
        }
    }
    None
}

pub(crate) fn rollout_supported_countries() -> BTreeSet<String> {
    [CANONICAL_UK_ISO2]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DemandSurfaceSource {
    ManagedPack,
    RepoPack,
    ManagedLegacy,
    RepoLegacy,
}

impl DemandSurfaceSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ManagedPack => "managed_pack",
            Self::RepoPack => "repo_pack",
            Self::ManagedLegacy => "managed_legacy_surface",
            Self::RepoLegacy => "repo_legacy_surface",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDemandSurfacePath {
    pub(crate) path: PathBuf,
    pub(crate) source: DemandSurfaceSource,
}

// Demand-surface source authority contract:
// 1) managed country pack surface,
// 2) repo country pack surface,
// 3) managed legacy surface store (compatibility only),
// 4) repo legacy surface store (compatibility only).
pub(crate) fn resolve_demand_surface_path(
    app: &AppHandle,
    country_iso2: &str,
) -> Option<ResolvedDemandSurfacePath> {
    let Some(canonical_iso) = canonical_country_iso2(country_iso2) else {
        return None;
    };
    let candidates = country_iso2_runtime_candidates(&canonical_iso);
    for candidate_iso in &candidates {
        let managed_pack_surface = managed_country_pack_dir(app, candidate_iso)
            .ok()?
            .join("surfaces")
            .join(format!("{candidate_iso}.surface.json"));
        if managed_pack_surface.exists() {
            return Some(ResolvedDemandSurfacePath {
                path: managed_pack_surface,
                source: DemandSurfaceSource::ManagedPack,
            });
        }
        let repo_pack_surface = repo_country_pack_dir(candidate_iso)
            .join("surfaces")
            .join(format!("{candidate_iso}.surface.json"));
        if repo_pack_surface.exists() {
            return Some(ResolvedDemandSurfacePath {
                path: repo_pack_surface,
                source: DemandSurfaceSource::RepoPack,
            });
        }
    }
    for candidate_iso in &candidates {
        let managed = demand_surfaces_root(app)
            .ok()?
            .join(format!("{candidate_iso}.surface.json"));
        if managed.exists() {
            return Some(ResolvedDemandSurfacePath {
                path: managed,
                source: DemandSurfaceSource::ManagedLegacy,
            });
        }
        let repo = repo_demand_surfaces_root().join(format!("{candidate_iso}.surface.json"));
        if repo.exists() {
            return Some(ResolvedDemandSurfacePath {
                path: repo,
                source: DemandSurfaceSource::RepoLegacy,
            });
        }
    }
    None
}

fn managed_demand_surface_file(app: &AppHandle, country_iso2: &str) -> Result<PathBuf, String> {
    let Some(iso) = canonical_country_iso2(country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    Ok(demand_surfaces_root(app)?.join(format!("{iso}.surface.json")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapAssetSource {
    ManagedPack,
    RepoPack,
    RepoFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedMapAssetPath {
    pub(crate) path: PathBuf,
    pub(crate) source: MapAssetSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapRuntimeTier {
    VectorMbtilesV1,
    GeojsonBasemapV2,
    GeojsonRoadsV1,
    Missing,
}

impl MapRuntimeTier {
    pub(crate) fn as_version(self) -> &'static str {
        match self {
            Self::VectorMbtilesV1 => "vector-mbtiles-v1",
            Self::GeojsonBasemapV2 => "geojson-basemap-v2",
            Self::GeojsonRoadsV1 => "geojson-roads-v1",
            Self::Missing => "missing",
        }
    }

    pub(crate) fn map_ready(self) -> bool {
        !matches!(self, Self::Missing)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedMapAssets {
    pub(crate) world_context: Option<ResolvedMapAssetPath>,
    pub(crate) counties: Option<ResolvedMapAssetPath>,
    pub(crate) style_template: Option<ResolvedMapAssetPath>,
    pub(crate) basemap_mbtiles: Option<ResolvedMapAssetPath>,
    pub(crate) major_roads: Option<ResolvedMapAssetPath>,
    pub(crate) county_roads_dir: Option<ResolvedMapAssetPath>,
    pub(crate) county_basemap_mid_dir: Option<ResolvedMapAssetPath>,
    pub(crate) county_basemap_full_dir: Option<ResolvedMapAssetPath>,
}

impl ResolvedMapAssets {
    pub(crate) fn world_context_requires_country_remap(&self) -> bool {
        self.world_context
            .as_ref()
            .and_then(|entry| entry.path.file_name().and_then(|v| v.to_str()))
            .map(|name| name.eq_ignore_ascii_case("countries.geojson"))
            .unwrap_or(false)
    }

    pub(crate) fn county_roads_file(&self, county_id: &str) -> Option<PathBuf> {
        let county = county_id.trim();
        if county.is_empty() {
            return None;
        }
        let dir = self.county_roads_dir.as_ref()?;
        let path = dir.path.join(format!("{county}.geojson"));
        path.exists().then_some(path)
    }

    pub(crate) fn county_basemap_mid_file(&self, county_id: &str) -> Option<PathBuf> {
        let county = county_id.trim();
        if county.is_empty() {
            return None;
        }
        if let Some(dir) = self.county_basemap_mid_dir.as_ref() {
            let path = dir.path.join(format!("{county}.geojson"));
            if path.exists() {
                return Some(path);
            }
        }
        self.county_roads_file(county)
    }

    pub(crate) fn county_basemap_full_file(&self, county_id: &str) -> Option<PathBuf> {
        let county = county_id.trim();
        if county.is_empty() {
            return None;
        }
        if let Some(dir) = self.county_basemap_full_dir.as_ref() {
            let path = dir.path.join(format!("{county}.geojson"));
            if path.exists() {
                return Some(path);
            }
        }
        self.county_roads_file(county)
    }

    pub(crate) fn has_county_roads(&self) -> bool {
        self.county_roads_dir.is_some()
    }

    pub(crate) fn has_mid_basemap(&self) -> bool {
        self.county_basemap_mid_dir.is_some()
    }

    pub(crate) fn has_full_basemap(&self) -> bool {
        self.county_basemap_full_dir.is_some()
    }

    pub(crate) fn vector_ready(&self) -> bool {
        self.world_context.is_some()
            && self.basemap_mbtiles.is_some()
            && self.style_template.is_some()
    }

    pub(crate) fn runtime_tier(&self) -> MapRuntimeTier {
        // Required runtime pack tiers:
        // - vector: world_context + basemap.mbtiles + style template
        // - geojson basemap: world_context + county_basemap_mid/full
        // - roads-only: world_context + county_roads
        if self.vector_ready() {
            return MapRuntimeTier::VectorMbtilesV1;
        }
        if self.world_context.is_some() && (self.has_mid_basemap() || self.has_full_basemap()) {
            return MapRuntimeTier::GeojsonBasemapV2;
        }
        if self.world_context.is_some() && self.has_county_roads() {
            return MapRuntimeTier::GeojsonRoadsV1;
        }
        MapRuntimeTier::Missing
    }

    pub(crate) fn map_ready(&self) -> bool {
        self.runtime_tier().map_ready()
    }

    pub(crate) fn map_installed(&self) -> bool {
        self.map_ready() || (self.world_context.is_some() && self.major_roads.is_some())
    }
}

fn managed_pack_map_root(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        let candidate = managed_country_pack_dir(app, &candidate_iso)
            .ok()
            .map(|dir| dir.join("map"))
            .filter(|dir| dir.exists());
        if candidate.is_some() {
            return candidate;
        }
    }
    None
}

fn repo_pack_map_root(country_iso2: &str) -> Option<PathBuf> {
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        let dir = repo_country_pack_dir(&candidate_iso).join("map");
        if dir.exists() {
            return Some(dir);
        }
    }
    None
}

fn prefer_pack_file(
    managed_map_root: Option<&PathBuf>,
    repo_map_root: Option<&PathBuf>,
    rel_path: &str,
) -> Option<ResolvedMapAssetPath> {
    if let Some(root) = managed_map_root {
        let candidate = root.join(rel_path);
        if candidate.exists() {
            return Some(ResolvedMapAssetPath {
                path: candidate,
                source: MapAssetSource::ManagedPack,
            });
        }
    }
    if let Some(root) = repo_map_root {
        let candidate = root.join(rel_path);
        if candidate.exists() {
            return Some(ResolvedMapAssetPath {
                path: candidate,
                source: MapAssetSource::RepoPack,
            });
        }
    }
    None
}

fn prefer_pack_dir(
    managed_map_root: Option<&PathBuf>,
    repo_map_root: Option<&PathBuf>,
    rel_path: &str,
) -> Option<ResolvedMapAssetPath> {
    if let Some(root) = managed_map_root {
        let candidate = root.join(rel_path);
        if candidate.exists() {
            return Some(ResolvedMapAssetPath {
                path: candidate,
                source: MapAssetSource::ManagedPack,
            });
        }
    }
    if let Some(root) = repo_map_root {
        let candidate = root.join(rel_path);
        if candidate.exists() {
            return Some(ResolvedMapAssetPath {
                path: candidate,
                source: MapAssetSource::RepoPack,
            });
        }
    }
    None
}

fn repo_fallback_file(path: PathBuf) -> Option<ResolvedMapAssetPath> {
    path.exists().then_some(ResolvedMapAssetPath {
        path,
        source: MapAssetSource::RepoFallback,
    })
}

static WORLD_CONTEXT_COVERAGE_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>,
> = std::sync::OnceLock::new();

fn world_context_has_minimum_coverage(path: &std::path::Path) -> bool {
    let cache = WORLD_CONTEXT_COVERAGE_CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(lock) = cache.lock() {
        if let Some(cached) = lock.get(path) {
            return *cached;
        }
    }
    let coverage_ok = read_json_file::<JsonValue>(path)
        .ok()
        .and_then(|value| value.get("features").and_then(|v| v.as_array()).cloned())
        .map(|features| {
            let mut seen = std::collections::HashSet::<String>::new();
            for feature in features {
                let iso = feature
                    .get("properties")
                    .and_then(|props| props.get("country_iso2"))
                    .and_then(|v| v.as_str())
                    .map(|v| v.trim().to_ascii_uppercase())
                    .filter(|v| v.len() == 2);
                if let Some(iso) = iso {
                    seen.insert(iso);
                }
            }
            ["FR", "NO", "DE", "US", "CN"]
                .iter()
                .all(|iso| seen.contains(*iso))
        })
        .unwrap_or(false);
    if let Ok(mut lock) = cache.lock() {
        lock.insert(path.to_path_buf(), coverage_ok);
    }
    coverage_ok
}

// Runtime map-asset authority contract:
// 1) country-pack map assets are the runtime authority (managed pack first, then repo pack),
// 2) repo-level boundary/style files are explicit fallback-only compatibility inputs.
// This keeps one resolver/precedence model for both config generation and HTTP serving.
pub(crate) fn resolve_map_assets(app: &AppHandle, country_iso2: &str) -> ResolvedMapAssets {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    let managed_map_root = managed_pack_map_root(app, &iso);
    let repo_map_root = repo_pack_map_root(&iso);

    let pack_world_context = prefer_pack_file(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "world_context.geojson",
    );
    let vetted_pack_world_context = pack_world_context
        .clone()
        .filter(|entry| world_context_has_minimum_coverage(&entry.path));
    // Product-correctness hotfix: fall back to canonical countries.geojson remap when
    // pack world_context is missing major locked countries (e.g. FR/NO omissions).
    let world_context = vetted_pack_world_context
        .or_else(|| repo_fallback_file(repo_boundaries_root().join("countries.geojson")))
        .or(pack_world_context)
        .or_else(|| repo_fallback_file(repo_boundaries_root().join("world_context.geojson")));

    let counties = prefer_pack_file(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "counties.geojson",
    )
    .or_else(|| {
        (is_uk_country_iso2(&iso))
            .then(|| repo_boundaries_root().join("gb_ceremonial_counties_canonical.geojson"))
            .and_then(repo_fallback_file)
    });

    let style_template = prefer_pack_file(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "style/interlinked-light.json",
    )
    .or_else(|| repo_fallback_file(repo_map_style_root().join("interlinked-light.json")));

    let basemap_mbtiles = prefer_pack_file(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "basemap.mbtiles",
    )
    .or_else(|| {
        if is_uk_country_iso2(&iso) {
            prefer_pack_file(
                managed_map_root.as_ref(),
                repo_map_root.as_ref(),
                "gb_basemap.mbtiles",
            )
        } else {
            None
        }
    });

    let major_roads = prefer_pack_file(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "major_roads.geojson",
    )
    .or_else(|| {
        if is_uk_country_iso2(&iso) {
            prefer_pack_file(
                managed_map_root.as_ref(),
                repo_map_root.as_ref(),
                "gb_major_roads.geojson",
            )
        } else {
            None
        }
    });

    let county_roads_dir = prefer_pack_dir(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "county_roads",
    );
    let county_basemap_mid_dir = prefer_pack_dir(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "county_basemap_mid",
    );
    let county_basemap_full_dir = prefer_pack_dir(
        managed_map_root.as_ref(),
        repo_map_root.as_ref(),
        "county_basemap_full",
    );

    ResolvedMapAssets {
        world_context,
        counties,
        style_template,
        basemap_mbtiles,
        major_roads,
        county_roads_dir,
        county_basemap_mid_dir,
        county_basemap_full_dir,
    }
}

pub(crate) fn counties_file(app: &AppHandle, country_iso2: &str) -> Option<PathBuf> {
    resolve_map_assets(app, country_iso2)
        .counties
        .map(|entry| entry.path)
}

pub(crate) fn county_roads_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    resolve_map_assets(app, country_iso2).county_roads_file(county_id)
}

pub(crate) fn county_basemap_mid_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    resolve_map_assets(app, country_iso2).county_basemap_mid_file(county_id)
}

pub(crate) fn county_basemap_full_file(
    app: &AppHandle,
    country_iso2: &str,
    county_id: &str,
) -> Option<PathBuf> {
    resolve_map_assets(app, country_iso2).county_basemap_full_file(county_id)
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
    let iso = canonical_country_iso2(country_iso2)
        .ok_or_else(|| "country_iso2 must be two-letter ISO code".to_string())?;
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

const DEFAULT_REGION_PROVIDER_MODEL: &str = "planning_surface_res6_v1";

#[derive(Debug, Clone, Deserialize, Default)]
struct RuntimeCountryPackManifestWire {
    #[serde(default)]
    country_iso2: String,
    #[serde(default)]
    surface_file: String,
    #[serde(default)]
    regions_file: String,
    #[serde(default)]
    region_provider_model: Option<String>,
}

fn resolve_runtime_pack_iso2(app: &AppHandle, country_iso2: &str) -> Option<String> {
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        let managed = managed_country_pack_dir(app, &candidate_iso).ok()?;
        if managed.exists() {
            return Some(candidate_iso);
        }
    }
    for candidate_iso in country_iso2_runtime_candidates(country_iso2) {
        if repo_country_pack_dir(&candidate_iso).exists() {
            return Some(candidate_iso);
        }
    }
    None
}

fn evaluate_runtime_pack_contract(
    app: &AppHandle,
    country_iso2: &str,
) -> (bool, Option<String>, Option<String>) {
    let Some(runtime_pack_iso2) = resolve_runtime_pack_iso2(app, country_iso2) else {
        return (false, None, None);
    };
    let Some(pack_dir) = country_pack_dir(app, &runtime_pack_iso2) else {
        return (false, Some(runtime_pack_iso2), None);
    };

    let manifest_path = pack_dir.join("manifest.json");
    let manifest = if manifest_path.exists() {
        read_json_file::<RuntimeCountryPackManifestWire>(&manifest_path).ok()
    } else {
        None
    };
    // Transitional compatibility: legacy packs may not ship manifest.json yet.
    // Runtime still validates essential files + provider model with stable defaults.

    let provider_model = manifest
        .as_ref()
        .and_then(|wire| wire.region_provider_model.clone())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some(DEFAULT_REGION_PROVIDER_MODEL.to_string()));
    let surface_rel = manifest
        .as_ref()
        .map(|wire| wire.surface_file.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("surfaces/{runtime_pack_iso2}.surface.json"));
    let regions_rel = manifest
        .as_ref()
        .map(|wire| wire.regions_file.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "regions.geojson".to_string());
    let manifest_iso_ok = manifest
        .as_ref()
        .and_then(|wire| canonical_country_iso2(&wire.country_iso2))
        .map(|wire_iso| wire_iso == canonical_country_iso2(country_iso2).unwrap_or_default())
        .unwrap_or(true);
    let surface_ok = pack_dir.join(surface_rel).exists();
    let regions_ok = pack_dir.join(regions_rel).exists();
    let provider_ok = provider_model
        .as_ref()
        .map(|model| model.starts_with("planning_surface_"))
        .unwrap_or(false);
    (
        manifest_iso_ok && surface_ok && regions_ok && provider_ok,
        Some(runtime_pack_iso2),
        provider_model,
    )
}

pub(crate) fn country_pack_status_for(
    app: &AppHandle,
    index: &CountryPackIndex,
    supported_rollout: &BTreeSet<String>,
    country_iso2: &str,
) -> CountryPackStatus {
    let iso = canonical_country_iso2(country_iso2)
        .unwrap_or_else(|| country_iso2.trim().to_ascii_uppercase());
    if is_uk_country_iso2(&iso) {
        let _ = migrate_managed_uk_pack_alias(app);
    }
    let idx_entry = index.packs.iter().find(|p| p.country_iso2 == iso);
    let resolved_surface = resolve_demand_surface_path(app, &iso);
    let surface_version = idx_entry.and_then(|p| p.surface_version.clone());
    let cells_count = idx_entry.map(|p| p.cells_count).unwrap_or(0);
    let mut last_updated_at = idx_entry.and_then(|p| p.last_updated_at.clone());
    let mut build_state = idx_entry
        .map(|p| p.build_state.clone())
        .unwrap_or_else(|| "missing".to_string());
    if resolved_surface.is_some() {
        // Deferred expensive load_surface_wire block.
        // We use index cache or fallback to 0/None to keep startup listing extremely fast.
        if build_state != "building" {
            build_state = "installed".to_string();
        }
        if last_updated_at.is_none() {
            last_updated_at = Some(now_string());
        }
    } else if build_state == "installed" {
        build_state = "missing".to_string();
    }

    let resolved_map = resolve_map_assets(app, &iso);
    let map_tier = resolved_map.runtime_tier();
    let map_installed = resolved_map.map_installed();
    let map_ready = resolved_map.map_ready();
    let map_size_bytes = country_pack_dir(app, &iso)
        .map(|dir| dir.join("map"))
        .filter(|dir| dir.exists())
        .map(|dir| directory_size_bytes(&dir));
    let demand_installed = resolved_surface.is_some();
    let fully_playable = demand_installed && map_ready;
    let map_pack_version = Some(map_tier.as_version().to_string());
    let supported = supported_rollout.contains(&iso);
    let (pack_contract_valid, runtime_pack_country_iso2, region_provider_model) =
        evaluate_runtime_pack_contract(app, &iso);
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
        canonical_country_iso2: canonical_country_iso2(country_iso2),
        runtime_pack_country_iso2,
        region_provider_model,
        pack_contract_valid,
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
        ("UK", "United Kingdom"),
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
        "UK" => vec![
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

fn constrain_uk_start_cities(cities: Vec<CityOption>) -> Vec<CityOption> {
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
    let Some(iso) = canonical_country_iso2(country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    let city_catalog_path =
        country_iso2_runtime_candidates(&iso)
            .into_iter()
            .find_map(|candidate| {
                location_catalog_file(app, &Path::new("cities").join(format!("{candidate}.json")))
            });
    if let Some(path) = city_catalog_path {
        let raw: Vec<CatalogCityWire> = read_json_file(&path)?;
        let capital_id = location_catalog_file(app, Path::new("capitals.json"))
            .and_then(|cap_path| read_json_file::<HashMap<String, i64>>(&cap_path).ok())
            .and_then(|caps| {
                country_iso2_runtime_candidates(&iso)
                    .iter()
                    .find_map(|candidate| caps.get(candidate).copied())
            });
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
        if is_uk_country_iso2(&iso) {
            let constrained = constrain_uk_start_cities(out);
            if !constrained.is_empty() {
                return Ok(constrained);
            }
            let fallback = constrain_uk_start_cities(fallback_cities(&iso));
            if !fallback.is_empty() {
                return Ok(fallback);
            }
            return Err("no UK start cities available".to_string());
        }
        return Ok(out);
    }
    let fallback = fallback_cities(&iso);
    if fallback.is_empty() {
        return Err(format!("no cities available for country {iso}"));
    }
    if is_uk_country_iso2(&iso) {
        return Ok(constrain_uk_start_cities(fallback));
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
            .map(|c| {
                let iso2 = canonical_country_iso2(&c.iso2).unwrap_or(c.iso2.clone());
                CountryOption {
                    name: if is_uk_country_iso2(&iso2) {
                        "United Kingdom".to_string()
                    } else {
                        c.name
                    },
                    iso2,
                }
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.iso2.cmp(&b.iso2));
        out.dedup_by(|a, b| a.iso2 == b.iso2);
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
    let Some(iso) = canonical_country_iso2(&country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };

    let mut installed_from_pack_iso = None::<String>;
    let mut repo_pack_dir = None::<PathBuf>;
    for candidate_iso in country_iso2_runtime_candidates(&iso) {
        let candidate_repo = repo_country_pack_dir(&candidate_iso);
        if candidate_repo.exists() {
            installed_from_pack_iso = Some(candidate_iso);
            repo_pack_dir = Some(candidate_repo);
            break;
        }
    }
    if let (Some(source_pack_iso), Some(repo_pack_dir)) = (installed_from_pack_iso, repo_pack_dir) {
        let destination = managed_country_pack_dir(&app, &iso)?;
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
        }
        copy_dir_recursive(&repo_pack_dir, &destination)?;
        canonicalize_pack_layout(&destination, &iso)?;

        if is_uk_country_iso2(&iso) {
            let legacy = managed_country_pack_dir(&app, UK_COMPAT_GB_ISO2)?;
            if legacy.exists() && legacy != destination {
                fs::remove_dir_all(&legacy).map_err(|e| e.to_string())?;
            }
        }

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
        country_map_context_cache()
            .lock()
            .map_err(|_| "country_map_context cache poisoned".to_string())?
            .remove(UK_COMPAT_GB_ISO2);
        return Ok(InstallResult {
            country_iso2: iso.clone(),
            ok: true,
            message: format!(
                "Installed canonical country pack ({source_pack_iso} -> {}) to {}",
                iso,
                destination.to_string_lossy()
            ),
        });
    }

    let source = country_iso2_runtime_candidates(&iso)
        .into_iter()
        .map(|candidate_iso| {
            repo_demand_surfaces_root().join(format!("{candidate_iso}.surface.json"))
        })
        .find(|candidate| candidate.exists());
    let Some(source) = source else {
        return Ok(InstallResult {
            country_iso2: iso,
            ok: false,
            message: "Country surface pack not found in local repository".to_string(),
        });
    };
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
    let Some(iso) = canonical_country_iso2(&country_iso2) else {
        return Err("country_iso2 must be two-letter ISO code".to_string());
    };
    for candidate_iso in country_iso2_runtime_candidates(&iso) {
        let managed_pack_dir = managed_country_pack_dir(&app, &candidate_iso)?;
        if managed_pack_dir.exists() {
            fs::remove_dir_all(&managed_pack_dir).map_err(|e| e.to_string())?;
        }
        let managed_surface =
            demand_surfaces_root(&app)?.join(format!("{candidate_iso}.surface.json"));
        if managed_surface.exists() {
            fs::remove_file(&managed_surface).map_err(|e| e.to_string())?;
        }
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
    country_map_context_cache()
        .lock()
        .map_err(|_| "country_map_context cache poisoned".to_string())?
        .remove(UK_COMPAT_GB_ISO2);
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
        .and_then(|start| canonical_country_iso2(&start.country_iso2))
        .or_else(|| unlocked_country_codes(manifest).into_iter().next())
}
