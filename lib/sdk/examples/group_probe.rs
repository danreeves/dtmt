//! Reports which gate stops an engine data file's `group_data` blob from
//! converting to a template, and where a template rebuild diverges.
//!
//! ```text
//! group_probe <engine_data>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::{mid_sections, GroupData, Record};
use sdk::filetype::shader_engine_data::EngineData;
use sdk::murmur::Murmur32;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// A group head's descriptor list: after the query id and one word, a u32 count
/// then `count` 16-byte `{name, flags, x, y}` records.
fn head_descriptors(head: &[u8]) -> Option<Vec<[u32; 4]>> {
    let tail = head.get(8..)?;
    let count = u32::from_le_bytes(tail.get(..4)?.try_into().ok()?) as usize;
    let bytes = tail.get(4..4 + count * 16)?;
    Some(
        bytes
            .chunks_exact(16)
            .map(|r| std::array::from_fn(|w| u32::from_le_bytes(r[w * 4..w * 4 + 4].try_into().unwrap())))
            .collect(),
    )
}

/// murmur32 of the engine's own resource names, for a readable descriptor dump.
fn known_name(hash: u32) -> Option<&'static str> {    for name in [
        "global_viewport",
        "c_per_object",
        "c_instance_offset",
        "c_depth_only",
        "bones",
        "last_bones",
        "idata",
        "global_texture2D",
        "global_feedback_buffers",
        "global_samplers",
    ] {
        if u32::from(Murmur32::hash(name.as_bytes())) == hash {
            return Some(name);
        }
    }
    None
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: group_probe <engine_data>")?;
    let engine_data = EngineData::from_text(&fs::read_to_string(&path)?)?;
    let blob = engine_data.group_data.clone();
    println!(
        "group_data {} bytes, contexts {} bytes, template {}",
        blob.len(),
        engine_data.contexts.len(),
        engine_data.group_template.is_some()
    );
    if blob.is_empty() {
        println!("no blob: the file already carries a template");
        if let Some(template) = &engine_data.group_template {
            for (index, parts) in template.groups.iter().enumerate() {
                println!(
                    "  group {index}: material_first={} has_engine={} head={} between={} mid={} tail={}",
                    parts.material_first,
                    parts.has_engine,
                    parts.head.len(),
                    parts.between.len(),
                    parts.mid.len(),
                    parts.tail.len()
                );
                if let Some(descriptors) = head_descriptors(&parts.head) {
                    for (slot, [name, flags, x, y]) in descriptors.iter().enumerate() {
                        let label = known_name(*name)
                            .map(|n| format!(" {n}"))
                            .unwrap_or_default();
                        println!("      [{slot:2}] {name:08X}{label:<24} flags {flags:08X} x {x} y {y}");
                    }
                }
                match mid_sections(&parts.mid) {
                    Some(sections) => println!(
                        "      mid: {}",
                        sections
                            .iter()
                            .map(|section| {
                                let step = if section.packed { 28 } else { 20 };
                                format!(
                                    "{{W1={} W2={} n={} x{step}w}}",
                                    section.w1,
                                    section.w2,
                                    section.records.len() / step
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ),
                    None => println!("      mid: does not walk ({} B)", parts.mid.len()),
                }
            }
            println!("engine records: {}", template.engine.len());
            println!(
                "material tables: {:?}",
                engine_data.material_tables.iter().map(Vec::len).collect::<Vec<_>>()
            );
            let rebuilt = GroupData::build(
                template,
                &engine_data.material_tables,
                &template.engine,
            )?;
            println!("rebuilt group data: {} bytes", rebuilt.len());
            if let Some(dump) = std::env::args().nth(2) {
                fs::write(&dump, &rebuilt)?;
                println!("wrote rebuilt group data to {dump}");
            }
        }
        return Ok(());
    }

    let query_ids = engine_data.query_ids()?;
    println!("query ids ({}): {:?}", query_ids.len(), query_ids);

    let group = GroupData::new(blob.clone());
    println!("group count (word 0): {:?}", group.group_count());
    println!("group_starts: {:?}", group.group_starts(&query_ids));
    println!(
        "condition_headers: {}",
        group.condition_headers(&query_ids).map(|h| h.len()).is_some()
    );
    println!("engine_records: {}", group.engine_records().is_some());
    println!(
        "object_tables: {}",
        group.object_tables(&query_ids).map(|t| t.len()).is_some()
    );

    let Some(template) = group.template(&query_ids) else {
        println!("template: None");
        return Ok(());
    };
    println!("template: Some ({} groups)", template.groups.len());
    for (index, parts) in template.groups.iter().enumerate() {
        println!(
            "  group {index}: material_first={} has_engine={} head={} between={} mid={} tail={}",
            parts.material_first,
            parts.has_engine,
            parts.head.len(),
            parts.between.len(),
            parts.mid.len(),
            parts.tail.len()
        );
    }
    let engine: Vec<Record> = group.engine_records().unwrap_or_default();
    println!("shared engine records: {}", engine.len());

    let tables = group.object_tables(&query_ids).unwrap_or_default();
    let material: Vec<Vec<Record>> = tables.iter().map(|(_, records)| records.clone()).collect();
    println!(
        "material table lengths: {:?}",
        material.iter().map(Vec::len).collect::<Vec<_>>()
    );

    let rebuilt = GroupData::build(&template, &material, &engine)?;
    println!("rebuilt {} bytes vs blob {}", rebuilt.len(), blob.len());
    let common = rebuilt.len().min(blob.len());
    if let Some(at) = (0..common).find(|i| rebuilt[*i] != blob[*i]) {
        let from = at.saturating_sub(16);
        let to = (at + 32).min(common);
        println!("first diff at +{at} (abs {})", at + 4);
        println!("  blob  [{from}..{to}]: {}", hex(&blob[from..to]));
        println!("  built [{from}..{to}]: {}", hex(&rebuilt[from..to]));
    } else {
        println!("identical up to {common} bytes");
    }
    Ok(())
}
