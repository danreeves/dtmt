use clap::{Arg, ArgMatches, Command};
use color_eyre::eyre::Result;

mod decompress;
mod extract;
mod inject;
mod list;

pub(crate) fn command_definition() -> Command {
    Command::new("bundle")
        .subcommand_required(true)
        .about("Manipulate the game's bundle files")
        .arg(Arg::new("oodle").long("oodle").help(
            "The oodle library to load. This may either be:\n\
                        - A library name that will be searched for in the system's default paths.\n\
                        - A file path relative to the current working directory.\n\
                        - An absolute file path.",
        ))
        .subcommand(decompress::command_definition())
        .subcommand(extract::command_definition())
        .subcommand(inject::command_definition())
        .subcommand(list::command_definition())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("decompress", sub_matches)) => decompress::run(ctx, sub_matches).await,
        Some(("extract", sub_matches)) => extract::run(ctx, sub_matches).await,
        Some(("inject", sub_matches)) => inject::run(ctx, sub_matches).await,
        Some(("list", sub_matches)) => list::run(ctx, sub_matches).await,
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
