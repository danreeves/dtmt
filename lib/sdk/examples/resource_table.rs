//! Checks the resource-record index rule with the group's own framing: a
//! group's prefix is `{query_id, word, count}` and its descriptor list is the
//! `count` 16-byte entries that follow at +12. A 7-word resource record's
//! second word should be the resource's index in the list of the group its
//! program belongs to; the program-to-group mapping is not decoded, so each
//! program is matched against every group and a single fully matching group is
//! the evidence. Prints one line per section and a summary.
//!
//! ```text
//! resource_table <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::{self, Section, Tail, TailLists};

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

/// One group's descriptor list: its entries' name hashes by index.
struct List {
    names: Vec<u32>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut sections = 0usize;
    let mut programs = 0usize;
    let mut programs_matched = 0usize;
    let mut records = 0usize;
    let mut records_matched = 0usize;
    let mut oddity: BTreeMap<String, usize> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        let group_data = GroupData::new(section.group_data().to_vec());
        sections += 1;

        // Every group's descriptor list, walked by the contexts' query ids.
        let query_ids = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect::<Vec<_>>();
        let Some(starts) = group_data.group_starts(&query_ids) else {
            *oddity.entry("no group walk".into()).or_default() += 1;
            continue;
        };
        let lists = starts
            .iter()
            .map(|start| {
                let count = u32_at(group_data.bytes(), start + 8) as usize;
                List {
                    names: (0..count)
                        .map(|index| u32_at(group_data.bytes(), start + 12 + index * 16))
                        .collect(),
                }
            })
            .collect::<Vec<_>>();

        let device = section.device_data();
        let all = shader::parse_programs(device)?;
        let mut by_group: BTreeMap<usize, usize> = BTreeMap::new();
        for (program_index, program) in all.iter().enumerate() {
            let next = all
                .get(program_index + 1)
                .map(|program| program.pos)
                .unwrap_or(device.len());
            let Some(tail) = device.get(program.meta_pos + 16..next) else {
                continue;
            };
            let Some(parsed) = Tail::parse(tail) else {
                continue;
            };
            let Some(lists_of_program) = TailLists::parse(&parsed.rest) else {
                continue;
            };
            // The 7-word lists: 2, 3, 4 and 5.
            let words = [2usize, 3, 4, 5].map(|list| lists_of_program.list(list).to_vec());
            let total: usize = words
                .iter()
                .flat_map(|records| records.iter())
                .filter(|record| record.len() == 7)
                .count();
            if total == 0 {
                continue;
            }
            programs += 1;
            records += total;

            // The group that matches most of this program's records.
            let mut best: Option<(usize, usize)> = None;
            for (index, list) in lists.iter().enumerate() {
                let good = words
                    .iter()
                    .flat_map(|records| records.iter())
                    .filter(|record| record.len() == 7)
                    .filter(|record| match list.names.iter().position(|name| *name == record[0]) {
                        Some(position) => position as u32 == record[1],
                        None => false,
                    })
                    .count();
                if best.map(|(_, best_good)| good > best_good).unwrap_or(true) {
                    best = Some((index, good));
                }
            }
            if let Some((group, good)) = best {
                records_matched += good;
                if good == total {
                    programs_matched += 1;
                    *by_group.entry(group).or_default() += 1;
                } else {
                    *oddity
                        .entry(format!("program short of full match: {good}/{total}"))
                        .or_default() += 1;
                }
            }
        }
        if std::env::var("RESOURCE_TABLE_VERBOSE").is_ok() {
            let lists = lists
                .iter()
                .enumerate()
                .map(|(index, list)| format!("g{index}:{}", list.names.len()))
                .collect::<Vec<_>>()
                .join(" ");
            let by_group = by_group
                .iter()
                .map(|(group, count)| format!("g{group}:{count}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "  {}: {lists} | programs by matching group: {by_group}",
                std::path::Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
        }
    }

    println!(
        "{sections} section(s): {programs} programs with 7-word records, {programs_matched} fully \
         matched by one group; {records} records, {records_matched} matched"
    );
    for (oddity, count) in oddity.iter().take(10) {
        println!("  {oddity} x{count}");
    }
    Ok(())
}
