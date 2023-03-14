use std::collections::HashMap;
use std::io::{Cursor, ErrorKind, Read};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Report, Result};
use druid::im::Vector;
use druid::{FileInfo, ImageBuf};
use dtmt_shared::ModConfig;
use nexusmods::Api as NexusApi;
use tokio::fs::{self, DirEntry};
use tokio::runtime::Runtime;
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;
use zip::ZipArchive;

use crate::state::{ActionState, ModInfo, ModOrder, NexusInfo, PackageInfo};
use crate::util::config::{ConfigSerialize, LoadOrderEntry};

use super::read_sjson_file;

#[tracing::instrument(skip(state))]
pub(crate) async fn import_mod(state: ActionState, info: FileInfo) -> Result<ModInfo> {
    let data = fs::read(&info.path)
        .await
        .wrap_err_with(|| format!("Failed to read file {}", info.path.display()))?;
    let data = Cursor::new(data);

    let nexus = if let Some((_, id, _, _)) = info
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
            Some(NexusInfo::from(mod_info))
        } else {
            None
        }
    } else {
        None
    };

    let mut archive = ZipArchive::new(data).wrap_err("Failed to open ZIP archive")?;

    if tracing::enabled!(tracing::Level::DEBUG) {
        let names = archive.file_names().fold(String::new(), |mut s, name| {
            s.push('\n');
            s.push_str(name);
            s
        });
        tracing::debug!("Archive contents:{}", names);
    }

    let dir_name = {
        let f = archive.by_index(0).wrap_err("Archive is empty")?;

        if !f.is_dir() {
            let err = eyre::eyre!("archive does not have a top-level directory");
            return Err(err).with_suggestion(|| "Use 'dtmt build' to create the mod archive.");
        }

        let name = f.name();
        // The directory name is returned with a trailing slash, which we don't want
        name[..(name.len().saturating_sub(1))].to_string()
    };

    tracing::info!("Importing mod {}", dir_name);

    let names: Vec<_> = archive.file_names().map(|s| s.to_string()).collect();

    let mod_cfg: ModConfig = {
        let name = names
            .iter()
            .find(|name| name.ends_with("dtmt.cfg"))
            .ok_or_else(|| eyre::eyre!("archive does not contain mod config"))?;

        let mut f = archive
            .by_name(name)
            .wrap_err("Failed to read mod config from archive")?;

        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read mod config from archive")?;

        let data = String::from_utf8(buf).wrap_err("Mod config is not valid UTF-8")?;

        serde_sjson::from_str(&data).wrap_err("Failed to deserialize mod config")?
    };

    tracing::debug!(?mod_cfg);

    let files: HashMap<String, Vec<String>> = {
        let name = names
            .iter()
            .find(|name| name.ends_with("files.sjson"))
            .ok_or_else(|| eyre::eyre!("archive does not contain file index"))?;

        let mut f = archive
            .by_name(name)
            .wrap_err("Failed to read file index from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read file index from archive")?;

        let data = String::from_utf8(buf).wrap_err("File index is not valid UTF-8")?;

        serde_sjson::from_str(&data).wrap_err("Failed to deserialize file index")?
    };

    tracing::trace!(?files);

    let image = if let Some(path) = &mod_cfg.image {
        let name = names
            .iter()
            .find(|name| name.ends_with(&path.display().to_string()))
            .ok_or_else(|| eyre::eyre!("archive does not contain configured image file"))?;

        let mut f = archive
            .by_name(name)
            .wrap_err("Failed to read image file from archive")?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("Failed to read file index from archive")?;

        // Druid somehow doesn't return an error compatible with eyre, here.
        // So we have to wrap through `Display` manually.
        let img = match ImageBuf::from_data(&buf) {
            Ok(img) => img,
            Err(err) => {
                let err = Report::msg(err.to_string()).wrap_err("Invalid image data");
                return Err(err).with_suggestion(|| {
                    "Supported formats are: PNG, JPEG, Bitmap and WebP".to_string()
                });
            }
        };

        Some(img)
    } else {
        None
    };

    let mod_dir = state.mod_dir;

    tracing::trace!("Creating mods directory {}", mod_dir.display());
    fs::create_dir_all(Arc::as_ref(&mod_dir))
        .await
        .wrap_err_with(|| format!("Failed to create data directory {}", mod_dir.display()))?;

    tracing::trace!("Extracting mod archive to {}", mod_dir.display());
    archive
        .extract(Arc::as_ref(&mod_dir))
        .wrap_err_with(|| format!("Failed to extract archive to {}", mod_dir.display()))?;

    if let Some(nexus) = &nexus {
        let data = serde_sjson::to_string(nexus).wrap_err("Failed to serialize Nexus info")?;
        let path = mod_dir.join(&mod_cfg.id).join("nexus.sjson");
        fs::write(&path, data.as_bytes())
            .await
            .wrap_err_with(|| format!("Failed to write Nexus info to '{}'", path.display()))?;
    }

    let packages = files
        .into_iter()
        .map(|(name, files)| Arc::new(PackageInfo::new(name, files.into_iter().collect())))
        .collect();
    let info = ModInfo::new(mod_cfg, packages, image, nexus);

    Ok(info)
}

#[tracing::instrument(skip(state))]
pub(crate) async fn delete_mod(state: ActionState, info: &ModInfo) -> Result<()> {
    let mod_dir = state.mod_dir.join(&info.id);
    fs::remove_dir_all(&mod_dir)
        .await
        .wrap_err_with(|| format!("Failed to remove directory {}", mod_dir.display()))?;

    Ok(())
}

#[tracing::instrument(skip(state))]
pub(crate) async fn save_settings(state: ActionState) -> Result<()> {
    let cfg = ConfigSerialize::from(&state);

    tracing::info!("Saving settings to '{}'", state.config_path.display());
    tracing::debug!(?cfg);

    let data = serde_sjson::to_string(&cfg).wrap_err("Failed to serialize config")?;

    fs::write(state.config_path.as_ref(), &data)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to write config to '{}'",
                state.config_path.display()
            )
        })
}

#[tracing::instrument(skip_all,fields(
    name = ?res.as_ref().map(|entry| entry.file_name())
))]
async fn read_mod_dir_entry(res: Result<DirEntry>) -> Result<ModInfo> {
    let entry = res?;
    let config_path = entry.path().join("dtmt.cfg");
    let nexus_path = entry.path().join("nexus.sjson");
    let index_path = entry.path().join("files.sjson");

    let cfg: ModConfig = read_sjson_file(&config_path)
        .await
        .wrap_err_with(|| format!("Failed to read mod config '{}'", config_path.display()))?;

    let nexus: Option<NexusInfo> = match read_sjson_file(&nexus_path)
        .await
        .wrap_err_with(|| format!("Failed to read Nexus info '{}'", nexus_path.display()))
    {
        Ok(nexus) => Some(nexus),
        Err(err) if err.is::<std::io::Error>() => match err.downcast_ref::<std::io::Error>() {
            Some(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            _ => return Err(err),
        },
        Err(err) => return Err(err),
    };

    let files: HashMap<String, Vec<String>> = read_sjson_file(&index_path)
        .await
        .wrap_err_with(|| format!("Failed to read file index '{}'", index_path.display()))?;

    let image = if let Some(path) = &cfg.image {
        let path = entry.path().join(path);
        if let Ok(data) = fs::read(&path).await {
            // Druid somehow doesn't return an error compatible with eyre, here.
            // So we have to wrap through `Display` manually.
            let img = match ImageBuf::from_data(&data) {
                Ok(img) => img,
                Err(err) => {
                    let err = Report::msg(err.to_string());
                    return Err(err)
                        .wrap_err_with(|| {
                            format!("Failed to import image file '{}'", path.display())
                        })
                        .with_suggestion(|| {
                            "Supported formats are: PNG, JPEG, Bitmap and WebP".to_string()
                        });
                }
            };

            Some(img)
        } else {
            None
        }
    } else {
        None
    };

    let packages = files
        .into_iter()
        .map(|(name, files)| Arc::new(PackageInfo::new(name, files.into_iter().collect())))
        .collect();
    let info = ModInfo::new(cfg, packages, image, nexus);
    Ok(info)
}

#[tracing::instrument(skip(mod_order))]
pub(crate) fn load_mods<'a, P, S>(mod_dir: P, mod_order: S) -> Result<Vector<Arc<ModInfo>>>
where
    S: Iterator<Item = &'a LoadOrderEntry>,
    P: AsRef<Path> + std::fmt::Debug,
{
    let rt = Runtime::new()?;

    rt.block_on(async move {
        let mod_dir = mod_dir.as_ref();
        let read_dir = match fs::read_dir(mod_dir).await {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Ok(Vector::new());
            }
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("Failed to open directory '{}'", mod_dir.display()));
            }
        };

        let stream = ReadDirStream::new(read_dir)
            .map(|res| res.wrap_err("Failed to read dir entry"))
            .then(read_mod_dir_entry);
        tokio::pin!(stream);

        let mut mods: HashMap<String, ModInfo> = HashMap::new();

        while let Some(res) = stream.next().await {
            let info = res?;
            mods.insert(info.id.clone(), info);
        }

        let mods = mod_order
            .filter_map(|entry| {
                if let Some(mut info) = mods.remove(&entry.id) {
                    info.enabled = entry.enabled;
                    Some(Arc::new(info))
                } else {
                    None
                }
            })
            .collect();

        Ok::<_, color_eyre::Report>(mods)
    })
}

pub(crate) fn check_mod_order(state: &ActionState) -> Result<()> {
    {
        let first = state.mods.get(0);
        if first.is_none() || !(first.unwrap().id == "dml" && first.unwrap().enabled) {
            // TODO: Add a suggestion where to get it, once that's published
            eyre::bail!("'Darktide Mod Loader' needs to be installed, enabled and at the top of the load order");
        }
    }

    state
        .mods
        .iter()
        .filter(|i| i.enabled)
        .enumerate()
        .for_each(|(i, info)| tracing::debug!(i, ?info));

    for (i, mod_info) in state.mods.iter().filter(|i| i.enabled).enumerate() {
        for dep in &mod_info.depends {
            let dep_info = state.mods.iter().enumerate().find(|(_, m)| m.id == dep.id);

            match dep_info {
                Some((_, dep_info)) if !dep_info.enabled => {
                    eyre::bail!(
                        "Dependency '{}' ({}) must be enabled.",
                        dep_info.name,
                        dep.id
                    );
                }
                Some((j, dep_info)) if dep.order == ModOrder::Before && j >= i => {
                    eyre::bail!(
                        "Dependency '{}' ({}) must be loaded before '{}'",
                        dep_info.name,
                        dep.id,
                        mod_info.name
                    );
                }
                Some((j, dep_info)) if dep.order == ModOrder::After && j <= i => {
                    eyre::bail!(
                        "Dependency '{}' ({}) must be loaded after '{}'",
                        dep_info.name,
                        dep.id,
                        mod_info.name
                    );
                }
                None => {
                    eyre::bail!(
                        "Missing dependency '{}' for mod '{}'",
                        dep.id,
                        mod_info.name
                    );
                }
                Some(_) => {
                    // All good
                }
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip(info, api), fields(id = info.id, name = info.name, version = info.version))]
async fn check_mod_update(info: Arc<ModInfo>, api: Arc<NexusApi>) -> Result<Option<ModInfo>> {
    let Some(nexus) = &info.nexus else {
        return Ok(None);
    };

    let updated_info = api
        .mods_id(nexus.id)
        .await
        .wrap_err_with(|| format!("Failed to query mod {} from Nexus", nexus.id))?;

    let mut info = Arc::unwrap_or_clone(info);
    info.nexus = Some(NexusInfo::from(updated_info));

    Ok(Some(info))
}

#[tracing::instrument(skip(state))]
pub(crate) async fn check_updates(state: ActionState) -> Result<Vec<ModInfo>> {
    if state.nexus_api_key.is_empty() {
        eyre::bail!("Nexus API key not set. Cannot check for updates.");
    }

    let api = NexusApi::new(state.nexus_api_key.to_string())
        .wrap_err("Failed to initialize Nexus API")?;
    let api = Arc::new(api);

    let tasks = state
        .mods
        .iter()
        .map(|info| check_mod_update(info.clone(), api.clone()));

    let results = futures::future::join_all(tasks).await;
    let updates = results
        .into_iter()
        .filter_map(|res| match res {
            Ok(info) => info,
            Err(err) => {
                tracing::error!("{:?}", err);
                None
            }
        })
        .collect();
    Ok(updates)
}
