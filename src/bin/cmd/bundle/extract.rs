use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::Result;
use glob::Pattern;

use dtmt::Context;

fn parse_glob_pattern(s: &str) -> Result<Pattern, String> {
    match Pattern::new(s) {
        Ok(p) => Ok(p),
        Err(e) => Err(format!("Invalid glob pattern '{}': {}", s, e)),
    }
}

pub(crate) fn command_definition() -> Command {
    Command::new("extract")
        .about("Extract files from the bundle(s).")
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
                .help("Directory to extract files to."),
        )
        .arg(
            Arg::new("include")
                .long("include")
                .short('i')
                .action(ArgAction::Append)
                .value_parser(parse_glob_pattern)
                .help("Only extract files that match the given glob pattern(s)."),
        )
        .arg(
            Arg::new("exclude")
                .long("exclude")
                .short('e')
                .action(ArgAction::Append)
                .value_parser(parse_glob_pattern)
                .help(
                    "Do not extract files that match the given glob pattern(s).\n\
                                This takes precedence over `include`.",
                ),
        )
        .arg(
            Arg::new("flatten")
                .long("flatten")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Flatten the paths of extracted files into the file name."),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .short('n')
                .action(ArgAction::SetTrue)
                .help("Simulate write operations and log what would have been done."),
        )
        .arg(
            Arg::new("decompile")
                .long("decompile")
                .short('d')
                .action(ArgAction::SetTrue)
                .help(
                    "Attempt to decompile files after extracting them. Not all file types \
                                are supported for this.",
                ),
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
        .arg(Arg::new("ljd").long("ljd").help(
            "Path to a custom ljd executable. If not set, \
                                `ljd` will be called from PATH.",
        ))
        .arg(Arg::new("revorb").long("revorb").help(
            "Path to a custom revorb executable. If not set, \
                                `revorb` will be called from PATH.",
        ))
        .arg(Arg::new("ww2ogg").long("ww2ogg").help(
            "Path to a custom ww2ogg executable. If not set, \
                                `ww2ogg` will be called from PATH.\nSee the documentation for how \
                                to set up the script.",
        ))
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<Context>, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
