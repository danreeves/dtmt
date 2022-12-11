use std::sync::Arc;

use clap::{Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::Result;
use sdk::murmur::{Murmur32, Murmur64};
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
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: Arc<RwLock<sdk::Context>>, matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("hash", sub_matches)) => {
            let s = sub_matches
                .get_one::<String>("string")
                .expect("missing required argument");

            if sub_matches.get_flag("half") {
                let hash = Murmur32::hash(s);
                println!("{hash:08X}");
            } else {
                let hash = Murmur64::hash(s);
                println!("{hash:016X}");
            }

            Ok(())
        }
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
