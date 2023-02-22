use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Cursor, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::{eyre, Help, Result};
use druid::FileInfo;
use futures::stream;
use futures::StreamExt;
use sdk::filetype::lua;
use sdk::filetype::package::Package;
use sdk::murmur::Murmur64;
use sdk::{
    Bundle, BundleDatabase, BundleFile, BundleFileType, BundleFileVariant, FromBinary, ModConfig,
    ToBinary,
};
use tokio::io::AsyncWriteExt;
use tokio::{fs, try_join};
use tracing::Instrument;
use zip::ZipArchive;

use crate::state::{ModInfo, PackageInfo, State};

const MOD_BUNDLE_NAME: &str = "packages/mods";
const BOOT_BUNDLE_NAME: &str = "packages/boot";
const BUNDLE_DATABASE_NAME: &str = "bundle_database.data";
const MOD_BOOT_SCRIPT: &str = "scripts/mod_main";
const MOD_DATA_SCRIPT: &str = "scripts/mods/mod_data";

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
                    "failed to back up {} '{}' to '{}'",
                    file_name,
                    path.display(),
                    backup_path.display()
                )
            })?;

            tracing::debug!("Reading {} from original '{}'", file_name, path.display());
            fs::read(path).await.wrap_err_with(|| {
                format!("failed to read {} file: {}", file_name, path.display())
            })?
        }
        Err(err) => {
            return Err(err).wrap_err_with(|| {
                format!(
                    "failed to read {} from backup '{}'",
                    file_name,
                    backup_path.display()
                )
            });
        }
    };
    Ok(bin)
}

#[tracing::instrument(skip_all)]
async fn patch_game_settings(state: Arc<State>) -> Result<()> {
    let settings_path = state
        .get_game_dir()
        .join("bundle/application_settings/settings_common.ini");

    let settings = read_file_with_backup(&settings_path)
        .await
        .wrap_err("failed to read settings.ini")?;
    let settings = String::from_utf8(settings).wrap_err("settings.ini is not valid UTF-8")?;

    let mut f = fs::File::create(&settings_path)
        .await
        .wrap_err_with(|| format!("failed to open {}", settings_path.display()))?;

    let Some(i) = settings.find("boot_script =") else {
        eyre::bail!("couldn't find 'boot_script' field");
    };

    f.write_all(settings[0..i].as_bytes()).await?;
    f.write_all(b"boot_script = \"scripts/mod_main\"").await?;

    let Some(j) = settings[i..].find('\n') else {
        eyre::bail!("couldn't find end of 'boot_script' field");
    };

    f.write_all(settings[(i + j)..].as_bytes()).await?;

    tracing::info!("Patched game settings");
    Ok(())
}

#[tracing::instrument(skip_all, fields(package = info.get_name()))]
fn make_package(info: &PackageInfo) -> Result<Package> {
    let mut pkg = Package::new(info.get_name().clone(), PathBuf::new());

    for f in info.get_files().iter() {
        let mut it = f.rsplit('.');
        let file_type = it
            .next()
            .ok_or_else(|| eyre::eyre!("missing file extension"))
            .and_then(BundleFileType::from_str)
            .wrap_err("invalid file name in package info")?;
        let name: String = it.collect();
        pkg.add_file(file_type, name);
    }

    Ok(pkg)
}

fn build_mod_data_lua(state: Arc<State>) -> String {
    let mut lua = String::from("return {\n");

    // DMF is handled explicitely by the loading procedures, as it actually drives most of that
    // and should therefore not show up in the load order.
    for mod_info in state
        .get_mods()
        .iter()
        .filter(|m| m.get_id() != "dml" && m.get_enabled())
    {
        lua.push_str("    {\n        name = \"");
        lua.push_str(mod_info.get_name());

        lua.push_str("\",\n        id = \"");
        lua.push_str(mod_info.get_id());

        lua.push_str("\",\n        run = function()\n");

        let resources = mod_info.get_resources();
        if resources.get_data().is_some() || resources.get_localization().is_some() {
            lua.push_str("            new_mod(\"");
            lua.push_str(mod_info.get_id());
            lua.push_str("\", {\n                init = \"");
            lua.push_str(resources.get_init());

            if let Some(data) = resources.get_data() {
                lua.push_str("\",\n                data = \"");
                lua.push_str(data);
            }

            if let Some(localization) = resources.get_localization() {
                lua.push_str("\",\n                localization = \"");
                lua.push_str(localization);
            }

            lua.push_str("\",\n            })\n");
        } else {
            lua.push_str("            return dofile(\"");
            lua.push_str(resources.get_init());
            lua.push_str("\")");
        }

        lua.push_str("        end,\n        packages = [\n");

        for pkg_info in mod_info.get_packages() {
            lua.push_str("            \"");
            lua.push_str(pkg_info.get_name());
            lua.push_str("\",\n");
        }

        lua.push_str("        ]\n    }\n");
    }

    lua.push('}');

    tracing::debug!("mod_data_lua:\n{}", lua);

    lua
}

#[tracing::instrument(skip_all)]
async fn build_bundles(state: Arc<State>) -> Result<()> {
    let mut bundle = Bundle::new(MOD_BUNDLE_NAME.into());
    let mut tasks = Vec::new();

    let bundle_dir = Arc::new(state.get_game_dir().join("bundle"));
    let database_path = bundle_dir.join(BUNDLE_DATABASE_NAME);

    let mut db = {
        let bin = read_file_with_backup(&database_path)
            .await
            .wrap_err("failed to read bundle database")?;
        let mut r = Cursor::new(bin);
        let db = BundleDatabase::from_binary(&mut r).wrap_err("failed to parse bundle database")?;
        tracing::trace!("Finished parsing bundle database");
        db
    };

    {
        let span = tracing::debug_span!("Building mod data script");
        let _enter = span.enter();

        let lua = build_mod_data_lua(state.clone());
        let lua = CString::new(lua).wrap_err("failed to build CString from mod data Lua string")?;
        let file =
            lua::compile(MOD_DATA_SCRIPT, &lua).wrap_err("failed to compile mod data Lua file")?;

        bundle.add_file(file);
    }

    for mod_info in state.get_mods().iter().filter(|m| m.get_enabled()) {
        let span = tracing::trace_span!("building mod packages", name = mod_info.get_name());
        let _enter = span.enter();

        let mod_dir = state.get_mod_dir().join(mod_info.get_name());
        for pkg_info in mod_info.get_packages() {
            let span = tracing::trace_span!("building package", name = pkg_info.get_name());
            let _enter = span.enter();

            let pkg = make_package(pkg_info).wrap_err("failed to make package")?;
            let mut variant = BundleFileVariant::new();
            let bin = pkg
                .to_binary()
                .wrap_err("failed to serialize package to binary")?;
            variant.set_data(bin);
            let mut file = BundleFile::new(pkg_info.get_name().clone(), BundleFileType::Package);
            file.add_variant(variant);

            bundle.add_file(file);

            let bundle_name = Murmur64::hash(pkg_info.get_name())
                .to_string()
                .to_ascii_lowercase();
            let src = mod_dir.join(&bundle_name);
            let dest = bundle_dir.clone();
            let pkg_name = pkg_info.get_name().clone();
            let mod_name = mod_info.get_name().clone();

            tracing::trace!(
                "Adding package {} for mod {} to bundle database",
                pkg_info.get_name(),
                mod_info.get_name()
            );

            // Explicitely drop the guard, so that we can move the span
            // into the async operation
            drop(_enter);

            let task = async move {
                tracing::debug!(
                    "Copying bundle {} for mod {}: {} -> {}",
                    pkg_name,
                    mod_name,
                    src.display(),
                    dest.display()
                );
                fs::hard_link(&src, dest.as_ref()).await.wrap_err_with(|| {
                    format!("failed to hard link bundle {pkg_name} for mod {mod_name}. src: {}, dest: {}", src.display(), dest.display())
                })
            }
            .instrument(span);

            tasks.push(task);
        }
    }

    tracing::debug!("Copying {} mod bundles", tasks.len());

    let mut tasks = stream::iter(tasks).buffer_unordered(10);

    while let Some(res) = tasks.next().await {
        res?;
    }

    db.add_bundle(&bundle);

    {
        let path = bundle_dir.join(format!("{:x}", Murmur64::hash(bundle.name())));
        tracing::trace!("Writing mod bundle to '{}'", path.display());
        fs::write(&path, bundle.to_binary()?)
            .await
            .wrap_err_with(|| format!("failed to write bundle to '{}'", path.display()))?;
    }

    {
        tracing::trace!("Writing bundle database to '{}'", database_path.display());
        let bin = db
            .to_binary()
            .wrap_err("failed to serialize bundle database")?;
        fs::write(&database_path, bin).await.wrap_err_with(|| {
            format!(
                "failed to write bundle database to '{}'",
                database_path.display()
            )
        })?;
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn patch_boot_bundle(state: Arc<State>) -> Result<()> {
    let bundle_dir = Arc::new(state.get_game_dir().join("bundle"));

    let bundle_path = bundle_dir.join(format!("{:x}", Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes())));
    let database_path = bundle_dir.join(BUNDLE_DATABASE_NAME);

    let (mut db, mut bundle) = try_join!(
        async {
            let bin = read_file_with_backup(&database_path)
                .await
                .wrap_err("failed to read bundle database")?;
            let mut r = Cursor::new(bin);

            BundleDatabase::from_binary(&mut r).wrap_err("failed to parse bundle database")
        }
        .instrument(tracing::trace_span!("read bundle database")),
        async {
            let bin = read_file_with_backup(&bundle_path)
                .await
                .wrap_err("failed to read boot bundle")?;

            Bundle::from_binary(&state.get_ctx(), BOOT_BUNDLE_NAME.to_string(), bin)
                .wrap_err("failed to parse boot bundle")
        }
        .instrument(tracing::trace_span!("read boot bundle"))
    )?;

    {
        tracing::trace!("Adding mod package file to boot bundle");
        let span = tracing::trace_span!("create mod package file");
        let _enter = span.enter();

        let mut pkg = Package::new(MOD_BUNDLE_NAME.to_string(), PathBuf::new());

        for mod_info in state.get_mods() {
            for pkg_info in mod_info.get_packages() {
                pkg.add_file(BundleFileType::Package, pkg_info.get_name());
            }
        }

        let mut variant = BundleFileVariant::new();
        variant.set_data(pkg.to_binary()?);
        let mut f = BundleFile::new(MOD_BUNDLE_NAME.to_string(), BundleFileType::Package);
        f.add_variant(variant);

        bundle.add_file(f);
    }

    {
        let span = tracing::debug_span!("Importing mod main script");
        let _enter = span.enter();

        let lua = include_str!("../assets/mod_main.lua");
        let lua = CString::new(lua).wrap_err("failed to build CString from mod main Lua string")?;
        let file =
            lua::compile(MOD_BOOT_SCRIPT, &lua).wrap_err("failed to compile mod main Lua file")?;

        bundle.add_file(file);
    }

    db.add_bundle(&bundle);

    try_join!(
        async {
            let bin = bundle
                .to_binary()
                .wrap_err("failed to serialize boot bundle")?;
            fs::write(&bundle_path, bin)
                .await
                .wrap_err_with(|| format!("failed to write main bundle: {}", bundle_path.display()))
        }
        .instrument(tracing::trace_span!("write boot bundle")),
        async {
            let bin = db
                .to_binary()
                .wrap_err("failed to serialize bundle database")?;
            fs::write(&database_path, bin).await.wrap_err_with(|| {
                format!(
                    "failed to write bundle database to '{}'",
                    database_path.display()
                )
            })
        }
        .instrument(tracing::trace_span!("write bundle database"))
    )?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(
    game_dir = %state.get_game_dir().display(),
    mods = state.get_mods().len()
))]
pub(crate) async fn deploy_mods(state: State) -> Result<()> {
    let state = Arc::new(state);

    {
        let mods = state.get_mods();
        let first = mods.get(0);
        if first.is_none() || !(first.unwrap().get_id() == "dml" && first.unwrap().get_enabled()) {
            // TODO: Add a suggestion where to get it, once that's published
            eyre::bail!("'Darktide Mod Loader' needs to be installed, enabled and at the top of the load order");
        }
    }

    tracing::info!(
        "Deploying {} mods to {}",
        state.get_mods().len(),
        state.get_game_dir().join("bundle").display()
    );

    tracing::info!("Build mod bundles");
    build_bundles(state.clone())
        .await
        .wrap_err("failed to build mod bundles")?;

    tracing::info!("Patch boot bundle");
    patch_boot_bundle(state.clone())
        .await
        .wrap_err("failed to patch boot bundle")?;

    tracing::info!("Patch game settings");
    patch_game_settings(state.clone())
        .await
        .wrap_err("failed to patch game settings")?;

    tracing::info!("Finished deploying mods");
    Ok(())
}

#[tracing::instrument(skip(state))]
pub(crate) async fn import_mod(state: State, info: FileInfo) -> Result<ModInfo> {
    let data = fs::read(&info.path)
        .await
        .wrap_err_with(|| format!("failed to read file {}", info.path.display()))?;
    let data = Cursor::new(data);

    let mut archive = ZipArchive::new(data).wrap_err("failed to open ZIP archive")?;

    if tracing::enabled!(tracing::Level::DEBUG) {
        let names = archive.file_names().fold(String::new(), |mut s, name| {
            s.push('\n');
            s.push_str(name);
            s
        });
        tracing::debug!("Archive contents:{}", names);
    }

    let dir_name = {
        let f = archive.by_index(0).wrap_err("archive is empty")?;

        if !f.is_dir() {
            let err = eyre::eyre!("archive does not have a top-level directory");
            return Err(err).with_suggestion(|| "Use 'dtmt build' to create the mod archive.");
        }

        let name = f.name();
        // The directory name is returned with a trailing slash, which we don't want
        name[..(name.len().saturating_sub(1))].to_string()
    };

    tracing::info!("Importing mod {}", dir_name);

    let mod_cfg: ModConfig = {
        let mut f = archive
            .by_name(&format!("{}/{}", dir_name, "dtmt.cfg"))
            .wrap_err("failed to read mod config from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("failed to read mod config from archive")?;

        let data = String::from_utf8(buf).wrap_err("mod config is not valid UTF-8")?;

        serde_sjson::from_str(&data).wrap_err("failed to deserialize mod config")?
    };

    tracing::debug!(?mod_cfg);

    let files: HashMap<String, Vec<String>> = {
        let mut f = archive
            .by_name(&format!("{}/{}", dir_name, "files.sjson"))
            .wrap_err("failed to read file index from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("failed to read file index from archive")?;

        let data = String::from_utf8(buf).wrap_err("file index is not valid UTF-8")?;

        serde_sjson::from_str(&data).wrap_err("failed to deserialize file index")?
    };

    tracing::trace!(?files);

    let mod_dir = state.get_mod_dir();

    tracing::trace!("Creating mods directory {}", mod_dir.display());
    fs::create_dir_all(&mod_dir)
        .await
        .wrap_err_with(|| format!("failed to create data directory {}", mod_dir.display()))?;

    tracing::trace!("Extracting mod archive to {}", mod_dir.display());
    archive
        .extract(&mod_dir)
        .wrap_err_with(|| format!("failed to extract archive to {}", mod_dir.display()))?;

    let packages = files
        .into_iter()
        .map(|(name, files)| PackageInfo::new(name, files.into_iter().collect()))
        .collect();
    let info = ModInfo::new(mod_cfg, packages);

    Ok(info)
}
