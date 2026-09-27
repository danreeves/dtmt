//! Extracts a shader's engine-side data and generates `shader43` sections from
//! our own compiled programs.
//!
//! This is the harness for the engine-data route described in
//! `docs/Shader Section Generation Notes.md`; the target is for DTMT to
//! generate everything from the mod's own shader definitions, and this tool is
//! how the pieces are tested until then.
//!
//! ```text
//! generate_shader --engine-data <out.txt> <material data file>
//! generate_shader --generate <engine_data.txt> <base.material> <out.material> \
//!     [--vs <container.dxbc>] [--ps <container.dxbc>]
//! ```
//!
//! `--generate` replaces the `shader_data`/`shader_size` of the given SJSON
//! material with the generated section; every other field (parent, textures,
//! channels, `unk3`, ...) is kept.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sdk::filetype::shader::Stage;
use sdk::filetype::shader_engine_data::EngineData;

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

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

fn extract_engine_data(
    data_path: &Path,
    out_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine_data = EngineData::from_path(data_path)?;
    fs::write(out_path, engine_data.to_text())?;

    println!(
        "=== {} ===\n  wrote the engine data of {} program(s) ({} contexts, {} condition bytes, \
         {} group data bytes, {} preamble bytes) to {}",
        data_path.display(),
        engine_data.programs.len(),
        engine_data.context_count,
        engine_data.conditions.len(),
        engine_data.group_data.len(),
        engine_data.device_preamble.len(),
        out_path.display()
    );

    Ok(())
}

fn generate(
    engine_data_path: &Path,
    base_path: &Path,
    out_path: &Path,
    vs: Option<&Path>,
    ps: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine_data = EngineData::from_text(&fs::read_to_string(engine_data_path)?)?;

    let mut containers: HashMap<Stage, Vec<u8>> = HashMap::new();
    if let Some(vs) = vs {
        containers.insert(Stage::Vertex, fs::read(vs)?);
    }
    if let Some(ps) = ps {
        containers.insert(Stage::Pixel, fs::read(ps)?);
    }

    let section = engine_data.generate(&containers)?;

    let base = fs::read_to_string(base_path)?;
    let base = replace_hex_field(&base, "shader_data", &to_hex(&section))?;
    let base = replace_number_field(&base, "shader_size", section.len())?;
    fs::write(out_path, base)?;

    println!(
        "=== {} ===\n  generated a {} byte shader section from {} program(s) into {}",
        base_path.display(),
        section.len(),
        engine_data.programs.len(),
        out_path.display()
    );

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut engine_data_out: Option<PathBuf> = None;
    let mut generate_from: Option<PathBuf> = None;
    let mut vs: Option<PathBuf> = None;
    let mut ps: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--engine-data" => {
                i += 1;
                engine_data_out = Some(PathBuf::from(
                    args.get(i).expect("--engine-data needs an output file"),
                ));
            }
            "--generate" => {
                i += 1;
                generate_from = Some(PathBuf::from(
                    args.get(i).expect("--generate needs an engine data file"),
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

    if let Some(out) = &engine_data_out {
        let data = files
            .first()
            .ok_or("usage: generate_shader --engine-data <out.txt> <material data file>")?;
        return extract_engine_data(data, out);
    }

    if let Some(engine_data_path) = &generate_from {
        let [base_path, out_path] = files.as_slice() else {
            return Err(
                "usage: generate_shader --generate <engine_data.txt> <base.material> \
                        <out.material> [--vs <container.dxbc>] [--ps <container.dxbc>]"
                    .into(),
            );
        };
        return generate(
            engine_data_path,
            base_path,
            out_path,
            vs.as_deref(),
            ps.as_deref(),
        );
    }

    Err(
        "usage: generate_shader --engine-data <out.txt> <material data file>\n       \
         generate_shader --generate <engine_data.txt> <base.material> <out.material> \
         [--vs <container.dxbc>] [--ps <container.dxbc>]"
            .into(),
    )
}
