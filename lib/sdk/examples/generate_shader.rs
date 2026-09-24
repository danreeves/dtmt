//! Generates a `shader43` section from a family preset plus our own compiled
//! DXBC programs, so a mod can ship a base material without carrying a shipped
//! shader blob.
//!
//! The preset is the wrapper a shader family shares - the contexts, conditions
//! and dependencies the engine queries, the group data with its variable
//! tables, and one metadata tail per program. Extract it once from a shipped
//! base material, keep it in the mod, and generate from it:
//!
//! ```text
//! generate_shader --preset <out.txt> <material data file>
//! generate_shader --generate <preset.txt> <base.material> <out.material> \
//!     [--vs <container.dxbc>] [--ps <container.dxbc>]
//! ```
//!
//! `--generate` replaces the `shader_data`/`shader_size` of the given SJSON
//! material with the generated section; every other field (parent, textures,
//! channels, `unk3`, ...) is kept. The programs are framed into the device data
//! from our containers, one record per program in the preset, using that
//! program's tail.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader;
use sdk::murmur;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn from_hex(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if text.len() % 2 != 0 {
        return Err("hex string has an odd length".into());
    }

    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        bytes.push(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?);
    }
    Ok(bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

/// The wrapper of one shader family.
struct Preset {
    version: u32,
    opaque: u32,
    context_count: u32,
    dependency_count: u32,
    contexts: Vec<u8>,
    conditions: Vec<u8>,
    dependencies: Vec<u8>,
    group_data: Vec<u8>,
    /// The packed table before the first program record.
    device_preamble: Vec<u8>,
    /// One entry per program of the template's device data, in order.
    programs: Vec<(String, Vec<u8>)>,
}

fn stage_name(stage: shader::Stage) -> String {
    format!("{stage:?}")
}

/// Reads a raw material data file and writes the preset of its shader section.
fn extract_preset(data_path: &Path, out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(data_path)?;

    let shader_offset = u32_at(&data, 12) as usize;
    let shader_size = u32_at(&data, 16) as usize;
    let section = data
        .get(shader_offset..shader_offset + shader_size)
        .ok_or("shader section is out of range")?;

    let slice = |start: usize, end: usize| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        section
            .get(start..end)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| format!("section slice {start:#x}..{end:#x} is out of range").into())
    };

    let contexts = slice(u32_at(section, 8) as usize, u32_at(section, 16) as usize)?;
    let conditions = slice(u32_at(section, 16) as usize, u32_at(section, 24) as usize)?;
    let dependencies = slice(u32_at(section, 24) as usize, u32_at(section, 32) as usize)?;
    let group_data = slice(
        u32_at(section, 32) as usize,
        u32_at(section, 32) as usize + u32_at(section, 36) as usize,
    )?;

    let device_offset = u32_at(section, 40) as usize;
    let device_size = u32_at(section, 44) as usize;
    let device = section
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;

    // The first program record marks the end of the packed reflection table the
    // device data starts with; keep it, the engine uses it to find programs.
    let first_program = programs.first().map(|program| program.pos).unwrap_or(0);
    let device_preamble = device
        .get(..first_program)
        .ok_or("device preamble is out of range")?;

    let mut text = String::new();
    text.push_str(&format!("version {}\n", u32_at(section, 0)));
    text.push_str(&format!("opaque {}\n", u32_at(section, 4)));
    text.push_str(&format!("context_count {}\n", u32_at(section, 12)));
    text.push_str(&format!("dependency_count {}\n", u32_at(section, 28)));
    text.push_str(&format!("contexts {}\n", to_hex(&contexts)));
    text.push_str(&format!("conditions {}\n", to_hex(&conditions)));
    text.push_str(&format!("dependencies {}\n", to_hex(&dependencies)));
    text.push_str(&format!("group_data {}\n", to_hex(&group_data)));
    text.push_str(&format!("device_preamble {}\n", to_hex(device_preamble)));

    for program in &programs {
        let tail_start = program.meta_pos + 16;
        let tail_end = programs
            .iter()
            .find(|next| next.pos > tail_start)
            .map(|next| next.pos)
            .unwrap_or(device.len());
        let tail = device
            .get(tail_start..tail_end)
            .ok_or("program tail is out of range")?;

        text.push_str(&format!(
            "program {} {}\n",
            stage_name(program.stage),
            to_hex(tail)
        ));
    }

    fs::write(out_path, text)?;
    println!(
        "=== {} ===\n  wrote the preset of {} program(s) ({} contexts, {} conditions bytes, \
         {} group data bytes) to {}",
        data_path.display(),
        programs.len(),
        u32_at(section, 12),
        conditions.len(),
        group_data.len(),
        out_path.display()
    );

    Ok(())
}

fn parse_preset(text: &str) -> Result<Preset, Box<dyn std::error::Error>> {
    let mut version = 43;
    let mut opaque = 0;
    let mut context_count = 0;
    let mut dependency_count = 0;
    let mut contexts = Vec::new();
    let mut conditions = Vec::new();
    let mut dependencies = Vec::new();
    let mut group_data = Vec::new();
    let mut device_preamble = Vec::new();
    let mut programs = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Program lines carry two fields (stage plus tail hex).
        if let Some(rest) = line.strip_prefix("program ") {
            let (stage, tail) = rest.split_once(' ').ok_or("malformed program line")?;
            programs.push((stage.to_string(), from_hex(tail)?));
            continue;
        }

        let (key, value) = line.split_once(' ').unwrap_or((line, ""));

        match key {
            "version" => version = value.parse()?,
            "opaque" => opaque = value.parse()?,
            "context_count" => context_count = value.parse()?,
            "dependency_count" => dependency_count = value.parse()?,
            "contexts" => contexts = from_hex(value)?,
            "conditions" => conditions = from_hex(value)?,
            "dependencies" => dependencies = from_hex(value)?,
            "group_data" => group_data = from_hex(value)?,
            "device_preamble" => device_preamble = from_hex(value)?,
            other => return Err(format!("unknown preset key '{other}'").into()),
        }
    }

    Ok(Preset {
        version,
        opaque,
        context_count,
        dependency_count,
        contexts,
        conditions,
        dependencies,
        group_data,
        device_preamble,
        programs,
    })
}

/// Builds the device data: one framed program record per preset program, using
/// our container for the stage and the preset's tail.
fn build_device(
    preset: &Preset,
    containers: &HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut device = preset.device_preamble.clone();

    for (index, (stage, tail)) in preset.programs.iter().enumerate() {
        let container = containers
            .get(stage)
            .ok_or_else(|| format!("no container given for program {index} ({stage})"))?;

        let frame = shader::encode_frame(container)?;
        let key = murmur::hash(&frame, 0);

        device.extend_from_slice(&1u32.to_le_bytes());
        device.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        device.extend_from_slice(&frame);
        device.extend_from_slice(&5u32.to_le_bytes());
        device.extend_from_slice(&(container.len() as u32).to_le_bytes());
        device.extend_from_slice(&key.to_le_bytes());
        device.extend_from_slice(tail);
    }

    Ok(device)
}

/// Assembles the shader section from the preset and our device data.
fn build_section(preset: &Preset, device: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let contexts_offset = 48usize;
    let conditions_offset = contexts_offset + preset.contexts.len();
    let dependencies_offset = conditions_offset + preset.conditions.len();
    let group_offset = dependencies_offset + preset.dependencies.len();
    let device_offset = group_offset + preset.group_data.len();
    let default_offset = device_offset + device.len();

    let header = [
        preset.version,
        preset.opaque,
        contexts_offset as u32,
        preset.context_count,
        conditions_offset as u32,
        default_offset as u32,
        dependencies_offset as u32,
        preset.dependency_count,
        group_offset as u32,
        preset.group_data.len() as u32,
        device_offset as u32,
        device.len() as u32,
    ];

    let mut section = Vec::with_capacity(default_offset + 32);
    for word in header {
        section.extend_from_slice(&word.to_le_bytes());
    }
    section.extend_from_slice(&preset.contexts);
    section.extend_from_slice(&preset.conditions);
    section.extend_from_slice(&preset.dependencies);
    section.extend_from_slice(&preset.group_data);
    section.extend_from_slice(device);
    section.extend_from_slice(&[0u8; 16]);
    while section.len() % 16 != 0 {
        section.push(0);
    }

    Ok(section)
}

fn replace_hex_field(
    text: &str,
    field: &str,
    value: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let needle = format!("{field} = \"");
    let start = text
        .find(&needle)
        .ok_or_else(|| format!("no '{field}' field in the material"))?
        + needle.len();
    let end = text[start..]
        .find('"')
        .ok_or_else(|| format!("unterminated '{field}' field"))?
        + start;

    Ok(format!("{}{}{}", &text[..start], value, &text[end..]))
}

fn replace_number_field(
    text: &str,
    field: &str,
    value: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let needle = format!("{field} = ");
    let start = text
        .find(&needle)
        .ok_or_else(|| format!("no '{field}' field in the material"))?
        + needle.len();
    let end = text[start..]
        .find(|next: char| !next.is_ascii_digit())
        .ok_or_else(|| format!("unterminated '{field}' field"))?
        + start;

    Ok(format!("{}{}{}", &text[..start], value, &text[end..]))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut preset_out: Option<PathBuf> = None;
    let mut generate: Option<PathBuf> = None;
    let mut vs: Option<PathBuf> = None;
    let mut ps: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--preset" => {
                i += 1;
                preset_out = Some(PathBuf::from(
                    args.get(i).expect("--preset needs an output file"),
                ));
            }
            "--generate" => {
                i += 1;
                generate = Some(PathBuf::from(
                    args.get(i).expect("--generate needs a preset file"),
                ));
            }
            "--vs" => {
                i += 1;
                vs = Some(PathBuf::from(args.get(i).expect("--vs needs a container")));
            }
            "--ps" => {
                i += 1;
                ps = Some(PathBuf::from(args.get(i).expect("--ps needs a container")));
            }
            other => files.push(PathBuf::from(other)),
        }
        i += 1;
    }

    if let Some(out) = &preset_out {
        let data = files
            .first()
            .ok_or("usage: generate_shader --preset <out.txt> <material data file>")?;
        return extract_preset(data, out);
    }

    if let Some(preset_path) = &generate {
        let [base_path, out_path] = files.as_slice() else {
            return Err(
                "usage: generate_shader --generate <preset.txt> <base.material> <out.material> \
                        [--vs <container.dxbc>] [--ps <container.dxbc>]"
                    .into(),
            );
        };

        let preset = parse_preset(&fs::read_to_string(preset_path)?)?;
        let mut containers = HashMap::new();
        if let Some(vs) = &vs {
            containers.insert(stage_name(shader::Stage::Vertex), fs::read(vs)?);
        }
        if let Some(ps) = &ps {
            containers.insert(stage_name(shader::Stage::Pixel), fs::read(ps)?);
        }

        let device = build_device(&preset, &containers)?;
        let section = build_section(&preset, &device)?;

        let base = fs::read_to_string(base_path)?;
        let base = replace_hex_field(&base, "shader_data", &to_hex(&section))?;
        let base = replace_number_field(&base, "shader_size", section.len())?;
        fs::write(out_path, base)?;

        println!(
            "=== {} ===\n  generated a {} byte shader section from {} program(s) into {}",
            base_path.display(),
            section.len(),
            preset.programs.len(),
            out_path.display()
        );

        return Ok(());
    }

    Err(
        "usage: generate_shader --preset <out.txt> <material data file>\n       \
         generate_shader --generate <preset.txt> <base.material> <out.material> \
         [--vs <container.dxbc>] [--ps <container.dxbc>]"
            .into(),
    )
}
