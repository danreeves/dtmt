//! Inspect and rebuild the shader section of Darktide materials.
//!
//! Usage:
//!   cargo run -p sdk --example shader43 -- <material data file>...
//!   cargo run -p sdk --example shader43 -- --dump <dir> <material data file>...
//!   cargo run -p sdk --example shader43 -- --rebuild <dir> \
//!       [--replace-ps <container.dxbc>] [--replace-vs <container.dxbc>] \
//!       <material data file>...
//!
//! `dtmt build` performs the same replacement automatically for shader sources
//! that sit next to a material (`<name>.hlsl`, `<name>.ps.hlsl`,
//! `<name>.vs.hlsl`); this example is for inspecting materials and for manual
//! experiments.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::material::{self, ShaderOverrides};
use sdk::filetype::shader;
use sdk::murmur;
use sdk::murmur::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump_dir: Option<PathBuf> = None;
    let mut rebuild_dir: Option<PathBuf> = None;
    let mut variables_dict: Option<PathBuf> = None;
    let mut overrides = ShaderOverrides::default();
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dump" => {
                i += 1;
                dump_dir = Some(PathBuf::from(
                    args.get(i).expect("--dump needs a directory"),
                ));
            }
            "--rebuild" => {
                i += 1;
                rebuild_dir = Some(PathBuf::from(
                    args.get(i).expect("--rebuild needs a directory"),
                ));
            }
            "--replace-ps" => {
                i += 1;
                overrides.pixel = Some(fs::read(
                    args.get(i).expect("--replace-ps needs a DXBC container"),
                )?);
            }
            "--replace-vs" => {
                i += 1;
                overrides.vertex = Some(fs::read(
                    args.get(i).expect("--replace-vs needs a DXBC container"),
                )?);
            }
            "--variables" => {
                i += 1;
                variables_dict = Some(PathBuf::from(
                    args.get(i).expect("--variables needs a dictionary"),
                ));
            }
            other => files.push(PathBuf::from(other)),
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!(
            "usage: shader43 [--dump <dir>] <material data file>...\n       \
             shader43 --rebuild <dir> [--replace-ps <file>] [--replace-vs <file>] \
             <material data file>...\n       \
             shader43 --variables <dictionary.csv> <material data file>..."
        );
        std::process::exit(1);
    }

    let variable_names = match &variables_dict {
        Some(path) => Some(load_dictionary(path)?),
        None => None,
    };

    for path in &files {
        let result = match (&rebuild_dir, &variable_names) {
            (Some(dir), _) => rebuild(path, dir, &overrides),
            (None, Some(names)) => variables(path, names),
            (None, None) => inspect(path, dump_dir.as_deref()),
        };
        if let Err(err) = result {
            eprintln!("{}: {err}", path.display());
        }
    }

    Ok(())
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Returns the shader section of a material stream.
fn shader_section(data: &[u8]) -> Result<&[u8], Box<dyn std::error::Error>> {
    if data.len() < 28 {
        return Err("too small to be a material".into());
    }
    let shader_offset = u32_at(data, 12) as usize;
    let shader_size = u32_at(data, 16) as usize;
    data.get(shader_offset..shader_offset + shader_size)
        .ok_or_else(|| "shader section is out of range".into())
}

fn inspect(path: &Path, dump_dir: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    println!("=== {} ({} byte shader) ===", path.display(), shader.len());

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;

    println!(
        "header: contexts {:#x}({}) conditions {:#x} default {:#x} deps {:#x}({}) \
         group {:#x}({}) device {:#x}({})",
        u32_at(shader, 8),
        u32_at(shader, 12),
        u32_at(shader, 16),
        u32_at(shader, 20),
        u32_at(shader, 24),
        u32_at(shader, 28),
        u32_at(shader, 32),
        u32_at(shader, 36),
        device_offset,
        device_size,
    );

    let programs = shader::parse_programs(device)?;
    for program in &programs {
        let frame_start = program.pos + 8;
        let frame = &device[frame_start..frame_start + program.frame_length];
        let key_ok = murmur::hash(frame, 0) == program.frame_key;

        let chunks = shader::chunks(&program.container)
            .iter()
            .map(|(name, size)| format!("{name}:{size}"))
            .collect::<Vec<_>>()
            .join(" ");

        println!(
            "  program {:<2} {:?} @{:#x} frame={} decoded={} key_ok={} chunks=[{chunks}]",
            program.index,
            program.stage,
            program.pos,
            program.frame_length,
            program.decoded_length,
            key_ok,
        );

        if let Some(dir) = dump_dir {
            fs::create_dir_all(dir)?;
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let out = dir.join(format!("{stem}_p{:02}.dxbc", program.index));
            fs::write(&out, &program.container)?;
        }
    }

    println!("  {} program(s)", programs.len());

    Ok(())
}

/// Loads a dictionary and indexes its short (32 bit) hashes.
fn load_dictionary(path: &Path) -> Result<HashMap<u32, String>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let rt = tokio::runtime::Runtime::new()?;
    let dictionary = rt.block_on(Dictionary::from_csv(&bytes[..]))?;

    let mut names = HashMap::with_capacity(dictionary.len());
    for entry in dictionary.entries() {
        names
            .entry(u32::from(entry.short()))
            .or_insert_with(|| entry.value().clone());
    }

    Ok(names)
}

/// Lists the shader variables a base material exposes.
///
/// The engine's variable table lives in the shader's group data as 20 byte
/// records: `{u32 type, u32 flags, u32 name_hash, u32 cbuffer_offset, u32 size}`.
/// Records are only accepted as part of a run of at least three consecutive
/// records, which filters out coincidental matches.
fn variables(path: &Path, names: &HashMap<u32, String>) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let group_offset = u32_at(shader, 32) as usize;
    let group_size = u32_at(shader, 36) as usize;
    let group = shader
        .get(group_offset..group_offset + group_size)
        .ok_or("group data is out of range")?;

    let record = |at: usize| -> Option<(u32, u32, u32, u32)> {
        if at + 20 > group.len() {
            return None;
        }
        let ty = u32_at(group, at);
        let flags = u32_at(group, at + 4);
        let hash = u32_at(group, at + 8);
        let offset = u32_at(group, at + 12);
        let size = u32_at(group, at + 16);

        let expected = match ty {
            0 => 4,
            1 => 8,
            2 => 12,
            3 => 16,
            4 => 64,
            _ => 0,
        };
        if ty > 12 || flags > 3 || offset > 4096 || (expected != 0 && size != expected) {
            return None;
        }

        Some((ty, hash, offset, size))
    };

    let mut found: BTreeMap<u32, (String, u32, u32)> = BTreeMap::new();
    let mut at = 0usize;
    while at + 60 <= group.len() {
        let run = record(at).is_some() && record(at + 20).is_some() && record(at + 40).is_some();
        if run
            && let Some((ty, hash, offset, size)) = record(at)
            && let Some(name) = names.get(&hash)
        {
            found.entry(offset).or_insert((name.clone(), size, ty));
        }
        at += 4;
    }

    println!("=== {} ===", path.display());
    if found.is_empty() {
        println!("  no variables found");
    }
    for (offset, (name, size, ty)) in &found {
        println!("  {name:<32} type={ty} offset={offset} size={size}");
    }

    Ok(())
}

fn rebuild(
    path: &Path,
    out_dir: &Path,
    overrides: &ShaderOverrides,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let new_data = material::replace_shader_programs(&data, overrides)?;

    fs::create_dir_all(out_dir)?;
    let out_path = out_dir.join(path.file_name().ok_or("input has no file name")?);
    fs::write(&out_path, &new_data)?;

    println!(
        "{} -> {} ({} -> {} bytes)",
        path.display(),
        out_path.display(),
        data.len(),
        new_data.len()
    );

    Ok(())
}
