//! Tests whether a pixel program's channel mask is the set of uv sets it samples
//! with, by reading the mask byte the block patches into the preamble's first
//! texture-slot config record beside the container's `CUSTOM` (interpolator)
//! indices. The reading is **refuted**: 75 of 300 sampled programs carry a mask
//! bit with no matching interpolator, so the four bits are not the uv sets. The
//! probe stays as the falsification's evidence, and its diff also picks up
//! blocks with other patches, so odd masks (`00`, `05`, `10`, `28`, `5E`) are
//! its own artifact.
//!
//! ```text
//! mask_probe <material data file>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section, Stage, Tail, TailLists};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn shader_section(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 28 {
        return None;
    }
    let offset = u32_at(data, 12) as usize;
    let size = u32_at(data, 16) as usize;
    if size == 0 {
        return None;
    }
    data.get(offset..offset + size)
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: mask_probe <material>")?;
    let data = fs::read(&path)?;
    let section = Section::parse(shader_section(&data).ok_or("no shader section")?)?;
    let device = section.device_data();
    let programs = shader::parse_programs(device)?;
    let preamble = &device[..programs.first().map_or(0, |program| program.pos)];
    // The preamble's body: its first 12 bytes are the head, and the block is
    // the body with the per-program masks patched in.
    let body = preamble.get(12..).unwrap_or_default();

    let mut mismatches = 0usize;
    let mut masks = 0usize;
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
        let block = &lists.block;

        // The mask: the byte that differs from the preamble's body. The block
        // is the body with the mask patched, optionally behind a leading word.
        let offset = block.len().checked_sub(body.len());
        let Some(offset) = offset.filter(|offset| *offset <= 40) else {
            println!(
                "program {index:3} {:?} block={} body={} preamble={} offset={:?}",
                program.stage,
                block.len(),
                body.len(),
                preamble.len(),
                block.len().checked_sub(body.len())
            );
            continue;
        };
        let mut mask = 0u8;
        let mut diff = 0usize;
        for at in 0..body.len().min(block.len() - offset) {
            if block[at + offset] != body[at] {
                diff += 1;
                if diff == 1 {
                    mask = block[at + offset];
                }
            }
        }
        if diff == 0 {
            println!(
                "program {index:3} {:?} block={} body={} preamble={} no diff",
                program.stage,
                block.len(),
                body.len(),
                preamble.len()
            );
            continue;
        }
        masks += 1;

        let texcoords: Vec<u32> = shader::signatures(&program.container)
            .map(|(inputs, _)| {
                inputs
                    .iter()
                    .filter(|element| element.name.eq_ignore_ascii_case("CUSTOM"))
                    .map(|element| element.index)
                    .collect()
            })
            .unwrap_or_default();
        let subset = (0..4)
            .filter(|bit| mask & (1 << bit) != 0)
            .all(|bit| texcoords.contains(&bit));
        if !subset {
            mismatches += 1;
        }
        println!(
            "program {index:3} {:?} mask={mask:02X} texcoords=[{}] subset={} diff_bytes={diff}",
            program.stage,
            texcoords
                .iter()
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            if subset { "yes" } else { "NO" }
        );
    }

    println!("{masks} program(s) with a patched mask, {mismatches} not a subset");
    let _ = Stage::Pixel;
    Ok(())
}
