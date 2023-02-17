use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, SectionExt};
use futures::StreamExt;
use sdk::Bundle;
use tokio::fs;

use crate::cmd::util::resolve_bundle_paths;

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

#[derive(Copy, Clone, Debug)]
enum OutputFormat {
    Text,
}

#[tracing::instrument(skip(ctx))]
async fn print_bundle_contents<P>(ctx: &sdk::Context, path: P, fmt: OutputFormat) -> Result<()>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let p = path.as_ref();
    let bundle = {
        let binary = fs::read(p).await?;
        let name = Bundle::get_name_from_path(ctx, p);
        Bundle::from_binary(ctx, name, binary)?
    };

    match fmt {
        OutputFormat::Text => {
            println!("Bundle: {}", bundle.name());

            for f in bundle.files().iter() {
                if f.variants().len() != 1 {
                    let err = eyre::eyre!("Expected exactly one version for this file.")
                        .with_section(|| f.variants().len().to_string().header("Bundle:"))
                        .with_section(|| bundle.name().clone().header("Bundle:"));

                    tracing::error!("{:#}", err);
                }

                let v = &f.variants()[0];
                println!(
                    "\t{}.{}: {} bytes",
                    f.base_name().display(),
                    f.file_type().ext_name(),
                    v.size()
                );
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let bundles = matches
        .get_many::<PathBuf>("bundle")
        .unwrap_or_default()
        .cloned();

    let paths = resolve_bundle_paths(bundles);

    let fmt = if matches.get_flag("json") {
        unimplemented!("JSON output is not implemented yet");
    } else {
        OutputFormat::Text
    };

    let ctx = Arc::new(ctx);

    paths
        .for_each_concurrent(10, |p| async {
            let ctx = ctx.clone();
            async move {
                if let Err(err) = print_bundle_contents(&ctx, &p, fmt)
                    .await
                    .wrap_err_with(|| format!("failed to list contents of bundle {}", p.display()))
                {
                    tracing::error!("{err:?}");
                }
            }
            .await;
        })
        .await;

    Ok(())
}
