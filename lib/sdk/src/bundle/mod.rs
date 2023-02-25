use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, Report, SectionExt};
use oodle_sys::{OodleLZ_CheckCRC, OodleLZ_FuzzSafe, CHUNK_SIZE};

use crate::binary::sync::*;
use crate::bundle::file::Properties;
use crate::murmur::{HashGroup, IdString64, Murmur64};

pub(crate) mod database;
pub(crate) mod file;

pub use file::{BundleFile, BundleFileType, BundleFileVariant};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
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

pub struct Bundle {
    format: BundleFormat,
    properties: [Murmur64; 32],
    files: Vec<BundleFile>,
    name: IdString64,
}

impl Bundle {
    pub fn new<S: Into<IdString64>>(name: S) -> Self {
        Self {
            name: name.into(),
            format: BundleFormat::F8,
            properties: [0.into(); 32],
            files: Vec::new(),
        }
    }

    pub fn get_name_from_path<P>(ctx: &crate::Context, path: P) -> String
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| Murmur64::try_from(name).ok())
            .map(|hash| {
                ctx.lookup_hash(hash, HashGroup::Filename)
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| path.display().to_string())
    }

    pub fn add_file(&mut self, file: BundleFile) {
        tracing::trace!("Adding file {}", file.name(false, None));
        let existing_index = self
            .files
            .iter()
            .enumerate()
            .find(|(_, f)| **f == file)
            .map(|val| val.0);

        self.files.push(file);

        if let Some(i) = existing_index {
            self.files.swap_remove(i);
        }
    }

    #[tracing::instrument(skip(ctx, binary), fields(len_binary = binary.as_ref().len()))]
    pub fn from_binary<B>(ctx: &crate::Context, name: String, binary: B) -> Result<Self>
    where
        B: AsRef<[u8]>,
    {
        let bundle_name = name;
        let mut r = BufReader::new(Cursor::new(binary));

        let format = r.read_u32().and_then(BundleFormat::try_from)?;
        tracing::debug!(?format);

        if !matches!(format, BundleFormat::F7 | BundleFormat::F8) {
            return Err(eyre::eyre!("Unknown bundle format: {:?}", format));
        }

        r.skip_u32(0x3)?;

        let num_entries = r.read_u32()? as usize;

        let mut properties = [0.into(); 32];
        for prop in properties.iter_mut().take(32) {
            *prop = Murmur64::from(r.read_u64()?);
        }

        let mut file_props = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            // Skip two u64 that contain the extension hash and file name hash.
            // We don't need them here, since we're reading the whole bundle into memory
            // anyways.
            r.seek(SeekFrom::Current((2 * size_of::<u64>()) as i64))?;
            file_props.push(Properties::from_bits_truncate(r.read_u32()?));
        }

        let num_chunks = r.read_u32()? as usize;

        tracing::debug!(num_chunks);

        let mut chunk_sizes = Vec::with_capacity(num_chunks);
        for _ in 0..num_chunks {
            chunk_sizes.push(r.read_u32()? as usize);
        }

        r.skip_padding()?;

        let unpacked_size = r.read_u32()? as usize;
        // Skip 4 unknown bytes
        r.skip_u32(0)?;

        let mut decompressed = Vec::with_capacity(unpacked_size);
        let mut unpacked_size_tracked = unpacked_size;

        for (chunk_index, chunk_size) in chunk_sizes.into_iter().enumerate() {
            let span = tracing::debug_span!("Decompressing chunk", chunk_index, chunk_size);
            let _enter = span.enter();

            let inner_chunk_size = r.read_u32()? as usize;

            if inner_chunk_size != chunk_size {
                eyre::bail!(
                    "Chunk sizes do not match. Expected {inner_chunk_size}, got {chunk_size}",
                );
            }

            r.skip_padding()?;

            let mut compressed_buffer = vec![0u8; chunk_size];
            r.read_exact(&mut compressed_buffer)?;

            if format >= BundleFormat::F8 && chunk_size == CHUNK_SIZE {
                decompressed.append(&mut compressed_buffer);
            } else {
                // TODO: Optimize to not reallocate?
                let mut raw_buffer = oodle_sys::decompress(
                    &compressed_buffer,
                    OodleLZ_FuzzSafe::No,
                    OodleLZ_CheckCRC::No,
                )
                .wrap_err_with(|| format!("failed to decompress chunk {chunk_index}"))?;

                if unpacked_size_tracked < CHUNK_SIZE {
                    raw_buffer.resize(unpacked_size_tracked, 0);
                } else {
                    unpacked_size_tracked -= CHUNK_SIZE;
                }

                decompressed.append(&mut raw_buffer);
            }
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
        tracing::trace!(num_files = num_entries);
        for (i, props) in file_props.iter().enumerate() {
            let span = tracing::debug_span!("Read file {}", i);
            let _enter = span.enter();

            let file = BundleFile::from_reader(ctx, &mut r, *props)
                .wrap_err_with(|| format!("failed to read file {i}"))?;
            files.push(file);
        }

        Ok(Self {
            name: bundle_name.into(),
            format,
            files,
            properties,
        })
    }

    #[tracing::instrument(skip_all)]
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut w = Cursor::new(Vec::new());
        w.write_u32(self.format.into())?;
        // TODO: Find out what this is.
        w.write_u32(0x3)?;
        w.write_u32(self.files.len() as u32)?;

        for prop in self.properties.iter() {
            w.write_u64((*prop).into())?;
        }

        for file in self.files.iter() {
            w.write_u64(file.file_type().into())?;
            w.write_u64(file.base_name().to_murmur64().into())?;
            w.write_u32(file.props().bits())?;
        }

        let unpacked_data = {
            let span = tracing::trace_span!("Write bundle files");
            let _enter = span.enter();
            tracing::trace!(num_files = self.files.len());

            self.files
                .iter()
                .fold(Ok::<Vec<u8>, Report>(Vec::new()), |data, file| {
                    let mut data = data?;
                    data.append(&mut file.to_binary()?);
                    Ok(data)
                })?
        };

        // Ceiling division (or division toward infinity) to calculate
        // the number of chunks required to fit the unpacked data.
        let num_chunks = (unpacked_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        tracing::trace!(num_chunks);
        w.write_u32(num_chunks as u32)?;

        let chunk_sizes_start = w.stream_position()?;
        tracing::trace!(chunk_sizes_start);
        w.seek(SeekFrom::Current(num_chunks as i64 * 4))?;

        w.write_padding()?;

        tracing::trace!(unpacked_size = unpacked_data.len());
        w.write_u32(unpacked_data.len() as u32)?;
        // NOTE: Unknown u32 that's always been 0 so far
        w.write_u32(0)?;

        let chunks = unpacked_data.chunks(CHUNK_SIZE);
        let mut chunk_sizes = Vec::with_capacity(num_chunks);

        for chunk in chunks {
            let compressed = oodle_sys::compress(chunk)?;
            tracing::trace!(
                raw_chunk_size = chunk.len(),
                compressed_chunk_size = compressed.len()
            );
            chunk_sizes.push(compressed.len());
            w.write_u32(compressed.len() as u32)?;
            w.write_padding()?;
            w.write_all(&compressed)?;
        }

        w.seek(SeekFrom::Start(chunk_sizes_start))?;

        for size in chunk_sizes {
            w.write_u32(size as u32)?;
        }

        Ok(w.into_inner())
    }

    pub fn name(&self) -> &IdString64 {
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
pub fn decompress<B>(_ctx: &crate::Context, binary: B) -> Result<Vec<u8>>
where
    B: AsRef<[u8]>,
{
    let mut r = BufReader::new(Cursor::new(binary.as_ref()));
    let mut w = Cursor::new(Vec::new());
    let format = r.read_u32().and_then(BundleFormat::try_from)?;

    if !matches!(format, BundleFormat::F7 | BundleFormat::F8) {
        eyre::bail!("Unknown bundle format: {:?}", format);
    }

    // Skip unknown 4 bytes
    r.skip_u32(0x3)?;

    let num_entries = r.read_u32()? as i64;
    tracing::debug!(num_entries);

    // Skip unknown 256 bytes
    r.seek(SeekFrom::Current(256))?;
    // Skip file meta
    r.seek(SeekFrom::Current(num_entries * 20))?;

    let num_chunks = r.read_u32()? as usize;
    tracing::debug!(num_chunks);
    // Skip chunk sizes
    r.seek(SeekFrom::Current(num_chunks as i64 * 4))?;

    r.skip_padding()?;

    let mut unpacked_size = r.read_u32()? as usize;
    tracing::debug!(unpacked_size);

    // Skip unknown 4 bytes
    r.seek(SeekFrom::Current(4))?;

    let chunks_start = r.stream_position()?;
    tracing::trace!(chunks_start);

    // Pipe the header into the output
    {
        let span = tracing::debug_span!("Pipe file header", chunks_start);
        let _enter = span.enter();
        r.rewind()?;

        let mut buf = vec![0; chunks_start as usize];
        r.read_exact(&mut buf)?;
        w.write_all(&buf)?;

        r.seek(SeekFrom::Start(chunks_start))?;
    }

    for chunk_index in 0..num_chunks {
        let span = tracing::debug_span!("Decompressing chunk", chunk_index);
        let _enter = span.enter();
        let chunk_size = r.read_u32()? as usize;

        tracing::trace!(chunk_size);

        r.skip_padding()?;

        let mut compressed_buffer = vec![0u8; chunk_size];
        r.read_exact(&mut compressed_buffer)?;

        // TODO: Optimize to not reallocate?
        let mut raw_buffer = oodle_sys::decompress(
            &compressed_buffer,
            OodleLZ_FuzzSafe::No,
            OodleLZ_CheckCRC::No,
        )?;

        if unpacked_size < CHUNK_SIZE {
            raw_buffer.resize(unpacked_size, 0);
        } else {
            unpacked_size -= CHUNK_SIZE;
        }

        w.write_all(&raw_buffer)?;
    }

    Ok(w.into_inner())
}
