use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report};
use futures::future::try_join_all;
use futures::StreamExt;
use glob::Pattern;
use sdk::{Bundle, BundleFile, CmdLine};
use tokio::fs;

use crate::cmd::util::resolve_bundle_paths;

fn parse_glob_pattern(s: &str) -> Result<Pattern, String> {
    match Pattern::new(s) {
        Ok(p) => Ok(p),
        Err(e) => Err(format!("Invalid glob pattern '{s}': {e}")),
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
                    "A custom command line to execute ljd with. It is treated as follows:\n\
                        * if the argument is a valid path to an existing file:\n\
                        ** if the file is called 'main.py', it is assumed that 'python.exe' \
                        exists in PATH to execute this with.\n\
                        ** otherwise it is treated as an executable\n\
                        * if it's a single word, it's treated as an executable in PATH\n\
                        * otherwise it is treated as a command line template.\n\
                        In any case, the application being run must accept ljd's flags '-c' and '-f'.",
                )
                .default_value("ljd"),
        )
    // .arg(
    //     Arg::new("revorb")
    //         .long("revorb")
    //         .help(
    //             "Path to a custom revorb executable. If not set, \
    //                 `revorb` will be called from PATH.",
    //         )
    //         .default_value("revorb"),
    // )
    // .arg(
    //     Arg::new("ww2ogg")
    //         .long("ww2ogg")
    //         .help(
    //             "Path to a custom ww2ogg executable. If not set, \
    //                 `ww2ogg` will be called from PATH.\nSee the documentation for how \
    //                 to set up the script for this.",
    //         )
    //         .default_value("ww2ogg"),
    // )
}

#[tracing::instrument]
async fn parse_command_line_template(tmpl: &String) -> Result<CmdLine> {
    if tmpl.trim().is_empty() {
        eyre::bail!("Command line template must not be empty");
    }

    let mut cmd = if matches!(fs::try_exists(tmpl).await, Ok(true)) {
        let path = PathBuf::from(tmpl);
        if path.file_name() == Some(OsStr::new("main.py")) {
            let arg = path.display().to_string();
            let mut cmd = CmdLine::new("python");
            cmd.arg(shlex::quote(&arg).to_string());
            cmd
        } else {
            CmdLine::new(path)
        }
    } else {
        let Some(args) = shlex::split(tmpl) else {
            eyre::bail!("Invalid shell syntax");
        };

        // We already checked that the template is not empty
        let mut cmd = CmdLine::new(args[0].clone());
        let mut it = args.iter();
        // Skip the first one, that's the command name
        it.next();

        for arg in it {
            cmd.arg(arg);
        }

        cmd
    };

    // Add ljd flags
    cmd.arg("-c");

    tracing::debug!("Parsed command line template: {:?}", cmd);

    Ok(cmd)
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(mut ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    {
        let ljd_bin = matches
            .get_one::<String>("ljd")
            .expect("no default value for 'ljd' parameter");
        // let revorb_bin = matches
        //     .get_one::<String>("revorb")
        //     .expect("no default value for 'revorb' parameter");
        // let ww2ogg_bin = matches
        //     .get_one::<String>("ww2ogg")
        //     .expect("no default value for 'ww2ogg' parameter");

        ctx.ljd = parse_command_line_template(ljd_bin)
            .await
            .map(Option::Some)
            .wrap_err("Failed to parse command line template for flag 'ljd'")?;
        // ctx.revorb = Some(revorb_bin.clone());
        // ctx.ww2ogg = Some(ww2ogg_bin.clone());
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
                    .with_suggestion(|| format!("Create the directory '{}'", dest.display()))
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

    let includes = Arc::new(includes);
    let excludes = Arc::new(excludes);
    let ctx = Arc::new(ctx);

    resolve_bundle_paths(bundles)
        .for_each_concurrent(10, |p| async {
            let includes = includes.clone();
            let excludes = excludes.clone();
            let ctx = ctx.clone();

            let options = ExtractOptions {
                includes,
                excludes,
                decompile: should_decompile,
                flatten: should_flatten,
                dry_run: is_dry_run,
            };

            async move {
                match extract_bundle(ctx, &p, &dest, options).await {
                    Ok(_) => {}
                    Err(err) => tracing::error!("{err:?}"),
                }
            }
            .await
        })
        .await;

    Ok(())
}

#[derive(Clone)]
struct ExtractOptions<'a> {
    decompile: bool,
    flatten: bool,
    dry_run: bool,
    includes: Arc<Vec<&'a Pattern>>,
    excludes: Arc<Vec<&'a Pattern>>,
}

#[tracing::instrument(
    skip(ctx, options),
    fields(decompile = options.decompile, flatten = options.flatten, dry_run = options.dry_run)
)]
async fn extract_bundle<P1, P2>(
    ctx: Arc<sdk::Context>,
    path: P1,
    dest: P2,
    options: ExtractOptions<'_>,
) -> Result<()>
where
    P1: AsRef<Path> + std::fmt::Debug,
    P2: AsRef<Path> + std::fmt::Debug,
{
    let bundle = {
        let data = fs::read(path.as_ref()).await?;
        let name = Bundle::get_name_from_path(&ctx, path.as_ref());
        Bundle::from_binary(&ctx, name, data)?
    };

    let includes = options.includes.as_ref();
    let excludes = options.excludes.as_ref();
    let dest = dest.as_ref();

    let files: Box<dyn Iterator<Item = &BundleFile>> = {
        if includes.is_empty() && excludes.is_empty() {
            Box::new(bundle.files().iter())
        } else {
            let iter = bundle.files().iter().filter(|file| {
                let name = file.name(false, None);
                let decompiled_name = file.name(true, None);

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
            });
            Box::new(iter)
        }
    };

    // TODO: Disabled for now, as the `files` iterator would be consumed.
    // if tracing::enabled!(tracing::Level::DEBUG) {
    //     let includes: Vec<_> = includes.iter().map(|pattern| pattern.as_str()).collect();
    //     let excludes: Vec<_> = excludes.iter().map(|pattern| pattern.as_str()).collect();
    //     let bundle_files: Vec<_> = bundle.files().iter().map(|file| file.name(false)).collect();
    //     let filtered: Vec<_> = files.map(|file| file.name(false)).collect();
    //     tracing::debug!(
    //         ?includes,
    //         ?excludes,
    //         files = ?bundle_files,
    //         ?filtered,
    //         "Built file list to extract"
    //     );
    // }

    let mut tasks = Vec::with_capacity(bundle.files().len());

    for file in files {
        let name = file.name(options.decompile, None);
        let data = if options.decompile {
            file.decompiled(&ctx).await
        } else {
            file.raw()
        };

        match data {
            Ok(mut files) => {
                match files.len() {
                    0 => {
                        tracing::warn!("Decompilation did not produce any data for file {}", name);
                    }
                    // For a single file we want to use the bundle file's name.
                    1 => {
                        // We already checked `files.len()`.
                        let file = files.pop().unwrap();

                        let name = file.name().unwrap_or(&name);
                        let name = if options.flatten {
                            flatten_name(name)
                        } else {
                            name.clone()
                        };

                        let mut path = dest.to_path_buf();
                        path.push(name);

                        if options.dry_run {
                            tracing::info!("Dry Run: Writing file '{}'", path.display());
                        } else {
                            tracing::info!("Writing file '{}'", path.display());
                            tasks.push(tokio::spawn(async move {
                                if let Some(parent) = path.parent() {
                                    fs::create_dir_all(&parent).await.wrap_err_with(|| {
                                        format!(
                                            "failed to create parent directories '{}'",
                                            parent.display()
                                        )
                                    })?;
                                }

                                fs::write(&path, file.data()).await.wrap_err_with(|| {
                                    format!(
                                        "failed to write extracted file to disc: '{}'",
                                        path.display()
                                    )
                                })
                            }));
                        }
                    }
                    // For multiple files we create a directory and name files
                    // by index.
                    _ => {
                        for (i, file) in files.into_iter().enumerate() {
                            let mut path = dest.to_path_buf();

                            let name = file
                                .name()
                                .map(|name| {
                                    if options.flatten {
                                        flatten_name(name)
                                    } else {
                                        name.clone()
                                    }
                                })
                                .unwrap_or(format!("{i}"));

                            path.push(name);

                            if options.dry_run {
                                tracing::info!("Dry Run: Writing file '{}'", path.display());
                            } else {
                                tracing::info!("Writing file '{}'", path.display());
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

                                    fs::create_dir_all(parent).await.wrap_err_with(|| {
                                        format!(
                                            "failed to create parent directory: '{}'",
                                            parent.display()
                                        )
                                    })?;

                                    fs::write(&path, file.data()).await.wrap_err_with(|| {
                                        format!(
                                            "failed to write extracted file to disc: '{}'",
                                            path.display()
                                        )
                                    })
                                }));
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let err = err.wrap_err(format!("Failed to decompile file {}", name));
                tracing::error!("{:?}", err);
            }
        };
    }

    // TODO: Check if this might need buffered execution to avoid
    // running out of file handles.
    let results = try_join_all(tasks).await?;

    for res in results {
        if let Err(err) = res {
            tracing::error!("{:#}", err);
        }
    }

    Ok(())
}
