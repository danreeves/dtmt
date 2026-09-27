use std::collections::HashMap;
use std::path::PathBuf;

use color_eyre::Result;
use color_eyre::eyre::{OptionExt as _, WrapErr as _};
use serde::{Deserialize, Serialize};
use steamlocate::SteamDir;
use time::OffsetDateTime;

pub use log::*;

mod log;

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
    #[serde(default)]
    pub name_overrides: HashMap<String, String>,
    /// Path to the DXC compiler library (`dxcompiler.dll` / `libdxcompiler.so`)
    /// used to compile custom shader sources, or the directory holding it. The
    /// validator (`dxil.dll` / `libdxil.so`) must sit beside it; without it the
    /// compiled DXIL is unsigned and D3D12 refuses it. Defaults to
    /// `DTMT_DXC_DLL`, then the tool's directory, then the platform's usual
    /// install (the newest Windows SDK, or `/opt/dxc/lib` and friends on
    /// Linux). Under Proton the Windows build runs: put both DLLs beside
    /// `dtmt.exe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dxc: Option<PathBuf>,
}

pub const STEAMAPP_ID: u32 = 1361210;

#[derive(Debug)]
pub struct GameInfo {
    pub path: PathBuf,
    pub last_updated: OffsetDateTime,
}

pub fn collect_game_info() -> Result<Option<GameInfo>> {
    let dir = SteamDir::locate().wrap_err("Failed to locate Steam installation")?;

    let found = dir
        .find_app(STEAMAPP_ID)
        .wrap_err("Failed to look up game by Steam app ID")?;

    let Some((app, library)) = found else {
        return Ok(None);
    };

    let last_updated = app
        .last_updated
        .ok_or_eyre("Missing field 'last_updated'")?;

    Ok(Some(GameInfo {
        path: library.path().join(app.install_dir),
        last_updated: last_updated.into(),
    }))
}
