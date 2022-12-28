use std::io::{Cursor, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report, SectionExt};
use tokio::fs;
use tokio::io::{
    AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::sync::RwLock;
use tracing::Instrument;

use crate::binary::*;
use crate::murmur::{HashGroup, Murmur64};
use crate::oodle::types::{OodleLZ_CheckCRC, OodleLZ_FuzzSafe};
use crate::oodle::CHUNK_SIZE;

pub(crate) mod file;

pub use file::BundleFile;

#[derive(Clone, Copy, Debug, PartialEq)]
enum BundleFormat {
    F7,
    F8,
}

impl TryFrom<u32> for BundleFormat {
    type Error = color_eyre::Report;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0xF0000007 => Ok(Self::F7),
            0xF0000008 => Ok(Self::F8),
            _ => Err(eyre::eyre!("Unknown bundle format '{:08X}'", value)),
        }
    }
}

impl From<BundleFormat> for u32 {
    fn from(value: BundleFormat) -> Self {
        match value {
            BundleFormat::F7 => 0xF0000007,
            BundleFormat::F8 => 0xF0000008,
        }
    }
}

struct EntryHeader {
    name_hash: u64,
    extension_hash: u64,
    flags: u32,
}

impl EntryHeader {
    #[tracing::instrument(name = "FileMeta::read", skip_all)]
    async fn read<R>(r: &mut R) -> Result<Self>
    where
        R: AsyncRead + AsyncSeek + std::marker::Unpin,
    {
        let extension_hash = read_u64(r).await?;
        let name_hash = read_u64(r).await?;
        let flags = read_u32(r).await?;

        // NOTE: Known values so far:
        // - 0x0: seems to be the default
        // - 0x4: seems to be used for files that point to something in `data/`
        //        seems to correspond to a change in value in the header's 'unknown_3'
        if flags != 0x0 {
            tracing::debug!(
                flags,
                "Unexpected meta flags for file {:016X}.{:016X}",
                name_hash,
                extension_hash
            );
        }

        Ok(Self {
            name_hash,
            extension_hash,
            flags,
        })
    }

    #[tracing::instrument(name = "FileMeta::write", skip_all)]
    async fn write<W>(&self, w: &mut W) -> Result<()>
    where
        W: AsyncWrite + AsyncSeek + std::marker::Unpin,
    {
        write_u64(w, self.extension_hash).await?;
        write_u64(w, self.name_hash).await?;
        write_u32(w, self.flags).await?;

        Ok(())
    }
}

pub struct Bundle {
    format: BundleFormat,
    _headers: Vec<EntryHeader>,
    files: Vec<BundleFile>,
    name: String,
    unknown_1: u32,
    unknown_header: [u8; 256],
}

impl Bundle {
    #[tracing::instrument(name = "Bundle::open", skip(ctx))]
    pub async fn open<P>(ctx: Arc<RwLock<crate::Context>>, path: P) -> Result<Self>
    where
        P: AsRef<Path> + std::fmt::Debug,
    {
        // We need to know the bundle name, so it's easier to be given the
        // file path and open the File internally, than to be given a generic
        // `AsyncRead` and the bundle name separately.
        let path = path.as_ref();
        let bundle_name = if let Some(name) = path.file_name() {
            match Murmur64::try_from(name.to_string_lossy().as_ref()) {
                Ok(hash) => ctx.read().await.lookup_hash(hash, HashGroup::Filename),
                Err(err) => {
                    tracing::debug!("failed to turn bundle name into hash: {}", err);
                    name.to_string_lossy().to_string()
                }
            }
        } else {
            eyre::bail!("Invalid path to bundle file: {}", path.display());
        };

        let f = fs::File::open(path)
            .await
            .wrap_err_with(|| format!("failed to open bundle file {}", path.display()))?;

        let mut r = BufReader::new(f);

        let format = read_u32(&mut r)
            .await
            .wrap_err("failed to read from file")
            .and_then(BundleFormat::try_from)?;

        if !matches!(format, BundleFormat::F7 | BundleFormat::F8) {
            return Err(eyre::eyre!("Unknown bundle format: {:?}", format));
        }

        let unknown_1 = read_u32(&mut r).await?;
        if unknown_1 != 0x3 {
            tracing::warn!(
                "Unexpected value for unknown header. Expected {:#08X}, got {:#08X}",
                0x3,
                unknown_1
            );
        }

        let num_entries = read_u32(&mut r).await? as usize;

        let mut unknown_header = [0; 256];
        r.read_exact(&mut unknown_header).await?;

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

        skip_padding(&mut r).await?;

        let unpacked_size = read_u32(&mut r).await? as usize;
        // Skip 4 unknown bytes
        r.seek(SeekFrom::Current(4)).await?;

        let mut decompressed = Vec::with_capacity(unpacked_size);
        let mut unpacked_size_tracked = unpacked_size;

        for (chunk_index, chunk_size) in chunk_sizes.into_iter().enumerate() {
            let span = tracing::debug_span!("Decompressing chunk", chunk_index, chunk_size);

            async {
                let inner_chunk_size = read_u32(&mut r).await? as usize;

                if inner_chunk_size != chunk_size {
                    eyre::bail!(
                        "Chunk sizes do not match. Expected {}, got {}",
                        inner_chunk_size,
                        chunk_size,
                    );
                }

                skip_padding(&mut r).await?;

                let mut compressed_buffer = vec![0u8; chunk_size];
                r.read_exact(&mut compressed_buffer).await?;

                // TODO: Optimize to not reallocate?
                let ctx = ctx.read().await;
                let oodle_lib = ctx.oodle.as_ref().unwrap();
                let mut raw_buffer = oodle_lib.decompress(
                    &compressed_buffer,
                    OodleLZ_FuzzSafe::No,
                    OodleLZ_CheckCRC::No,
                )?;

                if unpacked_size_tracked < CHUNK_SIZE {
                    raw_buffer.resize(unpacked_size_tracked, 0);
                } else {
                    unpacked_size_tracked -= CHUNK_SIZE;
                }

                tracing::trace!(raw_size = raw_buffer.len());

                decompressed.append(&mut raw_buffer);
                Ok(())
            }
            .instrument(span)
            .await?;
        }

        if decompressed.len() < unpacked_size {
            return Err(eyre::eyre!(
                "Decompressed data does not match the expected size"
            ))
            .with_section(|| decompressed.len().to_string().header("Actual:"))
            .with_section(|| unpacked_size.to_string().header("Expected:"));
        }

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
            format,
            _headers: meta,
            files,
            unknown_1,
            unknown_header,
        })
    }

    #[tracing::instrument(name = "Bundle::write", skip_all)]
    pub async fn write<W>(&self, ctx: Arc<RwLock<crate::Context>>, w: &mut W) -> Result<()>
    where
        W: AsyncWrite + AsyncSeek + std::marker::Unpin,
    {
        write_u32(w, self.format.into()).await?;
        write_u32(w, self.unknown_1).await?;
        write_u32(w, self.files.len() as u32).await?;
        w.write_all(&self.unknown_header).await?;

        for meta in self._headers.iter() {
            meta.write(w).await?;
        }

        let unpacked_data = {
            let span = tracing::trace_span!("Write bundle files");
            let buf = Vec::new();
            let mut c = Cursor::new(buf);

            tracing::trace!(num_files = self.files.len());

            async {
                for file in self.files.iter() {
                    file.write(ctx.clone(), &mut c).await?;
                }

                Ok::<(), Report>(())
            }
            .instrument(span)
            .await?;

            c.into_inner()
        };

        // Ceiling division (or division toward infinity) to calculate
        // the number of chunks required to fit the unpacked data.
        let num_chunks = (unpacked_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        tracing::trace!(num_chunks);
        write_u32(w, num_chunks as u32).await?;

        let chunk_sizes_start = w.stream_position().await?;
        tracing::trace!(chunk_sizes_start);
        w.seek(SeekFrom::Current(num_chunks as i64 * 4)).await?;

        write_padding(w).await?;

        tracing::trace!(unpacked_size = unpacked_data.len());
        write_u32(w, unpacked_data.len() as u32).await?;
        // NOTE: Unknown u32 that's always been 0 so far
        write_u32(w, 0).await?;

        let chunks = unpacked_data.chunks(CHUNK_SIZE);

        let ctx = ctx.read().await;
        let oodle_lib = ctx.oodle.as_ref().unwrap();
        let mut chunk_sizes = Vec::with_capacity(num_chunks);

        for chunk in chunks {
            let compressed = oodle_lib.compress(chunk)?;
            tracing::trace!(
                raw_chunk_size = chunk.len(),
                compressed_chunk_size = compressed.len()
            );
            chunk_sizes.push(compressed.len());
            write_u32(w, compressed.len() as u32).await?;
            write_padding(w).await?;
            w.write_all(&compressed).await?;
        }

        w.seek(SeekFrom::Start(chunk_sizes_start)).await?;

        for size in chunk_sizes {
            write_u32(w, size as u32).await?;
        }

        Ok(())
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn files(&self) -> &Vec<BundleFile> {
        &self.files
    }

    pub fn files_mut(&mut self) -> impl Iterator<Item = &mut BundleFile> {
        self.files.iter_mut()
    }
}

/// Returns a decompressed version of the bundle data.
/// This is mainly useful for debugging purposes or
/// to manullay inspect the raw data.
#[tracing::instrument(skip_all)]
pub async fn decompress<R, W>(ctx: Arc<RwLock<crate::Context>>, mut r: R, mut w: W) -> Result<()>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
    W: AsyncWrite + std::marker::Unpin,
{
    let format = read_u32(&mut r).await.and_then(BundleFormat::try_from)?;

    if !matches!(format, BundleFormat::F7 | BundleFormat::F8) {
        eyre::bail!("Unknown bundle format: {:?}", format);
    }

    // Skip unknown 4 bytes
    r.seek(SeekFrom::Current(4)).await?;

    let num_entries = read_u32(&mut r).await? as i64;
    tracing::debug!(num_entries);

    // Skip unknown 256 bytes
    r.seek(SeekFrom::Current(256)).await?;
    // Skip file meta
    r.seek(SeekFrom::Current(num_entries * 20)).await?;

    let num_chunks = read_u32(&mut r).await? as usize;
    tracing::debug!(num_chunks);
    // Skip chunk sizes
    r.seek(SeekFrom::Current(num_chunks as i64 * 4)).await?;

    skip_padding(&mut r).await?;

    let mut unpacked_size = read_u32(&mut r).await? as usize;
    tracing::debug!(unpacked_size);

    // Skip unknown 4 bytes
    r.seek(SeekFrom::Current(4)).await?;

    let chunks_start = r.stream_position().await?;
    tracing::trace!(chunks_start);

    // Pipe the header into the output
    {
        let span = tracing::debug_span!("Pipe file header", chunks_start);
        async {
            r.seek(SeekFrom::Start(0)).await?;

            let mut buf = vec![0; chunks_start as usize];
            r.read_exact(&mut buf).await?;
            w.write_all(&buf).await?;

            r.seek(SeekFrom::Start(chunks_start)).await
        }
        .instrument(span)
        .await?;
    }

    for chunk_index in 0..num_chunks {
        let span = tracing::debug_span!("Decompressing chunk", chunk_index);
        async {
            let chunk_size = read_u32(&mut r).await? as usize;

            tracing::trace!(chunk_size);

            skip_padding(&mut r).await?;

            let mut compressed_buffer = vec![0u8; chunk_size];
            r.read_exact(&mut compressed_buffer).await?;

            let ctx = ctx.read().await;
            let oodle_lib = ctx.oodle.as_ref().unwrap();
            // TODO: Optimize to not reallocate?
            let mut raw_buffer = oodle_lib.decompress(
                &compressed_buffer,
                OodleLZ_FuzzSafe::No,
                OodleLZ_CheckCRC::No,
            )?;

            if unpacked_size < CHUNK_SIZE {
                raw_buffer.resize(unpacked_size, 0);
            } else {
                unpacked_size -= CHUNK_SIZE;
            }

            w.write_all(&raw_buffer).await?;

            Ok::<(), color_eyre::Report>(())
        }
        .instrument(span)
        .await?;
    }

    Ok(())
}
