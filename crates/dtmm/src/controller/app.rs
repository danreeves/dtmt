use std::collections::HashMap;
use std::io::{Cursor, Read};

use color_eyre::eyre::{self, Context};
use color_eyre::{Help, Result};
use druid::FileInfo;
use dtmt_shared::ModConfig;
use tokio::fs;
use zip::ZipArchive;

use crate::state::{ModInfo, PackageInfo, State};
use crate::util::config::Config;

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
    // TODO: Avoid allocations, especially once the config grows, by
    // creating a separate struct with only borrowed data to serialize from.
    let cfg = Config {
        path: state.config_path.as_ref().clone(),
        game_dir: Some(state.game_dir.as_ref().clone()),
        data_dir: Some(state.data_dir.as_ref().clone()),
    };

    tracing::info!("Saving settings to '{}'", state.config_path.display());
    tracing::debug!(?cfg);

    let data = serde_sjson::to_string(&cfg).wrap_err("failed to serialize config")?;

    fs::write(&cfg.path, &data)
        .await
        .wrap_err_with(|| format!("failed to write config to '{}'", cfg.path.display()))
}
