use std::{io::Cursor, path::PathBuf};

use clap::{Arg, ArgMatches, Command, value_parser};
use color_eyre::{Result, eyre::Context as _};
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
        .subcommand(
            Command::new("find-file")
                .about("Find the bundle a file belongs to")
                .arg(
                    Arg::new("database")
                        .required(true)
                        .help("Path to the bundle database")
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("file-name")
                        .required(true)
                        .help("Name of the file. May be a hash in hex representation or a string"),
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
                            println!("{bundle_hash:016x} {name}");
                        }
                        IdString64::Hash(hash) => {
                            println!("{hash:016x}");
                        }
                    }

                    for file in files {
                        let name = ctx.lookup_hash(file.name, HashGroup::Filename);
                        let extension = file.extension.ext_name();

                        match name {
                            IdString64::String(name) => {
                                println!("\t{:016x}.{:<12} {}", file.name, extension, name);
                            }
                            IdString64::Hash(hash) => {
                                println!("\t{hash:016x}.{extension}");
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
                        println!("{bundle_hash:016x} {name}");
                    }
                    IdString64::Hash(hash) => {
                        println!("{hash:016x}");
                    }
                }
            }

            Ok(())
        }
        "find-file" => {
            let name = sub_matches
                .get_one::<String>("file-name")
                .expect("required argument");
            let name = match u64::from_str_radix(name, 16).map(Murmur64::from) {
                Ok(hash) => hash,
                Err(_) => Murmur64::hash(name),
            };

            let bundles = database.files().iter().filter_map(|(bundle_hash, files)| {
                if files.iter().any(|file| file.name == name) {
                    Some(bundle_hash)
                } else {
                    None
                }
            });

            let mut found = false;

            for bundle in bundles {
                found = true;
                println!("{bundle:016x}");
            }

            if !found {
                std::process::exit(1);
            }

            Ok(())
        }
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
