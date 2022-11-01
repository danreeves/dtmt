use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::Result;

pub(crate) fn command_definition() -> Command {
    Command::new("decompress")
        .about(
            "Create a decompressed version of the given bundle(s).\n\
                    This is mostly useful for staring at the decompressed data in a hex editor,\n\
                    as neither the game nor this tool can read the decompressed bundles.",
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
        .arg(
            Arg::new("destination")
                .required(true)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The destination to write to. If this points to a directory, the \
                            decompressed bundles will be written there, with their original name. \
                            Parent directories must exist.",
                ),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<dtmt::Context>, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
