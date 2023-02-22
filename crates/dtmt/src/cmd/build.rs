use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{value_parser, Arg, ArgMatches, Command};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report};
use futures::future::try_join_all;
use futures::StreamExt;
use sdk::filetype::package::Package;
use sdk::{Bundle, BundleFile, ModConfig};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;

use crate::mods::archive::Archive;

const PROJECT_CONFIG_NAME: &str = "dtmt.cfg";

pub(crate) fn command_definition() -> Command {
    Command::new("build")
        .about("Build a project")
        .arg(
            Arg::new("directory")
                .required(false)
                .value_parser(value_parser!(PathBuf))
                .help(
                    "The path to the project to build. \
                        If omitted, dtmt will search from the current working directory upward.",
                ),
        )
        .arg(Arg::new("oodle").long("oodle").help(
            "The oodle library to load. This may either be:\n\
                - A library name that will be searched for in the system's default paths.\n\
                - A file path relative to the current working directory.\n\
                - An absolute file path.",
        ))
}

#[tracing::instrument]
async fn find_project_config(dir: Option<PathBuf>) -> Result<ModConfig> {
    let (path, mut file) = if let Some(path) = dir {
        let file = File::open(&path.join(PROJECT_CONFIG_NAME))
            .await
            .wrap_err_with(|| format!("failed to open file: {}", path.display()))
            .with_suggestion(|| {
                format!(
                    "Make sure the file at '{}' exists and is readable",
                    path.display()
                )
            })?;
        (path, file)
    } else {
        let mut dir = std::env::current_dir()?;
        loop {
            let path = dir.join(PROJECT_CONFIG_NAME);
            match File::open(&path).await {
                Ok(file) => break (dir, file),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if let Some(parent) = dir.parent() {
                        // TODO: Re-write with recursion to avoid allocating the `PathBuf`.
                        dir = parent.to_path_buf();
                    } else {
                        eyre::bail!("Could not find project root");
                    }
                }
                Err(err) => {
                    let err = Report::new(err)
                        .wrap_err(format!("failed to open file: {}", path.display()));
                    return Err(err);
                }
            }
        }
    };

    let mut buf = String::new();
    file.read_to_string(&mut buf).await?;

    let mut cfg: ModConfig = serde_sjson::from_str(&buf)?;
    cfg.dir = path;
    Ok(cfg)
}

#[tracing::instrument(skip_all)]
async fn compile_package_files<P>(pkg: &Package, root: P) -> Result<Vec<BundleFile>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let root = Arc::new(root.as_ref());

    let tasks = pkg
        .iter()
        .flat_map(|(file_type, paths)| {
            paths.iter().map(|path| {
                (
                    *file_type,
                    path,
                    // Cloning the `Arc` here solves the issue that in the next `.map`, I need to
                    // `move` the closure parameters, but can't `move` `root` before it was cloned.
                    root.clone(),
                )
            })
        })
        .map(|(file_type, path, root)| async move {
            let sjson = fs::read_to_string(&path).await?;

            let mut path = path.clone();
            path.set_extension("");

            BundleFile::from_sjson(
                path.to_string_lossy().to_string(),
                file_type,
                sjson,
                root.as_ref(),
            )
            .await
        });

    let results = futures::stream::iter(tasks)
        .buffer_unordered(10)
        .collect::<Vec<Result<BundleFile>>>()
        .await;

    results.into_iter().collect()
}

#[tracing::instrument(skip_all, fields(files = files.len()))]
fn compile_bundle(name: String, files: Vec<BundleFile>) -> Result<Bundle> {
    let mut bundle = Bundle::new(name);

    for file in files {
        bundle.add_file(file);
    }

    Ok(bundle)
}

#[tracing::instrument]
async fn build_package<P1, P2>(package: P1, root: P2) -> Result<Bundle>
where
    P1: AsRef<Path> + std::fmt::Debug,
    P2: AsRef<Path> + std::fmt::Debug,
{
    let root = root.as_ref();
    let package = package.as_ref();

    let mut path = root.join(package);
    path.set_extension("package");
    let sjson = fs::read_to_string(&path)
        .await
        .wrap_err_with(|| format!("failed to read file {}", path.display()))?;

    let pkg_name = package.to_string_lossy().to_string();
    let pkg = Package::from_sjson(sjson, pkg_name.clone(), root)
        .await
        .wrap_err_with(|| format!("invalid package file {}", &pkg_name))?;

    compile_package_files(&pkg, root)
        .await
        .wrap_err("failed to compile package")
        .and_then(|files| compile_bundle(pkg_name, files))
        .wrap_err("failed to build bundle")
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    unsafe {
        oodle_sys::init(matches.get_one::<String>("oodle"));
    }

    let cfg = {
        let dir = matches.get_one::<PathBuf>("directory").cloned();
        find_project_config(dir).await?
    };

    let dest = {
        let mut path = PathBuf::from(&cfg.id);
        path.set_extension("zip");
        Arc::new(path)
    };
    let cfg = Arc::new(cfg);

    tracing::debug!(?cfg);

    let tasks = cfg
        .packages
        .iter()
        .map(|path| (path, cfg.clone()))
        .map(|(path, cfg)| async move {
            if path.extension().is_some() {
                eyre::bail!(
                    "Package name must be specified without file extension: {}",
                    path.display()
                );
            }

            build_package(path, &cfg.dir).await.wrap_err_with(|| {
                format!(
                    "failed to build package {} in {}",
                    path.display(),
                    cfg.dir.display()
                )
            })
        });

    let bundles = try_join_all(tasks)
        .await
        .wrap_err("failed to build mod bundles")?;

    let config_file = {
        let path = cfg.dir.join("dtmt.cfg");
        fs::read(&path)
            .await
            .wrap_err_with(|| format!("failed to read mod config at {}", path.display()))?
    };

    {
        let dest = dest.clone();
        let id = cfg.id.clone();
        tokio::task::spawn_blocking(move || {
            let mut archive = Archive::new(id);

            archive.add_config(config_file);

            for bundle in bundles {
                archive.add_bundle(bundle);
            }

            archive
                .write(dest.as_ref())
                .wrap_err("failed to write mod archive")
        })
        .await??;
    }

    tracing::info!("Mod archive written to {}", dest.display());
    Ok(())
}
