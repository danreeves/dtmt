use std::path::PathBuf;

mod log;

pub use log::*;
use serde::{Deserialize, Serialize};
use steamlocate::SteamDir;
use time::OffsetDateTime;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModConfigResources {
    pub init: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub localization: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModOrder {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ModDependency {
    ID(String),
    Config { id: String, order: ModOrder },
}

// A bit dumb, but serde doesn't support literal values with the
// `default` attribute, only paths.
fn default_true() -> bool {
    true
}

// Similarly dumb, as the `skip_serializing_if` attribute needs a function
fn is_true(val: &bool) -> bool {
    *val
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ModConfig {
    #[serde(skip)]
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub packages: Vec<PathBuf>,
    pub resources: ModConfigResources,
    #[serde(default)]
    pub depends: Vec<ModDependency>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub bundled: bool,
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
        .and_then(|app| app.last_updated.map(|v| (app.path.clone(), v)));

    let Some((path, last_updated)) = found else {
        tracing::debug!("Found Steam, but failed to find game installation");
        return None;
    };

    Some(GameInfo {
        path,
        last_updated: last_updated.into(),
    })
}
