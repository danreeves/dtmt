//! Mines a binary's printable strings for the names behind a set of murmur32
//! hashes - the engine's own resource and variable names, which the game's
//! shader sources would name but only the compiled shaders still carry.
//!
//! Every substring that starts at a letter or `_` is hashed, so names sitting
//! next to printable bitstream bytes are still found.
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
    let mut report = |text: &[u8]| {
        let hash = u32::from(Murmur32::hash(text));
        if let Some((_, target)) = targets.iter().find(|(target, _)| *target == hash) {
            println!("{hash:08X} = {} (for {target})", String::from_utf8_lossy(text));
            found += 1;
        }
    };
    for index in 0..=bytes.len() {
        let printable = index < bytes.len() && (0x20..0x7F).contains(&bytes[index]);
        match (printable, start) {
            (true, None) => start = Some(index),
            (false, Some(at)) => {
                let run = &bytes[at..index];
                for offset in 0..run.len() {
                    let first = run[offset];
                    if !(first.is_ascii_alphabetic() || first == b'_') {
                        continue;
                    }
                    let end = (offset + 48).min(run.len());
                    for end in (offset + 4)..=end {
                        report(&run[offset..end]);
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
