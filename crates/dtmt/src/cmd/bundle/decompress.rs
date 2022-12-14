use std::path::{Path, PathBuf};

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::Result;

use sdk::decompress;
use tokio::fs;

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
async fn decompress_bundle<P1, P2>(ctx: &sdk::Context, bundle: P1, destination: P2) -> Result<()>
where
    P1: AsRef<Path> + std::fmt::Debug,
    P2: AsRef<Path> + std::fmt::Debug,
{
    let binary = fs::read(bundle).await?;
    let data = decompress(ctx, binary)?;
    fs::write(destination, &data).await?;

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
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
        decompress_bundle(&ctx, bundle, out_path.join(name.unwrap())).await
    } else {
        decompress_bundle(&ctx, bundle, out_path).await
    }
}
