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
    // All other Stingray texture options (format, mipmap settings, ...) are
    // accepted but currently ignored. Serde drops unknown fields by default.
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
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
    stream: Option<Vec<u8>>,
    category: IdString32,
}

impl std::fmt::Debug for Texture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("Texture");
        out.field("header", &self.header);

        if self.data.len() <= 5 {
            out.field("data", &format!("{:x?}", &self.data));
        } else {
            out.field(
                "data",
                &format!("{:x?}.. ({} bytes)", &self.data[..5], &self.data.len()),
            );
        }

        if let Some(stream) = self.stream.as_ref() {
            if stream.len() <= 5 {
                out.field("stream", &format!("{:x?}", &stream));
            } else {
                out.field(
                    "stream",
                    &format!("{:x?}.. ({} bytes)", &stream[..5], &stream.len()),
                );
            }
        } else {
            out.field("stream", &"None");
        }

        out.field("category", &self.category).finish()
    }
}

impl Texture {
    #[tracing::instrument(skip(data, chunks))]
    fn decompress_stream_data(mut data: impl Read, chunks: impl AsRef<[usize]>) -> Result<Vec<u8>> {
        const RAW_SIZE: usize = 0x10000;

        let chunks = chunks.as_ref();

        let max_size = chunks.iter().max().copied().unwrap_or(RAW_SIZE);
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

            let raw = oodle::decompress(buf, RAW_SIZE, OodleLZ_FuzzSafe::No, OodleLZ_CheckCRC::No)
                .wrap_err("Failed to decompress stream chunk")?;
            eyre::ensure!(
                raw.len() == RAW_SIZE,
                "Invalid chunk length after decompression"
            );

            stream_raw.extend_from_slice(&raw);

            last = *offset_next;
        }
        Ok(stream_raw)
    }

    #[tracing::instrument(skip(data), fields(data_len = data.as_ref().len()))]
    fn reorder_stream_mipmap(
        data: impl AsRef<[u8]>,
        num_chunks: usize,
        bits_per_block: usize,
        bytes_per_block: usize,
        block_size: usize,
        pitch: usize,
    ) -> Result<Vec<u8>> {
        const CHUNK_SIZE: usize = 0x10000;
        let data = data.as_ref();

        // The stream contains up to `n_streamable_mipmaps` mipmaps. We only
        // reassemble the largest one, which is stored as bands of
        // `bytes_per_block` chunks (each covering the full width and 256 pixels
        // of height) in column-major order.
        let mut out = Vec::with_capacity(num_chunks * CHUNK_SIZE);
        let mut window = vec![0u8; pitch * 64];

        let row_size = bits_per_block * block_size;
        tracing::Span::current().record("row_size", row_size);

        eyre::ensure!(
            data.len() >= num_chunks * CHUNK_SIZE,
            "Stream data is too small for the expected number of chunks"
        );

        for (i, chunk) in data[..num_chunks * CHUNK_SIZE]
            .chunks_exact(CHUNK_SIZE)
            .enumerate()
        {
            let chunk_x = (i % bytes_per_block) * row_size;

            let span = tracing::trace_span!("chunk", i, chunk_x = chunk_x);
            let _guard = span.enter();

            if i > 0 && i % bytes_per_block == 0 {
                out.extend_from_slice(&window);
            }

            for (j, row) in chunk.chunks_exact(row_size).enumerate() {
                let start = chunk_x + j * pitch;
                let end = start + row_size;
                tracing::trace!("{i}/{j} at {start}:{end}");
                window[start..end].copy_from_slice(row);
            }
        }

        // The final band is only flushed once all its chunks have been written.
        out.extend_from_slice(&window);

        Ok(out)
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
                },
            },
        };
        serde_sjson::to_string(&texture).wrap_err("Failed to serialize texture definition")
    }

    #[tracing::instrument(fields(
        dds_header = tracing::field::Empty,
        dx10_header = tracing::field::Empty,
        image_format = tracing::field::Empty,
    ))]
    fn create_dds_user_file(&self, name: String) -> Result<UserFile> {
        // Without a stream file, the bundle already contains a complete DDS
        // image, which we can emit as-is.
        if self.stream.is_none() {
            return Ok(UserFile::with_name(self.data.clone(), name));
        }

        let mut data = Cursor::new(&self.data);
        let mut dds_header =
            dds::DDSHeader::from_binary(&mut data).wrap_err("Failed to read DDS header")?;

        {
            let span = tracing::Span::current();
            span.record("dds_header", format!("{dds_header:?}"));
        }

        if !dds_header.pixel_format.flags.contains(dds::DDPF::FOURCC) {
            tracing::debug!("Found DDS without FourCC. Dumping raw data");
            return Ok(UserFile::with_name(self.data.clone(), name));
        }

        let dx10_header =
            dds::Dx10Header::from_binary(&mut data).wrap_err("Failed to read DX10 header")?;

        {
            let span = tracing::Span::current();
            span.record("dx10_header", format!("{dx10_header:?}"));
        }

        let stingray_image_format = dds::stripped_format_from_header(&dds_header, &dx10_header)?;
        {
            let span = tracing::Span::current();
            span.record("image_format", format!("{stingray_image_format:?}"));
        }

        let block_size = 4 * dds_header.pitch_or_linear_size / dds_header.width;
        let bits_per_block: usize = match block_size {
            8 => 128,
            16 => 64,
            block_size => eyre::bail!("Unsupported block size {block_size}"),
        };

        let pitch = self.header.width / 4 * block_size;
        let bytes_per_block = self.header.width / bits_per_block / 4;

        tracing::debug!(
            "block_size = {} | pitch = {} | bits_per_block = {} | bytes_per_block = {}",
            block_size,
            pitch,
            bits_per_block,
            bytes_per_block
        );

        let mut out_data = Cursor::new(Vec::with_capacity(self.data.len()));

        // Currently, we only extract the largest mipmap,
        // so we need to set the dimensions accordingly, and remove the
        // flag.
        dds_header.width = self.header.width;
        dds_header.height = self.header.height;
        dds_header.mipmap_count = 0;
        dds_header.flags &= !dds::DDSD::MIPMAPCOUNT;

        dds_header
            .to_binary(&mut out_data)
            .wrap_err("Failed to write DDS header")?;

        dx10_header
            .to_binary(&mut out_data)
            .wrap_err("Failed to write DX10 header")?;

        // We returned early above when there is no stream, so this is always set.
        let stream = self.stream.as_ref().expect("stream presence checked above");

        // The largest mipmap consists of one band (256px tall) per 256 pixels of
        // height, with each band made up of `bytes_per_block` chunks.
        let num_bands = self.header.height.div_ceil(256);
        let num_chunks = num_bands * bytes_per_block;
        tracing::debug!(num_bands, num_chunks, "Reassembling largest mipmap");

        let data = Self::reorder_stream_mipmap(
            stream,
            num_chunks,
            bits_per_block,
            bytes_per_block,
            block_size,
            pitch,
        )
        .wrap_err("Failed to reorder stream chunks")?;

        out_data
            .write_all(&data)
            .wrap_err("Failed to write streamed mipmap data")?;

        Ok(UserFile::with_name(out_data.into_inner(), name))
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

        // For debugging purposes, also extract the raw files
        if cfg!(debug_assertions) {
            if let Some(stream) = &self.stream {
                let stream_name = PathBuf::from(&name).with_extension("stream");
                files.push(UserFile::with_name(
                    stream.clone(),
                    stream_name.display().to_string(),
                ));
            }

            let name = PathBuf::from(&name)
                .with_extension("raw.dds")
                .display()
                .to_string();
            files.push(UserFile::with_name(self.data.clone(), name));
        }

        match self
            .create_dds_user_file(name)
            .wrap_err("Failed to create DDS file")
        {
            Ok(dds) => files.push(dds),
            Err(err) => {
                if cfg!(debug_assertions) {
                    tracing::error!(
                        "{:?}",
                        err.with_section(|| {
                            "Running in debug mode, continuing to produce raw files".header("Note:")
                        })
                    );
                } else {
                    return Err(err);
                }
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

    let (width, height) = {
        let mut r = Cursor::new(&dds);
        let header = dds::DDSHeader::from_binary(&mut r).wrap_err("Failed to read DDS header")?;

        (header.width, header.height)
    };

    let mut w = Cursor::new(Vec::new());

    // Unknown categories are decompiled as 8-digit hex hashes, so we need to
    // turn those back into a hash instead of hashing the hex string itself.
    let category = {
        let value = definitions.common.output.category.as_deref().unwrap_or("");
        match u32::from_str_radix(value, 16) {
            Ok(hash) if value.len() == 8 => IdString32::from(hash),
            _ => IdString32::String(value.to_string()),
        }
    };

    let mut flags = TextureFlags::empty();
    if definitions.common.output.srgb {
        flags |= TextureFlags::SRGB;
    }

    if definitions.common.output.streamable {
        tracing::warn!(
            "Texture '{}' is marked streamable, but streamed mipmaps cannot be compiled yet. \
             Compiling the mipmaps inline instead.",
            name.display()
        );
    }

    let texture = Texture {
        header: TextureHeader {
            // As long as we can't handle mipmaps, these need be `0`
            flags,
            n_streamable_mipmaps: 0,
            width,
            height,
            mip_infos: [TextureHeaderMipInfo::default(); 16],
            meta_size: 0,
        },
        data: dds,
        stream: None,
        category,
    };
    texture.to_binary(&mut w)?;

    let mut variant = BundleFileVariant::new();
    variant.set_data(w.into_inner());

    let mut file = BundleFile::new(name, BundleFileType::Texture);
    file.add_variant(variant);

    Ok(file)
}
