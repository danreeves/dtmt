use std::io::{Cursor, Read, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use bitflags::bitflags;
use color_eyre::eyre::Context;
use color_eyre::{Help, Result, SectionExt, eyre};
use flate2::read::ZlibDecoder;
use oodle::{OodleLZ_CheckCRC, OodleLZ_FuzzSafe};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::binary::sync::{ReadExt, WriteExt};
use crate::bundle::file::UserFile;
use crate::murmur::{HashGroup, IdString32, IdString64};
use crate::{BundleFile, BundleFileType, BundleFileVariant, binary};

mod dds;

/// Size of a single Oodle-compressed chunk in a streamed texture data file.
const STREAM_CHUNK_SIZE: usize = 0x10000;
/// Number of 4x4 block rows a single stream chunk covers (256 pixels).
const CHUNK_ROWS: usize = 64;
/// Number of bytes covered by one row of a stream chunk.
const CHUNK_ROW_SIZE: usize = 0x400;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TextureDefinition {
    common: TextureDefinitionPlatform,
    // Stingray supports per-platform sections here, where you can create overrides with the same
    // values as in `common`. But since we only support PC, we don't need to implement
    // that.
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TextureDefinitionPlatform {
    input: TextureDefinitionInput,
    output: TextureDefinitionOutput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TextureDefinitionInput {
    filename: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct TextureDefinitionOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    srgb: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    streamable: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    mipmap_num_largest_steps_to_discard: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    mipmap_num_smallest_steps_to_discard: u32,
    // All other Stingray texture options (format, apply_processing, cut_alpha_threshold,
    // mipmap_filter, mipmap_filter_wrap_mode, mipmap_keep_original, ...) are accepted but
    // currently ignored. Serde drops unknown fields by default.
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    struct TextureFlags: u32 {
        const STREAMABLE = 0b0000_0001;
        const UNKNOWN = 1 << 1;
        const SRGB = 1 << 8;
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct TextureHeaderMipInfo {
    offset: usize,
    size: usize,
}

#[derive(Clone, Default)]
struct TextureHeader {
    flags: TextureFlags,
    n_streamable_mipmaps: usize,
    width: usize,
    height: usize,
    mip_infos: [TextureHeaderMipInfo; 16],
    meta_size: usize,
}

impl std::fmt::Debug for TextureHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureHeader")
            .field("flags", &self.flags)
            .field("n_streamable_mipmaps", &self.n_streamable_mipmaps)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("mip_infos", &{
                let mut s = self
                    .mip_infos
                    .iter()
                    .fold(String::from("["), |mut s, info| {
                        s.push_str(&format!("{}/{}, ", info.offset, info.size));
                        s
                    });
                s.push(']');
                s
            })
            .field("meta_size", &self.meta_size)
            .finish()
    }
}

impl TextureHeader {
    #[tracing::instrument(skip(r))]
    fn from_binary(mut r: impl ReadExt) -> Result<Self> {
        let flags = r.read_u32().map(binary::flags_from_bits)?;
        let n_streamable_mipmaps = r.read_u32()? as usize;
        let width = r.read_u32()? as usize;
        let height = r.read_u32()? as usize;

        let mut mip_infos = [TextureHeaderMipInfo::default(); 16];

        for info in mip_infos.iter_mut() {
            info.offset = r.read_u32()? as usize;
            info.size = r.read_u32()? as usize;
        }

        let meta_size = r.read_u32()? as usize;

        Ok(Self {
            flags,
            n_streamable_mipmaps,
            width,
            height,
            mip_infos,
            meta_size,
        })
    }

    #[tracing::instrument(skip(w))]
    fn to_binary(&self, mut w: impl WriteExt) -> Result<()> {
        w.write_u32(self.flags.bits())?;
        w.write_u32(self.n_streamable_mipmaps as u32)?;
        w.write_u32(self.width as u32)?;
        w.write_u32(self.height as u32)?;

        for info in self.mip_infos {
            w.write_u32(info.offset as u32)?;
            w.write_u32(info.size as u32)?;
        }

        w.write_u32(self.meta_size as u32)?;

        Ok(())
    }
}

#[derive(Clone)]
struct Texture {
    header: TextureHeader,
    data: Vec<u8>,
    /// Decompressed streamed mipmap data (only set when decompiling).
    stream: Option<Vec<u8>>,
    /// Cumulative, compressed end offsets of the stream chunks (only set when compiling).
    stream_chunk_ends: Vec<u32>,
    category: IdString32,
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("Texture");
        out.field("header", &self.header);

        if self.data.len() <= 5 {
            out.field("data", &format!("{:x?}", self.data));
        } else {
            out.field(
                "data",
                &format!("{:x?}.. ({} bytes)", &self.data[..5], self.data.len()),
            );
        }

        if let Some(stream) = self.stream.as_ref() {
            if stream.len() <= 5 {
                out.field("stream", &format!("{:x?}", stream));
            } else {
                out.field(
                    "stream",
                    &format!("{:x?}.. ({} bytes)", &stream[..5], stream.len()),
                );
            }
        } else {
            out.field("stream", &"None");
        }

        out.field("category", &self.category).finish()
    }
}

/// A single mipmap level of a DDS image.
#[derive(Copy, Clone, Debug)]
struct Mip {
    offset: usize,
    size: usize,
    width: usize,
    height: usize,
}

/// A parsed DDS image, including the location and size of every mipmap.
struct DdsImage {
    header: dds::DDSHeader,
    dx10: dds::Dx10Header,
    data_offset: usize,
    block_bytes: usize,
    mips: Vec<Mip>,
}

/// Size in bytes of a single block-compressed mipmap level.
fn mip_size(width: usize, height: usize, block_bytes: usize) -> usize {
    width.div_ceil(4).max(1) * height.div_ceil(4).max(1) * block_bytes
}

/// Number of chunk rows, i.e. 4x4 blocks, a single chunk is wide.
fn chunk_width_blocks(block_bytes: usize) -> usize {
    match block_bytes {
        8 => 128,
        16 => 64,
        other => unreachable!("unsupported block size {other}"),
    }
}

/// Number of stream chunks required for a mipmap of the given dimensions.
fn chunks_per_mip(width: usize, height: usize, block_bytes: usize) -> usize {
    let blocks_wide = width.div_ceil(4).max(1);
    let blocks_high = height.div_ceil(4).max(1);

    let chunks_wide = blocks_wide.div_ceil(chunk_width_blocks(block_bytes));
    let chunks_high = blocks_high.div_ceil(CHUNK_ROWS);

    chunks_wide * chunks_high
}

fn dds_block_bytes(header: &dds::DDSHeader, dx10: &dds::Dx10Header) -> Result<usize> {
    if header.pixel_format.flags.contains(dds::DDPF::FOURCC) {
        if header.pixel_format.four_cc == dds::FourCC::DX10 {
            return dx10.dxgi_format.block_bytes().ok_or_else(|| {
                eyre::eyre!(
                    "Streamed textures require a block-compressed format, but got {}",
                    dx10.dxgi_format
                )
            });
        }

        return header.pixel_format.four_cc.block_bytes().ok_or_else(|| {
            eyre::eyre!(
                "Streamed textures require a block-compressed format, but got {}",
                header.pixel_format.four_cc
            )
        });
    }

    eyre::bail!("Streamed uncompressed textures are not supported")
}

impl DdsImage {
    #[tracing::instrument(skip(data))]
    fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Cursor::new(data);

        let mut header = dds::DDSHeader::from_binary(&mut r).wrap_err("Invalid DDS header")?;

        // Normalize legacy DXT formats to their DX10 equivalent, since that is
        // what the engine uses.
        let dx10 = if header.pixel_format.four_cc == dds::FourCC::DX10 {
            dds::Dx10Header::from_binary(&mut r).wrap_err("Invalid DX10 header")?
        } else {
            let format = match header.pixel_format.four_cc {
                dds::FourCC::DXT1 => dds::DXGIFormat::BC1_UNORM,
                dds::FourCC::DXT2 | dds::FourCC::DXT3 => dds::DXGIFormat::BC2_UNORM,
                dds::FourCC::DXT4 | dds::FourCC::DXT5 => dds::DXGIFormat::BC3_UNORM,
                other => {
                    eyre::bail!("Unsupported FourCC for a streamed texture: {other}")
                }
            };

            header.pixel_format.four_cc = dds::FourCC::DX10;

            dds::Dx10Header {
                dxgi_format: format,
                resource_dimension: dds::D3D10ResourceDimension::Texture2D,
                misc_flag: dds::DdsResourceMiscFlags::empty(),
                array_size: 1,
                misc_flags2: 0,
            }
        };

        let data_offset = r.position() as usize;

        let block_bytes = dds_block_bytes(&header, &dx10)?;

        let count = header.mipmap_count.max(1);
        let mut mips = Vec::with_capacity(count);

        let mut width = header.width;
        let mut height = header.height;
        let mut offset = data_offset;

        for _ in 0..count {
            let size = mip_size(width, height, block_bytes);
            mips.push(Mip {
                offset,
                size,
                width,
                height,
            });

            offset += size;
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }

        eyre::ensure!(
            offset <= data.len(),
            "DDS file is too small for its mipmap chain (needs at least {offset} bytes, got {})",
            data.len()
        );

        Ok(Self {
            header,
            dx10,
            data_offset,
            block_bytes,
            mips,
        })
    }

    /// Writes a DDS header for a mipmap chain starting at the given dimensions.
    fn write_header(
        &self,
        w: &mut impl WriteExt,
        width: usize,
        height: usize,
        mip_count: usize,
    ) -> Result<()> {
        let mut header = self.header;
        header.width = width;
        header.height = height;
        header.mipmap_count = mip_count;
        header.pitch_or_linear_size = width.div_ceil(4).max(1) * self.block_bytes;

        if mip_count > 1 {
            header.flags |= dds::DDSD::MIPMAPCOUNT;
        } else {
            header.flags &= !dds::DDSD::MIPMAPCOUNT;
        }

        header
            .to_binary(&mut *w)
            .wrap_err("Failed to write DDS header")?;
        self.dx10
            .to_binary(&mut *w)
            .wrap_err("Failed to write DX10 header")?;

        Ok(())
    }
}

impl Texture {
    #[tracing::instrument(skip(data, chunks))]
    fn decompress_stream_data(mut data: impl Read, chunks: impl AsRef<[usize]>) -> Result<Vec<u8>> {
        let chunks = chunks.as_ref();

        let max_size = chunks.iter().max().copied().unwrap_or(STREAM_CHUNK_SIZE);
        let mut read_buf = vec![0; max_size];

        let mut stream_raw = Vec::with_capacity(chunks.iter().sum());
        let mut last = 0;

        for offset_next in chunks {
            let size = offset_next - last;

            let span = tracing::info_span!(
                "stream chunk",
                num_chunks = chunks.len(),
                chunk_size_comp = size,
                offset = last
            );
            let _enter = span.enter();

            let buf = &mut read_buf[0..size];
            data.read_exact(buf)
                .wrap_err("Failed to read chunk from stream file")?;

            let raw = oodle::decompress(
                buf,
                STREAM_CHUNK_SIZE,
                OodleLZ_FuzzSafe::No,
                OodleLZ_CheckCRC::No,
            )
            .wrap_err("Failed to decompress stream chunk")?;
            eyre::ensure!(
                raw.len() == STREAM_CHUNK_SIZE,
                "Invalid chunk length after decompression"
            );

            stream_raw.extend_from_slice(&raw);

            last = *offset_next;
        }
        Ok(stream_raw)
    }

    /// Reassembles a single mipmap from its stream chunks (column-major bands).
    #[tracing::instrument(skip(data), fields(data_len = data.len()))]
    fn reorder_mip(
        data: &[u8],
        width: usize,
        height: usize,
        block_bytes: usize,
    ) -> Result<Vec<u8>> {
        let chunks = chunks_per_mip(width, height, block_bytes);
        let pitch = width.div_ceil(4).max(1) * block_bytes;
        let chunks_wide = chunks_per_mip(width, 4, block_bytes);

        let mut out = Vec::with_capacity(mip_size(width, height, block_bytes));
        let mut window = vec![0u8; pitch * CHUNK_ROWS];

        let needed = chunks * STREAM_CHUNK_SIZE;
        eyre::ensure!(
            data.len() >= needed,
            "Stream data is too small for its mipmaps (needs {needed} bytes, got {})",
            data.len()
        );

        for (i, chunk) in data[..needed].chunks_exact(STREAM_CHUNK_SIZE).enumerate() {
            if i > 0 && i % chunks_wide == 0 {
                out.extend_from_slice(&window);
            }

            let chunk_x = (i % chunks_wide) * CHUNK_ROW_SIZE;

            for (j, row) in chunk.chunks_exact(CHUNK_ROW_SIZE).enumerate() {
                let start = chunk_x + j * pitch;
                window[start..start + CHUNK_ROW_SIZE].copy_from_slice(row);
            }
        }

        // The final band is only flushed once all its chunks have been written.
        out.extend_from_slice(&window);

        Ok(out)
    }

    /// Splits a single mipmap into its stream chunks (column-major bands).
    fn chunk_mip(data: &[u8], width: usize, height: usize, block_bytes: usize) -> Vec<Vec<u8>> {
        let pitch = width.div_ceil(4).max(1) * block_bytes;
        let chunks_wide = chunks_per_mip(width, 4, block_bytes);
        let chunks_high = height.div_ceil(4).max(1).div_ceil(CHUNK_ROWS);

        let mut chunks = Vec::with_capacity(chunks_wide * chunks_high);

        for band in 0..chunks_high {
            for column in 0..chunks_wide {
                let mut chunk = vec![0u8; STREAM_CHUNK_SIZE];

                for row in 0..CHUNK_ROWS {
                    let block_row = band * CHUNK_ROWS + row;
                    let src = block_row * pitch + column * CHUNK_ROW_SIZE;
                    let dst = row * CHUNK_ROW_SIZE;
                    chunk[dst..dst + CHUNK_ROW_SIZE]
                        .copy_from_slice(&data[src..src + CHUNK_ROW_SIZE]);
                }

                chunks.push(chunk);
            }
        }

        chunks
    }

    #[tracing::instrument(
        "Texture::from_binary",
        skip(ctx, r, stream_r),
        fields(
            compression_type = tracing::field::Empty,
            compressed_size = tracing::field::Empty,
            uncompressed_size = tracing::field::Empty,
        )
    )]
    fn from_binary(
        ctx: &crate::Context,
        mut r: impl Read + Seek,
        mut stream_r: Option<impl Read>,
    ) -> Result<Self> {
        let compression_type = r.read_u32()?;
        let compressed_size = r.read_u32()? as usize;
        let uncompressed_size = r.read_u32()? as usize;

        {
            let span = tracing::Span::current();
            span.record("compression_type", compression_type);
            span.record("compressed_size", compressed_size);
            span.record("uncompressed_size", uncompressed_size);
        }

        let mut comp_buf = vec![0; compressed_size];
        r.read_exact(&mut comp_buf)?;

        let out_buf = match compression_type {
            // Uncompressed
            // This one never seems to contain the additional `TextureHeader` metadata,
            // so we return early in this branch.
            0 => {
                eyre::ensure!(
                    compressed_size == 0 && uncompressed_size == 0,
                    "Cannot handle texture with compression_type == 0, but buffer sizes > 0"
                );
                tracing::trace!("Found raw texture");

                let pos = r.stream_position()?;
                let end = {
                    r.seek(SeekFrom::End(0))?;
                    let end = r.stream_position()?;
                    r.seek(SeekFrom::Start(pos))?;
                    end
                };

                // Reads until the last u32.
                let mut data = vec![0u8; (end - pos - 4) as usize];
                r.read_exact(&mut data)?;

                let category = r.read_u32().map(IdString32::from)?;

                return Ok(Self {
                    header: TextureHeader::default(),
                    data,
                    stream: None,
                    stream_chunk_ends: Vec::new(),
                    category,
                });
            }
            1 => oodle::decompress(
                comp_buf,
                uncompressed_size,
                OodleLZ_FuzzSafe::No,
                OodleLZ_CheckCRC::No,
            )?,
            2 => {
                let mut decoder = ZlibDecoder::new(comp_buf.as_slice());
                let mut buf = Vec::with_capacity(uncompressed_size);

                decoder.read_to_end(&mut buf)?;
                buf
            }
            _ => eyre::bail!(
                "Unknown compression type for texture '{}'",
                compression_type
            ),
        };

        eyre::ensure!(
            out_buf.len() == uncompressed_size,
            "Length of decompressed buffer did not match expected value. Expected {}, got {}",
            uncompressed_size,
            out_buf.len()
        );

        // No idea what this number is supposed to mean.
        // Even the game engine just skips this one.
        r.skip_u32(0x43)?;

        let header = TextureHeader::from_binary(&mut r)?;

        eyre::ensure!(
            header.meta_size == 0 || stream_r.is_some(),
            "Compression chunks and stream file don't match up. meta_size = {}, has_stream = {}",
            header.meta_size,
            stream_r.is_some()
        );

        let stream = if let Some(stream_r) = stream_r.as_mut() {
            // Number of compression chunks in the stream file
            let num_chunks = r.read_u32()? as usize;
            r.skip_u16(0)?;

            {
                let num_chunks_1 = r.read_u16()? as usize;

                eyre::ensure!(
                    num_chunks == num_chunks_1,
                    "Chunk numbers don't match. first = {}, second = {}",
                    num_chunks,
                    num_chunks_1
                );
            }

            let mut chunks = Vec::with_capacity(num_chunks);

            for _ in 0..num_chunks {
                chunks.push(r.read_u32()? as usize);
            }

            let stream_raw = Self::decompress_stream_data(stream_r, chunks)
                .wrap_err("Failed to decompress stream data")?;

            Some(stream_raw)
        } else {
            None
        };

        let category = ctx.lookup_hash_short(r.read_u32()?, HashGroup::TextureCategory);

        Ok(Self {
            category,
            header,
            data: out_buf,
            stream,
            stream_chunk_ends: Vec::new(),
        })
    }

    #[tracing::instrument(skip(w))]
    fn to_binary(&self, mut w: impl WriteExt) -> Result<()> {
        let compression_type = 1;
        w.write_u32(compression_type)?;

        let comp_buf = oodle::compress_exact(&self.data).wrap_err("Failed to compress DDS data")?;

        w.write_u32(comp_buf.len() as u32)?;
        w.write_u32(self.data.len() as u32)?;
        w.write_all(&comp_buf)?;

        // Unknown field, which the engine seems to ignore.
        // All game files have the same value here, so we just mirror that.
        w.write_u32(0x43)?;

        self.header.to_binary(&mut w)?;

        if self.header.meta_size > 0 {
            let num_chunks = self.stream_chunk_ends.len();
            w.write_u32(num_chunks as u32)?;
            w.write_u16(0)?;
            w.write_u16(num_chunks as u16)?;

            for end in &self.stream_chunk_ends {
                w.write_u32(*end)?;
            }
        }

        w.write_u32(self.category.to_murmur32().into())?;
        Ok(())
    }

    #[tracing::instrument]
    fn to_sjson(&self, filename: String) -> Result<String> {
        let texture = TextureDefinition {
            common: TextureDefinitionPlatform {
                input: TextureDefinitionInput { filename },
                output: TextureDefinitionOutput {
                    category: Some(self.category.display().to_string()),
                    srgb: self.header.flags.contains(TextureFlags::SRGB),
                    streamable: self.header.flags.contains(TextureFlags::STREAMABLE),
                    mipmap_num_largest_steps_to_discard: 0,
                    mipmap_num_smallest_steps_to_discard: 0,
                },
            },
        };
        serde_sjson::to_string(&texture).wrap_err("Failed to serialize texture definition")
    }

    /// Rebuilds a complete DDS image (all mipmaps) from the inline data and the
    /// streamed mipmaps.
    #[tracing::instrument(skip(self), fields(name = %name))]
    fn create_dds_user_file(&self, name: String) -> Result<UserFile> {
        // Without a stream file, the bundle already contains a complete DDS
        // image, which we can emit as-is.
        let Some(stream) = self.stream.as_ref() else {
            return Ok(UserFile::with_name(self.data.clone(), name));
        };

        let image = DdsImage::parse(&self.data).wrap_err("Failed to parse inline DDS image")?;
        let block_bytes = image.block_bytes;

        let inline_mips = image.header.mipmap_count.max(1);
        let total_mips = self.header.n_streamable_mipmaps + inline_mips;

        let mut out = Cursor::new(Vec::new());
        image
            .write_header(&mut out, self.header.width, self.header.height, total_mips)
            .wrap_err("Failed to write DDS header")?;

        let mut offset = 0;
        let mut width = self.header.width;
        let mut height = self.header.height;

        for _ in 0..self.header.n_streamable_mipmaps {
            let raw = Self::reorder_mip(&stream[offset..], width, height, block_bytes)
                .wrap_err("Failed to reassemble streamed mipmap")?;
            out.write_all(&raw)
                .wrap_err("Failed to write streamed mipmap")?;

            offset += chunks_per_mip(width, height, block_bytes) * STREAM_CHUNK_SIZE;
            width = (width / 2).max(1);
            height = (height / 2).max(1);
        }

        out.write_all(&self.data[image.data_offset..])
            .wrap_err("Failed to write inline mipmaps")?;

        Ok(UserFile::with_name(out.into_inner(), name))
    }

    #[tracing::instrument(skip(self))]
    fn to_user_files(&self, name: String) -> Result<Vec<UserFile>> {
        let mut files = Vec::with_capacity(2);

        {
            let data = self.to_sjson(name.clone())?.as_bytes().to_vec();
            let name = PathBuf::from(&name)
                .with_extension("texture")
                .display()
                .to_string();
            files.push(UserFile::with_name(data, name));
        }

        match self
            .create_dds_user_file(name)
            .wrap_err("Failed to create DDS file")
        {
            Ok(dds) => files.push(dds),
            Err(err) => {
                return Err(err);
            }
        };

        Ok(files)
    }
}

#[tracing::instrument(skip(ctx, data, stream_data), fields(data_len = data.as_ref().len()))]
pub(crate) async fn decompile_data(
    ctx: &crate::Context,
    name: String,
    data: impl AsRef<[u8]>,
    stream_data: Option<impl AsRef<[u8]>>,
) -> Result<Vec<UserFile>> {
    let mut r = Cursor::new(data);
    let mut stream_r = stream_data.map(Cursor::new);

    let texture = Texture::from_binary(ctx, &mut r, stream_r.as_mut())?;
    texture
        .to_user_files(name)
        .wrap_err("Failed to build user files")
}

#[tracing::instrument(skip(ctx))]
pub(crate) async fn decompile(
    ctx: &crate::Context,
    name: String,
    variant: &BundleFileVariant,
) -> Result<Vec<UserFile>> {
    let data_file = variant.data_file_name().map(|name| match &ctx.game_dir {
        Some(dir) => dir.join("bundle").join(name),
        None => PathBuf::from("bundle").join(name),
    });

    if variant.external() {
        let Some(path) = data_file else {
            eyre::bail!("File is marked external but has no data file name");
        };

        tracing::debug!(
            "Decompiling texture from external file '{}'",
            path.display()
        );

        let data = fs::read(&path)
            .await
            .wrap_err_with(|| format!("Failed to read data file '{}'", path.display()))
            .with_suggestion(|| {
                "Provide a game directory in the config file or make sure the `data` directory is next to the provided bundle."
            })?;

        decompile_data(ctx, name, data, None::<&[u8]>).await
    } else {
        tracing::debug!("Decompiling texture from bundle data");

        let stream_data = match data_file {
            Some(path) => {
                let data = fs::read(&path)
                    .await
                    .wrap_err_with(|| format!("Failed to read data file '{}'", path.display()))
                    .with_suggestion(|| {
                        "Provide a game directory in the config file or make sure the `data` directory is next to the provided bundle."
                    })?;
                Some(data)
            }
            None => None,
        };

        decompile_data(ctx, name, variant.data(), stream_data).await
    }
}

#[tracing::instrument(skip(sjson, root), fields(sjson_len = sjson.as_ref().len()))]
pub async fn compile(
    name: IdString64,
    sjson: impl AsRef<str>,
    root: impl AsRef<Path> + std::fmt::Debug,
) -> Result<BundleFile> {
    let definitions: TextureDefinition = serde_sjson::from_str(sjson.as_ref())
        .wrap_err("Failed to deserialize SJSON")
        .with_section(|| sjson.as_ref().to_string().header("SJSON:"))?;

    let dds = {
        let root = root.as_ref();
        let mut path = root.join(&definitions.common.input.filename);

        // Stingray texture definitions usually omit the file extension, while
        // DTMT projects reference a `.dds` directly.
        if !path.exists() && path.extension().is_none() {
            path.set_extension("dds");
        }

        fs::read(&path)
            .await
            .wrap_err_with(|| format!("Failed to read DDS file '{}'", path.display()))?
    };

    // Unknown categories are decompiled as 8-digit hex hashes, so we need to
    // turn those back into a hash instead of hashing the hex string itself.
    let category = {
        let value = definitions.common.output.category.as_deref().unwrap_or("");
        match u32::from_str_radix(value, 16) {
            Ok(hash) if value.len() == 8 => IdString32::from(hash),
            _ => IdString32::String(value.to_string()),
        }
    };

    let output = &definitions.common.output;

    let mut flags = TextureFlags::empty();
    if output.srgb {
        flags |= TextureFlags::SRGB;
    }

    let apply_discards = output.mipmap_num_largest_steps_to_discard > 0
        || output.mipmap_num_smallest_steps_to_discard > 0;

    let mut variant = BundleFileVariant::new();

    // Fast path: an inline texture without discards can be stored as-is,
    // which also supports uncompressed formats.
    if !output.streamable && !apply_discards {
        let mut r = Cursor::new(&dds);
        let header = dds::DDSHeader::from_binary(&mut r).wrap_err("Failed to parse DDS header")?;

        let texture = Texture {
            header: TextureHeader {
                flags,
                n_streamable_mipmaps: 0,
                width: header.width,
                height: header.height,
                mip_infos: [TextureHeaderMipInfo::default(); 16],
                meta_size: 0,
            },
            data: dds,
            stream: None,
            stream_chunk_ends: Vec::new(),
            category,
        };

        let mut wrapper = Cursor::new(Vec::new());
        texture.to_binary(&mut wrapper)?;
        variant.set_data(wrapper.into_inner());

        let mut file = BundleFile::new(name, BundleFileType::Texture);
        file.add_variant(variant);
        return Ok(file);
    }

    let image = DdsImage::parse(&dds).wrap_err("Failed to parse DDS image")?;
    let block_bytes = image.block_bytes;

    // Apply the requested mipmap discards.
    let start = (output.mipmap_num_largest_steps_to_discard as usize).min(image.mips.len() - 1);
    let end = image
        .mips
        .len()
        .saturating_sub(output.mipmap_num_smallest_steps_to_discard as usize)
        .max(start + 1);

    let mips = &image.mips[start..end];
    tracing::debug!(
        total_mips = image.mips.len(),
        kept_mips = mips.len(),
        width = mips[0].width,
        height = mips[0].height,
        "Compiling texture"
    );

    // Like the engine, stream every mipmap that is at least one full stream
    // chunk in size, and keep the rest inline.
    let n_streamable = if output.streamable {
        mips.iter()
            .take_while(|mip| mip.size >= STREAM_CHUNK_SIZE)
            .count()
            // The engine always keeps at least the smallest mipmap inline.
            .min(mips.len().saturating_sub(1))
    } else {
        0
    };

    let mut mip_infos = [TextureHeaderMipInfo::default(); 16];
    let mut stream_raster_offset = 0;
    let mut inline_raster_offset = 0;

    for (i, mip) in mips.iter().enumerate() {
        if i < n_streamable {
            mip_infos[i] = TextureHeaderMipInfo {
                offset: stream_raster_offset,
                size: mip.size,
            };
            stream_raster_offset += mip.size;
        } else {
            mip_infos[i] = TextureHeaderMipInfo {
                offset: inline_raster_offset,
                size: mip.size,
            };
            inline_raster_offset += mip.size;
        }
    }

    if n_streamable == 0 {
        // Fully inline texture assembled from the selected mipmaps.
        let mut data = Cursor::new(Vec::new());
        image.write_header(&mut data, mips[0].width, mips[0].height, mips.len())?;

        for mip in mips {
            data.write_all(&dds[mip.offset..mip.offset + mip.size])?;
        }

        let texture = Texture {
            header: TextureHeader {
                flags,
                n_streamable_mipmaps: 0,
                width: mips[0].width,
                height: mips[0].height,
                mip_infos,
                meta_size: 0,
            },
            data: data.into_inner(),
            stream: None,
            stream_chunk_ends: Vec::new(),
            category,
        };

        let mut wrapper = Cursor::new(Vec::new());
        texture.to_binary(&mut wrapper)?;
        variant.set_data(wrapper.into_inner());
    } else {
        // Streamed texture: compress the large mipmaps into a data file and
        // keep the small ones inline.
        let mut stream = Vec::new();
        let mut chunk_ends = Vec::new();

        for mip in &mips[..n_streamable] {
            let chunks = Texture::chunk_mip(
                &dds[mip.offset..mip.offset + mip.size],
                mip.width,
                mip.height,
                block_bytes,
            );

            for chunk in chunks {
                let compressed =
                    oodle::compress_exact(&chunk).wrap_err("Failed to compress stream chunk")?;
                stream.extend_from_slice(&compressed);
                chunk_ends.push(stream.len() as u32);
            }
        }

        let mut data = Cursor::new(Vec::new());
        let inline = &mips[n_streamable..];
        image.write_header(&mut data, inline[0].width, inline[0].height, inline.len())?;

        for mip in inline {
            data.write_all(&dds[mip.offset..mip.offset + mip.size])?;
        }

        let texture = Texture {
            header: TextureHeader {
                flags: flags | TextureFlags::STREAMABLE,
                n_streamable_mipmaps: n_streamable,
                width: mips[0].width,
                height: mips[0].height,
                mip_infos,
                // 8 bytes for the chunk count fields plus one u32 per chunk.
                meta_size: 8 + chunk_ends.len() * 4,
            },
            data: data.into_inner(),
            stream: None,
            stream_chunk_ends: chunk_ends,
            category,
        };

        let mut wrapper = Cursor::new(Vec::new());
        texture.to_binary(&mut wrapper)?;
        variant.set_data(wrapper.into_inner());

        // Name the data file after the resource, so it is unique and stable.
        let hash = format!("{:016x}", u64::from(name.to_murmur64()));
        let data_file_name = format!("data/{}/{}.stream", &hash[..2], hash);
        variant.set_external_data_file(data_file_name, stream);
    }

    let mut file = BundleFile::new(name, BundleFileType::Texture);
    file.add_variant(variant);

    Ok(file)
}
