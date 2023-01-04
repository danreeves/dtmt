use std::path::PathBuf;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command, ValueEnum};
use color_eyre::eyre::{Context, Result};
use color_eyre::{Help, SectionExt};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use tokio_stream::StreamExt;

#[derive(Copy, Clone, PartialEq, ValueEnum)]
pub enum HashGroup {
    Filename,
    Filetype,
    Strings,
    Other,
}

impl From<HashGroup> for sdk::murmur::HashGroup {
    fn from(value: HashGroup) -> Self {
        match value {
            HashGroup::Filename => sdk::murmur::HashGroup::Filename,
            HashGroup::Filetype => sdk::murmur::HashGroup::Filetype,
            HashGroup::Strings => sdk::murmur::HashGroup::Strings,
            HashGroup::Other => sdk::murmur::HashGroup::Other,
        }
    }
}

pub(crate) fn command_definition() -> Command {
    Command::new("dictionary")
        .about("Manipulate a hash dictionary file.")
        .subcommand(
            Command::new("lookup")
                .about("Lookup a hash in the dictionary.")
                .arg(Arg::new("hash").help("The hash to look up").required(true))
                .arg(
                    Arg::new("group")
                        .help(
                            "Check each group for a match. \
                            If no group is specified, all groups are checked.",
                        )
                        .short('g')
                        .long("group")
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(HashGroup)),
                ),
        )
        .subcommand(
            Command::new("add")
                .about(
                    "Add strings to the dictionary. \
                        Strings are read line by line from the given file.",
                )
                .arg(
                    Arg::new("group")
                        .help("The dictionary group to put these strings in.")
                        .short('g')
                        .long("group")
                        .value_parser(value_parser!(HashGroup))
                        .default_value("other"),
                )
                .arg(
                    Arg::new("file")
                        .help("Path to a file to read strings from.")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(Command::new("save").about(
            "Save back the currently loaded dictionary, with hashes pre-computed. \
                Pre-computing hashes speeds up loading large dictionaries, as they would \
                otherwise need to be computed on the fly.",
        ))
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(mut ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("lookup", sub_matches)) => {
            let hash = sub_matches
                .get_one::<u64>("hash")
                .expect("required argument not found");

            let groups = sub_matches
                .get_many::<HashGroup>("group")
                .unwrap_or_default();

            for group in groups {
                let value = ctx.lookup_hash(*hash, (*group).into());
                println!("{value}");
            }

            Ok(())
        }
        Some(("add", sub_matches)) => {
            let path = sub_matches
                .get_one::<PathBuf>("file")
                .expect("required argument not found");
            let group = sub_matches
                .get_one::<HashGroup>("group")
                .expect("required argument not found");

            let r: BufReader<Box<dyn tokio::io::AsyncRead + std::marker::Unpin>> = if let Some(name) = path.file_name() && name == "-" {
                let f = tokio::io::stdin();
                BufReader::new(Box::new(f))
            } else {
                let f = File::open(&path).await?;
                BufReader::new(Box::new(f))
            };

            let group = sdk::murmur::HashGroup::from(*group);

            let mut added = 0;
            let mut skipped = 0;

            let lines: Vec<_> = LinesStream::new(r.lines()).collect().await;
            let total = {
                for line in lines.into_iter() {
                    let value = line?;
                    if ctx.lookup.find(&value, group).is_some() {
                        skipped += 1;
                    } else {
                        ctx.lookup.add(value, group);
                        added += 1;
                    }
                }

                ctx.lookup.len()
            };

            let out_path = matches
                .get_one::<PathBuf>("dictionary")
                .expect("no default value for 'dictionary' parameter");
            let f = File::create(out_path)
                .await
                .wrap_err("Failed to open dictionary file")
                .with_suggestion(|| {
                    format!(
                        "Make sure the parent directories of '{}' exist and are writable",
                        out_path.display()
                    )
                })
                .with_section(|| out_path.display().to_string().header("Path:"))?;

            ctx.lookup
                .to_csv(f)
                .await
                .wrap_err("Failed to write dictionary to disk")?;

            tracing::info!(
                "Added {} entries, skipped {} duplicates. Total now {}.",
                added,
                skipped,
                total
            );
            Ok(())
        }
        Some(("save", _)) => {
            let out_path = matches
                .get_one::<PathBuf>("dictionary")
                .expect("no default value for 'dictionary' parameter");
            let f = File::create(out_path)
                .await
                .wrap_err("Failed to open dictionary file")
                .with_suggestion(|| {
                    format!(
                        "Make sure the parent directories of '{}' exist and are writable",
                        out_path.display()
                    )
                })
                .with_section(|| out_path.display().to_string().header("Path:"))?;

            ctx.lookup
                .to_csv(f)
                .await
                .wrap_err("Failed to write dictionary to disk")
        }
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
