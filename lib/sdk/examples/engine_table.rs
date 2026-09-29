//! Prints the engine's `global_viewport` record table from a material data file
//! or an engine-data text file: the records' index, name hash, offset and size.
//! The device preamble's config records index into the engine's variable order,
//! so this is the table to read them against.
//!
//! ```text
//! engine_table <material data file | engine_data file>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: engine_table <file>")?;
    let bytes = fs::read(&path)?;

    // An engine-data text file carries the group data as hex; a material data
    // file carries a section.
    let group_data = if let Ok(text) = std::str::from_utf8(&bytes) {
        let line = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("group_data "))
            .ok_or("no group_data line")?;
        let mut data = Vec::with_capacity(line.len() / 2);
        for pair in line.as_bytes().chunks(2) {
            data.push(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?);
        }
        data
    } else {
        let offset = u32_at(&bytes, 32) as usize;
        let size = u32_at(&bytes, 36) as usize;
        bytes[offset..offset + size].to_vec()
    };

    let group = GroupData::new(group_data);
    let Some(records) = group.engine_records() else {
        return Err("the engine table was not found".into());
    };
    println!("{} engine record(s)", records.len());
    for (index, record) in records.iter().enumerate() {
        println!(
            "{index:3} hash={:08X} offset={} size={}",
            record.hash, record.offset, record.size
        );
    }
    Ok(())
}
