//! Prints a pixel program's tail lists (the cbuffers and resource records the
//! shipped section carries) next to its device block's head, so the block's
//! records can be matched against the names and sizes the lists already give.
//!
//! ```text
//! block_match <material data file>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: block_match <material data file>")?;
    let data = fs::read(&path)?;
    let offset = u32_at(&data, 12) as usize;
    let size = u32_at(&data, 16) as usize;
    let section = Section::parse(&data[offset..offset + size])?;
    let device = section.device_data();
    let programs = shader::parse_programs(device)?;

    let mut shown = 0;
    for (index, program) in programs.iter().enumerate() {
        if program.stage != shader::Stage::Pixel {
            continue;
        }
        let next = programs
            .get(index + 1)
            .map(|program| program.pos)
            .unwrap_or(device.len());
        let tail = &device[program.meta_pos + 16..next];
        let Some(parsed) = shader::Tail::parse(tail) else {
            continue;
        };
        println!("program {index} ({:?}), tail {} B", program.stage, tail.len());
        println!("  cbuffers:");
        for entry in &parsed.cbuffers {
            println!("    name {:08X} size {}", entry.name_hash(), entry.size());
        }
        let Some(lists) = shader::TailLists::parse(&parsed.rest) else {
            continue;
        };
        for (list, records) in lists.lists.iter().enumerate() {
            for record in records {
                println!(
                    "    list {list}: {}",
                    record
                        .iter()
                        .map(|word| format!("{word:08X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
        println!("  block {} B:", lists.block.len());
        for (index, word) in lists.block.chunks_exact(4).enumerate() {
            print!("{:08X} ", u32::from_le_bytes(word.try_into().unwrap()));
            if index % 8 == 7 {
                println!();
            }
        }
        println!();
        shown += 1;
        if shown >= 4 {
            break;
        }
    }
    Ok(())
}
