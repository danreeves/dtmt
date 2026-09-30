use std::collections::HashSet;
use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::{Help, Report, Result, eyre};
use futures::StreamExt;
use futures::{TryStreamExt, stream};
use minijinja::Environment;
use sdk::filetype::lua;
use sdk::filetype::package::Package;
use sdk::murmur::Murmur64;
use sdk::{
    Bundle, BundleDatabase, BundleFile, BundleFileType, BundleFileVariant, FromBinary, ToBinary,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::fs::{self, DirEntry};
use tokio::io::AsyncWriteExt;
use tracing::Instrument;

use super::read_sjson_file;
use crate::controller::app::check_mod_order;
use crate::state::{ActionState, PackageInfo};

pub const MOD_BUNDLE_NAME: &str = "packages/mods";
pub const BOOT_BUNDLE_NAME: &str = "packages/boot";
pub const BUNDLE_DATABASE_NAME: &str = "bundle_database.data";
pub const MOD_BOOT_SCRIPT: &str = "scripts/mod_main";
pub const MOD_DATA_SCRIPT: &str = "scripts/mods/mod_data";
pub const SETTINGS_FILE_PATH: &str = "application_settings/settings_common.ini";
pub const DEPLOYMENT_DATA_PATH: &str = "dtmm-deployment.sjson";

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentData {
    pub bundles: Vec<String>,
    pub mod_folders: Vec<String>,
    /// Game files under `bundle/` (other than the bundles and database above)
    /// that a deployment overwrote, relative to `bundle/`. Their originals are
    /// kept next to them with a `.bak` suffix so a reset can put them back.
    #[serde(default)]
    pub data_files: Vec<String>,
    #[serde(with = "time::serde::iso8601")]
    pub timestamp: OffsetDateTime,
    /// Content hashes of the bundle database and the boot bundle as this
    /// deployment wrote them, so a later deploy can tell the game updating
    /// those files from our own writes touching their directory.
    ///
    /// Written as strings: sjson's integers do not fit a u64's range, and a
    /// plain number overflows the reader on the way back in.
    #[serde(default, with = "hash_list")]
    pub deployed_hashes: Vec<u64>,
}

/// Round-trips the hashes as hex (`0x…`) strings: sjson writes integers as
/// bare tokens the reader takes as an i64, which cannot carry a u64's range,
/// while a hex word with letters stays a string on the way back.
mod hash_list {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(hashes: &Vec<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // No `0x` prefix: the lexer would read that as the integer and leave
        // the rest for the next token (bare words that begin with a letter,
        // exactly like the bundle names, lex as strings).
        let words: Vec<String> = hashes.iter().map(|hash| format!("{hash:x}")).collect();
        words.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Text(String),
            Number(i64),
        }
        Vec::<Value>::deserialize(deserializer).and_then(|values| {
            values
                .into_iter()
                .map(|value| match value {
                    Value::Text(text) => {
                        // A hex word and a decimal one both land here; a
                        // hash written as bare decimal digits is a legacy
                        // write, so hex wins only when letters are present.
                        let text = text.strip_prefix("0x").unwrap_or(&text);
                        let radix = if text.bytes().any(|b| (b'a'..=b'f').contains(&b)) {
                            16
                        } else {
                            10
                        };
                        u64::from_str_radix(text, radix).map_err(serde::de::Error::custom)
                    }
                    Value::Number(number) => {
                        u64::from_str_radix(&number.to_string(), 10).map_err(serde::de::Error::custom)
                    }
                })
                .collect()
        })
    }
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

    match fs::read(path).await {
        Ok(bin) => {
            // Back the current content up on first touch, but never read it
            // back here. The game can rewrite this file between deployments (a
            // game update does exactly that); patching the backup's content
            // instead would silently revert whatever was written since.
            backup_file(path, false).await?;
            Ok(bin)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // Steam removes a file it considers corrupt and re-downloads it
            // later; until then the backup is all there is to patch.
            tracing::warn!(
                "{} is missing; falling back to backup '{}'",
                file_name,
                backup_path.display()
            );
            fs::read(&backup_path).await.wrap_err_with(|| {
                format!(
                    "Failed to read {} from backup '{}'",
                    file_name,
                    backup_path.display()
                )
            })
        }
        Err(err) => Err(err).wrap_err_with(|| {
            format!("Failed to read {} file: {}", file_name, path.display())
        }),
    }
}

/// The value of a settings file's `boot_script` field.
pub(crate) fn boot_script(settings: &str) -> Option<&str> {
    let start = settings.find("boot_script =")? + "boot_script =".len();
    let rest = &settings[start..];
    let end = rest.find('\n')?;
    Some(rest[..end].trim().trim_matches('"'))
}

/// A settings file with its `boot_script` field set to `script`; everything
/// else, including the `script_data` block that carries the client version the
/// backend checks, is left exactly as it was.
pub(crate) fn with_boot_script(settings: &str, script: &str) -> Result<String> {
    let Some(i) = settings.find("boot_script =") else {
        eyre::bail!("couldn't find 'boot_script' field");
    };
    let Some(j) = settings[i..].find('\n') else {
        eyre::bail!("couldn't find end of 'boot_script' field");
    };
    let mut out = String::with_capacity(settings.len());
    out.push_str(&settings[..i]);
    out.push_str(&format!("boot_script = \"{script}\""));
    out.push_str(&settings[i + j..]);
    Ok(out)
}

/// Sets a settings file's `boot_script` field from the file's *current*
/// content, so a game update's version fields are never overwritten with an
/// older file from a backup - only this one line changes, in both directions.
pub(crate) async fn set_boot_script(settings_path: &Path, script: &str) -> Result<()> {
    let settings = read_file_with_backup(settings_path).await?;
    let settings = String::from_utf8(settings).wrap_err("Settings.ini is not valid UTF-8")?;
    let patched = with_boot_script(&settings, script)?;
    fs::write(settings_path, patched)
        .await
        .wrap_err_with(|| format!("Failed to write {}", settings_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = concat!(
        "boot_script = \"scripts/main\"\n",
        "console_port = 14711\n",
        "script_data = {\n",
        "\tgame_revision = \"138030\"\n",
        "\tgame_version = \"1.13.0-b802981\"\n",
        "}\n",
    );

    #[test]
    fn only_the_boot_script_line_changes() {
        let patched = with_boot_script(SETTINGS, MOD_BOOT_SCRIPT).expect("patch");
        assert!(patched.contains("boot_script = \"scripts/mod_main\""));
        assert!(patched.contains("game_version = \"1.13.0-b802981\""));
        assert!(patched.contains("game_revision = \"138030\""));
        assert_eq!(boot_script(&patched), Some(MOD_BOOT_SCRIPT));
        assert_eq!(with_boot_script(&patched, "scripts/main").unwrap(), SETTINGS);
    }

    #[test]
    fn a_file_without_the_field_is_refused() {
        assert!(with_boot_script("console_port = 1\n", MOD_BOOT_SCRIPT).is_err());
    }
}

#[tracing::instrument(skip_all)]
async fn patch_game_settings(state: Arc<ActionState>) -> Result<()> {
    let settings_path = state.game_dir.join("bundle").join(SETTINGS_FILE_PATH);
    set_boot_script(&settings_path, MOD_BOOT_SCRIPT)
        .await
        .wrap_err("Failed to patch settings.ini")
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
                // Instead of trying to filter "already exists" errors out explicitly,
                // we just ignore all. It'll fail eventually with the next copy operation.
                let _ = fs::create_dir(&dest).await;
                Ok(())
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
    let game_dir = Arc::clone(&state.game_dir);

    let mut tasks = Vec::new();

    for mod_info in state.mods.iter().filter(|m| m.enabled && !m.bundled) {
        let span = tracing::trace_span!("copying legacy mod", name = mod_info.name);
        let _enter = span.enter();

        let mod_id = mod_info.id.clone();
        let mod_dir = Arc::clone(&state.mod_dir);
        let game_dir = Arc::clone(&game_dir);

        let task = async move {
            let from = mod_dir.join(&mod_id);
            let to = game_dir.join("mods").join(&mod_id);

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

fn build_mod_data_lua(state: Arc<ActionState>) -> Result<String> {
    #[derive(Serialize)]
    struct TemplateDataMod {
        id: String,
        name: String,
        bundled: bool,
        version: String,
        init: String,
        data: Option<String>,
        localization: Option<String>,
        packages: Vec<String>,
    }

    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.add_template("mod_data.lua", include_str!("../../assets/mod_data.lua.j2"))
        .wrap_err("Failed to compile template for `mod_data.lua`")?;
    let tmpl = env
        .get_template("mod_data.lua")
        .wrap_err("Failed to get template `mod_data.lua`")?;

    let data: Vec<TemplateDataMod> = state
        .mods
        .iter()
        .filter_map(|m| {
            if !m.enabled {
                return None;
            }

            Some(TemplateDataMod {
                id: m.id.clone(),
                name: m.name.clone(),
                bundled: m.bundled,
                version: m.version.clone(),
                init: m.resources.init.to_string_lossy().to_string(),
                data: m
                    .resources
                    .data
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                localization: m
                    .resources
                    .localization
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                packages: m.packages.iter().map(|p| p.name.clone()).collect(),
            })
        })
        .collect();

    let lua = tmpl
        .render(minijinja::context!(mods => data))
        .wrap_err("Failed to render template `mod_data.lua`")?;

    tracing::debug!("mod_data.lua:\n{}", lua);

    Ok(lua)
}

#[tracing::instrument(skip_all)]
async fn build_bundles(state: Arc<ActionState>) -> Result<(Vec<Bundle>, Vec<String>)> {
    let mut mod_bundle = Bundle::new(MOD_BUNDLE_NAME.to_string());
    let mut tasks = Vec::new();

    let bundle_dir = Arc::new(state.game_dir.join("bundle"));

    let mut bundles = Vec::new();
    let mut data_files = Vec::new();

    let mut add_lua_asset = |name: &str, data: &str| {
        let span = tracing::info_span!("Compiling Lua", name, data_len = data.len());
        let _enter = span.enter();

        let file = lua::compile(name.to_string(), data).wrap_err("Failed to compile Lua")?;

        mod_bundle.add_file(file);

        Ok::<_, Report>(())
    };

    build_mod_data_lua(state.clone())
        .wrap_err("Failed to build 'mod_data.lua'")
        .and_then(|data| add_lua_asset(MOD_DATA_SCRIPT, &data))?;
    add_lua_asset("scripts/mods/init", include_str!("../../assets/init.lua"))?;
    add_lua_asset(
        "scripts/mods/mod_loader",
        include_str!("../../assets/mod_loader.lua"),
    )?;

    tracing::trace!("Preparing tasks to deploy bundle files");

    let previously = previously_deployed_data_files(&bundle_dir).await;

    for mod_info in state.mods.iter().filter(|m| m.enabled && m.bundled) {
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

        // Deploy the mod's external data files, if it has any.
        let data_src = mod_dir.join("data");
        if data_src.is_dir() {
            tracing::trace!("Copying external data files for mod '{}'", mod_info.name);

            for path in copy_dir_all(
                &data_src,
                &bundle_dir.join("data"),
                &bundle_dir,
                &previously,
            )
            .await?
            {
                if let Some(relative) = relative_bundle_path(bundle_dir.as_path(), &path)
                    && !data_files.contains(&relative)
                {
                    data_files.push(relative);
                }
            }
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

        for path in write_external_data_files(&mod_bundle, &bundle_dir).await? {
            if !data_files.contains(&path) {
                data_files.push(path);
            }
        }
    }

    bundles.push(mod_bundle);

    Ok((bundles, data_files))
}

#[tracing::instrument(skip_all)]
async fn patch_boot_bundle(
    state: Arc<ActionState>,
    deployment_info: &str,
) -> Result<(Vec<Bundle>, Vec<String>)> {
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
    .wrap_err_with(|| format!("Failed to read bundle '{BOOT_BUNDLE_NAME}'"))?;

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
        let span = tracing::debug_span!("Importing mod main script");
        let _enter = span.enter();

        let mut env = Environment::new();
        env.set_trim_blocks(true);
        env.set_lstrip_blocks(true);
        env.add_template("mod_main.lua", include_str!("../../assets/mod_main.lua.j2"))
            .wrap_err("Failed to compile template for `mod_main.lua`")?;
        let tmpl = env
            .get_template("mod_main.lua")
            .wrap_err("Failed to get template `mod_main.lua`")?;

        let is_io_enabled = if state.is_io_enabled { "true" } else { "false" };
        let deployment_info = deployment_info.replace("\"", "\\\"").replace("\n", "\\n");
        let lua = tmpl
            .render(minijinja::context!(is_io_enabled => is_io_enabled, deployment_info => deployment_info))
            .wrap_err("Failed to render template `mod_main.lua`")?;

        tracing::trace!("Main script rendered:\n===========\n{}\n=============", lua);
        let file = lua::compile(MOD_BOOT_SCRIPT.to_string(), lua)
            .wrap_err("Failed to compile mod main Lua file")?;

        boot_bundle.add_file(file);
    }

    let data_files = async {
        let bin = boot_bundle
            .to_binary()
            .wrap_err("Failed to serialize boot bundle")?;
        fs::write(&bundle_path, bin)
            .await
            .wrap_err_with(|| format!("Failed to write main bundle: {}", bundle_path.display()))?;

        write_external_data_files(&boot_bundle, &bundle_dir)
            .await
            .wrap_err("Failed to write boot bundle data files")
    }
    .instrument(tracing::trace_span!("write boot bundle"))
    .await?;

    // The boot bundle itself is protected by its own `.bak` sibling, so it is
    // not listed in `data_files`.
    bundles.push(boot_bundle);

    Ok((bundles, data_files))
}

/// Returns the path a file's backup is stored at. Backups append a `.bak`
/// suffix instead of replacing the extension, because the game's streamed data
/// files (e.g. `data/f7/f7b841c5068f2d40`) have no extension.
pub(crate) fn backup_path_for(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

/// Copies `path` to its `.bak` sibling unless a backup already exists, so the
/// original game file survives however many times we overwrite it. Returns
/// whether a backup is available afterwards.
///
/// `mod_owned` marks files that a previous deployment wrote and that were never
/// backed up (because they did not exist before). Backing those up now would
/// capture modded content as if it were the original, which would then be
/// restored on reset.
async fn backup_file(path: &Path, mod_owned: bool) -> Result<bool> {
    let backup = backup_path_for(path);

    if fs::metadata(&backup).await.is_ok() {
        return Ok(true);
    }

    // Nothing to back up: the file does not exist yet, so a reset only has to
    // delete it again.
    if fs::metadata(path).await.is_err() {
        return Ok(false);
    }

    if mod_owned {
        tracing::debug!(
            "Not backing up '{}'; it was written by a previous deployment",
            path.display()
        );
        return Ok(false);
    }

    fs::copy(path, &backup).await.wrap_err_with(|| {
        format!(
            "Failed to back up '{}' to '{}'. Refusing to overwrite a game file \
             without a backup; verify your game files and try again.",
            path.display(),
            backup.display()
        )
    })?;

    tracing::debug!("Backed up '{}' to '{}'", path.display(), backup.display());

    Ok(true)
}

/// Relative paths (to the game's `bundle` directory) that the active
/// deployment wrote. Used to distinguish mod-owned files from game files when
/// backing up before an overwrite.
async fn previously_deployed_data_files(bundle_dir: &Path) -> HashSet<String> {
    let Some(root) = bundle_dir.parent() else {
        return HashSet::new();
    };

    let path = root.join(DEPLOYMENT_DATA_PATH);
    match read_sjson_file::<_, DeploymentData>(&path).await {
        Ok(data) => data.data_files.into_iter().collect(),
        Err(_) => HashSet::new(),
    }
}

/// The content hashes (database, boot bundle) a deployment records so a later
/// deploy can tell the game rewriting those files from our own writes. A file
/// that is absent hashes to zero, keeping the vector's shape stable.
async fn deployment_hashes(bundle_dir: &Path, boot_bundle_path: &str) -> Vec<u64> {
    async fn hash(path: PathBuf) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        match fs::read(&path).await {
            Ok(bytes) => {
                let mut hasher = DefaultHasher::new();
                bytes.hash(&mut hasher);
                hasher.finish()
            }
            Err(_) => 0,
        }
    }

    vec![
        hash(bundle_dir.join(BUNDLE_DATABASE_NAME)).await,
        hash(bundle_dir.join(boot_bundle_path)).await,
    ]
}

/// Whether `relative` is a plain, safe relative path: not absolute and without
/// any parent-directory components, so joining it to the bundle directory can
/// never escape it.
fn is_safe_relative_path(relative: &str) -> bool {
    let path = Path::new(relative);

    !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// The path a written file is recorded under, relative to the bundle directory
/// and with forward slashes so the deployment data stays readable. Returns
/// `None` for paths that would escape the bundle directory.
fn relative_bundle_path(bundle_dir: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(bundle_dir).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");

    if is_safe_relative_path(&relative) {
        Some(relative)
    } else {
        tracing::warn!("Ignoring unsafe data file path '{}'", path.display());
        None
    }
}

/// Writes any external data files referenced by a bundle's variants, such as
/// streamed texture mipmaps, into the game's `bundle` directory.
///
/// Every file that already exists is backed up first. The overwritten paths are
/// returned so they can be recorded in the deployment data and restored on
/// reset.
async fn write_external_data_files(bundle: &Bundle, bundle_dir: &Path) -> Result<Vec<String>> {
    let mut written = Vec::new();
    let previously = previously_deployed_data_files(bundle_dir).await;

    for file in bundle.files() {
        for variant in file.variants() {
            let (Some(name), Some(data)) = (variant.data_file_name(), variant.external_data())
            else {
                continue;
            };

            if !is_safe_relative_path(name) {
                eyre::bail!(
                    "Refusing to write streamed data file with unsafe name '{name}'. \
                     This mod would write outside the game's bundle directory."
                );
            }

            let path = bundle_dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.wrap_err_with(|| {
                    format!("Failed to create directory '{}'", parent.display())
                })?;
            }

            backup_file(&path, previously.contains(name)).await?;

            tracing::trace!("Writing external data file to '{}'", path.display());
            fs::write(&path, data)
                .await
                .wrap_err_with(|| format!("Failed to write '{}'", path.display()))?;

            if let Some(relative) = relative_bundle_path(bundle_dir, &path)
                && !written.contains(&relative)
            {
                written.push(relative);
            }
        }
    }

    Ok(written)
}

/// Recursively copies a directory, used to deploy a mod's external data files
/// (such as streamed texture mipmaps) into the game's `bundle` directory.
///
/// Any destination file that already exists is backed up first. The written
/// destination paths are returned so a reset can restore the originals.
fn copy_dir_all<'a>(
    src: &'a Path,
    dst: &'a Path,
    bundle_dir: &'a Path,
    previously: &'a HashSet<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<PathBuf>>> + Send + 'a>> {
    Box::pin(async move {
        fs::create_dir_all(dst)
            .await
            .wrap_err_with(|| format!("Failed to create '{}'", dst.display()))?;

        let mut written = Vec::new();
        let mut entries = fs::read_dir(src)
            .await
            .wrap_err_with(|| format!("Failed to read '{}'", src.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let dest = dst.join(entry.file_name());

            if entry.file_type().await?.is_dir() {
                written.extend(copy_dir_all(&path, &dest, bundle_dir, previously).await?);
            } else {
                let mod_owned = relative_bundle_path(bundle_dir, &dest)
                    .map(|relative| previously.contains(&relative))
                    .unwrap_or(false);
                backup_file(&dest, mod_owned).await?;

                fs::copy(&path, &dest).await.wrap_err_with(|| {
                    format!(
                        "Failed to copy '{}' to '{}'",
                        path.display(),
                        dest.display()
                    )
                })?;

                written.push(dest);
            }
        }

        Ok(written)
    })
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
fn build_deployment_data(
    bundles: impl AsRef<[Bundle]>,
    mod_folders: impl AsRef<[String]>,
    data_files: impl AsRef<[String]>,
    deployed_hashes: Vec<u64>,
) -> Result<String> {
    let info = DeploymentData {
        timestamp: OffsetDateTime::now_utc(),
        bundles: bundles
            .as_ref()
            .iter()
            .map(|bundle| format!("{:x}", bundle.name().to_murmur64()))
            .collect(),
        // TODO:
        mod_folders: mod_folders.as_ref().to_vec(),
        data_files: data_files.as_ref().to_vec(),
        deployed_hashes,
    };
    serde_sjson::to_string(&info).wrap_err("Failed to serizalize deployment data")
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
            match read_sjson_file::<_, DeploymentData>(&path).await {
                Ok(data) => Ok(Some(data)),
                Err(err) => {
                    if let Some(err) = err.downcast_ref::<std::io::Error>()
                        && err.kind() == ErrorKind::NotFound
                    {
                        Ok(None)
                    } else {
                        Err(err).wrap_err(format!(
                            "Failed to read deployment data from: {}",
                            path.display()
                        ))
                    }
                }
            }
        }
    )
    .wrap_err("Failed to gather deployment information")?;

    let game_info = match game_info {
        Ok(game_info) => game_info,
        Err(err) => {
            tracing::error!("Failed to collect game info: {:#?}", err);
            None
        }
    };

    tracing::debug!(?game_info, ?deployment_info);

    if let Some(game_info) = game_info
        && deployment_info
            .as_ref()
            .map(|i| game_info.last_updated > i.timestamp)
            .unwrap_or(false)
    {
        tracing::warn!(
            "Game was updated since last mod deployment. \
                    Attempting to reconcile game files."
        );

        // The game rewrites the bundle files it owns - a Steam update does, and
        // so does a boot. Our own deployments touch the same files, so "the
        // bundle directory changed" alone means nothing: re-creating the
        // backups then would capture patched content as if it were the game's,
        // and every later deploy would stack another set of dirty patches until
        // the game could no longer resolve its own resource types. Compare the
        // live files against the hashes the last deployment recorded instead;
        // only back up when something other than us wrote them.
        let recorded = deployment_info
            .as_ref()
            .map(|info| info.deployed_hashes.clone())
            .unwrap_or_default();
        let live = deployment_hashes(&bundle_dir, &boot_bundle_path).await;

        if !recorded.is_empty() && recorded == live {
            tracing::info!(
                "Bundle files are exactly as the last deployment wrote them; \
                        skipping the game-update reconciliation."
            );
        } else {
            tokio::try_join!(
                async {
                    let path = bundle_dir.join(BUNDLE_DATABASE_NAME);
                    let backup_path = path.with_extension("data.bak");

                    fs::copy(&path, &backup_path)
                        .await
                        .wrap_err("Failed to re-create backup for bundle database.")
                },
                async {
                    let path = bundle_dir.join(&boot_bundle_path);
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
    let (mut bundles, mut data_files) = build_bundles(state.clone())
        .await
        .wrap_err("Failed to build mod bundles")?;

    // Rendered into the boot script as a record of what is being deployed.
    // The hashes are not known yet (the boot bundle changes below), so this
    // pre-render carries none; the file's copy is built after the writing.
    let script_deployment_info =
        build_deployment_data(&bundles, &mod_folders, &data_files, Vec::new())
            .wrap_err("Failed to build new deployment data")?;

    tracing::info!("Patch boot bundle");
    let (mut boot_bundles, boot_data_files) =
        patch_boot_bundle(state.clone(), &script_deployment_info)
            .await
            .wrap_err("Failed to patch boot bundle")?;
    bundles.append(&mut boot_bundles);

    for path in boot_data_files {
        if !data_files.contains(&path) {
            data_files.push(path);
        }
    }

    // Every file has been written by now, so record the complete deployment for
    // the reset path to undo.
    let boot_bundle_file = format!("{:016x}", Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes()));
    let deployed_hashes = deployment_hashes(&bundle_dir, &boot_bundle_file).await;
    let new_deployment_info =
        build_deployment_data(&bundles, &mod_folders, &data_files, deployed_hashes)
            .wrap_err("Failed to build new deployment data")?;

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
    {
        let path = state.game_dir.join(DEPLOYMENT_DATA_PATH);
        fs::write(&path, &new_deployment_info)
            .await
            .wrap_err_with(|| format!("Failed to write deployment data to '{}'", path.display()))?;
    }

    tracing::info!("Finished deploying mods");
    Ok(())
}
