//! Prints each program of a section as `stage, tail length, tail hex` - the
//! per-program record list a family's generator needs. One family per file.
//!
//! ```text
//! program_records <material data file>...
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
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
        println!("{}: {} programs", path.rsplit(['\\', '/']).next().unwrap_or(&path), programs.len());
        for (index, program) in programs.iter().enumerate() {
            let start = program.meta_pos + 16;
            let end = programs.get(index + 1).map_or(device.len(), |next| next.pos);
            let tail = device.get(start..end).unwrap_or_default();
            // Print a prefix only: the tails are large and repetitive.
            let shown = tail.len().min(64);
            println!(
                "  {index:3} {:?} {} B  {}",
                program.stage,
                tail.len(),
                hex(&tail[..shown])
            );
        }
    }
    Ok(())
}
