#![recursion_limit = "256"]
#![feature(let_chains)]

use std::sync::Arc;

use clap::command;
use clap::Arg;
use color_eyre::Report;
use color_eyre::Result;
use druid::AppLauncher;
use druid::ExtEventSink;
use druid::SingleUse;
use druid::Target;
use engine::import_mod;
use state::ACTION_FINISH_ADD_MOD;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::engine::deploy_mods;
use crate::state::{AsyncAction, Delegate, State, ACTION_FINISH_DEPLOY};

mod controller;
mod engine;
mod main_window;
mod state;
mod theme;
mod widget;

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
            };
        }
    });

    Ok(())
}

#[tracing::instrument]
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let matches = command!()
        .arg(Arg::new("oodle").long("oodle").help(
            "The oodle library to load. This may either be:\n\
                        - A library name that will be searched for in the system's default paths.\n\
                        - A file path relative to the current working directory.\n\
                        - An absolute file path.",
        ))
        .get_matches();

    {
        let filter_layer =
            EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

        if cfg!(debug_assertions) {
            let fmt_layer = tracing_subscriber::fmt::layer().pretty();

            tracing_subscriber::registry()
                .with(filter_layer)
                .with(fmt_layer)
                .with(ErrorLayer::new(
                    tracing_subscriber::fmt::format::Pretty::default(),
                ))
                .init();
        } else {
            let fmt_layer = tracing_subscriber::fmt::layer().compact();

            tracing_subscriber::registry()
                .with(filter_layer)
                .with(fmt_layer)
                .with(ErrorLayer::new(
                    tracing_subscriber::fmt::format::Pretty::default(),
                ))
                .init();
        }
    }

    unsafe {
        oodle_sys::init(matches.get_one::<String>("oodle"));
    }

    let initial_state = State::new();

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
