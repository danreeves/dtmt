//! Prints a section's device preamble header and length, then each program's
//! tail length, so a family's device framing can be measured (the UI base's
//! preamble is 561 bytes with a 549-byte body; other families differ).
//!
//! ```text
//! device_shape <material data file>...
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader;

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
        let device = parsed.device_data();
        let Ok(programs) = shader::parse_programs(device) else {
            continue;
        };
        let first = programs.first().map(|p| p.pos).unwrap_or(0);
        let last_end = programs.last().map(|p| p.meta_pos + 16).unwrap_or(0);
        // Header words of the device region before the first program.
        let head: Vec<String> = (0..(first.min(32) / 4))
            .map(|i| format!("{:08X}", u32_at(device, i * 4)))
            .collect();
        println!(
            "{}: device {} B, first program at {}, {} programs, pre-program {} B {:?}",
            path.rsplit(['\\', '/']).next().unwrap_or(&path),
            device.len(),
            first,
            programs.len(),
            first,
            head
        );
        let tails: Vec<usize> = programs
            .iter()
            .enumerate()
            .map(|(index, program)| {
                let end = programs.get(index + 1).map_or(device.len(), |next| next.pos);
                end.saturating_sub(program.meta_pos + 16)
            })
            .collect();
        let _ = last_end;
        println!("    tails (first 16): {tails:?}");
    }
    Ok(())
}
