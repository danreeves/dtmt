use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use color_eyre::eyre::{Context, Result};
use dtmt_shared::ModConfig;
use notify::{Event, Watcher};

use crate::cmd::build::{build, read_project_config};

use super::package::package;

pub(crate) fn command_definition() -> Command {
    Command::new("watch")
        .about("Watch for file system changes and re-build the mod archive.")
        .arg(
            Arg::new("debounce")
                .long("debounce")
                .short('b')
                .default_value("150")
                .value_parser(value_parser!(u64))
                .help(
                    "The delay to debounce events by. This avoids continously \
                        rebuilding on rapid file changes, such as version control checkouts.",
                ),
        )
        .arg(
            Arg::new("directory")
                .required(false)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the project to build. \
                        If omitted, the current working directory is used.",
                ),
        )
        .arg(
            Arg::new("out")
                .long("out")
                .short('o')
                .default_value("out")
                .value_parser(value_parser!(PathBuf))
                .help("The directory to write output files to."),
        )
        .arg(
            Arg::new("deploy")
                .long("deploy")
                .short('d')
                .value_parser(value_parser!(PathBuf))
                .help(
                    "If the path to the game (without the trailing '/bundle') is specified, \
                        deploy the newly built bundles. \
                        This will not adjust the bundle database or package files, so if files are \
                        added or removed, you will have to import into DTMM and re-deploy there.",
                ),
        )
        .arg(
            Arg::new("archive")
                .long("archive")
                .short('a')
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to write the packaged file to. Will default to a file in the \
                        current working directory",
                ),
        )
        .arg(
            Arg::new("ignore")
                .long("ignore")
                .short('i')
                .value_parser(value_parser!(PathBuf))
                .action(ArgAction::Append)
                .help(
                    "A directory or file path to ignore. May be specified multiple times. \
                        The values of 'out' and 'archive' are ignored automatically.",
                ),
        )
}

#[tracing::instrument]
async fn compile(
    cfg: &ModConfig,
    out_path: impl AsRef<Path> + std::fmt::Debug,
    archive_path: impl AsRef<Path> + std::fmt::Debug,
    game_dir: Arc<Option<impl AsRef<Path> + std::fmt::Debug>>,
) -> Result<()> {
    let out_path = out_path.as_ref();
    build(cfg, out_path, game_dir, false)
        .await
        .wrap_err("Failed to build bundles")?;
    package(cfg, out_path, archive_path)
        .await
        .wrap_err("Failed to package bundles")
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let cfg = read_project_config(matches.get_one::<PathBuf>("directory").cloned())
        .await
        .wrap_err("failed to load project config")?;
    tracing::debug!(?cfg);
    let cfg = Arc::new(cfg);

    let game_dir = matches
        .get_one::<PathBuf>("deploy")
        .map(path_clean::clean)
        .map(|p| if p.is_absolute() { p } else { cfg.dir.join(p) })
        .map(|p| p.join("bundle"));

    let out_path = matches
        .get_one::<PathBuf>("out")
        .map(path_clean::clean)
        .map(|p| if p.is_absolute() { p } else { cfg.dir.join(p) })
        .expect("parameter should have default value");

    let archive_path = matches
        .get_one::<PathBuf>("archive")
        .map(path_clean::clean)
        .map(|p| if p.is_absolute() { p } else { cfg.dir.join(p) })
        .unwrap_or_else(|| cfg.dir.join(format!("{}.zip", cfg.id)));

    let ignored = {
        let mut ignored: Vec<_> = matches
            .get_many::<PathBuf>("ignore")
            .unwrap_or_default()
            .map(path_clean::clean)
            .map(|p| if p.is_absolute() { p } else { cfg.dir.join(p) })
            .collect();

        ignored.push(out_path.clone());
        ignored.push(archive_path.clone());

        ignored
    };

    if tracing::enabled!(tracing::Level::INFO) {
        let list = ignored.iter().fold(String::new(), |mut s, p| {
            s.push_str("\n - ");
            s.push_str(&p.display().to_string());
            s
        });

        tracing::info!("Ignoring:{}", list);
    }

    let game_dir = Arc::new(game_dir);

    let duration =
        Duration::from_millis(matches.get_one::<u64>("debounce").copied().unwrap_or(150));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        let ignored = match &res {
            Ok(evt) => evt.paths.iter().any(|p1| {
                let p1 = path_clean::clean(p1);
                ignored.iter().any(|p2| p1.starts_with(p2))
            }),
            Err(_) => false,
        };

        tracing::trace!(?res, ignored, "Received file system event");

        if !ignored && let Err(err) = tx.send(res) {
            tracing::error!("Failed to send file system event: {:?}", err);
        }
    })
    .wrap_err("failed to create file system watcher")?;

    tracing::info!("Starting file watcher on '{}'", cfg.dir.display());

    let path = cfg.dir.clone();
    watcher
        .watch(&path, notify::RecursiveMode::Recursive)
        .wrap_err_with(|| {
            format!(
                "failed to watch directory for file changes: {}",
                path.display()
            )
        })?;

    tracing::trace!("Starting debounce loop");

    let mut dirty = false;
    loop {
        // While we could just always await on the timeout, splitting things like this
        // optimizes the case when no events happen for a while. Rather than being woken every
        // `duration` just to do nothing, this way we always wait for a new event first until
        // we start the debounce timeouts.
        if dirty {
            match tokio::time::timeout(duration, rx.recv()).await {
                // The error is the wanted case, as it signals that we haven't received an
                // event within `duration`, which es what the debounce is supposed to wait for.
                Err(_) => {
                    tracing::trace!("Received debounce timeout, running build");
                    if let Err(err) =
                        compile(&cfg, &out_path, &archive_path, game_dir.clone()).await
                    {
                        tracing::error!("Failed to build mod archive: {:?}", err);
                    }
                    dirty = false;
                }
                Ok(None) => {
                    break;
                }
                // We received a value before the timeout, so we reset it
                Ok(_) => {
                    tracing::trace!("Received value before timeout, resetting");
                }
            }
        } else {
            match rx.recv().await {
                Some(_) => {
                    tracing::trace!("Received event, starting debounce");
                    dirty = true;
                }
                None => {
                    break;
                }
            }
        }
    }

    tracing::trace!("Event channel closed");
    if let Err(err) = compile(&cfg, &out_path, &archive_path, game_dir.clone()).await {
        tracing::error!("Failed to build mod archive: {:?}", err);
    }

    Ok(())
}
