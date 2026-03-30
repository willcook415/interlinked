param(
  [string[]]$Countries = @("GB"),
  [string]$CountryBoundariesGeoJson = "data/boundaries/countries.geojson",
  [string]$PopulationRaster = "data/raw/population/worldpop.tif",
  [string]$BuiltRaster = "data/raw/built/ghsl_built.tif",
  [string]$OsmDir = "data/osm",
  [string]$OutDir = "data/demand_surfaces",
  [string]$WorkDir = "data/demand_surfaces/_build",
  [double]$SampleDeg = 0.05,
  [int]$H3Res = 8,
  [string]$TargetCrs = "epsg3857",
  [int]$BuildTimeoutSec = 7200,
  [switch]$ForceRebuild,
  [switch]$RasterOnly = $true
)

$ErrorActionPreference = "Stop"

function Write-Log {
  param([string]$Message)
  $stamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
  $line = "[$stamp] $Message"
  Write-Host $line
  Add-Content -Path (Join-Path $WorkDir "build_country_surfaces.log") -Value $line
}

function Get-Iso3ForIso2 {
  param([string]$Iso2)
  $map = @{
    "GB" = "GBR"
    "LU" = "LUX"
    "ES" = "SPA"
    "PT" = "POR"
    "IT" = "ITA"
    "BE" = "BEL"
    "IE" = "IRL"
    "NL" = "NLD"
    "FR" = "FRA"
    "DE" = "DEU"
  }
  if ($map.ContainsKey($Iso2)) {
    return $map[$Iso2]
  }
  return $Iso2
}

function Get-CountryBbox {
  param(
    [string]$GeoJsonPath,
    [string]$Iso2
  )
  $py = @'
import json, sys
path, iso = sys.argv[1], sys.argv[2].upper()
with open(path, "r", encoding="utf-8") as f:
    gj = json.load(f)
features = gj.get("features", [])
target = None
for ft in features:
    props = ft.get("properties", {}) or {}
    cand = None
    for k in ("ISO_A2", "iso_a2", "ISO2", "iso2", "CNTR_ID", "country_iso2"):
        v = props.get(k)
        if isinstance(v, str) and len(v.strip()) == 2:
            cand = v.strip().upper()
            break
    if cand == iso:
        target = ft
        break
if target is None:
    raise SystemExit(f"country {iso} not found in boundaries")
geom = target.get("geometry", {}) or {}
coords = geom.get("coordinates", [])
def walk(node):
    if isinstance(node, (list, tuple)):
        if len(node) == 2 and isinstance(node[0], (int, float)) and isinstance(node[1], (int, float)):
            yield float(node[0]), float(node[1])
        else:
            for n in node:
                yield from walk(n)
pts = list(walk(coords))
if not pts:
    raise SystemExit(f"country {iso} geometry has no coordinates")
xs = [p[0] for p in pts]
ys = [p[1] for p in pts]
print(f"{min(xs)},{min(ys)},{max(xs)},{max(ys)}")
'@
  $bboxRaw = & python -c $py $GeoJsonPath $Iso2
  if ($LASTEXITCODE -ne 0) {
    throw "failed to compute bbox for $Iso2"
  }
  $parts = $bboxRaw.Trim().Split(",")
  if ($parts.Length -ne 4) {
    throw "invalid bbox result for $Iso2: $bboxRaw"
  }
  return @([double]$parts[0], [double]$parts[1], [double]$parts[2], [double]$parts[3])
}

function Convert-XyzToCsv {
  param(
    [string]$XyzPath,
    [string]$CsvPath
  )
  $reader = [System.IO.File]::OpenText($XyzPath)
  try {
    $writer = New-Object System.IO.StreamWriter($CsvPath, $false, [System.Text.Encoding]::UTF8)
    try {
      $writer.WriteLine("lon,lat,value")
      while (-not $reader.EndOfStream) {
        $line = $reader.ReadLine()
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $tokens = $line.Trim() -split "\s+"
        if ($tokens.Length -lt 3) { continue }
        $lon = $tokens[0]
        $lat = $tokens[1]
        $val = $tokens[2]
        if ($val -eq "nan" -or $val -eq "NaN" -or $val -eq "-nan") { continue }
        $writer.WriteLine("$lon,$lat,$val")
      }
    } finally {
      $writer.Dispose()
    }
  } finally {
    $reader.Dispose()
  }
}

function Convert-RasterToXyz {
  param(
    [string]$RasterPath,
    [string]$XyzPath
  )
  if (Get-Command gdal_translate -ErrorAction SilentlyContinue) {
    & gdal_translate -of XYZ $RasterPath $XyzPath
    if ($LASTEXITCODE -ne 0) { throw "gdal_translate failed for $RasterPath" }
    return
  }
  if (Get-Command gdal2xyz.py -ErrorAction SilentlyContinue) {
    & gdal2xyz.py $RasterPath $XyzPath
    if ($LASTEXITCODE -ne 0) { throw "gdal2xyz.py failed for $RasterPath" }
    return
  }
  throw "Neither gdal_translate nor gdal2xyz.py found in PATH"
}

function Invoke-BuildCountry {
  param(
    [string]$Iso2,
    [string]$PbfPath,
    [string]$PopCsv,
    [string]$BuiltCsv,
    [string]$OutSurfacePath,
    [double[]]$Bbox
  )
  $args = @(
    "run", "-p", "interlinked-osm", "--", "build-demand-surface",
    "--pbf", $PbfPath,
    "--country-iso2", $Iso2,
    "--country-boundaries-geojson", $CountryBoundariesGeoJson,
    "--population-raster-csv", $PopCsv,
    "--built-raster-csv", $BuiltCsv,
    "--out", $OutSurfacePath,
    "--h3-res", "$H3Res",
    "--target-crs", $TargetCrs,
    "--bbox", "$($Bbox[0])", "$($Bbox[1])", "$($Bbox[2])", "$($Bbox[3])"
  )
  if ($RasterOnly) {
    $args += "--raster-only"
  }
  $process = Start-Process -FilePath "cargo" -ArgumentList $args -PassThru -NoNewWindow
  $ok = $process.WaitForExit($BuildTimeoutSec * 1000)
  if (-not $ok) {
    try { $process.Kill() } catch {}
    throw "timeout building $Iso2 after $BuildTimeoutSec seconds"
  }
  if ($process.ExitCode -ne 0) {
    throw "cargo build-demand-surface failed for $Iso2 (exit $($process.ExitCode))"
  }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

if (-not (Test-Path $CountryBoundariesGeoJson)) { throw "missing boundaries file: $CountryBoundariesGeoJson" }
if (-not (Test-Path $PopulationRaster)) { throw "missing population raster: $PopulationRaster" }
if (-not (Test-Path $BuiltRaster)) { throw "missing built raster: $BuiltRaster" }

Write-Log "Starting country surface build batch"
Write-Log "Countries: $($Countries -join ', ') | sample_deg=$SampleDeg | h3_res=$H3Res"

foreach ($code in $Countries) {
  $iso2 = $code.Trim().ToUpperInvariant()
  if ($iso2.Length -ne 2) {
    Write-Log "SKIP $code (invalid ISO2)"
    continue
  }
  $start = Get-Date
  try {
    $iso3 = Get-Iso3ForIso2 -Iso2 $iso2
    $pbfCandidates = @(
      (Join-Path $OsmDir "$iso3.osm.pbf"),
      (Join-Path $OsmDir "$iso2.osm.pbf")
    )
    $pbfPath = $pbfCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $pbfPath) {
      throw "PBF not found for $iso2 (checked $($pbfCandidates -join '; '))"
    }

    $surfaceOut = Join-Path $OutDir "$iso2.surface.json"
    if ((Test-Path $surfaceOut) -and -not $ForceRebuild) {
      Write-Log "SKIP $iso2 (already built: $surfaceOut)"
      continue
    }

    $bbox = Get-CountryBbox -GeoJsonPath $CountryBoundariesGeoJson -Iso2 $iso2
    Write-Log "[$iso2] bbox=$($bbox -join ',')"

    $popTif = Join-Path $WorkDir "$iso2.population.tif"
    $builtTif = Join-Path $WorkDir "$iso2.built.tif"
    $popXyz = Join-Path $WorkDir "$iso2.population.xyz"
    $builtXyz = Join-Path $WorkDir "$iso2.built.xyz"
    $popCsv = Join-Path $WorkDir "$iso2.population.csv"
    $builtCsv = Join-Path $WorkDir "$iso2.built.csv"

    & gdalwarp -overwrite -te $bbox[0] $bbox[1] $bbox[2] $bbox[3] -tr $SampleDeg $SampleDeg -r bilinear $PopulationRaster $popTif
    if ($LASTEXITCODE -ne 0) { throw "gdalwarp failed for population ($iso2)" }
    & gdalwarp -overwrite -te $bbox[0] $bbox[1] $bbox[2] $bbox[3] -tr $SampleDeg $SampleDeg -r bilinear $BuiltRaster $builtTif
    if ($LASTEXITCODE -ne 0) { throw "gdalwarp failed for built raster ($iso2)" }

    Convert-RasterToXyz -RasterPath $popTif -XyzPath $popXyz
    Convert-RasterToXyz -RasterPath $builtTif -XyzPath $builtXyz

    Convert-XyzToCsv -XyzPath $popXyz -CsvPath $popCsv
    Convert-XyzToCsv -XyzPath $builtXyz -CsvPath $builtCsv

    Invoke-BuildCountry `
      -Iso2 $iso2 `
      -PbfPath $pbfPath `
      -PopCsv $popCsv `
      -BuiltCsv $builtCsv `
      -OutSurfacePath $surfaceOut `
      -Bbox $bbox

    $sizeKb = [math]::Round(((Get-Item $surfaceOut).Length / 1kb), 1)
    $elapsed = [math]::Round(((Get-Date) - $start).TotalMinutes, 2)
    Write-Log "PASS $iso2 -> $surfaceOut ($sizeKb KB, $elapsed min)"
  } catch {
    $elapsed = [math]::Round(((Get-Date) - $start).TotalMinutes, 2)
    Write-Log "FAIL $iso2 after $elapsed min: $($_.Exception.Message)"
  }
}

Write-Log "Country surface build batch complete"
