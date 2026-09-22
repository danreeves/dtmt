Decompiling the game binary shows a rather elaborate algorithm to load
DDS images from binary. Though comparing it to Microsoft's documentation
on DDS, most of it seems to be pretty standard handling.

However, we don't actually need all of it. The part about calculating
pitch and reading blocks only accesses a subset of the `ImageFormat`
struct, so we can strip our implementation to just that.

The diff and Rust file below is the full commit, containing everything I've ported
from IDA so far, to be kept as a backup here, and as a reference in case I
do need more of it in the future.

```rust
use bitflags::bitflags;
use color_eyre::eyre;
use color_eyre::Result;

const FOURCC_DXT1: u32 = 0x31545844;
const FOURCC_DXT3: u32 = 0x33545844;
const FOURCC_DXT5: u32 = 0x35545844;
const FOURCC_AXI1: u32 = 0x31495441;
const FOURCC_AXI2: u32 = 0x32495441;
const FOURCC_D3D_A16B16G16R16: u32 = 0x24;
const FOURCC_D3D_R16F: u32 = 0x6F;
const FOURCC_D3D_G16R16F: u32 = 0x70;
const FOURCC_D3D_A16B16G16R16F: u32 = 0x71;
const FOURCC_D3D_R32F: u32 = 0x72;
const FOURCC_D3D_G32R32F: u32 = 0x73;
const FOURCC_D3D_A32B32G32R32F: u32 = 0x74;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct DDSD: u32 {
        /// Required
        const CAPS = 0x1;
        /// Required
        const HEIGHT = 0x2;
        /// Required
        const WIDTH = 0x4;
        /// Pitch for an uncompressed texture
        const PITCH = 0x8;
        /// Required
        const PIXELFORMAT = 0x1000;
        /// Required in a mipmapped texture
        const MIPMAPCOUNT = 0x20000;
        /// Pitch for a compressed texture
        const LINEARSIZE = 0x80000;
        /// Required in a depth texture
        const DEPTH = 0x800000;
    }

    #[derive(Clone, Copy, Debug)]
    pub struct DDSCAPS: u32 {
        const COMPLEX = 0x8;
        const MIPMAP = 0x400000;
        const TEXTURE = 0x1000;
    }

    #[derive(Clone, Copy, Debug)]
    pub struct DDSCAPS2: u32 {
        const CUBEMAP = 0x200;
        const CUBEMAP_POSITIVEX = 0x400;
        const CUBEMAP_NEGATIVEX = 0x800;
        const CUBEMAP_POSITIVEY = 0x1000;
        const CUBEMAP_NEGATIVEY = 0x2000;
        const CUBEMAP_POSITIVEZ = 0x4000;
        const CUBEMAP_NEGATIVEZ = 0x8000;
        const VOLUME = 0x200000;

        const CUBEMAP_ALLFACES = Self::CUBEMAP_POSITIVEX.bits()
            | Self::CUBEMAP_NEGATIVEX.bits()
            | Self::CUBEMAP_POSITIVEY.bits()
            | Self::CUBEMAP_NEGATIVEY.bits()
            | Self::CUBEMAP_POSITIVEZ.bits()
            | Self::CUBEMAP_NEGATIVEZ.bits();
    }

    #[derive(Clone, Copy, Debug)]
    pub struct DDPF: u32 {
        const ALPHAPIXELS = 0x1;
        const ALPHA = 0x2;
        const FOURCC = 0x4;
        const RGB = 0x40;
        const YUV = 0x200;
        const LUMINANCE = 0x20000;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DDS_RESOURCE_MISC_FLAGS {
    TEXTURECUBE = 0x4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D3D10_RESOURCE_DIMENSION {
    UNKNOWN = 0,
    BUFFER = 1,
    TEXTURE1D = 2,
    TEXTURE2D = 3,
    TEXTURE3D = 4,
}

// Mostly copied from VT2's PDB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    R8G8B8A8 = 0x0,
    R32F = 0x1,
    R16I = 0x2,
    DEPTH_STENCIL = 0x3,
    BC1 = 0x4,
    BC2 = 0x5,
    BC3 = 0x6,
    BC4 = 0x7,
    BC5 = 0x8,
    BC6H_UF16 = 0x9,
    BC6H_SF16 = 0xA,
    BC7 = 0xB,
    R32G32B32A32F = 0xC,
    SHADOW_MAP = 0xD,
    BUFFER_32F = 0xE,
    R16F = 0xF,
    R16G16B16A16F = 0x10,
    R16UNORM = 0x11,
    R8G8 = 0x12,
    R16G16UNORM = 0x13,
    R16G16F = 0x14,
    R8 = 0x15,
    R32G32UINT = 0x16,
    R16UINT = 0x17,
    R32UINT = 0x18,
    R11G11B10F = 0x19,
    R32G32F = 0x1A,
    R10G10B10A2UNORM = 0x1B,
    // The next two did not exist in VT2.
    // Names are assumed based on which entries in `DXGI_FORMAT` they are mapped to.
    R16G16B16A16UINT = 0x1C,
    R16G16B16A16UNORM = 0x1D,
    // With the addition of the two other entries, this is probably `UNKNOWN` from VT2.
    // The gap here is weird, so maybe there are two more entries that the engine code
    // simply never maps.
    UNKNOWN = 0x20,
}

impl PixelFormat {
    /// Maps an [DXGI_FORMAT](https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format)
    /// to Fatshark's internal `PixelFormat` enum.
    pub fn from_dxgi(dxgi_format: u32) -> Self {
        match dxgi_format {
            1..=4 => PixelFormat::R32G32B32A32F,
            0xA => PixelFormat::R16G16B16A16F,
            0xB => PixelFormat::R16G16B16A16UNORM,
            0xC => PixelFormat::R16G16B16A16UINT,
            0xF | 0x10 | 0x12 => PixelFormat::R32G32F,
            0x11 => PixelFormat::R32G32UINT,
            0x17 | 0x18 | 0x19 => PixelFormat::R10G10B10A2UNORM,
            0x1A => PixelFormat::R11G11B10F,
            0x1B..=0x20 => PixelFormat::R8G8B8A8,
            0x22 => PixelFormat::R16G16F,
            0x23 => PixelFormat::R16G16UNORM,
            0x27 | 0x29 | 0x2B => PixelFormat::BUFFER_32F,
            0x28 => PixelFormat::SHADOW_MAP,
            0x2A => PixelFormat::R32UINT,
            0x2D => PixelFormat::DEPTH_STENCIL,
            0x30..=0x34 => PixelFormat::R8G8,
            0x36 => PixelFormat::R16F,
            0x38 => PixelFormat::R16UNORM,
            0x39 => PixelFormat::R16UINT,
            0x3B => PixelFormat::R16I,
            0x3C..=0x40 => PixelFormat::R8,
            0x46..=0x48 => PixelFormat::BC1,
            0x49..=0x4B => PixelFormat::BC2,
            0x4C..=0x4E => PixelFormat::BC3,
            0x4F..=0x51 => PixelFormat::BC4,
            0x52..=0x54 => PixelFormat::BC5,
            0x5F => PixelFormat::BC6H_UF16,
            0x60 => PixelFormat::BC6H_SF16,
            0x61..=0x63 => PixelFormat::BC7,
            _ => PixelFormat::UNKNOWN,
        }
    }
}

pub struct DX10_Header {
    /// Resource data formats, including fully-typed and typeless formats.
    /// See https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format
    dxgi_format: u32,
    resource_dimension: D3D10_RESOURCE_DIMENSION,
    misc_flag: DDS_RESOURCE_MISC_FLAGS,
    array_size: u32,
    misc_flags2: u32,
}

pub struct DDSPixelFormat {
    /// Structure size. Must be `32`.
    size: u32,
    flags: DDPF,
    four_cc: u32,
    rgb_bit_count: u32,
    r_bit_mask: u32,
    g_bit_mask: u32,
    b_bit_mask: u32,
    a_bit_mask: u32,
}

pub struct DDSHeader {
    /// Size of this structure. Must be `124`.
    size: u32,
    /// Flags to indicate which members contain valid data.
    flags: DDSD,
    height: u32,
    width: u32,
    pitch_or_linear_size: u32,
    depth: u32,
    mipmap_count: u32,
    reserved_1: [u8; 11],
    pixel_format: DDSPixelFormat,
    caps: DDSCAPS,
    caps_2: DDSCAPS2,
    reserved_2: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageType {
    IMAGE2D = 0,
    IMAGE3D = 1,
    IMAGECUBE = 2,
    UNKNOWN = 3,
    IMAGE2D_ARRAY = 4,
    IMAGECUBE_ARRAY = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageValidity {
    STATIC = 0,
    UPDATABLE = 1,
    DYNAMIC = 2,
}

pub struct ImageFormat {
    pub pixel_format: PixelFormat,
    pub image_type: ImageType,
    pub validity: ImageValidity,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
    pub srgb: bool,
}

/// A stripped version of `ImageType` that only contains just the data needed
/// to read a DDS image stream.
pub struct StrippedImageFormat {
    pub image_type: ImageType,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
}

fn label_98(dds_header: &DDSHeader, image_format: &mut ImageFormat) -> Result<()> {
    image_format.width = dds_header.width;
    image_format.height = dds_header.height;

    if dds_header.mipmap_count == 0 {
        image_format.mip_levels = 1;
    } else {
        image_format.mip_levels = dds_header.mipmap_count;
    }

    if dds_header.caps_2.contains(DDSCAPS2::CUBEMAP) {
        image_format.image_type = ImageType::IMAGECUBE;
        image_format.layers = 6;
    } else if dds_header.caps_2.contains(DDSCAPS2::VOLUME) {
        image_format.image_type = ImageType::IMAGE3D;
        image_format.layers = dds_header.depth;
    } else {
        image_format.image_type = ImageType::IMAGE2D;
        image_format.layers = 1;
    }

    return Ok(());
}

fn label_46(
    dds_header: &DDSHeader,
    dx10_header: &DX10_Header,
    image_format: &mut ImageFormat,
) -> Result<()> {
    if dds_header.pixel_format.four_cc != 0x30315844 {
        eyre::bail!("Unsupported DDS type");
    }

    image_format.pixel_format = match PixelFormat::from_dxgi(dx10_header.dxgi_format) {
        PixelFormat::UNKNOWN => {
            eyre::bail!(
                "dxgi format '{}' is not yet supported",
                dx10_header.dxgi_format
            );
        }
        format => format,
    };

    if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE2D {
        if dx10_header.misc_flag == DDS_RESOURCE_MISC_FLAGS::TEXTURECUBE {
            image_format.image_type = ImageType::IMAGECUBE;
            if dx10_header.array_size > 1 {
                image_format.layers = dx10_header.array_size;
            } else {
                image_format.layers = 6;
            }
        } else {
            image_format.image_type = ImageType::IMAGE2D;
            image_format.layers = dx10_header.array_size;
        }
    } else if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE3D {
        image_format.image_type = ImageType::IMAGE3D;
        image_format.layers = dds_header.depth;
    }

    if dx10_header.array_size > 1 {
        match image_format.image_type {
            ImageType::IMAGE2D => image_format.image_type = ImageType::IMAGE2D_ARRAY,
            ImageType::IMAGECUBE => image_format.image_type = ImageType::IMAGECUBE_ARRAY,
            ImageType::IMAGE3D => {
                eyre::bail!("3D-Arrays are not a supported image format")
            }
            _ => {}
        }
    }

    image_format.width = dds_header.width;
    image_format.height = dds_header.height;

    if dds_header.mipmap_count > 0 {
        image_format.mip_levels = dds_header.mipmap_count;
    } else {
        image_format.mip_levels = 1;
    }

    return Ok(());
}

pub fn image_format_from_header(
    dds_header: &DDSHeader,
    dx10_header: &DX10_Header,
    image_format: &mut ImageFormat,
) -> Result<()> {
    let ddspf = &dds_header.pixel_format;

    match ddspf.four_cc {
        FOURCC_DXT1 => {
            image_format.pixel_format = PixelFormat::BC1;
            return label_98(dds_header, image_format);
        }
        FOURCC_DXT3 => {
            image_format.pixel_format = PixelFormat::BC2;
            return label_98(dds_header, image_format);
        }
        FOURCC_DXT5 => {
            image_format.pixel_format = PixelFormat::BC3;
            return label_98(dds_header, image_format);
        }
        FOURCC_AXI1 => {
            image_format.pixel_format = PixelFormat::BC4;
            return label_98(dds_header, image_format);
        }
        FOURCC_AXI2 => {
            image_format.pixel_format = PixelFormat::BC5;
            return label_98(dds_header, image_format);
        }
        _ => {}
    }

    if ddspf.rgb_bit_count == 32 {
        // This is a heavily collapsed version of what the decompiled code does through `goto`s.
        // It doesn't really make sense that these two very different forms map to the same format,
        // but this is what Fatshark's code does. And their direct mapping above also omits all of
        // the BGR formats.
        let is_argb = ddspf.a_bit_mask == 0xFF000000
            && ddspf.r_bit_mask == 0xFF0000
            && ddspf.g_bit_mask == 0xFF00
            && ddspf.b_bit_mask == 0xFF;
        let is_bgr = ddspf.a_bit_mask == 0x0
            && ddspf.r_bit_mask == 0xFF
            && ddspf.g_bit_mask == 0xFF00
            && ddspf.b_bit_mask == 0xFF0000;
        if is_argb || is_bgr {
            image_format.pixel_format = PixelFormat::R8G8B8A8;
            return label_98(dds_header, image_format);
        }
    } else if ddspf.rgb_bit_count == 8 && ddspf.r_bit_mask == 0xFF {
        image_format.pixel_format = PixelFormat::R8;
        return label_98(dds_header, image_format);
    }

    match dds_header.pixel_format.four_cc {
        FOURCC_D3D_R16F => {
            image_format.pixel_format = PixelFormat::R16F;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_R32F => {
            image_format.pixel_format = PixelFormat::R32F;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_G16R16F => {
            image_format.pixel_format = PixelFormat::R16G16F;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_G32R32F => {
            image_format.pixel_format = PixelFormat::R32G32F;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_A16B16G16R16F => {
            image_format.pixel_format = PixelFormat::R16G16B16A16F;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_A16B16G16R16 => {
            image_format.pixel_format = PixelFormat::R16G16B16A16UNORM;
            return label_98(dds_header, image_format);
        }
        FOURCC_D3D_A32B32G32R32F => {
            image_format.pixel_format = PixelFormat::R32G32B32A32F;
            return label_98(dds_header, image_format);
        }
        _ => {}
    }

    if ddspf.r_bit_mask == 0xFFFF {
        if ddspf.g_bit_mask == 0 {
            if ddspf.b_bit_mask == 0 && ddspf.a_bit_mask == 0 {
                image_format.pixel_format = PixelFormat::R16UNORM;
                return label_98(dds_header, image_format);
            }
            return label_46(dds_header, dx10_header, image_format);
        }

        if ddspf.g_bit_mask != 0xFFFF || ddspf.b_bit_mask != 0 || ddspf.a_bit_mask != 0 {
            return label_46(dds_header, dx10_header, image_format);
        }

        image_format.pixel_format = PixelFormat::R16G16F;
        return label_98(dds_header, image_format);
    }

    label_46(dds_header, dx10_header, image_format)
}

// This is a stripped down version of the logic that the engine implements to fill
// `stingray::ImageFormat`. With the `type` field we need to distinguish between `IMAGE3D`
// and everything else, and we the various dimensions filled to calculate the chunks.
pub fn stripped_format_from_header(
    dds_header: &DDSHeader,
    dx10_header: &DX10_Header,
) -> Result<StrippedImageFormat> {
    let mut image_format = StrippedImageFormat {
        image_type: ImageType::UNKNOWN,
        width: dds_header.width,
        height: dds_header.height,
        layers: 0,
        mip_levels: 0,
    };

    if dds_header.mipmap_count > 0 {
        image_format.mip_levels = dds_header.mipmap_count;
    } else {
        image_format.mip_levels = 1;
    }

    // INFO: These next two sections are conditional in the engine code,
    // based on a lot of stuff in "fourcc" and other fields. But it might
    // actually be fine to just do it like this, as this seems universal
    // to DDS.
    // Will have to check how it plays out with actual assets.

    if dds_header.caps_2.contains(DDSCAPS2::CUBEMAP) {
        image_format.image_type = ImageType::IMAGECUBE;
        image_format.layers = 6;
    } else if dds_header.caps_2.contains(DDSCAPS2::VOLUME) {
        image_format.image_type = ImageType::IMAGE3D;
        image_format.layers = dds_header.depth;
    } else {
        image_format.image_type = ImageType::IMAGE2D;
        image_format.layers = 1;
    }

    if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE2D {
        if dx10_header.misc_flag == DDS_RESOURCE_MISC_FLAGS::TEXTURECUBE {
            image_format.image_type = ImageType::IMAGECUBE;
            if dx10_header.array_size > 1 {
                image_format.layers = dx10_header.array_size;
            } else {
                image_format.layers = 6;
            }
        } else {
            image_format.image_type = ImageType::IMAGE2D;
            image_format.layers = dx10_header.array_size;
        }
    } else if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE3D {
        image_format.image_type = ImageType::IMAGE3D;
        image_format.layers = dds_header.depth;
    }

    if dx10_header.array_size > 1 {
        match image_format.image_type {
            ImageType::IMAGE2D => image_format.image_type = ImageType::IMAGE2D_ARRAY,
            ImageType::IMAGECUBE => image_format.image_type = ImageType::IMAGECUBE_ARRAY,
            ImageType::IMAGE3D => {
                eyre::bail!("3D-Arrays are not a supported image format")
            }
            _ => {}
        }
    }

    Ok(image_format)
}
```

```diff
diff --git a/lib/sdk/src/filetype/texture.rs b/lib/sdk/src/filetype/texture.rs
index 7912e45..58c0dbf 100644
--- a/lib/sdk/src/filetype/texture.rs
+++ b/lib/sdk/src/filetype/texture.rs
@@ -14,6 +14,8 @@ use crate::bundle::file::UserFile;
 use crate::murmur::{HashGroup, IdString32, IdString64};
 use crate::{BundleFile, BundleFileType, BundleFileVariant};

+mod dds;
+
 #[derive(Clone, Debug, Deserialize, Serialize)]
 struct TextureDefinition {
     common: TextureDefinitionPlatform,
@@ -53,6 +55,7 @@ struct TextureHeader {
     n_streamable_mipmaps: u32,
     width: u32,
     height: u32,
+    mip_info_size: u32,
 }

 impl TextureHeader {
@@ -66,18 +69,19 @@ impl TextureHeader {
         let width = r.read_u32()?;
         let height = r.read_u32()?;

-        // Don't quite know yet what this is, only that it is related to mipmaps.
-        // The reference to "streamable mipmaps" comes from VT2, so far.
-        // As such, it might be related to the stream file, but since all texture files have it,
-        // The engine calculates some offset and then moves 68 bytes at that offset to the beginning.
-        // Hence the split between `68` and `60` in the length.
-        r.seek(SeekFrom::Current(68 + 60))?;
+        r.skip_u32(0)?;
+
+        // A section of 15 pairs of two u32
+        r.seek(SeekFrom::Current(2 * 4 * 15))?;
+
+        let mip_info_size = r.read_u32()?;

         Ok(Self {
             flags,
             n_streamable_mipmaps,
             width,
             height,
+            mip_info_size,
         })
     }

@@ -94,9 +98,12 @@ impl TextureHeader {
         w.write_u32(self.height)?;

         // See `from_binary` about this unknown section.
-        let buf = [0; 148];
+        let buf = [0; (2 * 4 * 15) + 4];
         w.write_all(&buf)?;

+        // TODO: For now we write `0` here, until the mipmap section is figured out
+        w.write_u32(0)?;
+
         Ok(())
     }
 }
@@ -154,12 +161,10 @@ impl Texture {

         let header = TextureHeader::from_binary(&mut r)?;

-        let meta_size = r.read_u32()?;
-
         eyre::ensure!(
-            meta_size == 0 || stream_r.is_some(),
-            "Compression chunks and stream file don't match up. meta_size = {}, stream = {}",
-            meta_size,
+            header.mip_info_size == 0 || stream_r.is_some(),
+            "Compression chunks and stream file don't match up. mip_info_size = {}, has_stream = {}",
+            header.mip_info_size,
             stream_r.is_some()
         );

diff --git a/lib/sdk/src/filetype/texture/dds.rs b/lib/sdk/src/filetype/texture/dds.rs
new file mode 100644
index 0000000..7129b9e
--- /dev/null
+++ b/lib/sdk/src/filetype/texture/dds.rs
@@ -0,0 +1,502 @@
+use bitflags::bitflags;
+use color_eyre::eyre;
+use color_eyre::Result;
+
+const FOURCC_DXT1: u32 = 0x31545844;
+const FOURCC_DXT3: u32 = 0x33545844;
+const FOURCC_DXT5: u32 = 0x35545844;
+const FOURCC_AXI1: u32 = 0x31495441;
+const FOURCC_AXI2: u32 = 0x32495441;
+const FOURCC_D3D_A16B16G16R16: u32 = 0x24;
+const FOURCC_D3D_R16F: u32 = 0x6F;
+const FOURCC_D3D_G16R16F: u32 = 0x70;
+const FOURCC_D3D_A16B16G16R16F: u32 = 0x71;
+const FOURCC_D3D_R32F: u32 = 0x72;
+const FOURCC_D3D_G32R32F: u32 = 0x73;
+const FOURCC_D3D_A32B32G32R32F: u32 = 0x74;
+
+bitflags! {
+    #[derive(Clone, Copy, Debug)]
+    pub struct DDSD: u32 {
+        /// Required
+        const CAPS = 0x1;
+        /// Required
+        const HEIGHT = 0x2;
+        /// Required
+        const WIDTH = 0x4;
+        /// Pitch for an uncompressed texture
+        const PITCH = 0x8;
+        /// Required
+        const PIXELFORMAT = 0x1000;
+        /// Required in a mipmapped texture
+        const MIPMAPCOUNT = 0x20000;
+        /// Pitch for a compressed texture
+        const LINEARSIZE = 0x80000;
+        /// Required in a depth texture
+        const DEPTH = 0x800000;
+    }
+
+    #[derive(Clone, Copy, Debug)]
+    pub struct DDSCAPS: u32 {
+        const COMPLEX = 0x8;
+        const MIPMAP = 0x400000;
+        const TEXTURE = 0x1000;
+    }
+
+    #[derive(Clone, Copy, Debug)]
+    pub struct DDSCAPS2: u32 {
+        const CUBEMAP = 0x200;
+        const CUBEMAP_POSITIVEX = 0x400;
+        const CUBEMAP_NEGATIVEX = 0x800;
+        const CUBEMAP_POSITIVEY = 0x1000;
+        const CUBEMAP_NEGATIVEY = 0x2000;
+        const CUBEMAP_POSITIVEZ = 0x4000;
+        const CUBEMAP_NEGATIVEZ = 0x8000;
+        const VOLUME = 0x200000;
+
+        const CUBEMAP_ALLFACES = Self::CUBEMAP_POSITIVEX.bits()
+            | Self::CUBEMAP_NEGATIVEX.bits()
+            | Self::CUBEMAP_POSITIVEY.bits()
+            | Self::CUBEMAP_NEGATIVEY.bits()
+            | Self::CUBEMAP_POSITIVEZ.bits()
+            | Self::CUBEMAP_NEGATIVEZ.bits();
+    }
+
+    #[derive(Clone, Copy, Debug)]
+    pub struct DDPF: u32 {
+        const ALPHAPIXELS = 0x1;
+        const ALPHA = 0x2;
+        const FOURCC = 0x4;
+        const RGB = 0x40;
+        const YUV = 0x200;
+        const LUMINANCE = 0x20000;
+    }
+}
+
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+pub enum DDS_RESOURCE_MISC_FLAGS {
+    TEXTURECUBE = 0x4,
+}
+
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+pub enum D3D10_RESOURCE_DIMENSION {
+    UNKNOWN = 0,
+    BUFFER = 1,
+    TEXTURE1D = 2,
+    TEXTURE2D = 3,
+    TEXTURE3D = 4,
+}
+
+// Mostly copied from VT2's PDB.
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+pub enum PixelFormat {
+    R8G8B8A8 = 0x0,
+    R32F = 0x1,
+    R16I = 0x2,
+    DEPTH_STENCIL = 0x3,
+    BC1 = 0x4,
+    BC2 = 0x5,
+    BC3 = 0x6,
+    BC4 = 0x7,
+    BC5 = 0x8,
+    BC6H_UF16 = 0x9,
+    BC6H_SF16 = 0xA,
+    BC7 = 0xB,
+    R32G32B32A32F = 0xC,
+    SHADOW_MAP = 0xD,
+    BUFFER_32F = 0xE,
+    R16F = 0xF,
+    R16G16B16A16F = 0x10,
+    R16UNORM = 0x11,
+    R8G8 = 0x12,
+    R16G16UNORM = 0x13,
+    R16G16F = 0x14,
+    R8 = 0x15,
+    R32G32UINT = 0x16,
+    R16UINT = 0x17,
+    R32UINT = 0x18,
+    R11G11B10F = 0x19,
+    R32G32F = 0x1A,
+    R10G10B10A2UNORM = 0x1B,
+    // The next two did not exist in VT2.
+    // Names are assumed based on which entries in `DXGI_FORMAT` they are mapped to.
+    R16G16B16A16UINT = 0x1C,
+    R16G16B16A16UNORM = 0x1D,
+    // With the addition of the two other entries, this is probably `UNKNOWN` from VT2.
+    // The gap here is weird, so maybe there are two more entries that the engine code
+    // simply never maps.
+    UNKNOWN = 0x20,
+}
+
+impl PixelFormat {
+    /// Maps an [DXGI_FORMAT](https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format)
+    /// to Fatshark's internal `PixelFormat` enum.
+    pub fn from_dxgi(dxgi_format: u32) -> Self {
+        match dxgi_format {
+            1..=4 => PixelFormat::R32G32B32A32F,
+            0xA => PixelFormat::R16G16B16A16F,
+            0xB => PixelFormat::R16G16B16A16UNORM,
+            0xC => PixelFormat::R16G16B16A16UINT,
+            0xF | 0x10 | 0x12 => PixelFormat::R32G32F,
+            0x11 => PixelFormat::R32G32UINT,
+            0x17 | 0x18 | 0x19 => PixelFormat::R10G10B10A2UNORM,
+            0x1A => PixelFormat::R11G11B10F,
+            0x1B..=0x20 => PixelFormat::R8G8B8A8,
+            0x22 => PixelFormat::R16G16F,
+            0x23 => PixelFormat::R16G16UNORM,
+            0x27 | 0x29 | 0x2B => PixelFormat::BUFFER_32F,
+            0x28 => PixelFormat::SHADOW_MAP,
+            0x2A => PixelFormat::R32UINT,
+            0x2D => PixelFormat::DEPTH_STENCIL,
+            0x30..=0x34 => PixelFormat::R8G8,
+            0x36 => PixelFormat::R16F,
+            0x38 => PixelFormat::R16UNORM,
+            0x39 => PixelFormat::R16UINT,
+            0x3B => PixelFormat::R16I,
+            0x3C..=0x40 => PixelFormat::R8,
+            0x46..=0x48 => PixelFormat::BC1,
+            0x49..=0x4B => PixelFormat::BC2,
+            0x4C..=0x4E => PixelFormat::BC3,
+            0x4F..=0x51 => PixelFormat::BC4,
+            0x52..=0x54 => PixelFormat::BC5,
+            0x5F => PixelFormat::BC6H_UF16,
+            0x60 => PixelFormat::BC6H_SF16,
+            0x61..=0x63 => PixelFormat::BC7,
+            _ => PixelFormat::UNKNOWN,
+        }
+    }
+}
+
+pub struct DX10_Header {
+    /// Resource data formats, including fully-typed and typeless formats.
+    /// See https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format
+    dxgi_format: u32,
+    resource_dimension: D3D10_RESOURCE_DIMENSION,
+    misc_flag: DDS_RESOURCE_MISC_FLAGS,
+    array_size: u32,
+    misc_flags2: u32,
+}
+
+pub struct DDSPixelFormat {
+    /// Structure size. Must be `32`.
+    size: u32,
+    flags: DDPF,
+    four_cc: u32,
+    rgb_bit_count: u32,
+    r_bit_mask: u32,
+    g_bit_mask: u32,
+    b_bit_mask: u32,
+    a_bit_mask: u32,
+}
+
+pub struct DDSHeader {
+    /// Size of this structure. Must be `124`.
+    size: u32,
+    /// Flags to indicate which members contain valid data.
+    flags: DDSD,
+    height: u32,
+    width: u32,
+    pitch_or_linear_size: u32,
+    depth: u32,
+    mipmap_count: u32,
+    reserved_1: [u8; 11],
+    pixel_format: DDSPixelFormat,
+    caps: DDSCAPS,
+    caps_2: DDSCAPS2,
+    reserved_2: [u8; 3],
+}
+
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+pub enum ImageType {
+    IMAGE2D = 0,
+    IMAGE3D = 1,
+    IMAGECUBE = 2,
+    UNKNOWN = 3,
+    IMAGE2D_ARRAY = 4,
+    IMAGECUBE_ARRAY = 5,
+}
+
+#[derive(Clone, Copy, Debug, PartialEq, Eq)]
+pub enum ImageValidity {
+    STATIC = 0,
+    UPDATABLE = 1,
+    DYNAMIC = 2,
+}
+
+pub struct ImageFormat {
+    pub pixel_format: PixelFormat,
+    pub image_type: ImageType,
+    pub validity: ImageValidity,
+    pub width: u32,
+    pub height: u32,
+    pub layers: u32,
+    pub mip_levels: u32,
+    pub srgb: bool,
+}
+
+/// A stripped version of `ImageType` that only contains just the data needed
+/// to read a DDS image stream.
+pub struct StrippedImageFormat {
+    pub image_type: ImageType,
+    pub width: u32,
+    pub height: u32,
+    pub layers: u32,
+    pub mip_levels: u32,
+}
+
+fn label_98(dds_header: &DDSHeader, image_format: &mut ImageFormat) -> Result<()> {
+    image_format.width = dds_header.width;
+    image_format.height = dds_header.height;
+
+    if dds_header.mipmap_count == 0 {
+        image_format.mip_levels = 1;
+    } else {
+        image_format.mip_levels = dds_header.mipmap_count;
+    }
+
+    if dds_header.caps_2.contains(DDSCAPS2::CUBEMAP) {
+        image_format.image_type = ImageType::IMAGECUBE;
+        image_format.layers = 6;
+    } else if dds_header.caps_2.contains(DDSCAPS2::VOLUME) {
+        image_format.image_type = ImageType::IMAGE3D;
+        image_format.layers = dds_header.depth;
+    } else {
+        image_format.image_type = ImageType::IMAGE2D;
+        image_format.layers = 1;
+    }
+
+    return Ok(());
+}
+
+fn label_46(
+    dds_header: &DDSHeader,
+    dx10_header: &DX10_Header,
+    image_format: &mut ImageFormat,
+) -> Result<()> {
+    if dds_header.pixel_format.four_cc != 0x30315844 {
+        eyre::bail!("Unsupported DDS type");
+    }
+
+    image_format.pixel_format = match PixelFormat::from_dxgi(dx10_header.dxgi_format) {
+        PixelFormat::UNKNOWN => {
+            eyre::bail!(
+                "dxgi format '{}' is not yet supported",
+                dx10_header.dxgi_format
+            );
+        }
+        format => format,
+    };
+
+    if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE2D {
+        if dx10_header.misc_flag == DDS_RESOURCE_MISC_FLAGS::TEXTURECUBE {
+            image_format.image_type = ImageType::IMAGECUBE;
+            if dx10_header.array_size > 1 {
+                image_format.layers = dx10_header.array_size;
+            } else {
+                image_format.layers = 6;
+            }
+        } else {
+            image_format.image_type = ImageType::IMAGE2D;
+            image_format.layers = dx10_header.array_size;
+        }
+    } else if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE3D {
+        image_format.image_type = ImageType::IMAGE3D;
+        image_format.layers = dds_header.depth;
+    }
+
+    if dx10_header.array_size > 1 {
+        match image_format.image_type {
+            ImageType::IMAGE2D => image_format.image_type = ImageType::IMAGE2D_ARRAY,
+            ImageType::IMAGECUBE => image_format.image_type = ImageType::IMAGECUBE_ARRAY,
+            ImageType::IMAGE3D => {
+                eyre::bail!("3D-Arrays are not a supported image format")
+            }
+            _ => {}
+        }
+    }
+
+    image_format.width = dds_header.width;
+    image_format.height = dds_header.height;
+
+    if dds_header.mipmap_count > 0 {
+        image_format.mip_levels = dds_header.mipmap_count;
+    } else {
+        image_format.mip_levels = 1;
+    }
+
+    return Ok(());
+}
+
+pub fn image_format_from_header(
+    dds_header: &DDSHeader,
+    dx10_header: &DX10_Header,
+    image_format: &mut ImageFormat,
+) -> Result<()> {
+    let ddspf = &dds_header.pixel_format;
+
+    match ddspf.four_cc {
+        FOURCC_DXT1 => {
+            image_format.pixel_format = PixelFormat::BC1;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_DXT3 => {
+            image_format.pixel_format = PixelFormat::BC2;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_DXT5 => {
+            image_format.pixel_format = PixelFormat::BC3;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_AXI1 => {
+            image_format.pixel_format = PixelFormat::BC4;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_AXI2 => {
+            image_format.pixel_format = PixelFormat::BC5;
+            return label_98(dds_header, image_format);
+        }
+        _ => {}
+    }
+
+    if ddspf.rgb_bit_count == 32 {
+        // This is a heavily collapsed version of what the decompiled code does through `goto`s.
+        // It doesn't really make sense that these two very different forms map to the same format,
+        // but this is what Fatshark's code does. And their direct mapping above also omits all of
+        // the BGR formats.
+        let is_argb = ddspf.a_bit_mask == 0xFF000000
+            && ddspf.r_bit_mask == 0xFF0000
+            && ddspf.g_bit_mask == 0xFF00
+            && ddspf.b_bit_mask == 0xFF;
+        let is_bgr = ddspf.a_bit_mask == 0x0
+            && ddspf.r_bit_mask == 0xFF
+            && ddspf.g_bit_mask == 0xFF00
+            && ddspf.b_bit_mask == 0xFF0000;
+        if is_argb || is_bgr {
+            image_format.pixel_format = PixelFormat::R8G8B8A8;
+            return label_98(dds_header, image_format);
+        }
+    } else if ddspf.rgb_bit_count == 8 && ddspf.r_bit_mask == 0xFF {
+        image_format.pixel_format = PixelFormat::R8;
+        return label_98(dds_header, image_format);
+    }
+
+    match dds_header.pixel_format.four_cc {
+        FOURCC_D3D_R16F => {
+            image_format.pixel_format = PixelFormat::R16F;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_R32F => {
+            image_format.pixel_format = PixelFormat::R32F;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_G16R16F => {
+            image_format.pixel_format = PixelFormat::R16G16F;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_G32R32F => {
+            image_format.pixel_format = PixelFormat::R32G32F;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_A16B16G16R16F => {
+            image_format.pixel_format = PixelFormat::R16G16B16A16F;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_A16B16G16R16 => {
+            image_format.pixel_format = PixelFormat::R16G16B16A16UNORM;
+            return label_98(dds_header, image_format);
+        }
+        FOURCC_D3D_A32B32G32R32F => {
+            image_format.pixel_format = PixelFormat::R32G32B32A32F;
+            return label_98(dds_header, image_format);
+        }
+        _ => {}
+    }
+
+    if ddspf.r_bit_mask == 0xFFFF {
+        if ddspf.g_bit_mask == 0 {
+            if ddspf.b_bit_mask == 0 && ddspf.a_bit_mask == 0 {
+                image_format.pixel_format = PixelFormat::R16UNORM;
+                return label_98(dds_header, image_format);
+            }
+            return label_46(dds_header, dx10_header, image_format);
+        }
+
+        if ddspf.g_bit_mask != 0xFFFF || ddspf.b_bit_mask != 0 || ddspf.a_bit_mask != 0 {
+            return label_46(dds_header, dx10_header, image_format);
+        }
+
+        image_format.pixel_format = PixelFormat::R16G16F;
+        return label_98(dds_header, image_format);
+    }
+
+    label_46(dds_header, dx10_header, image_format)
+}
+
+// This is a stripped down version of the logic that the engine implements to fill
+// `stingray::ImageFormat`. With the `type` field we need to distinguish between `IMAGE3D`
+// and everything else, and we the various dimensions filled to calculate the chunks.
+pub fn stripped_format_from_header(
+    dds_header: &DDSHeader,
+    dx10_header: &DX10_Header,
+) -> Result<StrippedImageFormat> {
+    let mut image_format = StrippedImageFormat {
+        image_type: ImageType::UNKNOWN,
+        width: dds_header.width,
+        height: dds_header.height,
+        layers: 0,
+        mip_levels: 0,
+    };
+
+    if dds_header.mipmap_count > 0 {
+        image_format.mip_levels = dds_header.mipmap_count;
+    } else {
+        image_format.mip_levels = 1;
+    }
+
+    // INFO: These next two sections are conditional in the engine code,
+    // based on a lot of stuff in "fourcc" and other fields. But it might
+    // actually be fine to just do it like this, as this seems universal
+    // to DDS.
+    // Will have to check how it plays out with actual assets.
+
+    if dds_header.caps_2.contains(DDSCAPS2::CUBEMAP) {
+        image_format.image_type = ImageType::IMAGECUBE;
+        image_format.layers = 6;
+    } else if dds_header.caps_2.contains(DDSCAPS2::VOLUME) {
+        image_format.image_type = ImageType::IMAGE3D;
+        image_format.layers = dds_header.depth;
+    } else {
+        image_format.image_type = ImageType::IMAGE2D;
+        image_format.layers = 1;
+    }
+
+    if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE2D {
+        if dx10_header.misc_flag == DDS_RESOURCE_MISC_FLAGS::TEXTURECUBE {
+            image_format.image_type = ImageType::IMAGECUBE;
+            if dx10_header.array_size > 1 {
+                image_format.layers = dx10_header.array_size;
+            } else {
+                image_format.layers = 6;
+            }
+        } else {
+            image_format.image_type = ImageType::IMAGE2D;
+            image_format.layers = dx10_header.array_size;
+        }
+    } else if dx10_header.resource_dimension == D3D10_RESOURCE_DIMENSION::TEXTURE3D {
+        image_format.image_type = ImageType::IMAGE3D;
+        image_format.layers = dds_header.depth;
+    }
+
+    if dx10_header.array_size > 1 {
+        match image_format.image_type {
+            ImageType::IMAGE2D => image_format.image_type = ImageType::IMAGE2D_ARRAY,
+            ImageType::IMAGECUBE => image_format.image_type = ImageType::IMAGECUBE_ARRAY,
+            ImageType::IMAGE3D => {
+                eyre::bail!("3D-Arrays are not a supported image format")
+            }
+            _ => {}
+        }
+    }
+
+    Ok(image_format)
+}
```