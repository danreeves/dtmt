//! Tests whether the device preamble head's third word (`w2`) counts the
//! constant buffers a shader's programs use. For every material section it
//! prints one line: `w2`, the distinct constant buffer names across the
//! programs (all stages, and per stage), the program count, the preamble's
//! config count and its stream count.
//!
//! ```text
//! w2_probe [--limit <n>] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader::{self, Section, Stage, Tail};

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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut limit = usize::MAX;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--limit" => {
                i += 1;
                limit = args.get(i).expect("--limit needs a number").parse()?;
            }
            other => paths.push(PathBuf::from(other)),
        }
        i += 1;
    }
    if paths.is_empty() {
        eprintln!("usage: w2_probe [--limit <n>] <file or directory>...");
        std::process::exit(1);
    }

    let mut sections = 0usize;
    let mut files = 0usize;
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
            let preamble = &device[..first.pos];
            if preamble.len() < 16 || u32_at(preamble, 0) != 1 {
                return;
            }
            let w2 = u32_at(preamble, 8);
            let configs = u32_at(preamble, 12) as usize;
            let stream_at = 16 + configs * 13;
            let stream = if stream_at + 4 <= preamble.len() {
                u32_at(preamble, stream_at) as usize
            } else {
                usize::MAX
            };

            let mut all: BTreeSet<u32> = BTreeSet::new();
            let mut vertex: BTreeSet<u32> = BTreeSet::new();
            let mut pixel: BTreeSet<u32> = BTreeSet::new();
            let mut distinct_lists = 0usize;
            let mut tails = 0usize;
            let mut list_sets: BTreeSet<Vec<usize>> = BTreeSet::new();
            for (index, program) in programs.iter().enumerate() {
                let next = programs
                    .get(index + 1)
                    .map(|program| program.pos)
                    .unwrap_or(device.len());
                let Some(tail) = device.get(program.meta_pos + 16..next) else {
                    continue;
                };
                let Some(parsed) = Tail::parse(tail) else {
                    continue;
                };
                tails += 1;
                for entry in &parsed.cbuffers {
                    all.insert(entry.name_hash());
                    match program.stage {
                        Stage::Vertex => {
                            vertex.insert(entry.name_hash());
                        }
                        Stage::Pixel => {
                            pixel.insert(entry.name_hash());
                        }
                        _ => {}
                    }
                }
                if let Some(lists) = shader::TailLists::parse(&parsed.rest) {
                    let counts = lists.lists.iter().map(Vec::len).collect::<Vec<_>>();
                    if list_sets.insert(counts) {
                        distinct_lists += 1;
                    }
                }
            }

            println!(
                "w2={w2} cbuffers={} vertex={} pixel={} programs={} tails={tails} \
                 configs={configs} stream={stream} list_sets={distinct_lists} {}",
                all.len(),
                vertex.len(),
                pixel.len(),
                programs.len(),
                file.display()
            );
            sections += 1;
        });
    }

    eprintln!("{sections} section(s) from {files} file(s)");
    Ok(())
}
