//! Find the strings behind an `IdString32` hash.
//!
//! Reads candidate strings from the given files - a dictionary's CSV column, or
//! any bytes the game shipped - and prints those whose `Murmur32` is the target
//! hash. Useful when a decompiled file names a channel (or a variable, or a
//! context) by hash only and the dictionary does not know it.
//!
//! Usage: `hash32_reverse <hash> <file>...`

use std::collections::BTreeSet;
use std::fs;

use sdk::murmur::Murmur32;

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let Some(target) = args.get(1) else {
        color_eyre::eyre::bail!("usage: hash32_reverse <hash> <file>...");
    };
    let target = u32::from_str_radix(
        target.trim_start_matches('#').trim_start_matches("0x"),
        16,
    )?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut files = Vec::new();
    for path in &args[2..] {
        let path = std::path::PathBuf::from(path);
        if path.is_dir() {
            collect_files(&path, &mut files)?;
        } else {
            files.push(path);
        }
    }
    for path in &files {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if token.len() < 2 || token.len() > 64 {
                continue;
            }
            // A name is often stored with a prefix (`BINDLESS_MINLOD_bca`), so
            // try the token and every suffix after one of its underscores.
            for (at, _) in token.match_indices('_') {
                let candidate = &token[at + 1..];
                if candidate.len() < 2 || !seen.insert(candidate.to_string()) {
                    continue;
                }
                if u32::from(Murmur32::hash(candidate)) == target {
                    println!("{candidate}");
                }
            }
            if !seen.insert(token.to_string()) {
                continue;
            }
            if u32::from(Murmur32::hash(token)) == target {
                println!("{token}");
            }
        }
    }

    eprintln!("{} candidates in {} files", seen.len(), files.len());
    Ok(())
}

/// Collects every file under `dir`, skipping the directories a repository
/// keeps its build output and history in.
fn collect_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) -> color_eyre::Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if matches!(name.to_str(), Some("target" | ".git")) {
                continue;
            }
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}
