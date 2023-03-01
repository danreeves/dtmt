mod log;

use std::path::PathBuf;

pub use log::*;

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct ModConfigResources {
    pub init: PathBuf,
    #[serde(default)]
    pub data: Option<PathBuf>,
    #[serde(default)]
    pub localization: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct ModConfig {
    #[serde(skip)]
    pub dir: std::path::PathBuf,
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub packages: Vec<std::path::PathBuf>,
    pub resources: ModConfigResources,
    #[serde(default)]
    pub depends: Vec<String>,
}
