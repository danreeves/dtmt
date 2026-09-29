use std::io::{self, ErrorKind};
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::Result;
use sdk::murmur::Murmur64;
use time::OffsetDateTime;
use tokio::fs::{self};

use crate::controller::deploy::{
    BOOT_BUNDLE_NAME, BUNDLE_DATABASE_NAME, DEPLOYMENT_DATA_PATH, DeploymentData,
};
use crate::state::ActionState;

use super::deploy::{SETTINGS_FILE_PATH, backup_path_for, set_boot_script};

/// The file's modification time when it is newer than `timestamp`, i.e. when
/// something wrote it after the deployment. Such a file is not the copy this
/// deployment patched, so a reset must not put an older backup over it.
async fn modified_after(
    path: &std::path::Path,
    timestamp: OffsetDateTime,
) -> Option<OffsetDateTime> {
    let metadata = fs::metadata(path).await.ok()?;
    let modified: OffsetDateTime = metadata.modified().ok()?.into();
    (modified > timestamp).then_some(modified)
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

    for name in &info.bundles {
        let path = bundle_dir.join(name);

        match fs::remove_file(&path).await {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::error!("Failed to remove '{}': {:?}", path.display(), err);
            }
        };
    }

    // The settings file is handled separately below: restoring it from its
    // backup would also put back the version the game had when the backup was
    // taken, which is older than the game after an update.
    for p in paths.iter().filter(|p| **p != SETTINGS_FILE_PATH) {
        let p = *p;
        let path = bundle_dir.join(p);
        let backup = bundle_dir.join(format!("{p}.bak"));

        // If the game (or Steam) wrote this file after the deployment, it is
        // not our patched copy any more; restoring the backup would replace a
        // newer file with an older one. Leave it alone instead.
        if let Some(modified) = modified_after(&path, info.timestamp).await {
            tracing::warn!(
                "'{}' was written after the deployment ({}); leaving it as it is and removing \
                 the stale backup '{}'. Verify the game files in Steam if the game does not \
                 start.",
                path.display(),
                modified,
                backup.display()
            );
            let _ = fs::remove_file(&backup).await;
            continue;
        }

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

    // Undo the boot script in the current settings file rather than restoring
    // the backup: the file also carries the client version the backend checks,
    // and the game may have updated it since the backup was taken.
    {
        let settings_path = bundle_dir.join(SETTINGS_FILE_PATH);
        if fs::metadata(&settings_path).await.is_ok() {
            tracing::debug!("Undoing the boot script in '{}'", settings_path.display());
            if let Err(err) = set_boot_script(&settings_path, "scripts/main").await {
                tracing::error!(
                    "Failed to undo the boot script in '{}'. You may need to verify game files. \
                     Error: {:?}",
                    settings_path.display(),
                    err
                );
            }
        }
        let backup = backup_path_for(&settings_path);
        match fs::remove_file(&backup).await {
            Ok(_) => tracing::debug!("Removed stale backup '{}'", backup.display()),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => tracing::error!("Failed to remove '{}': {:?}", backup.display(), err),
        }
    }

    // Restore the game files under `bundle/` that this deployment overwrote,
    // such as streamed texture mipmaps. Each file that already existed was
    // backed up next to itself as `<name>.bak` before it was first written.
    for relative in &info.data_files {
        let path = bundle_dir.join(relative);
        let backup = backup_path_for(&path);

        // As above: never put an older backup over a file the game rewrote
        // after the deployment.
        if let Some(modified) = modified_after(&path, info.timestamp).await {
            tracing::warn!(
                "'{}' was written after the deployment ({}); leaving it as it is and removing \
                 the stale backup '{}'. Verify the game files in Steam if the game does not \
                 start.",
                path.display(),
                modified,
                backup.display()
            );
            let _ = fs::remove_file(&backup).await;
            continue;
        }

        let res = async {
            if fs::metadata(&backup).await.is_ok() {
                tracing::debug!(
                    "Restoring '{}' from backup '{}'",
                    path.display(),
                    backup.display()
                );

                fs::copy(&backup, &path).await.wrap_err_with(|| {
                    format!(
                        "Failed to restore '{}' from '{}'",
                        path.display(),
                        backup.display()
                    )
                })?;

                fs::remove_file(&backup)
                    .await
                    .wrap_err_with(|| format!("Failed to remove backup '{}'", backup.display()))?;
            } else {
                // Without a backup the file did not exist before this
                // deployment, so it belongs to us and can simply be removed.
                tracing::debug!("Removing deployed file '{}'", path.display());

                match fs::remove_file(&path).await {
                    Ok(_) => {}
                    Err(err) if err.kind() == ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(err)
                            .wrap_err_with(|| format!("Failed to remove '{}'", path.display()))
                    }
                }
            }

            Ok(())
        }
        .await;

        if let Err(err) = res {
            tracing::error!(
                "Failed to restore '{}'. You may need to verify game files. Error: {:?}",
                relative,
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
