param(
  [string]$Pbf = "data/osm/GBR.osm.pbf",
  [string]$CountryIso2 = "UK",
  [string]$CountryBoundariesGeoJson = "data/boundaries/countries.geojson",
  [string]$OutDir = "data/country_packs/UK",
  [switch]$SkipPlanetiler,
  [switch]$Force
)

$ErrorActionPreference = "Stop"

function Resolve-JavaPath {
  $javaCmd = Get-Command java -ErrorAction SilentlyContinue
  if ($javaCmd) {
    return $javaCmd.Source
  }
  $fallback = "C:\Program Files\Android\Android Studio\jbr\bin\java.exe"
  if (Test-Path $fallback) {
    return $fallback
  }
  throw "Java runtime not found. Install Java 21+ or Android Studio JBR."
}

$root = (Resolve-Path ".").Path
$pythonDeps = Join-Path $root ".tmp_pydeps"
if (Test-Path $pythonDeps) {
  if ($env:PYTHONPATH) {
    $env:PYTHONPATH = "$pythonDeps;$env:PYTHONPATH"
  } else {
    $env:PYTHONPATH = $pythonDeps
  }
}

if (-not (Test-Path $Pbf) -and -not $SkipPlanetiler) {
  throw "OSM PBF not found: $Pbf"
}
if (-not (Test-Path $CountryBoundariesGeoJson)) {
  throw "Country boundaries GeoJSON not found: $CountryBoundariesGeoJson"
}

$canonicalScript = "scripts/build_gb_ceremonial_counties.py"
if (-not (Test-Path $canonicalScript)) {
  throw "County canonicalization script not found: $canonicalScript"
}

$countryIso2 = $CountryIso2.Trim().ToUpperInvariant()
if ($countryIso2 -eq "GB") {
  $countryIso2 = "UK"
}
if ($countryIso2.Length -ne 2) {
  throw "CountryIso2 must be a two-letter ISO code"
}

Write-Host "Generating canonical GB ceremonial counties"
& python $canonicalScript
if ($LASTEXITCODE -ne 0) {
  throw "build_gb_ceremonial_counties.py failed with exit code $LASTEXITCODE"
}

$mapDir = Join-Path $OutDir "map"
$styleDir = Join-Path $mapDir "style"
New-Item -ItemType Directory -Force $styleDir | Out-Null

Copy-Item "data/boundaries/gb_ceremonial_counties_canonical.geojson" (Join-Path $mapDir "counties.geojson") -Force
Copy-Item "data/boundaries/gb_ceremonial_county_aliases.json" (Join-Path $mapDir "gb_ceremonial_county_aliases.json") -Force
Copy-Item "apps/interlinked-desktop/src-tauri/map_assets/style/interlinked-light.json" (Join-Path $styleDir "interlinked-light.json") -Force

$worldOut = Join-Path $mapDir "world_context.geojson"
@"
import json
from pathlib import Path
import sys

source = Path(r"$CountryBoundariesGeoJson")
out = Path(r"$worldOut")
country_iso2 = sys.argv[1].strip().upper()
data = json.loads(source.read_text(encoding="utf-8"))
features = []
for feature in data.get("features", []):
    props = feature.get("properties") or {}
    iso = (props.get("ISO_A2") or props.get("ISO_A2_EH") or "").strip().upper()
    if iso == "GB":
        iso = "UK"
    if len(iso) != 2:
        continue
    if iso == country_iso2:
        continue
    name = (props.get("NAME_EN") or props.get("ADMIN") or "").strip()
    features.append({
        "type": "Feature",
        "geometry": feature.get("geometry"),
        "properties": {
            "country_iso2": iso,
            "name": name,
            "playable_now": False,
            "coming_soon": True,
        },
    })

out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps({"type": "FeatureCollection", "features": features}, separators=(",", ":")), encoding="utf-8")
"@ | python - $countryIso2
if ($LASTEXITCODE -ne 0) {
  throw "world context generation failed with exit code $LASTEXITCODE"
}

if (-not $SkipPlanetiler) {
  $planetilerJar = Join-Path $root "tools/planetiler/planetiler.jar"
  if (-not (Test-Path $planetilerJar)) {
    throw "Planetiler jar not found: $planetilerJar"
  }

  $java = Resolve-JavaPath
  $mbtiles = Join-Path $mapDir "basemap.mbtiles"
  if ((Test-Path $mbtiles) -and -not $Force) {
    Write-Host "Skipping Planetiler build; basemap already exists. Use -Force to rebuild."
  } else {
    Write-Host "Building $countryIso2 vector basemap MBTiles"
    & $java `
      "-Xmx12g" `
      "-jar" $planetilerJar `
      "--download=true" `
      "--fetch-wikidata=false" `
      "--osm_path=$Pbf" `
      "--output=$mbtiles" `
      "--tile_compression=none" `
      "--force=$([bool]$Force -or -not (Test-Path $mbtiles))"
    if ($LASTEXITCODE -ne 0) {
      throw "Planetiler build failed with exit code $LASTEXITCODE"
    }
  }
}

Write-Host ("$countryIso2 country map pack assembled at {0}" -f (Resolve-Path $OutDir))
