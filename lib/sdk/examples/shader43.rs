//! Inspect and rebuild the shader section of Darktide materials.
//!
//! Usage:
//!   cargo run -p sdk --example shader43 -- <material data file>...
//!   cargo run -p sdk --example shader43 -- --dump <dir> <material data file>...
//!   cargo run -p sdk --example shader43 -- --rebuild <dir> \
//!       [--replace-ps <container.dxbc>] [--replace-vs <container.dxbc>] \
//!       <material data file>...
//!   cargo run -p sdk --example shader43 -- --decompile <dir> \
//!       [--dxil-spirv <exe>] [--spirv-cross <exe>] <material data file>...
//!   shader43 --tail <program index> <material data file>
//!   shader43 --slots <dictionary.csv> [--program <index>] [--hlsl <dir>]
//!       <material data file>
//!   shader43 --compile <dir> [--against <material>] <declaration.shader_node>
//!       <library.shader_source | directory>...
//!
//! `dtmt build` performs the same replacement automatically for shader sources
//! that sit next to a material (`<name>.hlsl`, `<name>.ps.hlsl`,
//! `<name>.vs.hlsl`); this example is for inspecting materials and for manual
//! experiments.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sdk::filetype::condition_tree::ConditionTree;
use sdk::filetype::condition::Defines;
use sdk::filetype::group_data::GroupData;
use sdk::filetype::material::{self, ShaderOverrides};
use sdk::filetype::shader;
use sdk::filetype::shader_block::{self, BlockTemplate};
use sdk::filetype::shader_decl::ChannelDef;
use sdk::filetype::shader_node::ShaderNode;
use sdk::filetype::shader_engine_data::channel_record_len;
use sdk::filetype::shader_source::ShaderSource;
use sdk::murmur;
use sdk::murmur::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dump_dir: Option<PathBuf> = None;
    let mut rebuild_dir: Option<PathBuf> = None;
    let mut decompile_dir: Option<PathBuf> = None;
    let mut variables_dict: Option<PathBuf> = None;
    let mut slots_dict: Option<PathBuf> = None;
    let mut hlsl_dir: Option<PathBuf> = None;
    let mut dxil_spirv: Option<PathBuf> = None;
    let mut spirv_cross: Option<PathBuf> = None;
    let mut program_filter: Option<usize> = None;
    let mut tail_index: Option<usize> = None;
    let mut tails_mode = false;
    let mut section: Option<String> = None;
    let mut preamble_mode = false;
    let mut dependencies_mode = false;
    let mut conditions_mode = false;
    let mut conditions_map_mode = false;
    let mut channels_mode = false;
    let mut layout_mode = false;
    let mut plan_declaration: Option<PathBuf> = None;
    let mut reconstruct_dir: Option<PathBuf> = None;
    let mut substitute_mode = false;
    let mut records_mode = false;
    let mut registry_mode = false;
    let mut channel_filter: Option<String> = None;
    let mut block_declaration: Option<PathBuf> = None;
    let mut compile_dir: Option<PathBuf> = None;
    let mut against: Option<PathBuf> = None;
    let mut group_data_mode = false;
    let mut group_conditions_mode = false;
    let mut graph_mode = false;
    let mut core_dir: Option<PathBuf> = None;
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
            "--registry" => {
                i += 1;
                registry_mode = true;
                variables_dict = Some(PathBuf::from(
                    args.get(i).expect("--registry needs a dictionary"),
                ));
            }
            "--slots" => {
                i += 1;
                slots_dict = Some(PathBuf::from(
                    args.get(i).expect("--slots needs a dictionary"),
                ));
            }
            "--hlsl" => {
                i += 1;
                hlsl_dir = Some(PathBuf::from(
                    args.get(i).expect("--hlsl needs a directory"),
                ));
            }
            "--section" => {
                i += 1;
                section = Some(args.get(i).expect("--section needs a name").clone());
            }
            "--preamble" => preamble_mode = true,
            "--dependencies" => dependencies_mode = true,
            "--conditions" => conditions_mode = true,
            "--conditions-map" => conditions_map_mode = true,
            "--channels" => channels_mode = true,
            "--layout" => layout_mode = true,
            "--plan" => {
                i += 1;
                plan_declaration = Some(PathBuf::from(
                    args.get(i).expect("--plan needs a declaration").clone(),
                ));
            }
            "--reconstruct" => {
                i += 1;
                reconstruct_dir = Some(PathBuf::from(
                    args.get(i).expect("--reconstruct needs a directory").clone(),
                ));
            }
            "--substitute" => substitute_mode = true,
            "--build-block" => {
                i += 1;
                block_declaration = Some(PathBuf::from(
                    args.get(i).expect("--build-block needs a shader declaration"),
                ));
            }
            "--group-data" => {
                group_data_mode = true;
            }
            "--group-conditions" => {
                group_conditions_mode = true;
            }
            "--graph" => {
                graph_mode = true;
            }
            "--core" => {
                i += 1;
                core_dir = Some(PathBuf::from(
                    args.get(i).expect("--core needs a directory"),
                ));
            }
            "--compile" => {
                i += 1;
                compile_dir = Some(PathBuf::from(
                    args.get(i).expect("--compile needs a directory"),
                ));
            }
            "--against" => {
                i += 1;
                against = Some(PathBuf::from(
                    args.get(i).expect("--against needs a material or section"),
                ));
            }
            "--records" => records_mode = true,
            "--channel" => {
                i += 1;
                channel_filter = Some(args.get(i).expect("--channel needs a name").clone());
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
            "--tail" => {
                i += 1;
                tail_index = Some(
                    args.get(i)
                        .expect("--tail needs an index")
                        .parse()
                        .expect("--tail must be a number"),
                );
            }
            "--tails" => tails_mode = true,
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
             [--spirv-cross <exe>] <material data file>...\n       \
             shader43 --tail <program index> <material data file>\n       \
             shader43 --tails [--variables <dictionary.csv>] <material data file>\n       \
             shader43 --reconstruct <dir> [--variables <dictionary.csv>] \
             [--dxil-spirv <exe>] [--spirv-cross <exe>] <material data file>...\n       \
             shader43 --compile <dir> [--against <material>] \
             <declaration.shader_node> <library.shader_source | directory>...\n       \
             shader43 --slots <dictionary.csv> [--program <index>] [--hlsl <dir>] \
             <material data file>..."
        );
        std::process::exit(1);
    }

    if let Some(dir) = &compile_dir {
        let (declaration, libraries) = files
            .split_first()
            .ok_or("--compile needs a declaration and its libraries")?;
        return compile(declaration, libraries, dir, against.as_deref(), core_dir.as_deref());
    }

    if let Some(index) = tail_index {
        return tail(&files[0], index);
    }

    if tails_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = tails(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(section) = &section {
        for path in &files {
            if let Err(err) = dump_section(path, section) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if preamble_mode {
        for path in &files {
            if let Err(err) = dump_preamble(path, dump_dir.as_deref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if substitute_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = substitute(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(dir) = &reconstruct_dir {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        let dxil = decompiler(&dxil_spirv, "DXIL_SPIRV", "dxil-spirv");
        let cross = decompiler(&spirv_cross, "SPIRV_CROSS", "spirv-cross");
        for path in &files {
            if let Err(err) = reconstruct(path, dir, names.as_ref(), &dxil, &cross) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(declaration) = &plan_declaration {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = plan(declaration, path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if layout_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = layout(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if channels_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = channels(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if dependencies_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = dependencies(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if conditions_map_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = conditions_map(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if conditions_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = conditions(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if group_data_mode {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = group_data(path, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if group_conditions_mode {
        for path in &files {
            if let Err(err) = group_conditions(path) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if graph_mode {
        for path in &files {
            if let Err(err) = graph(path, core_dir.as_deref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(declaration) = &block_declaration {
        let names = match &variables_dict {
            Some(path) => Some(load_dictionary(path)?),
            None => None,
        };
        for path in &files {
            if let Err(err) = build_block(path, declaration, names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(dict) = &slots_dict {
        let names = load_dictionary(dict)?;
        for path in &files {
            if let Err(err) = slots(path, &names, hlsl_dir.as_deref(), program_filter) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    let variable_names = match &variables_dict {
        Some(path) => Some(load_dictionary(path)?),
        None => None,
    };

    if registry_mode {
        let names = variable_names.ok_or("--registry needs a dictionary")?;
        for path in &files {
            if let Err(err) = registry(path, &names) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if records_mode {
        for path in &files {
            if let Err(err) = records(path, variable_names.as_ref()) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    if let Some(channel) = &channel_filter {
        let hash = hash_name(channel);
        for path in &files {
            if let Err(err) = channel_records(path, hash) {
                eprintln!("{}: {err}", path.display());
            }
        }
        return Ok(());
    }

    let dxil_spirv = decompiler(&dxil_spirv, "DXIL_SPIRV", "dxil-spirv");
    let spirv_cross = decompiler(&spirv_cross, "SPIRV_CROSS", "spirv-cross");

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

/// Dumps the metadata tail of one program: the counted tables and opaque words
/// between its frame key and the next program's frame.
fn tail(path: &Path, index: usize) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;

    let programs = shader::parse_programs(device)?;
    let program = programs
        .get(index)
        .ok_or_else(|| format!("no program {index} (of {})", programs.len()))?;
    let next_pos = programs
        .get(index + 1)
        .map(|next| next.pos)
        .unwrap_or(device.len());

    let tail = &device[program.meta_pos + 16..next_pos];
    let start = program.meta_pos + 16;

    let chunks = shader::chunks(&program.container)
        .iter()
        .map(|(name, size)| format!("{name}:{size}"))
        .collect::<Vec<_>>()
        .join(" ");

    println!(
        "=== {} program {} ({:?}) ===\n  record @{:#x}..{:#x}, tail is {} bytes \
         (relative offsets are from the shader section)\n  chunks [{}]",
        path.display(),
        index,
        program.stage,
        program.pos,
        next_pos,
        tail.len(),
        chunks
    );

    for at in (0..tail.len()).step_by(4) {
        let value = u32_at(tail, at);
        let mut words = format!("+{:04} @{:08x}: {value:12} 0x{value:08x}", at, start + at);
        if at + 7 < tail.len() {
            let qword = u64::from_le_bytes(tail[at..at + 8].try_into().unwrap());
            words.push_str(&format!("   q={qword:016x}"));
        }
        println!("{words}");
    }

    Ok(())
}

/// Dumps the metadata tail of every program, four words per line, with words
/// that the dictionary can name shown next to their value.
fn tails(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;

    let programs = shader::parse_programs(device)?;

    if let Some(first) = programs.first() {
        let preamble = device.get(..first.pos).unwrap_or_default();
        println!("=== preamble, {} bytes ===", preamble.len());
        for (row, chunk) in preamble.chunks(16).enumerate() {
            let mut line = format!("+{:04}:", row * 16);
            for word in chunk.chunks_exact(4) {
                let value = u32::from_le_bytes(word.try_into().unwrap());
                let name = names
                    .and_then(|names| names.get(&value))
                    .map(|name| format!(" {name}"))
                    .unwrap_or_default();
                line.push_str(&format!(" {value:>10}{name}"));
            }
            println!("{line}");
        }
    }

    for (index, program) in programs.iter().enumerate() {
        let next_pos = programs
            .get(index + 1)
            .map(|next| next.pos)
            .unwrap_or(device.len());
        let start = program.meta_pos + 16;
        let Some(tail) = device.get(start..next_pos) else {
            continue;
        };

        println!(
            "=== program {index} ({:?}), tail {} bytes ===",
            program.stage,
            tail.len()
        );
        match shader::Tail::parse(tail) {
            Some(parsed) => {
                let round_trip = parsed.bytes() == tail;
                let cbuffers = parsed
                    .cbuffers
                    .iter()
                    .map(|entry| {
                        let name = names
                            .and_then(|names| names.get(&entry.name_hash()))
                            .map(String::as_str)
                            .unwrap_or("?");
                        format!("{name}:{}", entry.size())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "  cbuffers [{cbuffers}] roundtrip={}",
                    if round_trip { "ok" } else { "FAILED" }
                );
            }
            None => println!("  cbuffers unparsed"),
        }
        for (row, chunk) in tail.chunks(16).enumerate() {
            let mut line = format!("+{:04}:", row * 16);
            for word in chunk.chunks_exact(4) {
                let value = u32::from_le_bytes(word.try_into().unwrap());
                let name = names
                    .and_then(|names| names.get(&value))
                    .map(|name| format!(" {name}"))
                    .unwrap_or_default();
                line.push_str(&format!(" {value:>10}{name}"));
            }
            println!("{line}");
        }
    }

    Ok(())
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

/// Pairs the block's binding records with the group's variable tables. A block
/// record's index is the engine's global variable order, and a declaration's table
/// lists the variables it uses in that order, so every run position that also
/// appears in the block's index set names that index. This is the table a
/// generated declaration needs for the engine variables it touches.
fn registry(path: &Path, names: &HashMap<u32, String>) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;
    let first = programs.first().map(|program| program.pos).unwrap_or(0);
    let preamble = device.get(..first).ok_or("preamble is out of range")?;

    let count = u32_at(preamble, 12).saturating_sub(8) as usize;
    let mut indices = BTreeMap::new();
    for i in 0..count {
        let at = 0x78 + i * 13;
        if at + 13 > preamble.len() {
            break;
        }
        indices.insert(u32_at(preamble, at), u32_at(preamble, at + 5));
    }

    let group_offset = u32_at(shader, 32) as usize;
    let group_size = u32_at(shader, 36) as usize;
    let group = shader
        .get(group_offset..group_offset + group_size)
        .ok_or("group data is out of range")?;

    let record = |at: usize| -> Option<u32> {
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
        Some(hash)
    };

    // Collect the runs once, then report the positions the block also names.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut at = 0usize;
    while at + 20 <= group.len() {
        if record(at).is_some() {
            let mut start = at;
            while start >= 20 && record(start - 20).is_some() {
                start -= 20;
            }
            let mut end = at + 20;
            while record(end).is_some() {
                end += 20;
            }
            let run = (start, (end - start) / 20);
            if !runs.contains(&run) {
                runs.push(run);
            }
            at = end;
        } else {
            at += 4;
        }
    }

    println!("=== {} ===", path.display());
    println!(
        "  {} block record(s), {} variable run(s)",
        indices.len(),
        runs.len()
    );
    let mut named = 0;
    for (start, len) in &runs {
        for i in 0..*len {
            let Some(hash) = record(start + i * 20) else {
                continue;
            };
            let Some(&value) = indices.get(&(i as u32)) else {
                continue;
            };
            let index = i as u32;
            let name = names.get(&hash).map(String::as_str).unwrap_or("?");
            named += 1;
            println!("  run+{start:#06x}[{i:>3}] = index {index:<3} value {value:<10} {name}");
        }
    }
    println!("  {named} of the block's indices named by the tables");
    let missing: Vec<String> = indices
        .keys()
        .copied()
        .filter(|index| {
            !runs.iter().any(|(start, len)| {
                (0..*len).any(|i| record(start + i * 20).is_some() && *index == i as u32)
            })
        })
        .map(|index| format!("{index}"))
        .collect();
    println!("  unnamed block indices: {}", missing.join(" "));
    Ok(())
}

/// Reads a section's group data: the descriptors, the tables it can find, and
/// whether rebuilding it from the table it already carries is a no-op.
/// The byte ranges at which two versions of a section differ, as runs.
fn diffs(old: &[u8], new: &[u8]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let end = old.len().min(new.len());
    let mut at = 0;
    while at < end {
        if old[at] == new[at] {
            at += 1;
            continue;
        }
        let start = at;
        while at < end && old[at] != new[at] {
            at += 1;
        }
        runs.push((start, at - start));
    }
    runs
}

fn substitute(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::{GroupData, Variable};
    use sdk::filetype::shader::Section;
    use sdk::murmur::Murmur32;

    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let named = |hash: u32| match names.and_then(|names| names.get(&hash)) {
        Some(name) => format!("{name}"),
        None => String::new(),
    };
    println!("=== {} ===", path.display());
    let original = Section::parse(bytes)?;

    // One: rename a context. Nothing but that context's name word may move.
    let probe: u32 = Murmur32::hash("probe_context").into();
    let mut section = original.clone();
    let before = section.contexts()[0].name;
    section.contexts_mut()[0].name = probe;
    let runs = diffs(bytes, &section.into_bytes());
    let at = 48;
    println!(
        "  a renamed context: {:08X} {} -> {:08X}: {}",
        before,
        named(before),
        probe,
        if runs == [(at, 4)] {
            "4 bytes at +48, and only those"
        } else {
            println!("    {runs:?}");
            "MOVED MORE"
        }
    );

    // Two: a length-changing edit, which has to move every offset after the
    // contexts table. A rename never does, so this is the half of the test the
    // round trip cannot cover. The query and the group count are raised together,
    // because the section now checks that the queries are the groups; anything
    // else would be testing an invalid section. Checked by reading the rebuilt
    // section back rather than against a known answer.
    let group = original.group_data().to_vec();
    let mut wider = original.clone();
    let groups = u32::from_le_bytes(group[0..4].try_into().unwrap());
    wider.contexts_mut()[0].queries.push(sdk::filetype::shader::Query {
        id: 0xC0DE_0001,
        conditions: sdk::filetype::shader::NO_CONDITIONS,
    });
    let mut wider_group = group.clone();
    wider_group[0..4].copy_from_slice(&(groups + 1).to_le_bytes());
    wider.set_group_data(wider_group.clone());
    let rebuilt = wider.into_bytes();
    match Section::parse(&rebuilt) {
        Err(err) => println!("  a query added: the rebuilt section did not read: {err}"),
        Ok(back) => {
            let conditions = u32::from_le_bytes(rebuilt[16..20].try_into().unwrap()) as usize;
            let expect: usize =
                48 + back.contexts().iter().map(|context| context.len()).sum::<usize>();
            println!(
                "  a query added: {} contexts, conditions at {conditions} (48 + the {} record bytes = {expect}): {}",
                back.contexts().len(),
                expect - 48,
                if conditions == expect
                    && back.group_data() == wider_group.as_slice()
                    && back.contexts()[0].queries.len() == original.contexts()[0].queries.len() + 1
                {
                    "the offsets followed the sum of the records, the group data is what was written"
                } else {
                    "THE SUM DID NOT HOLD"
                }
            );
        }
    }

    // Three: rename one material variable, through the group data's own rebuild.
    // Nothing outside the group data may move, and inside it only the one name.
    let group_data = GroupData::new(group.clone());
    let table = group_data.object_table().map(|(_, table)| table);
    // The first record the dictionary can name, so the rename has a real name to
    // go to and a real old name to replace.
    let named_index = table.as_ref().and_then(|table| {
        table
            .iter()
            .position(|record| names.is_some_and(|names| names.contains_key(&record.hash)))
    });
    let (Some(table), Some(index)) = (table, named_index) else {
        println!("  a renamed variable: skipped, no named variable in the table");
        return Ok(());
    };
    let mut variables: Vec<Variable> = table
        .iter()
        .map(
            |record| match names.and_then(|names| names.get(&record.hash)) {
                Some(name) => Variable::new(name.clone(), record.offset, record.kind),
                None => Variable::from_hash(record.hash, record.offset, record.kind),
            },
        )
        .collect();
    let old_name = variables[index].name.clone();
    variables[index].name = "probe_variable".to_string();
    let rebuilt_group = group_data.rebuild(&variables)?;
    let mut section = original;
    section.set_group_data(rebuilt_group);
    let section_bytes = section.into_bytes();
    let runs = diffs(bytes, &section_bytes);
    let group_at = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    let device_at = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    let inside = runs
        .iter()
        .all(|(at, _)| *at >= group_at && *at < device_at);
    let total: usize = runs.iter().map(|(_, len)| len).sum();
    println!(
        "  a renamed variable: {old_name} -> probe_variable: {total} bytes in {} run(s): {}",
        runs.len(),
        if inside {
            "all inside the group data"
        } else {
            println!("    {runs:?}");
            "OUTSIDE THE GROUP DATA"
        }
    );
    Ok(())
}

/// A decompiler tool path: the explicit flag, the environment variable, or the
/// tool's name on `PATH`.
fn decompiler(explicit: &Option<PathBuf>, env: &str, default: &str) -> PathBuf {
    explicit.clone().unwrap_or_else(|| {
        PathBuf::from(std::env::var(env).unwrap_or_else(|_| default.to_string()))
    })
}

/// Reconstructs the dialect source of a compiled section: a `.shader_node`
/// declaration skeleton, a `.shader_source` with the decompiled programs (when
/// the decompiler tools are available), and the `.engine_data` carry. This is
/// the bundle -> source direction; the three files are what `dtmt build` needs
/// to generate the section again.
fn reconstruct(
    path: &Path,
    dir: &Path,
    names: Option<&HashMap<u32, String>>,
    dxil_spirv: &Path,
    spirv_cross: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::GroupData;
    use sdk::filetype::shader::Section;
    use sdk::filetype::shader_engine_data::EngineData;

    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let section = Section::parse(bytes)?;
    let group = GroupData::new(section.group_data().to_vec());
    let query_ids: Vec<u32> = section
        .contexts()
        .iter()
        .flat_map(|context| context.queries.iter().map(|query| query.id))
        .collect();
    let name_of = |hash: u32| names.and_then(|names| names.get(&hash)).cloned();
    let type_of = |kind: u32| match kind {
        0 => "scalar",
        1 => "vector2",
        2 => "vector3",
        3 => "vector4",
        4 => "matrix",
        5 => "texture",
        _ => "scalar",
    };

    let tag = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let device_offset = u32_at(bytes, 40) as usize;
    let device_size = u32_at(bytes, 44) as usize;
    let device = bytes
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;
    let preamble_len = programs.first().map_or(device.len(), |program| program.pos);

    let mut out = String::new();
    out.push_str("// Reconstructed from a compiled shader43 section. This is not the\n");
    out.push_str("// original declaration: permutation sets and HLSL are compiled away, and\n");
    out.push_str("// records whose hashes the dictionary cannot name are omitted.\n");
    out.push_str(&format!(
        "// Engine data: {tag}.engine_data ({} groups, hash {:08X}, {} programs, \
         preamble {preamble_len} bytes)\n",
        group.group_count(),
        group.hash(),
        programs.len(),
    ));
    out.push_str(&format!(
        "// Shader source: {tag}.shader_source (the first program of each stage,\n\
         // under the engine's stage guards)\n\n"
    ));

    out.push_str("inputs = {\n");
    if let Some((_, records)) = group
        .object_tables(&query_ids)
        .and_then(|tables| tables.into_iter().next())
    {
        for record in records {
            if let Some(name) = name_of(record.hash) {
                out.push_str(&format!(
                    "    {name} = {{ name = \"{name}\" type = \"{}\" }}\n",
                    type_of(record.kind)
                ));
            }
        }
    }
    out.push_str("}\n\nchannels = {\n");
    for channel in group.channels() {
        if let Some(name) = name_of(channel.hash) {
            let kind = channel.records.first().map_or(0, |record| record.kind);
            out.push_str(&format!(
                "    {name} = {{ type = \"{}\" }}\n",
                type_of(kind)
            ));
        }
    }
    out.push_str("}\n\ncode_blocks = {\n");
    out.push_str(&format!("    {tag} = {{\n    }}\n}}\n\nshader_contexts = {{\n"));
    for context in section.contexts() {
        if let Some(name) = name_of(context.name) {
            out.push_str(&format!("    {name} = {{}}\n"));
        }
    }
    out.push_str("    default = {\n        passes = [\n");
    out.push_str(&format!(
        "            {{ layer=\"default\" code_block=\"{tag}\" render_state=\"default\" }}\n"
    ));
    out.push_str("        ]\n    }\n}\n");

    fs::create_dir_all(dir)?;
    let out_path = dir.join(format!("{tag}.shader_node"));
    fs::write(&out_path, &out)?;

    // The engine data: the parts of the section the generator cannot currently
    // derive, captured from this material. `dtmt build` generates the section
    // from it and the shader source.
    let engine_data = EngineData::from_path(path)?;
    let engine_data_path = dir.join(format!("{tag}.engine_data"));
    fs::write(&engine_data_path, engine_data.to_text())?;

    // The decompiled programs, when the tools are there.
    let source = reconstruct_source(bytes, dxil_spirv, spirv_cross)?;
    let mut wrote_source = None;
    if let Some(body) = source {
        if body.contains("\"\"\"") {
            return Err("the decompiled HLSL contains triple quotes and cannot be embedded".into());
        }
        let source_path = dir.join(format!("{tag}.shader_source"));
        let text = format!(
            "// The decompiled programs of the section, one entry point per stage.\n\n\
             hlsl_shaders = {{\n\t{tag} = {{\n\t\tcode = \"\"\"\n{body}\t\t\"\"\"\n\t}}\n}}\n"
        );
        fs::write(&source_path, text)?;
        wrote_source = Some(source_path);
    }

    // The material SJSON, so the source tree is complete: the shader section is
    // carried by the sibling `.shader_data` file (written by `dtmt build`) and
    // the material points at the engine data. Only a full material data file
    // decompiles; a sliced section has no material template to read.
    let context = sdk::Context::new();
    let mut wrote_material = None;
    match sdk::filetype::material::decompile_data(&context, &data) {
        Ok(material_sjson) => {
            let mut material_out = String::with_capacity(material_sjson.len() + 128);
            material_out
                .push_str("// Reconstructed from a compiled material. The shader section is\n");
            material_out.push_str(&format!(
                "// generated at build time from {tag}.engine_data + {tag}.shader_node + {tag}.shader_source.\n"
            ));
            for line in without_shader_data(&material_sjson).lines() {
                material_out.push_str(line);
                material_out.push('\n');
            }
            material_out.push_str(&format!("shader_engine_data = \"{tag}.engine_data\"\n"));
            let material_path = dir.join(format!("{tag}.material"));
            fs::write(&material_path, material_out)?;
            wrote_material = Some(material_path);
        }
        Err(err) => {
            println!("  no material SJSON written: {err}");
        }
    }

    println!(
        "wrote {} ({} contexts){} + {}{}",
        out_path.display(),
        section.contexts().len(),
        wrote_material
            .as_ref()
            .map(|path| format!(" + {}", path.display()))
            .unwrap_or_default(),
        engine_data_path.display(),
        wrote_source
            .as_ref()
            .map(|path| format!(" + {}", path.display()))
            .unwrap_or_else(|| " + no shader source (decompiler tools not found)".to_string()),
    );
    Ok(())
}

/// Drops a decompiled material's `shader_size`/`shader_data` fields: the
/// reconstructed section is generated from the engine data and shader source
/// instead.
fn without_shader_data(sjson: &str) -> String {
    let mut out = String::with_capacity(sjson.len());
    for line in sjson.lines() {
        let key = line.trim_start();
        if key.starts_with("shader_size") || key.starts_with("shader_data") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The decompiled programs as one `.shader_source` body: the first program of
/// each stage, each under the engine's stage macro. `None` when a decompiler
/// tool cannot be found or the section has no vertex or pixel program.
fn reconstruct_source(
    section: &[u8],
    dxil_spirv: &Path,
    spirv_cross: &Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let device_offset = u32_at(section, 40) as usize;
    let device_size = u32_at(section, 44) as usize;
    let device = section
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;

    let mut vertex = None;
    let mut pixel = None;
    for program in &programs {
        match program.stage {
            shader::Stage::Vertex if vertex.is_none() => {
                match decompile_container(&program.container, program.stage, dxil_spirv, spirv_cross)? {
                    Some(hlsl) => vertex = Some(hlsl),
                    None => return Ok(None),
                }
            }
            shader::Stage::Pixel if pixel.is_none() => {
                match decompile_container(&program.container, program.stage, dxil_spirv, spirv_cross)? {
                    Some(hlsl) => pixel = Some(hlsl),
                    None => return Ok(None),
                }
            }
            _ => {}
        }
        if vertex.is_some() && pixel.is_some() {
            break;
        }
    }

    let mut body = String::new();
    match (&vertex, &pixel) {
        (Some(vs), Some(ps)) => {
            body.push_str("#if defined(STAGE_VERTEX)\n");
            body.push_str(vs);
            body.push_str("\n#elif defined(STAGE_FRAGMENT)\n");
            body.push_str(ps);
            body.push_str("\n#endif\n");
        }
        (Some(vs), None) => {
            body.push_str("#if defined(STAGE_VERTEX)\n");
            body.push_str(vs);
            body.push_str("\n#endif\n");
        }
        (None, Some(ps)) => {
            body.push_str("#if defined(STAGE_FRAGMENT)\n");
            body.push_str(ps);
            body.push_str("\n#endif\n");
        }
        (None, None) => return Ok(None),
    }

    Ok(Some(body))
}

/// Decompiles one container to HLSL with `dxil-spirv` and `spirv-cross`, with
/// the entry point and semantics restored. `None` when a tool is not found;
/// other failures are errors.
fn decompile_container(
    container: &[u8],
    stage: shader::Stage,
    dxil_spirv: &Path,
    spirv_cross: &Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir();
    let stem = format!("dtmt-reconstruct-{}-{stage:?}", std::process::id());
    let container_path = dir.join(format!("{stem}.dxbc"));
    let spv_path = dir.join(format!("{stem}.spv"));
    let hlsl_path = dir.join(format!("{stem}.hlsl"));
    fs::write(&container_path, container)?;

    let output = match Command::new(dxil_spirv)
        .arg(&container_path)
        .arg("--output")
        .arg(&spv_path)
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Failed to run '{}': {err}", dxil_spirv.display()).into()),
    };
    if !output.status.success() {
        return Err(format!(
            "dxil-spirv failed on the {stage:?} program: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    let output = match Command::new(spirv_cross)
        .args(["--hlsl", "--shader-model", "60"])
        .arg(&spv_path)
        .arg("--output")
        .arg(&hlsl_path)
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!("Failed to run '{}': {err}", spirv_cross.display()).into());
        }
    };
    if !output.status.success() {
        return Err(format!(
            "spirv-cross failed on the {stage:?} program: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    let hlsl = fs::read_to_string(&hlsl_path)?;
    let _ = fs::remove_file(&container_path);
    let _ = fs::remove_file(&spv_path);
    let _ = fs::remove_file(&hlsl_path);
    Ok(Some(fix_up_hlsl(&hlsl, stage, container)))
}

/// A dry run: what a generated section *would* contain, and whether it holds
/// together. Reads only - it writes nothing, and touches no game install.
///
/// This is the check that makes a deploy safe. A generated declaration cannot be
/// verified against a shipped one, because a declaration and a section cannot be
/// paired, so the invariants are all there is. `Section::build` enforces the ones
/// that can fail silently - a context pointing at a group or a hash the group data
/// does not have - and refusing here means the same refusal happens before
/// anything is deployed.
fn plan(
    declaration: &Path,
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::shader::Section;

    let text = fs::read_to_string(declaration)?;
    let node = sdk::filetype::shader_node::ShaderNode::from_sjson(&text)?;
    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let template = Section::parse(bytes)?;
    let group_data = GroupData::new(template.group_data().to_vec());
    let groups = group_data.group_count();
    let hash = group_data.hash();
    let named = |hash: u32| match names.and_then(|names| names.get(&hash)) {
        Some(name) => format!("{name}"),
        None => String::new(),
    };

    println!("=== {} + {} ===", declaration.display(), path.display());
    println!(
        "  declared: {} inputs, {} channels, {} permutation sets, {} contexts",
        node.variables.len(),
        node.channels.len(),
        node.permutation_sets.len(),
        node.contexts.len()
    );
    let channels = match node.contexts.first() {
        None => Err(color_eyre::eyre::eyre!("the declaration has no contexts")),
        Some(context) => match node.permutations_for(context).first() {
            None => Err(color_eyre::eyre::eyre!("the context permutes over nothing")),
            Some(permutation) => node.channels_of(permutation),
        },
    };
    let channels = match channels {
        Ok(channels) => channels
            .iter()
            .map(|channel| channel.name.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        Err(err) => format!("unresolved: {err}"),
    };
    println!("  the first group's channels: {channels}");

    let records = node.context_records(hash);
    let carried = sdk::filetype::shader::Carried::of(&template);
    // A generated single-group section writes no conditions tree: its one query
    // selects no conditions. Handing it the template's blob would point at the
    // template's query ids, which is the mismatch Section::check exists to catch.
    match Section::build(&records, Vec::new(), template.group_data().to_vec(), &carried) {
        Err(err) => println!("  refused: {err}"),
        Ok(section) => {
            let bytes = section.into_bytes();
            let back = Section::parse(&bytes).expect("a built section reads back");
            println!(
                "  built: {} contexts against the template's {} groups, {} bytes (the template is {}), {} of programs carried",
                back.contexts().len(),
                groups,
                bytes.len(),
                template.into_bytes().len(),
                back.device_data().len()
            );
            for context in back.contexts() {
                let queries: Vec<String> = context
                    .queries
                    .iter()
                    .map(|query| {
                        let conditions = if query.conditions == sdk::filetype::shader::NO_CONDITIONS
                        {
                            "none".to_string()
                        } else {
                            format!("@{:#x}", query.conditions)
                        };
                        format!("{:08X}->{}", query.id, conditions)
                    })
                    .collect();
                println!(
                    "    context {:08X} {}  {} quer{}",
                    context.name,
                    named(context.name),
                    queries.join(" "),
                    if queries.len() == 1 { "y" } else { "ies" }
                );
            }
            println!("  the invariants hold; nothing was written");
        }
    }
    Ok(())
}

/// Compiles the declaration's programs with DXC: every job's source is the
/// macros, its block's includes and its body; the containers land in `out_dir`.
/// With `against`, each container is also compared with the first program of
/// its stage in that material, so a replacement can be checked before it ships.
fn compile(
    declaration: &Path,
    libraries: &[PathBuf],
    out_dir: &Path,
    against: Option<&Path>,
    core: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;

    use sdk::filetype::shader_compile::{compile as dxc_compile, find_library};
    use sdk::filetype::shader_graph::{Graph, NodeDef};
    use sdk::filetype::shader_node::{STAGES, entry_for, profile_for};

    let text = fs::read_to_string(declaration)?;

    // A material with a shader graph carries the graph that its output node's
    // declaration is compiled with: the declaration comes from the node
    // definitions under `core`, and the evaluation is generated from the graph.
    // A plain `.shader_node` is a declaration without a graph.
    let (node, evaluation) = match Graph::from_material(&text)? {
        Some(graph) => {
            let core = core
                .ok_or("compiling a material needs --core <the folder holding shader_nodes/>")?;
            // A node kind is a resource path: the engine's nodes are under
            // `core/`, while a mod-authored declaration and node live at the
            // path they name under the tree that holds `core/`.
            let node_path = |kind: &str| -> PathBuf {
                let relative = kind.strip_prefix("core/").unwrap_or(kind);
                let under_core = core.join(format!("{relative}.shader_node"));
                if under_core.exists() || kind.starts_with("core/") {
                    return under_core;
                }
                core.parent()
                    .unwrap_or(core)
                    .join(format!("{kind}.shader_node"))
            };
            let mut defs = BTreeMap::new();
            for graph_node in &graph.nodes {
                let path = node_path(&graph_node.kind);
                let def = NodeDef::from_text(&fs::read_to_string(&path)?)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                defs.insert(graph_node.kind.clone(), def);
            }
            let output = graph.output_node().ok_or("the graph has no output node")?;
            let shader_inputs: BTreeMap<String, String> = defs
                .get(&output.kind)
                .map(|def| {
                    def.inputs
                        .iter()
                        .map(|(uuid, input)| (uuid.clone(), input.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let resolution = graph.resolve(&defs, &shader_inputs)?;
            let evaluation = resolution.evaluate(&defs)?;
            let path = node_path(&output.kind);
            println!(
                "  graph: {} nodes, {} channels, {} samplers, {} defines -> \
                 {} bytes of vertex and {} of pixel evaluation",
                graph.nodes.len(),
                evaluation.channels.len(),
                evaluation.samplers.len(),
                evaluation.defines.len(),
                evaluation.vertex.len(),
                evaluation.pixel.len()
            );
            (ShaderNode::from_sjson(&fs::read_to_string(&path)?)?, Some(evaluation))
        }
        None => (ShaderNode::from_sjson(&text)?, None),
    };

    let mut sources = Vec::new();
    for path in libraries {
        if path.is_dir() {
            for entry in walk_shader_sources(path)? {
                sources.push(ShaderSource::from_sjson(&fs::read_to_string(&entry)?)?);
            }
        } else {
            sources.push(ShaderSource::from_sjson(&fs::read_to_string(path)?)?);
        }
    }

    // The programs to compare interfaces against, when one was given.
    let originals = match against {
        Some(path) => {
            let data = fs::read(path)?;
            let section = shader_section(&data)?;
            let device_offset = u32_at(section, 40) as usize;
            let device_size = u32_at(section, 44) as usize;
            let device = section
                .get(device_offset..device_offset + device_size)
                .ok_or("device data is out of range")?;
            Some(shader::parse_programs(device)?)
        }
        None => None,
    };

    let library = find_library(None)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|err| format!("not found ({err})"));
    let jobs = node.compile_jobs()?;
    fs::create_dir_all(out_dir)?;

    println!("=== {} (with {}) ===", declaration.display(), library);
    let mut written = 0usize;
    for job in &jobs {
        for stage in STAGES {
            let Some(profile) = profile_for(stage) else {
                continue;
            };
            let Some(entry) = entry_for(profile) else {
                continue;
            };
            let source = node.job_source(job, stage, &sources, evaluation.as_ref());
            match dxc_compile(&source, profile, entry) {
                Ok(container) => {
                    let name = format!(
                        "{}_p{}_{}.dxbc",
                        sanitize(&job.context),
                        job.permutation,
                        profile
                    );
                    fs::write(out_dir.join(&name), &container)?;
                    println!(
                        "  {} p{} {} {profile}/{entry}: {} bytes -> {name}",
                        job.context,
                        job.permutation,
                        job.code_block,
                        container.len()
                    );
                    written += 1;

                    if let Some(programs) = &originals {
                        let wanted = if stage == "vertex" {
                            shader::Stage::Vertex
                        } else {
                            shader::Stage::Pixel
                        };
                        match programs.iter().find(|program| program.stage == wanted) {
                            Some(original) => {
                                match shader::interface_mismatch(&original.container, &container) {
                                    None => println!(
                                        "    interface matches program {} {:?}",
                                        original.index, original.stage
                                    ),
                                    Some(reason) => println!(
                                        "    interface MISMATCH with program {} {:?}: {reason}",
                                        original.index, original.stage
                                    ),
                                }
                            }
                            None => println!("    no {wanted:?} program to compare against"),
                        }
                    }
                }
                Err(err) => {
                    // Keep the source that failed, so a compile error can be read
                    // against the generated scaffolding and the block's own code.
                    let name = format!(
                        "{}_p{}_{}.failed.hlsl",
                        sanitize(&job.context),
                        job.permutation,
                        profile
                    );
                    let _ = fs::write(out_dir.join(&name), &source);
                    println!(
                        "  {} p{} {} {profile}/{entry}: failed: {err}",
                        job.context, job.permutation, job.code_block
                    );
                }
            }
        }
    }
    println!(
        "  {written} container(s) from {} job(s) into {}",
        jobs.len(),
        out_dir.display()
    );
    Ok(())
}

/// The `.shader_source` files under a directory, in path order.
fn walk_shader_sources(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "shader_source") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// A context name that can be a file name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn layout(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::shader::Section;

    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let named = |hash: u32| match names.and_then(|names| names.get(&hash)) {
        Some(name) => format!("{name}"),
        None => String::new(),
    };
    let section = Section::parse(bytes)?;
    println!("=== {} ===", path.display());
    println!(
        "  {} contexts, {} bytes of conditions, {} dependencies, {} bytes of group data, {} of programs",
        section.contexts().len(),
        section.conditions().len(),
        section.dependencies().len(),
        section.group_data().len(),
        section.device_data().len()
    );
    for (index, context) in section.contexts().iter().enumerate() {
        let queries: Vec<String> = context
            .queries
            .iter()
            .map(|query| {
                let conditions = if query.conditions == sdk::filetype::shader::NO_CONDITIONS {
                    "none".to_string()
                } else {
                    format!("@{:#x}", query.conditions)
                };
                format!("{:08X}->{}", query.id, conditions)
            })
            .collect();
        println!(
            "    context {index}: {:08X} {}  {} quer{}",
            context.name,
            named(context.name),
            queries.join(" "),
            if context.queries.len() == 1 {
                "y"
            } else {
                "ies"
            }
        );
    }
    match section.into_bytes() {
        rebuilt if rebuilt == bytes => println!("  section round trip: identical"),
        rebuilt => {
            let at = rebuilt
                .iter()
                .zip(bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(rebuilt.len().min(bytes.len()));
            println!("  section round trip: differs at byte {at}");
        }
    }
    Ok(())
}

fn channels(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::GroupData;

    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let offset = u32_at(shader, 32) as usize;
    let size = u32_at(shader, 36) as usize;
    let group_data = GroupData::new(
        shader
            .get(offset..offset + size)
            .ok_or("group data is out of range")?
            .to_vec(),
    );

    let channels = group_data.channels();
    println!("=== {} ===", path.display());
    println!(
        "  {} groups, {} channels",
        group_data.group_count(),
        channels.len()
    );
    for channel in &channels {
        let name = names
            .and_then(|names| names.get(&channel.hash))
            .cloned()
            .unwrap_or_default();
        let records: Vec<String> = channel
            .records
            .iter()
            .map(|record| format!("{}@{}", record.kind, record.offset))
            .collect();
        println!(
            "    {:08X} offset {:4}  {:<28} {}",
            channel.hash,
            channel.offset(),
            name,
            records.join(" ")
        );
    }

    // The channel table is the one a from-scratch group data has to write, so
    // rewriting it with the channels it was read as must change nothing.
    let rebuilt = group_data.rebuild_channels(&channels);
    match rebuilt {
        Ok(rebuilt) if rebuilt == group_data.bytes() => {
            println!("  channel round trip: identical");
        }
        Ok(rebuilt) => {
            let at = diffs(group_data.bytes(), &rebuilt)
                .first()
                .map_or(usize::MAX, |(at, _)| *at);
            println!("  channel round trip: differs at +{at}");
        }
        Err(err) => println!("  channel round trip: {err}"),
    }
    Ok(())
}

/// Dumps the conditions region: its records and, whole, their payload words.
fn conditions(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::shader::Section;

    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let section = Section::parse(bytes)?;
    let tree = ConditionTree::parse(section.conditions())?;
    let named = |hash: u32| match names.and_then(|names| names.get(&hash)) {
        Some(name) => format!(" {name}"),
        None => String::new(),
    };
    println!("=== {} ===", path.display());
    println!(
        "  {} bytes of conditions, {} records",
        section.conditions().len(),
        tree.len()
    );
    let mut at = 0;
    for (index, node) in tree.nodes().iter().enumerate() {
        let hashes: Vec<String> = node
            .hashes
            .iter()
            .map(|hash| format!("{hash:08X}{}", named(*hash)))
            .collect();
        let payload: Vec<String> = node
            .payload
            .iter()
            .map(|word| format!("{word:04X}"))
            .collect();
        println!(
            "    {index:2} @{at:4} {} bytes, {} hashes: {}",
            node.len(),
            node.hashes.len(),
            hashes.join(" ")
        );
        println!("         payload: {}", payload.join(" "));
        match node.branches() {
            Some((branches, fallback)) => {
                let branches: Vec<String> = branches
                    .iter()
                    .map(|branch| {
                        let tests: Vec<String> = branch
                            .tests
                            .iter()
                            .map(|index| {
                                node.hashes
                                    .get(*index as usize)
                                    .map(|hash| format!("{hash:08X}"))
                                    .unwrap_or_else(|| format!("#{index}"))
                            })
                            .collect();
                        format!("[{} -> {}]", tests.join(" "), branch.result)
                    })
                    .collect();
                println!(
                    "         branches: {}{}",
                    branches.join(" "),
                    fallback
                        .map(|result| format!(" fallback {result}"))
                        .unwrap_or_default()
                );
            }
            None => println!("         branches: not the mapped opcodes"),
        }
        at += node.len();
    }
    Ok(())
}

/// The comparison the notes call for: each context query's conditions record
/// (its branch results) beside the group's material table length and its three
/// descriptors' `Y` fields. The result values are small, so the question is
/// what they index: the table, the descriptors' usage counts, or something
/// else.
fn conditions_map(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::GroupData;
    use sdk::filetype::shader::{NO_CONDITIONS, Section};

    let data = fs::read(path)?;
    let bytes = shader_section(&data)?;
    let section = Section::parse(bytes)?;
    let tree = ConditionTree::parse(section.conditions())?;
    let group = GroupData::new(section.group_data().to_vec());
    let query_ids: Vec<u32> = section
        .contexts()
        .iter()
        .flat_map(|context| context.queries.iter().map(|query| query.id))
        .collect();
    let tables = group.object_tables(&query_ids).ok_or("the group walk failed")?;
    let starts = group.group_starts(&query_ids).ok_or("the group walk failed")?;
    let name_of = |hash: u32| names.and_then(|names| names.get(&hash)).cloned();

    // The conditions records' offsets, by cumulative record length.
    let mut offsets = Vec::with_capacity(tree.nodes().len());
    let mut at = 0;
    for node in tree.nodes() {
        offsets.push(at);
        at += node.len();
    }

    println!("=== {} ===", path.display());
    let mut index = 0usize;
    for context in section.contexts() {
        println!(
            "context {:08X}{}",
            context.name,
            name_of(context.name)
                .map(|name| format!(" {name}"))
                .unwrap_or_default()
        );
        for query in &context.queries {
            let (table_at, records) = &tables[index];
            let start = starts[index];
            index += 1;

            let record = if query.conditions == NO_CONDITIONS {
                None
            } else {
                offsets
                    .iter()
                    .position(|offset| *offset == query.conditions as usize)
                    .map(|node| &tree.nodes()[node])
            };
            let results: Vec<String> = match record.and_then(|node| node.branches()) {
                Some((branches, fallback)) => {
                    let mut values: Vec<String> = branches
                        .iter()
                        .map(|branch| branch.result.to_string())
                        .collect();
                    if let Some(fallback) = fallback {
                        values.push(format!("fallback {fallback}"));
                    }
                    values
                }
                None => vec!["none".to_string()],
            };
            let y: Vec<u32> = (0..3)
                .map(|descriptor| {
                    // The descriptors sit at the group's +28; the first group's
                    // query id is at +4, so group 0 reads at +32.
                    let offset = start + 28 + descriptor * 16 + 12;
                    if offset + 4 <= group.bytes().len() {
                        u32_at(group.bytes(), offset)
                    } else {
                        0
                    }
                })
                .collect();
            let table: Vec<String> = records
                .iter()
                .map(|record| match name_of(record.hash) {
                    Some(name) => format!("{name}@{}+{}", record.offset, record.size),
                    None => format!("{:08X}@{}+{}", record.hash, record.offset, record.size),
                })
                .collect();
            println!(
                "  group {index:2} query {:08X} record@{:#06x} table@{:#06x} {} records Y {}/{}/{} results {}",
                query.id,
                query.conditions,
                table_at,
                records.len(),
                y[0],
                y[1],
                y[2],
                results.join(",")
            );
            println!("      table: {}", table.join(" "));
        }
    }
    Ok(())
}

fn dependencies(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::{Dependency, GroupData};

    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let at = u32_at(shader, 24) as usize;
    let count = u32_at(shader, 28) as usize;
    let entries = Dependency::read(shader, at, count);

    let offset = u32_at(shader, 32) as usize;
    let size = u32_at(shader, 36) as usize;
    let group_data = GroupData::new(
        shader
            .get(offset..offset + size)
            .ok_or("group data is out of range")?
            .to_vec(),
    );

    println!("=== {} ===", path.display());
    println!(
        "  dependencies @{at} x{count}; group data {size} bytes, {} groups, hash {:08X}",
        group_data.group_count(),
        group_data.hash()
    );
    for (index, entry) in entries.iter().enumerate() {
        let name = names
            .and_then(|names| names.get(&((entry.id >> 32) as u32)))
            .map(String::as_str)
            .unwrap_or(if entry.id == Dependency::RENDERER {
                "core/stingray_renderer/renderer"
            } else {
                "unnamed"
            });
        println!("    {index}: dependency {:016X} ({name})", entry.id);
    }
    Ok(())
}

fn group_data(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::{GroupData, Variable};

    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let offset = u32_at(shader, 32) as usize;
    let size = u32_at(shader, 36) as usize;
    let bytes = shader
        .get(offset..offset + size)
        .ok_or("group data is out of range")?
        .to_vec();
    let group_data = GroupData::new(bytes);

    println!("=== {} ===", path.display());
    println!(
        "group data: {size} bytes, {} groups",
        group_data.group_count()
    );
    if let Some(descriptors) = group_data.descriptors() {
        println!(
            "  descriptors: engine {{name {:08X}, flags {:X}, X {}, Y {}}} texture {{name {:08X}, flags {:X}, X {}, Y {}}} uav {{name {:08X}, flags {:X}, X {}, Y {}}}",
            descriptors.engine.name,
            descriptors.engine.flags,
            descriptors.engine.x,
            descriptors.engine.y,
            descriptors.texture.name,
            descriptors.texture.flags,
            descriptors.texture.x,
            descriptors.texture.y,
            descriptors.uav.name,
            descriptors.uav.flags,
            descriptors.uav.x,
            descriptors.uav.y,
        );
    }

    // Every run of canonical records, and the packed runs.
    let runs = group_data.runs();
    println!("  {} runs of variable records:", runs.len());
    for (index, run) in runs.iter().enumerate() {
        println!("    run {index}: {} records", run.len());
        for record in run.iter().take(6) {
            let name = names
                .and_then(|names| names.get(&record.hash))
                .cloned()
                .unwrap_or_default();
            println!(
                "      type {} {:08X} offset {:5} size {:3} {}",
                record.kind, record.hash, record.offset, record.size, name
            );
        }
        if run.len() > 6 {
            println!("      ... {} more", run.len() - 6);
        }
    }
    for (key, hashes) in group_data.packed_runs() {
        println!("  packed run {:08X}: {} copies", key, hashes.len());
    }

    // The material's own table, and the round trip: rebuilding from the table the
    // section already carries must change nothing.
    match group_data.object_table() {
        None => println!("  no material variable table was found"),
        Some((at, records)) => {
            println!("  material table at +{at}: {} records", records.len());
            let variables: Vec<Variable> = records
                .iter()
                .map(
                    |record| match names.and_then(|names| names.get(&record.hash)) {
                        // A name the dictionary knows: rebuild from the name.
                        Some(name) => Variable::new(name.clone(), record.offset, record.kind),
                        // Otherwise the hash is all there is, and a variable built
                        // from it rewrites to the same bytes.
                        None => Variable::from_hash(record.hash, record.offset, record.kind),
                    },
                )
                .collect();
            match group_data.rebuild(&variables) {
                Ok(rebuilt) if rebuilt == group_data.bytes() => {
                    println!("  round trip: identical");
                }
                Ok(rebuilt) => {
                    let at = rebuilt
                        .iter()
                        .zip(group_data.bytes())
                        .position(|(a, b)| a != b)
                        .unwrap_or(rebuilt.len().min(group_data.bytes().len()));
                    println!(
                        "  round trip: differs at byte {at} ({} vs {} bytes)",
                        rebuilt.len(),
                        group_data.bytes().len()
                    );
                }
                Err(err) => println!("  round trip failed: {err}"),
            }
        }
    }
    Ok(())
}

/// Reads and rewrites every group's byte-packed condition header: a group's last
/// `40 + 17 x n` bytes (a 28-byte packed record, the `n` word, `n` 17-byte
/// entries and an 8-byte trailer). The rebuild is the check - the bytes must
/// come back exactly - and it is run on both the UI base and the small families,
/// whose headers differ in shape.
fn group_conditions(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use sdk::filetype::group_data::GroupData;
    use sdk::filetype::shader::Section;

    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let section = Section::parse(shader)?;
    let query_ids: Vec<u32> = section
        .contexts()
        .iter()
        .flat_map(|context| context.queries.iter().map(|query| query.id))
        .collect();
    let group = GroupData::new(section.group_data().to_vec());
    let headers = group
        .condition_headers(&query_ids)
        .ok_or("a group's condition header was not found")?;

    println!("=== {} ===", path.display());
    println!("  {} groups", headers.len());
    for (index, header) in headers.iter().enumerate() {
        let hashes: Vec<String> = header
            .entries
            .iter()
            .map(|entry| format!("{:08X}", entry.hash))
            .collect();
        let trailer: String = header
            .trailer
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();
        println!(
            "    group {index:2}: n {}, trailer {trailer}, entries {}",
            header.entries.len(),
            hashes.join(" ")
        );
    }

    let rebuilt = group.rebuild_conditions(&query_ids, &headers)?;
    let identical = rebuilt == group.bytes();
    println!(
        "  rebuild: {} bytes, {}",
        rebuilt.len(),
        if identical {
            "byte-identical"
        } else {
            "DIFFERS"
        }
    );
    if !identical {
        return Err("the condition-header rebuild changed bytes".into());
    }
    Ok(())
}

/// Reads a graph material's `shader` block and resolves its wiring against the
/// node definitions under the core folder: every node's inputs (fed by a node,
/// an instance value, a sampler, or nothing) and the graph's outputs - the
/// output node's connectors, named by the shader declaration's input table.
fn graph(path: &Path, core: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;

    use sdk::filetype::shader_graph::{Graph, NodeDef, Source};

    let text = fs::read_to_string(path)?;
    let graph = Graph::from_material(&text)?.ok_or("the material has no shader graph")?;
    let core = core.ok_or("--graph needs --core <the folder holding shader_nodes/>")?;

    // Every node's definition, and the output node's declaration (which is the
    // shader itself, so its inputs table names the graph's outputs).
    let mut defs = BTreeMap::new();
    for node in &graph.nodes {
        let relative = node.kind.strip_prefix("core/").unwrap_or(&node.kind);
        let def_path = core.join(format!("{relative}.shader_node"));
        let def = NodeDef::from_text(&fs::read_to_string(&def_path)?)
            .map_err(|err| format!("{}: {err}", def_path.display()))?;
        defs.insert(node.kind.clone(), def);
    }
    let output = graph.output_node().ok_or("the graph has no output node")?;
    let shader_inputs: BTreeMap<String, String> = defs
        .get(&output.kind)
        .map(|def| {
            def.inputs
                .iter()
                .map(|(uuid, input)| (uuid.clone(), input.name.clone()))
                .collect()
        })
        .unwrap_or_default();

    let resolution = graph.resolve(&defs, &shader_inputs)?;

    println!("=== {} ===", path.display());
    println!(
        "  {} nodes, {} connections, output node '{}'",
        graph.nodes.len(),
        graph.connections.len(),
        output.title
    );
    for node in &resolution.nodes {
        let options = if node.options.is_empty() {
            String::new()
        } else {
            format!(" [{}]", node.options.join(" "))
        };
        println!(
            "    {}'{}' '{}'{}",
            if node.output { "output " } else { "" },
            node.kind,
            node.title,
            options
        );
        for (name, source) in &node.inputs {
            let described = match source {
                Source::Node(id) => format!("<- {id}"),
                Source::Value(value) => format!(
                    "= {}",
                    value.hlsl(false).unwrap_or_else(|| "?".to_string())
                ),
                Source::Sampler(slot) => format!("~ {slot}"),
                Source::Unbound => "-".to_string(),
            };
            println!("        {name} {described}");
        }
    }
    println!("  outputs:");
    for output in &resolution.outputs {
        let described = match &output.source {
            Source::Node(id) => format!("<- {id}"),
            other => format!("{other:?}"),
        };
        println!("    {} [{:?}] {described}", output.name, output.domain);
    }
    Ok(())
}

/// Builds a block from a declaration for a section, using the section's own preamble as
/// the engine template, and reports the round trip: a declaration naming exactly
/// the template's channels must rebuild the template byte for byte.
fn build_block(
    path: &Path,
    declaration_path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;
    let first = programs.first().ok_or("no programs")?.pos;
    let preamble = device.get(..first).ok_or("preamble is out of range")?;
    let template = BlockTemplate::from_preamble(preamble)?;

    let node = ShaderNode::from_sjson(&fs::read_to_string(declaration_path)?)?;

    println!("=== {} ===", path.display());
    println!(
        "template: {} groups, {} cbuffers, {} engine records, {} bytes",
        template.groups(),
        template.cbuffers(),
        template.records().len(),
        preamble.len()
    );
    for (index, value) in template.records() {
        println!("  record {index} = {value}");
    }
    for hash in template.channel_names() {
        match names.and_then(|names| names.get(&hash)) {
            Some(name) => println!("  channel {hash:08X} {name}"),
            None => println!("  channel {hash:08X}"),
        }
    }

    println!(
        "declaration: {} groups from {} permutation sets, {} gated variables, \
         {} channels",
        node.group_count(),
        node.permutation_sets.len(),
        node.flags().len(),
        node.channels.len()
    );
    for set in &node.permutation_sets {
        println!("  set {}: {} choices", set.name, set.choices.len());
        for (index, choice) in set.choices.iter().enumerate() {
            println!(
                "    {index}: if [{}] macros [{}] stages [{}] default {}",
                choice.condition.clone().unwrap_or_default(),
                choice.macros.join(" "),
                choice.stages.join(" "),
                choice.is_default,
            );
        }
    }
    for (index, permutation) in node.permutations().iter().enumerate() {
        let channels = node.channel_names_of(permutation)?;
        println!(
            "  group {index:02}: macros [{}] channels [{}]",
            permutation.macros.join(" "),
            channels.join(" "),
        );
    }
    // The interface a material gets when it declares every gated variable: the
    // most a material can ask for.
    let inputs: Vec<String> = node
        .variables
        .iter()
        .filter(|(_, variable)| variable.flag.is_some())
        .map(|(name, _)| name.clone())
        .collect();
    let interface = node.interface(&inputs);
    println!(
        "  interface of every gated input: mask {:02} flags [{}] variables [{}] \
         channels [{}]",
        interface.mask,
        interface.flags.join(" "),
        interface.variables.join(" "),
        interface.channels.join(" "),
    );

    // The contexts: what each one compiles, and the passes it draws. The
    // interface's flags stand in for the defines, since a pass condition reads
    // the same input flags.
    for context in &node.contexts {
        let defines = Defines::new(interface.flags.iter().cloned());
        let passes = context.passes_of(&defines)?;
        println!(
            "  context {}: sort {} permutes {} sets, {} passes",
            context.name,
            context.sort_mode.clone().unwrap_or_default(),
            node.permutations_for(context).len(),
            passes.len(),
        );
        for entry in &context.compile_with {
            println!(
                "    compiles if [{}] over [{}]",
                entry.condition.clone().unwrap_or_default(),
                entry.permute_with.join(" ")
            );
        }
        for pass in passes {
            println!(
                "    pass layer [{}] block {} macros [{}] state [{}]",
                pass.layer.clone().unwrap_or_default(),
                pass.code_block,
                pass.macros().join(" "),
                pass.render_state.clone().unwrap_or_default(),
            );
        }
    }
    println!(
        "  groups: {} over the declaration's sets, {} over its contexts",
        node.group_count(),
        node.context_group_count(),
    );

    let channels: Vec<(String, ChannelDef)> = node
        .channels
        .iter()
        .map(|channel| (channel.name.clone(), channel.clone()))
        .collect();
    let cbuffers = node.programs.len() as u32;
    let block = shader_block::build_block(
        &template,
        &channels,
        node.group_count() as u32,
        cbuffers.max(1),
    )?;
    println!(
        "built: {} bytes, {} channels, header says {} groups / {} cbuffers / {} records",
        block.len(),
        channels.len(),
        u32_at(&block, 4),
        u32_at(&block, 8),
        u32_at(&block, 12),
    );

    // The round trip. The channel stream is keyed by name, so the declaration
    // is emitted in the template's own channel order to make the comparison
    // exact; a name the template lacks goes last and shows up as a difference.
    let declared: Vec<(u32, String, ChannelDef)> = channels
        .iter()
        .map(|(name, def)| (hash_name(name), name.clone(), def.clone()))
        .collect();
    let template_channels = template.channel_names();
    let mut ordered: Vec<(String, ChannelDef)> = Vec::new();
    for hash in &template_channels {
        if let Some((_, name, def)) = declared.iter().find(|(h, _, _)| h == hash) {
            ordered.push((name.clone(), def.clone()));
        }
    }
    for (hash, name, def) in &declared {
        if !template_channels.contains(hash) {
            ordered.push((name.clone(), def.clone()));
        }
    }
    let missing: Vec<String> = template_channels
        .iter()
        .filter(|hash| !declared.iter().any(|(h, _, _)| h == *hash))
        .map(|hash| match names.and_then(|names| names.get(hash)) {
            Some(name) => format!("{name} ({hash:08X})"),
            None => format!("{hash:08X}"),
        })
        .collect();
    println!(
        "declaration covers {}/{} of the template's channels; not declared: {}",
        template_channels.len() - missing.len(),
        template_channels.len(),
        if missing.is_empty() {
            "none".to_string()
        } else {
            missing.join(", ")
        }
    );

    let rebuilt =
        shader_block::build_block(&template, &ordered, template.groups(), template.cbuffers())?;
    match rebuilt == preamble {
        true => println!("round trip: identical to the template preamble"),
        false => {
            let at = rebuilt
                .iter()
                .zip(preamble)
                .position(|(a, b)| a != b)
                .unwrap_or(rebuilt.len().min(preamble.len()));
            println!(
                "round trip: differs at byte {at} ({} vs {} bytes)",
                rebuilt.len(),
                preamble.len()
            );
        }
    }
    Ok(())
}

/// Dumps the packed table the device data starts with, before the first program
/// record: its size, the program positions, and the raw bytes.
/// Dumps the channel records of the device preamble: the chain of fixed-length
/// records at the end of the preamble, with their names resolved through the
/// dictionary when one is given.
fn records(
    path: &Path,
    names: Option<&HashMap<u32, String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;

    let programs = shader::parse_programs(device)?;
    let first = programs.first().ok_or("no programs")?.pos;
    let preamble = device.get(..first).ok_or("preamble is out of range")?;

    println!(
        "=== {} preamble {} bytes ===",
        path.display(),
        preamble.len()
    );

    // Find the chain of records that consumes the preamble exactly.
    let mut found = None;
    for start in 0..preamble.len() {
        let mut at = start;
        let mut records = Vec::new();
        while at < preamble.len() {
            if at + 12 > preamble.len() {
                break;
            }
            let hash = u32_at(preamble, at);
            let kind = u32_at(preamble, at + 4);
            let count = u32_at(preamble, at + 8);
            if count != 1 {
                break;
            }
            let Some(len) = channel_record_len(kind) else {
                break;
            };
            if at + len > preamble.len() {
                break;
            }
            records.push((at, hash, kind, len));
            at += len;
        }
        if at == preamble.len() && !records.is_empty() {
            found = Some(records);
            break;
        }
    }

    let Some(records) = found else {
        println!("  no record stream found");
        return Ok(());
    };
    println!("  {} records", records.len());
    for (at, hash, kind, len) in records {
        let name = names
            .and_then(|names| names.get(&hash))
            .cloned()
            .unwrap_or_else(|| format!("#{hash:08X}"));
        println!("  +{at:#06x}  kind {kind}  {len:>2} bytes  {name}");
    }
    Ok(())
}

/// Resolves a name or 8 digit hex hash to a 32 bit hash.
fn hash_name(token: &str) -> u32 {
    let trimmed = token.strip_prefix('#').unwrap_or(token);
    if trimmed.len() == 8 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(trimmed, 16).unwrap_or(0)
    } else {
        u32::from(murmur::Murmur32::hash(token))
    }
}

/// Dumps the group data records of a channel, classified by framing: canonical
/// 20-byte records (a small type word before the hash) and packed copies (a
/// cbuffer hash before the hash).
fn channel_records(path: &Path, hash: u32) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;
    let group_offset = u32_at(shader, 32) as usize;
    let group_size = u32_at(shader, 36) as usize;
    let group = shader
        .get(group_offset..group_offset + group_size)
        .ok_or("group data is out of range")?;

    let needle = hash.to_le_bytes();
    let mut canonical = Vec::new();
    let mut packed = Vec::new();
    for at in 8..group.len().saturating_sub(4) {
        if group[at..at + 4] != needle {
            continue;
        }
        let before = u32_at(group, at - 8);
        if before <= 12 {
            canonical.push((
                at,
                before,
                u32_at(group, at - 4),
                u32_at(group, at + 4),
                u32_at(group, at + 8),
            ));
        } else {
            packed.push((
                at,
                before,
                u32_at(group, at - 4),
                u32_at(group, at + 4),
                u32_at(group, at + 8),
                u32_at(group, at + 12),
            ));
        }
    }

    println!(
        "=== {} group data {} bytes, #{hash:08X} ===",
        path.display(),
        group.len()
    );
    println!("  canonical {} records", canonical.len());
    for (at, kind, flags, offset, size) in &canonical {
        println!("    +{at:#06x}  type {kind}  flags {flags}  offset {offset}  size {size}");
    }
    println!("  packed {} records", packed.len());
    for (at, cbuffer, zero, a, b, c) in &packed {
        println!("    +{at:#06x}  cbuffer #{cbuffer:08X}  {zero} {a} {b} {c}");
    }
    Ok(())
}

fn dump_preamble(path: &Path, dump_dir: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;

    let first = programs.first().map(|program| program.pos).unwrap_or(0);
    let preamble = device.get(..first).ok_or("preamble is out of range")?;

    println!(
        "=== {} ===\n  {} program(s), first at {:#x}, preamble {} bytes",
        path.display(),
        programs.len(),
        first,
        preamble.len()
    );

    let positions = programs
        .iter()
        .take(12)
        .map(|program| format!("{}:{:?}@{:#x}", program.index, program.stage, program.pos))
        .collect::<Vec<_>>()
        .join(" ");
    println!("  first programs: {positions}");

    // The block's header: group count, cbuffer count and record count + 8; the
    // byte-packed 13-byte records follow at +0x78 and end at the stream's count
    // word (see `docs/Shader RE TODO.md`).
    if preamble.len() >= 0x78 {
        let groups = u32_at(preamble, 4);
        let cbuffers = u32_at(preamble, 8);
        let records = u32_at(preamble, 12).saturating_sub(8) as usize;
        let table_end = 0x78 + records * 13;
        let stream = preamble
            .get(table_end..table_end + 4)
            .map(|bytes| u32_at(bytes, 0));
        println!(
            "  block: {groups} group(s), {cbuffers} cbuffer(s), {records} record(s), \
             table +0x78..+{table_end:#x}, stream count {}",
            stream
                .map(|count| count.to_string())
                .unwrap_or_else(|| "?".into())
        );
    }

    for (row, chunk) in preamble.chunks(16).enumerate() {
        let hex = chunk
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|byte| {
                if (0x20..0x7F).contains(byte) {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  +{:#06x}: {hex:<47} {ascii}", row * 16);
    }

    if let Some(dir) = dump_dir {
        fs::create_dir_all(dir)?;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let out = dir.join(format!("{stem}_preamble.bin"));
        fs::write(&out, preamble)?;
        println!("  wrote {}", out.display());
    }

    Ok(())
}

/// Hex-dumps one section of a shader43 section: `header`, `contexts`,
/// `conditions`, `dependencies`, `group`, `device` or `default`. The sections
/// are located by the offsets in the header, so a section's end is the next
/// section's start.
fn dump_section(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let contexts = u32_at(shader, 8) as usize;
    let conditions = u32_at(shader, 16) as usize;
    let dependencies = u32_at(shader, 24) as usize;
    let group = u32_at(shader, 32) as usize;
    let group_size = u32_at(shader, 36) as usize;
    let device = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let default = u32_at(shader, 20) as usize;

    if name == "header" {
        dump_bytes(shader, 0, contexts, path, "header");
        return Ok(());
    }

    let sections: [(&str, usize, usize); 6] = [
        ("contexts", contexts, conditions),
        ("conditions", conditions, dependencies),
        ("dependencies", dependencies, group),
        ("group", group, group + group_size),
        ("device", device, device + device_size),
        ("default", default, shader.len()),
    ];

    let Some((section, start, end)) = sections.iter().find(|(section, ..)| *section == name) else {
        return Err(format!(
            "unknown section '{name}' (header, contexts, conditions, dependencies, group, \
             device, default)"
        )
        .into());
    };

    dump_bytes(shader, *start, *end, path, section);
    Ok(())
}

fn dump_bytes(shader: &[u8], start: usize, end: usize, path: &Path, name: &str) {
    let end = end.min(shader.len());
    let bytes = &shader[start.min(end)..end];

    println!(
        "=== {} {} ({} bytes at {:#x}) ===",
        path.display(),
        name,
        bytes.len(),
        start
    );

    for (row, chunk) in bytes.chunks(16).enumerate() {
        let hex = chunk
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|byte| {
                if (0x20..0x7F).contains(byte) {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  +{:#06x}: {hex:<47} {ascii}", row * 16);
    }
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

/// One cbuffer of a program, as listed in its metadata tail: 24 byte entries
/// whose `{name_hash, size}` sit at `+4`/`+12`, listing the cbuffers in
/// register order.
#[derive(Clone, Debug)]
struct TailCbuffer {
    name: Option<String>,
    size: u32,
}

fn tail_cbuffers(tail: &[u8], names: &HashMap<u32, String>) -> Vec<TailCbuffer> {
    let mut cbuffers = Vec::new();
    let mut at = 0;

    while at + 24 <= tail.len() {
        let hash = u32_at(tail, at + 4);
        let size = u32_at(tail, at + 12);

        if hash == 0 || size == 0 || size >= 8192 || size % 16 != 0 {
            break;
        }
        let Some(..) = names.get(&hash) else {
            break;
        };

        cbuffers.push(TailCbuffer {
            name: names.get(&hash).cloned(),
            size,
        });
        at += 24;
    }

    cbuffers
}

/// Reads a 20 byte group data variable record, if the bytes look like one.
/// Returns `(kind, flags, name_hash, cbuffer_offset, size)`.
fn read_variable(group: &[u8], at: usize) -> Option<(u32, u32, u32, u32, u32)> {
    const SIZES: [(u32, u32); 5] = [(0, 4), (1, 8), (2, 12), (3, 16), (4, 64)];

    if at + 20 > group.len() {
        return None;
    }

    let kind = u32_at(group, at);
    let flags = u32_at(group, at + 4);
    let hash = u32_at(group, at + 8);
    let offset = u32_at(group, at + 12);
    let size = u32_at(group, at + 16);

    if kind > 12 || flags > 3 || offset > 4096 {
        return None;
    }
    if let Some(&(.., expected)) = SIZES.iter().find(|(code, ..)| *code == kind)
        && size != expected
    {
        return None;
    }

    Some((kind, flags, hash, offset, size))
}

/// Returns the variable tables of a group data blob: runs of valid records
/// whose preceding word is their record count.
fn variable_tables(group: &[u8]) -> Vec<Vec<(u32, u32, u32, u32, u32)>> {
    let mut tables = Vec::new();
    let mut at = 0;

    while at + 20 <= group.len() {
        let mut count = 0;
        while read_variable(group, at + count * 20).is_some() {
            count += 1;
        }

        if count >= 3 && at >= 4 && u32_at(group, at - 4) as usize == count {
            tables.push(
                (0..count)
                    .filter_map(|index| read_variable(group, at + index * 20))
                    .collect(),
            );
        }

        at += 1;
    }

    tables
}

/// One cbuffer declaration of a decompiled program's HLSL.
struct HlslCbuffer {
    register: u32,
    array: String,
    float4_count: u32,
}

/// Parses `cbuffer _x : register(bN, ...) { float4 _x_m0[NN] ... }` blocks from
/// decompiled HLSL, which is where the decompiler lands the engine's cbuffers.
fn hlsl_cbuffers(hlsl: &str) -> Vec<HlslCbuffer> {
    let mut cbuffers = Vec::new();

    for index in 0..hlsl.matches("register(b").count() {
        let at = hlsl
            .match_indices("register(b")
            .nth(index)
            .map(|(position, ..)| position)
            .expect("index out of range");

        let after = &hlsl[at + "register(b".len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        let register: u32 = digits.parse().unwrap_or(0);

        // The engine lands the cbuffer arrays as `float4 <name>_m0[NN]` a few
        // lines after the register.
        let block = after.find('}').map(|end| &after[..end]).unwrap_or(after);
        let Some(array_start) = block.find("_m0[") else {
            continue;
        };
        let after_array = &block[array_start + 4..];
        let count: String = after_array
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let name_start = block[..array_start]
            .rfind(|next: char| next.is_whitespace())
            .map_or(0, |position| position + 1);
        let array = &block[name_start..array_start + 3];

        cbuffers.push(HlslCbuffer {
            register,
            array: array.to_string(),
            float4_count: count.parse().unwrap_or(0),
        });
    }

    cbuffers
}

/// Prints which decompiled array and slot a shader's variables land in.
///
/// A program's tail lists its cbuffers in register order as
/// `{name_hash, flag, size, ?, ?}` entries; the group data's variable tables
/// are matched to them by size, and HLSL files from `--decompile` (given with
/// `--hlsl <dir>`) supply the local array names, like `_25_m0[14]`.
fn slots(
    path: &Path,
    names: &HashMap<u32, String>,
    hlsl_dir: Option<&Path>,
    program_filter: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    const VARIABLE_KINDS: [(u32, &str); 6] = [
        (0, "float"),
        (1, "float2"),
        (2, "float3"),
        (3, "float4"),
        (4, "float4x4"),
        (5, "uint"),
    ];

    let data = fs::read(path)?;
    let shader = shader_section(&data)?;

    let group_offset = u32_at(shader, 32) as usize;
    let group_size = u32_at(shader, 36) as usize;
    let group = shader
        .get(group_offset..group_offset + group_size)
        .ok_or("group data is out of range")?;
    let tables = variable_tables(group);

    let device_offset = u32_at(shader, 40) as usize;
    let device_size = u32_at(shader, 44) as usize;
    let device = shader
        .get(device_offset..device_offset + device_size)
        .ok_or("device data is out of range")?;
    let programs = shader::parse_programs(device)?;

    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let decompiled: HashMap<usize, String> = match hlsl_dir {
        Some(dir) => (0..programs.len())
            .filter_map(|index| {
                fs::read_to_string(dir.join(format!("{stem}_p{index:02}.hlsl")))
                    .ok()
                    .map(|text| (index, text))
            })
            .collect(),
        None => HashMap::new(),
    };

    let selected: Vec<&shader::Program> = match program_filter {
        Some(index) => vec![
            programs
                .get(index)
                .ok_or_else(|| format!("no program {index} (of {})", programs.len()))?,
        ],
        None => {
            let mut picked: Vec<&shader::Program> = Vec::new();
            for program in &programs {
                if !picked.iter().any(|picked| picked.stage == program.stage) {
                    picked.push(program);
                }
            }
            picked
        }
    };

    // Dedupe table copies by size and first/last record hashes.
    let mut printed = std::collections::HashSet::new();

    for program in &selected {
        let tail_start = program.meta_pos + 16;
        let tail_end = programs
            .iter()
            .find(|next| next.pos > tail_start)
            .map(|next| next.pos)
            .unwrap_or_else(|| device.len());
        let tail = &device[tail_start..tail_end];
        let cbuffers = tail_cbuffers(tail, names);

        println!(
            "=== {} program {} ({:?}) ===",
            path.display(),
            program.index,
            program.stage
        );
        if cbuffers.is_empty() {
            println!("  (the tail's cbuffer entries could not be read)");
        }
        for (index, cbuffer) in cbuffers.iter().enumerate() {
            let name = cbuffer.name.as_deref().unwrap_or("(unknown hash)");
            println!("  b{index} {name} ({} bytes)", cbuffer.size);
        }

        for table in &tables {
            let extent = table
                .iter()
                .map(|(.., offset, size)| *offset + *size)
                .max()
                .unwrap_or(0);
            let key = format!(
                "{}:{}:{}:{}",
                table.len(),
                extent,
                table.first().map(|record| record.2).unwrap_or(0),
                table.last().map(|record| record.2).unwrap_or(0)
            );
            if !printed.insert(key) {
                continue;
            }

            println!("  table of {} records, offsets 0..{extent}:", table.len());

            // The smallest tail cbuffer the table fits into is its likely owner.
            let owner = cbuffers
                .iter()
                .enumerate()
                .filter(|(.., cbuffer)| cbuffer.size >= extent && extent > 0)
                .min_by_key(|(index, cbuffer)| (cbuffer.size, *index as u32))
                .map(|(index, ..)| index);

            let declarations = decompiled
                .get(&program.index)
                .map(|text| hlsl_cbuffers(text))
                .unwrap_or_default();

            for (kind, hash, offset, size) in table
                .iter()
                .copied()
                .map(|(kind, _flags, hash, offset, size)| (kind, hash, offset, size))
            {
                let name = names
                    .get(&hash)
                    .cloned()
                    .unwrap_or_else(|| format!("hash_0x{hash:08X}"));
                let kind_name = VARIABLE_KINDS
                    .iter()
                    .find(|(code, ..)| *code == kind)
                    .map_or("?", |(.., name)| *name);

                let target = match owner {
                    Some(index) => declarations
                        .iter()
                        .find(|declaration| declaration.register as usize == index)
                        .map(|declaration| {
                            let slot = offset / 16;
                            let component = (offset % 16) / 4;
                            match component {
                                0 => format!("{}[{slot}]", declaration.array),
                                _ => format!(
                                    "{}[{slot}].{}",
                                    declaration.array,
                                    ['x', 'y', 'z', 'w'][component as usize]
                                ),
                            }
                        })
                        .unwrap_or_else(|| format!("b{index}[{}]", offset / 16)),
                    None => format!("@{offset}"),
                };

                println!(
                    "    {name:<32} {kind_name:<8} @{offset:<5} ({size:>3} bytes) -> {target}"
                );
            }
        }
    }

    Ok(())
}
