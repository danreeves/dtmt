//! Extracts a section's pass table - one `(head, seam, mask)` per pixel program -
//! from its device data, and optionally prints it as Rust arrays. Walks the
//! programs' own tail boundaries (`parse_programs`), so it works on any family.
//!
//! ```text
//! family_table <material data file>... [--rust]
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Stage};

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rust = args.iter().any(|a| a == "--rust");
    for path in args.iter().filter(|a| *a != "--rust") {
        let data = fs::read(path)?;
        let Some(sec) = section(&data) else { continue };
        let Ok(parsed) = shader::Section::parse(sec) else {
            continue;
        };
        let device = parsed.device_data();
        let Ok(programs) = shader::parse_programs(device) else {
            continue;
        };
        // The preamble body: bytes 12.. of the device region up to the first
        // program. Its length is the family's, so it is whatever precedes the
        // first program minus the 12-byte head.
        let first = programs.first().map(|p| p.pos).unwrap_or(0);
        let body_len = first.saturating_sub(12);

        let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
        let mut heads = Vec::new();
        let mut masks = Vec::new();
        let mut seams = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            if program.stage != Stage::Pixel {
                continue;
            }
            let start = program.meta_pos + 16;
            let end = programs.get(index + 1).map_or(device.len(), |next| next.pos);
            let tail = device.get(start..end).unwrap_or_default();
            // The block is `head + body`; find the body by its first bytes, then
            // derive the head length, the seam word and the mask.
            let body_probe = device.get(12..76).unwrap_or_default();
            let body_at = tail.windows(body_probe.len()).position(|w| w == body_probe);
            match body_at {
                Some(at) => {
                    heads.push(at as u8);
                    seams.push(tail.get(at - 4).copied().unwrap_or(0));
                    masks.push(tail.get(at + 477).copied().unwrap_or(0x07));
                }
                None => {
                    // A block with no body (the family's last program).
                    heads.push(tail.len() as u8);
                    seams.push(0);
                    masks.push(0x07);
                }
            }
        }
        if rust {
            println!("// {name}: body {body_len} B");
            println!("const HEADS: [u8; {}] = {:?};", heads.len(), heads);
            println!("const SEAMS: [u8; {}] = {:?};", seams.len(), seams);
            println!("const MASKS: [u8; {}] = {:?};", masks.len(), masks);
        } else {
            println!(
                "{name}: body {body_len}, {} pixel, heads {:?}",
                heads.len(),
                &heads[..heads.len().min(24)]
            );
            println!("    seams {:?}", &seams[..seams.len().min(24)]);
            println!("    masks {:?}", &masks[..masks.len().min(24)]);
        }
    }
    Ok(())
}
