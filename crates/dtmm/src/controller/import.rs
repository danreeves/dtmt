use std::collections::HashMap;
use std::ffi::CStr;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Report, Result};
use druid::im::Vector;
use druid::{FileInfo, ImageBuf};
use dtmt_shared::{ModConfig, ModConfigResources};
use luajit2_sys as lua;
use nexusmods::Api as NexusApi;
use tokio::fs;
use zip::ZipArchive;

use crate::state::{ActionState, ModInfo, NexusInfo, PackageInfo};

fn find_archive_file<R: Read + Seek>(
    archive: &ZipArchive<R>,
    name: impl AsRef<str>,
) -> Option<String> {
    let path = archive
        .file_names()
        .find(|path| path.ends_with(name.as_ref()))
        .map(|s| s.to_string());
    path
}

fn image_data_to_buffer(data: impl AsRef<[u8]>) -> Result<ImageBuf> {
    // Druid somehow doesn't return an error compatible with eyre, here.
    // So we have to wrap through `Display` manually.
    ImageBuf::from_data(data.as_ref()).map_err(|err| {
        Report::msg(err.to_string())
            .wrap_err("Invalid image data")
            .suggestion("Supported formats are: PNG, JPEG, Bitmap and WebP")
    })
}

// Runs the content of a `.mod` file to extract what data we can get
// from legacy mods.
// 1. Create a global function `new_mod` that stores
// the relevant bits in global variables.
// 2. Run the `.mod` file, which will return a table.
// 3. Run the `run` function from that table.
// 4. Access the global variables from #1.
#[tracing::instrument]
fn parse_mod_id_file(data: &str) -> Result<(String, ModConfigResources)> {
    tracing::debug!("Parsing mod file:\n{}", data);

    let ret = unsafe {
        let state = lua::luaL_newstate();
        lua::luaL_openlibs(state);

        let run = b"
function fassert() end
function new_mod(id, resources)
    _G.id = id
    _G.script = resources.mod_script
    _G.data = resources.mod_data
    _G.localization = resources.mod_localization
end
\0";
        match lua::luaL_loadstring(state, run.as_ptr() as _) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to create `new_mod`")
            }
            _ => unreachable!(),
        }

        match lua::lua_pcall(state, 0, 0, 0) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRRUN => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Failed to run buffer: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to run buffer")
            }
            // We don't use an error handler function, so this should be unreachable
            lua::LUA_ERRERR => unreachable!(),
            _ => unreachable!(),
        }

        let name = b".mod\0";
        match lua::luaL_loadbuffer(
            state,
            data.as_ptr() as _,
            data.len() as _,
            name.as_ptr() as _,
        ) as u32
        {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to load `.mod` file buffer")
            }
            _ => unreachable!(),
        }

        match lua::lua_pcall(state, 0, 1, 0) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRRUN => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Failed to run `.mod` file: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to run `.mod` file")
            }
            // We don't use an error handler function, so this should be unreachable
            lua::LUA_ERRERR => unreachable!(),
            _ => unreachable!(),
        }

        let key = b"run\0";
        lua::lua_pushstring(state, key.as_ptr() as _);
        lua::lua_gettable(state, -2);

        match lua::lua_pcall(state, 0, 0, 0) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRRUN => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Failed to run `.mod.run`: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to run `.mod.run`")
            }
            // We don't use an error handler function, so this should be unreachable
            lua::LUA_ERRERR => unreachable!(),
            _ => unreachable!(),
        }

        let get_global = |state, key: &[u8]| {
            lua::lua_getglobal(state, key.as_ptr() as _);

            if lua::lua_isnil(state, -1) != 0 {
                return Ok(None);
            }

            let s = lua::lua_tostring(state, -1);

            if s.is_null() {
                eyre::bail!("Expected string, got NULL");
            }

            let ret = CStr::from_ptr(s).to_string_lossy().to_string();
            lua::lua_pop(state, 1);
            Ok(Some(ret))
        };

        let mod_id = get_global(state, b"id\0")
            .and_then(|s| s.ok_or_else(|| eyre::eyre!("Got `nil`")))
            .wrap_err("Failed to get `id`")?;

        let resources = ModConfigResources {
            init: get_global(state, b"script\0")
                .and_then(|s| s.map(PathBuf::from).ok_or_else(|| eyre::eyre!("Got `nil`")))
                .wrap_err("Failed to get `script`.")?,
            data: get_global(state, b"data\0")
                .wrap_err("Failed to get `data`.")?
                .map(PathBuf::from),
            localization: get_global(state, b"localization\0")
                .wrap_err("Failed to get `localization`")?
                .map(PathBuf::from),
        };

        lua::lua_close(state);

        (mod_id, resources)
    };

    Ok(ret)
}

// Extracts the mod configuration from the mod archive.
// This may either be a proper `dtmt.cfg`, or the legacy `<mod_name>.mod` ID file.
//
// It also returns the directory where this file was found, used as root path. This
// allows flexibility in what the directory structure is exactly, since many people
// still end up creating tarbombs and Nexus does its own re-packaging.
#[tracing::instrument(skip(archive))]
fn extract_mod_config<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(ModConfig, String)> {
    let legacy_mod_data = if let Some(name) = find_archive_file(archive, ".mod") {
        let (mod_id, resources) = {
            let mut f = archive
                .by_name(&name)
                .wrap_err("Failed to read `.mod` file from archive")?;

            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)
                .wrap_err("Failed to read `.mod` file from archive")?;

            let data = String::from_utf8(buf).wrap_err("`.mod` file is not valid UTF-8")?;
            parse_mod_id_file(&data)
                .wrap_err("Invalid `.mod` file")
                .note(
                    "The `.mod` file's `run` function may not contain any additional logic \
                    besides the default.",
                )
                .suggestion("Contact the mod author to fix this.")?
        };

        let root = if let Some(index) = name.rfind('/') {
            name[..index].to_string()
        } else {
            String::new()
        };

        Some((mod_id, resources, root))
    } else {
        None
    };

    tracing::debug!(?legacy_mod_data);

    if let Some(name) = find_archive_file(archive, "dtmt.cfg") {
        let mut f = archive
            .by_name(&name)
            .wrap_err("Failed to read mod config from archive")?;

        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read mod config from archive")?;

        let data = String::from_utf8(buf).wrap_err("Mod config is not valid UTF-8")?;

        let mut cfg: ModConfig = serde_sjson::from_str(&data)
            .wrap_err("Failed to deserialize mod config")
            .suggestion("Contact the mod author to fix this.")?;

        if let Some((mod_id, resources, root)) = legacy_mod_data {
            if cfg.id != mod_id {
                let err = eyre::eyre!("Mod ID in `dtmt.cfg` does not match mod ID in `.mod` file");
                return Err(err).suggestion("Contact the mod author to fix this.");
            }

            cfg.resources = resources;

            // Enforce that packages are skipped
            cfg.bundled = false;
            cfg.packages = vec![];

            Ok((cfg, root))
        } else {
            let root = name
                .strip_suffix("dtmt.cfg")
                .expect("String must end with that suffix")
                .to_string();

            Ok((cfg, root))
        }
    } else if let Some((mod_id, resources, root)) = legacy_mod_data {
        let cfg = ModConfig {
            bundled: false,
            dir: PathBuf::new(),
            id: mod_id.clone(),
            name: mod_id,
            summary: "A mod for the game Warhammer 40,000: Darktide".into(),
            version: "N/A".into(),
            description: None,
            author: None,
            image: None,
            categories: Vec::new(),
            packages: Vec::new(),
            resources,
            depends: Vec::new(),
            name_overrides: Default::default(),
        };

        Ok((cfg, root))
    } else {
        eyre::bail!(
            "Mod needs a config file or `.mod` file. \
                Please get in touch with the author to provide a properly packaged mod."
        );
    }
}

#[tracing::instrument(skip(archive))]
fn extract_bundled_mod<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: String,
    dest: impl AsRef<Path> + std::fmt::Debug,
) -> Result<Vector<Arc<PackageInfo>>> {
    let files: HashMap<String, Vec<String>> = {
        let name = archive
            .file_names()
            .find(|name| name.ends_with("files.sjson"))
            .map(|s| s.to_string())
            .ok_or_else(|| eyre::eyre!("archive does not contain file index"))?;

        let mut f = archive
            .by_name(&name)
            .wrap_err("Failed to read file index from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read file index from archive")?;

        let data = String::from_utf8(buf).wrap_err("File index is not valid UTF-8")?;
        serde_sjson::from_str(&data).wrap_err("Failed to deserialize file index")?
    };

    tracing::trace!(?files);

    let dest = dest.as_ref();
    tracing::trace!("Extracting mod archive to {}", dest.display());
    archive
        .extract(dest)
        .wrap_err_with(|| format!("Failed to extract archive to {}", dest.display()))?;

    let packages = files
        .into_iter()
        .map(|(name, files)| Arc::new(PackageInfo::new(name, files.into_iter().collect())))
        .collect();

    tracing::trace!(?packages);

    Ok(packages)
}

#[tracing::instrument(skip(archive))]
fn extract_legacy_mod<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: String,
    dest: impl Into<PathBuf> + std::fmt::Debug,
) -> Result<()> {
    let dest = dest.into();
    let file_count = archive.len();

    for i in 0..file_count {
        let mut f = archive
            .by_index(i)
            .wrap_err_with(|| format!("Failed to get file at index {}", i))?;

        let Some(name) = f.enclosed_name().map(|p| p.to_path_buf()) else {
            let err = eyre::eyre!("File name in archive is not a safe path value.").suggestion(
                "Only use well-known applications to create the ZIP archive, \
                and don't create paths that point outside the archive directory.",
            );
            return Err(err);
        };

        let Ok(suffix) = name.strip_prefix(&root) else {
            tracing::warn!(
                "Skipping file outside of the mod root directory: {}",
                name.display()
            );
            continue;
        };
        let name = dest.join(suffix);

        if f.is_dir() {
            // The majority of errors will actually be "X already exists".
            // But rather than filter them invidually, we just ignore all of them.
            // If there is a legitimate error of "couldn't create X", it will eventually fail when
            // we try to put a file in there.
            tracing::trace!("Creating directory '{}'", name.display());
            let _ = std::fs::create_dir_all(&name);
        } else {
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)
                .wrap_err_with(|| format!("Failed to read file '{}'", name.display()))?;

            tracing::trace!("Writing file '{}'", name.display());
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&name)
                .wrap_err_with(|| format!("Failed to open file '{}'", name.display()))?;

            out.write_all(&buf)
                .wrap_err_with(|| format!("Failed to write to '{}'", name.display()))?;
        }
    }

    Ok(())
}

#[tracing::instrument(skip(state))]
pub(crate) async fn import_from_file(state: ActionState, info: FileInfo) -> Result<ModInfo> {
    let data = fs::read(&info.path)
        .await
        .wrap_err_with(|| format!("Failed to read file {}", info.path.display()))?;

    let nexus = if let Some((_, id, version, timestamp)) = info
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(NexusApi::parse_file_name)
    {
        if !state.nexus_api_key.is_empty() {
            let api = NexusApi::new(state.nexus_api_key.to_string())?;
            let mod_info = api
                .mods_id(id)
                .await
                .wrap_err_with(|| format!("Failed to query mod {} from Nexus", id))?;

            let version = match api.file_version(id, timestamp).await {
                Ok(version) => version,
                Err(err) => {
                    let err = Report::new(err);
                    tracing::warn!(
                        "Failed to fetch version for Nexus download. \
                        Falling back to file name:\n{:?}",
                        err
                    );
                    version
                }
            };

            let info = NexusInfo::from(mod_info);
            tracing::debug!(version, ?info);

            Some((info, version))
        } else {
            None
        }
    } else {
        None
    };

    tracing::trace!(?nexus);

    import_mod(state, nexus, data).await
}

#[tracing::instrument(skip(state))]
pub(crate) async fn import_from_nxm(state: ActionState, uri: String) -> Result<ModInfo> {
    let url = uri
        .parse()
        .wrap_err_with(|| format!("Invalid Uri '{}'", uri))?;

    let api = NexusApi::new(state.nexus_api_key.to_string())?;
    let (mod_info, file_info, data) = api
        .handle_nxm(url)
        .await
        .wrap_err_with(|| format!("Failed to download mod from NXM uri '{}'", uri))?;

    let nexus = NexusInfo::from(mod_info);
    import_mod(state, Some((nexus, file_info.version)), data).await
}

#[tracing::instrument(skip(state, data), fields(data = data.len()))]
pub(crate) async fn import_mod(
    state: ActionState,
    nexus: Option<(NexusInfo, String)>,
    data: Vec<u8>,
) -> Result<ModInfo> {
    let data = Cursor::new(data);
    let mut archive = ZipArchive::new(data).wrap_err("Failed to open ZIP archive")?;

    if tracing::enabled!(tracing::Level::DEBUG) {
        let names = archive.file_names().fold(String::new(), |mut s, name| {
            s.push('\n');
            s.push_str(name);
            s
        });
        tracing::debug!("Archive contents:{}", names);
    }

    let (mut mod_cfg, root) =
        extract_mod_config(&mut archive).wrap_err("Failed to extract mod configuration")?;
    tracing::info!("Importing mod {} ({})", mod_cfg.name, mod_cfg.id);

    let mod_dir = state.data_dir.join(state.mod_dir.as_ref());
    let dest = mod_dir.join(&mod_cfg.id);
    tracing::trace!("Creating mods directory {}", dest.display());
    fs::create_dir_all(&dest)
        .await
        .wrap_err_with(|| format!("Failed to create data directory '{}'", dest.display()))?;

    let image = if let Some(path) = &mod_cfg.image {
        let name = archive
            .file_names()
            .find(|name| name.ends_with(&path.display().to_string()))
            .map(|s| s.to_string())
            .ok_or_else(|| eyre::eyre!("archive does not contain configured image file"))?;

        let mut f = archive
            .by_name(&name)
            .wrap_err("Failed to read image file from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read file index from archive")?;

        let img = image_data_to_buffer(buf)?;
        Some(img)
    } else if let Some((nexus, _)) = &nexus {
        let api = NexusApi::new(state.nexus_api_key.to_string())?;
        let url = nexus.picture_url.as_ref();
        let data = api
            .picture(url)
            .await
            .wrap_err_with(|| format!("Failed to download Nexus image from '{}'", url))?;

        let img = image_data_to_buffer(&data)?;

        let name = "image.bin";
        let path = dest.join(name);
        match fs::write(&path, &data).await {
            Ok(_) => {
                mod_cfg.image = Some(name.into());
                Some(img)
            }
            Err(err) => {
                let err = Report::new(err).wrap_err(format!(
                    "Failed to write Nexus picture to file '{}'",
                    path.display()
                ));
                tracing::error!("{:?}", err);
                None
            }
        }
    } else {
        None
    };

    tracing::trace!(?image);
    tracing::debug!(root, ?mod_cfg);

    let packages = if mod_cfg.bundled {
        extract_bundled_mod(&mut archive, root, &mod_dir).wrap_err("Failed to extract mod")?
    } else {
        extract_legacy_mod(&mut archive, root, &dest).wrap_err("Failed to extract legacy mod")?;

        if let Some((_, version)) = &nexus {
            // We use the version number stored in the `ModInfo` to compare against the `NexusInfo`
            // for version checks. So for this one, we can't actually rely on merely shadowing,
            // like with the other fields.
            mod_cfg.version = version.clone();
        }

        let data = serde_sjson::to_string(&mod_cfg).wrap_err("Failed to serialize mod config")?;
        fs::write(dest.join("dtmt.cfg"), &data)
            .await
            .wrap_err("Failed to write mod config")?;

        Default::default()
    };

    if let Some((nexus, _)) = &nexus {
        let data = serde_sjson::to_string(nexus).wrap_err("Failed to serialize Nexus info")?;
        let path = dest.join("nexus.sjson");
        fs::write(&path, data.as_bytes())
            .await
            .wrap_err_with(|| format!("Failed to write Nexus info to '{}'", path.display()))?;
    }

    let info = ModInfo::new(mod_cfg, packages, image, nexus.map(|(info, _)| info));
    Ok(info)
}
