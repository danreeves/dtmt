use clap::command;
use color_eyre::Report;
use color_eyre::Result;
use druid::AppLauncher;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::state::State;

mod main_window;
mod state;
mod theme;
mod widget;

#[tracing::instrument]
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let _matches = command!().get_matches();

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

    let initial_state = State::new();

    AppLauncher::with_window(main_window::new())
        .launch(initial_state)
        .map_err(Report::new)
}
