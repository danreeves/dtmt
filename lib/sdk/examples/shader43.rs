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
use std::process::Command;

use sdk::filetype::material::{self, ShaderOverrides};
use sdk::filetype::shader;
use sdk::murmur;
use sdk::murmur::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump_dir: Option<PathBuf> = None;
    let mut rebuild_dir: Option<PathBuf> = None;
    let mut decompile_dir: Option<PathBuf> = None;
    let mut variables_dict: Option<PathBuf> = None;
    let mut dxil_spirv: Option<PathBuf> = None;
    let mut spirv_cross: Option<PathBuf> = None;
    let mut program_filter: Option<usize> = None;
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
            "--decompile" => {
                i += 1;
                decompile_dir = Some(PathBuf::from(
                    args.get(i).expect("--decompile needs a directory"),
                ));
            }
            "--dxil-spirv" => {
                i += 1;
                dxil_spirv = Some(PathBuf::from(
                    args.get(i).expect("--dxil-spirv needs a path"),
                ));
            }
            "--spirv-cross" => {
                i += 1;
                spirv_cross = Some(PathBuf::from(
                    args.get(i).expect("--spirv-cross needs a path"),
                ));
            }
            "--program" => {
                i += 1;
                program_filter = Some(
                    args.get(i)
                        .expect("--program needs an index")
                        .parse()
                        .expect("--program must be a number"),
                );
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
             shader43 --variables <dictionary.csv> <material data file>...\n       \
             shader43 --decompile <dir> [--program <index>] [--dxil-spirv <exe>] \
             [--spirv-cross <exe>] <material data file>..."
        );
        std::process::exit(1);
    }

    let variable_names = match &variables_dict {
        Some(path) => Some(load_dictionary(path)?),
        None => None,
    };

    let dxil_spirv = dxil_spirv.unwrap_or_else(|| {
        PathBuf::from(std::env::var("DXIL_SPIRV").unwrap_or_else(|_| "dxil-spirv".to_string()))
    });
    let spirv_cross = spirv_cross.unwrap_or_else(|| {
        PathBuf::from(std::env::var("SPIRV_CROSS").unwrap_or_else(|_| "spirv-cross".to_string()))
    });

    for path in &files {
        let result = match (&rebuild_dir, &variable_names, &decompile_dir) {
            (Some(dir), _, _) => rebuild(path, dir, &overrides),
            (_, Some(names), _) => variables(path, names),
            (_, _, Some(dir)) => decompile(path, dir, program_filter, &dxil_spirv, &spirv_cross),
            _ => inspect(path, dump_dir.as_deref()),
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

/// Entry point name a decompiled shader should use.
fn entry_point(stage: shader::Stage) -> Option<&'static str> {
    match stage {
        shader::Stage::Pixel => Some("ps_main"),
        shader::Stage::Vertex => Some("vs_main"),
        _ => None,
    }
}

/// Translates a program's container to HLSL with `dxil-spirv` and
/// `spirv-cross`, then restores the entry point and semantic names from the
/// container's own signatures so the result compiles as-is.
fn decompile(
    path: &Path,
    out_dir: &Path,
    program_filter: Option<usize>,
    dxil_spirv: &Path,
    spirv_cross: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;

    fs::create_dir_all(out_dir)?;
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    for program in &programs {
        if let Some(index) = program_filter
            && program.index != index
        {
            continue;
        }

        let base = out_dir.join(format!("{stem}_p{:02}", program.index));
        let container_path = base.with_extension("original.dxbc");
        fs::write(&container_path, &program.container)?;

        let spv_path = base.with_extension("spv");
        let output = Command::new(dxil_spirv)
            .arg(&container_path)
            .arg("--output")
            .arg(&spv_path)
            .output()
            .map_err(|err| format!("Failed to run '{}': {err}", dxil_spirv.display()))?;
        if !output.status.success() {
            println!(
                "  p{:02} {:?}: dxil-spirv failed: {}",
                program.index,
                program.stage,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            continue;
        }

        let hlsl_path = base.with_extension("hlsl");
        let output = Command::new(spirv_cross)
            .args(["--hlsl", "--shader-model", "60"])
            .arg(&spv_path)
            .arg("--output")
            .arg(&hlsl_path)
            .output()
            .map_err(|err| format!("Failed to run '{}': {err}", spirv_cross.display()))?;
        if !output.status.success() {
            println!(
                "  p{:02} {:?}: spirv-cross failed: {}",
                program.index,
                program.stage,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            continue;
        }

        let hlsl = fs::read_to_string(&hlsl_path)?;
        let fixed = fix_up_hlsl(&hlsl, program.stage, &program.container);
        fs::write(&hlsl_path, &fixed)?;

        let entry = entry_point(program.stage).unwrap_or("main");
        let target = match program.stage {
            shader::Stage::Vertex => "vs_6_0",
            _ => "ps_6_0",
        };

        println!(
            "  p{:02} {:?}: {} (dxc -T {target} -E {entry})",
            program.index,
            program.stage,
            hlsl_path.display()
        );
    }

    Ok(())
}

/// Restores the entry point, the semantic names and any signature elements that
/// `spirv-cross` dropped because the shader did not use them.
///
/// The generated structs use `TEXCOORD<register>` semantics and only contain
/// the elements the shader reads. Rebuilding them in the shipped signature's
/// register order puts every element back, so the compiled program matches the
/// original interface (and the engine's input layouts).
fn fix_up_hlsl(hlsl: &str, stage: shader::Stage, container: &[u8]) -> String {
    let (input, output) = shader::signatures(container).unwrap_or_default();
    let mut lines: Vec<String> = hlsl.lines().map(str::to_string).collect();

    if let Some((start, end)) = struct_range(&lines, "struct SPIRV_Cross_Input") {
        let body_start = body_start(&lines, start, end);
        let body = lines[body_start..end].to_vec();
        let rebuilt = rebuild_struct(&body, &input);
        lines.splice(body_start..end, rebuilt);
    }

    if let Some((start, end)) = struct_range(&lines, "struct SPIRV_Cross_Output") {
        let body_start = body_start(&lines, start, end);
        let body = lines[body_start..end].to_vec();
        let rebuilt = rebuild_struct(&body, &output);
        lines.splice(body_start..end, rebuilt);
    }

    let mut result = lines.join("\n");
    result.push('\n');

    if let Some(entry) = entry_point(stage) {
        result = result.replace(" main(", &format!(" {entry}("));
    }

    result
}

/// Returns the line range of a struct's body, excluding the `struct` line and
/// the closing `};`.
fn struct_range(lines: &[String], header: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| line.contains(header))?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with("};"))?
        + start
        + 1;
    Some((start, end))
}

/// First line of a struct's body: after the opening brace.
fn body_start(lines: &[String], start: usize, end: usize) -> usize {
    lines[start + 1..end]
        .iter()
        .position(|line| line.contains('{'))
        .map(|offset| start + 1 + offset + 1)
        .unwrap_or(start + 1)
}

/// Rewrites a generated signature struct so it contains the original elements
/// in register order, with their original names and indices.
///
/// Signature elements may share a register (with different masks), and the
/// generated struct only contains the ones the shader reads, so the elements
/// are paired with the generated fields register by register, in order.
fn rebuild_struct(body: &[String], elements: &[shader::SignatureElement]) -> Vec<String> {
    let registers: Vec<Option<u32>> = body.iter().map(|line| texcoord_register(line)).collect();
    let semantics: Vec<Option<(String, u32)>> =
        body.iter().map(|line| parse_semantic(line)).collect();
    let mut used = vec![false; body.len()];
    let mut rebuilt = Vec::with_capacity(body.len());

    let mut elements: Vec<&shader::SignatureElement> = elements.iter().collect();
    elements.sort_by_key(|element| (element.register, element.index));

    for element in elements {
        let semantic = semantic_name(element);

        // Prefer an exact semantic match (`SV_Target0`, `SV_Position`, ...),
        // then pair with the next unused field in the same register.
        let found = body
            .iter()
            .enumerate()
            .position(|(index, _)| {
                !used[index]
                    && semantics[index]
                        .as_ref()
                        .is_some_and(|(name, element_index)| {
                            *name == element.name && *element_index == element.index
                        })
            })
            .or_else(|| {
                body.iter().enumerate().position(|(index, _)| {
                    !used[index] && registers[index] == Some(element.register)
                })
            });

        if let Some(index) = found {
            used[index] = true;
            rebuilt.push(restore_semantic(&body[index], &semantic));
        } else {
            // The shader does not read this element, so `spirv-cross` dropped
            // it; declare it again so the signature still matches.
            let kind = match element.mask.count_ones() {
                1 => "float",
                2 => "float2",
                3 => "float3",
                _ => "float4",
            };
            rebuilt.push(format!("    {kind} unused_{semantic} : {semantic};"));
        }
    }

    // Keep anything the generator emitted that did not match an element.
    for (index, line) in body.iter().enumerate() {
        if !used[index] {
            rebuilt.push(line.clone());
        }
    }

    rebuilt
}

/// Register a generated struct field is bound to (`: TEXCOORD<register>`).
fn texcoord_register(line: &str) -> Option<u32> {
    const SEMANTIC: &str = ": TEXCOORD";

    let position = line.find(SEMANTIC)?;
    let after = &line[position + SEMANTIC.len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    Some(digits.parse().unwrap_or(0))
}

/// Semantic of a generated struct field, split into name and index
/// (`SV_Target0` becomes `("SV_Target", 0)`, `CUSTOM1` becomes `("CUSTOM", 1)`).
fn parse_semantic(line: &str) -> Option<(String, u32)> {
    let position = line.find(": ")?;
    let rest = &line[position + 2..];
    let token: String = rest
        .chars()
        .take_while(|next| next.is_ascii_alphanumeric() || *next == '_')
        .collect();

    if token.is_empty() {
        return None;
    }

    let split = token
        .find(|next: char| next.is_ascii_digit())
        .unwrap_or(token.len());
    let (name, digits) = token.split_at(split);

    Some((name.to_string(), digits.parse().unwrap_or(0)))
}

/// The HLSL form of an element's name and index.
fn semantic_name(element: &shader::SignatureElement) -> String {
    if element.index == 0 {
        element.name.clone()
    } else {
        format!("{}{}", element.name, element.index)
    }
}

/// Replaces a generated `TEXCOORD`-style semantic with the given one.
fn restore_semantic(line: &str, semantic: &str) -> String {
    const PATTERN: &str = ": TEXCOORD";

    let Some(position) = line.find(PATTERN) else {
        return line.to_string();
    };

    let after = &line[position + PATTERN.len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    let end = position + PATTERN.len() + digits.len();

    format!("{}: {}{}", &line[..position], semantic, &line[end..])
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
