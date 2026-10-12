//! Dumps a pixel program's device block split into its head, its run of the
//! preamble body's records, and the trailing bytes after them, as `u32` words -
//! to see whether the trailing follows a record grammar (and is derivable) or is
//! per-shader data. It also reports how much of the preamble the block carries.
//!
//! ```text
//! trailing_probe <engine_data>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Stage};
use sdk::filetype::shader_engine_data::EngineData;

fn words(bytes: &[u8]) -> String {
    bytes
        .chunks_exact(4)
        .map(|word| format!("{:08X}", u32::from_le_bytes(word.try_into().unwrap())))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The longest run of `needle` that appears verbatim in `haystack`.
fn longest_common(haystack: &[u8], needle: &[u8]) -> usize {
    let mut best = 0;
    for start in 0..needle.len() {
        for at in 0..haystack.len() {
            let mut run = 0;
            while start + run < needle.len()
                && at + run < haystack.len()
                && needle[start + run] == haystack[at + run]
            {
                run += 1;
            }
            best = best.max(run);
        }
    }
    best
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: trailing_probe <engine_data>")?;
    let engine_data = EngineData::from_text(&fs::read_to_string(&path)?)?;

    let preamble = &engine_data.device_preamble;
    let body = preamble.get(16..).unwrap_or_default();
    let count = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let records: Vec<&[u8]> = body[4..4 + count * 12].chunks_exact(12).collect();
    println!("preamble {} B; {} records", preamble.len(), records.len());

    let mut reported = false;
    for (index, (stage, tail)) in engine_data.programs.iter().enumerate() {
        if *stage != Stage::Pixel {
            continue;
        }
        let Some(parsed) = shader::Tail::parse(tail) else {
            continue;
        };
        let Some(lists) = shader::TailLists::parse(&parsed.rest) else {
            continue;
        };
        let block = &lists.block;
        // The head runs to where the first record starts; then the records, then
        // the trailing.
        let at = records
            .iter()
            .filter_map(|record| block.windows(12).position(|window| window == *record))
            .min()
            .unwrap_or(0);
        // The end of the last whole record, keeping record order.
        let mut end = at;
        for record in &records {
            if let Some(offset) = block[end..].windows(12).position(|window| window == *record) {
                end += offset + 12;
            }
        }
        let head = &block[..at];
        let trailing = &block[end..];
        println!(
            "program {index:2}: block {} B; head {}; records {}..{}; trailing {} B",
            block.len(),
            words(head),
            at,
            end,
            trailing.len()
        );
        println!("   trailing: {}", words(trailing));

        if !reported {
            reported = true;
            println!(
                "   << longest run of the block in the preamble: {} B; of the preamble in the \
                 block: {} B; whole block in preamble: {} >>",
                longest_common(preamble, block),
                longest_common(block, preamble),
                preamble.windows(block.len()).any(|w| w == block)
            );
        }
    }
    Ok(())
}
