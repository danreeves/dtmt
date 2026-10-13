//! Dump a `.slug` font: header, chunk records and the tables they point at.
//!
//! Usage: `slug_probe <file.slug> [glyphs]`

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
    let path = std::env::args().nth(1).expect("usage: slug_probe <file.slug> [glyphs]");
    let show: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let b = std::fs::read(&path)?;

    println!(
        "{path}: {} bytes, magic {:?}, version {:#06x}, kind {:?}",
        b.len(),
        String::from_utf8_lossy(&b[0..4]),
        u32(&b, 4),
        String::from_utf8_lossy(&b[8..12])
    );

    let records: Vec<(String, usize, usize)> = (0..4)
        .map(|i| {
            let at = 32 + i * 12;
            (tag(&b, at), u32(&b, at + 4) as usize, u32(&b, at + 8) as usize)
        })
        .collect();
    for (name, base, count) in &records {
        println!("  {name:5} base {base:>9}  count {count}");
    }

    // `2U61` is the glyph table: 64-byte records from its base.
    for (name, base, count) in &records {
        if name != "16U2" {
            continue;
        }
        println!("glyphs ({count}), first {show}:");
        for g in 0..(*count).min(show) {
            // The tables are laid out from the region base: HLF4 first (16-byte
            // records), then a 32-byte header and the 64-byte glyph records.
            let at = base + 64 + g * 64;
            if at + 64 > b.len() {
                break;
            }
            let glyph_box = [f32(&b, at), f32(&b, at + 4), f32(&b, at + 8), f32(&b, at + 12)];
            println!(
                "  {g:>4}  box {:>10.4} {:>10.4} {:>10.4} {:>10.4}  u32 {:>6} {:>10}  f32 {:>10.4} {:>10.4}  u32 {:>10} {:>6}",
                glyph_box[0], glyph_box[1], glyph_box[2], glyph_box[3],
                u32(&b, at + 16), u32(&b, at + 20),
                f32(&b, at + 24), f32(&b, at + 28),
                u32(&b, at + 32), u32(&b, at + 36),
            );
        }
    }
    Ok(())
}
