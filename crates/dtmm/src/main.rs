#![recursion_limit = "256"]
#![feature(let_chains)]
#![feature(arc_unwrap_or_clone)]
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;

use clap::command;
use clap::value_parser;
use clap::Arg;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::{Report, Result};
use druid::AppLauncher;
use druid::SingleUse;
use druid::Target;
use tokio::sync::RwLock;

use crate::controller::app::load_mods;
use crate::controller::worker::work_thread;
use crate::state::ACTION_SHOW_ERROR_DIALOG;
use crate::state::{Delegate, State};
use crate::ui::theme;

mod controller;
mod state;
mod util {
    pub mod config;
    pub mod log;
}
mod ui;

#[tracing::instrument]
fn main() -> Result<()> {
    color_eyre::install()?;

    let default_config_path = util::config::get_default_config_path();

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

    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    util::log::create_tracing_subscriber(log_tx);

    unsafe {
        oodle_sys::init(matches.get_one::<String>("oodle"));
    }

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();
    let delegate = Delegate::new(action_tx);

    let launcher = AppLauncher::with_window(ui::window::main::new())
        .delegate(delegate)
        .configure_env(theme::set_theme_env);

    let event_sink = launcher.get_external_handle();

    let config = util::config::read_config(&default_config_path, &matches)
        .wrap_err("Failed to read config file")?;
    let game_info = dtmt_shared::collect_game_info();

    tracing::debug!(?config, ?game_info);

    let game_dir = config.game_dir.or_else(|| game_info.map(|i| i.path));
    if game_dir.is_none() {
        let err =
            eyre::eyre!("No Game Directory set. Head to the 'Settings' tab to set it manually",);
        event_sink
            .submit_command(ACTION_SHOW_ERROR_DIALOG, SingleUse::new(err), Target::Auto)
            .expect("failed to send command");
    }

    let initial_state = {
        let mut state = State::new(
            config.path,
            game_dir.unwrap_or_default(),
            config.data_dir.unwrap_or_default(),
            config.nexus_api_key.unwrap_or_default(),
        );
        state.mods = load_mods(state.get_mod_dir(), config.mod_order.iter())
            .wrap_err("Failed to load mods")?;
        state
    };

    std::thread::spawn(move || {
        let event_sink = Arc::new(RwLock::new(event_sink));
        let action_rx = Arc::new(RwLock::new(action_rx));
        let log_rx = Arc::new(RwLock::new(log_rx));
        loop {
            if let Err(err) = work_thread(event_sink.clone(), action_rx.clone(), log_rx.clone()) {
                tracing::error!("Work thread failed, restarting: {:?}", err);
            }
        }
    });

    launcher.launch(initial_state).map_err(Report::new)
}
