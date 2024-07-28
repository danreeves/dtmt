use std::{io::Cursor, path::PathBuf};

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::{eyre::Context as _, Result};
use sdk::murmur::{HashGroup, IdString64, Murmur64};
use sdk::{BundleDatabase, FromBinary as _};
use tokio::fs;

pub(crate) fn command_definition() -> Command {
    Command::new("db")
        .about("Various operations regarding `bundle_database.data`.")
        .subcommand_required(true)
        .subcommand(
            Command::new("list-files")
                .about("List bundle contents")
                .arg(
                    Arg::new("database")
                        .required(true)
                        .help("Path to the bundle database")
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("bundle")
                        .help("The bundle name. If omitted, all bundles will be listed.")
                        .required(false),
                ),
        )
        .subcommand(
            Command::new("list-bundles").about("List bundles").arg(
                Arg::new("database")
                    .required(true)
                    .help("Path to the bundle database")
                    .value_parser(value_parser!(PathBuf)),
            ),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let Some((op, sub_matches)) = matches.subcommand() else {
        unreachable!("clap is configured to require a subcommand");
    };

    let database = {
        let path = sub_matches
            .get_one::<PathBuf>("database")
            .expect("argument is required");

        let binary = fs::read(&path)
            .await
            .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;

        let mut r = Cursor::new(binary);

        BundleDatabase::from_binary(&mut r).wrap_err("Failed to parse bundle database")?
    };

    match op {
        "list-files" => {
            let index = database.files();

            if let Some(bundle) = sub_matches.get_one::<String>("bundle") {
                let hash = u64::from_str_radix(bundle, 16)
                    .map(Murmur64::from)
                    .wrap_err("Invalid hex sequence")?;

                if let Some(files) = index.get(&hash) {
                    for file in files {
                        let name = ctx.lookup_hash(file.name, HashGroup::Filename);
                        let extension = file.extension.ext_name();
                        println!("{}.{}", name.display(), extension);
                    }
                } else {
                    tracing::info!("Bundle {} not found in the database", bundle);
                }
            } else {
                for (bundle_hash, files) in index.iter() {
                    let bundle_name = ctx.lookup_hash(*bundle_hash, HashGroup::Filename);

                    match bundle_name {
                        IdString64::String(name) => {
                            println!("{:016X} {}", bundle_hash, name);
                        }
                        IdString64::Hash(hash) => {
                            println!("{:016X}", hash);
                        }
                    }

                    for file in files {
                        let name = ctx.lookup_hash(file.name, HashGroup::Filename);
                        let extension = file.extension.ext_name();

                        match name {
                            IdString64::String(name) => {
                                println!("\t{:016X}.{:<12} {}", file.name, extension, name);
                            }
                            IdString64::Hash(hash) => {
                                println!("\t{:016X}.{}", hash, extension);
                            }
                        }
                    }

                    println!();
                }
            }

            Ok(())
        }
        "list-bundles" => {
            for bundle_hash in database.bundles().keys() {
                let bundle_name = ctx.lookup_hash(*bundle_hash, HashGroup::Filename);

                match bundle_name {
                    IdString64::String(name) => {
                        println!("{:016X} {}", bundle_hash, name);
                    }
                    IdString64::Hash(hash) => {
                        println!("{:016X}", hash);
                    }
                }
            }

            Ok(())
        }
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
