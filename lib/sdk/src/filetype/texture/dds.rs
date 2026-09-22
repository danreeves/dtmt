use std::io::SeekFrom;

use bitflags::bitflags;
use color_eyre::Result;
use color_eyre::eyre::Context as _;
use color_eyre::eyre::{self, OptionExt as _};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive as _, ToPrimitive as _};

use crate::binary;
use crate::binary::sync::{ReadExt, WriteExt};

const MAGIC_DDS: u32 = 0x20534444;

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

    #[derive(Clone, Copy, Debug)]
    pub struct DdsResourceMiscFlags: u32 {
        const TEXTURECUBE = 0x4;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, FromPrimitive, ToPrimitive)]
#[repr(u32)]
pub enum D3D10ResourceDimension {
    Unknown = 0,
    Buffer = 1,
    Texture1D = 2,
    Texture2D = 3,
    Texture3D = 4,
}

#[allow(clippy::upper_case_acronyms)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive)]
#[repr(u32)]
pub enum DXGIFormat {
    UNKNOWN = 0,
    R32G32B32A32_TYPELESS = 1,
    R32G32B32A32_FLOAT = 2,
    R32G32B32A32_UINT = 3,
    R32G32B32A32_SINT = 4,
    R32G32B32_TYPELESS = 5,
    R32G32B32_FLOAT = 6,
    R32G32B32_UINT = 7,
    R32G32B32_SINT = 8,
    R16G16B16A16_TYPELESS = 9,
    R16G16B16A16_FLOAT = 10,
    R16G16B16A16_UNORM = 11,
    R16G16B16A16_UINT = 12,
    R16G16B16A16_SNORM = 13,
    R16G16B16A16_SINT = 14,
    R32G32_TYPELESS = 15,
    R32G32_FLOAT = 16,
    R32G32_UINT = 17,
    R32G32_SINT = 18,
    R32G8X24_TYPELESS = 19,
    D32_FLOAT_S8X24_UINT = 20,
    R32_FLOAT_X8X24_TYPELESS = 21,
    X32_TYPELESS_G8X24_UINT = 22,
    R10G10B10A2_TYPELESS = 23,
    R10G10B10A2_UNORM = 24,
    R10G10B10A2_UINT = 25,
    R11G11B10_FLOAT = 26,
    R8G8B8A8_TYPELESS = 27,
    R8G8B8A8_UNORM = 28,
    R8G8B8A8_UNORM_SRGB = 29,
    R8G8B8A8_UINT = 30,
    R8G8B8A8_SNORM = 31,
    R8G8B8A8_SINT = 32,
    R16G16_TYPELESS = 33,
    R16G16_FLOAT = 34,
    R16G16_UNORM = 35,
    R16G16_UINT = 36,
    R16G16_SNORM = 37,
    R16G16_SINT = 38,
    R32_TYPELESS = 39,
    D32_FLOAT = 40,
    R32_FLOAT = 41,
    R32_UINT = 42,
    R32_SINT = 43,
    R24G8_TYPELESS = 44,
    D24_UNORM_S8_UINT = 45,
    R24_UNORM_X8_TYPELESS = 46,
    X24_TYPELESS_G8_UINT = 47,
    R8G8_TYPELESS = 48,
    R8G8_UNORM = 49,
    R8G8_UINT = 50,
    R8G8_SNORM = 51,
    R8G8_SINT = 52,
    R16_TYPELESS = 53,
    R16_FLOAT = 54,
    D16_UNORM = 55,
    R16_UNORM = 56,
    R16_UINT = 57,
    R16_SNORM = 58,
    R16_SINT = 59,
    R8_TYPELESS = 60,
    R8_UNORM = 61,
    R8_UINT = 62,
    R8_SNORM = 63,
    R8_SINT = 64,
    A8_UNORM = 65,
    R1_UNORM = 66,
    R9G9B9E5_SHAREDEXP = 67,
    R8G8_B8G8_UNORM = 68,
    G8R8_G8B8_UNORM = 69,
    BC1_TYPELESS = 70,
    BC1_UNORM = 71,
    BC1_UNORM_SRGB = 72,
    BC2_TYPELESS = 73,
    BC2_UNORM = 74,
    BC2_UNORM_SRGB = 75,
    BC3_TYPELESS = 76,
    BC3_UNORM = 77,
    BC3_UNORM_SRGB = 78,
    BC4_TYPELESS = 79,
    BC4_UNORM = 80,
    BC4_SNORM = 81,
    BC5_TYPELESS = 82,
    BC5_UNORM = 83,
    BC5_SNORM = 84,
    B5G6R5_UNORM = 85,
    B5G5R5A1_UNORM = 86,
    B8G8R8A8_UNORM = 87,
    B8G8R8X8_UNORM = 88,
    R10G10B10_XR_BIAS_A2_UNORM = 89,
    B8G8R8A8_TYPELESS = 90,
    B8G8R8A8_UNORM_SRGB = 91,
    B8G8R8X8_TYPELESS = 92,
    B8G8R8X8_UNORM_SRGB = 93,
    BC6H_TYPELESS = 94,
    BC6H_UF16 = 95,
    BC6H_SF16 = 96,
    BC7_TYPELESS = 97,
    BC7_UNORM = 98,
    BC7_UNORM_SRGB = 99,
    AYUV = 100,
    Y410 = 101,
    Y416 = 102,
    NV12 = 103,
    P010 = 104,
    P016 = 105,
    OPAQUE = 106,
    YUY2 = 107,
    Y210 = 108,
    Y216 = 109,
    NV11 = 110,
    AI44 = 111,
    IA44 = 112,
    P8 = 113,
    A8P8 = 114,
    B4G4R4A4_UNORM = 115,
    P208 = 130,
    V208 = 131,
    V408 = 132,
    SAMPLER_FEEDBACK_MIN_MIP_OPAQUE,
    SAMPLER_FEEDBACK_MIP_REGION_USED_OPAQUE,
}

impl std::fmt::Display for DXGIFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl DXGIFormat {
    /// Returns the number of bytes per 4x4 block if this is a block-compressed
    /// format, or `None` for uncompressed formats.
    pub fn block_bytes(&self) -> Option<usize> {
        use DXGIFormat::*;

        match self {
            BC1_TYPELESS | BC1_UNORM | BC1_UNORM_SRGB | BC4_TYPELESS | BC4_UNORM | BC4_SNORM => {
                Some(8)
            }
            BC2_TYPELESS | BC2_UNORM | BC2_UNORM_SRGB | BC3_TYPELESS | BC3_UNORM
            | BC3_UNORM_SRGB | BC5_TYPELESS | BC5_UNORM | BC5_SNORM | BC6H_TYPELESS | BC6H_UF16
            | BC6H_SF16 | BC7_TYPELESS | BC7_UNORM | BC7_UNORM_SRGB => Some(16),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Dx10Header {
    /// Resource data formats, including fully-typed and typeless formats.
    /// See https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format
    pub dxgi_format: DXGIFormat,
    pub resource_dimension: D3D10ResourceDimension,
    pub misc_flag: DdsResourceMiscFlags,
    pub array_size: usize,
    pub misc_flags2: u32,
}

impl Dx10Header {
    #[tracing::instrument("Dx10Header::from_binary", skip(r))]
    pub fn from_binary(mut r: impl ReadExt) -> Result<Self> {
        let dxgi_format = r
            .read_u32()
            .map(|val| DXGIFormat::from_u32(val).unwrap_or(DXGIFormat::UNKNOWN))?;
        let resource_dimension = r.read_u32().map(|val| {
            D3D10ResourceDimension::from_u32(val).unwrap_or(D3D10ResourceDimension::Unknown)
        })?;
        let misc_flag = r.read_u32().map(binary::flags_from_bits)?;
        let array_size = r.read_u32()? as usize;
        let misc_flags2 = r.read_u32()?;

        Ok(Self {
            dxgi_format,
            resource_dimension,
            misc_flag,
            array_size,
            misc_flags2,
        })
    }

    #[tracing::instrument("Dx10Header::to_binary", skip(w))]
    pub fn to_binary(&self, mut w: impl WriteExt) -> Result<()> {
        w.write_u32(
            self.dxgi_format
                .to_u32()
                .ok_or_eyre("DXGIFormat should fit in a u32")?,
        )?;
        w.write_u32(
            self.resource_dimension
                .to_u32()
                .ok_or_eyre("D3D10ResourceDimension should fit in a u32")?,
        )?;
        w.write_u32(self.misc_flag.bits())?;
        w.write_u32(self.array_size as u32)?;
        w.write_u32(self.misc_flags2)?;

        Ok(())
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromPrimitive, ToPrimitive)]
#[repr(u32)]
pub enum FourCC {
    Empty = u32::MAX,
    DXT1 = 0x31545844,
    DXT2 = 0x32545844,
    DXT3 = 0x33545844,
    DXT4 = 0x34545844,
    DXT5 = 0x35545844,
    AXI1 = 0x31495441,
    AXI2 = 0x32495441,
    DX10 = 0x30315844,
    D3D_A16B16G16R16 = 0x24,
    D3D_R16F = 0x6F,
    D3D_G16R16F = 0x70,
    D3D_A16B16G16R16F = 0x71,
    D3D_R32F = 0x72,
    D3D_G32R32F = 0x73,
    D3D_A32B32G32R32F = 0x74,
}

impl FourCC {
    /// Number of bytes per 4x4 block for legacy block-compressed FourCCs.
    pub fn block_bytes(&self) -> Option<usize> {
        match self {
            FourCC::DXT1 => Some(8),
            FourCC::DXT2 | FourCC::DXT3 | FourCC::DXT4 | FourCC::DXT5 => Some(16),
            _ => None,
        }
    }
}

impl std::fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DDSPixelFormat {
    pub flags: DDPF,
    pub four_cc: FourCC,
    pub rgb_bit_count: u32,
    pub r_bit_mask: u32,
    pub g_bit_mask: u32,
    pub b_bit_mask: u32,
    pub a_bit_mask: u32,
}

impl DDSPixelFormat {
    #[tracing::instrument("DDSPixelFormat::from_binary", skip(r))]
    pub fn from_binary(mut r: impl ReadExt) -> Result<Self> {
        let size = r.read_u32()? as usize;
        eyre::ensure!(
            size == 32,
            "Invalid structure size. Got 0X{:0X}, expected 0x20",
            size
        );

        let flags: DDPF = r.read_u32().map(binary::flags_from_bits)?;

        let four_cc = if flags.contains(DDPF::FOURCC) {
            r.read_u32().and_then(|bytes| {
                FourCC::from_u32(bytes).ok_or_eyre(format!("Unknown FourCC value: {:08X}", bytes))
            })?
        } else {
            r.skip_u32(0)?;
            FourCC::Empty
        };

        let rgb_bit_count = r.read_u32()?;
        let r_bit_mask = r.read_u32()?;
        let g_bit_mask = r.read_u32()?;
        let b_bit_mask = r.read_u32()?;
        let a_bit_mask = r.read_u32()?;

        Ok(Self {
            flags,
            four_cc,
            rgb_bit_count,
            r_bit_mask,
            g_bit_mask,
            b_bit_mask,
            a_bit_mask,
        })
    }

    #[tracing::instrument("DDSPixelFormat::to_binary", skip(w))]
    pub fn to_binary(&self, mut w: impl WriteExt) -> Result<()> {
        // Structure size
        w.write_u32(32)?;

        w.write_u32(self.flags.bits())?;
        w.write_u32(self.four_cc.to_u32().unwrap_or_default())?;
        w.write_u32(self.rgb_bit_count)?;
        w.write_u32(self.r_bit_mask)?;
        w.write_u32(self.g_bit_mask)?;
        w.write_u32(self.b_bit_mask)?;
        w.write_u32(self.a_bit_mask)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DDSHeader {
    /// Flags to indicate which members contain valid data.
    pub flags: DDSD,
    pub height: usize,
    pub width: usize,
    pub pitch_or_linear_size: usize,
    pub depth: usize,
    pub mipmap_count: usize,
    pub pixel_format: DDSPixelFormat,
    pub caps: DDSCAPS,
    pub caps_2: DDSCAPS2,
}

impl DDSHeader {
    #[tracing::instrument("DDSHeader::from_binary", skip(r))]
    pub fn from_binary(mut r: impl ReadExt) -> Result<Self> {
        r.skip_u32(MAGIC_DDS).wrap_err("Invalid magic bytes")?;

        let size = r.read_u32()?;
        eyre::ensure!(
            size == 124,
            "Invalid structure size. Got 0x{:0X}, expected 0x7C",
            size
        );

        let flags = r.read_u32().map(binary::flags_from_bits)?;
        let height = r.read_u32()? as usize;
        let width = r.read_u32()? as usize;
        let pitch_or_linear_size = r.read_u32()? as usize;
        let depth = r.read_u32()? as usize;
        let mipmap_count = r.read_u32()? as usize;

        // Skip reserved bytes
        r.seek(SeekFrom::Current(11 * 4))?;

        let pixel_format = DDSPixelFormat::from_binary(&mut r)?;
        let caps = r.read_u32().map(binary::flags_from_bits)?;
        let caps_2 = r.read_u32().map(binary::flags_from_bits)?;

        // Skip unused and reserved bytes
        r.seek(SeekFrom::Current(3 * 4))?;

        Ok(Self {
            flags,
            height,
            width,
            pitch_or_linear_size,
            depth,
            mipmap_count,
            pixel_format,
            caps,
            caps_2,
        })
    }

    #[tracing::instrument("DDSHeader::to_binary", skip(w))]
    pub fn to_binary(&self, mut w: impl WriteExt) -> Result<()> {
        w.write_u32(MAGIC_DDS)?;

        // Structure size in bytes
        w.write_u32(124)?;
        w.write_u32(self.flags.bits())?;
        w.write_u32(self.height as u32)?;
        w.write_u32(self.width as u32)?;
        w.write_u32(self.pitch_or_linear_size as u32)?;
        w.write_u32(self.depth as u32)?;
        w.write_u32(self.mipmap_count as u32)?;

        w.write_all(&[0u8; 11 * 4])?;

        self.pixel_format.to_binary(&mut w)?;
        w.write_u32(self.caps.bits())?;
        w.write_u32(self.caps_2.bits())?;

        w.write_all(&[0u8; 3 * 4])?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ImageType {
    Image2D = 0,
    Image3D = 1,
    ImageCube = 2,
    Unknown = 3,
    Image2dArray = 4,
    ImagecubeArray = 5,
}

impl std::fmt::Display for ImageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// A stripped version of `ImageType` that only contains just the data needed
/// to read a DDS image stream.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct StrippedImageFormat {
    pub image_type: ImageType,
    pub width: usize,
    pub height: usize,
    pub layers: usize,
    pub mip_levels: usize,
}

// This is a stripped down version of the logic that the engine implements to fill
// `stingray::ImageFormat`. With the `type` field we need to distinguish between `IMAGE3D`
// and everything else, and we need the various dimensions filled to calculate the chunks.
pub fn stripped_format_from_header(
    dds_header: &DDSHeader,
    dx10_header: &Dx10Header,
) -> Result<StrippedImageFormat> {
    let mut image_format = StrippedImageFormat {
        image_type: ImageType::Unknown,
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
        image_format.image_type = ImageType::ImageCube;
        image_format.layers = 6;
    } else if dds_header.caps_2.contains(DDSCAPS2::VOLUME) {
        image_format.image_type = ImageType::Image3D;
        image_format.layers = dds_header.depth;
    } else {
        image_format.image_type = ImageType::Image2D;
        image_format.layers = 1;
    }

    if dx10_header.resource_dimension == D3D10ResourceDimension::Texture2D {
        if dx10_header
            .misc_flag
            .contains(DdsResourceMiscFlags::TEXTURECUBE)
        {
            image_format.image_type = ImageType::ImageCube;
            if dx10_header.array_size > 1 {
                image_format.layers = dx10_header.array_size;
            } else {
                image_format.layers = 6;
            }
        } else {
            image_format.image_type = ImageType::Image2D;
            image_format.layers = dx10_header.array_size;
        }
    } else if dx10_header.resource_dimension == D3D10ResourceDimension::Texture3D {
        image_format.image_type = ImageType::Image3D;
        image_format.layers = dds_header.depth;
    }

    if dx10_header.array_size > 1 {
        match image_format.image_type {
            ImageType::Image2D => image_format.image_type = ImageType::Image2dArray,
            ImageType::ImageCube => image_format.image_type = ImageType::ImagecubeArray,
            ImageType::Image3D => {
                eyre::bail!("3D-Arrays are not a supported image format")
            }
            _ => {}
        }
    }

    Ok(image_format)
}
