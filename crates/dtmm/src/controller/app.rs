use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Report, Result};
use druid::im::Vector;
use druid::ImageBuf;
use dtmt_shared::ModConfig;
use nexusmods::Api as NexusApi;
use tokio::fs::{self, DirEntry, File};
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;

use crate::state::{ActionState, InitialLoadResult, ModInfo, ModOrder, NexusInfo, PackageInfo};
use crate::util;
use crate::util::config::{ConfigSerialize, LoadOrderEntry};

use super::read_sjson_file;

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
                "Failed to write config to '{}'",
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

    let files: HashMap<String, Vec<String>> = if cfg.bundled {
        read_sjson_file(&index_path)
            .await
            .wrap_err_with(|| format!("Failed to read file index '{}'", index_path.display()))?
    } else {
        Default::default()
    };

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
pub(crate) async fn load_mods<'a, P, S>(mod_dir: P, mod_order: S) -> Result<Vector<Arc<ModInfo>>>
where
    S: Iterator<Item = &'a LoadOrderEntry>,
    P: AsRef<Path> + std::fmt::Debug,
{
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

    Ok(mods)
}

pub(crate) fn check_mod_order(state: &ActionState) -> Result<()> {
    if tracing::enabled!(tracing::Level::DEBUG) {
        let order = state
            .mods
            .iter()
            .enumerate()
            .filter(|(_, i)| i.enabled)
            .fold(String::new(), |mut s, (i, info)| {
                s.push_str(&format!("{}: {} - {}\n", i, info.id, info.name));
                s
            });

        tracing::debug!("Mod order:\n{}", order);
    }

    for (i, mod_info) in state.mods.iter().enumerate().filter(|(_, i)| i.enabled) {
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

pub(crate) async fn load_initial(path: PathBuf, is_default: bool) -> Result<InitialLoadResult> {
    let config = util::config::read_config(path, is_default)
        .await
        .wrap_err("Failed to read config file")?;

    // Create or truncate the log file
    let log_path = config.data_dir.join("dtmm.log");
    tokio::spawn(async move {
        let _ = File::create(&log_path).await;
        tracing::debug!("Truncated log file");
    });

    let game_info = tokio::task::spawn_blocking(dtmt_shared::collect_game_info)
        .await
        .wrap_err("Failed to spawn task to collect Steam game info")?;

    let game_info = match game_info {
        Ok(game_info) => game_info,
        Err(err) => {
            tracing::error!("Failed to collect game info: {:?}", err);
            None
        }
    };

    if config.game_dir.is_none() && game_info.is_none() {
        tracing::error!("No Game Directory set. Head to the 'Settings' tab to set it manually",);
    }

    let mod_dir = config.data_dir.join("mods");
    let mods = load_mods(mod_dir, config.mod_order.iter())
        .await
        .wrap_err("Failed to load mods")?;

    Ok((config, mods))
}
