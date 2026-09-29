//! Walks the game's data directory and dumps what the section-generation work
//! still needs many samples of: every material's contexts, its conditions
//! records with their payloads, and its distinct device preambles and tail
//! blocks.
//!
//! ```text
//! mine_sections [--out <dir>] <file or directory>...
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use sdk::filetype::condition_tree::ConditionTree;
use sdk::filetype::shader::{self, Section, Tail, TailLists};
use sdk::murmur::Murmur32;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// The material stream header (version 60/61/62) points at the shader section.
fn shader_section(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 28 {
        return None;
    }
    let version = u32_at(data, 0);
    // The stream version moved from 60/61 to 62 in the September 2026 build.
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

/// Walks `path` and calls `visit` for every file, without collecting them all
/// first (the game's data directory has hundreds of thousands of files).
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

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

fn words32(values: &[u32]) -> String {
    values
        .iter()
        .map(|value| format!("{value:08X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn words16(values: &[u16]) -> String {
    values
        .iter()
        .map(|value| format!("{value:04X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn row(out: &mut impl Write, fields: &[String]) -> std::io::Result<()> {
    let mut line = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        if field.contains(',') || field.contains('"') {
            line.push('"');
            line.push_str(&field.replace('"', "\"\""));
            line.push('"');
        } else {
            line.push_str(field);
        }
    }
    line.push('\n');
    out.write_all(line.as_bytes())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out_dir = PathBuf::from(".");
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).expect("--out needs a directory"));
            }
            other => paths.push(PathBuf::from(other)),
        }
        i += 1;
    }
    if paths.is_empty() {
        eprintln!("usage: mine_sections [--out <dir>] <file or directory>...");
        std::process::exit(1);
    }
    fs::create_dir_all(&out_dir)?;

    let mut contexts = BufWriter::new(File::create(out_dir.join("section-contexts.csv"))?);
    let mut conditions = BufWriter::new(File::create(out_dir.join("section-conditions.csv"))?);
    let mut preambles = BufWriter::new(File::create(out_dir.join("section-preambles.csv"))?);
    let mut blocks = BufWriter::new(File::create(out_dir.join("section-blocks.csv"))?);
    let mut program_blocks =
        BufWriter::new(File::create(out_dir.join("section-program-blocks.csv"))?);

    row(
        &mut contexts,
        &[
            "file".into(),
            "context".into(),
            "name".into(),
            "flags".into(),
            "queries".into(),
        ],
    )?;
    row(
        &mut conditions,
        &[
            "file".into(),
            "record".into(),
            "offset".into(),
            "hashes".into(),
            "payload".into(),
        ],
    )?;
    row(
        &mut preambles,
        &["file".into(), "length".into(), "hash".into(), "hex".into()],
    )?;
    row(
        &mut blocks,
        &["file".into(), "hash".into(), "length".into(), "hex".into()],
    )?;
    row(
        &mut program_blocks,
        &[
            "file".into(),
            "program".into(),
            "stage".into(),
            "block_hash".into(),
        ],
    )?;

    let mut files = 0usize;
    let mut sections = 0usize;
    for path in &paths {
        walk(path, &mut |file| {
            files += 1;
            match process(
                file,
                &mut contexts,
                &mut conditions,
                &mut preambles,
                &mut blocks,
                &mut program_blocks,
            ) {
                Ok(true) => sections += 1,
                Ok(false) => {}
                Err(err) => eprintln!("{}: {err}", file.display()),
            }
            if files % 2000 == 0 {
                eprintln!("{files} files read, {sections} section(s) so far");
                let _ = contexts.flush();
                let _ = conditions.flush();
                let _ = preambles.flush();
                let _ = blocks.flush();
                let _ = program_blocks.flush();
            }
        });
    }

    contexts.flush()?;
    conditions.flush()?;
    preambles.flush()?;
    blocks.flush()?;
    program_blocks.flush()?;

    println!(
        "wrote {sections} section(s) from {files} file(s) to {}",
        out_dir.display()
    );
    Ok(())
}

/// Dumps one material data file's section parts. Returns whether the file was
/// a material with a shader section.
fn process(
    file: &Path,
    contexts: &mut impl Write,
    conditions: &mut impl Write,
    preambles: &mut impl Write,
    blocks: &mut impl Write,
    program_blocks: &mut impl Write,
) -> Result<bool, Box<dyn Error>> {
    let data = fs::read(file)?;
    let Some(shader) = shader_section(&data) else {
        return Ok(false);
    };
    let section = Section::parse(shader)?;
    let path = file.display().to_string();

    for (index, context) in section.contexts().iter().enumerate() {
        let queries = context
            .queries
            .iter()
            .map(|query| format!("{:08X}:{}", query.id, query.conditions))
            .collect::<Vec<_>>()
            .join(" ");
        row(
            contexts,
            &[
                path.clone(),
                index.to_string(),
                format!("{:08X}", context.name),
                format!("{:08X}", context.flags),
                queries,
            ],
        )?;
    }

    let tree = ConditionTree::parse(section.conditions())?;
    let mut offset = 0usize;
    for (index, node) in tree.nodes().iter().enumerate() {
        row(
            conditions,
            &[
                path.clone(),
                index.to_string(),
                offset.to_string(),
                words32(&node.hashes),
                words16(&node.payload),
            ],
        )?;
        offset += node.len();
    }

    let device = section.device_data();
    let programs = shader::parse_programs(device)?;
    if let Some(first) = programs.first() {
        let preamble = &device[..first.pos];
        row(
            preambles,
            &[
                path.clone(),
                preamble.len().to_string(),
                format!("{:08X}", u32::from(Murmur32::hash(preamble))),
                hex(preamble),
            ],
        )?;
    }

    let mut seen: BTreeSet<u32> = BTreeSet::new();
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
        let Some(lists) = TailLists::parse(&parsed.rest) else {
            continue;
        };
        let hash = u32::from(Murmur32::hash(&lists.block));
        if seen.insert(hash) {
            row(
                blocks,
                &[
                    path.clone(),
                    format!("{hash:08X}"),
                    lists.block.len().to_string(),
                    hex(&lists.block),
                ],
            )?;
        }
        row(
            program_blocks,
            &[
                path.clone(),
                index.to_string(),
                format!("{:?}", program.stage),
                format!("{hash:08X}"),
            ],
        )?;
    }

    Ok(true)
}
