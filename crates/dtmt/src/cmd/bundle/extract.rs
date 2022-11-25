use std::path::PathBuf;
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report, SectionExt};
use futures::future::try_join_all;
use glob::Pattern;
use sdk::Bundle;
use tokio::{fs, sync::RwLock};

use crate::cmd::util::collect_bundle_paths;

fn parse_glob_pattern(s: &str) -> Result<Pattern, String> {
    match Pattern::new(s) {
        Ok(p) => Ok(p),
        Err(e) => Err(format!("Invalid glob pattern '{}': {}", s, e)),
    }
}

fn flatten_name(s: &str) -> String {
    s.replace('/', "_")
}

pub(crate) fn command_definition() -> Command {
    Command::new("extract")
        .about("Extract files from the bundle(s).")
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
                .help("Directory to extract files to."),
        )
        .arg(
            Arg::new("include")
                .long("include")
                .short('i')
                .action(ArgAction::Append)
                .value_parser(parse_glob_pattern)
                .help("Only extract files that match the given glob pattern(s)."),
        )
        .arg(
            Arg::new("exclude")
                .long("exclude")
                .short('e')
                .action(ArgAction::Append)
                .value_parser(parse_glob_pattern)
                .help(
                    "Do not extract files that match the given glob pattern(s).\n\
                        This takes precedence over `include`.",
                ),
        )
        .arg(
            Arg::new("flatten")
                .long("flatten")
                .short('f')
                .action(ArgAction::SetTrue)
                .help("Flatten the paths of extracted files into the file name."),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .short('n')
                .action(ArgAction::SetTrue)
                .help("Simulate write operations and log what would have been done."),
        )
        .arg(
            Arg::new("decompile")
                .long("decompile")
                .short('d')
                .action(ArgAction::SetTrue)
                .help(
                    "Attempt to decompile files after extracting them. Not all file types \
                        are supported for this.",
                ),
        )
        .arg(
            Arg::new("ljd")
                .long("ljd")
                .help(
                    "Path to a custom ljd executable. If not set, \
                        `ljd` will be called from PATH.",
                )
                .default_value("ljd"),
        )
        .arg(
            Arg::new("revorb")
                .long("revorb")
                .help(
                    "Path to a custom revorb executable. If not set, \
                        `revorb` will be called from PATH.",
                )
                .default_value("revorb"),
        )
        .arg(
            Arg::new("ww2ogg")
                .long("ww2ogg")
                .help(
                    "Path to a custom ww2ogg executable. If not set, \
                        `ww2ogg` will be called from PATH.\nSee the documentation for how \
                        to set up the script for this.",
                )
                .default_value("ww2ogg"),
        )
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(ctx: Arc<RwLock<sdk::Context>>, matches: &ArgMatches) -> Result<()> {
    {
        let ljd_bin = matches
            .get_one::<String>("ljd")
            .expect("no default value for 'ljd' parameter");
        let revorb_bin = matches
            .get_one::<String>("revorb")
            .expect("no default value for 'revorb' parameter");
        let ww2ogg_bin = matches
            .get_one::<String>("ww2ogg")
            .expect("no default value for 'ww2ogg' parameter");

        let mut ctx = ctx.write().await;
        ctx.ljd = Some(ljd_bin.clone());
        ctx.revorb = Some(revorb_bin.clone());
        ctx.ww2ogg = Some(ww2ogg_bin.clone());
    }

    let includes = match matches.get_many::<Pattern>("include") {
        Some(values) => values.collect(),
        None => Vec::new(),
    };

    let excludes = match matches.get_many::<Pattern>("exclude") {
        Some(values) => values.collect(),
        None => Vec::new(),
    };

    let bundles = matches
        .get_many::<PathBuf>("bundle")
        .unwrap_or_default()
        .cloned();

    let paths = collect_bundle_paths(bundles).await;

    if paths.is_empty() {
        return Err(eyre::eyre!("No bundle provided"));
    }

    let bundles = try_join_all(paths.into_iter().map(|p| async {
        let ctx = ctx.clone();
        let path_display = p.display().to_string();
        async move { Bundle::open(ctx, &p).await }
            .await
            .with_section(|| path_display.header("Bundle Path:"))
    }))
    .await?;

    let files: Vec<_> = {
        let iter = bundles.iter().flat_map(|bundle| bundle.files());

        // Short-curcit the iteration if there is nothing to filter by
        if includes.is_empty() && excludes.is_empty() {
            iter.collect()
        } else {
            iter.filter(|file| {
                let name = file.name(false);
                let decompiled_name = file.name(true);

                // When there is no `includes`, all files are included
                let is_included = includes.is_empty()
                    || includes
                        .iter()
                        .any(|glob| glob.matches(&name) || glob.matches(&decompiled_name));
                // When there is no `excludes`, no file is excluded
                let is_excluded = !excludes.is_empty()
                    && excludes
                        .iter()
                        .any(|glob| glob.matches(&name) || glob.matches(&decompiled_name));

                is_included && !is_excluded
            })
            .collect()
        }
    };

    if tracing::enabled!(tracing::Level::DEBUG) {
        let includes: Vec<_> = includes.iter().map(|pattern| pattern.as_str()).collect();
        let excludes: Vec<_> = excludes.iter().map(|pattern| pattern.as_str()).collect();
        let bundle_files: Vec<_> = bundles
            .iter()
            .flat_map(|bundle| bundle.files())
            .map(|file| file.name(false))
            .collect();
        let filtered: Vec<_> = files.iter().map(|file| file.name(false)).collect();
        tracing::debug!(
            ?includes,
            ?excludes,
            files = ?bundle_files,
            ?filtered,
            "Built file list to extract"
        );
    }

    let should_decompile = matches.get_flag("decompile");
    let should_flatten = matches.get_flag("flatten");
    let is_dry_run = matches.get_flag("dry-run");

    let dest = matches
        .get_one::<PathBuf>("destination")
        .expect("required argument 'destination' missing");

    {
        let res = match fs::metadata(&dest).await {
            Ok(meta) if !meta.is_dir() => Err(eyre::eyre!("Destination path is not a directory")),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Err(eyre::eyre!("Destination path does not exist"))
                    .with_suggestion(|| "Create the directory")
            }
            Err(err) => Err(Report::new(err)),
            _ => Ok(()),
        };

        if res.is_err() {
            return res.wrap_err(format!(
                "Failed to open destination directory: {}",
                dest.display()
            ));
        }
    }

    let mut tasks = Vec::with_capacity(files.len());

    for file in files {
        let name = file.name(should_decompile);
        let data = if should_decompile {
            file.decompiled(ctx.clone()).await
        } else {
            file.raw()
        };

        match data {
            Ok(mut files) => {
                match files.len() {
                    0 => {
                        println!(
                            "Decompilation did not produce any data for file {}",
                            file.name(should_decompile)
                        );
                    }
                    // For a single file we want to use the bundle file's name.
                    1 => {
                        // We already checked `files.len()`.
                        let file = files.pop().unwrap();

                        let name = file.name().unwrap_or(&name);
                        let name = if should_flatten {
                            flatten_name(name)
                        } else {
                            name.clone()
                        };

                        let mut path = dest.clone();
                        path.push(name);

                        if is_dry_run {
                            tracing::info!(path = %path.display(), "Writing file");
                        } else {
                            tracing::debug!(path = %path.display(), "Writing file");
                            tasks.push(tokio::spawn(async move {
                                fs::write(&path, file.data())
                                    .await
                                    .wrap_err("failed to write extracted file to disc")
                                    .with_section(|| path.display().to_string().header("Path"))
                            }));
                        }
                    }
                    // For multiple files we create a directory and name files
                    // by index.
                    _ => {
                        for (i, file) in files.into_iter().enumerate() {
                            let mut path = dest.clone();

                            let name = file
                                .name()
                                .map(|name| {
                                    if should_flatten {
                                        flatten_name(name)
                                    } else {
                                        name.clone()
                                    }
                                })
                                .unwrap_or(format!("{}", i));

                            path.push(name);

                            if is_dry_run {
                                tracing::info!(path = %path.display(), "Writing file");
                            } else {
                                tracing::debug!(path = %path.display(), "Writing file");
                                tasks.push(tokio::spawn(async move {
                                    let parent = match path.parent() {
                                        Some(parent) => parent,
                                        None => {
                                            eyre::bail!(
                                                "Decompilation produced invalid path: {}",
                                                &path.display()
                                            )
                                        }
                                    };

                                    fs::create_dir_all(parent)
                                        .await
                                        .wrap_err("failed to create parent directory")
                                        .with_section(|| {
                                            parent.display().to_string().header("Path")
                                        })?;

                                    fs::write(&path, file.data())
                                        .await
                                        .wrap_err("failed to write extracted file to disc")
                                        .with_section(|| path.display().to_string().header("Path"))
                                }));
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let err = err
                    .wrap_err("Failed to decompile")
                    .with_section(|| name.header("File"));

                tracing::error!("{:#}", err);
            }
        };
    }

    let results = try_join_all(tasks).await?;

    for res in results {
        if let Err(err) = res {
            tracing::error!("{:#}", err);
        }
    }

    Ok(())
}
