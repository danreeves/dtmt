//! Checks the resource-record index rule: a 7-word resource record's second
//! word is the resource's index in the group's 16-byte descriptor list (the
//! entries after the group's 12-byte prefix, before the first table). Prints
//! one line per section and a summary.
//!
//! ```text
//! resource_table <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

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

fn main() -> Result<(), Box<dyn Error>> {
    let mut sections = 0usize;
    let mut checked = 0usize;
    let mut matched = 0usize;
    let mut failures: BTreeMap<String, usize> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        let group = sdk::filetype::group_data::GroupData::new(section.group_data().to_vec());
        sections += 1;

        // The descriptor entries start at 16, right after the group's 12-byte
        // prefix, one 16-byte entry each, while the first word is a real name
        // hash - the table headers that follow are small words, so the size
        // tells the boundary. The X word advances by 24 for a constant buffer
        // and 8 otherwise, starting at 0.
        let mut entries: BTreeMap<u32, usize> = BTreeMap::new();
        let mut index = 0usize;
        while 16 + index * 16 + 16 <= group.bytes().len() {
            let at = 16 + index * 16;
            let name = u32_at(group.bytes(), at);
            let x = u32_at(group.bytes(), at + 8);
            if name <= 0x1_0000 {
                break;
            }
            if index == 0 {
                if x != 0 {
                    break;
                }
            } else {
                let previous = u32_at(group.bytes(), 16 + (index - 1) * 16 + 8);
                if x != previous + 8 && x != previous + 24 {
                    break;
                }
            }
            entries.insert(name, index);
            index += 1;
        }

        let device = section.device_data();
        let programs = shader::parse_programs(device)?;
        let mut offsets: BTreeMap<(usize, i64), usize> = BTreeMap::new();
        let mut by_list: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for (program_index, program) in programs.iter().enumerate() {
            let next = programs
                .get(program_index + 1)
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
            // The 7-word lists: 2, 3, 4 and 5.
            for list in [2, 3, 4, 5] {
                for record in lists.list(list) {
                    if record.len() != 7 {
                        continue;
                    }
                    checked += 1;
                    let entry = entries.get(&record[0]);
                    let counts = by_list.entry(list).or_default();
                    counts.0 += 1;
                    match entry {
                        Some(entry) => {
                            let offset = record[1] as i64 - *entry as i64;
                            *offsets.entry((list, offset)).or_default() += 1;
                            if offset == 0 {
                                counts.1 += 1;
                                matched += 1;
                            }
                        }
                        None => {
                            *failures
                                .entry(format!("name: {:08X} not in the entries", record[0]))
                                .or_default() += 1;
                        }
                    }
                }
            }
        }
        if std::env::var("RESOURCE_TABLE_VERBOSE").is_ok() {
            let histogram = offsets
                .iter()
                .map(|((list, offset), count)| format!("L{list}:{offset}:{count}"))
                .collect::<Vec<_>>()
                .join(" ");
            let lists = by_list
                .iter()
                .map(|(list, (total, good))| format!("L{list} {good}/{total}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "  {}: {index} entries [{lists}] {histogram}",
                std::path::Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
        }
    }

    println!("{sections} section(s): {checked} 7-word resource records, {matched} match the rule");
    for (failure, count) in failures.iter().take(10) {
        println!("  {failure} x{count}");
    }
    Ok(())
}
