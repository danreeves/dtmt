use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use color_eyre::Help;
use color_eyre::eyre::{self, Context, OptionExt, Result};
use path_slash::PathBufExt as _;
use sdk::murmur::IdString64;
use sdk::{Bundle, BundleFile, BundleFileType};
use tokio::fs;

pub(crate) fn command_definition() -> Command {
    Command::new("inject")
        .subcommand_required(true)
        .about("Inject a file into a bundle.\n\
            Raw binary data can be used to directly replace the file's variant data blob without affecting the metadata.\n\
            Alternatively, a compiler format may be specified, and a complete bundle file is created.")
        .arg(
            Arg::new("output")
                .help(
                    "The path to write the changed bundle to. \
                    If omitted, the input bundle will be overwritten.\n\
                    Remember to add a `.patch_<NUMBER>` suffix if you also use '--patch'.",
                )
                .short('o')
                .long("output")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("patch")
                .help("Create a patch bundle. Optionally, a patch NUMBER may be specified as \
                    '--patch=123'.\nThe maximum number is 999, the default is 1.\n\
                    If `--output` is not specified, the `.patch_<NUMBER>` suffix is added to \
                    the given bundle name.")
                .short('p')
                .long("patch")
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value("1")
                .value_name("NUMBER")
                .value_parser(value_parser!(u16))
        )
        .arg(
            Arg::new("type")
                .help("Compile the new file as the given TYPE. If omitted, the file type is \
                    is guessed from the file extension.")
                .value_name("TYPE")
        )
        .subcommand(
            Command::new("replace")
                .about("Replace an existing file in the bundle")
                .arg(
                    Arg::new("variant")
                        .help("In combination with '--raw', specify the variant index to replace.")
                        .long("variant")
                        .default_value("0")
                        .value_parser(value_parser!(u8))
                )
                .arg(
                    Arg::new("raw")
                        .help("Insert the given file as raw binary data.\n\
                            Cannot be used with '--patch'.")
                        .long("raw")
                        .action(ArgAction::SetTrue)
                )
                .arg(
                    Arg::new("bundle")
                        .help("Path to the bundle to inject the file into.")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("bundle-file")
                        .help("The name of a file in the bundle whose content should be replaced.")
                        .required(true),
                )
                .arg(
                    Arg::new("new-file")
                        .help("Path to the file to inject.")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
    // .subcommand(
    //     Command::new("add")
    //         .about("Add a new file to the bundle")
    //         .arg(
    //             Arg::new("new-file")
    //                 .help("Path to the file to inject.")
    //                 .required(true)
    //                 .value_parser(value_parser!(PathBuf)),
    //         )
    //         .arg(
    //             Arg::new("bundle")
    //                 .help("Path to the bundle to inject the file into.")
    //                 .required(true)
    //                 .value_parser(value_parser!(PathBuf)),
    //         ),
    // )
}

#[tracing::instrument]
async fn compile_file(
    path: impl AsRef<Path> + std::fmt::Debug,
    name: impl Into<IdString64> + std::fmt::Debug,
    file_type: BundleFileType,
) -> Result<BundleFile> {
    let path = path.as_ref();

    let file_data = fs::read(&path)
        .await
        .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;
    let _sjson = String::from_utf8(file_data)
        .wrap_err_with(|| format!("Invalid UTF8 data in '{}'", path.display()))?;

    let _root = path.parent().ok_or_eyre("File path has no parent")?;

    eyre::bail!(
        "Compilation for type '{}' is not implemented, yet",
        file_type
    )
}

#[tracing::instrument(
    skip_all,
    fields(
        bundle_path = tracing::field::Empty,
        in_file_path = tracing::field::Empty,
        output_path = tracing::field::Empty,
        target_name = tracing::field::Empty,
        file_type = tracing::field::Empty,
        raw = tracing::field::Empty,
    )
)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let Some((op, sub_matches)) = matches.subcommand() else {
        unreachable!("clap is configured to require a subcommand, and they're all handled above");
    };

    let bundle_path = sub_matches
        .get_one::<PathBuf>("bundle")
        .expect("required parameter not found");

    let in_file_path = sub_matches
        .get_one::<PathBuf>("new-file")
        .expect("required parameter not found");

    let patch_number = matches
        .get_one::<u16>("patch")
        .map(|num| format!("{num:03}"));

    let output_path = matches
        .get_one::<PathBuf>("output")
        .cloned()
        .unwrap_or_else(|| {
            let mut output_path = bundle_path.clone();

            if let Some(patch_number) = patch_number.as_ref() {
                output_path.set_extension(format!("patch_{patch_number:03}"));
            }

            output_path
        });

    let target_name = if op == "replace" {
        sub_matches
            .get_one::<String>("bundle-file")
            .map(|name| match u64::from_str_radix(name, 16) {
                Ok(id) => IdString64::from(id),
                Err(_) => IdString64::String(name.clone()),
            })
            .expect("argument is required")
    } else {
        let mut path = PathBuf::from(in_file_path);
        path.set_extension("");
        IdString64::from(path.to_slash_lossy().to_string())
    };

    let file_type = if let Some(forced_type) = matches.get_one::<String>("type") {
        BundleFileType::from_str(forced_type.as_str()).wrap_err("Unknown file type")?
    } else {
        in_file_path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or_eyre("File extension missing")
            .and_then(BundleFileType::from_str)
            .wrap_err("Unknown file type")
            .with_suggestion(|| "Use '--type TYPE' to specify the file type")?
    };

    {
        let span = tracing::Span::current();
        if !span.is_disabled() {
            span.record("bundle_path", bundle_path.display().to_string());
            span.record("in_file_path", in_file_path.display().to_string());
            span.record("output_path", output_path.display().to_string());
            span.record("raw", sub_matches.get_flag("raw"));
            span.record("target_name", target_name.display().to_string());
            span.record("file_type", format!("{file_type:?}"));
        }
    }

    let bundle_name = Bundle::get_name_from_path(&ctx, bundle_path);
    let mut bundle = {
        fs::read(bundle_path)
            .await
            .map_err(From::from)
            .and_then(|binary| Bundle::from_binary(&ctx, bundle_name.clone(), binary))
            .wrap_err_with(|| format!("Failed to open bundle '{}'", bundle_path.display()))?
    };

    let output_bundle = match op {
        "replace" => {
            let Some(file) = bundle
                .files_mut()
                .find(|file| *file.base_name() == target_name)
            else {
                let err = eyre::eyre!(
                    "No file with name '{}' in bundle '{}'",
                    target_name.display(),
                    bundle_path.display()
                );

                return Err(err).with_suggestion(|| {
                    format!(
                        "Run '{} bundle list \"{}\"' to list the files in this bundle.",
                        clap::crate_name!(),
                        bundle_path.display()
                    )
                });
            };

            if sub_matches.get_flag("raw") {
                let variant_index = sub_matches
                    .get_one::<u8>("variant")
                    .expect("argument with default missing");

                let Some(variant) = file.variants_mut().nth(*variant_index as usize) else {
                    let err = eyre::eyre!(
                        "Variant index '{}' does not exist in '{}'",
                        variant_index,
                        target_name.display()
                    );

                    return Err(err).with_suggestion(|| {
                        format!(
                            "See '{} bundle inject add --help' if you want to add it as a new file",
                            clap::crate_name!(),
                        )
                    });
                };

                let data = tokio::fs::read(&in_file_path).await.wrap_err_with(|| {
                    format!("Failed to read file '{}'", in_file_path.display())
                })?;
                variant.set_data(data);
                file.set_modded(true);
                bundle
            } else {
                let mut bundle_file = compile_file(in_file_path, target_name.clone(), file_type)
                    .await
                    .wrap_err("Failed to compile")?;

                bundle_file.set_modded(true);

                if patch_number.is_some() {
                    let mut output_bundle = Bundle::new(bundle_name);
                    output_bundle.add_file(bundle_file);
                    output_bundle
                } else {
                    *file = bundle_file;

                    dbg!(&file);
                    bundle
                }
            }
        }
        "add" => {
            unimplemented!("Implement adding a new file to the bundle.");
        }
        "copy" => {
            unimplemented!("Implement copying a file from one bundle to the other.");
        }
        _ => unreachable!("no other operations exist"),
    };

    let data = output_bundle
        .to_binary()
        .wrap_err("Failed to write changed bundle to output")?;

    fs::write(&output_path, &data)
        .await
        .wrap_err_with(|| format!("Failed to write data to '{}'", output_path.display()))?;

    tracing::info!("Modified bundle written to '{}'", output_path.display());

    Ok(())
}
