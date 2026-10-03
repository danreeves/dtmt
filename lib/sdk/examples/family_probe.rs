//! Reports a section's pass table: its contexts, its programs per stage, and
//! each pixel program's mask byte (read from its own tail block at body offset
//! 477), in program order. One family per file, so different families' pass
//! structures can be compared and recorded.
//!
//! ```text
//! family_probe <material data file>...
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
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(sec) = section(&data) else { continue };
        let Ok(parsed) = shader::Section::parse(sec) else {
            continue;
        };
        let contexts: Vec<String> = parsed
            .contexts()
            .iter()
            .map(|c| format!("{:08X}", c.name))
            .collect();
        let device = parsed.device_data();
        let Ok(programs) = shader::parse_programs(device) else {
            continue;
        };
        let preamble = device.get(..561).unwrap_or_default();
        let body_len = preamble.len().saturating_sub(12);

        // Each program's tail is the bytes after its metadata record, up to the
        // next program. The mask byte sits at `tail_body_start + 477`, where the
        // body starts after the lists' count words (4 + 36).
        let mut masks = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let tail_start = program.meta_pos + 16;
            let tail_end = programs.get(index + 1).map_or(device.len(), |next| next.pos);
            let tail = device.get(tail_start..tail_end).unwrap_or_default();
            // The block inside the tail is `head + preamble body`; the body is
            // 549 bytes and the mask sits at body offset 477. Find the body's
            // first 64 bytes in the tail, then read +477.
            let body_probe = device.get(12..76).unwrap_or_default();
            let body_at = tail.windows(body_probe.len()).position(|w| w == body_probe);
            let mask = body_at.and_then(|at| tail.get(at + 477)).copied();
            masks.push(mask);
        }
        let pixel_masks: Vec<String> = programs
            .iter()
            .zip(&masks)
            .filter(|(program, _)| program.stage == Stage::Pixel)
            .map(|(_, mask)| mask.map_or("-".to_string(), |m| format!("{m:02X}")))
            .collect();
        let vertex = programs
            .iter()
            .filter(|program| program.stage == Stage::Vertex)
            .count();
        let pixel = programs.len() - vertex;
        println!(
            "{}: contexts [{}], {} programs ({}V {}P), pixel masks [{}]",
            path.rsplit(['\\', '/']).next().unwrap_or(&path),
            contexts.join(" "),
            programs.len(),
            vertex,
            pixel,
            pixel_masks.join(" ")
        );
    }
    Ok(())
}
