//! Prints the first pixel program's metadata tail of each engine-data file and
//! marks the words that are the same across all of them, so the invariant
//! engine part can be told apart from the shader's.
//!
//! ```text
//! align_tails <engine_data.txt>...
//! ```

use std::path::Path;

use sdk::filetype::shader::{Stage, Tail};
use sdk::filetype::shader_engine_data::EngineData;

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])))
        .collect()
}

fn main() -> color_eyre::Result<()> {
    let mut families = Vec::new();
    for arg in std::env::args().skip(1) {
        let data = EngineData::from_text(&std::fs::read_to_string(&arg)?)?;
        let Some((_, tail)) = data
            .programs
            .iter()
            .find(|(stage, _)| *stage == Stage::Pixel)
        else {
            println!("{}: no pixel program", arg);
            continue;
        };
        let Some(tail) = Tail::parse(tail) else {
            println!("{}: the tail does not parse", arg);
            continue;
        };
        let name = Path::new(&arg)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| arg.clone());
        families.push((name, tail));
    }

    println!("=== cbuffer lists");
    for (name, tail) in &families {
        let list: Vec<String> = tail
            .cbuffers
            .iter()
            .map(|entry| format!("{:08X}/{}", entry.name_hash(), entry.size()))
            .collect();
        println!("  {name:<18} {}", list.join("  "));
    }

    let rests: Vec<Vec<u32>> = families
        .iter()
        .map(|(_, tail)| words(&tail.rest))
        .collect();
    let longest = rests.iter().map(Vec::len).max().unwrap_or(0);
    println!("=== rest lengths: {longest} words at most");

    for (label, range) in [
        ("front", 0..32usize),
        ("back", longest.saturating_sub(32)..longest),
    ] {
        println!("=== {label}");
        let mut index = range.start;
        while index < range.end {
            let mut line = format!("  {index:>4}:");
            let mut agreed = true;
            for rest in &rests {
                let word = rest.get(index).copied().unwrap_or(0);
                line.push_str(&format!(" {word:08X}"));
                if rest.len() <= index {
                    agreed = false;
                }
            }
            // The words agree when every family has one and they are equal.
            let first = rests.first().and_then(|rest| rest.get(index)).copied();
            agreed = agreed && rests.iter().all(|rest| rest.get(index) == first.as_ref());
            line.push_str(if agreed { "  =" } else { "  ." });
            println!("{line}");
            index += 1;
        }
    }

    // The block sits at the end of every pixel tail and its prologue starts
    // with the `linear_depth` record, so aligning on the last occurrence of
    // that hash lines the engine's part up across families.
    const LINEAR_DEPTH: u32 = 0x9B80_38E0;
    let anchors: Vec<Option<usize>> = rests
        .iter()
        .map(|rest| rest.iter().rposition(|word| *word == LINEAR_DEPTH))
        .collect();
    println!("=== block, aligned on the linear_depth record");
    for ((name, _), anchor) in families.iter().zip(&anchors) {
        match anchor {
            Some(anchor) => println!("  {name:<18} record at word {anchor}"),
            None => println!("  {name:<18} no linear_depth record"),
        }
    }
    for offset in 0..44usize {
        let mut line = format!("  {offset:>4}:");
        let mut agreed = true;
        let mut seen = false;
        for (rest, anchor) in rests.iter().zip(&anchors) {
            let Some(anchor) = anchor else {
                agreed = false;
                continue;
            };
            // One word before the record is its count word.
            let at = anchor + offset;
            match rest.get(at) {
                Some(word) => {
                    line.push_str(&format!(" {word:08X}"));
                    seen = true;
                }
                None => {
                    line.push_str(" --------");
                    agreed = false;
                }
            }
        }
        let first = rests
            .iter()
            .zip(&anchors)
            .find_map(|(rest, anchor)| anchor.and_then(|anchor| rest.get(anchor + offset)));
        agreed = agreed
            && seen
            && rests
                .iter()
                .zip(&anchors)
                .all(|(rest, anchor)| anchor.and_then(|anchor| rest.get(anchor + offset)) == first);
        line.push_str(if agreed { "  =" } else { "  ." });
        println!("{line}");
    }

    Ok(())
}
