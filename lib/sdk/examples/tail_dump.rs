//! Segments a program tail's rest into the counted resource lists and prints
//! each record, then dumps the head of the trailing block.
//!
//! The framing read off the five tails: eight counted lists (the first two
//! never carry records in any sample) with record sizes 7, 8, 7, 4, 3 and 3
//! words, then the block.
//!
//! ```text
//! tail_dump <material data file | engine_data text> [Vertex|Pixel] [program index]
//! ```

use std::fs;

use sdk::filetype::shader::{Stage, Tail};
use sdk::filetype::shader_engine_data::EngineData;

/// Names found for hashes that appear in tails. The question marks are roles
/// read off the decompiled shaders, not names.
fn name(word: u32) -> &'static str {
    match word {
        0x516D_5CCD => "global_viewport",
        0xB563_9618 => "c_per_object",
        0x977B_01D3 => "c_material_exports",
        0xB3A2_EB88 => "?cbuffer-b1",
        0x3AFC_636C => "global_texture2D",
        0xDA56_0F03 => "global_samplers",
        0x9B80_38E0 => "linear_depth",
        0x41B1_CFF8 => "?uav-array",
        0x4B42_C5E6 => "?sampler-31",
        0xDC05_48BC => "?position",
        0x39A5_6531 => "?tex",
        0xD1D6_7F3B => "?tex",
        0x20BC_BF88 => "?channel",
        0x0B30_BAF2 => "?channel",
        0x2FAD_CC8C => "?channel",
        0x96B9_600E => "CUSTOM",
        0xF01E_3E37 => "CUSTOM1",
        0x1B58_E975 => "CUSTOM2",
        0x3FFE_ABD6 => "POSITION",
        0xFCDC_BA12 => "COLOR",
        0xB77A_0F36 => "TEXCOORD",
        0x403C_D3F2 => "SV_Target",
        0xBA91_C317 => "SV_Position",
        _ => "",
    }
}

/// Record sizes in words for the nine lists. The first two never carry
/// records in any sample, so their record size is unknown.
const SIZES: [usize; 9] = [0, 0, 7, 7, 7, 7, 4, 3, 3];

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bytes = fs::read(&args[0])?;
    let engine = match String::from_utf8(bytes.clone()) {
        Ok(text) if EngineData::looks_like_text(&text) => EngineData::from_text(&text)?,
        _ => EngineData::from_material(&bytes)?,
    };
    let want = match args.get(1).map(String::as_str) {
        Some("Vertex") => Stage::Vertex,
        _ => Stage::Pixel,
    };
    let only: Option<usize> = args.get(2).and_then(|arg| arg.parse().ok());
    let check = args.get(3).map(String::as_str) == Some("check");
    let found: Vec<usize> = engine
        .programs
        .iter()
        .enumerate()
        .filter(|(index, (stage, _))| {
            *stage == want && (check || only.map_or(true, |only| *index == only))
        })
        .map(|(index, _)| index)
        .collect();

    for index in found {
        let (_, bytes) = &engine.programs[index];
        let Some(tail) = Tail::parse(bytes) else {
            println!("program {index}: the tail does not parse");
            continue;
        };
        println!("=== program {index}, tail {} bytes", bytes.len());
        println!("--- cbuffers ({}):", tail.cbuffers.len());
        for (slot, entry) in tail.cbuffers.iter().enumerate() {
            let words: Vec<String> = entry
                .words
                .iter()
                .map(|word| format!("{word:08X}"))
                .collect();
            println!(
                "  [{slot}] {}  {:<18} size {}",
                words.join(" "),
                name(entry.name_hash()),
                entry.size()
            );
        }
        let words: Vec<u32> = tail
            .rest
            .chunks(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])))
            .collect();
        if check {
            let mut at = 0usize;
            let mut counts: Vec<u32> = Vec::new();
            let mut failed = false;
            for size in SIZES.iter() {
                let Some(count) = words.get(at).copied() else {
                    failed = true;
                    break;
                };
                at += 1;
                counts.push(count);
                if *size == 0 {
                    if count != 0 {
                        failed = true;
                        break;
                    }
                    continue;
                }
                let needed = count as usize * size;
                if words.len() < at + needed {
                    failed = true;
                    break;
                }
                at += needed;
            }
            let block = &words[at.min(words.len())..];
            let first = block.first().copied().unwrap_or(0);
            let last = block.last().copied().unwrap_or(0);
            println!(
                "program {index:>3} {:?}: lists {counts:?} block {:>3} words first {first:08X} last {last:08X}{}",
                engine.programs[index].0,
                block.len(),
                if failed { "  FAILED" } else { "" }
            );
            continue;
        }
        if args.get(3).map(String::as_str) == Some("raw") {
            println!("--- rest, raw ({} words):", words.len());
            for (at, word) in words.iter().enumerate() {
                let label = name(*word);
                if label.is_empty() {
                    println!("  {at:>4}: {word:08X}");
                } else {
                    println!("  {at:>4}: {word:08X}  {label}");
                }
            }
            continue;
        }
        let mut at = 0usize;
        for (list, size) in SIZES.iter().enumerate() {
            let Some(count) = words.get(at).copied() else {
                println!("--- list {list}: ran out of words");
                break;
            };
            at += 1;
            if *size == 0 {
                println!("--- list {list}: count {count} (unknown record size)");
                continue;
            }
            println!("--- list {list}: count {count}, {size}-word records");
            for record in 0..count as usize {
                let Some(slice) = words.get(at..at + size) else {
                    println!("    record {record}: out of words");
                    break;
                };
                at += size;
                let first = name(slice[0]);
                let fields: Vec<String> = slice[1..].iter().map(|word| format!("{word}")).collect();
                println!("    {:<18} {}", first, fields.join(" "));
            }
        }
        let block = &words[at..];
        println!("--- block: {} words, head:", block.len());
        for (offset, word) in block.iter().take(48).enumerate() {
            let label = name(*word);
            if label.is_empty() {
                println!("  {offset:>4}: {word:08X}");
            } else {
                println!("  {offset:>4}: {word:08X}  {label}");
            }
        }
    }

    Ok(())
}
