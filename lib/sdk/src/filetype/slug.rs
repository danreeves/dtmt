//! The `slug` font resource: a font compiled by the commercial Slug library
//! (<https://sluglibrary.com/>), which is how Darktide ships fonts - as `Slug`
//! bundle entries (`a27b4d04a9ba6f9e`) inside `packages/ui/fonts/slug_*`.
//!
//! Reading only, for now. Layout, established against `slugfont.exe`'s output,
//! the demo fonts and a shipped entry:
//!
//! - `guls`, a version (`0x0700` shipped, `0x0760` in the current toolchain),
//!   a four-character kind (`fnt` / `alb`) and 16 zero bytes;
//! - four 12-byte records at `0x20`, `{4CC tag, u32 base, u32 count}` - `HLF4`
//!   (kerning pairs) and `16U2` (glyphs);
//! - the tables lay out consecutively from the record's base: `HLF4`'s 16-byte
//!   `{u32, f32, u32, f32}` pairs, then the glyph table;
//! - a glyph is a 64-byte record: a box (four `f32`), a `u32`, a packed pair,
//!   two advances (`f32`) and further words;
//!
//! The glyph table's base differs between the two versions (the record base plus
//! this file's `HLF4` table for `0x0760`; a strided scan finds it for `0x0700`).

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::UserFile;
use crate::{BundleFileVariant, Context};

const RECORDS_AT: usize = 0x20;
const RECORD_SIZE: usize = 12;
const RECORD_COUNT: usize = 4;
const GLYPH_SIZE: usize = 64;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// One glyph, as stored: a box, the codepoint and the words that follow.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Glyph {
    pub index: u32,
    /// The codepoint this glyph draws (`word1` of the record).
    pub codepoint: u32,
    pub glyph_box: [f32; 4],
    pub word2: u32,
    pub advance: [f32; 2],
    pub word3: u32,
    pub word4: u32,
}

/// A marker record, interleaved with the glyphs: a full-height box and a word
/// that reads as a float.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Marker {
    pub index: u32,
    pub glyph_box: [f32; 4],
    pub word: u32,
}

/// A parsed `slug` font.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SlugFont {
    pub version: u32,
    pub kind: String,
    /// `HLF4`: the kerning pairs, `{u32, f32, u32, f32}` each.
    pub kerning: Vec<[f32; 4]>,
    /// `16U2`: the glyphs, each with its codepoint. The table interleaves marker
    /// records, which are collected separately.
    pub glyphs: Vec<Glyph>,
    pub markers: Vec<Marker>,
    /// The address the glyph table was located at. Absent when the file's layout
    /// was not recognised, in which case no glyphs are reported.
    pub glyph_table: Option<u32>,
}

impl SlugFont {
    pub fn parse(b: &[u8]) -> Result<Self> {
        if b.len() < RECORDS_AT + RECORD_COUNT * RECORD_SIZE || &b[0..4] != b"guls" {
            bail!("not a slug font");
        }
        let version = u32_at(b, 4);
        let kind = String::from_utf8_lossy(&b[8..12]).trim_end_matches('\0').to_string();

        let records: Vec<(String, usize, usize)> = (0..RECORD_COUNT)
            .map(|i| {
                let at = RECORDS_AT + i * RECORD_SIZE;
                // The 4CC is stored little-endian, so "COMP" reads back as "PMOC".
                let tag: String = b[at..at + 4].iter().rev().map(|c| *c as char).collect();
                (tag, u32_at(b, at + 4) as usize, u32_at(b, at + 8) as usize)
            })
            .collect();

        let region = records
            .first()
            .map(|(_, base, _)| *base)
            .unwrap_or(RECORDS_AT);

        // `HLF4`: 16-byte pairs from the region base.
        let mut kerning = Vec::new();
        if let Some((_, base, count)) = records.iter().find(|(tag, _, _)| tag == "HLF4") {
            for i in 0..*count {
                let at = base + i * 16;
                if at + 16 > b.len() {
                    break;
                }
                kerning.push([
                    f32_at(b, at),
                    f32_at(b, at + 4),
                    f32_at(b, at + 8),
                    f32_at(b, at + 12),
                ]);
            }
        }

        // `16U2`: 64-byte glyph records. Their base is `region + the HLF4 table`
        // for `0x0760`; for `0x0700` it is found by scanning for the first
        // 64-byte-aligned run of plausible boxes.
        let glyph_count = records
            .iter()
            .find(|(tag, _, _)| tag == "16U2")
            .map(|(_, _, count)| *count)
            .unwrap_or(0);
        let hlf4_bytes = kerning.len() * 16;
        let default_base = region + hlf4_bytes;
        let looks_like_glyph = |at: usize| -> bool {
            if at + GLYPH_SIZE > b.len() {
                return false;
            }
            let x0 = f32_at(b, at);
            let y1 = f32_at(b, at + 12);
            x0.is_finite() && (-2.0..=2.0).contains(&x0) && y1 > 0.05 && y1 <= 2.0
        };
        let mut glyph_table = None;
        if (0..4).all(|k| looks_like_glyph(default_base + k * GLYPH_SIZE)) {
            glyph_table = Some(default_base as u32);
        } else {
            let mut at = region;
            while at + 5 * GLYPH_SIZE <= b.len() {
                if (0..4).all(|k| looks_like_glyph(at + k * GLYPH_SIZE)) {
                    glyph_table = Some(at as u32);
                    break;
                }
                at += GLYPH_SIZE;
            }
        }
        // When neither the default base nor the scan fits, the table is left
        // unlocated and its records are not reported as glyphs.
        let glyph_base = glyph_table.map(|at| at as usize).unwrap_or(default_base);
        // Unlocated table: report no records rather than guess at them.
        let glyph_count = if glyph_table.is_some() { glyph_count } else { 0 };

        let mut glyphs = Vec::new();
        let mut markers = Vec::new();
        for index in 0..glyph_count {
            let at = glyph_base + index * GLYPH_SIZE;
            if at + GLYPH_SIZE > b.len() {
                break;
            }
            let glyph_box = [
                f32_at(b, at),
                f32_at(b, at + 4),
                f32_at(b, at + 8),
                f32_at(b, at + 12),
            ];
            let word1 = u32_at(b, at + 16);
            let word2 = u32_at(b, at + 20);
            // A marker carries a full-height box and leaves `word2` at zero; a
            // glyph has a non-zero `word2` *and* a plausible box. Anything else
            // (e.g. a table of ascending ids) is neither, so it is left out
            // rather than mis-reported as a glyph.
            if word2 == 0 {
                markers.push(Marker {
                    index: index as u32,
                    glyph_box,
                    word: word1,
                });
                continue;
            }
            // A glyph's box must be a *real* box, not the denormal floats an
            // ascending-id table reads as: 386, 387 … are `5.4e-43`-ish, so a
            // width or height of at least half a percent is what separates the
            // two. (`word2 != 0` alone is not enough - an id run passes it.)
            let width = glyph_box[2] - glyph_box[0];
            let height = glyph_box[3] - glyph_box[1];
            let plausible = width >= 0.005 || height >= 0.005;
            if !plausible {
                continue;
            }
            glyphs.push(Glyph {
                index: index as u32,
                codepoint: word1,
                glyph_box,
                word2,
                advance: [f32_at(b, at + 24), f32_at(b, at + 28)],
                word3: u32_at(b, at + 32),
                word4: u32_at(b, at + 36),
            });
        }

        Ok(Self {
            version,
            kind,
            kerning,
            glyphs,
            markers,
            glyph_table,
        })
    }

    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }
}

/// Decompiles a `slug` variant to its SJSON inventory.
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a slug resource needs a variant");
    };
    let font = SlugFont::parse(variant.data())?;
    Ok(vec![UserFile::new(font.to_sjson()?.into_bytes())])
}

#[cfg(test)]
mod test {
    use super::*;

    /// A minimal font: `guls`, version, `fnt`, the four records, then one
    /// `HLF4` pair and two glyphs at `region + 16`.
    fn sample() -> Vec<u8> {
        let mut b = vec![0u8; 0x1000];
        b[0..4].copy_from_slice(b"guls");
        b[4..8].copy_from_slice(&0x760u32.to_le_bytes());
        b[8..12].copy_from_slice(b"fnt\0");
        for (i, (tag, base, count)) in [
            ("HLF4", 0x1000usize, 1usize),
            ("PMOC", 0x2000, 0),
            ("16U2", 0x1000, 2),
            ("PMOC", 0x3000, 0),
        ]
        .iter()
        .enumerate()
        {
            let at = RECORDS_AT + i * RECORD_SIZE;
            // The tag is stored little-endian, so a reader reverses it back.
        let stored: Vec<u8> = tag.bytes().rev().collect();
            b[at..at + 4].copy_from_slice(&stored);
            b[at + 4..at + 8].copy_from_slice(&(*base as u32).to_le_bytes());
            b[at + 8..at + 12].copy_from_slice(&(*count as u32).to_le_bytes());
        }
        // Make the table area large enough for 1 kerning pair + 4 records (a
        // located table needs four plausible records in a row).
        b.resize(0x1000 + 16 + 4 * GLYPH_SIZE, 0);
        // The kerning pair.
        for (i, v) in [1.0f32, 0.5, 2.0, 0.25].iter().enumerate() {
            b[0x1000 + i * 4..0x1000 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        // Glyph 0: a plausible box, then a word and advances.
        let g0 = 0x1000 + 16;
        for (i, v) in [0.1f32, -0.01, 0.5, 0.7].iter().enumerate() {
            b[g0 + i * 4..g0 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[g0 + 16..g0 + 20].copy_from_slice(&2555u32.to_le_bytes());
        // A non-zero `word2` marks a glyph (zero would make it a marker).
        b[g0 + 20..g0 + 24].copy_from_slice(&1835022u32.to_le_bytes());
        for (i, v) in [35.0f32, 42.5].iter().enumerate() {
            b[g0 + 24 + i * 4..g0 + 24 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        let g1 = g0 + GLYPH_SIZE;
        for (i, v) in [0.0f32, 0.0, 0.55, 1.0].iter().enumerate() {
            b[g1 + i * 4..g1 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        // Two more marker records, so the table is locatable.
        for (k, w) in [(2usize, 0.6f32), (3, 0.65)] {
            let at = g0 + k * GLYPH_SIZE;
            for (i, v) in [0.0f32, 0.0, w, 1.0].iter().enumerate() {
                b[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_slug_font_reads() {
        let font = SlugFont::parse(&sample()).expect("the sample parses");
        assert_eq!(font.version, 0x760);
        assert_eq!(font.kind, "fnt");
        assert_eq!(font.kerning.len(), 1);
        assert_eq!(font.glyphs.len(), 1);
        assert_eq!(font.markers.len(), 1);
        assert_eq!(font.glyph_table, Some(0x1000 + 16));
        assert_eq!(font.glyphs[0].codepoint, 2555);
        assert_eq!(font.glyphs[0].advance, [35.0, 42.5]);
        assert_eq!(font.markers[0].glyph_box[3], 1.0);
    }

    #[test]
    fn a_non_slug_blob_is_rejected() {
        assert!(SlugFont::parse(b"not a slug font at all").is_err());
    }
}
