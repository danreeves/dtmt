#![recursion_limit = "256"]
#![feature(let_chains)]

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use clap::command;
use clap::parser::ValueSource;
use clap::value_parser;
use clap::Arg;
use color_eyre::eyre::Context;
use color_eyre::Report;
use color_eyre::Result;
use druid::AppLauncher;
use druid::ExtEventSink;
use druid::SingleUse;
use druid::Target;
use engine::delete_mod;
use engine::import_mod;
use serde::Deserialize;
use serde::Serialize;
use state::ACTION_FINISH_ADD_MOD;
use state::ACTION_FINISH_DELETE_SELECTED_MOD;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;

use crate::engine::deploy_mods;
use crate::state::{AsyncAction, Delegate, State, ACTION_FINISH_DEPLOY};

mod controller;
mod engine;
mod main_window;
mod state;
mod theme;
mod widget;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Config {
    data_dir: Option<PathBuf>,
    game_dir: Option<PathBuf>,
}

fn work_thread(
    event_sink: Arc<RwLock<ExtEventSink>>,
    action_queue: Arc<RwLock<UnboundedReceiver<AsyncAction>>>,
) -> Result<()> {
    let rt = Runtime::new()?;

    rt.block_on(async {
        while let Some(action) = action_queue.write().await.recv().await {
            let event_sink = event_sink.clone();
            match action {
                AsyncAction::DeployMods(state) => tokio::spawn(async move {
                    if let Err(err) = deploy_mods(state).await {
                        tracing::error!("Failed to deploy mods: {:?}", err);
                    }

                    event_sink
                        .write()
                        .await
                        .submit_command(ACTION_FINISH_DEPLOY, (), Target::Auto)
                        .expect("failed to send command");
                }),
                AsyncAction::AddMod((state, info)) => tokio::spawn(async move {
                    match import_mod(state, info).await {
                        Ok(mod_info) => {
                            event_sink
                                .write()
                                .await
                                .submit_command(
                                    ACTION_FINISH_ADD_MOD,
                                    SingleUse::new(mod_info),
                                    Target::Auto,
                                )
                                .expect("failed to send command");
                        }
                        Err(err) => {
                            tracing::error!("Failed to import mod: {:?}", err);
                        }
                    }
                }),
                AsyncAction::DeleteMod((state, info)) => tokio::spawn(async move {
                    if let Err(err) = delete_mod(state, &info).await {
                        tracing::error!(
                            "Failed to delete mod files. \
                                You might want to clean up the data directory manually. \
                                Reason: {:?}",
                            err
                        );
                    }

                    event_sink
                        .write()
                        .await
                        .submit_command(
                            ACTION_FINISH_DELETE_SELECTED_MOD,
                            SingleUse::new(info),
                            Target::Auto,
                        )
                        .expect("failed to send command");
                }),
            };
        }
    });

    Ok(())
}

#[cfg(not(arget_os = "windows"))]
fn get_default_config_path() -> PathBuf {
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
fn get_default_config_path() -> PathBuf {
    let config_dir = std::env::var("APPDATA").expect("appdata env var not set");
    PathBuf::from(config_dir).join("dtmm").join("dtmm.cfg")
}

#[cfg(not(arget_os = "windows"))]
fn get_default_data_dir() -> PathBuf {
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
fn get_default_data_dir() -> PathBuf {
    let data_dir = std::env::var("APPDATA").expect("appdata env var not set");
    PathBuf::from(data_dir).join("dtmm")
}

#[tracing::instrument]
fn main() -> Result<()> {
    color_eyre::install()?;

    let default_config_path = get_default_config_path();

    tracing::trace!(default_config_path = %default_config_path.display());

    let matches = command!()
        .arg(Arg::new("oodle").long("oodle").help(
            "The oodle library to load. This may either be:\n\
                        - A library name that will be searched for in the system's default paths.\n\
                        - A file path relative to the current working directory.\n\
                        - An absolute file path.",
        ))
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .help("Path to the config file")
                .value_parser(value_parser!(PathBuf))
                .default_value(default_config_path.to_string_lossy().to_string()),
        )
        .get_matches();

    dtmt_shared::create_tracing_subscriber();

    unsafe {
        oodle_sys::init(matches.get_one::<String>("oodle"));
    }

    let config: Config = {
        let path = matches
            .get_one::<PathBuf>("config")
            .expect("argument missing despite default");
        match fs::read(path) {
            Ok(data) => {
                let data = String::from_utf8(data).wrap_err_with(|| {
                    format!("config file {} contains invalid UTF-8", path.display())
                })?;
                serde_sjson::from_str(&data)
                    .wrap_err_with(|| format!("invalid config file {}", path.display()))?
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                if matches.value_source("config") != Some(ValueSource::DefaultValue) {
                    return Err(err).wrap_err_with(|| {
                        format!("failed to read config file {}", path.display())
                    })?;
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
                    fs::write(&default_config_path, data).wrap_err_with(|| {
                        format!(
                            "failed to write default config to {}",
                            default_config_path.display()
                        )
                    })?;
                }

                config
            }
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;
            }
        }
    };

    let initial_state = State::new(config);

    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let delegate = Delegate::new(sender);

    let launcher = AppLauncher::with_window(main_window::new()).delegate(delegate);

    let event_sink = launcher.get_external_handle();
    std::thread::spawn(move || {
        let event_sink = Arc::new(RwLock::new(event_sink));
        let receiver = Arc::new(RwLock::new(receiver));
        loop {
            if let Err(err) = work_thread(event_sink.clone(), receiver.clone()) {
                tracing::error!("Work thread failed, restarting: {:?}", err);
            }
        }
    });

    launcher.launch(initial_state).map_err(Report::new)
}
