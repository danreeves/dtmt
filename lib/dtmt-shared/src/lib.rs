use std::path::PathBuf;

use color_eyre::eyre;
use color_eyre::Result;

mod log;

pub use log::*;
use steamlocate::SteamDir;
use time::OffsetDateTime;

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

pub const STEAMAPP_ID: u32 = 1361210;

pub struct GameInfo {
    pub path: PathBuf,
    pub last_updated: OffsetDateTime,
}

pub fn collect_game_info() -> Result<GameInfo> {
    let mut dir = if let Some(dir) = SteamDir::locate() {
        dir
    } else {
        eyre::bail!("Failed to locate Steam installation")
    };

    let found = dir
        .app(&STEAMAPP_ID)
        .and_then(|app| app.vdf.get("LastUpdated").map(|v| (app.path.clone(), v)));

    let Some((path, last_updated)) = found else {
        eyre::bail!("Failed to find game installation");
    };

    let Some(last_updated) = last_updated
        .as_value()
        .and_then(|v| v.to::<i64>())
        .and_then(|v| OffsetDateTime::from_unix_timestamp(v).ok()) else {
            eyre::bail!("Couldn't read 'LastUpdate'.");
    };

    Ok(GameInfo { path, last_updated })
}
