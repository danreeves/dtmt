#![recursion_limit = "256"]
#![feature(let_chains)]

use std::path::PathBuf;
use std::sync::Arc;

use clap::command;
use clap::value_parser;
use clap::Arg;
use color_eyre::eyre::Context;
use color_eyre::{Report, Result};
use druid::AppLauncher;
use tokio::sync::RwLock;

use crate::controller::worker::work_thread;
use crate::state::{Delegate, State};

mod controller {
    pub mod engine;
    pub mod worker;
}
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
        .wrap_err("failed to read config file")?;

    let initial_state = State::new(config);

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
