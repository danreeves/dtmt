use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use color_eyre::eyre::{self, Context};
use color_eyre::Result;

use crate::binary::sync::{ReadExt, WriteExt};
use crate::bundle::file::{BundleFileType, UserFile};
use crate::murmur::{HashGroup, Murmur64};

#[tracing::instrument(skip(_ctx))]
async fn resolve_wildcard<P>(
    _ctx: &crate::Context,
    wildcard: P,
    t: Option<BundleFileType>,
) -> Result<Vec<PathBuf>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let wildcard = wildcard.as_ref();

    if wildcard.is_absolute() {
        eyre::bail!(
            "Path or wildcard must be relative. Got '{}'",
            wildcard.display()
        );
    }

    if !wildcard.ends_with("*") {
        let mut path = wildcard.to_path_buf();

        if let Some(t) = t {
            path.push(t.ext_name());
        }

        return Ok(vec![path]);
    }

    // let parent = wildcard.parent().unwrap_or(&ctx.project_dir);

    // let paths = Vec::new();
    // let dir = fs::read_dir(parent).await?;

    // while let Some(file) = dir.next_entry().await? {
    //     if let Some(ext) = file.file_name()
    // }

    todo!();
}

type PackageType = HashMap<BundleFileType, HashSet<PathBuf>>;
type PackageDefinition = HashMap<String, HashSet<String>>;

#[derive(Default)]
pub struct Package {
    name: String,
    root: PathBuf,
    inner: PackageType,
}

impl Deref for Package {
    type Target = PackageType;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Package {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[tracing::instrument]
async fn glob_stream<PB, P>(pattern: PB, root: P) -> Result<(PathBuf, tokio::fs::ReadDir)>
where
    PB: Into<PathBuf> + std::fmt::Debug,
    P: AsRef<Path> + std::fmt::Debug,
{
    let pattern: PathBuf = pattern.into();
    if pattern.is_absolute() {
        eyre::bail!(
            "Path in package definition must not be absolute. Got '{}'",
            pattern.display()
        )
    }

    let _is_pattern = pattern.ends_with("*");
    let _dir = pattern.parent().unwrap_or(root.as_ref());
    todo!();
    // let stream = fs::read_dir(dir).await?;
    // Ok((dir.to_path_buf(), stream))
}

impl Package {
    fn len(&self) -> usize {
        self.values().fold(0, |total, files| total + files.len())
    }

    fn add_file<P>(&mut self, t: BundleFileType, path: P)
    where
        P: Into<PathBuf>,
    {
        self.entry(t).or_default().insert(path.into());
    }

    #[tracing::instrument("Package::from_sjson", skip(sjson), fields(sjson_len = sjson.as_ref().len()))]
    pub async fn from_sjson<P, S>(sjson: S, name: String, root: P) -> Result<Self>
    where
        P: AsRef<Path> + std::fmt::Debug,
        S: AsRef<str>,
    {
        let root = root.as_ref();
        let definition: PackageDefinition = serde_sjson::from_str(sjson.as_ref())?;
        let mut inner: PackageType = Default::default();

        for (ty, patterns) in definition.iter() {
            if ty == "*" {
                for pattern in patterns.iter() {
                    let (dir, mut stream) = glob_stream(pattern, root).await?;

                    while let Some(entry) = stream.next_entry().await? {
                        let name = PathBuf::from(entry.file_name());
                        let ext = if let Some(ext) = name.extension().and_then(|ext| ext.to_str()) {
                            match BundleFileType::from_str(ext) {
                                Ok(t) => t,
                                Err(_) => {
                                    tracing::debug!(
                                        "Skipping file with invalid extension: {}",
                                        dir.join(name).display()
                                    );
                                    continue;
                                }
                            }
                        } else {
                            tracing::debug!(
                                "Skipping file without extension: {}",
                                dir.join(name).display()
                            );
                            continue;
                        };

                        inner.entry(ext).or_default().insert(dir.join(name));
                    }
                }
            } else if let Ok(t) = BundleFileType::from_str(ty) {
                for pattern in patterns.iter() {
                    let (dir, mut stream) = glob_stream(pattern, root).await?;

                    while let Some(entry) = stream.next_entry().await? {
                        let name = PathBuf::from(entry.file_name());
                        let ext = if let Some(ext) = name.extension().and_then(|ext| ext.to_str()) {
                            match BundleFileType::from_str(ext) {
                                Ok(t) => t,
                                Err(_) => {
                                    tracing::debug!(
                                        "Skipping file with invalid extension: {}",
                                        dir.join(name).display()
                                    );
                                    continue;
                                }
                            }
                        } else {
                            tracing::debug!(
                                "Skipping file without extension: {}",
                                dir.join(name).display()
                            );
                            continue;
                        };

                        if t == ext {
                            inner.entry(ext).or_default().insert(dir.join(name));
                        }
                    }
                }
            } else {
                eyre::bail!("Unknown file type '{}'", ty);
            };
        }

        let pkg = Self {
            inner,
            name,
            root: root.to_path_buf(),
        };

        Ok(pkg)
    }

    #[tracing::instrument("Package::to_sjson", skip(self), fields(types = self.inner.len(), files = self.len()))]
    pub fn to_sjson(&self) -> Result<String> {
        let mut map: PackageDefinition = Default::default();

        for (t, paths) in self.iter() {
            for path in paths.iter() {
                map.entry(t.ext_name())
                    .or_default()
                    .insert(path.display().to_string());
            }
        }

        serde_sjson::to_string(&map).wrap_err("failed to serialize Package to SJSON")
    }

    #[tracing::instrument("Package::from_binary", skip(binary, ctx), fields(binary_len = binary.as_ref().len()))]
    pub fn from_binary<B>(ctx: &crate::Context, name: String, binary: B) -> Result<Self>
    where
        B: AsRef<[u8]>,
    {
        let mut r = Cursor::new(binary.as_ref());

        // TODO: Figure out what this is
        let unknown = r.read_u32()?;
        if unknown != 0x2b {
            tracing::warn!("Unknown u32 header. Expected 0x2b, got: {unknown:#08X} ({unknown})");
        }

        let file_count = r.read_u32()? as usize;
        let mut inner: PackageType = Default::default();

        for _ in 0..file_count {
            let t = BundleFileType::from(r.read_u64()?);
            let hash = Murmur64::from(r.read_u64()?);
            let path = ctx.lookup_hash(*hash, HashGroup::Filename);
            inner.entry(t).or_default().insert(PathBuf::from(path));
        }

        let pkg = Self {
            inner,
            name,
            root: PathBuf::new(),
        };

        Ok(pkg)
    }

    #[tracing::instrument("Package::to_binary", skip(self), fields(types = self.inner.len(), files = self.len()))]
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut w = Cursor::new(Vec::new());

        // TODO: Figure out what this is
        w.write_u32(0x2b)?;
        w.write_u32(self.values().flatten().count() as u32)?;

        for (t, paths) in self.iter() {
            for path in paths.iter() {
                w.write_u64(*t.hash())?;

                let hash = Murmur64::hash(path.to_string_lossy().as_bytes());
                w.write_u64(*hash)?;
            }
        }

        Ok(w.into_inner())
    }
}

#[tracing::instrument(skip(ctx, data))]
pub fn decompile<B>(ctx: &crate::Context, name: String, data: B) -> Result<Vec<UserFile>>
where
    B: AsRef<[u8]>,
{
    let pkg = Package::from_binary(ctx, name, data)?;
    let s = pkg.to_sjson()?;
    Ok(vec![UserFile::new(s.into_bytes())])
}

// #[tracing::instrument(skip_all)]
// pub fn compile(_ctx: &crate::Context, data: String) -> Result<Vec<u8>> {
//     let pkg = Package::from_sjson(data)?;
//     pkg.to_binary()
// }

#[cfg(test)]
mod test {
    use crate::BundleFileType;

    use super::Package;

    #[test]
    fn to_binary_empty_package() {
        let pkg = Package::default();

        assert_eq!(
            pkg.to_binary().unwrap(),
            vec![0x2b, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0]
        );
    }

    #[test]
    fn to_binary_single_file() {
        let mut pkg = Package::default();
        pkg.add_file(BundleFileType::Lua, "lua");

        assert_eq!(
            pkg.to_binary().unwrap(),
            vec![
                0x2b, 0x0, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0xe2, 0x17, 0xd1, 0x2c, 0xfa, 0x8d, 0x4e,
                0xa1, 0xe2, 0x17, 0xd1, 0x2c, 0xfa, 0x8d, 0x4e, 0xa1,
            ]
        );
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn to_sjson_empty_package() {
        todo!();
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn to_sjson_single_file() {
        todo!();
    }

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn to_sjson_multiple_file_types() {
        todo!();
    }

    #[tokio::test]
    async fn from_sjson_empty() {
        let root = std::env::current_dir().unwrap();
        let sjson = "";
        let name = String::new();

        assert_eq!(
            Package::from_sjson(sjson, name, root).await.unwrap().inner,
            Default::default()
        );
    }
}
