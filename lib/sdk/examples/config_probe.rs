//! Dumps each material section's device-preamble config records beside the
//! section-side features they might track: the contexts (the render passes),
//! the stream (the material's channels), the program count and the cbuffers.
//!
//! The config records are the last per-shader blob in the device data. The
//! index space is a grid - the base set has columns 28..30, the optional rows
//! run eight apart (31/32, 38..40, 46..48, 54..56) and the channel row is
//! 94..101 - so the question this probe feeds is what selects a row and what
//! sets a value.
//!
//! ```text
//! config_probe [--limit <n>] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader::{self, Section, Tail};

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

/// The preamble's head and its config records, as `index:value`.
fn preamble_configs(preamble: &[u8]) -> Option<([u32; 4], Vec<(u32, u32)>)> {
    if preamble.len() < 16 || u32_at(preamble, 0) != 1 {
        return None;
    }
    let head = [
        u32_at(preamble, 0),
        u32_at(preamble, 4),
        u32_at(preamble, 8),
        u32_at(preamble, 12),
    ];
    let count = head[3] as usize;
    if 16 + count * 13 > preamble.len() {
        return None;
    }
    let mut configs = Vec::with_capacity(count);
    for index in 0..count {
        let at = 16 + index * 13;
        configs.push((u32_at(preamble, at), u32_at(preamble, at + 5)));
    }
    Some((head, configs))
}

/// The record names of the device preamble's stream.
fn stream_names(preamble: &[u8]) -> Option<Vec<u32>> {
    let (head, _) = preamble_configs(preamble)?;
    let mut at = 16 + head[3] as usize * 13;
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
        eprintln!("usage: config_probe [--limit <n>] <file or directory>...");
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
            let Some((head, configs)) = preamble_configs(preamble) else {
                return;
            };

            let contexts = section
                .contexts()
                .iter()
                .map(|context| format!("{:08X}", context.name))
                .collect::<Vec<_>>()
                .join(" ");
            let stream = stream_names(preamble)
                .unwrap_or_default()
                .iter()
                .map(|hash| format!("{hash:08X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let mut cbuffers: BTreeSet<u32> = BTreeSet::new();
            for (index, program) in programs.iter().enumerate() {
                let next = programs
                    .get(index + 1)
                    .map(|program| program.pos)
                    .unwrap_or(device.len());
                if let Some(tail) = device.get(program.meta_pos + 16..next) {
                    if let Some(parsed) = Tail::parse(tail) {
                        for entry in &parsed.cbuffers {
                            cbuffers.insert(entry.name_hash());
                        }
                    }
                }
            }

            let configs = configs
                .iter()
                .map(|(index, value)| format!("{index}:{value:X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "w1={} w2={} configs={} [{}] ctx={} [{}] stream={} [{}] programs={} cbuffers={} {}",
                head[1],
                head[2],
                head[3],
                configs,
                section.contexts().len(),
                contexts,
                stream.split(' ').filter(|s| !s.is_empty()).count(),
                stream,
                programs.len(),
                cbuffers.len(),
                file.display()
            );
            sections += 1;
        });
    }

    eprintln!("{sections} section(s) from {files} file(s)");
    Ok(())
}
