use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::{eyre, Result};
use sdk::murmur::Murmur64;
use tokio::fs::{self};
use tokio::io::AsyncWriteExt;

use crate::controller::deploy::{
    DeploymentData, BOOT_BUNDLE_NAME, BUNDLE_DATABASE_NAME, DEPLOYMENT_DATA_PATH,
};
use crate::state::ActionState;

use super::deploy::SETTINGS_FILE_PATH;

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

    f.write_all(&settings.as_bytes()[0..i]).await?;
    f.write_all(b"boot_script = \"scripts/mod_main\"").await?;

    let Some(j) = settings[i..].find('\n') else {
        eyre::bail!("couldn't find end of 'boot_script' field");
    };

    f.write_all(&settings.as_bytes()[(i + j)..]).await?;

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn reset_dtkit_patch(state: ActionState) -> Result<()> {
    let bundle_dir = state.game_dir.join("bundle");

    {
        let path = bundle_dir.join(BUNDLE_DATABASE_NAME);
        let backup_path = path.with_extension("data.bak");
        fs::rename(&backup_path, &path).await.wrap_err_with(|| {
            format!(
                "Failed to move bundle database backup '{}' -> '{}'",
                backup_path.display(),
                path.display()
            )
        })?;
        tracing::trace!("Reverted bundle database from backup");
    }

    for path in [
        bundle_dir.join(format!(
            "{:016x}.patch_999",
            Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes())
        )),
        state.game_dir.join("binaries/mod_loader"),
        state.game_dir.join("toggle_darktide_mods.bat"),
        state.game_dir.join("README.md"),
    ] {
        match fs::remove_file(&path).await {
            Ok(_) => tracing::trace!("Removed file '{}'", path.display()),
            Err(err) if err.kind() != io::ErrorKind::NotFound => {
                tracing::error!("Failed to remove file '{}': {}", path.display(), err)
            }
            Err(_) => {}
        }
    }

    // We deliberately skip the `mods/` directory here.
    // Many modders did their development right in there, and as people are prone to not read
    // error messages and guides in full, there is bound to be someone who would have
    // deleted all their source code if this removed the `mods/` folder.
    for path in [state.game_dir.join("tools")] {
        match fs::remove_dir_all(&path).await {
            Ok(_) => tracing::trace!("Removed directory '{}'", path.display()),
            Err(err) if err.kind() != io::ErrorKind::NotFound => {
                tracing::error!("Failed to remove directory '{}': {}", path.display(), err)
            }
            Err(_) => {}
        }
    }

    tracing::info!("Removed dtkit-patch-based mod installation.");
    Ok(())
}

#[tracing::instrument(skip(state))]
pub(crate) async fn reset_mod_deployment(state: ActionState) -> Result<()> {
    let boot_bundle_path = format!("{:016x}", Murmur64::hash(BOOT_BUNDLE_NAME.as_bytes()));
    let paths = [BUNDLE_DATABASE_NAME, &boot_bundle_path, SETTINGS_FILE_PATH];
    let bundle_dir = state.game_dir.join("bundle");

    tracing::info!("Resetting mod deployment in {}", bundle_dir.display());

    if fs::metadata(bundle_dir.join(format!("{boot_bundle_path}.patch_999")))
        .await
        .is_ok()
    {
        tracing::info!("Found dtkit-patch-based mod installation. Removing.");
        return reset_dtkit_patch(state).await;
    }

    tracing::debug!("Reading mod deployment");

    let info: DeploymentData = {
        let path = state.game_dir.join(DEPLOYMENT_DATA_PATH);
        let data = match fs::read(&path).await {
            Ok(data) => data,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                tracing::info!("No deployment to reset");
                return Ok(());
            }
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!("Failed to read deployment info at '{}'", path.display())
                });
            }
        };

        let data = String::from_utf8(data).wrap_err("Invalid UTF8 in deployment data")?;

        serde_sjson::from_str(&data).wrap_err("Invalid SJSON in deployment data")?
    };

    for name in info.bundles {
        let path = bundle_dir.join(name);

        match fs::remove_file(&path).await {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::error!("Failed to remove '{}': {:?}", path.display(), err);
            }
        };
    }

    for p in paths {
        let path = bundle_dir.join(p);
        let backup = bundle_dir.join(format!("{}.bak", p));

        let res = async {
            tracing::debug!(
                "Copying from backup: {} -> {}",
                backup.display(),
                path.display()
            );

            fs::copy(&backup, &path)
                .await
                .wrap_err_with(|| format!("Failed to copy from '{}'", backup.display()))?;

            tracing::debug!("Deleting backup: {}", backup.display());

            match fs::remove_file(&backup).await {
                Ok(_) => Ok(()),
                Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
                Err(err) => {
                    Err(err).wrap_err_with(|| format!("Failed to remove '{}'", backup.display()))
                }
            }
        }
        .await;

        if let Err(err) = res {
            tracing::error!(
                "Failed to restore '{}' from backup. You may need to verify game files. Error: {:?}",
                &p,
                err
            );
        }
    }

    {
        let path = state.game_dir.join(DEPLOYMENT_DATA_PATH);
        if let Err(err) = fs::remove_file(&path).await {
            tracing::error!(
                "Failed to remove deployment data '{}': {:?}",
                path.display(),
                err
            );
        }
    }

    tracing::info!("Reset finished");

    Ok(())
}
