//! Rebuilds a group data from its template: `GroupData::template` cuts the
//! carried bytes out of a shipped group data, `GroupData::build` puts them back
//! around the material's own record tables. The rebuild must be byte-identical
//! on the template's own tables, and a rename must move exactly the bytes it
//! should - which is the evidence a round trip cannot give.
//!
//! ```text
//! group_build <engine_data file | material data file>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::{GroupData, Record};

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: group_build <file>")?;
    let bytes = fs::read(&path)?;

    let (group_data, query_ids) = if let Ok(text) = std::str::from_utf8(&bytes) {
        let line = |key: &str| -> Option<String> {
            text.lines()
                .find_map(|line| line.trim().strip_prefix(key))
                .map(str::to_string)
        };
        let group = unhex(&line("group_data ").ok_or("no group_data line")?);
        let contexts = unhex(&line("contexts ").ok_or("no contexts line")?);
        let mut ids = Vec::new();
        let mut at = 0usize;
        while at + 12 <= contexts.len() {
            let name = u32_at(&contexts, at);
            let count = u32_at(&contexts, at + 8) as usize;
            if name == 0 || count > 64 || at + 12 + count * 8 > contexts.len() {
                break;
            }
            for query in 0..count {
                ids.push(u32_at(&contexts, at + 12 + query * 8));
            }
            at += 12 + count * 8;
        }
        (group, ids)
    } else {
        // A material data file: the shader section is at the header's offset.
        let offset = u32_at(&bytes, 12) as usize;
        let size = u32_at(&bytes, 16) as usize;
        let section = sdk::filetype::shader::Section::parse(
            bytes.get(offset..offset + size).ok_or("shader section out of range")?,
        )?;
        let ids = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        (section.group_data().to_vec(), ids)
    };

    let group = GroupData::new(group_data);
    let template = group.template(&query_ids).ok_or("no template")?;
    let tables: Vec<Vec<Record>> = group
        .object_tables(&query_ids)
        .ok_or("no material tables")?
        .into_iter()
        .map(|(_, records)| records)
        .collect();

    println!(
        "{} groups, template: prefix {} bytes, engine {} records, mid {} bytes per group",
        template.groups.len(),
        template.prefix.len(),
        template.engine.len(),
        template.groups[0].mid.len()
    );
    let head_bytes: usize = template.groups.iter().map(|parts| parts.head.len()).sum();
    let between_bytes: usize = template.groups.iter().map(|parts| parts.between.len()).sum();
    let mid_bytes: usize = template.groups.iter().map(|parts| parts.mid.len()).sum();
    let tail_bytes: usize = template.groups.iter().map(|parts| parts.tail.len()).sum();
    println!(
        "carried bytes: heads {head_bytes}, between {between_bytes}, mid {mid_bytes}, \
         tails {tail_bytes}; material records {}",
        tables.iter().map(Vec::len).sum::<usize>()
    );

    // 1. The rebuild with the template's own tables.
    let rebuilt = GroupData::build(&template, &tables, &template.engine)?;
    let identical = rebuilt == group.bytes();
    println!(
        "rebuild: {} bytes, {}",
        rebuilt.len(),
        if identical {
            "byte-identical"
        } else {
            "DIFFERS"
        }
    );
    if !identical {
        let diff: Vec<usize> = (0..rebuilt.len().min(group.bytes().len()))
            .filter(|at| rebuilt[*at] != group.bytes()[*at])
            .take(20)
            .collect();
        println!(
            "  {} differing byte(s); first at {diff:?}",
            diff_count(&rebuilt, group.bytes())
        );
        return Err("the rebuild changed bytes".into());
    }

    // 2. A rename: one record, then every record of the first channel. The new
    //    hash is a whole word apart from the old one, so the byte counts are
    //    exact: 4 per record written with the new hash.
    let mut renamed = tables.clone();
    renamed[0][0].hash = 0xDEAD_BEEF;
    let rebuilt = GroupData::build(&template, &renamed, &template.engine)?;
    let diff = diff_count(&rebuilt, group.bytes());
    println!("rename one record: {diff} byte(s) changed (expected 4)");

    let mut renamed = tables.clone();
    let channel = tables[0][0].hash;
    let sharing = tables[0].iter().filter(|record| record.hash == channel).count();
    for record in renamed[0].iter_mut().filter(|record| record.hash == channel) {
        record.hash = 0xDEAD_BEEF;
    }
    let rebuilt = GroupData::build(&template, &renamed, &template.engine)?;
    let diff = diff_count(&rebuilt, group.bytes());
    println!(
        "rename the first channel: {diff} byte(s) changed (expected {})",
        4 * sharing
    );
    Ok(())
}

fn diff_count(a: &[u8], b: &[u8]) -> usize {
    (0..a.len().min(b.len())).filter(|at| a[*at] != b[*at]).count()
}
