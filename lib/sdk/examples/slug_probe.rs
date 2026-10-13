//! Dump a `.slug` font: what the reader makes of it, then raw records at an
//! optional address (for reverse engineering a layout the reader did not
//! recognise).
//!
//! Usage: `slug_probe <file.slug> [glyphs] [raw-glyph-table-base]`

use sdk::filetype::slug::SlugFont;

fn u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn f32(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn tag(b: &[u8], at: usize) -> String {
    // The 4CC is stored little-endian, so "COMP" reads back as "PMOC".
    b[at..at + 4].iter().rev().map(|c| *c as char).collect()
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path = std::env::args().nth(1).expect("usage: slug_probe <file.slug> [glyphs] [base]");
    let show: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let raw_base: Option<usize> = std::env::args().nth(3).and_then(|s| s.parse().ok());
    let b = std::fs::read(&path)?;

    println!(
        "{path}: {} bytes, magic {:?}, version {:#06x}, kind {:?}",
        b.len(),
        String::from_utf8_lossy(&b[0..4]),
        u32(&b, 4),
        String::from_utf8_lossy(&b[8..12])
    );

    let mut u16_count = 0usize;
    if b.len() >= 32 + 4 * 12 {
        for i in 0..4 {
            let at = 32 + i * 12;
            let (name, base, count) = (tag(&b, at), u32(&b, at + 4), u32(&b, at + 8));
            if name == "16U2" {
                u16_count = count as usize;
            }
            println!("  {name:5} base {base:>9}  count {count}");
        }
    }

    // What the reader itself makes of the file: the counts it reports and where
    // it decided the glyph table is (or that it could not find one).
    let font = SlugFont::parse(&b)?;
    println!(
        "reader: version {:#06x}, kind {:?}, kerning {}, glyphs {}, markers {}, glyph table {:?}",
        font.version,
        font.kind,
        font.kerning.len(),
        font.glyphs.len(),
        font.markers.len(),
        font.glyph_table,
    );
    for glyph in font.glyphs.iter().take(show) {
        println!(
            "  glyph {:>4}  codepoint {:>6} ({:?})  box {:>10.4} {:>10.4} {:>10.4} {:>10.4}  advance {:>9.4} {:>9.4}",
            glyph.index,
            glyph.codepoint,
            char::from_u32(glyph.codepoint),
            glyph.glyph_box[0],
            glyph.glyph_box[1],
            glyph.glyph_box[2],
            glyph.glyph_box[3],
            glyph.advance[0],
            glyph.advance[1],
        );
    }

    // Raw records at an explicit base, for a file whose table the reader did not
    // recognise (`16U2`'s count is the record count, so that many are walked).
    let Some(base) = raw_base else {
        return Ok(());
    };
    let count = u16_count.max(font.glyphs.len() + font.markers.len());
    println!("raw records at {base} ({count}), first {show}:");
    for g in 0..count.min(show) {
        let at = base + g * 64;
        if at + 64 > b.len() {
            break;
        }
        println!(
            "  {g:>4}  box {:>10.4} {:>10.4} {:>10.4} {:>10.4}  u32 {:>6} {:>10}  f32 {:>10.4} {:>10.4}  u32 {:>10} {:>6}",
            f32(&b, at),
            f32(&b, at + 4),
            f32(&b, at + 8),
            f32(&b, at + 12),
            u32(&b, at + 16),
            u32(&b, at + 20),
            f32(&b, at + 24),
            f32(&b, at + 28),
            u32(&b, at + 32),
            u32(&b, at + 36),
        );
    }
    Ok(())
}
