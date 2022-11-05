use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::{self, Result};
use color_eyre::{Help, SectionExt};
use dtmt::Bundle;
use futures::future::try_join_all;
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
pub(crate) async fn run(ctx: Arc<RwLock<dtmt::Context>>, matches: &ArgMatches) -> Result<()> {
    let bundles = matches
        .get_many::<PathBuf>("bundle")
        .unwrap_or_default()
        .cloned();

    let bundles = try_join_all(bundles.into_iter().map(|p| async {
        let ctx = ctx.clone();
        let path_display = p.display().to_string();
        async move { Bundle::open(ctx, &p).await }
            .await
            .with_section(|| path_display.header("Bundle Path:"))
    }))
    .await?;

    if matches.get_flag("json") {
        unimplemented!("JSON output is not implemented yet");
    } else {
        for b in bundles.iter() {
            println!("Bundle: {}", b.name());

            for f in b.files().iter() {
                if f.variants().len() != 1 {
                    return Err(eyre::eyre!("Expected exactly one version for this file."))
                        .with_section(|| f.variants().len().to_string().header("Bundle:"))
                        .with_section(|| b.name().clone().header("Bundle:"));
                }

                let v = &f.variants()[0];
                println!(
                    "\t{}.{}: {} bytes",
                    f.base_name(),
                    f.file_type().ext_name(),
                    v.size()
                );
            }
        }

        Ok(())
    }
}
