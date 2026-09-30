//! Synthesizes the **minimal** engine data for a shader whose interface is a
//! shipped one: one default context with one query, no conditions, a one-group
//! template, and the minimal device preamble (no config records, the stream, a
//! zero tail). Only the parts whose rules are still unknown stay borrowed from
//! the carried file - the group's head/between/mid/tail, the engine table, the
//! stream record and the programs' cbuffer and resource lists.
//!
//! All 70 zero-config sections in the sampled corpus are exactly this shape:
//! `w1 = 1` (one query), one context, `w2 = 2`, no config records.
//!
//! **In-game result:** the reduced group/count wrappers crash the engine at
//! `dispatch_loadtime`, and so does reducing the program set with everything
//! else carried. The one reduction the engine accepts is `--empty-conditions`
//! with everything else carried: the shader loads and renders with no
//! conditions blob. So this file is a probe, not a generator - the engine needs
//! the contexts, groups and programs consistent with the shader's own compiled
//! structure.
//!
//! ```text
//! synth_engine_data <carried.engine_data> <out.engine_data> [--keep-preamble]
//! ```
//!
//! With `--keep-preamble` the device preamble and the programs' blocks stay the
//! carried ones, so only the contexts, conditions and group count change - one
//! variable at a time.

use std::collections::HashSet;
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupTemplate;
use sdk::filetype::shader::{self, Stage, Tail, TailLists};
use sdk::filetype::shader_engine_data::EngineData;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keep_preamble = args.iter().any(|arg| arg == "--keep-preamble");
    // The carried blocks keep the full per-program interface records; the
    // minimal preamble plus carried blocks is the middle step between the rich
    // wrapper and the fully minimal one.
    let keep_blocks = keep_preamble || args.iter().any(|arg| arg == "--keep-blocks");
    // `--keep-groups` keeps the contexts, conditions and the whole group
    // template, so the only change is the programs (the first of each stage).
    let keep_groups = args.iter().any(|arg| arg == "--keep-groups");
    // `--empty-conditions` keeps the contexts and groups but rewrites every
    // query to `FFFFFFFF` and drops the conditions blob.
    let empty_conditions = args.iter().any(|arg| arg == "--empty-conditions");
    // `--keep-programs` keeps every program rather than the first of each stage.
    let keep_programs = args.iter().any(|arg| arg == "--keep-programs");
    let files: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();
    let [input, output] = files.as_slice() else {
        return Err(
            "usage: synth_engine_data <carried.engine_data> <out.engine_data> \
             [--keep-preamble] [--keep-blocks] [--keep-groups] [--empty-conditions] \
             [--keep-programs]"
                .into(),
        );
    };

    let current = EngineData::from_text(&fs::read_to_string(input)?)?;
    let template = current
        .group_template
        .clone()
        .ok_or("the carried file has no group template")?;
    let first_material = current
        .material_tables
        .first()
        .cloned()
        .ok_or("the carried file has no material table")?;

    // The query id: the first group's own hash, which the contexts must name.
    let query = u32_at(&template.groups[0].head, 0);

    // One default context (`F2760503`) with one query and no conditions.
    let mut contexts = Vec::new();
    for word in [0xF276_0503u32, 0, 1, query, 0xFFFF_FFFF] {
        contexts.extend_from_slice(&word.to_le_bytes());
    }
    let (contexts, context_count, conditions, template, material_tables) =
        if keep_groups || empty_conditions {
            // The carried contexts, optionally with every query's conditions
            // offset rewritten to "none" and the blob dropped.
            let mut contexts = current.contexts.clone();
            let mut conditions = current.conditions.clone();
            if empty_conditions {
                let mut at = 0usize;
                while at + 12 <= contexts.len() {
                    let count = u32_at(&contexts, at + 8) as usize;
                    if at + 12 + count * 8 > contexts.len() {
                        break;
                    }
                    for query in 0..count {
                        let offset = at + 12 + query * 8 + 4;
                        contexts[offset..offset + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                    }
                    at += 12 + count * 8;
                }
                conditions = Vec::new();
            }
            (
                contexts,
                current.context_count,
                conditions,
                template,
                current.material_tables.clone(),
            )
        } else {
            (
                contexts,
                1,
                Vec::new(),
                GroupTemplate {
                    prefix: 1u32.to_le_bytes().to_vec(),
                    groups: vec![template.groups[0].clone()],
                    engine: template.engine,
                },
                vec![first_material],
            )
        };

    // The minimal preamble: `{1, 1, 2, 0}`, the stream taken from the carried
    // one, and a 20-byte zero tail. With `--keep-preamble` the carried preamble
    // stays, so the change is only the contexts and the group count.
    let preamble = if keep_preamble {
        current.device_preamble.clone()
    } else {
        let carried = &current.device_preamble;
        let configs = u32_at(carried, 12) as usize;
        let stream_at = 16 + configs * 13;
        let stream_count = u32_at(carried, stream_at) as usize;
        let mut at = stream_at + 4;
        for _ in 0..stream_count {
            let name = u32_at(carried, at);
            let kind = u32_at(carried, at + 4);
            at += if kind == 5 && name >= 0x1000 { 73 } else { 60 };
        }
        let mut preamble = Vec::new();
        for word in [1u32, 1, 2, 0] {
            preamble.extend_from_slice(&word.to_le_bytes());
        }
        preamble.extend_from_slice(&carried[stream_at..at]);
        preamble.extend_from_slice(&[0u8; 20]);
        preamble
    };
    let body = preamble[12..].to_vec();

    // One program per stage: the first of each. Its block is the preamble's body
    // (with `--keep-preamble`, the carried block stays).
    let mut programs = Vec::new();
    let mut stages: HashSet<Stage> = HashSet::new();
    for (stage, tail) in &current.programs {
        if !matches!(stage, Stage::Vertex | Stage::Pixel) {
            continue;
        }
        let first_of_stage = stages.insert(*stage);
        if !keep_programs && !first_of_stage {
            continue;
        }
        if keep_blocks {
            programs.push((*stage, tail.clone()));
            continue;
        }
        let parsed = Tail::parse(tail).ok_or("a carried tail does not parse")?;
        let lists = TailLists::parse(&parsed.rest).ok_or("a carried tail's lists do not parse")?;
        let rebuilt = TailLists {
            lists: lists.lists,
            block: body.clone(),
        }
        .bytes();
        let tail = Tail {
            cbuffers: parsed.cbuffers,
            rest: rebuilt,
        };
        programs.push((*stage, tail.bytes()));
    }

    let synthesized = EngineData {
        material_hash: current.material_hash,
        context_count,
        dependency_count: 1,
        contexts,
        conditions,
        dependencies: Vec::new(),
        group_data: Vec::new(),
        group_template: Some(template),
        material_tables,
        device_preamble: preamble,
        programs,
        tails: Vec::new(),
        containers: Vec::new(),
        program_containers: vec![None; 2],
    };

    let text = synthesized.to_text();
    fs::write(output, &text)?;
    println!(
        "wrote {} bytes: {context_count} context(s), query {query:08X}, {} condition bytes, \
         {} group(s), {} program(s), preamble {} bytes",
        text.len(),
        synthesized.conditions.len(),
        synthesized.group_template.as_ref().map_or(0, |template| template.groups.len()),
        synthesized.programs.len(),
        synthesized.device_preamble.len()
    );
    Ok(())
}
