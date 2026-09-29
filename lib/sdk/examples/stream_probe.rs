//! Tests whether the device preamble's stream is the material's texture
//! channels: for every material section it collects the stream's record names
//! and the group data's channel records (the `{kind 5, offset 0, size 4}` ones)
//! and reports whether the two sets agree.
//!
//! ```text
//! stream_probe [--limit <n>] [--verbose] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::{self, Section};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn shader_section(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 28 {
        return None;
    }
    let version = u32_at(data, 0);
    if !(60..=62).contains(&version) {
        return None;
    }
    if u32_at(data, 4) != 28 {
        return None;
    }
    let offset = u32_at(data, 12) as usize;
    let size = u32_at(data, 16) as usize;
    if size == 0 {
        return None;
    }
    data.get(offset..offset + size)
}

fn walk(path: &Path, visit: &mut impl FnMut(&Path)) {
    if path.is_file() {
        visit(path);
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

/// The record names of the device preamble's stream.
fn stream_names(preamble: &[u8]) -> Option<Vec<u32>> {
    if preamble.len() < 16 || u32_at(preamble, 0) != 1 {
        return None;
    }
    let configs = u32_at(preamble, 12) as usize;
    let mut at = 16 + configs * 13;
    let stream = u32_at(preamble, at) as usize;
    at += 4;
    let mut names = Vec::with_capacity(stream);
    for _ in 0..stream {
        let name = u32_at(preamble, at);
        let kind = u32_at(preamble, at + 4);
        let len = if kind == 5 && name >= 0x1000 { 73 } else { 60 };
        if at + len > preamble.len() {
            return None;
        }
        names.push(name);
        at += len;
    }
    Some(names)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut limit = usize::MAX;
    let mut verbose = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                i += 1;
                limit = args.get(i).expect("--limit needs a number").parse()?;
            }
            "--verbose" => verbose = true,
            other => paths.push(PathBuf::from(other)),
        }
        i += 1;
    }
    if paths.is_empty() {
        eprintln!("usage: stream_probe [--limit <n>] [--verbose] <file or directory>...");
        std::process::exit(1);
    }

    let mut sections = 0usize;
    let mut files = 0usize;
    let mut agree = 0usize;
    let mut no_stream = 0usize;
    let mut mismatches = 0usize;
    for path in &paths {
        if sections >= limit {
            break;
        }
        walk(path, &mut |file| {
            if sections >= limit {
                return;
            }
            files += 1;
            let Ok(data) = fs::read(file) else {
                return;
            };
            let Some(shader) = shader_section(&data) else {
                return;
            };
            let Ok(section) = Section::parse(shader) else {
                return;
            };
            let device = section.device_data();
            let Ok(programs) = shader::parse_programs(device) else {
                return;
            };
            let Some(first) = programs.first() else {
                return;
            };
            let Some(stream) = stream_names(&device[..first.pos]) else {
                no_stream += 1;
                return;
            };

            let group = GroupData::new(section.group_data().to_vec());
            let mut group_hashes: BTreeSet<u32> = BTreeSet::new();
            for run in group.runs() {
                for record in run {
                    group_hashes.insert(record.hash);
                }
            }

            sections += 1;
            let stream_set: BTreeSet<u32> = stream.iter().copied().collect();
            let missing: Vec<u32> = stream_set.difference(&group_hashes).copied().collect();
            if missing.is_empty() {
                agree += 1;
                if verbose {
                    println!("agree stream={} {}", stream.len(), file.display());
                }
            } else {
                mismatches += 1;
                println!(
                    "UNEXPLAINED stream={} group={} names=[{}] {}",
                    stream.len(),
                    group_hashes.len(),
                    missing
                        .iter()
                        .map(|hash| format!("{hash:08X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    file.display()
                );
            }
        });
    }

    eprintln!(
        "{sections} section(s) from {files} file(s): {agree} agree, {mismatches} mismatch, \
         {no_stream} without a walkable stream"
    );
    Ok(())
}
