use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::Result;
use tokio::sync::RwLock;

pub(crate) fn command_definition() -> Command {
    Command::new("watch")
        .about("Re-build the given directory on file changes.")
        .arg(
            Arg::new("directory")
                .required(false)
                .default_value(".")
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the project to build. \
                If omitted, the current working directory is used.",
                ),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<RwLock<dtmt::Context>>, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
