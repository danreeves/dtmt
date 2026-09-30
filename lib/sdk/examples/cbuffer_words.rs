//! Collects every tail constant-buffer entry's words by name hash, to see
//! whether the words beside the name and size (1, 3, 4 and 5) are constants per
//! name or program-specific. Prints one line per name with the distinct tuples.
//!
//! ```text
//! cbuffer_words <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

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
    // name -> word 1/3/4/5 tuple -> count
    let mut by_name: BTreeMap<u32, BTreeMap<(u32, u32, u32, u32), usize>> = BTreeMap::new();
    let mut sections = 0;
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        sections += 1;
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
                let tuple = (entry.words[1], entry.words[3], entry.words[4], entry.words[5]);
                *by_name
                    .entry(entry.name_hash())
                    .or_default()
                    .entry(tuple)
                    .or_default() += 1;
            }
        }
    }

    println!("{sections} section(s)");
    for (name, tuples) in &by_name {
        let total: usize = tuples.values().sum();
        let distinct = tuples.len();
        println!("  {name:08X}: {total} entries, {distinct} distinct tuple(s)");
        for (tuple, count) in tuples.iter().take(4) {
            println!(
                "      words 1/3/4/5 = {:08X}/{:08X}/{:08X}/{:08X} x{count}",
                tuple.0, tuple.1, tuple.2, tuple.3
            );
        }
    }
    Ok(())
}
