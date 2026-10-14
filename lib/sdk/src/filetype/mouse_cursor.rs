//! The `mouse_cursor` resource: a small bitmap with a hotspot.
//!
//! ```text
//! 0   u32   width
//! 4   u32   height
//! 8   u32   hotspot_x     9 in both shipped cursors
//! 12  u32   hotspot_y     9 in both shipped cursors
//! 16  u32   byte count    the pixel data's length
//! 20  [u8]  the pixels, four bytes each, BGRA, bottom row first
//! ```
//!
//! Only **two** of these ship. The size word closes arithmetically - `20 + it` is
//! the whole resource - and that is the check [`MouseCursor::from_binary`] makes,
//! exactly as `wwise_dep` does with its name length. The second relation,
//! `byte count == width * height * 4`, holds for the sampled cursor but is *not*
//! required: two files cannot establish it as an invariant, so the probe reports it
//! instead.
//!
//! The two middle words were carried as `unknown_8`/`unknown_12` until the
//! community runtime `darktide-simple-assets-runtime` named them: its
//! `compileMouseCursor` writes exactly `{width, height, hotspotX, hotspotY,
//! byteCount}` over a PNG decoded BGRA **bottom row first**. The shipped 9/9 on a
//! 40x40 cursor is a plausible arrow tip, so the names are no longer a guess.
//!
//! No community tool writes this type, so the cooked-resource envelope is not
//! accepted: a shipped variant is the bare body.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type id, for callers that need to recognise the payload.
pub const MOUSE_CURSOR_TYPE: u64 = 0xb277_b11f_e4a6_1d37;

/// The header's length; the pixels follow immediately.
const HEADER: usize = 20;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// A cursor bitmap.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouseCursor {
    /// The bitmap's width in pixels.
    pub width: u32,
    /// The bitmap's height in pixels.
    pub height: u32,
    /// The hotspot's x, in pixels from the left.
    pub hotspot_x: u32,
    /// The hotspot's y, in pixels from the top.
    pub hotspot_y: u32,
    /// The pixels, four bytes each.
    pub pixels: Vec<u8>,
}

impl MouseCursor {
    /// Reads a cursor from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER {
            bail!("a mouse_cursor is shorter than its {HEADER}-byte header");
        }
        let len = u32_at(bytes, 16) as usize;
        // The declared length is the whole pixel field, so the resource's size
        // follows from it - the invariant the shipped cursors satisfy.
        if HEADER + len != bytes.len() {
            bail!(
                "a mouse_cursor declares {len} pixel bytes but is {} bytes",
                bytes.len()
            );
        }
        Ok(Self {
            width: u32_at(bytes, 0),
            height: u32_at(bytes, 4),
            hotspot_x: u32_at(bytes, 8),
            hotspot_y: u32_at(bytes, 12),
            pixels: bytes[HEADER..].to_vec(),
        })
    }

    /// Writes the cursor back, deriving the byte count from the pixels.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(HEADER + self.pixels.len());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.hotspot_x.to_le_bytes());
        out.extend_from_slice(&self.hotspot_y.to_le_bytes());
        out.extend_from_slice(&(self.pixels.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.pixels);
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

/// Decompiles a `mouse_cursor` variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a mouse_cursor resource needs a variant");
    };
    let cursor = MouseCursor::from_binary(variant.data())?;
    Ok(vec![UserFile::new(cursor.to_sjson()?.into_bytes())])
}

/// A cursor **source**, which is what a project writes. Either the pixels
/// themselves - with the width, the height and the hotspot written out, which is
/// the shape `snoopymod` uses - or a reference to a **texture**, whose image
/// supplies the pixels and the size. The size belongs to the image either way,
/// so a texture-backed cursor does not repeat it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouseCursorSource {
    /// The texture resource whose image is the cursor. Both the pixels and the
    /// size come from it, so neither is written out here.
    pub texture: String,
    /// The bitmap's width, when the source wants to *assert* it - it must match
    /// the image. Omit it and the image decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// The bitmap's height, on the same terms as `width`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The hotspot's x, in pixels. Zero when the source does not say.
    #[serde(default)]
    pub hotspot_x: u32,
    /// The hotspot's y, in pixels. Zero when the source does not say.
    #[serde(default)]
    pub hotspot_y: u32,
}

impl MouseCursorSource {
    /// Reads a source from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }

    /// The cursor this source describes, from the decoded texture it names.
    ///
    /// The pixels are written **BGRA with the bottom row first**, which is what
    /// the engine's own cursors carry and what the community runtime's
    /// `compileMouseCursor` writes from a decoded image. `width` and `height`
    /// are assertions when the source states them; the image is the truth.
    pub fn compile(&self, name: IdString64, image: &crate::png::Image) -> Result<BundleFile> {
        if let Some(width) = self.width
            && width != image.width
        {
            bail!(
                "a cursor asserts a width of {width}, but the texture '{}' is {} wide",
                self.texture,
                image.width
            );
        }
        if let Some(height) = self.height
            && height != image.height
        {
            bail!(
                "a cursor asserts a height of {height}, but the texture '{}' is {} high",
                self.texture,
                image.height
            );
        }

        let stride = image.width as usize * 4;
        let mut pixels = Vec::with_capacity(image.rgba.len());
        for row in (0..image.height as usize).rev() {
            for pixel in image.rgba[row * stride..row * stride + stride].chunks_exact(4) {
                pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }

        let cursor = MouseCursor {
            width: image.width,
            height: image.height,
            hotspot_x: self.hotspot_x,
            hotspot_y: self.hotspot_y,
            pixels,
        };
        let mut variant = BundleFileVariant::new();
        variant.set_data(cursor.to_binary()?);
        let mut file = BundleFile::new(name, BundleFileType::MouseCursor);
        file.add_variant(variant);
        Ok(file)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// A cursor shaped like the shipped one: 40 by 40, 9 and 9, white-opaque pixels.
    fn shipped_shaped() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(6420);
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&(40u32 * 40 * 4).to_le_bytes());
        for _ in 0..40 * 40 {
            bytes.extend_from_slice(&[0xff, 0xff, 0xff, 0x00]);
        }
        bytes
    }

    #[test]
    fn a_shipped_shaped_cursor_round_trips() {
        let bytes = shipped_shaped();
        assert_eq!(bytes.len(), 6420, "the shipped size");
        let cursor = MouseCursor::from_binary(&bytes).expect("the sample parses");
        assert_eq!(cursor.width, 40);
        assert_eq!(cursor.height, 40);
        assert_eq!(cursor.hotspot_x, 9);
        assert_eq!(cursor.hotspot_y, 9);
        assert_eq!(cursor.pixels.len(), 6400);
        assert_eq!(cursor.to_binary().unwrap(), bytes, "byte-for-byte");
        // And through the SJSON a build uses.
        let sjson = cursor.to_sjson().unwrap();
        assert_eq!(MouseCursor::from_sjson(&sjson).unwrap(), cursor);
    }

    #[test]
    fn a_byte_count_that_lies_is_rejected() {
        let mut bytes = shipped_shaped();
        bytes[16] ^= 0xFF;
        assert!(MouseCursor::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_truncated_header_is_rejected() {
        assert!(MouseCursor::from_binary(&[40, 0, 0, 0]).is_err());
    }

    /// The cooked bytes are the engine's layout: the 20-byte header, then the
    /// pixels BGRA with the bottom row first. The image's rows differ, so a
    /// wrong row order fails; the channels are checked by value.
    #[test]
    fn a_texture_backed_cursor_writes_bgra_bottom_row_first() {
        let image = crate::png::Image {
            width: 2,
            height: 2,
            rgba: vec![
                1, 2, 3, 4, 5, 6, 7, 8, // the top row
                9, 10, 11, 12, 13, 14, 15, 16, // the bottom row
            ],
        };
        let source = MouseCursorSource {
            texture: "a/texture".to_string(),
            width: None,
            height: None,
            hotspot_x: 1,
            hotspot_y: 2,
        };

        let file = source
            .compile(IdString64::from("cursors/test".to_string()), &image)
            .expect("the image compiles");

        let mut wanted = Vec::new();
        for word in [2u32, 2, 1, 2, 16] {
            wanted.extend_from_slice(&word.to_le_bytes());
        }
        // The bottom row first, each pixel B, G, R, A.
        for pixel in [
            [11u8, 10, 9, 12],
            [15, 14, 13, 16],
            [3, 2, 1, 4],
            [7, 6, 5, 8],
        ] {
            wanted.extend_from_slice(&pixel);
        }
        assert_eq!(file.variants()[0].data(), wanted.as_slice());
    }

    /// A source that asserts a size it does not have is refused, rather than
    /// quietly cooked at the image's size.
    #[test]
    fn a_size_that_does_not_match_the_texture_is_refused() {
        let image = crate::png::Image {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        let source = MouseCursorSource {
            texture: "a/texture".to_string(),
            width: Some(32),
            height: Some(32),
            hotspot_x: 0,
            hotspot_y: 0,
        };
        assert!(
            source
                .compile(IdString64::from("cursors/test".to_string()), &image)
                .is_err()
        );
    }

    /// The pixels come only from a texture, so a source has no field for them.
    #[test]
    fn a_source_that_writes_out_pixels_is_refused() {
        let text = "texture = \"a/texture\"\npixels = [\n  1\n  2\n]\n";
        assert!(MouseCursorSource::from_sjson(text).is_err());
    }
}
