use std::io::{Cursor, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, SectionExt};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::RwLock;
use tracing::Instrument;

use crate::binary::*;
use crate::context::lookup_hash;
use crate::murmur::{HashGroup, Murmur64};
use crate::oodle;

mod file;

use file::BundleFile;

#[derive(Debug, PartialEq)]
enum BundleFormat {
    Darktide,
}

impl TryFrom<u32> for BundleFormat {
    type Error = color_eyre::Report;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0xF0000007 => Ok(Self::Darktide),
            _ => Err(eyre::eyre!("Unknown bundle format '{:08X}'", value)),
        }
    }
}

struct EntryHeader {
    _name_hash: u64,
    _extension_hash: u64,
    _flags: u32,
}

impl EntryHeader {
    #[tracing::instrument(name = "FileMeta::read", skip_all)]
    async fn read<R>(mut r: R) -> Result<Self>
    where
        R: AsyncRead + AsyncSeek + std::marker::Unpin,
    {
        let extension_hash = r.read_u64().await?;
        let name_hash = r.read_u64().await?;
        let flags = read_u32(r).await?;

        // NOTE: Known values so far:
        // - 0x0: seems to be the default
        // - 0x4: seems to be used for files that point to something in `data/`
        //        seems to correspond to a change in value in the header's 'unknown_3'
        if flags != 0x0 {
            tracing::debug!(
                flags,
                "Unexpected meta flags for file {:08X}.{:08X}",
                name_hash,
                extension_hash
            );
        }

        Ok(Self {
            _name_hash: name_hash,
            _extension_hash: extension_hash,
            _flags: flags,
        })
    }
}

pub struct Bundle {
    _format: BundleFormat,
    _headers: Vec<EntryHeader>,
    files: Vec<BundleFile>,
    name: String,
}

impl Bundle {
    #[tracing::instrument(name = "Bundle::open", skip(ctx))]
    pub async fn open<P>(ctx: Arc<RwLock<crate::Context>>, path: P) -> Result<Self>
    where
        P: AsRef<Path> + std::fmt::Debug,
    {
        let path = path.as_ref();
        let bundle_name = if let Some(name) = path.file_name() {
            let hash = Murmur64::try_from(name.to_string_lossy().as_ref())?;
            lookup_hash(ctx.clone(), hash, HashGroup::Filename).await
        } else {
            return Err(eyre::eyre!("Invalid path to bundle file"))
                .with_section(|| path.display().to_string().header("Path:"));
        };

        let mut r = fs::File::open(path)
            .await
            .wrap_err("Failed to open bundle file")
            .with_section(|| path.display().to_string().header("Path"))?;

        let format = read_u32(&mut r)
            .await
            .wrap_err("failed to read from file")
            .and_then(BundleFormat::try_from)?;

        if format != BundleFormat::Darktide {
            return Err(eyre::eyre!("Unknown bundle format: {:?}", format));
        }

        // Skip unknown 4 bytes
        r.seek(SeekFrom::Current(4)).await?;

        let num_entries = read_u32(&mut r).await? as usize;

        // Skip unknown 256 bytes. I believe this data is somewhat related to packaging and the
        // `.package` files
        r.seek(SeekFrom::Current(256)).await?;

        let mut meta = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            meta.push(EntryHeader::read(&mut r).await?);
        }

        let num_chunks = read_u32(&mut r).await? as usize;
        tracing::debug!(num_chunks);
        let mut chunk_sizes = Vec::with_capacity(num_chunks);
        for _ in 0..num_chunks {
            chunk_sizes.push(read_u32(&mut r).await? as usize);
        }

        let unpacked_size = {
            let size_1 = read_u32(&mut r).await? as usize;

            // Skip unknown 4 bytes
            r.seek(SeekFrom::Current(4)).await?;

            // NOTE: Unknown why this sometimes needs a second value.
            // Also unknown if there is a different part in the data that actually
            // determines whether this second value exists.
            if size_1 == 0x0 {
                let size_2 = read_u32(&mut r).await? as usize;
                // Skip unknown 4 bytes
                r.seek(SeekFrom::Current(4)).await?;
                size_2
            } else {
                size_1
            }
        };

        let mut decompressed = Vec::new();
        oodle::decompress(ctx.clone(), r, &mut decompressed, num_chunks).await?;

        if decompressed.len() < unpacked_size {
            return Err(eyre::eyre!(
                "Decompressed data does not match the expected size"
            ))
            .with_section(|| decompressed.len().to_string().header("Actual:"))
            .with_section(|| unpacked_size.to_string().header("Expected:"));
        }

        // Truncate to the actual data size
        decompressed.resize(unpacked_size, 0);

        let mut r = Cursor::new(decompressed);
        let mut files = Vec::with_capacity(num_entries);
        for i in 0..num_entries {
            let span = tracing::trace_span!("", file_index = i);
            let file = BundleFile::read(ctx.clone(), &mut r)
                .instrument(span)
                .await?;
            files.push(file);
        }

        Ok(Self {
            name: bundle_name,
            _format: format,
            _headers: meta,
            files,
        })
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn files(&self) -> &Vec<BundleFile> {
        &self.files
    }
}

/// Returns a decompressed version of the bundle data.
/// This is mainly useful for debugging purposes or
/// to manullay inspect the raw data.
#[tracing::instrument(skip(ctx, r, w))]
pub async fn decompress<R, W>(ctx: Arc<RwLock<crate::Context>>, mut r: R, mut w: W) -> Result<()>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
    W: AsyncWrite + std::marker::Unpin,
{
    let format = read_u32(&mut r).await.and_then(BundleFormat::try_from)?;

    if format != BundleFormat::Darktide {
        return Err(eyre::eyre!("Unknown bundle format: {:?}", format));
    }

    // Skip unknown 4 bytes
    r.seek(SeekFrom::Current(4)).await?;

    let num_entries = read_u32(&mut r).await? as i64;

    // Skip unknown 256 bytes
    r.seek(SeekFrom::Current(256)).await?;
    // Skip file meta
    r.seek(SeekFrom::Current(num_entries * 20)).await?;

    let num_chunks = read_u32(&mut r).await? as usize;
    // Skip chunk sizes
    r.seek(SeekFrom::Current(num_chunks as i64 * 4)).await?;

    {
        let size_1 = read_u32(&mut r).await?;

        // Skip unknown 4 bytes
        r.seek(SeekFrom::Current(4)).await?;

        // NOTE: Unknown why there sometimes is a second value.
        if size_1 == 0x0 {
            // Skip unknown 4 bytes
            r.seek(SeekFrom::Current(8)).await?;
        }
    }

    let chunks_start = r.stream_position().await?;

    {
        // Pipe the header into the output
        r.seek(SeekFrom::Start(0)).await?;
        let mut buf = vec![0; chunks_start as usize];
        r.read_exact(&mut buf).await?;
        w.write_all(&buf).await?;
    }

    oodle::decompress(ctx, r, w, num_chunks).await
}
