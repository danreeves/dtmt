//! Checks the understood parts of a program tail against a material's own
//! engine data: the cbuffer list (name hash + size) and the signature records
//! (`{murmur32(name), semantic index, register}` for every element of the
//! container's input signature; the engine spells system values in upper case,
//! `SV_POSITION`, while the container says `SV_Position`).
//!
//! ```text
//! tail_check <material data file> [Vertex|Pixel]
//! ```

use std::fs;

use sdk::filetype::shader::{self, Stage, Tail};
use sdk::filetype::shader_engine_data::EngineData;
use sdk::murmur::Murmur32;

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = fs::read(&args[0])?;
    let engine = EngineData::from_material(&data)?;
    let want = match args.get(1).map(String::as_str) {
        Some("Vertex") => Stage::Vertex,
        _ => Stage::Pixel,
    };

    let index = engine
        .programs
        .iter()
        .position(|(stage, _)| *stage == want)
        .ok_or_else(|| color_eyre::eyre::eyre!("no {want:?} program"))?;
    let (_, tail_bytes) = &engine.programs[index];
    let container = engine
        .program_containers
        .get(index)
        .copied()
        .flatten()
        .and_then(|at| engine.containers.get(at))
        .ok_or_else(|| color_eyre::eyre::eyre!("no container for the program"))?;

    let tail = Tail::parse(tail_bytes).ok_or_else(|| color_eyre::eyre::eyre!("tail does not parse"))?;
    println!("=== {want:?} tail, {} bytes, {} cbuffers", tail_bytes.len(), tail.cbuffers.len());
    for entry in &tail.cbuffers {
        println!("  cbuffer {:08X} size {}", entry.name_hash(), entry.size());
    }

    let (inputs, outputs) = shader::signatures(container)
        .ok_or_else(|| color_eyre::eyre::eyre!("no signatures in the container"))?;
    println!("=== signature elements");
    let rest = &tail.rest;
    let words: Vec<u32> = rest
        .chunks(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4])))
        .collect();
    for (label, elements) in [("input", &inputs), ("output", &outputs)] {
        for element in elements.iter() {
            let system = element.name.starts_with("SV_");
            // The engine hashes its own semantic names, which spell system
            // values in upper case (`SV_POSITION`), so try both forms.
            let hashes = [
                u32::from(Murmur32::hash(&element.name)),
                u32::from(Murmur32::hash(element.name.to_uppercase().as_str())),
            ];
            // The record is `{hash, semantic index, register}`.
            let found = words.windows(3).position(|window| {
                hashes.contains(&window[0])
                    && window[1] == element.index as u32
                    && window[2] == element.register as u32
            });
            match found {
                Some(at) => println!(
                    "  {label:>6} {:>12} idx {} reg {}  record at word {at}: {:08X} {} {}",
                    element.name, element.index, element.register, words[at], words[at + 1], words[at + 2]
                ),
                None => println!(
                    "  {label:>6} {:>12} idx {} reg {}{}  NOT in the tail",
                    element.name,
                    element.index,
                    element.register,
                    if system { " (system value)" } else { "" }
                ),
            }
        }
    }

    Ok(())
}
