//! Walks a section's conditions blob as the tree it is and prints every leaf's
//! path, then hashes the paths and compares them with the section's queries and
//! group ids - testing whether the queries are derived from the condition tree.
//!
//! ```text
//! conditions_walk <material data file>
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::shader::Section;
use sdk::murmur::{Murmur32, Murmur64};

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

/// One conditions record: the hashes it tests and the flagged children it
/// selects between.
struct Record {
    hashes: Vec<u32>,
    children: Vec<u16>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: conditions_walk <data>")?;
    let data = fs::read(&path)?;
    let offset = u32_at(&data, 12) as usize;
    let size = u32_at(&data, 16) as usize;
    let section = Section::parse(&data[offset..offset + size])?;

    // Parse every record of the conditions blob: `{u16 tag, u16 words, u16
    // payload_offset, u16 count}` then `count` hashes then `words` u16s.
    let conditions = section.conditions();
    let mut records = Vec::new();
    let mut at = 0usize;
    while at + 8 <= conditions.len() {
        let tag = u16_at(conditions, at);
        let words = u16_at(conditions, at + 2) as usize;
        let payload_offset = u16_at(conditions, at + 4) as usize;
        let count = u16_at(conditions, at + 6) as usize;
        if tag != 1 {
            break;
        }
        let mut hashes = Vec::new();
        for index in 0..count {
            hashes.push(u32_at(conditions, at + 8 + index * 4));
        }
        let mut children = Vec::new();
        for index in 0..words {
            children.push(u16_at(conditions, at + payload_offset + index * 2));
        }
        records.push(Record { hashes, children });
        at += payload_offset + words * 2;
    }
    println!("{} records", records.len());

    // Walk depth first from every record that nothing points at; a leaf is a
    // record with no children. Print the path of hashes (and the flagged
    // children) for the first records.
    for (index, record) in records.iter().enumerate() {
        let flags: Vec<u16> = record.children.iter().map(|child| child >> 12).collect();
        let children: Vec<u16> = record.children.iter().map(|child| child & 0xFFF).collect();
        println!(
            "  record {index}: {} hashes, {} children (flags {flags:?} -> {children:?})",
            record.hashes.len(),
            record.children.len()
        );
        if index > 6 {
            println!("  ...");
            break;
        }
    }

    // The queries and group ids, for the comparison.
    let queries: Vec<u32> = section
        .contexts()
        .iter()
        .flat_map(|context| context.queries.iter().map(|query| query.id))
        .collect();
    let mut hits: BTreeMap<String, usize> = BTreeMap::new();

    // Every record's hash sequence, hashed in several ways, against the query
    // ids.
    for record in &records {
        for (what, bytes) in [
            ("hashes", record.hashes.iter().flat_map(|h| h.to_le_bytes()).collect::<Vec<u8>>()),
            ("hashes big", record.hashes.iter().flat_map(|h| h.to_be_bytes()).collect::<Vec<u8>>()),
            (
                "children",
                record.children.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>(),
            ),
        ] {
            let short = u32::from(Murmur32::hash(&bytes));
            let long = u64::from(Murmur64::hash(&bytes));
            if queries.contains(&short) {
                *hits.entry(format!("{what}: murmur32 = {short:08X}"))
                    .or_default() += 1;
            }
            let low = long as u32;
            let high = (long >> 32) as u32;
            if queries.contains(&low) {
                *hits.entry(format!("{what}: murmur64 low = {low:08X}")).or_default() += 1;
            }
            if queries.contains(&high) {
                *hits.entry(format!("{what}: murmur64 high = {high:08X}")).or_default() += 1;
            }
        }
    }

    println!("queries: {}", queries.len());
    for (hit, count) in &hits {
        println!("  {hit} x{count}");
    }
    if hits.is_empty() {
        println!("  no query matches any record's hashes or children");
    }
    Ok(())
}
