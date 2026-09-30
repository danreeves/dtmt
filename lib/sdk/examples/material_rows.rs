//! Collects the rows of every section's material table (the group data's object
//! table) and counts how often each distinct row appears, to separate the
//! engine's fixed vocabulary from the rows a material brings.
//!
//! ```text
//! material_rows <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;
use sdk::filetype::shader::Section;

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
    // (kind, offset, size) -> name hash -> how many sections carry the row.
    let mut rows: BTreeMap<(u32, u32, u32), BTreeMap<u32, usize>> = BTreeMap::new();
    let mut sections = 0usize;
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        let group = GroupData::new(section.group_data().to_vec());
        let query_ids: Vec<u32> = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let Some(tables) = group.object_tables(&query_ids) else {
            continue;
        };
        let Some((_, records)) = tables.into_iter().next() else {
            continue;
        };
        sections += 1;
        for record in records {
            *rows
                .entry((record.kind, record.offset, record.size))
                .or_default()
                .entry(record.hash)
                .or_default() += 1;
        }
    }

    println!("{sections} section(s)");
    // One line per (kind, offset, size), with the names that appear there and
    // how many sections carry them.
    for ((kind, offset, size), names) in &rows {
        let mut list: Vec<_> = names.iter().collect();
        list.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        let rendered = list
            .iter()
            .take(6)
            .map(|(hash, count)| format!("{hash:08X}x{count}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("  kind {kind:2} offset {offset:5} size {size:3}: {rendered}");
    }
    Ok(())
}
