use geo::algorithm::contains::Contains;
use geo::algorithm::simplify::Simplify;
use geo::{LineString, MultiPolygon, Point, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value};
use osmpbfreader::{
    Node, NodeId, OsmId, OsmObj, OsmPbfReader, Relation, RelationId, Tags, Way, WayId,
};
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexFile {
    counties: Vec<UkCountyIndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct UkCountyIndexEntry {
    county_id: String,
    name: String,
    nation: String,
    country_iso2: String,
    source_code: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn normalize_admin_name(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn county_relation_name_score(candidate: &str, wanted: &str) -> i32 {
    let candidate_norm = normalize_admin_name(candidate);
    let wanted_norm = normalize_admin_name(wanted);
    if candidate_norm.is_empty() || wanted_norm.is_empty() {
        return -1;
    }
    if candidate_norm == wanted_norm {
        return 500;
    }
    if candidate_norm.ends_with(&wanted_norm) || wanted_norm.ends_with(&candidate_norm) {
        return 360;
    }
    if candidate_norm.contains(&wanted_norm) || wanted_norm.contains(&candidate_norm) {
        return 240;
    }
    -1
}

fn relation_boundary_weight(tags: &Tags) -> i32 {
    let mut score = 0;
    if tags.get("boundary").map(|v| v.as_str()) == Some("administrative") {
        score += 80;
    }
    if matches!(
        tags.get("type").map(|v| v.as_str()),
        Some("boundary") | Some("multipolygon")
    ) {
        score += 20;
    }
    match tags.get("admin_level").map(|v| v.as_str()) {
        Some("6") => score += 35,
        Some("5") | Some("4") => score += 24,
        Some("7") | Some("8") => score += 12,
        _ => {}
    }
    score
}

fn relation_tag_name_candidates(tags: &Tags) -> Vec<String> {
    ["name", "official_name", "short_name", "alt_name"]
        .iter()
        .filter_map(|key| tags.get(*key).map(|value| value.to_string()))
        .flat_map(|value| {
            value
                .split(';')
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn point_eq(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

fn close_ring_coords(mut coords: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if coords.len() >= 2 && !point_eq(coords[0], *coords.last().unwrap_or(&(0.0, 0.0))) {
        coords.push(coords[0]);
    }
    coords
}

fn polygon_from_ring_pairs(coords: &[(f64, f64)]) -> Option<Polygon<f64>> {
    if coords.len() < 4 {
        return None;
    }
    Some(Polygon::new(LineString::from(coords.to_vec()), vec![]))
}

fn stitch_way_segments(mut segments: Vec<Vec<(f64, f64)>>) -> Vec<Vec<(f64, f64)>> {
    let mut rings = Vec::<Vec<(f64, f64)>>::new();
    while let Some(mut current) = segments.pop() {
        if current.len() < 2 {
            continue;
        }
        loop {
            let first = current[0];
            let last = *current.last().unwrap_or(&first);
            if point_eq(first, last) {
                let ring = close_ring_coords(current);
                if ring.len() >= 4 {
                    rings.push(ring);
                }
                break;
            }
            let mut matched_idx = None::<usize>;
            let mut prepend = false;
            let mut reverse = false;
            for (idx, segment) in segments.iter().enumerate() {
                let seg_first = segment[0];
                let seg_last = *segment.last().unwrap_or(&seg_first);
                if point_eq(last, seg_first) {
                    matched_idx = Some(idx);
                    break;
                }
                if point_eq(last, seg_last) {
                    matched_idx = Some(idx);
                    reverse = true;
                    break;
                }
                if point_eq(first, seg_last) {
                    matched_idx = Some(idx);
                    prepend = true;
                    break;
                }
                if point_eq(first, seg_first) {
                    matched_idx = Some(idx);
                    prepend = true;
                    reverse = true;
                    break;
                }
            }
            let Some(idx) = matched_idx else {
                let ring = close_ring_coords(current);
                if ring.len() >= 4 {
                    rings.push(ring);
                }
                break;
            };
            let mut segment = segments.swap_remove(idx);
            if reverse {
                segment.reverse();
            }
            if prepend {
                if !segment.is_empty() {
                    segment.pop();
                }
                segment.extend(current);
                current = segment;
            } else {
                if !segment.is_empty() {
                    segment.remove(0);
                }
                current.extend(segment);
            }
        }
    }
    rings
}

fn relation_to_multipolygon(
    relation: &Relation,
    ways: &HashMap<WayId, Way>,
    nodes: &HashMap<NodeId, Node>,
) -> Option<MultiPolygon<f64>> {
    let mut outer_segments = Vec::<Vec<(f64, f64)>>::new();
    let mut inner_segments = Vec::<Vec<(f64, f64)>>::new();
    for member in &relation.refs {
        let OsmId::Way(way_id) = member.member else {
            continue;
        };
        let way = ways.get(&way_id)?;
        let coords = way
            .nodes
            .iter()
            .filter_map(|node_id| nodes.get(node_id).map(|node| (node.lon(), node.lat())))
            .collect::<Vec<_>>();
        if coords.len() < 2 {
            continue;
        }
        match member.role.as_str() {
            "inner" => inner_segments.push(coords),
            _ => outer_segments.push(coords),
        }
    }
    let outer_rings = stitch_way_segments(outer_segments);
    if outer_rings.is_empty() {
        return None;
    }
    let inner_rings = stitch_way_segments(inner_segments);
    let mut holes_by_outer = vec![Vec::<LineString<f64>>::new(); outer_rings.len()];
    for inner in inner_rings {
        let Some(inner_polygon) = polygon_from_ring_pairs(&inner) else {
            continue;
        };
        let Some(inner_point) = inner_polygon.exterior().points().next() else {
            continue;
        };
        for (idx, outer) in outer_rings.iter().enumerate() {
            let Some(outer_polygon) = polygon_from_ring_pairs(outer) else {
                continue;
            };
            if outer_polygon.contains(&Point::new(inner_point.x(), inner_point.y())) {
                holes_by_outer[idx].push(inner_polygon.exterior().clone());
                break;
            }
        }
    }
    let polygons = outer_rings
        .iter()
        .enumerate()
        .filter_map(|(idx, outer)| {
            let outer = close_ring_coords(outer.clone());
            (outer.len() >= 4).then_some(Polygon::new(
                LineString::from(outer),
                holes_by_outer[idx].clone(),
            ))
        })
        .collect::<Vec<_>>();
    (!polygons.is_empty()).then_some(MultiPolygon(polygons))
}

fn simplify_county_geometry(geometry: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    geometry.simplify(&0.0018)
}

fn multipolygon_to_geometry(geometry: &MultiPolygon<f64>) -> Geometry {
    let coordinates = geometry
        .0
        .iter()
        .map(|polygon| {
            let mut rings = Vec::<Vec<Vec<f64>>>::new();
            rings.push(
                polygon
                    .exterior()
                    .points()
                    .map(|point| vec![point.x(), point.y()])
                    .collect(),
            );
            for interior in polygon.interiors() {
                rings.push(
                    interior
                        .points()
                        .map(|point| vec![point.x(), point.y()])
                        .collect(),
                );
            }
            rings
        })
        .collect::<Vec<_>>();
    Geometry::new(Value::MultiPolygon(coordinates))
}

fn main() -> Result<(), String> {
    let root = repo_root();
    let index_path = root
        .join("data")
        .join("boundaries")
        .join("uk_counties_index.json");
    let pbf_path = root.join("data").join("osm").join("GBR.osm.pbf");
    let out_path = root
        .join("data")
        .join("boundaries")
        .join("uk_counties_canonical.geojson");

    let raw =
        fs::read_to_string(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let index_file: UkCountyIndexFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let wanted = index_file
        .counties
        .iter()
        .map(|entry| (entry.county_id.clone(), entry.name.clone()))
        .collect::<Vec<_>>();

    let reader = File::open(&pbf_path).map_err(|e| format!("{}: {e}", pbf_path.display()))?;
    let mut pbf = OsmPbfReader::new(reader);
    let objects = pbf
        .get_objs_and_deps(|obj| {
            let Some(relation) = obj.relation() else {
                return false;
            };
            if relation.refs.is_empty() {
                return false;
            }
            let tag_names = relation_tag_name_candidates(&relation.tags);
            if tag_names.is_empty() {
                return false;
            }
            wanted.iter().any(|(_, name)| {
                tag_names
                    .iter()
                    .any(|candidate| county_relation_name_score(candidate, name) >= 0)
            })
        })
        .map_err(|e| e.to_string())?;

    let mut nodes = HashMap::<NodeId, Node>::new();
    let mut ways = HashMap::<WayId, Way>::new();
    let mut relations = Vec::<Relation>::new();
    for obj in objects.into_values() {
        match obj {
            OsmObj::Node(node) => {
                nodes.insert(node.id, node);
            }
            OsmObj::Way(way) => {
                ways.insert(way.id, way);
            }
            OsmObj::Relation(relation) => relations.push(relation),
        }
    }

    let mut best_relation_for_county = HashMap::<String, (i32, usize, RelationId)>::new();
    for relation in &relations {
        let tag_names = relation_tag_name_candidates(&relation.tags);
        if tag_names.is_empty() {
            continue;
        }
        let boundary_weight = relation_boundary_weight(&relation.tags);
        for (county_id, target_name) in &wanted {
            let mut best_name_score = -1;
            for candidate in &tag_names {
                best_name_score =
                    best_name_score.max(county_relation_name_score(candidate, target_name));
            }
            if best_name_score < 0 {
                continue;
            }
            let score = best_name_score + boundary_weight;
            let refs = relation.refs.len();
            let replace = match best_relation_for_county.get(county_id) {
                Some((current_score, current_refs, _)) => {
                    score > *current_score || (score == *current_score && refs > *current_refs)
                }
                None => true,
            };
            if replace {
                best_relation_for_county.insert(county_id.clone(), (score, refs, relation.id));
            }
        }
    }

    let relation_by_id = relations
        .into_iter()
        .map(|relation| (relation.id, relation))
        .collect::<HashMap<_, _>>();

    let mut features = Vec::<Feature>::new();
    let mut found = 0usize;
    for entry in &index_file.counties {
        let Some((_, _, relation_id)) = best_relation_for_county.get(&entry.county_id) else {
            eprintln!("missing relation for {}", entry.name);
            continue;
        };
        let Some(relation) = relation_by_id.get(relation_id) else {
            eprintln!("missing relation object for {}", entry.name);
            continue;
        };
        let Some(geometry) = relation_to_multipolygon(relation, &ways, &nodes) else {
            eprintln!("failed geometry for {}", entry.name);
            continue;
        };
        let geometry = simplify_county_geometry(&geometry);
        let mut props = JsonMap::<String, JsonValue>::new();
        props.insert(
            "county_id".to_string(),
            JsonValue::String(entry.county_id.clone()),
        );
        props.insert("name".to_string(), JsonValue::String(entry.name.clone()));
        props.insert(
            "nation".to_string(),
            JsonValue::String(entry.nation.clone()),
        );
        props.insert(
            "country_iso2".to_string(),
            JsonValue::String(entry.country_iso2.clone()),
        );
        props.insert(
            "source_code".to_string(),
            JsonValue::String(entry.source_code.clone()),
        );
        features.push(Feature {
            bbox: None,
            geometry: Some(multipolygon_to_geometry(&geometry)),
            id: None,
            properties: Some(props),
            foreign_members: None,
        });
        found += 1;
    }

    let fc = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    let out = GeoJson::FeatureCollection(fc).to_string();
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&out_path, out).map_err(|e| format!("{}: {e}", out_path.display()))?;
    println!("wrote {} county features to {}", found, out_path.display());
    Ok(())
}
