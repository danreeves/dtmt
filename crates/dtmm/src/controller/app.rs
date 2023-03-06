use std::collections::HashMap;
use std::io::{Cursor, ErrorKind, Read};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Result};
use druid::im::Vector;
use druid::FileInfo;
use dtmt_shared::ModConfig;
use serde::Deserialize;
use tokio::fs::{self, DirEntry};
use tokio::runtime::Runtime;
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;
use zip::ZipArchive;

use crate::state::{ModInfo, PackageInfo, State};
use crate::util::config::{ConfigSerialize, LoadOrderEntry};

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

    let names: Vec<_> = archive.file_names().map(|s| s.to_string()).collect();

    let mod_cfg: ModConfig = {
        let name = names
            .iter()
            .find(|name| name.ends_with("dtmt.cfg"))
            .ok_or_else(|| eyre::eyre!("archive does not contain mod config"))?;

        let mut f = archive
            .by_name(name)
            .wrap_err("failed to read mod config from archive")?;

        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .wrap_err("failed to read mod config from archive")?;

        let data = String::from_utf8(buf).wrap_err("mod config is not valid UTF-8")?;

        serde_sjson::from_str(&data).wrap_err("failed to deserialize mod config")?
    };

    tracing::debug!(?mod_cfg);

    let files: HashMap<String, Vec<String>> = {
        let name = names
            .iter()
            .find(|name| name.ends_with("files.sjson"))
            .ok_or_else(|| eyre::eyre!("archive does not contain file index"))?;

        let mut f = archive
            .by_name(name)
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
        .map(|(name, files)| Arc::new(PackageInfo::new(name, files.into_iter().collect())))
        .collect();
    let info = ModInfo::new(mod_cfg, packages);

    Ok(info)
}

#[tracing::instrument(skip(state))]
pub(crate) async fn delete_mod(state: State, info: &ModInfo) -> Result<()> {
    let mod_dir = state.get_mod_dir().join(&info.id);
    fs::remove_dir_all(&mod_dir)
        .await
        .wrap_err_with(|| format!("failed to remove directory {}", mod_dir.display()))?;

    Ok(())
}

#[tracing::instrument(skip(state))]
pub(crate) async fn save_settings(state: State) -> Result<()> {
    let cfg = ConfigSerialize::from(&state);

    tracing::info!("Saving settings to '{}'", state.config_path.display());
    tracing::debug!(?cfg);

    let data = serde_sjson::to_string(&cfg).wrap_err("failed to serialize config")?;

    fs::write(state.config_path.as_ref(), &data)
        .await
        .wrap_err_with(|| {
            format!(
                "failed to write config to '{}'",
                state.config_path.display()
            )
        })
}

async fn read_sjson_file<P, T>(path: P) -> Result<T>
where
    T: for<'a> Deserialize<'a>,
    P: AsRef<Path> + std::fmt::Debug,
{
    let buf = fs::read(path).await.wrap_err("failed to read file")?;
    let data = String::from_utf8(buf).wrap_err("invalid UTF8")?;
    serde_sjson::from_str(&data).wrap_err("failed to deserialize")
}

#[tracing::instrument(skip_all,fields(
    name = ?res.as_ref().map(|entry| entry.file_name())
))]
async fn read_mod_dir_entry(res: Result<DirEntry>) -> Result<ModInfo> {
    let entry = res?;
    let config_path = entry.path().join("dtmt.cfg");
    let index_path = entry.path().join("files.sjson");

    let cfg: ModConfig = read_sjson_file(&config_path)
        .await
        .wrap_err_with(|| format!("failed to read mod config '{}'", config_path.display()))?;

    let files: HashMap<String, Vec<String>> = read_sjson_file(&index_path)
        .await
        .wrap_err_with(|| format!("failed to read file index '{}'", index_path.display()))?;

    let packages = files
        .into_iter()
        .map(|(name, files)| Arc::new(PackageInfo::new(name, files.into_iter().collect())))
        .collect();
    let info = ModInfo::new(cfg, packages);
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
                    .wrap_err_with(|| format!("failed to open directory '{}'", mod_dir.display()));
            }
        };

        let stream = ReadDirStream::new(read_dir)
            .map(|res| res.wrap_err("failed to read dir entry"))
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
