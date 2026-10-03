//! Reports a section's pass structure: the contexts, the program count per
//! stage, and the distinct `(head length, mask byte)` pairs its pixel programs
//! use, so different shader families can be compared.
//!
//! ```text
//! family_probe <material data file>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn section(data: &[u8]) -> Option<&[u8]> {
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
        let Some(sec) = section(&data) else { continue };
        let Ok(parsed) = sdk::filetype::shader::Section::parse(sec) else {
            continue;
        };
        let contexts = parsed.contexts();
        let queries: usize = contexts.iter().map(|c| c.queries.len()).sum();
        let programs = sdk::filetype::shader::parse_programs(parsed.device_data())
            .map(|p| p.len())
            .unwrap_or(0);
        // The device data starts with the preamble; the body is [12..].
        let device = parsed.device_data();
        let body = device.get(12..561).unwrap_or_default();
        // Count the distinct mask bytes around the body in the device data.
        let before = body.get(..477).unwrap_or_default();
        let after = body.get(478..).unwrap_or_default();
        let mut masks: BTreeMap<u8, usize> = BTreeMap::new();
        let mut at = 0;
        while let Some(pos) = device[at..]
            .windows(before.len().max(1))
            .position(|w| w == before)
        {
            let start = at + pos;
            let mask_at = start + 477;
            if device.get(mask_at + 1..mask_at + 1 + after.len()) == Some(after)
                && let Some(byte) = device.get(mask_at)
            {
                *masks.entry(*byte).or_default() += 1;
            }
            at = start + 1;
        }
        let context_names: Vec<String> = contexts.iter().map(|c| format!("{:08X}", c.name)).collect();
        println!(
            "{}: material_hash {:08X}, contexts [{}], {} queries, {} programs, masks {:?}",
            path.rsplit(['\\', '/']).next().unwrap_or(&path),
            u32_at(sec, 4),
            context_names.join(" "),
            queries,
            programs,
            masks
        );
    }
    Ok(())
}
