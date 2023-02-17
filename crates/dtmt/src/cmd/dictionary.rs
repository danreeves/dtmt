use std::path::PathBuf;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command, ValueEnum};
use cli_table::{print_stdout, WithTitle};
use color_eyre::eyre::{Context, Result};
use color_eyre::{Help, SectionExt};
use sdk::murmur::{IdString64, Murmur32, Murmur64};
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

#[derive(cli_table::Table)]
struct TableRow {
    #[table(title = "Value")]
    value: String,
    #[table(title = "Murmur64")]
    long: Murmur64,
    #[table(title = "Murmur32")]
    short: Murmur32,
    #[table(title = "Group")]
    group: sdk::murmur::HashGroup,
}

impl From<&sdk::murmur::Entry> for TableRow {
    fn from(entry: &sdk::murmur::Entry) -> Self {
        Self {
            value: entry.value().clone(),
            long: entry.long(),
            short: entry.short(),
            group: entry.group(),
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
        .subcommand(Command::new("show").about("Show the contents of the dictionary"))
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
            let hash = {
                let s = sub_matches
                    .get_one::<String>("hash")
                    .expect("required argument not found");

                u64::from_str_radix(s, 16)
                    .wrap_err("failed to parse argument as hexadecimal string")?
            };

            let groups = sub_matches
                .get_many::<HashGroup>("group")
                .unwrap_or_default();

            for group in groups {
                if let IdString64::String(value) = ctx.lookup_hash(hash, (*group).into()) {
                    println!("{group}: {value}");
                }
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
        Some(("show", _)) => {
            let lookup = &ctx.lookup;
            let rows: Vec<_> = lookup.entries().iter().map(TableRow::from).collect();

            print_stdout(rows.with_title())?;

            Ok(())
        }
        _ => unreachable!(
            "clap is configured to require a subcommand, and they're all handled above"
        ),
    }
}
