//! Dumps the per-program block's header/body boundary for a shipped or generated
//! section: the block is `header + preamble-body`, and the header length is
//! where the body's first bytes re-appear. Prints the header bytes, the bytes
//! right after them, the mask byte, and dedups the headers seen.
//!
//! ```text
//! block_grammar <material data file | .raw section>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section, Stage, Tail, TailLists};

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

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        let device = section.device_data();
        let programs = shader::parse_programs(device)?;
        let preamble_end = programs.first().map(|p| p.pos).unwrap_or(0);
        let body = device.get(12..preamble_end).unwrap_or_default().to_vec();
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        println!(
            "== {name}: {} programs, preamble {} B, body {} B",
            programs.len(),
            preamble_end,
            body.len()
        );

        let mut headers: BTreeMap<(usize, String), usize> = BTreeMap::new();
        for (index, program) in programs.iter().enumerate() {
            if program.stage != Stage::Pixel {
                continue;
            }
            let end = programs
                .get(index + 1)
                .map(|next| next.pos)
                .unwrap_or(device.len());
            let Some(tail) = device.get(program.meta_pos + 16..end) else {
                continue;
            };
            let Some(parsed) = Tail::parse(tail) else {
                continue;
            };
            let Some(lists) = TailLists::parse(&parsed.rest) else {
                continue;
            };
            let block = &lists.block;
            // Find the body by a prefix (the body's mask byte is patched per
            // program, so the whole body never matches).
            let probe_len = body.len().min(16);
            let at = if probe_len == 0 {
                Some(0)
            } else {
                block
                    .windows(probe_len)
                    .position(|w| w == &body[..probe_len])
            };
            let head_len = at.unwrap_or(usize::MAX);
            let head = if head_len == usize::MAX {
                block.to_vec()
            } else {
                block[..head_len].to_vec()
            };
            let after = if head_len == usize::MAX {
                Vec::new()
            } else {
                block[head_len..(head_len + 16).min(block.len())].to_vec()
            };
            let mask = if head_len == usize::MAX {
                None
            } else {
                block.get(head_len + 477).copied()
            };
            // The bytes the block changes from the body (offset, value).
            let diffs: Vec<String> = if head_len == usize::MAX {
                Vec::new()
            } else {
                let body_part = &block[head_len..];
                (0..body.len().min(body_part.len()))
                    .filter(|at| body_part[*at] != body[*at])
                    .map(|at| format!("{at}={:02X}", body_part[at]))
                    .collect()
            };
            println!(
                "  p{index:2} tail {:4} block {:4} head {:3} cb {:2} mask {:?} diffs[{}] head={} body={}",
                tail.len(),
                block.len(),
                if head_len == usize::MAX {
                    -1
                } else {
                    head_len as isize
                },
                parsed.cbuffers.len(),
                mask.map(|m| format!("{m:02X}")),
                diffs.join(" "),
                hex(&head),
                hex(&after),
            );
            *headers
                .entry((head_len, hex(&head[..head.len().min(8)])))
                .or_default() += 1;
        }
        println!("  distinct headers (len, bytes) x count:");
        for ((len, head), count) in &headers {
            println!("    {len:3} x{count:2}  {head}");
        }
    }
    Ok(())
}
