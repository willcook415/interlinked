use serde::{Deserialize, Serialize};

/// Coordinate reference system (CRS) for `World` coordinates.
///
/// - `epsg3857` matches web map tiles (OSM/MapLibre/Leaflet) directly.
/// - `local` keeps your synthetic/city-local test scenarios but anchors them to a real lon/lat.
/// - `wgs84` is included for completeness (lat/lon storage), but sim/core still works best in meters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Crs {
    /// Local planar coordinates (x/y) in meters, anchored to a real-world lon/lat.
    ///
    /// Interpretation:
    /// - `x` increases east (meters)
    /// - `y` increases north (meters)
    /// - `origin_lon/lat` defines where (0,0) sits on the globe
    Local { origin_lon: f64, origin_lat: f64 },

    /// Web Mercator meters (EPSG:3857). Native for most web maps / OSM tiles.
    Epsg3857,

    /// Latitude/longitude degrees (EPSG:4326). Prefer converting to meters for sim.
    Wgs84,
}

impl Default for Crs {
    fn default() -> Self {
        // Backwards compatible: old scenarios had no CRS and used synthetic local meters.
        // Anchor at (0,0) unless the scenario provides a real origin.
        Crs::Local {
            origin_lon: 0.0,
            origin_lat: 0.0,
        }
    }
}

const EARTH_RADIUS_M: f64 = 6_378_137.0;

/// Convert lon/lat degrees -> Web Mercator meters (EPSG:3857).
pub fn lonlat_to_web_mercator_m(lon_deg: f64, lat_deg: f64) -> (f64, f64) {
    let lon_rad = lon_deg.to_radians();
    let lat_rad = lat_deg.to_radians();

    let x = EARTH_RADIUS_M * lon_rad;
    let y = EARTH_RADIUS_M * (0.5 * (std::f64::consts::FRAC_PI_2 + lat_rad)).tan().ln();
    (x, y)
}

/// Convert Web Mercator meters (EPSG:3857) -> lon/lat degrees.
pub fn web_mercator_m_to_lonlat(x_m: f64, y_m: f64) -> (f64, f64) {
    let lon_rad = x_m / EARTH_RADIUS_M;
    let lat_rad = (2.0 * (y_m / EARTH_RADIUS_M).exp().atan()) - std::f64::consts::FRAC_PI_2;
    (lon_rad.to_degrees(), lat_rad.to_degrees())
}

/// Convert a world (x,y) into Web Mercator meters based on the scenario CRS.
/// This is the single function your UI should rely on for placing stops/links on the map.
pub fn world_xy_to_web_mercator_m(crs: &Crs, x: f64, y: f64) -> (f64, f64) {
    match crs {
        Crs::Epsg3857 => (x, y),

        Crs::Wgs84 => {
            // In WGS84 storage, interpret x=lon, y=lat.
            lonlat_to_web_mercator_m(x, y)
        }

        Crs::Local {
            origin_lon,
            origin_lat,
        } => {
            // Anchor local meters to Mercator meters at the origin.
            let (ox, oy) = lonlat_to_web_mercator_m(*origin_lon, *origin_lat);
            (ox + x, oy + y)
        }
    }
}

/// Convert Web Mercator meters back into scenario world coordinates for a given CRS.
pub fn web_mercator_m_to_world_xy(crs: &Crs, x_m: f64, y_m: f64) -> (f64, f64) {
    match crs {
        Crs::Epsg3857 => (x_m, y_m),
        Crs::Wgs84 => web_mercator_m_to_lonlat(x_m, y_m),
        Crs::Local {
            origin_lon,
            origin_lat,
        } => {
            let (ox, oy) = lonlat_to_web_mercator_m(*origin_lon, *origin_lat);
            (x_m - ox, y_m - oy)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) {
        assert!(
            (a - b).abs() <= eps,
            "expected approx equality: {a} vs {b} (eps {eps})"
        );
    }

    #[test]
    fn mercator_roundtrip_lonlat() {
        let (lon, lat) = (-1.5491, 53.8008); // Leeds-ish
        let (x, y) = lonlat_to_web_mercator_m(lon, lat);
        let (lon2, lat2) = web_mercator_m_to_lonlat(x, y);

        approx(lon, lon2, 1e-8);
        approx(lat, lat2, 1e-8);
    }

    #[test]
    fn world_xy_to_mercator_epsg3857_passthrough() {
        let crs = Crs::Epsg3857;
        let (x, y) = world_xy_to_web_mercator_m(&crs, 123.0, 456.0);
        assert_eq!(x, 123.0);
        assert_eq!(y, 456.0);
    }

    #[test]
    fn world_xy_to_mercator_wgs84_interprets_xy_as_lonlat() {
        let crs = Crs::Wgs84;
        let lon = -1.5491;
        let lat = 53.8008;
        let (x, y) = world_xy_to_web_mercator_m(&crs, lon, lat);
        let (lon2, lat2) = web_mercator_m_to_lonlat(x, y);

        approx(lon, lon2, 1e-8);
        approx(lat, lat2, 1e-8);
    }

    #[test]
    fn local_anchor_adds_offsets_in_meters() {
        let crs = Crs::Local {
            origin_lon: -1.5491,
            origin_lat: 53.8008,
        };

        let (ox, oy) = world_xy_to_web_mercator_m(&crs, 0.0, 0.0);
        let (x, y) = world_xy_to_web_mercator_m(&crs, 1000.0, 2000.0);

        approx(x, ox + 1000.0, 1e-6);
        approx(y, oy + 2000.0, 1e-6);
    }

    #[test]
    fn default_crs_is_local() {
        let crs = Crs::default();
        match crs {
            Crs::Local { .. } => {}
            _ => panic!("default CRS should be Local for backwards compatibility"),
        }
    }

    #[test]
    fn mercator_world_roundtrip_for_local() {
        let crs = Crs::Local {
            origin_lon: -1.5491,
            origin_lat: 53.8008,
        };

        let (wx, wy) = (1234.0, -456.0);
        let (mx, my) = world_xy_to_web_mercator_m(&crs, wx, wy);
        let (wx2, wy2) = web_mercator_m_to_world_xy(&crs, mx, my);

        approx(wx, wx2, 1e-6);
        approx(wy, wy2, 1e-6);
    }
}
