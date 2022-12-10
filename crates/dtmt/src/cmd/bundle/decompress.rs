use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::Result;

use sdk::decompress;
use tokio::fs::{self, File};
use tokio::io::BufReader;
use tokio::sync::RwLock;

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
                .value_parser(value_parser!(PathBuf))
                .help(
                    "Path to the bundle to read. Unlike other operations, this only accepts only a single bundle.",
                ),
        )
        .arg(
            Arg::new("destination")
                .required(true)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The destination to write to. If this points to a directory, the \
                            name of the input bundle will be used. \
                            Parent directories must exist.",
                ),
        )
}

#[tracing::instrument(skip(ctx))]
async fn decompress_bundle<P1, P2>(
    ctx: Arc<RwLock<sdk::Context>>,
    bundle: P1,
    destination: P2,
) -> Result<()>
where
    P1: AsRef<Path> + std::fmt::Debug,
    P2: AsRef<Path> + std::fmt::Debug,
{
    let in_file = File::open(bundle).await?;
    let out_file = File::create(destination).await?;

    // A `BufWriter` does not help here, as we're mostly just out chunks.
    decompress(ctx, BufReader::new(in_file), out_file).await
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: Arc<RwLock<sdk::Context>>, matches: &ArgMatches) -> Result<()> {
    let bundle = matches
        .get_one::<PathBuf>("bundle")
        .expect("required argument 'bundle' is missing");
    let out_path = matches
        .get_one::<PathBuf>("destination")
        .expect("required parameter 'destination' is missing");

    let is_dir = fs::metadata(out_path)
        .await
        .map(|meta| meta.is_dir())
        .unwrap_or(false);

    let name = bundle.file_name();

    if is_dir && name.is_some() {
        decompress_bundle(ctx, bundle, out_path.join(name.unwrap())).await
    } else {
        decompress_bundle(ctx, bundle, out_path).await
    }
}
