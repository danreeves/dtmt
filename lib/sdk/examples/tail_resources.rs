//! Dumps the resource records of every program tail's nine lists across a
//! sample, to see whether their names, bindings and sets follow a per-kind
//! convention (which would make them engine constants plus the declaration's
//! `samplers`) or are per-shader data.
//!
//! ```text
//! tail_resources <material data file>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section, Tail, TailLists};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    // list -> (record words -> count)
    let mut lists: BTreeMap<usize, BTreeMap<Vec<u32>, usize>> = BTreeMap::new();
    let mut sections = 0usize;
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let offset = u32_at(&data, 12) as usize;
        let size = u32_at(&data, 16) as usize;
        let section = Section::parse(&data[offset..offset + size])?;
        let device = section.device_data();
        let programs = shader::parse_programs(device)?;
        sections += 1;

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
            let Some(lists_parsed) = TailLists::parse(&parsed.rest) else {
                continue;
            };
            for (list, records) in lists_parsed.lists.iter().enumerate() {
                for record in records {
                    *lists
                        .entry(list)
                        .or_default()
                        .entry(record.clone())
                        .or_default() += 1;
                }
            }
        }
    }

    println!("{sections} section(s)");
    for (list, records) in &lists {
        let mut top: Vec<(&Vec<u32>, &usize)> = records.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        println!("list {list}: {} distinct record(s)", records.len());
        for (record, count) in top.iter().take(6) {
            println!(
                "   x{count:<5} [{}]",
                record
                    .iter()
                    .map(|word| format!("{word:08X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    Ok(())
}
