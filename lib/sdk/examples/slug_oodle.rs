//! Try to Oodle-decompress the `COMP` chunks of a `.slug` font, to confirm the
//! body is compressed and find its raw size.
//!
//! Usage: `slug_oodle <file.slug>`

use std::io::Read as _;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path = std::env::args().nth(1).expect("usage: slug_oodle <file.slug>");
    let bytes = std::fs::read(&path)?;
    println!("{path}: {} bytes", bytes.len());

    // Chunk directory: 12-byte `{4CC tag, u32 offset, u32 size}` records at 32.
    let mut at = 32usize;
    let mut chunks = Vec::new();
    while at + 12 <= bytes.len() {
        let tag = String::from_utf8_lossy(&bytes[at..at + 4]).to_string();
        let offset = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        if !tag.chars().all(|c| c.is_ascii_graphic()) {
            break;
        }
        println!("  {tag}: offset {offset}, size {size}");
        chunks.push((tag, offset, size));
        at += 12;
    }

    for (tag, offset, size) in &chunks {
        // The 4CC is stored little-endian, so "COMP" reads back as "PMOC".
        let tag: String = tag.chars().rev().collect();
        if tag != "COMP" || *offset + *size > bytes.len() {
            continue;
        }
        let chunk = &bytes[*offset..*offset + *size];
        println!("COMP at {offset} ({size} bytes): first 8 {:#04x?}", &chunk[..8]);
        for shift in 22..=27u32 {
            let raw = 1usize << shift;
            if let Ok(out) = oodle::decompress(
                chunk.to_vec(),
                raw,
                oodle::OodleLZ_FuzzSafe::No,
                oodle::OodleLZ_CheckCRC::No,
            ) {
                println!("  raw {raw}: OK, {} bytes -> {:02x?}", out.len(), &out[..out.len().min(16)]);
                std::fs::write(format!("{path}.{offset}.raw"), &out)?;
                break;
            }
        }
    }
    let mut _drain = std::io::empty();
    let _ = _drain.read(&mut []);
    Ok(())
}