#![recursion_limit = "256"]
#![feature(let_chains)]
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;

use clap::command;
use clap::value_parser;
use clap::Arg;
use color_eyre::eyre::Context;
use color_eyre::{Report, Result};
use druid::AppLauncher;
use tokio::sync::RwLock;

use crate::controller::app::load_mods;
use crate::controller::worker::work_thread;
use crate::state::{Delegate, State};

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

    let config = util::config::read_config(&default_config_path, &matches)
        .wrap_err("Failed to read config file")?;

    let game_info = dtmt_shared::collect_game_info()?;

    tracing::debug!(?config, ?game_info);

    let initial_state = {
        let mut state = State::new(
            config.path,
            config.game_dir.unwrap_or(game_info.path),
            config.data_dir.unwrap_or_default(),
        );
        state.mods = load_mods(state.get_mod_dir(), config.mod_order.iter())
            .wrap_err("Failed to load mods")?;
        state
    };

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();
    let delegate = Delegate::new(action_tx);

    let launcher = AppLauncher::with_window(ui::window::main::new()).delegate(delegate);

    let event_sink = launcher.get_external_handle();
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
