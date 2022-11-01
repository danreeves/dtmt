use std::sync::Arc;

use clap::{Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::Result;
use tokio::sync::RwLock;

pub(crate) fn command_definition() -> Command {
    Command::new("murmur")
        .about("Perform various operations on Murmur hashes.")
        .subcommand(
            Command::new("hash")
                .arg(
                    Arg::new("string")
                        .required(true)
                        .help("The string to hash."),
                )
                .arg(
                    Arg::new("half")
                        .long("half")
                        .action(ArgAction::SetTrue)
                        .help(
                            "Use Fatshark's fake 32 bit algorithm \
                                        that cuts a 64 bit hash in half.",
                        ),
                ),
        )
        .subcommand(
            Command::new("lookup")
                .arg(Arg::new("hash").required(true).help("The hash to look up.")),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<RwLock<dtmt::Context>>, _matches: &ArgMatches) -> Result<()> {
    unimplemented!()
}
