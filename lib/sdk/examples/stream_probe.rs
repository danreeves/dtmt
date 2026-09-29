//! Tests whether a material's device-preamble stream is its own channel list
//! (plus the shader's engine resources). For every material data file it reads
//! the template's channel hashes (`unk1`, at `material_offset + 24`) and the
//! section's stream, and reports the stream names the channel list does not
//! cover (expected: the engine's render-set textures) and the channels the
//! stream does not carry (expected: slots the shader does not use).
//!
//! ```text
//! stream_probe [--limit <n>] [--verbose] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader::{self, Section};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// The material stream header (version 60/61/62) points at the shader section.
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

/// The template's channel hashes (`unk1`), from the material template record.
fn material_channels(data: &[u8]) -> Option<Vec<u32>> {
    let offset = u32_at(data, 4) as usize;
    if offset + 24 > data.len() {
        return None;
    }
    // name (u32), parent material (u64 x2), then the counted channel list.
    let count = u32_at(data, offset + 20) as usize;
    if count > 256 {
        return None;
    }
    let mut channels = Vec::with_capacity(count);
    for index in 0..count {
        channels.push(u32_at(data, offset + 24 + index * 4));
    }
    Some(channels)
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
    let mut covered = 0usize;
    let mut no_stream = 0usize;
    let mut no_channels = 0usize;
    let mut engine_freq: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
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
            let Some(channels) = material_channels(&data) else {
                no_channels += 1;
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

            sections += 1;
            let channel_set: BTreeSet<u32> = channels.iter().copied().collect();
            let stream_set: BTreeSet<u32> = stream.iter().copied().collect();
            let engine: Vec<u32> = stream_set.difference(&channel_set).copied().collect();
            let unused: Vec<u32> = channel_set.difference(&stream_set).copied().collect();
            if engine.is_empty() {
                covered += 1;
                if verbose {
                    println!(
                        "covered stream={} channels={} unused={} {}",
                        stream.len(),
                        channels.len(),
                        unused.len(),
                        file.display()
                    );
                }
            } else {
                for name in &engine {
                    *engine_freq.entry(*name).or_default() += 1;
                }
                println!(
                    "engine stream={} channels={} names=[{}] unused={} {}",
                    stream.len(),
                    channels.len(),
                    engine
                        .iter()
                        .map(|hash| format!("{hash:08X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    unused.len(),
                    file.display()
                );
            }
        });
    }

    eprintln!(
        "{sections} section(s) from {files} file(s): {covered} fully covered by the channel \
         list, {} with engine names, {no_stream} without a walkable stream, {no_channels} \
         without a channel list",
        sections - covered
    );
    let mut engine: Vec<(u32, usize)> = engine_freq.into_iter().collect();
    engine.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("distinct engine names: {}", engine.len());
    for (name, count) in engine.iter().take(24) {
        eprintln!("  {name:08X} x{count}");
    }
    Ok(())
}
