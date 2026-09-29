//! Mines a binary's printable strings for the names behind a set of murmur32
//! hashes - the engine's own resource and variable names, which the game's
//! shader sources would name but only the executable still carries.
//!
//! ```text
//! mine_strings <binary> <hash>...
//! ```

use std::fs;

use sdk::murmur::Murmur32;

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, targets) = args
        .split_first()
        .expect("usage: mine_strings <binary> <hash>...");
    let targets: Vec<(u32, String)> = targets
        .iter()
        .map(|text| {
            (
                u32::from_str_radix(text.trim_start_matches("0x"), 16).expect("a hex hash"),
                text.clone(),
            )
        })
        .collect();

    let bytes = fs::read(path)?;
    let mut start: Option<usize> = None;
    let mut found = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        let printable = (0x20..0x7F).contains(byte);
        match (printable, start) {
            (true, None) => start = Some(index),
            (false, Some(at)) => {
                if index - at >= 4 {
                    let text = String::from_utf8_lossy(&bytes[at..index]);
                    let hash = u32::from(Murmur32::hash(text.as_ref()));
                    if let Some((_, target)) = targets.iter().find(|(target, _)| *target == hash) {
                        println!("{hash:08X} = {text} (for {target})");
                        found += 1;
                    }
                }
                start = None;
            }
            _ => {}
        }
    }
    println!("{found} hits over {} targets", targets.len());
    Ok(())
}
