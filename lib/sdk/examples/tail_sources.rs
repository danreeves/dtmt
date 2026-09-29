//! Tests whether a program tail's constant-buffer entries are derivable from the
//! section's own group data: the names from the group header and the descriptors,
//! the sizes from the tables' records (a table's cbuffer ends at the largest
//! `offset + size` it holds). If they are, the tails' cbuffer lists need no
//! container reflection.
//!
//! ```text
//! tail_sources <material data file>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::{self, Section, Tail};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut checked = 0usize;
    let mut matched = 0usize;
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let offset = u32_at(&data, 12) as usize;
        let size = u32_at(&data, 16) as usize;
        let section = Section::parse(&data[offset..offset + size])?;
        let device = section.device_data();
        let programs = shader::parse_programs(device)?;

        let query_ids: Vec<u32> = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let group = GroupData::new(section.group_data().to_vec());
        let Some(starts) = group.group_starts(&query_ids) else {
            failures.insert("no group walk".into(), 1);
            continue;
        };

        // The cbuffers the group data names, with the size each table implies:
        // a table's cbuffer ends at the largest `offset + size` it holds, rounded
        // up to the 16-byte granularity the tail records.
        let round16 = |end: u32| (end + 15) / 16 * 16;
        let mut cbuffers: BTreeMap<u32, u32> = BTreeMap::new();
        if let Some((_, records)) = group.object_tables(&query_ids).and_then(|t| t.into_iter().next())
        {
            let end = records
                .iter()
                .map(|record| record.offset + record.size)
                .max()
                .unwrap_or(0);
            cbuffers.insert(u32_at(group.bytes(), starts[0] + 12), round16(end));
        }
        if let Some(engine) = group.engine_records() {
            let end = engine
                .iter()
                .map(|record| record.offset + record.size)
                .max()
                .unwrap_or(0);
            if let Some(descriptors) = group.descriptors() {
                cbuffers.insert(descriptors.engine.name, round16(end));
            }
        }

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
            for entry in &parsed.cbuffers {
                checked += 1;
                match cbuffers.get(&entry.name_hash()) {
                    Some(size) if *size == entry.size() => matched += 1,
                    Some(size) => {
                        *failures
                            .entry(format!("size: {:08X} tail {} vs table {}", entry.name_hash(), entry.size(), size))
                            .or_default() += 1;
                    }
                    None => {
                        *failures
                            .entry(format!("name: {:08X} not in the group data", entry.name_hash()))
                            .or_default() += 1;
                    }
                }
            }
        }
    }

    println!("cbuffer entries checked: {checked}; matched: {matched}");
    for (failure, count) in failures.iter().take(12) {
        println!("  {failure} x{count}");
    }
    Ok(())
}
