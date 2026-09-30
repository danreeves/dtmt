//! For every program of an engine-data file: the trailing block's byte length,
//! a hash of it, the program's interface hash (from its container's signatures)
//! and whether the block ends with the device preamble's body (the preamble
//! minus its 3-word header).
//!
//! ```text
//! block_census <engine_data text | material data>...
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sdk::filetype::shader::{self, Tail, TailLists};
use sdk::filetype::shader_engine_data::EngineData;
use sdk::murmur::Murmur32;

fn main() -> color_eyre::Result<()> {
    for arg in std::env::args().skip(1) {
        let bytes = fs::read(&arg)?;
        let engine = match String::from_utf8(bytes.clone()) {
            Ok(text) if EngineData::looks_like_text(&text) => EngineData::from_text(&text)?,
            _ => EngineData::from_material(&bytes)?,
        };
        let name = Path::new(&arg)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| arg.clone());
        let body = engine.device_preamble.get(12..).unwrap_or(&[]);
        let mut by_block: BTreeMap<u32, (usize, usize, Vec<u32>)> = BTreeMap::new();
        println!(
            "=== {name}: preamble {} bytes, {} programs",
            engine.device_preamble.len(),
            engine.programs.len()
        );

        for (index, (stage, tail_bytes)) in engine.programs.iter().enumerate() {
            let Some(tail) = Tail::parse(tail_bytes) else {
                continue;
            };
            let Some(lists) = TailLists::parse(&tail.rest) else {
                continue;
            };
            let block = &lists.block;
            let container = engine
                .program_containers
                .get(index)
                .copied()
                .flatten()
                .and_then(|at| engine.containers.get(at));
            let interface = container
                .and_then(|container| shader::signatures(container))
                .map(|(inputs, outputs)| {
                let mut text = String::new();
                for element in inputs.iter().chain(outputs.iter()) {
                    text.push_str(&format!(
                        "{}{}:{}:{};",
                        element.name, element.index, element.register, element.mask
                    ));
                }
                u32::from(Murmur32::hash(text.as_bytes()))
            });
            let hash = u32::from(Murmur32::hash(block));
            let ends = !body.is_empty() && block.ends_with(body);
            let head: Vec<String> = block
                .chunks(4)
                .take(10)
                .map(|chunk| {
                    format!(
                        "{:08X}",
                        u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]))
                    )
                })
                .collect();
            let entry = by_block.entry(hash).or_insert((0, block.len(), Vec::new()));
            entry.0 += 1;
            entry.2.push(interface.unwrap_or(0));
            println!(
                "  program {index:>3} {stage:?} interface {:08X} block {:>4} bytes hash {hash:08X}{}\n      head: {}",
                interface.unwrap_or(0),
                block.len(),
                if ends {
                    format!(", ends with the preamble body after {}", block.len() - body.len())
                } else {
                    String::new()
                },
                head.join(" ")
            );
        }

        println!("--- distinct blocks:");
        for (hash, (count, len, interfaces)) in &by_block {
            let distinct: std::collections::BTreeSet<u32> = interfaces.iter().copied().collect();
            println!(
                "  {hash:08X}: {count} programs, {len} bytes, {} interfaces",
                distinct.len()
            );
        }
    }
    Ok(())
}
