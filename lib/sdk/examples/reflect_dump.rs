//! Dumps what the compiled containers bind, per stage, through DXC's own
//! reflection (`IDxcUtils::CreateReflection`) - the source the engine's tail
//! lists are derived from, per stage and without reading the HLSL text.
//!
//! ```text
//! reflect_dump <material data file | .raw section>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section};

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn section_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() >= 28
        && (60..=62).contains(&u32_at(data, 0)?)
        && u32_at(data, 4) == Some(28)
    {
        let offset = u32_at(data, 12)? as usize;
        let size = u32_at(data, 16)? as usize;
        return data.get(offset..offset + size);
    }
    data.get(20..)
}

fn kind_name(kind: u32) -> &'static str {
    match kind {
        0 => "cbuffer",
        1 => "tbuffer",
        2 => "texture",
        3 => "sampler",
        4 => "uav-rwtyped",
        5 => "structured",
        6 => "uav-rwstructured",
        7 => "byteaddress",
        8 => "uav-rwbyteaddress",
        9 => "uav-append",
        10 => "uav-consume",
        11 => "uav-rwstructured-counter",
        _ => "?",
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let bytes = section_bytes(&data).ok_or("not a section")?;
        let section = Section::parse(bytes)?;
        let programs = shader::parse_programs(section.device_data())?;
        println!("=== {path} ({} programs)", programs.len());
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for program in &programs {
            if seen.iter().any(|other| *other == program.container) {
                continue;
            }
            seen.push(program.container.clone());
            println!(
                "  {:?} container {} bytes:",
                program.stage,
                program.container.len()
            );
            match dxc::reflect(&program.container) {
                Ok(bindings) => {
                    for binding in &bindings {
                        println!(
                            "    {:?} kind {} ({}) bind {} count {} flags {:#x} space {}",
                            binding.name,
                            binding.kind,
                            kind_name(binding.kind),
                            binding.bind_point,
                            binding.bind_count,
                            binding.flags,
                            binding.space
                        );
                    }
                }
                Err(err) => println!("    reflection failed: {err}"),
            }
        }
    }
    Ok(())
}
