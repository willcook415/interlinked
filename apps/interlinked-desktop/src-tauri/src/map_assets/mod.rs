use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub(crate) struct MapAssetServer {
    pub(crate) base_url: String,
}

pub(crate) static MAP_ASSET_SERVER: OnceLock<MapAssetServer> = OnceLock::new();

pub mod server;
