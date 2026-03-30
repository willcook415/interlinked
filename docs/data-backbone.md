# Data Backbone Commands

This project now has a staged real-data pipeline in `interlinked-osm`.

## 1) OSM ingestion v2

```bash
cargo run -p interlinked-osm -- import-pbf ^
  --pbf data/osm/leeds.osm.pbf ^
  --name leeds_v2 ^
  --out-root data/osm_import ^
  --bbox -1.90 53.65 -1.25 53.98 ^
  --snap-m 60 ^
  --inferred-headway-s 600 ^
  --cleanup-topology true ^
  --infer-services true
```

Output: `data/osm_import/<name>/scenario.json`

## 2) LSOA normalization (West Yorkshire)

Use the raw LSOA files in `data/census/lsoa` and generate canonical rows:

```bash
cargo run -p interlinked-osm -- normalize-lsoa ^
  --population-csv "data/census/lsoa/mid 2024 lsoa.csv" ^
  --jobs-csv data/census/lsoa/810508442665481.csv ^
  --centroids-csv data/census/lsoa/lsoa_centroids_wgs84.csv ^
  --out data/census/lsoa/wy_lsoa_normalized.csv ^
  --region west_yorkshire ^
  --target-crs epsg3857
```

Output schema is strict: `zone_id,x,y,population,jobs`.

## 3) Census attach (zone replace mode)

Replace scenario zones with normalized LSOA zones:

```bash
cargo run -p interlinked-osm -- attach-census ^
  --scenario data/osm_import/leeds_v2/scenario.json ^
  --csv data/census/lsoa/wy_lsoa_normalized.csv ^
  --replace-zones ^
  --profile-csv data/census/lsoa/demand_profile.csv ^
  --out data/osm_import/leeds_v2/scenario.lsoa.json
```

Without `--replace-zones`, attach keeps legacy update behavior (match by `zone_id`/nearest x,y).

## 4) GTFS merge

```bash
cargo run -p interlinked-osm -- import-gtfs ^
  --scenario data/osm_import/leeds_v2/scenario.lsoa.json ^
  --gtfs-dir data/gtfs/west_yorkshire ^
  --snap-m 80 ^
  --default-headway-s 600
```

Default output is sibling `scenario.gtfs.json` unless `--out` is passed.

## 5) Worldwide demand fabric (H3)

Build deterministic demand cells from OSM + optional raster CSV samples:

```bash
cargo run -p interlinked-osm -- build-demand-fabric ^
  --pbf data/osm/leeds.osm.pbf ^
  --bbox -1.90 53.65 -1.25 53.98 ^
  --country-iso2 GB ^
  --h3-res 8 ^
  --target-crs epsg3857 ^
  --population-raster-csv data/demand/leeds/population.csv ^
  --built-raster-csv data/demand/leeds/built.csv ^
  --out data/demand/leeds/demand_fabric.json
```

Raster CSV contract: `lon,lat,value`.

Apply demand fabric to a scenario:

```bash
cargo run -p interlinked-osm -- apply-demand-fabric ^
  --scenario data/osm_import/leeds_v2/scenario.gtfs.json ^
  --fabric data/demand/leeds/demand_fabric.json ^
  --replace-zones true ^
  --out data/osm_import/leeds_v2/scenario.demand.json
```

## 6) Demand Surface V3 (country-scoped runtime packs)

Build a smooth multi-resolution surface for one country:

```bash
cargo run -p interlinked-osm -- build-demand-surface ^
  --pbf data/osm/planet.osm.pbf ^
  --country-iso2 GB ^
  --country-boundaries-geojson data/boundaries/countries.geojson ^
  --population-raster-csv data/rasters/worldpop_samples.csv ^
  --built-raster-csv data/rasters/ghsl_built_samples.csv ^
  --h3-res 8 ^
  --target-crs epsg3857 ^
  --out data/demand_surfaces/GB.surface.json
```

Fast deterministic path (recommended during development on large PBFs):

```bash
cargo run -p interlinked-osm -- build-demand-surface ^
  --pbf data/osm/GBR.osm.pbf ^
  --country-iso2 GB ^
  --country-boundaries-geojson data/boundaries/countries.geojson ^
  --population-raster-csv data/rasters/worldpop_samples.csv ^
  --built-raster-csv data/rasters/ghsl_built_samples.csv ^
  --h3-res 8 ^
  --target-crs epsg3857 ^
  --raster-only ^
  --out data/demand_surfaces/GB.surface.json
```

Build a country pack in one pass:

```bash
cargo run -p interlinked-osm -- build-demand-surface-pack ^
  --pbf data/osm/planet.osm.pbf ^
  --countries GB,FR,DE ^
  --country-boundaries-geojson data/boundaries/countries.geojson ^
  --population-raster-csv data/rasters/worldpop_samples.csv ^
  --built-raster-csv data/rasters/ghsl_built_samples.csv ^
  --h3-res 8 ^
  --target-crs epsg3857 ^
  --out-dir data/demand_surfaces
```

Runtime expects `<ISO2>.surface.json` files in app-managed `demand_surfaces/` (or repo `data/demand_surfaces` for local development).

### Country surface production wrapper (recommended)

Use the PowerShell wrapper for repeatable per-country builds with ISO2->PBF mapping, GDAL clipping/downsampling, CSV conversion, and timing logs:

```bash
powershell -ExecutionPolicy Bypass -File scripts/build_country_surfaces.ps1 `
  -Countries GB,LU,BE,IE,NL `
  -CountryBoundariesGeoJson data/boundaries/countries.geojson `
  -PopulationRaster data/raw/population/worldpop.tif `
  -BuiltRaster data/raw/built/ghsl_built.tif `
  -OsmDir data/osm `
  -OutDir data/demand_surfaces `
  -SampleDeg 0.05
```

Logs are written to `data/demand_surfaces/_build/build_country_surfaces.log`.

### Country pack status and unlock policy

- Runtime key is ISO2 (`GB.surface.json`, `FR.surface.json`, etc).
- UK-first rollout is enforced for new-game eligibility:
  - current rollout set: `GB`
  - next unlock candidates (data QA order): `LU`, `BE`, `IE`, `NL`
  - countries outside rollout return `Coming Soon`.
- A country in rollout without a local/repo surface pack returns `Install Required`.
- Commands:
  - `list_country_pack_status`
  - `install_country_pack`
  - `uninstall_country_pack`

## Contract notes

- Scenario IO remains dual-read, canonical wrapped-write.
- Deterministic simulation mode is required by planning/stateful runtime.
- Seed override is supported through planning/stateful run options.
- Game sessions no longer synthesize demand from fallback city lists; missing country packs are surfaced as explicit diagnostics.
