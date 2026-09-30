//! Builds a tail's constant-buffer list from the sources and compares it with
//! the shipped tail, and prints the per-program block masks (the bytes a block
//! patches into the preamble body) for the mask investigation.
//!
//! The built list is: for the stage's own `cbuffer NAME : register(bN, spaceM)`
//! declarations in source order - the name's murmur32, its index in the group's
//! descriptor list, its size (the engine table's records for the engine's
//! cbuffer, the group's material table for the material's, both rounded up to
//! 16), the register, and the constant 1 and 0 words.
//!
//! ```text
//! tail_build <material data file | .raw section> <shader source file>
//! ```

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::{self, Section, Stage, Tail};
use sdk::murmur::Murmur32;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn section_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() >= 28 && (60..=62).contains(&u32_at(data, 0)) && u32_at(data, 4) == 28 {
        let offset = u32_at(data, 12) as usize;
        let size = u32_at(data, 16) as usize;
        return data.get(offset..offset + size);
    }
    data.get(20..)
}

/// One `cbuffer NAME : register(bN, ...)` declaration of a stage.
struct Decl {
    name: String,
    register: u32,
}

/// The `cbuffer` declarations of each stage, told apart by the
/// `STAGE_VERTEX`/`STAGE_FRAGMENT` guards the sources use.
fn declarations(source: &str) -> HashMap<Stage, Vec<Decl>> {
    let mut out: HashMap<Stage, Vec<Decl>> = HashMap::new();
    let mut stage = Stage::Vertex;
    for line in source.lines() {
        let line = line.trim();
        if line.contains("STAGE_VERTEX") {
            stage = Stage::Vertex;
        } else if line.contains("STAGE_FRAGMENT") {
            stage = Stage::Pixel;
        }
        let Some(rest) = line.strip_prefix("cbuffer ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(" : register(") else {
            continue;
        };
        let register = rest
            .trim_start_matches('b')
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|digits| digits.parse::<u32>().ok())
            .unwrap_or(0);
        out.entry(stage).or_default().push(Decl {
            name: name.trim().to_string(),
            register,
        });
    }
    out
}

/// A record table's size: the largest `offset + size` rounded up to 16.
fn table_size(records: &[sdk::filetype::group_data::Record]) -> u32 {
    let end = records
        .iter()
        .map(|record| record.offset + record.size)
        .max()
        .unwrap_or(0);
    end.div_ceil(16) * 16
}

fn main() -> Result<(), Box<dyn Error>> {
    let data_path = std::env::args().nth(1).ok_or("usage: tail_build <data> <source>")?;
    let source_path = std::env::args().nth(2).ok_or("usage: tail_build <data> <source>")?;
    let data = fs::read(&data_path)?;
    let bytes = section_bytes(&data).ok_or("not a section")?;
    let section = Section::parse(bytes)?;
    let source = fs::read_to_string(&source_path)?;
    let declarations = declarations(&source);

    let query_ids: Vec<u32> = section
        .contexts()
        .iter()
        .flat_map(|context| context.queries.iter().map(|query| query.id))
        .collect();
    let group = GroupData::new(section.group_data().to_vec());
    let starts = group.group_starts(&query_ids).ok_or("the groups do not walk")?;

    // The first group's descriptor list (name -> index); the material's
    // cbuffer is its first descriptor, the engine's is the one `descriptors`
    // reports as the engine's.
    let start = starts[0];
    let count = u32_at(group.bytes(), start + 8) as usize;
    let mut descriptors: BTreeMap<u32, u32> = BTreeMap::new();
    for index in 0..count {
        descriptors.insert(u32_at(group.bytes(), start + 12 + index * 16), index as u32);
    }
    let material_cbuffer = u32_at(group.bytes(), start + 12);
    let material_size = group
        .object_tables(&query_ids)
        .and_then(|tables| tables.into_iter().next())
        .map(|(_, records)| table_size(&records))
        .unwrap_or(0);
    let engine_cbuffer = group
        .descriptors()
        .map(|descriptors| descriptors.engine.name)
        .unwrap_or(0);
    let engine_size = group
        .engine_records()
        .map(|records| table_size(&records))
        .unwrap_or(0);

    let expected_for = |stage: Stage| -> Vec<[u32; 6]> {
        declarations
            .get(&stage)
            .map(|decls| {
                decls
                    .iter()
                    .map(|decl| {
                        let hash = u32::from(Murmur32::hash(decl.name.as_bytes()));
                        let index = descriptors.get(&hash).copied().unwrap_or(u32::MAX);
                        let size = if hash == engine_cbuffer {
                            engine_size
                        } else if hash == material_cbuffer {
                            material_size
                        } else {
                            0
                        };
                        [hash, index, size, decl.register, 1, 0]
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let device = section.device_data();
    let programs = shader::parse_programs(device)?;
    // The preamble's body: what a block is a diff against.
    let preamble_end = programs.first().map(|program| program.pos).unwrap_or(0);
    let preamble = device.get(..preamble_end).unwrap_or_default();
    let body = preamble.get(12..).unwrap_or_default();
    let mut checked = 0usize;
    let mut matched = 0usize;
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut masks: Vec<(usize, Stage, u8)> = Vec::new();
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
        let expected = expected_for(program.stage);
        if parsed.cbuffers.len() != expected.len() {
            *failures
                .entry(format!(
                    "program {index} ({:?}): {} cbuffers shipped vs {} built",
                    program.stage,
                    parsed.cbuffers.len(),
                    expected.len()
                ))
                .or_default() += 1;
        }
        for (actual, wanted) in parsed.cbuffers.iter().zip(&expected) {
            checked += 1;
            if actual.words == *wanted {
                matched += 1;
            } else {
                *failures
                    .entry(format!(
                        "program {index} ({:?}): shipped {:08X?} vs built {:08X?}",
                        program.stage, actual.words, wanted
                    ))
                    .or_default() += 1;
            }
        }

        // The block's patched bytes: where it differs from the preamble body.
        if let Some(lists) = shader::TailLists::parse(&parsed.rest) {
            let block = &lists.block;
            if block.len() >= body.len() && block.len() - body.len() <= 64 {
                let header = block.len() - body.len();
                for at in 0..body.len() {
                    if block[header + at] != body[at] {
                        masks.push((index, program.stage, block[header + at]));
                    }
                }
            }
        }
    }

    println!("{}: cbuffer entries checked {checked}, matched {matched}", data_path);
    for (failure, count) in failures.iter().take(8) {
        println!("  {failure} x{count}");
    }
    println!("== block patch values (program, stage, value), {} total, first 40:", masks.len());
    for (program, stage, value) in masks.iter().take(40) {
        println!("  {program:3} {stage:?} {value:02X}");
    }
    Ok(())
}
