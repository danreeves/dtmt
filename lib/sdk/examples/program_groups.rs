//! Prints each program's group, matched by the descriptor-index rule: a program's
//! resource records name a descriptor of its own group, so the group whose list
//! contains every record at its recorded index is the program's group.
//!
//! ```text
//! program_groups <material data file | .raw section>...
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
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
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else { continue };
        let Ok(section) = Section::parse(bytes) else { continue };
        let group_data = GroupData::new(section.group_data().to_vec());
        let query_ids: Vec<u32> = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let Some(starts) = group_data.group_starts(&query_ids) else {
            println!("{}: no group walk", path.rsplit(['\\', '/']).next().unwrap_or(&path));
            continue;
        };
        let lists: Vec<Vec<u32>> = starts
            .iter()
            .map(|start| {
                let count = u32_at(group_data.bytes(), start + 8) as usize;
                (0..count)
                    .map(|index| u32_at(group_data.bytes(), start + 12 + index * 16))
                    .collect()
            })
            .collect();

        for (group, start) in starts.iter().enumerate() {
            let words: Vec<String> = (0..10)
                .map(|word| format!("{:08X}", u32_at(group_data.bytes(), start + word * 4)))
                .collect();
            println!("  group {group} @ {start:#x}: {}", words.join(" "));
        }

        // Search the group data for the per-group program counts, as a u32 run.
        let bytes = group_data.bytes();
        for seq in [[10u32, 10, 2, 2, 2], [5, 5, 1, 1, 1], [2, 2, 2, 2, 2]] {
            let mut at = 0usize;
            while at + seq.len() * 4 <= bytes.len() {
                if seq
                    .iter()
                    .enumerate()
                    .all(|(k, v)| u32_at(bytes, at + k * 4) == *v)
                {
                    println!("  sequence {seq:?} at {at:#x}");
                }
                at += 4;
            }
        }

        for (index, context) in section.contexts().iter().enumerate() {
            println!(
                "  context {index}: name {:08X} word2 {:08X} {} queries {:?}",
                context.name,
                context.flags,
                context.queries.len(),
                context
                    .queries
                    .iter()
                    .map(|query| format!("{:08X}@{}", query.id, query.conditions))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(headers) = group_data.condition_headers(&query_ids) {
            for (group, header) in headers.iter().enumerate() {
                println!(
                    "  condition header {group}: n={} entries {:?}",
                    header.entries.len(),
                    header
                        .entries
                        .iter()
                        .map(|entry| format!("{:08X}", entry.hash))
                        .collect::<Vec<_>>()
                );
            }
        }

        let device = section.device_data();
        let programs = shader::parse_programs(device)?;
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        println!("== {name}: {} programs, {} groups", programs.len(), lists.len());
        let mut by_group: Vec<usize> = vec![0; lists.len()];
        let mut assigned: Vec<Option<usize>> = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let end = programs
                .get(index + 1)
                .map(|next| next.pos)
                .unwrap_or(device.len());
            let tail = device.get(program.meta_pos + 16..end).unwrap_or_default();
            let records: Vec<Vec<u32>> = Tail::parse(tail)
                .and_then(|parsed| TailLists::parse(&parsed.rest))
                .map(|lists| {
                    [2usize, 3, 4, 5]
                        .iter()
                        .flat_map(|list| lists.list(*list).to_vec())
                        .filter(|record| record.len() == 7)
                        .collect()
                })
                .unwrap_or_default();
            let total = records.len();
            let mut best: Option<(usize, usize)> = None;
            for (group, names) in lists.iter().enumerate() {
                let good = records
                    .iter()
                    .filter(|record| {
                        names
                            .iter()
                            .position(|name| *name == record[0])
                            .is_some_and(|position| position as u32 == record[1])
                    })
                    .count();
                if best.is_none_or(|(_, best_good)| good > best_good) {
                    best = Some((group, good));
                }
            }
            let assigned_group = best.and_then(|(group, good)| (good == total && total > 0).then_some(group));
            if let Some(group) = assigned_group {
                by_group[group] += 1;
            }
            assigned.push(assigned_group);
            let stage = match program.stage {
                Stage::Vertex => "V",
                Stage::Pixel => "P",
                _ => "?",
            };
            println!(
                "  {index:3} {stage} group {:?} records {total}",
                assigned_group.map(|g| g.to_string()).unwrap_or_else(|| "-".into())
            );
        }
        println!("  per-group program counts: {by_group:?}");
    }
    Ok(())
}
