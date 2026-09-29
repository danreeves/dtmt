//! Prints the part map of a group data: per group, where it starts, which
//! canonical runs and packed runs it holds, where its condition header sits, and
//! which bytes no reader covers. That gap list is the group data constructor's
//! worklist.
//!
//! ```text
//! group_parts <engine_data file | material data file>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupData;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

const PACKED_KEY: u32 = 0xB5639618;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: group_parts <file>")?;
    let bytes = fs::read(&path)?;

    let (group_data, query_ids) = if let Ok(text) = std::str::from_utf8(&bytes) {
        let line = |key: &str| -> Option<String> {
            text.lines()
                .find_map(|line| line.trim().strip_prefix(key))
                .map(str::to_string)
        };
        let unhex = |hex: &str| -> Vec<u8> {
            hex.as_bytes()
                .chunks(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
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
        let offset = u32_at(&bytes, 32) as usize;
        let size = u32_at(&bytes, 36) as usize;
        let section = sdk::filetype::shader::Section::parse(&bytes[..])?;
        let group = section.group_data().to_vec();
        let ids = section
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let _ = (offset, size);
        (group, ids)
    };

    let group = GroupData::new(group_data);
    let starts = group
        .group_starts(&query_ids)
        .ok_or("the groups do not walk")?;
    let headers = group.condition_headers(&query_ids);

    println!(
        "{} bytes, {} groups, header {} bytes",
        group.bytes().len(),
        starts.len(),
        32 + 48
    );
    for (index, start) in starts.iter().enumerate() {
        let start = *start;
        let end = starts.get(index + 1).copied().unwrap_or(group.bytes().len());
        let mut covered: Vec<(usize, usize)> = Vec::new();

        // Canonical runs.
        let mut at = start;
        let mut runs = Vec::new();
        while at + 40 <= end {
            let run = group.run_at(at);
            if run.is_empty() {
                at += 1;
                continue;
            }
            runs.push((at, run.len() * 20));
            covered.push((at, at + run.len() * 20));
            at += run.len() * 20;
        }

        // Packed runs: a record's sixth word is the key.
        let mut packed = Vec::new();
        let mut at = start;
        while at + 28 <= end {
            if u32_at(group.bytes(), at + 20) == PACKED_KEY {
                let run_start = at;
                while at + 28 <= end && u32_at(group.bytes(), at + 20) == PACKED_KEY {
                    at += 28;
                }
                packed.push((run_start, at - run_start));
                covered.push((run_start, at));
            } else {
                at += 1;
            }
        }

        // The condition header, from the end of the group.
        let header = headers.as_ref().and_then(|headers| headers.get(index));
        let cond = header.map(|header| (end - header.len(), header.len()));
        if let Some(cond) = cond {
            covered.push(cond);
        }

        covered.sort();
        let mut gaps = Vec::new();
        let mut cursor = start;
        for (from, to) in covered {
            if from > cursor {
                gaps.push((cursor, from));
            }
            cursor = cursor.max(to);
        }
        if cursor < end {
            gaps.push((cursor, end));
        }

        println!("group {index:2} [{start}..{end}] len={}", end - start);
        println!(
            "   runs={:?}",
            runs.iter()
                .map(|(at, len)| format!("{at}+{len}"))
                .collect::<Vec<_>>()
        );
        println!(
            "   packed={:?} cond={:?}",
            packed
                .iter()
                .map(|(at, len)| format!("{at}+{len}"))
                .collect::<Vec<_>>(),
            cond.map(|(at, len)| format!("{at}+{len}"))
        );
        if !gaps.is_empty() {
            let hex = |from: usize, to: usize| {
                let mut out = String::new();
                for byte in &group.bytes()[from..to] {
                    out.push_str(&format!("{byte:02X}"));
                }
                out
            };
            for (from, to) in gaps.iter().take(6) {
                println!(
                    "   gap {from}..{to} ({} bytes) {}{}",
                    to - from,
                    hex(*from, (*from + 24).min(*to)),
                    if to - from > 24 { "..." } else { "" }
                );
            }
        }
    }
    Ok(())
}
