use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use futures::{Stream, StreamExt};
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

#[tracing::instrument]
pub async fn foo<P>(path: P) -> Vec<PathBuf>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let dir = match fs::read_dir(path.as_ref()).await {
        Ok(dir) => {
            tracing::trace!(is_dir = true);
            dir
        }
        Err(err) => {
            if err.kind() != io::ErrorKind::NotADirectory {
                tracing::error!("Failed to read path: {:?}", err);
            }
            let paths = vec![PathBuf::from(path.as_ref())];
            tracing::debug!(is_dir = false, resolved_paths = ?paths);
            return paths;
        }
    };

    let stream = ReadDirStream::new(dir);
    let paths: Vec<PathBuf> = stream
        .filter_map(|entry| async move {
            if let Ok(path) = entry.map(|e| e.path()) {
                match path.file_name().and_then(OsStr::to_str) {
                    Some(name) if name.len() == 16 => {
                        if name.chars().all(|c| c.is_ascii_hexdigit()) {
                            Some(path)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect()
        .await;

    tracing::debug!(resolved_paths = ?paths);

    paths
}

pub async fn resolve_bundle_path<P>(path: P) -> Pin<Box<dyn Stream<Item = PathBuf>>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let dir = match fs::read_dir(path.as_ref()).await {
        Ok(dir) => {
            tracing::trace!(is_dir = true);
            dir
        }
        Err(err) => {
            if err.kind() != io::ErrorKind::NotADirectory {
                tracing::error!("Failed to read path: {:?}", err);
            }
            let paths = vec![PathBuf::from(path.as_ref())];
            tracing::debug!(is_dir = false, resolved_paths = ?paths);
            return Box::pin(futures::stream::iter(paths));
        }
    };

    let stream = ReadDirStream::new(dir);
    let stream = stream.filter_map(|entry| async move {
        if let Ok(path) = entry.map(|e| e.path()) {
            match path.file_name().and_then(OsStr::to_str) {
                Some(name) if name.len() == 16 => {
                    if name.chars().all(|c| c.is_ascii_hexdigit()) {
                        Some(path)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    });
    Box::pin(stream)
}

#[tracing::instrument(skip_all)]
pub async fn collect_bundle_paths<I>(paths: I) -> Vec<PathBuf>
where
    I: Iterator<Item = PathBuf> + std::fmt::Debug,
{
    let tasks = paths.map(|p| async move {
        match tokio::spawn(async move { foo(&p).await }).await {
            Ok(paths) => paths,
            Err(err) => {
                tracing::error!(%err, "failed to spawn task to resolve bundle paths");
                vec![]
            }
        }
    });

    let results = futures_util::future::join_all(tasks).await;
    results.into_iter().flatten().collect()
}

#[tracing::instrument(skip_all)]
pub fn resolve_bundle_paths<I>(paths: I) -> impl Stream<Item = PathBuf>
where
    I: Iterator<Item = PathBuf> + std::fmt::Debug,
{
    let limit = 10;
    futures::stream::iter(paths)
        .then(resolve_bundle_path)
        .flat_map_unordered(limit, |p| p)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;
    use tokio::process::Command;

    use super::foo;

    #[tokio::test]
    async fn resolve_single_file() {
        let path = PathBuf::from("foo");
        let paths = foo(&path).await;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], path);
    }

    #[tokio::test]
    async fn resolve_empty_directory() {
        let dir = tempdir().expect("failed to create temporary directory");
        let paths = foo(dir).await;
        assert!(paths.is_empty());
    }

    #[tokio::test]
    async fn resolve_mixed_directory() {
        let dir = tempdir().expect("failed to create temporary directory");
        let temp_dir = dir.path();

        let bundle_names = ["000957451622b061", "000b7a0d86775831", "00231e322d01c363"];
        let other_names = ["settings.ini", "metadata_database.db"];
        let _ = futures::future::try_join_all(
            bundle_names
                .into_iter()
                .chain(other_names.into_iter())
                .map(|name| async move {
                    Command::new("touch")
                        .arg(name)
                        .current_dir(temp_dir)
                        .status()
                        .await?;

                    Ok::<_, std::io::Error>(name)
                }),
        )
        .await
        .expect("failed to create temporary files");

        let paths = foo(dir).await;

        assert_eq!(bundle_names.len(), paths.len());

        for p in paths.iter() {
            let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap();
            assert!(bundle_names.iter().any(|n| n == &name));
        }
    }
}
