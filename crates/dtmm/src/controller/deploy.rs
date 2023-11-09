use std::collections::HashMap;
use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::{eyre, Help, Report, Result};
use futures::StreamExt;
use futures::{stream, TryStreamExt};
use path_slash::PathBufExt;
use sdk::filetype::lua;
use sdk::filetype::package::Package;
use sdk::murmur::Murmur64;
use sdk::{
    Bundle, BundleDatabase, BundleFile, BundleFileType, BundleFileVariant, FromBinary, ToBinary,
};
use serde::{Deserialize, Serialize};
use string_template::Template;
use time::OffsetDateTime;
use tokio::fs::{self, DirEntry};
use tokio::io::AsyncWriteExt;
use tracing::Instrument;

use super::read_sjson_file;
use crate::controller::app::check_mod_order;
use crate::state::{ActionState, PackageInfo};

pub const MOD_BUNDLE_NAME: &str = "packages/mods";
pub const BOOT_BUNDLE_NAME: &str = "packages/boot";
pub const DML_BUNDLE_NAME: &str = "packages/dml";
pub const BUNDLE_DATABASE_NAME: &str = "bundle_database.data";
pub const MOD_BOOT_SCRIPT: &str = "scripts/mod_main";
pub const MOD_DATA_SCRIPT: &str = "scripts/mods/mod_data";
pub const SETTINGS_FILE_PATH: &str = "application_settings/settings_common.ini";
pub const DEPLOYMENT_DATA_PATH: &str = "dtmm-deployment.sjson";

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentData {
    pub bundles: Vec<String>,
    pub mod_folders: Vec<String>,
    #[serde(with = "time::serde::iso8601")]
    pub timestamp: OffsetDateTime,
}

#[tracing::instrument]
async fn read_file_with_backup<P>(path: P) -> Result<Vec<u8>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let path = path.as_ref();
    let backup_path = {
        let mut p = PathBuf::from(path);
        let ext = if let Some(ext) = p.extension() {
            ext.to_string_lossy().to_string() + ".bak"
        } else {
            String::from("bak")
        };
        p.set_extension(ext);
        p
    };

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("file"));

    let bin = match fs::read(&backup_path).await {
        Ok(bin) => bin,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // TODO: This doesn't need to be awaited here, yet.
            // I only need to make sure it has finished before writing the changed bundle.
            tracing::debug!(
                "Backup does not exist. Backing up original {} to '{}'",
                file_name,
                backup_path.display()
            );
            fs::copy(path, &backup_path).await.wrap_err_with(|| {
                format!(
                    "Failed to back up {} '{}' to '{}'",
                    file_name,
                    path.display(),
                    backup_path.display()
                )
            })?;

            tracing::debug!("Reading {} from original '{}'", file_name, path.display());
            fs::read(path).await.wrap_err_with(|| {
                format!("Failed to read {} file: {}", file_name, path.display())
            })?
        }
        Err(err) => {
            return Err(err).wrap_err_with(|| {
                format!(
                    "Failed to read {} from backup '{}'",
                    file_name,
                    backup_path.display()
                )
            });
        }
    };
    Ok(bin)
}

#[tracing::instrument(skip_all)]
async fn patch_game_settings(state: Arc<ActionState>) -> Result<()> {
    let settings_path = state.game_dir.join("bundle").join(SETTINGS_FILE_PATH);

    let settings = read_file_with_backup(&settings_path)
        .await
        .wrap_err("Failed to read settings.ini")?;
    let settings = String::from_utf8(settings).wrap_err("Settings.ini is not valid UTF-8")?;

    let mut f = fs::File::create(&settings_path)
        .await
        .wrap_err_with(|| format!("Failed to open {}", settings_path.display()))?;

    let Some(i) = settings.find("boot_script =") else {
        eyre::bail!("couldn't find 'boot_script' field");
    };

    f.write_all(settings[0..i].as_bytes()).await?;
    f.write_all(b"boot_script = \"scripts/mod_main\"").await?;

    let Some(j) = settings[i..].find('\n') else {
        eyre::bail!("couldn't find end of 'boot_script' field");
    };

    f.write_all(settings[(i + j)..].as_bytes()).await?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(package = info.name))]
fn make_package(info: &PackageInfo) -> Result<Package> {
    let mut pkg = Package::new(info.name.clone(), PathBuf::new());

    for f in &info.files {
        let mut it = f.rsplit('.');
        let file_type = it
            .next()
            .ok_or_else(|| eyre::eyre!("missing file extension"))
            .and_then(BundleFileType::from_str)
            .wrap_err("Invalid file name in package info")?;
        let name: String = it.collect();
        pkg.add_file(file_type, name);
    }

    Ok(pkg)
}

#[tracing::instrument]
async fn copy_recursive(
    from: impl Into<PathBuf> + std::fmt::Debug,
    to: impl AsRef<Path> + std::fmt::Debug,
) -> Result<()> {
    let to = to.as_ref();

    #[tracing::instrument]
    async fn handle_dir(from: PathBuf) -> Result<Vec<(bool, DirEntry)>> {
        let mut dir = fs::read_dir(&from)
            .await
            .wrap_err("Failed to read directory")?;
        let mut entries = Vec::new();

        while let Some(entry) = dir.next_entry().await? {
            let meta = entry.metadata().await.wrap_err_with(|| {
                format!("Failed to get metadata for '{}'", entry.path().display())
            })?;
            entries.push((meta.is_dir(), entry));
        }

        Ok(entries)
    }

    let base = from.into();
    stream::unfold(vec![base.clone()], |mut state| async {
        let from = state.pop()?;
        let inner = match handle_dir(from).await {
            Ok(entries) => {
                for (is_dir, entry) in &entries {
                    if *is_dir {
                        state.push(entry.path());
                    }
                }
                stream::iter(entries).map(Ok).left_stream()
            }
            Err(e) => stream::once(async { Err(e) }).right_stream(),
        };

        Some((inner, state))
    })
    .flatten()
    .try_for_each(|(is_dir, entry)| {
        let path = entry.path();
        let dest = path
            .strip_prefix(&base)
            .map(|suffix| to.join(suffix))
            .expect("all entries are relative to the directory we are walking");

        async move {
            if is_dir {
                tracing::trace!("Creating directory '{}'", dest.display());
                fs::create_dir(&dest)
                    .await
                    .map(|_| ())
                    .wrap_err_with(|| format!("Failed to create directory '{}'", dest.display()))
            } else {
                tracing::trace!("Copying file '{}' -> '{}'", path.display(), dest.display());
                fs::copy(&path, &dest).await.map(|_| ()).wrap_err_with(|| {
                    format!(
                        "Failed to copy file '{}' -> '{}'",
                        path.display(),
                        dest.display()
                    )
                })
            }
        }
    })
    .await
    .map(|_| ())
}

#[tracing::instrument(skip(state))]
async fn copy_mod_folders(state: Arc<ActionState>) -> Result<Vec<String>> {
    let bundle_dir = Arc::new(state.game_dir.join("bundle"));

    let mut tasks = Vec::new();

    for mod_info in state
        .mods
        .iter()
        .filter(|m| m.id != "dml" && m.enabled && !m.bundled)
    {
        let span = tracing::trace_span!("copying legacy mod", name = mod_info.name);
        let _enter = span.enter();

        let mod_id = mod_info.id.clone();
        let mod_dir = Arc::clone(&state.mod_dir);
        let bundle_dir = Arc::clone(&bundle_dir);

        let task = async move {
            let from = mod_dir.join(&mod_id);
            let to = bundle_dir.join("mods").join(&mod_id);

            tracing::debug!(from = %from.display(), to = %to.display(), "Copying legacy mod '{}'", mod_id);
            let _ = fs::create_dir_all(&to).await;
            copy_recursive(&from, &to).await.wrap_err_with(|| {
                format!(
                    "Failed to copy legacy mod from '{}' to '{}'",
                    from.display(),
                    to.display()
                )
            })?;

            Ok::<_, Report>(mod_id)
        };
        tasks.push(task);
    }

    let ids = futures::future::try_join_all(tasks).await?;
    Ok(ids)
}

fn build_mod_data_lua(state: Arc<ActionState>) -> String {
    let mut lua = String::from("return {\n");

    // DMF is handled explicitely by the loading procedures, as it actually drives most of that
    // and should therefore not show up in the load order.
    for mod_info in state.mods.iter().filter(|m| m.id != "dml" && m.enabled) {
        lua.push_str("    {\n        name = \"");
        lua.push_str(&mod_info.name);

        lua.push_str("\",\n        id = \"");
        lua.push_str(&mod_info.id);

        lua.push_str("\",\n        bundled = \"");
        if mod_info.bundled {
            lua.push_str("true");
        } else {
            lua.push_str("false");
        }

        lua.push_str("\",\n        run = function()\n");

        let resources = &mod_info.resources;
        if resources.data.is_some() || resources.localization.is_some() {
            lua.push_str("            new_mod(\"");
            lua.push_str(&mod_info.id);
            lua.push_str("\", {\n                mod_script = \"");
            lua.push_str(&resources.init.to_slash_lossy());

            if let Some(data) = resources.data.as_ref() {
                lua.push_str("\",\n                mod_data = \"");
                lua.push_str(&data.to_slash_lossy());
            }

            if let Some(localization) = &resources.localization {
                lua.push_str("\",\n                mod_localization = \"");
                lua.push_str(&localization.to_slash_lossy());
            }

            lua.push_str("\",\n            })\n");
        } else {
            lua.push_str("            return dofile(\"");
            lua.push_str(&resources.init.to_slash_lossy());
            lua.push_str("\")\n");
        }

        lua.push_str("        end,\n        packages = {\n");

        for pkg_info in &mod_info.packages {
            lua.push_str("            \"");
            lua.push_str(&pkg_info.name);
            lua.push_str("\",\n");
        }

        lua.push_str("        },\n    },\n");
    }

    lua.push('}');

    tracing::debug!("mod_data_lua:\n{}", lua);

    lua
}

#[tracing::instrument(skip_all)]
async fn build_bundles(state: Arc<ActionState>) -> Result<Vec<Bundle>> {
    let mut mod_bundle = Bundle::new(MOD_BUNDLE_NAME.to_string());
    let mut tasks = Vec::new();

    let bundle_dir = Arc::new(state.game_dir.join("bundle"));

    let mut bundles = Vec::new();

    {
        tracing::trace!("Building mod data script");

        let span = tracing::debug_span!("Building mod data script");
        let _enter = span.enter();

        let lua = build_mod_data_lua(state.clone());

        tracing::trace!("Compiling mod data script");

        let file =
            lua::compile(MOD_DATA_SCRIPT, lua).wrap_err("Failed to compile mod data Lua file")?;

        tracing::trace!("Compile mod data script");

        mod_bundle.add_file(file);
    }

    tracing::trace!("Preparing tasks to deploy bundle files");

    for mod_info in state
        .mods
        .iter()
        .filter(|m| m.id != "dml" && m.enabled && m.bundled)
    {
        let span = tracing::trace_span!("building mod packages", name = mod_info.name);
        let _enter = span.enter();

        let mod_dir = state.mod_dir.join(&mod_info.id);
        for pkg_info in &mod_info.packages {
            let span = tracing::trace_span!("building package", name = pkg_info.name);
            let _enter = span.enter();

            tracing::trace!(
                "Building package {} for mod {}",
                pkg_info.name,
                mod_info.name
            );

            let pkg = make_package(pkg_info).wrap_err("Failed to make package")?;
            let mut variant = BundleFileVariant::new();
            let bin = pkg
                .to_binary()
                .wrap_err("Failed to serialize package to binary")?;
            variant.set_data(bin);
            let mut file = BundleFile::new(pkg_info.name.clone(), BundleFileType::Package);
            file.add_variant(variant);

            tracing::trace!(
                "Compiled package {} for mod {}",
                pkg_info.name,
                mod_info.name
            );

            mod_bundle.add_file(file);

            let bundle_name = format!("{:016x}", Murmur64::hash(&pkg_info.name));
            let src = mod_dir.join(&bundle_name);
            let dest = bundle_dir.join(&bundle_name);
            let pkg_name = pkg_info.name.clone();
            let mod_name = mod_info.name.clone();

            // Explicitely drop the guard, so that we can move the span
            // into the async operation
            drop(_enter);

            let ctx = state.ctx.clone();

            let task = async move {
                let bundle = {
                    let bin = fs::read(&src).await.wrap_err_with(|| {
                        format!("Failed to read bundle file '{}'", src.display())
                    })?;
                    let name = Bundle::get_name_from_path(&ctx, &src);
                    Bundle::from_binary(&ctx, name, bin)
                        .wrap_err_with(|| format!("Failed to parse bundle '{}'", src.display()))?
                };

                tracing::debug!(
                    src = %src.display(),
                    dest = %dest.display(),
                    "Copying bundle '{}' for mod '{}'",
                    pkg_name,
                    mod_name,
                );
                // We attempt to remove any previous file, so that the hard link can be created.
                // We can reasonably ignore errors here, as a 'NotFound' is actually fine, the copy
                // may be possible despite an error here, or the error will be reported by it anyways.
                // TODO: There is a chance that we delete an actual game bundle, but with 64bit
                // hashes, it's low enough for now, and the setup required to detect
                // "game bundle vs mod bundle" is non-trivial.
                let _ = fs::remove_file(&dest).await;
                fs::copy(&src, &dest).await.wrap_err_with(|| {
                    format!(
                        "Failed to copy bundle {pkg_name} for mod {mod_name}. Src: {}, dest: {}",
                        src.display(),
                        dest.display()
                    )
                })?;

                Ok::<Bundle, color_eyre::Report>(bundle)
            }
            .instrument(span);

            tasks.push(task);
        }
    }

    tracing::debug!("Copying {} mod bundles", tasks.len());

    let mut tasks = stream::iter(tasks).buffer_unordered(10);

    while let Some(res) = tasks.next().await {
        let bundle = res?;
        bundles.push(bundle);
    }

    {
        let path = bundle_dir.join(format!("{:x}", mod_bundle.name().to_murmur64()));
        tracing::trace!("Writing mod bundle to '{}'", path.display());
        fs::write(&path, mod_bundle.to_binary()?)
            .await
            .wrap_err_with(|| format!("Failed to write bundle to '{}'", path.display()))?;
    }

    bundles.push(mod_bundle);

    Ok(bundles)
}

#[tracing::instrument(skip_all)]
async fn patch_boot_bundle(state: Arc<ActionState>) -> Result<Vec<Bundle>> {
    let bundle_dir = Arc::new(state.game_dir.join("bundle"));
    let bundle_path = bundle_dir.join(format!("{:x}", Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes())));

    let mut bundles = Vec::with_capacity(2);

    let mut boot_bundle = async {
        let bin = read_file_with_backup(&bundle_path)
            .await
            .wrap_err("Failed to read boot bundle")?;

        Bundle::from_binary(&state.ctx, BOOT_BUNDLE_NAME.to_string(), bin)
            .wrap_err("Failed to parse boot bundle")
    }
    .instrument(tracing::trace_span!("read boot bundle"))
    .await
    .wrap_err_with(|| format!("Failed to read bundle '{}'", BOOT_BUNDLE_NAME))?;

    {
        tracing::trace!("Adding mod package file to boot bundle");
        let span = tracing::trace_span!("create mod package file");
        let _enter = span.enter();

        let mut pkg = Package::new(MOD_BUNDLE_NAME.to_string(), PathBuf::new());

        for mod_info in &state.mods {
            for pkg_info in &mod_info.packages {
                pkg.add_file(BundleFileType::Package, &pkg_info.name);
            }
        }

        pkg.add_file(BundleFileType::Lua, MOD_DATA_SCRIPT);

        let mut variant = BundleFileVariant::new();
        variant.set_data(pkg.to_binary()?);
        let mut f = BundleFile::new(MOD_BUNDLE_NAME.to_string(), BundleFileType::Package);
        f.add_variant(variant);

        boot_bundle.add_file(f);
    }

    {
        tracing::trace!("Handling DML packages and bundle");
        let span = tracing::trace_span!("handle DML");
        let _enter = span.enter();

        let mut variant = BundleFileVariant::new();

        let mod_info = state
            .mods
            .iter()
            .find(|m| m.id == "dml")
            .ok_or_else(|| eyre::eyre!("DML not found in mod list"))?;
        let pkg_info = mod_info
            .packages
            .get(0)
            .ok_or_else(|| eyre::eyre!("invalid mod package for DML"))
            .with_suggestion(|| "Re-download and import the newest version.".to_string())?;
        let bundle_name = format!("{:016x}", Murmur64::hash(&pkg_info.name));
        let src = state.mod_dir.join(&mod_info.id).join(&bundle_name);

        {
            let bin = fs::read(&src)
                .await
                .wrap_err_with(|| format!("Failed to read bundle file '{}'", src.display()))?;
            let name = Bundle::get_name_from_path(&state.ctx, &src);

            let dml_bundle = Bundle::from_binary(&state.ctx, name, bin)
                .wrap_err_with(|| format!("Failed to parse bundle '{}'", src.display()))?;

            bundles.push(dml_bundle);
        };

        {
            let dest = bundle_dir.join(&bundle_name);
            let pkg_name = pkg_info.name.clone();
            let mod_name = mod_info.name.clone();

            tracing::debug!(
                "Copying bundle {} for mod {}: {} -> {}",
                pkg_name,
                mod_name,
                src.display(),
                dest.display()
            );
            // We attempt to remove any previous file, so that the hard link can be created.
            // We can reasonably ignore errors here, as a 'NotFound' is actually fine, the copy
            // may be possible despite an error here, or the error will be reported by it anyways.
            // TODO: There is a chance that we delete an actual game bundle, but with 64bit
            // hashes, it's low enough for now, and the setup required to detect
            // "game bundle vs mod bundle" is non-trivial.
            let _ = fs::remove_file(&dest).await;
            fs::copy(&src, &dest).await.wrap_err_with(|| {
                format!(
                    "Failed to copy bundle {pkg_name} for mod {mod_name}. Src: {}, dest: {}",
                    src.display(),
                    dest.display()
                )
            })?;
        }

        let pkg = make_package(pkg_info).wrap_err("Failed to create package file for dml")?;
        variant.set_data(pkg.to_binary()?);

        let mut f = BundleFile::new(DML_BUNDLE_NAME.to_string(), BundleFileType::Package);
        f.add_variant(variant);

        boot_bundle.add_file(f);
    }

    {
        let span = tracing::debug_span!("Importing mod main script");
        let _enter = span.enter();

        let is_io_enabled = format!("{}", state.is_io_enabled);
        let mut data = HashMap::new();
        data.insert("is_io_enabled", is_io_enabled.as_str());

        let tmpl = include_str!("../../assets/mod_main.lua");
        let lua = Template::new(tmpl).render(&data);
        tracing::trace!("Main script rendered:\n===========\n{}\n=============", lua);
        let file =
            lua::compile(MOD_BOOT_SCRIPT, lua).wrap_err("Failed to compile mod main Lua file")?;

        boot_bundle.add_file(file);
    }

    async {
        let bin = boot_bundle
            .to_binary()
            .wrap_err("Failed to serialize boot bundle")?;
        fs::write(&bundle_path, bin)
            .await
            .wrap_err_with(|| format!("Failed to write main bundle: {}", bundle_path.display()))
    }
    .instrument(tracing::trace_span!("write boot bundle"))
    .await?;

    bundles.push(boot_bundle);

    Ok(bundles)
}

#[tracing::instrument(skip_all, fields(bundles = bundles.as_ref().len()))]
async fn patch_bundle_database<B>(state: Arc<ActionState>, bundles: B) -> Result<()>
where
    B: AsRef<[Bundle]>,
{
    let bundle_dir = Arc::new(state.game_dir.join("bundle"));
    let database_path = bundle_dir.join(BUNDLE_DATABASE_NAME);

    let mut db = {
        let bin = read_file_with_backup(&database_path)
            .await
            .wrap_err("Failed to read bundle database")?;
        let mut r = Cursor::new(bin);
        let db = BundleDatabase::from_binary(&mut r).wrap_err("Failed to parse bundle database")?;
        tracing::trace!("Finished parsing bundle database");
        db
    };

    for bundle in bundles.as_ref() {
        tracing::trace!("Adding '{}' to bundle database", bundle.name().display());
        db.add_bundle(bundle);
    }

    {
        let bin = db
            .to_binary()
            .wrap_err("Failed to serialize bundle database")?;
        fs::write(&database_path, bin).await.wrap_err_with(|| {
            format!(
                "failed to write bundle database to '{}'",
                database_path.display()
            )
        })?;
    }

    Ok(())
}

#[tracing::instrument(skip_all, fields(bundles = bundles.as_ref().len()))]
async fn write_deployment_data<B>(
    state: Arc<ActionState>,
    bundles: B,
    mod_folders: Vec<String>,
) -> Result<()>
where
    B: AsRef<[Bundle]>,
{
    let info = DeploymentData {
        timestamp: OffsetDateTime::now_utc(),
        bundles: bundles
            .as_ref()
            .iter()
            .map(|bundle| format!("{:x}", bundle.name().to_murmur64()))
            .collect(),
        // TODO:
        mod_folders,
    };
    let path = state.game_dir.join(DEPLOYMENT_DATA_PATH);
    let data = serde_sjson::to_string(&info).wrap_err("Failed to serizalie deployment data")?;

    fs::write(&path, &data)
        .await
        .wrap_err_with(|| format!("Failed to write deployment data to '{}'", path.display()))?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(
    game_dir = %state.game_dir.display(),
    mods = state.mods.len()
))]
pub(crate) async fn deploy_mods(state: ActionState) -> Result<()> {
    let state = Arc::new(state);
    let bundle_dir = state.game_dir.join("bundle");
    let boot_bundle_path = format!("{:016x}", Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes()));

    if fs::metadata(bundle_dir.join(format!("{boot_bundle_path}.patch_999")))
        .await
        .is_ok()
    {
        let err = eyre::eyre!("Found dtkit-patch-based mod installation.");
        return Err(err)
            .with_suggestion(|| {
                "If you're a mod author and saved projects directly in 'mods/', \
                use DTMT to migrate them to the new project structure."
                    .to_string()
            })
            .with_suggestion(|| {
                "Click 'Reset Game' to remove the previous mod installation.".to_string()
            });
    }

    let (_, game_info, deployment_info) = tokio::try_join!(
        async {
            fs::metadata(&bundle_dir)
                .await
                .wrap_err("Failed to open game bundle directory")
                .with_suggestion(|| "Double-check 'Game Directory' in the Settings tab.")
        },
        async {
            tokio::task::spawn_blocking(dtmt_shared::collect_game_info)
                .await
                .map_err(Report::new)
        },
        async {
            let path = state.game_dir.join(DEPLOYMENT_DATA_PATH);
            match read_sjson_file::<_, DeploymentData>(path).await {
                Ok(data) => Ok(Some(data)),
                Err(err) => {
                    if let Some(err) = err.downcast_ref::<std::io::Error>()
                        && err.kind() == ErrorKind::NotFound
                    {
                        Ok(None)
                    } else {
                        Err(err).wrap_err("Failed to read deployment data")
                    }
                }
            }
        }
    )
    .wrap_err("Failed to gather deployment information")?;

    tracing::debug!(?game_info, ?deployment_info);

    if let Some(game_info) = game_info {
        if deployment_info
            .as_ref()
            .map(|i| game_info.last_updated > i.timestamp)
            .unwrap_or(false)
        {
            tracing::warn!(
                "Game was updated since last mod deployment. \
                    Attempting to reconcile game files."
            );

            tokio::try_join!(
                async {
                    let path = bundle_dir.join(BUNDLE_DATABASE_NAME);
                    let backup_path = path.with_extension("data.bak");

                    fs::copy(&path, &backup_path)
                        .await
                        .wrap_err("Failed to re-create backup for bundle database.")
                },
                async {
                    let path = bundle_dir.join(boot_bundle_path);
                    let backup_path = path.with_extension("bak");

                    fs::copy(&path, &backup_path)
                        .await
                        .wrap_err("Failed to re-create backup for boot bundle")
                }
            )
            .with_suggestion(|| {
                "Reset the game using 'Reset Game', then verify game files.".to_string()
            })?;

            tracing::info!(
                "Successfully re-created game file backups. \
                    Continuing mod deployment."
            );
        }
    }

    check_mod_order(&state)?;

    tracing::info!(
        "Deploying {} mods to '{}'.",
        state.mods.iter().filter(|i| i.enabled).count(),
        bundle_dir.display()
    );

    tracing::info!("Copy legacy mod folders");
    let mod_folders = copy_mod_folders(state.clone())
        .await
        .wrap_err("Failed to copy mod folders")?;

    tracing::info!("Build mod bundles");
    let mut bundles = build_bundles(state.clone())
        .await
        .wrap_err("Failed to build mod bundles")?;

    tracing::info!("Patch boot bundle");
    let mut boot_bundles = patch_boot_bundle(state.clone())
        .await
        .wrap_err("Failed to patch boot bundle")?;
    bundles.append(&mut boot_bundles);

    if let Some(info) = &deployment_info {
        let bundle_dir = Arc::new(bundle_dir);
        // Remove bundles from the previous deployment that don't match the current one.
        // I.e. mods that used to be installed/enabled but aren't anymore.
        {
            let tasks = info.bundles.iter().cloned().filter_map(|file_name| {
                let is_being_deployed = bundles.iter().any(|b2| {
                    let name = format!("{:016x}", b2.name());
                    file_name == name
                });

                if !is_being_deployed {
                    let bundle_dir = bundle_dir.clone();
                    let task = async move {
                        let path = bundle_dir.join(&file_name);

                        tracing::debug!("Removing unused bundle '{}'", file_name);

                        if let Err(err) = fs::remove_file(&path).await.wrap_err_with(|| {
                            format!("Failed to remove unused bundle '{}'", path.display())
                        }) {
                            tracing::error!("{:?}", err);
                        }
                    };
                    Some(task)
                } else {
                    None
                }
            });

            futures::future::join_all(tasks).await;
        }

        // Do the same thing for mod folders
        {
            let tasks = info.mod_folders.iter().filter_map(|mod_id| {
                let is_being_deployed = mod_folders.iter().any(|id| id == mod_id);

                if !is_being_deployed {
                    let path = bundle_dir.join("mods").join(mod_id);
                    tracing::debug!("Removing unused mod folder '{}'", path.display());

                    let task = async move {
                        if let Err(err) = fs::remove_dir_all(&path).await.wrap_err_with(|| {
                            format!("Failed to remove unused legacy mod '{}'", path.display())
                        }) {
                            tracing::error!("{:?}", err);
                        }
                    };

                    Some(task)
                } else {
                    None
                }
            });
            futures::future::join_all(tasks).await;
        }
    }

    tracing::info!("Patch game settings");
    patch_game_settings(state.clone())
        .await
        .wrap_err("Failed to patch game settings")?;

    tracing::info!("Patching bundle database");
    patch_bundle_database(state.clone(), &bundles)
        .await
        .wrap_err("Failed to patch bundle database")?;

    tracing::info!("Writing deployment data");
    write_deployment_data(state.clone(), &bundles, mod_folders)
        .await
        .wrap_err("Failed to write deployment data")?;

    tracing::info!("Finished deploying mods");
    Ok(())
}
