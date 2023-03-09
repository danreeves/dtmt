use std::path::PathBuf;

mod log;

pub use log::*;
use serde::Deserialize;
use steamlocate::SteamDir;
use time::OffsetDateTime;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModConfigResources {
    pub init: PathBuf,
    #[serde(default)]
    pub data: Option<PathBuf>,
    #[serde(default)]
    pub localization: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModOrder {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ModDependency {
    ID(String),
    Config { id: String, order: ModOrder },
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModConfig {
    #[serde(skip)]
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: String,
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub packages: Vec<PathBuf>,
    pub resources: ModConfigResources,
    #[serde(default)]
    pub depends: Vec<ModDependency>,
}

pub const STEAMAPP_ID: u32 = 1361210;

#[derive(Debug)]
pub struct GameInfo {
    pub path: PathBuf,
    pub last_updated: OffsetDateTime,
}

pub fn collect_game_info() -> Option<GameInfo> {
    let mut dir = if let Some(dir) = SteamDir::locate() {
        dir
    } else {
        tracing::debug!("Failed to locate Steam installation");
        return None;
    };

    let found = dir
        .app(&STEAMAPP_ID)
        .and_then(|app| app.vdf.get("LastUpdated").map(|v| (app.path.clone(), v)));

    let Some((path, last_updated)) = found else {
        tracing::debug!("Found Steam, but failed to find game installation");
        return None;
    };

    let Some(last_updated) = last_updated
        .as_value()
        .and_then(|v| v.to::<i64>())
        .and_then(|v| OffsetDateTime::from_unix_timestamp(v).ok()) else {
            tracing::error!("Found Steam game, but couldn't read 'LastUpdate'.");
            return None;
    };

    Some(GameInfo { path, last_updated })
}
