use super::Crs;
use super::{Params, World};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub meta: Meta,
    pub params: Params,
    pub world: World,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    pub seed: u64,
    pub time_period_hours: f64,
    /// Coordinate reference system for `World` x/y coordinates.
    /// Backwards compatible: if missing in JSON, defaults to `local` anchored at (0,0).
    #[serde(default)]
    pub crs: Crs,
}
