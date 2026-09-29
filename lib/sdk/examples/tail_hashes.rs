//! Collects the distinct hash-like words from every program tail of the given
//! engine-data and material-data files, one per line, so they can be mined
//! against the compiled shaders' names with `mine_strings`.
//!
//! ```text
//! tail_hashes <file>...
//! ```

use std::collections::BTreeSet;
use std::fs;

use sdk::filetype::shader::Tail;
use sdk::filetype::shader_engine_data::EngineData;

fn main() -> color_eyre::Result<()> {
    let mut hashes: BTreeSet<u32> = BTreeSet::new();
    for arg in std::env::args().skip(1) {
        let bytes = fs::read(&arg)?;
        let engine = match String::from_utf8(bytes.clone()) {
            Ok(text) if text.contains("program ") => EngineData::from_text(&text)?,
            _ => EngineData::from_material(&bytes)?,
        };
        for (_, tail_bytes) in &engine.programs {
            let Some(tail) = Tail::parse(tail_bytes) else {
                continue;
            };
            for chunk in tail.rest.chunks(4) {
                let word = u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]));
                // Hashes are large, unstructured words; counts and flags are not.
                if word >= 0x0100_0000 {
                    hashes.insert(word);
                }
            }
        }
    }
    for hash in &hashes {
        println!("{hash:08X}");
    }
    eprintln!("{} distinct words", hashes.len());
    Ok(())
}
