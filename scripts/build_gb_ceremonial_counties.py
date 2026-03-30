import json
import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
PYDEPS = REPO_ROOT / ".tmp_pydeps"
if PYDEPS.exists():
    sys.path.insert(0, str(PYDEPS))

import geopandas as gpd  # noqa: E402
from shapely.geometry import mapping  # noqa: E402


SOURCE_GPKG = REPO_ROOT / "data" / "bdline_gpkg_gb" / "Data" / "bdline_gb.gpkg"
SOURCE_LAYER = "boundary_line_ceremonial_counties"
LEGACY_INDEX = REPO_ROOT / "data" / "boundaries" / "uk_counties_index.json"
OUT_GEOJSON = REPO_ROOT / "data" / "boundaries" / "gb_ceremonial_counties_canonical.geojson"
OUT_INDEX = REPO_ROOT / "data" / "boundaries" / "gb_ceremonial_counties_index.json"
OUT_ALIASES = REPO_ROOT / "data" / "boundaries" / "gb_ceremonial_county_aliases.json"


DISPLAY_OVERRIDES = {
    "City and County of the City of London": "City of London",
    "City of Aberdeen": "Aberdeen",
    "City of Dundee": "Dundee",
    "City of Edinburgh": "Edinburgh",
    "City of Glasgow": "Glasgow",
    "Tyne & Wear": "Tyne and Wear",
}

WALES = {
    "Clwyd",
    "Dyfed",
    "Gwent",
    "Gwynedd",
    "Mid Glamorgan",
    "Powys",
    "South Glamorgan",
    "West Glamorgan",
}

SCOTLAND = {
    "Aberdeen",
    "Aberdeenshire",
    "Angus",
    "Argyll and Bute",
    "Ayrshire and Arran",
    "Banffshire",
    "Berwickshire",
    "Caithness",
    "Clackmannan",
    "Dumfries",
    "Dunbartonshire",
    "Dundee",
    "East Lothian",
    "Edinburgh",
    "Fife",
    "Glasgow",
    "Inverness",
    "Kincardineshire",
    "Lanarkshire",
    "Midlothian",
    "Moray",
    "Nairn",
    "Orkney",
    "Perth and Kinross",
    "Renfrewshire",
    "Ross and Cromarty",
    "Roxburgh, Ettrick and Lauderdale",
    "Shetland",
    "Stirling and Falkirk",
    "Sutherland",
    "The Stewartry of Kirkcudbright",
    "Tweeddale",
    "West Lothian",
    "Western Isles",
    "Wigtown",
}

MANUAL_LEGACY_ALIASES = {
    "county:GB:clackmannanshire": "county:GB:clackmannan",
    "county:GB:peeblesshire": "county:GB:tweeddale",
    "county:GB:selkirkshire": "county:GB:roxburgh-ettrick-and-lauderdale",
}


def slugify(value: str) -> str:
    value = value.strip().lower()
    value = value.replace("&", " and ")
    value = re.sub(r"[^a-z0-9]+", "-", value)
    return value.strip("-")


def detect_nation(name: str) -> str:
    if name in WALES:
        return "Wales"
    if name in SCOTLAND:
        return "Scotland"
    return "England"


def source_code_for(nation: str) -> str:
    if nation == "Wales":
        return "WLS_PRESERVED"
    if nation == "Scotland":
        return "SCT_CEREMONIAL"
    return "ENG_CEREMONIAL"


def load_legacy_index() -> dict[str, str]:
    raw = json.loads(LEGACY_INDEX.read_text(encoding="utf-8"))
    out: dict[str, str] = {}
    for row in raw.get("counties", []):
        if row.get("country_iso2") != "GB":
            continue
        county_id = row.get("county_id")
        name = row.get("name")
        if county_id and name:
            out[f"county:GB:{county_id}"] = name
    return out


def build_aliases(new_rows: list[dict[str, str]]) -> dict[str, str]:
    new_by_slug = {slugify(row["name"]): row["region_id"] for row in new_rows}
    aliases: dict[str, str] = {}
    for legacy_region_id, legacy_name in load_legacy_index().items():
        slug = slugify(legacy_name)
        mapped = new_by_slug.get(slug)
        if mapped:
            aliases[legacy_region_id] = mapped
    aliases.update(MANUAL_LEGACY_ALIASES)
    for row in new_rows:
        raw_slug = slugify(row["raw_name"])
        aliases.setdefault(f"county:GB:{raw_slug}", row["region_id"])
    return dict(sorted(aliases.items()))


def main() -> int:
    if not SOURCE_GPKG.exists():
        raise SystemExit(f"missing source geopackage: {SOURCE_GPKG}")

    gdf = gpd.read_file(SOURCE_GPKG, layer=SOURCE_LAYER)
    gdf = gdf.to_crs(epsg=4326)
    gdf["geometry"] = gdf.geometry.simplify(0.00005, preserve_topology=True)

    features = []
    index_rows = []
    seen_ids: set[str] = set()
    for _, row in gdf.sort_values("Name").iterrows():
        raw_name = str(row["Name"]).strip()
        display_name = DISPLAY_OVERRIDES.get(raw_name, raw_name)
        county_id = slugify(display_name)
        if county_id in seen_ids:
            raise SystemExit(f"duplicate county_id generated: {county_id} ({display_name})")
        seen_ids.add(county_id)
        nation = detect_nation(display_name)
        source_code = source_code_for(nation)
        region_id = f"county:GB:{county_id}"
        features.append(
            {
                "type": "Feature",
                "properties": {
                    "county_id": county_id,
                    "region_id": region_id,
                    "name": display_name,
                    "raw_name": raw_name,
                    "nation": nation,
                    "country_iso2": "GB",
                    "source_code": source_code,
                    "source_dataset": "os-boundary-line-ceremonial-counties",
                },
                "geometry": mapping(row.geometry),
            }
        )
        index_rows.append(
            {
                "county_id": county_id,
                "name": display_name,
                "nation": nation,
                "country_iso2": "GB",
                "source_code": source_code,
            }
        )

    if len(features) != 91:
        raise SystemExit(f"expected 91 ceremonial counties, got {len(features)}")

    aliases = build_aliases(
        [
            {
                "county_id": item["county_id"],
                "name": item["name"],
                "raw_name": item["raw_name"],
                "region_id": item["region_id"],
            }
            for item in (feature["properties"] for feature in features)
        ]
    )

    OUT_GEOJSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_GEOJSON.write_text(
        json.dumps(
            {"type": "FeatureCollection", "features": features},
            ensure_ascii=False,
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )
    OUT_INDEX.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "description": "Canonical OS Boundary-Line ceremonial county metadata for Great Britain gameplay regions.",
                "counties": index_rows,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    OUT_ALIASES.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "description": "Legacy GB county region-id aliases mapped to canonical OS ceremonial county ids.",
                "aliases": aliases,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"Wrote {len(features)} GB ceremonial counties -> {OUT_GEOJSON}")
    print(f"Wrote county index -> {OUT_INDEX}")
    print(f"Wrote {len(aliases)} county aliases -> {OUT_ALIASES}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
