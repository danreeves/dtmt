//! Prints the per-program structure of a section: stage, tail index, block mask,
//! and the distinct (stage, tail, mask) triples with their program indices, plus
//! each distinct tail's list record counts. This is the raw shape for working out
//! the program order and the mask byte.
//!
//! ```text
//! program_order <material data file | .raw section>
//! ```

use std::collections::HashMap;
use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section, Stage, Tail, TailLists};

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

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: program_order <data>")?;
    let data = fs::read(&path)?;
    let bytes = section_bytes(&data).ok_or("not a section")?;
    let section = Section::parse(bytes)?;
    let device = section.device_data();
    let programs = shader::parse_programs(device)?;

    // The preamble body, for the block masks.
    let preamble_end = programs.first().map(|program| program.pos).unwrap_or(0);
    let body = device
        .get(12..preamble_end)
        .unwrap_or_default()
        .to_vec();

    // Dedup the tails.
    let mut tails: Vec<Vec<u8>> = Vec::new();
    let mut tail_index = Vec::new();
    for program in &programs {
        let next = programs
            .iter()
            .find(|next| next.pos > program.meta_pos + 16)
            .map(|next| next.pos)
            .unwrap_or(device.len());
        let tail = device
            .get(program.meta_pos + 16..next)
            .unwrap_or_default()
            .to_vec();
        match tails.iter().position(|other| *other == tail) {
            Some(index) => tail_index.push(index),
            None => {
                tails.push(tail);
                tail_index.push(tails.len() - 1);
            }
        }
    }

    println!("== tails ({} distinct):", tails.len());
    for (index, tail) in tails.iter().enumerate() {
        let parsed = Tail::parse(tail);
        let lists = parsed
            .as_ref()
            .and_then(|parsed| TailLists::parse(&parsed.rest));
        let counts: Vec<usize> = lists
            .as_ref()
            .map(|lists| lists.lists.iter().map(|records| records.len()).collect())
            .unwrap_or_default();
        println!(
            "  tail {index:2}: {} cbuffers, lists {:?}",
            parsed.as_ref().map(|p| p.cbuffers.len()).unwrap_or(0),
            counts
        );
    }

    println!("== programs:");
    let mut masks: HashMap<(Stage, usize, u8), Vec<usize>> = HashMap::new();
    for (index, program) in programs.iter().enumerate() {
        let tail = &tails[tail_index[index]];
        let parsed = Tail::parse(tail);
        let lists = parsed
            .as_ref()
            .and_then(|parsed| TailLists::parse(&parsed.rest));
        let mask = lists
            .as_ref()
            .and_then(|lists| {
                let block = &lists.block;
                (block.len() >= body.len() && block.len() - body.len() <= 64).then(|| {
                    let header = block.len() - body.len();
                    (0..body.len())
                        .find(|at| block[header + at] != body[*at])
                        .map(|at| block[header + at])
                        .unwrap_or(0)
                })
            })
            .unwrap_or(255);
        println!(
            "  {index:3} {:?} tail {:2} mask {:02X}",
            program.stage, tail_index[index], mask
        );
        masks
            .entry((program.stage, tail_index[index], mask))
            .or_default()
            .push(index);
    }

    println!("== distinct (stage, tail, mask) triples: {}", masks.len());
    let mut entries: Vec<_> = masks.into_iter().collect();
    entries.sort_by_key(|((stage, tail, mask), _)| (format!("{stage:?}"), *tail, *mask));
    for ((stage, tail, mask), indices) in &entries {
        println!(
            "  {stage:?} tail {tail:2} mask {mask:02X}: {} programs {}",
            indices.len(),
            indices
                .iter()
                .take(8)
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    Ok(())
}
