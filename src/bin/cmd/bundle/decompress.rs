use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, SectionExt};

use dtmt::decompress;
use futures::future::try_join_all;
use tokio::fs::{self, File};
use tokio::io::{BufReader, BufWriter};
use tokio::sync::RwLock;

use crate::cmd::util::collect_bundle_paths;

pub(crate) fn command_definition() -> Command {
    Command::new("decompress")
        .about(
            "Create a decompressed version of the given bundle(s).\n\
                    This is mostly useful for staring at the decompressed data in a hex editor,\n\
                    as neither the game nor this tool can read the decompressed bundles.",
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

#[tracing::instrument(skip(ctx))]
async fn decompress_bundle<P1, P2>(
    ctx: Arc<RwLock<dtmt::Context>>,
    bundle: P1,
    destination: P2,
) -> Result<()>
where
    P1: AsRef<Path> + std::fmt::Debug,
    P2: AsRef<Path> + std::fmt::Debug,
{
    let in_file = File::open(bundle).await?;
    let out_file = File::create(destination).await?;

    decompress(ctx, BufReader::new(in_file), BufWriter::new(out_file)).await
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: Arc<RwLock<dtmt::Context>>, matches: &ArgMatches) -> Result<()> {
    let bundles = matches
        .get_many::<PathBuf>("bundle")
        .unwrap_or_default()
        .cloned();
    let out_path = matches
        .get_one::<PathBuf>("destination")
        .expect("required parameter 'destination' is missing");

    let is_dir = fs::metadata(out_path)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false);

    let paths = collect_bundle_paths(bundles).await;

    if paths.is_empty() {
        return Err(eyre::eyre!("No bundle provided"));
    }

    if paths.len() == 1 {
        let bundle = &paths[0];
        let name = bundle.file_name();

        if is_dir && name.is_some() {
            decompress_bundle(ctx, bundle, out_path.join(name.unwrap())).await?;
        } else {
            decompress_bundle(ctx, bundle, out_path).await?;
        }
    } else {
        if !is_dir {
            return Err(eyre::eyre!(
                "Multiple bundles provided, but destination is not a directory."
            ))
            .with_section(|| out_path.display().to_string().header("Path:"))?;
        }

        let _ = try_join_all(paths.into_iter().map(|p| async {
            let ctx = ctx.clone();
            async move {
                let name = if let Some(name) = p.file_name() {
                    name
                } else {
                    return Err(eyre::eyre!("Invalid bundle path. No file name."))
                        .with_section(|| p.display().to_string().header("Path:"))?;
                };

                let dest = out_path.join(name);
                decompress_bundle(ctx, p, dest).await
            }
            .await
        }))
        .await?;
    }

    Ok(())
}
