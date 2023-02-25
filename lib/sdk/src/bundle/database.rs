use std::collections::HashMap;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::Write;

use color_eyre::eyre;
use color_eyre::Result;

use crate::binary::sync::*;
use crate::binary::FromBinary;
use crate::binary::ToBinary;
use crate::murmur::Murmur64;
use crate::Bundle;

use super::file::BundleFileType;

const DATABASE_VERSION: u32 = 0x6;
const FILE_VERSION: u32 = 0x4;

pub struct BundleFile {
    name: String,
    stream: String,
    platform_specific: bool,
    file_time: u64,
}

pub struct FileName {
    extension: BundleFileType,
    name: Murmur64,
}

pub struct BundleDatabase {
    stored_files: HashMap<Murmur64, Vec<BundleFile>>,
    resource_hashes: HashMap<Murmur64, u64>,
    bundle_contents: HashMap<Murmur64, Vec<FileName>>,
}

impl BundleDatabase {
    pub fn add_bundle(&mut self, bundle: &Bundle) {
        let hash = bundle.name().to_murmur64();
        let name = hash.to_string();
        let stream = format!("{}.stream", &name);

        tracing::trace!(
            "Adding bundle '{} ({:?} | {:016X})' to database. Hash exists: {}",
            bundle.name().display(),
            bundle.name(),
            hash,
            self.stored_files.contains_key(&hash)
        );

        {
            let entry = self.stored_files.entry(hash).or_default();
            let existing = entry.iter().position(|f| f.name == name);

            let file = BundleFile {
                name,
                stream,
                file_time: 0,
                platform_specific: false,
            };

            entry.push(file);

            if let Some(pos) = existing {
                tracing::debug!("Found bundle '{}' at {}. Replacing.", hash.to_string(), pos);
                entry.swap_remove(pos);
            }
        }

        for f in bundle.files() {
            let file_name = FileName {
                extension: f.file_type(),
                name: f.base_name().to_murmur64(),
            };

            // TODO: Compute actual resource hash
            self.resource_hashes.insert(hash, 0);

            self.bundle_contents
                .entry(hash)
                .or_default()
                .push(file_name);
        }
    }
}

impl FromBinary for BundleDatabase {
    #[tracing::instrument(name = "BundleDatabase::from_binary", skip_all)]
    fn from_binary<R: Read + Seek>(r: &mut R) -> Result<Self> {
        {
            let format = r.read_u32()?;
            eyre::ensure!(
                format == DATABASE_VERSION,
                "invalid file format, expected {:#X}, got {:#X}",
                DATABASE_VERSION,
                format
            );
        }

        let num_entries = r.read_u32()? as usize;
        let mut stored_files = HashMap::with_capacity(num_entries);

        for _ in 0..num_entries {
            let hash = Murmur64::from(r.read_u64()?);

            let num_files = r.read_u32()? as usize;
            let mut files = Vec::with_capacity(num_files);

            for _ in 0..num_files {
                {
                    let version = r.read_u32()?;
                    eyre::ensure!(
                        version == FILE_VERSION,
                        "invalid file version, expected {:#X}, got {:#X}",
                        FILE_VERSION,
                        version
                    );
                }

                let len_name = r.read_u32()? as usize;
                let mut buf = vec![0; len_name];
                r.read_exact(&mut buf)?;

                let name = String::from_utf8(buf)?;

                let len_stream = r.read_u32()? as usize;
                let mut buf = vec![0; len_stream];
                r.read_exact(&mut buf)?;

                let stream = String::from_utf8(buf)?;

                let platform_specific = r.read_u8()? != 0;

                // TODO: Unknown what this is. In VT2's SDK, it's simply ignored,
                // and always written as `0`, but in DT, it seems to be used.
                let mut buffer = [0; 20];
                r.read_exact(&mut buffer)?;

                if cfg!(debug_assertions) && buffer.iter().any(|b| *b != 0) {
                    tracing::warn!("Unknown value in 20-byte buffer: {:?}", buffer);
                }

                let file_time = r.read_u64()?;

                let file = BundleFile {
                    name,
                    stream,
                    platform_specific,
                    file_time,
                };

                files.push(file);
            }

            stored_files.insert(hash, files);
        }

        let num_hashes = r.read_u32()? as usize;
        let mut resource_hashes = HashMap::with_capacity(num_hashes);

        for _ in 0..num_hashes {
            let name = Murmur64::from(r.read_u64()?);
            let hash = r.read_u64()?;

            resource_hashes.insert(name, hash);
        }

        let num_contents = r.read_u32()? as usize;
        let mut bundle_contents = HashMap::with_capacity(num_contents);

        for _ in 0..num_contents {
            let hash = Murmur64::from(r.read_u64()?);

            let num_files = r.read_u32()? as usize;
            let mut files = Vec::with_capacity(num_files);

            for _ in 0..num_files {
                let extension = BundleFileType::from(r.read_u64()?);
                let name = Murmur64::from(r.read_u64()?);

                files.push(FileName { extension, name });
            }

            bundle_contents.insert(hash, files);
        }

        Ok(Self {
            stored_files,
            resource_hashes,
            bundle_contents,
        })
    }
}

impl ToBinary for BundleDatabase {
    #[tracing::instrument(name = "BundleDatabase::to_binary", skip_all)]
    fn to_binary(&self) -> Result<Vec<u8>> {
        let mut binary = Vec::new();

        {
            let mut w = Cursor::new(&mut binary);

            w.write_u32(DATABASE_VERSION)?;

            w.write_u32(self.stored_files.len() as u32)?;

            for (hash, files) in self.stored_files.iter() {
                w.write_u64((*hash).into())?;
                w.write_u32(files.len() as u32)?;

                for f in files.iter() {
                    w.write_u32(FILE_VERSION)?;
                    w.write_u32(f.name.len() as u32)?;
                    w.write_all(f.name.as_bytes())?;
                    w.write_u32(f.stream.len() as u32)?;
                    w.write_all(f.stream.as_bytes())?;

                    w.write_u8(if f.platform_specific { 1 } else { 0 })?;

                    // TODO: Don't know what goes here
                    let buffer = [0; 20];
                    w.write_all(&buffer)?;

                    w.write_u64(f.file_time)?;
                }
            }

            w.write_u32(self.resource_hashes.len() as u32)?;

            for (name, hash) in self.resource_hashes.iter() {
                w.write_u64((*name).into())?;
                w.write_u64(*hash)?;
            }

            w.write_u32(self.bundle_contents.len() as u32)?;

            for (hash, contents) in self.bundle_contents.iter() {
                w.write_u64((*hash).into())?;
                w.write_u32(contents.len() as u32)?;

                for FileName { extension, name } in contents.iter() {
                    w.write_u64((*extension).into())?;
                    w.write_u64((*name).into())?;
                }
            }
        }

        Ok(binary)
    }
}
