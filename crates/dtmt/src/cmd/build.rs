use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Arg, ArgMatches, Command, value_parser};
use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report};
use dtmt_shared::ModConfig;
use futures::StreamExt;
use futures::future::try_join_all;
use path_slash::PathExt;
use sdk::filetype::package::Package;
use sdk::murmur::IdString64;
use sdk::{Bundle, BundleFile};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

const PROJECT_CONFIG_NAME: &str = "dtmt.cfg";

type FileIndexMap = HashMap<String, HashSet<String>>;

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
}

/// Try to find a `dtmt.cfg` in the given directory or traverse up the parents.
#[tracing::instrument]
async fn find_project_config(dir: Option<PathBuf>) -> Result<ModConfig> {
    let (path, mut file) = if let Some(path) = dir {
        let file = File::open(&path.join(PROJECT_CONFIG_NAME))
            .await
            .wrap_err_with(|| format!("Failed to open file: {}", path.display()))
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
                        .wrap_err(format!("Failed to open file: {}", path.display()));
                    return Err(err);
                }
            }
        }
    };

    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .await
        .wrap_err("Invalid UTF-8")?;

    let mut cfg: ModConfig =
        serde_sjson::from_str(&buf).wrap_err("Failed to deserialize mod config")?;
    cfg.dir = path;
    Ok(cfg)
}

/// Iterate over the paths in the given `Package` and
/// compile each file by its file type.
#[tracing::instrument(skip_all)]
async fn compile_package_files(pkg: &Package, cfg: &ModConfig) -> Result<Vec<BundleFile>> {
    let root = Arc::new(&cfg.dir);
    let name_overrides = &cfg.name_overrides;

    let tasks = pkg
        .iter()
        .flat_map(|(file_type, names)| {
            names.iter().map(|name| {
                (
                    *file_type,
                    name,
                    // Cloning the `Arc` here solves the issue that in the next `.map`, I need to
                    // `move` the closure parameters, but can't `move` `root` before it was cloned.
                    root.clone(),
                )
            })
        })
        .map(|(file_type, name, root)| async move {
            let path = PathBuf::from(name);
            let sjson = fs::read_to_string(&path)
                .await
                .wrap_err_with(|| format!("Failed to read file '{}'", path.display()))?;

            let name = path.with_extension("").to_slash_lossy().to_string();
            let name = if let Some(new_name) = name_overrides.get(&name) {
                let new_name = match u64::from_str_radix(new_name, 16) {
                    Ok(hash) => IdString64::from(hash),
                    Err(_) => IdString64::from(new_name.clone()),
                };
                tracing::info!("Overriding '{}' -> '{}'", name, new_name.display());
                new_name
            } else {
                IdString64::from(name.clone())
            };
            BundleFile::from_sjson(name, file_type, sjson, root.as_ref()).await
        });

    let results = futures::stream::iter(tasks)
        .buffer_unordered(10)
        .collect::<Vec<Result<BundleFile>>>()
        .await;

    results.into_iter().collect()
}

/// Read a `.package` file, collect the referenced files
/// and compile all of them into a bundle.
#[tracing::instrument]
async fn build_package(
    cfg: &ModConfig,
    package: impl AsRef<Path> + std::fmt::Debug,
) -> Result<Bundle> {
    let root = &cfg.dir;
    let package = package.as_ref();

    let mut path = root.join(package);
    path.set_extension("package");
    let sjson = fs::read_to_string(&path)
        .await
        .wrap_err_with(|| format!("Failed to read file {}", path.display()))?;

    let pkg_name = package.to_slash_lossy().to_string();
    let pkg = Package::from_sjson(sjson, pkg_name.clone(), root)
        .await
        .wrap_err_with(|| format!("Invalid package file {}", &pkg_name))?;

    let files = compile_package_files(&pkg, cfg).await?;
    let mut bundle = Bundle::new(pkg_name);
    for file in files {
        bundle.add_file(file);
    }

    Ok(bundle)
}

/// Writes any external data files referenced by the bundle's variants, such as
/// streamed texture mipmaps, relative to the given base directory.
#[tracing::instrument(skip_all, fields(base = %base.as_ref().display()))]
async fn write_external_data_files(
    bundle: &Bundle,
    base: impl AsRef<Path> + std::fmt::Debug,
) -> Result<()> {
    let base = base.as_ref();

    for file in bundle.files() {
        for variant in file.variants() {
            let (Some(name), Some(data)) = (variant.data_file_name(), variant.external_data())
            else {
                continue;
            };

            let path = base.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .wrap_err_with(|| format!("Failed to create '{}'", parent.display()))?;
            }

            tracing::trace!(path = %path.display(), "Writing external data file");
            fs::write(&path, data)
                .await
                .wrap_err_with(|| format!("Failed to write '{}'", path.display()))?;
        }
    }

    Ok(())
}

/// Cleans the path of internal parent (`../`) or self (`./`) components,
/// and ensures that it is relative.
fn normalize_file_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path.as_ref();

    if path.is_absolute() || path.has_root() {
        let err = eyre::eyre!("Path is absolute: {}", path.display());
        return Err(err).with_suggestion(|| "Specify a relative file path.".to_string());
    }

    let path = path_clean::clean(path);

    if path.starts_with("..") {
        eyre::bail!("path starts with a parent component: {}", path.display());
    }

    Ok(path)
}

#[tracing::instrument]
pub(crate) async fn read_project_config(dir: Option<PathBuf>) -> Result<ModConfig> {
    let mut cfg = find_project_config(dir).await?;

    if let Some(path) = cfg.image {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'image'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                    directory where 'dtmt.cfg' is."
                    .to_string()
            })?;
        cfg.image = Some(path);
    }

    cfg.resources.init = normalize_file_path(cfg.resources.init)
        .wrap_err("Invalid config field 'resources.init'")
        .with_suggestion(|| {
            "Specify a file path relative to and child path of the \
                    directory where 'dtmt.cfg' is."
                .to_string()
        })
        .with_suggestion(|| {
            "Use 'dtmt new' in a separate directory to generate \
                    a valid mod template."
                .to_string()
        })?;

    if let Some(path) = cfg.resources.data {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'resources.data'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                            directory where 'dtmt.cfg' is."
                    .to_string()
            })
            .with_suggestion(|| {
                "Use 'dtmt new' in a separate directory to generate \
                            a valid mod template."
                    .to_string()
            })?;
        cfg.resources.data = Some(path);
    }

    if let Some(path) = cfg.resources.localization {
        let path = normalize_file_path(path)
            .wrap_err("Invalid config field 'resources.localization'")
            .with_suggestion(|| {
                "Specify a file path relative to and child path of the \
                        directory where 'dtmt.cfg' is."
                    .to_string()
            })
            .with_suggestion(|| {
                "Use 'dtmt new' in a separate directory to generate \
                        a valid mod template."
                    .to_string()
            })?;
        cfg.resources.localization = Some(path);
    }

    Ok(cfg)
}

#[tracing::instrument]
pub(crate) async fn build<P>(
    cfg: &ModConfig,
    out_path: impl AsRef<Path> + std::fmt::Debug,
    game_dir: Arc<Option<P>>,
) -> Result<()>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let out_path = out_path.as_ref();

    fs::create_dir_all(out_path)
        .await
        .wrap_err_with(|| format!("Failed to create output directory '{}'", out_path.display()))?;

    let file_map = Arc::new(Mutex::new(FileIndexMap::new()));

    let tasks = cfg
        .packages
        .iter()
        // The closure below would capture the `Arc`s before they could be cloned,
        // so instead we need to clone them in a non-move block and inject them
        // via parameters.
        .map(|path| (path, cfg.clone(), file_map.clone(), game_dir.clone()))
        .map(|(path, cfg, file_map, game_dir)| async move {
            if path.extension().is_some() {
                eyre::bail!(
                    "Package name must be specified without file extension: {}",
                    path.display()
                );
            }

            let bundle = build_package(&cfg, path).await.wrap_err_with(|| {
                format!(
                    "Failed to build package '{}' at '{}'",
                    path.display(),
                    cfg.dir.display()
                )
            })?;

            let bundle_name = match bundle.name() {
                IdString64::Hash(_) => {
                    eyre::bail!("bundle name must be known as string. got hash")
                }
                IdString64::String(s) => s.clone(),
            };

            {
                let mut file_map = file_map.lock().await;
                let map_entry = file_map.entry(bundle_name).or_default();

                for file in bundle.files() {
                    map_entry.insert(file.name(false, None));
                }
            }

            let name = bundle.name().to_murmur64().to_string().to_ascii_lowercase();
            let path = out_path.join(&name);
            let data = bundle.to_binary()?;

            tracing::trace!(
                "Writing bundle {} to '{}'",
                bundle.name().display(),
                path.display()
            );
            fs::write(&path, &data)
                .await
                .wrap_err_with(|| format!("Failed to write bundle to '{}'", path.display()))?;

            write_external_data_files(&bundle, out_path).await?;

            if let Some(game_dir) = game_dir.as_ref() {
                let path = game_dir.as_ref().join(&name);

                tracing::trace!(
                    "Deploying bundle {} to '{}'",
                    bundle.name().display(),
                    path.display()
                );
                fs::write(&path, &data)
                    .await
                    .wrap_err_with(|| format!("Failed to write bundle to '{}'", path.display()))?;

                write_external_data_files(&bundle, game_dir.as_ref()).await?;
            }

            Ok(())
        });

    try_join_all(tasks)
        .await
        .wrap_err("Failed to build mod bundles")?;

    {
        let path = out_path.join("files.sjson");
        tracing::trace!(path = %path.display(), "Writing file index");
        let file_map = file_map.lock().await;
        let data = serde_sjson::to_string(file_map.deref())?;
        fs::write(&path, data)
            .await
            .wrap_err_with(|| format!("Failed to write file index to '{}'", path.display()))?;
    }

    if let Some(img_path) = &cfg.image {
        let path = cfg.dir.join(img_path);
        let dest = out_path.join(img_path);

        tracing::trace!(src = %path.display(), dest = %dest.display(), "Copying image file");

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(&parent)
                .await
                .wrap_err_with(|| format!("Failed to create directory '{}'", parent.display()))?;
        }

        fs::copy(&path, &dest).await.wrap_err_with(|| {
            format!(
                "Failed to copy image from '{}' to '{}'",
                path.display(),
                dest.display()
            )
        })?;
    }

    tracing::info!("Compiled bundles written to '{}'", out_path.display());

    if let Some(game_dir) = game_dir.as_ref() {
        tracing::info!("Deployed bundles to '{}'", game_dir.as_ref().display());
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(_ctx: sdk::Context, matches: &ArgMatches) -> Result<()> {
    let cfg = read_project_config(matches.get_one::<PathBuf>("directory").cloned()).await?;

    let game_dir = matches
        .get_one::<PathBuf>("deploy")
        .map(|p| p.join("bundle"));

    let out_path = matches
        .get_one::<PathBuf>("out")
        .expect("parameter should have default value");

    tracing::debug!(?cfg, ?game_dir, ?out_path);

    let game_dir = Arc::new(game_dir);

    build(&cfg, out_path, game_dir).await?;

    Ok(())
}
