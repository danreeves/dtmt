#![feature(io_error_more)]

use std::sync::Arc;

use clap::{command, Arg, ArgAction};
use color_eyre::eyre::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use dtmt::Context;

mod cmd {
    pub mod build;
    pub mod bundle;
    pub mod murmur;
    pub mod new;
    pub mod watch;
}

#[tokio::main]
#[tracing::instrument]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let matches = command!()
        .subcommand_required(true)
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .action(ArgAction::Count)
                .help(
                    "Increase verbosity of informational and debugging output. \
                    May be specified multiple times.",
                ),
        )
        .subcommand(cmd::build::command_definition())
        .subcommand(cmd::bundle::command_definition())
        .subcommand(cmd::murmur::command_definition())
        .subcommand(cmd::new::command_definition())
        .subcommand(cmd::watch::command_definition())
        .get_matches();

    {
        let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);
        let filter_layer =
            EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .with(ErrorLayer::default())
            .init();
    }

    let ctx = Context::new();

    match matches.subcommand() {
        Some(("bundle", sub_matches)) => cmd::bundle::run(Arc::new(ctx), sub_matches).await?,
        Some(("murmur", sub_matches)) => cmd::murmur::run(Arc::new(ctx), sub_matches).await?,
        Some(("new", sub_matches)) => cmd::new::run(Arc::new(ctx), sub_matches).await?,
        Some(("build", sub_matches)) => cmd::build::run(Arc::new(ctx), sub_matches).await?,
        Some(("watch", sub_matches)) => cmd::watch::run(Arc::new(ctx), sub_matches).await?,
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }

    Ok(())
}
