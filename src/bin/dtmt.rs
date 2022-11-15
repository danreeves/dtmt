#![feature(io_error_more)]
#![feature(let_chains)]

use std::path::PathBuf;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::value_parser;
use clap::{command, Arg};
use color_eyre::eyre::{Context, Result};
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::sync::RwLock;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod cmd {
    pub mod build;
    pub mod bundle;
    pub mod dictionary;
    pub mod murmur;
    pub mod new;
    mod util;
    pub mod watch;
}

#[tokio::main]
#[tracing::instrument]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let matches = command!()
        .subcommand_required(true)
        .arg(
            Arg::new("dictionary")
                .help(
                    "Path to a dictionary file CSV format used to look up pre-computed murmur hashes.\
                    \nWill default to `dictionary.csv` in the current directory.",
                )
                .default_value("dictionary.csv")
                .long("dict")
                .global(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .subcommand(cmd::build::command_definition())
        .subcommand(cmd::bundle::command_definition())
        .subcommand(cmd::dictionary::command_definition())
        .subcommand(cmd::murmur::command_definition())
        .subcommand(cmd::new::command_definition())
        .subcommand(cmd::watch::command_definition())
        .get_matches();

    {
        let fmt_layer = tracing_subscriber::fmt::layer().pretty();
        let filter_layer =
            EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .with(ErrorLayer::new(
                tracing_subscriber::fmt::format::Pretty::default(),
            ))
            .init();
    }

    let ctx = dtmt::Context::new();
    let ctx = Arc::new(RwLock::new(ctx));

    {
        let path = matches
            .get_one::<PathBuf>("dictionary")
            .cloned()
            .expect("no default value for 'dictionary' parameter");
        let is_default = matches.value_source("dictionary") == Some(ValueSource::DefaultValue);
        let ctx = ctx.clone();

        tokio::spawn(async move {
            let mut ctx = ctx.write().await;
            let res = File::open(&path)
                .await
                .wrap_err_with(|| format!("failed to open dictionary file: {}", path.display()));

            let f = match res {
                Ok(f) => f,
                Err(err) => {
                    if is_default {
                        return;
                    }
                    tracing::error!("{}", err);

                    return;
                }
            };

            let r = BufReader::new(f);
            if let Err(err) = ctx.lookup.from_csv(r).await {
                tracing::error!("{:?}", err);
            }
        });
    }

    match matches.subcommand() {
        Some(("bundle", sub_matches)) => cmd::bundle::run(ctx, sub_matches).await?,
        Some(("murmur", sub_matches)) => cmd::murmur::run(ctx, sub_matches).await?,
        Some(("new", sub_matches)) => cmd::new::run(ctx, sub_matches).await?,
        Some(("build", sub_matches)) => cmd::build::run(ctx, sub_matches).await?,
        Some(("watch", sub_matches)) => cmd::watch::run(ctx, sub_matches).await?,
        Some(("dictionary", sub_matches)) => cmd::dictionary::run(ctx, sub_matches).await?,
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }

    Ok(())
}
