#![recursion_limit = "256"]
#![feature(let_chains)]
#![feature(arc_unwrap_or_clone)]
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::{command, value_parser, Arg};
use color_eyre::eyre;
use color_eyre::{Report, Result};
use druid::AppLauncher;
use tokio::sync::RwLock;

use crate::controller::worker::work_thread;
use crate::state::AsyncAction;
use crate::state::{Delegate, State};
use crate::ui::theme;
use crate::util::log::LogLevel;

mod controller;
mod state;
mod util {
    pub mod ansi;
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
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .help("Path to the config file")
                .value_parser(value_parser!(PathBuf))
                .default_value(default_config_path.to_string_lossy().to_string()),
        )
        .arg(
            Arg::new("log-level")
                .long("log-level")
                .help("The maximum level of log events to print")
                .value_parser(value_parser!(LogLevel))
                .default_value("info"),
        )
        .get_matches();

    let (log_tx, log_rx) = tokio::sync::mpsc::unbounded_channel();
    let level = if matches.value_source("log-level") == Some(ValueSource::DefaultValue) {
        None
    } else {
        matches.get_one::<LogLevel>("log-level").cloned()
    };
    util::log::create_tracing_subscriber(log_tx, level);

    let (action_tx, action_rx) = tokio::sync::mpsc::unbounded_channel();

    let config_path = matches
        .get_one::<PathBuf>("config")
        .cloned()
        .expect("argument has default value");
    let is_config_default = matches.value_source("config") == Some(ValueSource::DefaultValue);
    if action_tx
        .send(AsyncAction::LoadInitial((config_path, is_config_default)))
        .is_err()
    {
        let err = eyre::eyre!("Failed to send action");
        return Err(err);
    }

    let launcher = AppLauncher::with_window(ui::window::main::new())
        .delegate(Delegate::new(action_tx))
        .configure_env(theme::set_theme_env);

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

    launcher.launch(State::new()).map_err(Report::new)
}
