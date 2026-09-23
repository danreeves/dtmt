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

use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::material::{self, ShaderOverrides};
use sdk::filetype::shader;
use sdk::murmur;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump_dir: Option<PathBuf> = None;
    let mut rebuild_dir: Option<PathBuf> = None;
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
            other => files.push(PathBuf::from(other)),
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!(
            "usage: shader43 [--dump <dir>] <material data file>...\n       \
             shader43 --rebuild <dir> [--replace-ps <file>] [--replace-vs <file>] \
             <material data file>..."
        );
        std::process::exit(1);
    }

    for path in &files {
        let result = if let Some(dir) = &rebuild_dir {
            rebuild(path, dir, &overrides)
        } else {
            inspect(path, dump_dir.as_deref())
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
