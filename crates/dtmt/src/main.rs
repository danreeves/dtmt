#![feature(io_error_more)]
#![feature(let_chains)]
#![feature(test)]
#![windows_subsystem = "console"]

use std::path::PathBuf;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::value_parser;
use clap::{command, Arg};
use color_eyre::eyre;
use color_eyre::eyre::{Context, Result};
use sdk::murmur::Dictionary;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::sync::RwLock;

mod cmd {
    pub mod build;
    pub mod bundle;
    pub mod dictionary;
    pub mod migrate;
    pub mod murmur;
    pub mod new;
    pub mod package;
    mod util;
    pub mod watch;
}
mod shell_parse;

#[derive(Default, Deserialize, Serialize)]
struct GlobalConfig {
    game_dir: Option<PathBuf>,
}

#[tokio::main]
#[tracing::instrument(level = "error", fields(cmd_line = tracing::field::Empty))]
async fn main() -> Result<()> {
    color_eyre::install()?;

    {
        let span = tracing::Span::current();
        if !span.is_disabled() {
            let cmdline: String = std::env::args_os().fold(String::new(), |mut s, arg| {
                s.push_str(&arg.to_string_lossy());
                s
            });
            span.record("cmd_line", cmdline);
        }
    }

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
        .subcommand(cmd::migrate::command_definition())
        .subcommand(cmd::murmur::command_definition())
        .subcommand(cmd::new::command_definition())
        .subcommand(cmd::package::command_definition())
        .subcommand(cmd::watch::command_definition())
        .get_matches();

    dtmt_shared::create_tracing_subscriber();

    // TODO: Move this into a `Context::init` method?
    let ctx = sdk::Context::new();
    let ctx = Arc::new(RwLock::new(ctx));

    let dicitonary_task = {
        let path = matches
            .get_one::<PathBuf>("dictionary")
            .cloned()
            .expect("no default value for 'dictionary' parameter");
        let is_default = matches.value_source("dictionary") == Some(ValueSource::DefaultValue);
        let ctx = ctx.clone();

        tokio::spawn(async move {
            let res = File::open(&path)
                .await
                .wrap_err_with(|| format!("Failed to open dictionary file: {}", path.display()));

            let f = match res {
                Ok(f) => f,
                Err(err) => {
                    if !is_default {
                        // The dictionary is entirely optional, so only report the error
                        // when the user asked for a specific path.
                        tracing::error!("{:#}", err);
                    }
                    return;
                }
            };

            let r = BufReader::new(f);
            let mut ctx = ctx.write().await;
            match Dictionary::from_csv(r).await {
                Ok(lookup) => ctx.lookup = Arc::new(lookup),
                Err(err) => tracing::error!("{:#}", err),
            }
        })
    };

    let global_config_task = {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let conf = tokio::task::spawn_blocking(|| {
                confy::load::<GlobalConfig>(clap::crate_name!(), None)
                    .wrap_err("Failed to load global configuration")
            })
            .await;

            match conf {
                Ok(Ok(cfg)) => {
                    let mut ctx = ctx.write().await;
                    ctx.game_dir = cfg.game_dir;
                }
                Ok(Err(err)) => tracing::error!("{:#}", err),
                Err(err) => tracing::error!("{:#}", err),
            }
        })
    };

    tokio::try_join!(dicitonary_task, global_config_task)?;

    let ctx = match Arc::try_unwrap(ctx).map(|ctx| ctx.into_inner()) {
        Ok(ctx) => ctx,
        Err(_) => eyre::bail!("failed to unwrap context"),
    };

    match matches.subcommand() {
        Some(("build", sub_matches)) => cmd::build::run(ctx, sub_matches).await?,
        Some(("bundle", sub_matches)) => cmd::bundle::run(ctx, sub_matches).await?,
        Some(("dictionary", sub_matches)) => cmd::dictionary::run(ctx, sub_matches).await?,
        Some(("migrate", sub_matches)) => cmd::migrate::run(ctx, sub_matches).await?,
        Some(("murmur", sub_matches)) => cmd::murmur::run(ctx, sub_matches).await?,
        Some(("new", sub_matches)) => cmd::new::run(ctx, sub_matches).await?,
        Some(("package", sub_matches)) => cmd::package::run(ctx, sub_matches).await?,
        Some(("watch", sub_matches)) => cmd::watch::run(ctx, sub_matches).await?,
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }

    Ok(())
}
