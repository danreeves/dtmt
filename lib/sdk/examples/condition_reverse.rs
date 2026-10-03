//! Hashes candidate condition names with the engine's Murmur32 and prints the
//! ones matching a set of target hashes read from a section's conditions blob.
//!
//! A `shader43` section's conditions tree tests condition names by their
//! Murmur32 hash (see `condition_tree.rs`), so a section's blob names its
//! vocabulary only through those hashes. This tool turns a name list back into
//! that vocabulary.
//!
//! ```text
//! condition_reverse <targets.txt> <names...>
//! ```
//!
//! `targets.txt` holds one 8-digit hex hash per line. `names` are files with one
//! candidate name per line, or literal names. Every match is printed as
//! `hash name`.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use sdk::murmur::Murmur32;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let targets_path = args.next().ok_or("usage: condition_reverse <targets> <names...>")?;
    let mut targets: BTreeSet<u32> = BTreeSet::new();
    for line in fs::read_to_string(&targets_path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        targets.insert(u32::from_str_radix(line, 16)?);
    }

    let mut names: Vec<String> = Vec::new();
    for arg in args {
        if let Ok(text) = fs::read_to_string(&arg) {
            names.extend(text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
        } else {
            names.push(arg);
        }
    }
    names.sort();
    names.dedup();

    let mut hits: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for name in &names {
        let hash = u32::from(Murmur32::hash(name.as_bytes()));
        if targets.contains(&hash) {
            hits.entry(hash).or_default().push(name.clone());
        }
    }

    println!("{} names, {} targets", names.len(), targets.len());
    let mut found = 0;
    for (hash, names) in &hits {
        println!("  {hash:08X} {}", names.join(", "));
        found += 1;
    }
    println!("{found} targets named, {} still unknown", targets.len() - found);
    for hash in &targets {
        if !hits.contains_key(hash) {
            println!("  {hash:08X} ?");
        }
    }
    Ok(())
}
