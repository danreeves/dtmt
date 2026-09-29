//! Compares a program tail's trailing block with the engine data's device
//! preamble: the block looks like an 8-word header followed by the preamble
//! minus its 3-word header.
//!
//! ```text
//! preamble_block <engine_data text> [Vertex|Pixel] [program index]
//! ```

use std::fs;

use sdk::filetype::shader::{Stage, Tail};
use sdk::filetype::shader_engine_data::EngineData;

/// Record sizes in words for the nine lists of a tail's rest.
const SIZES: [usize; 9] = [0, 0, 7, 7, 7, 7, 4, 3, 3];

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let text = fs::read_to_string(&args[0])?;
    let engine = EngineData::from_text(&text)?;
    let want = match args.get(1).map(String::as_str) {
        Some("Vertex") => Stage::Vertex,
        _ => Stage::Pixel,
    };
    let only: Option<usize> = args.get(2).and_then(|arg| arg.parse().ok());
    let preamble = &engine.device_preamble;
    println!("preamble: {} bytes", preamble.len());

    for (index, (stage, bytes)) in engine.programs.iter().enumerate() {
        if *stage != want || only.map_or(false, |only| only != index) {
            continue;
        }
        let Some(tail) = Tail::parse(bytes) else {
            continue;
        };
        let words: Vec<u32> = tail
            .rest
            .chunks(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])))
            .collect();
        let mut at = 0usize;
        for size in SIZES.iter() {
            let Some(count) = words.get(at).copied() else { break };
            at += 1;
            if *size == 0 {
                continue;
            }
            at += count as usize * size;
        }
        let block = &tail.rest[(at * 4).min(tail.rest.len())..];
        println!("program {index} {stage:?}: block {} bytes", block.len());
        let head: Vec<String> = block
            .chunks(4)
            .take(9)
            .map(|chunk| format!("{:08X}", u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]))))
            .collect();
        println!("  block head:    {}", head.join(" "));
        let phead: Vec<String> = preamble
            .chunks(4)
            .take(4)
            .map(|chunk| format!("{:08X}", u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]))))
            .collect();
        println!("  preamble head: {}", phead.join(" "));
        // Where does the preamble's body reappear in the block?
        let body = preamble.get(12..).unwrap_or(&[]);
        if !body.is_empty() && block.ends_with(body) {
            println!("  block ends with preamble[12..] ({} bytes, prefix {})", body.len(), block.len() - body.len());
        }
        for skip in 0..64usize {
            if let Some(candidate) = block.get(skip..) {
                if candidate.len() == body.len() && candidate == body {
                    println!("  block[{skip}..] == preamble[12..] ({} bytes)", body.len());
                }
            }
        }
    }
    Ok(())
}
