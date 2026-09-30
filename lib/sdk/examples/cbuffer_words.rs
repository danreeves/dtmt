//! Checks the constant-buffer entries' index word against the group data: for
//! every tail cbuffer, word 1 should be the resource's index in the group's
//! descriptor list, the same rule the 7-word resource records follow. Also
//! prints the words beside it (4 is always 1, 5 always 0, so far).
//!
//! ```text
//! cbuffer_words <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::{self, Section, Tail};

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
    let mut sections = 0usize;
    let mut checked = 0usize;
    let mut matched = 0usize;
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut side_words: BTreeMap<(u32, u32, u32), usize> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        sections += 1;

        // Every group's descriptor list (name -> index), walked by the
        // contexts' query ids.
        let group = GroupData::new(section.group_data().to_vec());
        let query_ids: Vec<u32> = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let Some(starts) = group.group_starts(&query_ids) else {
            continue;
        };
        let lists: Vec<BTreeMap<u32, usize>> = starts
            .iter()
            .map(|start| {
                let count = u32_at(group.bytes(), start + 8) as usize;
                (0..count)
                    .map(|index| (u32_at(group.bytes(), start + 12 + index * 16), index))
                    .collect()
            })
            .collect();

        let device = section.device_data();
        let programs = shader::parse_programs(device)?;
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
                let name = entry.words[0];
                let word1 = entry.words[1];
                *side_words
                    .entry((entry.words[3], entry.words[4], entry.words[5]))
                    .or_default() += 1;
                // The index may be against any group's list (the
                // program-to-group mapping is not decoded), so a match on one
                // list is enough.
                if lists
                    .iter()
                    .any(|list| list.get(&name).copied() == Some(word1 as usize))
                {
                    matched += 1;
                } else {
                    let indexes: Vec<String> = lists
                        .iter()
                        .filter_map(|list| list.get(&name))
                        .map(|index| index.to_string())
                        .collect();
                    *failures
                        .entry(format!(
                            "name {:08X}: word1 {word1} vs descriptor index(es) [{}]",
                            name,
                            indexes.join(",")
                        ))
                        .or_default() += 1;
                }
            }
        }
    }

    println!("{sections} section(s): {checked} cbuffer entries, {matched} match the descriptor-index rule");
    for (failure, count) in failures.iter().take(10) {
        println!("  {failure} x{count}");
    }
    println!("== the words beside name/size (3/4/5), by frequency:");
    let mut side: Vec<_> = side_words.into_iter().collect();
    side.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (tuple, count) in side.iter().take(8) {
        println!(
            "  words 3/4/5 = {:08X}/{:08X}/{:08X} x{count}",
            tuple.0, tuple.1, tuple.2
        );
    }
    Ok(())
}
