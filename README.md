# Interlinked

Interlinked is a transport simulation and planning sandbox built around a Rust simulation engine, a Tauri desktop app, and data-ingestion tools for scenario generation.

## What Is In This Repo

- `crates/interlinked-engine`: Core simulation model, scenario schema/services, and test suites.
- `crates/interlinked-cli`: Command-line interface for running and migrating scenario files.
- `crates/interlinked-osm`: Data backbone tooling for OSM/GTFS/census-driven scenario and demand-surface workflows.
- `apps/interlinked-desktop`: Tauri + React desktop experience.
- `docs/`: Design notes and phased implementation docs.
- `scripts/`: Utility scripts for pack/surface generation tasks.

## Prerequisites

- Rust (stable) and Cargo
- Node.js and npm (for `apps/interlinked-desktop`)
- Tauri platform prerequisites: https://v2.tauri.app/start/prerequisites/

## Quick Start

### 1. Clone and install desktop dependencies

```powershell
git clone https://github.com/willcook415/interlinked
cd interlinked
cd apps/interlinked-desktop
npm install
```

### 2. Run the desktop app (dev mode)

```powershell
npm run tauri dev
```

### 3. Run Rust checks/tests from repo root

```powershell
cd ../..
cargo check --workspace
cargo test --workspace
```

## CLI Usage

Show CLI help:

```powershell
cargo run -p interlinked-cli -- --help
```

Run a scenario and write `results.json` next to the input file:

```powershell
cargo run -p interlinked-cli -- run crates/interlinked-engine/tests/fixtures/scenario_small_city.json
```

Run with explicit output:

```powershell
cargo run -p interlinked-cli -- run crates/interlinked-engine/tests/fixtures/scenario_small_city.json --out .\results.json
```

Migrate a scenario to the wrapped canonical schema:

```powershell
cargo run -p interlinked-cli -- migrate path\to\scenario.json --out path\to\scenario.migrated.json
```

Or migrate in place:

```powershell
cargo run -p interlinked-cli -- migrate path\to\scenario.json --in-place
```

## Data Tooling

The OSM/country-pack tooling lives in `interlinked-osm`.

```powershell
cargo run -p interlinked-osm -- --help
```

Because these workflows depend on large external datasets, they are typically run with local data files under `data/`.

## Large Files and Generated Data

This repo intentionally ignores large/generated artifacts such as:

- `data/`
- `target/`
- frontend build outputs

If you need reproducible data workflows, capture command recipes in `docs/` or `scripts/`, and keep committed fixtures small.

## Useful Commands

```powershell
# Rust workspace
cargo check --workspace
cargo test --workspace

# Desktop frontend only (in apps/interlinked-desktop)
npm run dev
npm run build
npm run tauri dev

# CLIs
cargo run -p interlinked-cli -- --help
cargo run -p interlinked-osm -- --help
```
