//! A small PNG reader, so a project can keep **PNG** sources in its repository
//! instead of binary DDS images.
//!
//! It reads what a source image needs and nothing else: 8-bit greyscale, RGB,
//! greyscale+alpha, RGBA and palette (with `tRNS`), non-interlaced, and the five
//! row filters. Anything outside that is refused with a message naming what it
//! is, rather than decoded wrongly. The `flate2` dependency the texture reader
//! already uses does the inflation, so this costs no new crate.
//!
//! The output is tightly packed RGBA, which is what a texture encoder and the
//! cursor compiler both want.

use color_eyre::{Result, eyre::bail};
use flate2::read::ZlibDecoder;
use std::io::Read;

/// A decoded image: tightly packed RGBA, row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Whether `bytes` start with the PNG signature.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

/// Reads a PNG into tightly packed RGBA.
pub fn decode(bytes: &[u8]) -> Result<Image> {
    if !is_png(bytes) {
        bail!("not a PNG: the signature is missing");
    }

    let mut at = 8usize;
    let mut header = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut transparency: Vec<u8> = Vec::new();
    let mut data: Vec<u8> = Vec::new();

    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes
            .get(at + 8..at + 8 + length)
            .ok_or_else(|| color_eyre::eyre::eyre!("a PNG chunk runs past the end of the file"))?;
        at += 12 + length; // length, kind, body, CRC

        match kind {
            b"IHDR" => {
                if length != 13 {
                    bail!("a PNG IHDR is {length} bytes, not 13");
                }
                let width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(body[4..8].try_into().unwrap());
                let depth = body[8];
                let colour = body[9];
                let interlace = body[12];
                if depth != 8 {
                    bail!("a PNG with bit depth {depth} is not read; 8 is");
                }
                if interlace != 0 {
                    bail!("an interlaced PNG is not read");
                }
                if !matches!(colour, 0 | 2 | 3 | 4 | 6) {
                    bail!("a PNG with colour type {colour} is not read");
                }
                if width == 0 || height == 0 {
                    bail!("a PNG is {width}x{height}");
                }
                header = Some((width, height, colour));
            }
            b"PLTE" => {
                if length % 3 != 0 {
                    bail!("a PNG palette is {length} bytes, which is not a multiple of 3");
                }
                palette = body.chunks_exact(3).map(|e| [e[0], e[1], e[2]]).collect();
            }
            b"tRNS" => transparency = body.to_vec(),
            b"IDAT" => data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
    }

    let (width, height, colour) = header.ok_or_else(|| color_eyre::eyre::eyre!("a PNG has no IHDR"))?;
    if data.is_empty() {
        bail!("a PNG has no image data");
    }

    let mut raw = Vec::new();
    ZlibDecoder::new(data.as_slice())
        .read_to_end(&mut raw)
        .map_err(|e| color_eyre::eyre::eyre!("a PNG's image data is not valid zlib: {e}"))?;

    // Channels per pixel, and the stride a filtered row carries.
    let channels = match colour {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        _ => 4,
    };
    let stride = width as usize * channels;
    let expected = (stride + 1) * height as usize;
    if raw.len() < expected {
        bail!(
            "a PNG declares {width}x{height} ({} bytes filtered) but holds {}",
            expected,
            raw.len()
        );
    }

    // Undo the row filters. `bpp` is the byte distance the filters use.
    let mut rows: Vec<u8> = Vec::with_capacity(stride * height as usize);
    let mut previous = vec![0u8; stride];
    for row in 0..height as usize {
        let start = row * (stride + 1);
        let filter = raw[start];
        let mut line = raw[start + 1..start + 1 + stride].to_vec();
        for index in 0..stride {
            let left = if index >= channels { line[index - channels] } else { 0 };
            let up = previous[index];
            let up_left = if index >= channels { previous[index - channels] } else { 0 };
            let value = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((left as u16 + up as u16) / 2) as u8,
                4 => paeth(left, up, up_left),
                other => bail!("a PNG row uses filter {other}, which does not exist"),
            };
            line[index] = line[index].wrapping_add(value);
        }
        previous = line.clone();
        rows.extend_from_slice(&line);
    }

    // Widen to RGBA.
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in rows.chunks_exact(channels) {
        match colour {
            0 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            2 => rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            3 => {
                let index = pixel[0] as usize;
                let entry = palette
                    .get(index)
                    .ok_or_else(|| color_eyre::eyre::eyre!("a PNG palette index {index} is out of range"))?;
                let alpha = transparency.get(index).copied().unwrap_or(255);
                rgba.extend_from_slice(&[entry[0], entry[1], entry[2], alpha]);
            }
            4 => rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            _ => rgba.extend_from_slice(pixel),
        }
    }

    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// The PNG Paeth predictor.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    /// Writes a PNG around already-filtered rows, so a test can choose the
    /// filter. The CRCs are zero: the reader does not check them.
    fn png(width: u32, height: u32, colour: u8, filtered: &[u8]) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut chunk = |kind: &[u8; 4], body: &[u8]| {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out.extend_from_slice(&[0, 0, 0, 0]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, colour, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(filtered).unwrap();
        let data = encoder.finish().unwrap();
        chunk(b"IDAT", &data);
        chunk(b"IEND", &[]);
        out
    }

    #[test]
    fn an_unfiltered_rgba_png_decodes() {
        // Two pixels: opaque red, then half-transparent green.
        let filtered = [0u8, 255, 0, 0, 255, 0, 255, 0, 128];
        let image = decode(&png(2, 1, 6, &filtered)).expect("the sample decodes");
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, vec![255, 0, 0, 255, 0, 255, 0, 128]);
    }

    #[test]
    fn a_sub_filtered_row_decodes() {
        // Filter 1 (Sub) on three RGB pixels: each byte is the difference from
        // the byte one pixel to the left, so the row reads back as a ramp.
        let mut filtered = vec![1u8];
        let wanted = [10u8, 20, 30, 40, 50, 60, 70, 80, 90];
        let mut previous = [0u8; 3];
        for pixel in wanted.chunks_exact(3) {
            for index in 0..3 {
                filtered.push(pixel[index].wrapping_sub(previous[index]));
            }
            previous = [pixel[0], pixel[1], pixel[2]];
        }
        let image = decode(&png(3, 1, 2, &filtered)).expect("the sample decodes");
        assert_eq!(image.rgba[0..3], wanted[0..3]);
        assert_eq!(image.rgba[8..11], wanted[6..9]);
    }

    #[test]
    fn a_palette_png_decodes_with_its_transparency() {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut chunk = |kind: &[u8; 4], body: &[u8], out: &mut Vec<u8>| {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out.extend_from_slice(&[0, 0, 0, 0]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&2u32.to_be_bytes());
        ihdr.extend_from_slice(&1u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
        chunk(b"IHDR", &ihdr, &mut out);
        chunk(b"PLTE", &[255, 0, 0, 0, 0, 255], &mut out);
        chunk(b"tRNS", &[128], &mut out);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&[0u8, 0, 1]).unwrap();
        let data = encoder.finish().unwrap();
        chunk(b"IDAT", &data, &mut out);
        chunk(b"IEND", &[], &mut out);

        let image = decode(&out).expect("the sample decodes");
        assert_eq!(image.rgba, vec![255, 0, 0, 128, 0, 0, 255, 255]);
    }

    #[test]
    fn something_that_is_not_a_png_is_refused() {
        assert!(!is_png(b"not a png at all"));
        assert!(decode(b"not a png at all").is_err());
    }
}
