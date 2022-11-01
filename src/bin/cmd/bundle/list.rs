use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::Result;
use tokio::sync::RwLock;

pub(crate) fn command_definition() -> Command {
    Command::new("list")
        .about("List the contents of one or multiple bundles.")
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print machine-readable JSON"),
        )
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
        .arg(
            Arg::new("bundle")
                .required(true)
                .action(ArgAction::Append)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "Path to the bundle(s) to read. If this points to a directory instead \
                            of a file, all files in that directory will be checked.",
                ),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<RwLock<dtmt::Context>>, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
