export type GeoFeature = {
  type: "Feature";
  geometry: { type: "Point" | "LineString" | "Polygon" | "MultiPolygon"; coordinates: unknown };
  properties: Record<string, string | number | boolean | null>;
};

export type GeoCollection = { type: "FeatureCollection"; features: GeoFeature[] };

export function fc(features: GeoFeature[] = []): GeoCollection {
  return { type: "FeatureCollection", features };
}
