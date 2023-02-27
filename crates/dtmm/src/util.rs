use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use clap::{parser::ValueSource, ArgMatches};
use color_eyre::{eyre::Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    data_dir: Option<PathBuf>,
    game_dir: Option<PathBuf>,
}

impl Config {
    pub fn game_dir(&self) -> Option<&PathBuf> {
        self.game_dir.as_ref()
    }

    pub fn data_dir(&self) -> Option<&PathBuf> {
        self.data_dir.as_ref()
    }
}

#[cfg(not(arget_os = "windows"))]
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

#[cfg(not(arget_os = "windows"))]
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
    let data_dir = std::env::var("APPDATA").expect("appdata env var not set");
    PathBuf::from(data_dir).join("dtmm")
}

pub(crate) fn read_config<P: AsRef<Path>>(
    default_config_path: P,
    matches: &ArgMatches,
) -> Result<Config> {
    let path = matches
        .get_one::<PathBuf>("config")
        .expect("argument missing despite default");
    let default_config_path = default_config_path.as_ref();

    match fs::read(path) {
        Ok(data) => {
            let data = String::from_utf8(data).wrap_err_with(|| {
                format!("config file {} contains invalid UTF-8", path.display())
            })?;
            serde_sjson::from_str(&data)
                .wrap_err_with(|| format!("invalid config file {}", path.display()))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            if matches.value_source("config") != Some(ValueSource::DefaultValue) {
                return Err(err)
                    .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;
            }

            {
                let parent = default_config_path
                    .parent()
                    .expect("a file path always has a parent directory");
                fs::create_dir_all(parent).wrap_err_with(|| {
                    format!("failed to create directories {}", parent.display())
                })?;
            }

            let config = Config {
                data_dir: Some(get_default_data_dir()),
                game_dir: None,
            };

            {
                let data = serde_sjson::to_string(&config)
                    .wrap_err("failed to serialize default config value")?;
                fs::write(default_config_path, data).wrap_err_with(|| {
                    format!(
                        "failed to write default config to {}",
                        default_config_path.display()
                    )
                })?;
            }

            Ok(config)
        }
        Err(err) => {
            Err(err).wrap_err_with(|| format!("failed to read config file {}", path.display()))
        }
    }
}
