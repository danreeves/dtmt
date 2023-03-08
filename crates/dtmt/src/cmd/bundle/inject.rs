use std::path::PathBuf;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::Help;
use sdk::Bundle;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;

pub(crate) fn command_definition() -> Command {
    Command::new("inject")
        .about("Inject a file into a bundle.")
        .arg(
            Arg::new("replace")
                .help("The name of a file in the bundle whos content should be replaced.")
                .short('r')
                .long("replace"),
        )
        .arg(
            Arg::new("output")
                .help(
                    "The path to write the changed bundle to. \
                    If omitted, the input bundle will be overwritten.",
                )
                .short('o')
                .long("output")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("bundle")
                .help("Path to the bundle to inject the file into.")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("file")
                .help("Path to the file to inject.")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let bundle_path = matches
        .get_one::<PathBuf>("bundle")
        .expect("required parameter not found");

    let file_path = matches
        .get_one::<PathBuf>("file")
        .expect("required parameter not found");

    tracing::trace!(bundle_path = %bundle_path.display(), file_path = %file_path.display());

    let mut bundle = {
        let binary = fs::read(bundle_path).await?;
        let name = Bundle::get_name_from_path(&ctx, bundle_path);
        Bundle::from_binary(&ctx, name, binary).wrap_err("Failed to open bundle file")?
    };

    if let Some(name) = matches.get_one::<String>("replace") {
        let mut file = File::open(&file_path)
            .await
            .wrap_err_with(|| format!("Failed to open '{}'", file_path.display()))?;

        if let Some(variant) = bundle
            .files_mut()
            .filter(|file| file.matches_name(name.clone()))
            // TODO: Handle file variants
            .find_map(|file| file.variants_mut().next())
        {
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .await
                .wrap_err("Failed to read input file")?;
            variant.set_data(data);
        } else {
            let err = eyre::eyre!("No file '{}' in this bundle.", name)
                .with_suggestion(|| {
                    format!(
                        "Run '{} bundle list {}' to list the files in this bundle.",
                        clap::crate_name!(),
                        bundle_path.display()
                    )
                })
                .with_suggestion(|| {
                    format!(
                        "Use '{} bundle inject --add {} {} {}' to add it as a new file",
                        clap::crate_name!(),
                        name,
                        bundle_path.display(),
                        file_path.display()
                    )
                });

            return Err(err);
        }

        let out_path = matches.get_one::<PathBuf>("output").unwrap_or(bundle_path);
        let data = bundle
            .to_binary()
            .wrap_err("Failed to write changed bundle to output")?;

        fs::write(out_path, &data)
            .await
            .wrap_err("Failed to write data to output file")?;

        Ok(())
    } else {
        eyre::bail!("Currently, only the '--replace' operation is supported.");
    }
}
