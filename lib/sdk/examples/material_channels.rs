//! Measures whether a built material's channel list is derivable from what an
//! authoring material states. The authoring format (the SDK's `.material`
//! files) carries `textures` keyed by channel name and `variables`, but no
//! channel list of its own; the built file carries one. This example compares
//! the built list with the textures' keys and the variable count, so the design
//! of the authoring field can rest on a sample size.
//!
//! ```text
//! material_channels [--limit <n>] [--verbose] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Whether the file carries its own shader section: a base material. Instance
/// materials inherit their shader through the parent chain.
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

/// The template's channel list, its textures' channel keys, its context count
/// and its variable count.
struct Template {
    channels: Vec<u32>,
    texture_keys: Vec<u32>,
    contexts: usize,
    variables: usize,
}

fn template(data: &[u8]) -> Option<Template> {
    if data.len() < 28 {
        return None;
    }
    let version = u32_at(data, 0);
    if !(60..=62).contains(&version) {
        return None;
    }
    let offset = u32_at(data, 4) as usize;
    let mut at = offset + 20;
    let mut word = |at: &mut usize| -> Option<u32> {
        let value = u32_at(data, *at);
        *at += 4;
        Some(value)
    };
    let count = word(&mut at)? as usize;
    if count > 256 {
        return None;
    }
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
        channels.push(word(&mut at)?);
    }
    let count = word(&mut at)? as usize;
    if count > 256 {
        return None;
    }
    let mut texture_keys = Vec::with_capacity(count);
    for _ in 0..count {
        texture_keys.push(word(&mut at)?);
        at += 8; // the texture's 64-bit hash
    }
    let contexts = word(&mut at)? as usize;
    at += contexts * 8;
    let variables = word(&mut at)? as usize;
    Some(Template {
        channels,
        texture_keys,
        contexts,
        variables,
    })
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
        eprintln!("usage: material_channels [--limit <n>] [--verbose] <file or directory>...");
        std::process::exit(1);
    }

    let mut files = 0usize;
    let mut base = 0usize;
    let mut instance = 0usize;
    let mut instance_with_list = 0usize;
    let mut equal = 0usize;
    let mut extra = 0usize;
    let mut missing = 0usize;
    let mut extra_hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for path in &paths {
        if files >= limit {
            break;
        }
        walk(path, &mut |file| {
            if files >= limit {
                return;
            }
            let Ok(data) = fs::read(file) else {
                return;
            };
            let Some(template) = template(&data) else {
                return;
            };
            files += 1;
            let has_shader = shader_section(&data).is_some();
            if has_shader {
                base += 1;
            } else {
                instance += 1;
                if !template.channels.is_empty() {
                    instance_with_list += 1;
                }
            }
            let channel_set: BTreeSet<u32> = template.channels.iter().copied().collect();
            let texture_set: BTreeSet<u32> = template.texture_keys.iter().copied().collect();
            let only_channels: Vec<u32> = channel_set.difference(&texture_set).copied().collect();
            let only_textures: Vec<u32> = texture_set.difference(&channel_set).copied().collect();
            if channel_set == texture_set {
                equal += 1;
                if verbose {
                    println!(
                        "same {} channels={} vars={} contexts={} {}",
                        if has_shader { "base" } else { "instance" },
                        template.channels.len(),
                        template.variables,
                        template.contexts,
                        file.display()
                    );
                }
            } else {
                if !only_channels.is_empty() {
                    extra += 1;
                }
                if !only_textures.is_empty() {
                    missing += 1;
                }
                *extra_hist.entry(only_channels.len()).or_default() += 1;
                println!(
                    "{} channels={} textures={} extra=[{}] unbound=[{}] vars={} contexts={} {}",
                    if has_shader { "base" } else { "instance" },
                    template.channels.len(),
                    template.texture_keys.len(),
                    only_channels
                        .iter()
                        .map(|hash| format!("{hash:08X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    only_textures
                        .iter()
                        .map(|hash| format!("{hash:08X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    template.variables,
                    template.contexts,
                    file.display()
                );
            }
        });
    }

    eprintln!("{files} material(s): {base} base, {instance} instance");
    eprintln!("  instance materials with a channel list: {instance_with_list}");
    eprintln!("  channel list == textures' keys: {equal}");
    eprintln!("  channels without a texture:     {extra}");
    eprintln!("  textures without a channel:     {missing}");
    eprintln!("  extra-channel counts: {extra_hist:?}");
    Ok(())
}
