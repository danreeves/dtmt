use std::sync::Arc;

use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use tokio::sync::RwLock;

mod decompress;
mod extract;
mod list;

pub(crate) fn command_definition() -> Command {
    Command::new("bundle")
        .subcommand_required(true)
        .about("Manipulate the game's bundle files")
        .arg(
            Arg::new("oodle")
                .long("oodle")
                .default_value("oodle-cli")
                .help(
                    "Name of or path to the Oodle decompression helper. \
                    The helper is a small executable that wraps the Oodle library \
                    with a CLI.",
                ),
        )
        .subcommand(decompress::command_definition())
        .subcommand(extract::command_definition())
        .subcommand(list::command_definition())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: Arc<RwLock<dtmt::Context>>, matches: &ArgMatches) -> Result<()> {
    let oodle_bin = matches
        .get_one::<String>("oodle")
        .expect("no default value for 'oodle' parameter");
    {
        let mut ctx = ctx.write().await;
        ctx.oodle = Some(oodle_bin.clone());
    }

    match matches.subcommand() {
        Some(("decompress", sub_matches)) => decompress::run(ctx, sub_matches).await,
        Some(("extract", sub_matches)) => extract::run(ctx, sub_matches).await,
        Some(("list", sub_matches)) => list::run(ctx, sub_matches).await,
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
