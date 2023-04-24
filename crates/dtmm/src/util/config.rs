use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::{eyre::Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::state::{ActionState, ModInfo};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LoadOrderEntrySerialize<'a> {
    pub id: &'a String,
    pub enabled: bool,
}

impl<'a> From<&'a ModInfo> for LoadOrderEntrySerialize<'a> {
    fn from(info: &'a ModInfo) -> Self {
        Self {
            id: &info.id,
            enabled: info.enabled,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigSerialize<'a> {
    game_dir: &'a Path,
    data_dir: &'a Path,
    nexus_api_key: &'a String,
    mod_order: Vec<LoadOrderEntrySerialize<'a>>,
    unsafe_io: bool,
}

impl<'a> From<&'a ActionState> for ConfigSerialize<'a> {
    fn from(state: &'a ActionState) -> Self {
        Self {
            game_dir: &state.game_dir,
            data_dir: &state.data_dir,
            nexus_api_key: &state.nexus_api_key,
            unsafe_io: state.is_io_enabled,
            mod_order: state
                .mods
                .iter()
                .map(Arc::as_ref)
                .map(LoadOrderEntrySerialize::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LoadOrderEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(default = "get_default_data_dir")]
    pub data_dir: PathBuf,
    pub game_dir: Option<PathBuf>,
    #[serde(default)]
    pub unsafe_io: bool,
    pub nexus_api_key: Option<String>,
    #[serde(default)]
    pub mod_order: Vec<LoadOrderEntry>,
}

#[cfg(not(target_os = "windows"))]
pub fn get_default_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| {
            let user = std::env::var("USER").expect("user env variable not set");
            format!("/home/{user}")
        });
        format!("{home}/.config")
    });

    PathBuf::from(config_dir).join("dtmm").join("dtmm.cfg")
}

#[cfg(target_os = "windows")]
pub fn get_default_config_path() -> PathBuf {
    let config_dir = std::env::var("APPDATA").expect("appdata env var not set");
    PathBuf::from(config_dir).join("dtmm").join("dtmm.cfg")
}

#[cfg(not(target_os = "windows"))]
pub fn get_default_data_dir() -> PathBuf {
    let data_dir = std::env::var("XDG_DATA_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| {
            let user = std::env::var("USER").expect("user env variable not set");
            format!("/home/{user}")
        });
        format!("{home}/.local/share")
    });

    PathBuf::from(data_dir).join("dtmm")
}

#[cfg(target_os = "windows")]
pub fn get_default_data_dir() -> PathBuf {
    let data_dir = std::env::var("LOCALAPPDATA").expect("appdata env var not set");
    PathBuf::from(data_dir).join("dtmm")
}

#[tracing::instrument]
pub(crate) async fn read_config<P>(path: P, is_default: bool) -> Result<Config>
where
    P: Into<PathBuf> + std::fmt::Debug,
{
    let path = path.into();
    let default_path = get_default_config_path();

    match fs::read(&path).await {
        Ok(data) => {
            let data = String::from_utf8(data).wrap_err_with(|| {
                format!("Config file '{}' contains invalid UTF-8", path.display())
            })?;
            let mut cfg: Config = serde_sjson::from_str(&data)
                .wrap_err_with(|| format!("Invalid config file {}", path.display()))?;

            cfg.path = path;
            Ok(cfg)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            if !is_default {
                return Err(err)
                    .wrap_err_with(|| format!("Failed to read config file {}", path.display()))?;
            }

            {
                let parent = default_path
                    .parent()
                    .expect("a file path always has a parent directory");
                fs::create_dir_all(parent).await.wrap_err_with(|| {
                    format!("Failed to create directories {}", parent.display())
                })?;
            }

            let config = Config {
                path: default_path,
                data_dir: get_default_data_dir(),
                game_dir: None,
                nexus_api_key: None,
                mod_order: Vec::new(),
                unsafe_io: false,
            };

            {
                let data = serde_sjson::to_string(&config)
                    .wrap_err("Failed to serialize default config value")?;
                fs::write(&config.path, data).await.wrap_err_with(|| {
                    format!(
                        "Failed to write default config to {}",
                        config.path.display()
                    )
                })?;
            }

            Ok(config)
        }
        Err(err) => {
            Err(err).wrap_err_with(|| format!("Failed to read config file {}", path.display()))
        }
    }
}
