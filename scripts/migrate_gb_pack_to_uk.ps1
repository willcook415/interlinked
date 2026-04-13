param(
  [string]$SourceIso2 = "GB",
  [string]$CanonicalIso2 = "UK",
  [string]$CountryPacksRoot = "data/country_packs",
  [switch]$Force
)

$ErrorActionPreference = "Stop"

function Resolve-Iso2 {
  param([string]$Iso2)
  $value = ($Iso2 ?? "").Trim().ToUpperInvariant()
  if ($value -eq "GB") { return "UK" }
  return $value
}

function New-HardLinkOrCopy {
  param(
    [string]$SourcePath,
    [string]$TargetPath
  )
  if (-not (Test-Path $SourcePath)) { return }
  if (Test-Path $TargetPath) { return }
  $parent = Split-Path $TargetPath -Parent
  if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
  try {
    New-Item -ItemType HardLink -Path $TargetPath -Target $SourcePath -ErrorAction Stop | Out-Null
  } catch {
    Copy-Item -Path $SourcePath -Destination $TargetPath -Force
  }
}

function Mirror-DirectoryWithHardLinks {
  param(
    [string]$SourceDir,
    [string]$TargetDir
  )
  if (-not (Test-Path $SourceDir)) { return }
  New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
  Get-ChildItem -Path $SourceDir -Recurse -File | ForEach-Object {
    $relative = $_.FullName.Substring($SourceDir.Length).TrimStart('\', '/')
    $targetPath = Join-Path $TargetDir $relative
    New-HardLinkOrCopy -SourcePath $_.FullName -TargetPath $targetPath
  }
}

function Canonical-MapPackVersion {
  param([string]$PackRoot)
  $mapRoot = Join-Path $PackRoot "map"
  $hasWorld = Test-Path (Join-Path $mapRoot "world_context.geojson")
  if (-not $hasWorld) { return $null }
  if ((Test-Path (Join-Path $mapRoot "basemap.mbtiles")) -or (Test-Path (Join-Path $mapRoot "gb_basemap.mbtiles"))) {
    return "vector-mbtiles-v1"
  }
  if ((Test-Path (Join-Path $mapRoot "county_basemap_mid")) -or (Test-Path (Join-Path $mapRoot "county_basemap_full"))) {
    return "geojson-basemap-v2"
  }
  if (Test-Path (Join-Path $mapRoot "county_roads")) {
    return "geojson-roads-v1"
  }
  return $null
}

$sourceIso2 = ($SourceIso2 ?? "").Trim().ToUpperInvariant()
$canonicalIso2 = Resolve-Iso2 -Iso2 $CanonicalIso2
if ($canonicalIso2.Length -ne 2) {
  throw "CanonicalIso2 must be a two-letter ISO code"
}
if ($sourceIso2.Length -ne 2) {
  throw "SourceIso2 must be a two-letter ISO code"
}

$sourceRoot = Join-Path $CountryPacksRoot $sourceIso2
$targetRoot = Join-Path $CountryPacksRoot $canonicalIso2
if (-not (Test-Path $sourceRoot)) {
  throw "Source pack does not exist: $sourceRoot"
}
if ((Test-Path $targetRoot) -and -not $Force) {
  throw "Target pack already exists: $targetRoot (use -Force to recreate)"
}
if ((Test-Path $targetRoot) -and $Force) {
  Remove-Item -Recurse -Force $targetRoot
}

New-Item -ItemType Directory -Force -Path $targetRoot | Out-Null

# Mirror large pack payload with hard links where possible.
Mirror-DirectoryWithHardLinks -SourceDir (Join-Path $sourceRoot "map") -TargetDir (Join-Path $targetRoot "map")
Mirror-DirectoryWithHardLinks -SourceDir (Join-Path $sourceRoot "region_cells") -TargetDir (Join-Path $targetRoot "region_cells")
Mirror-DirectoryWithHardLinks -SourceDir (Join-Path $sourceRoot "region_macro") -TargetDir (Join-Path $targetRoot "region_macro")
New-HardLinkOrCopy -SourcePath (Join-Path $sourceRoot "regions.geojson") -TargetPath (Join-Path $targetRoot "regions.geojson")

# Canonical surface file naming.
$targetSurfaces = Join-Path $targetRoot "surfaces"
New-Item -ItemType Directory -Force -Path $targetSurfaces | Out-Null
$sourceSurface = Join-Path (Join-Path $sourceRoot "surfaces") "$sourceIso2.surface.json"
$targetSurface = Join-Path $targetSurfaces "$canonicalIso2.surface.json"
if (-not (Test-Path $sourceSurface)) {
  $sourceSurface = Get-ChildItem -Path (Join-Path $sourceRoot "surfaces") -Filter "*.surface.json" -File | Select-Object -First 1 | ForEach-Object { $_.FullName }
}
if (-not $sourceSurface) {
  throw "No surface file found in source pack: $sourceRoot/surfaces"
}
New-HardLinkOrCopy -SourcePath $sourceSurface -TargetPath $targetSurface

# Canonical map filenames (legacy gb_* retained as compatibility).
$targetMapRoot = Join-Path $targetRoot "map"
New-HardLinkOrCopy -SourcePath (Join-Path $targetMapRoot "gb_basemap.mbtiles") -TargetPath (Join-Path $targetMapRoot "basemap.mbtiles")
New-HardLinkOrCopy -SourcePath (Join-Path $targetMapRoot "gb_major_roads.geojson") -TargetPath (Join-Path $targetMapRoot "major_roads.geojson")

$sourceManifestPath = Join-Path $sourceRoot "manifest.json"
$manifest = @{}
if (Test-Path $sourceManifestPath) {
  $manifest = Get-Content $sourceManifestPath -Raw | ConvertFrom-Json -AsHashtable
}

$surface = Get-Content $targetSurface -Raw | ConvertFrom-Json -AsHashtable
$regionCount = @($surface.cells_res6).Count
$cellsRes8 = @($surface.cells_res8).Count

$manifest["schema_version"] = [Math]::Max([int]($manifest["schema_version"] ?? 0), 2)
$manifest["country_iso2"] = $canonicalIso2
$manifest["surface_file"] = "surfaces/$canonicalIso2.surface.json"
$manifest["regions_file"] = "regions.geojson"
$manifest["region_provider_model"] = "planning_surface_res6_v1"
$manifest["compatibility_country_aliases"] = @("GB")
$manifest["region_count"] = $regionCount
$manifest["cells_res8"] = $cellsRes8
if (Test-Path (Join-Path $targetMapRoot "world_context.geojson")) {
  $manifest["world_context_file"] = "map/world_context.geojson"
}
if (Test-Path (Join-Path $targetMapRoot "major_roads.geojson")) {
  $manifest["major_roads_file"] = "map/major_roads.geojson"
} elseif (Test-Path (Join-Path $targetMapRoot "gb_major_roads.geojson")) {
  $manifest["major_roads_file"] = "map/gb_major_roads.geojson"
}
$mapPackVersion = Canonical-MapPackVersion -PackRoot $targetRoot
if ($mapPackVersion) {
  $manifest["map_pack_version"] = $mapPackVersion
}

$manifestPath = Join-Path $targetRoot "manifest.json"
$json = $manifest | ConvertTo-Json -Depth 24
Set-Content -Path $manifestPath -Value $json -Encoding UTF8

Write-Host "Canonical UK pack materialized:"
Write-Host "  source:  $sourceRoot"
Write-Host "  target:  $targetRoot"
Write-Host "  surface: $targetSurface"
Write-Host "  manifest: $manifestPath"
