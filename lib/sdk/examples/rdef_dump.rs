//! Dumps the RDEF (resource definition) chunk of the containers inside a shader
//! section: the constant buffers and bound resources with their names, bind
//! points and spaces - the reflection the tail lists are to derive from.
//!
//! ```text
//! rdef_dump <material data file | .raw section>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section};

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The RDEF chunk of a container: through the DXBC chunk table when it has the
/// magic, by searching for the fourcc otherwise.
fn rdef(container: &[u8]) -> Option<&[u8]> {
    if container.get(..4)? == b"DXBC" {
        let count = u32_at(container, 28)? as usize;
        for index in 0..count {
            let offset = u32_at(container, 32 + index * 4)? as usize;
            let fourcc = container.get(offset..offset + 4)?;
            let size = u32_at(container, offset + 4)? as usize;
            if fourcc == b"RDEF" {
                return container.get(offset + 8..offset + 8 + size);
            }
        }
        return None;
    }
    let at = container
        .windows(4)
        .position(|window| window == b"RDEF")?;
    let size = u32_at(container, at + 4)? as usize;
    container.get(at + 8..at + 8 + size)
}

fn name(rdef: &[u8], offset: u32) -> String {
    let bytes = rdef.get(offset as usize..).unwrap_or_default();
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

/// The D3D_SIT_* resource kinds the bound resource table uses.
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
        // A material data file carries its section at the header's offset; a
        // `.raw` section is wrapped by 20 bytes.
        let bytes: &[u8] = if data.len() >= 28
            && (60..=62).contains(&u32_at(&data, 0).unwrap_or(0))
            && u32_at(&data, 4) == Some(28)
        {
            let offset = u32_at(&data, 12).unwrap_or(0) as usize;
            let size = u32_at(&data, 16).unwrap_or(0) as usize;
            data.get(offset..offset + size).ok_or("the section is out of range")?
        } else {
            data.get(20..).ok_or("too small")?
        };
        let section = Section::parse(bytes)?;
        let programs = shader::parse_programs(section.device_data())?;
        println!("=== {path} ({} programs)", programs.len());
        for program in &programs {
            let Some(chunk) = rdef(&program.container) else {
                let words: Vec<String> = (0..12)
                    .map(|index| format!("{:08X}", u32_at(&program.container, index * 4).unwrap_or(0)))
                    .collect();
                let chunks: Vec<String> = (0..8)
                    .filter_map(|index| {
                        let offset = u32_at(&program.container, 32 + index * 4)? as usize;
                        let fourcc = program.container.get(offset..offset + 4)?;
                        Some(String::from_utf8_lossy(fourcc).to_string())
                    })
                    .collect();
                println!(
                    "  {:?} container {} bytes, header {words:?}, chunks {chunks:?}",
                    program.stage,
                    program.container.len()
                );
                continue;
            };
            println!("  {:?} container {} bytes:", program.stage, program.container.len());
            let constant_buffers = u32_at(chunk, 0).unwrap_or(0);
            let constant_buffers_at = u32_at(chunk, 4).unwrap_or(0) as usize;
            let resources = u32_at(chunk, 8).unwrap_or(0);
            let resources_at = u32_at(chunk, 12).unwrap_or(0) as usize;
            let target = u32_at(chunk, 16).unwrap_or(0);
            let flags = u32_at(chunk, 20).unwrap_or(0);
            let creator = u32_at(chunk, 24).unwrap_or(0);
            let version = u32_at(chunk, 28).unwrap_or(0);
            println!(
                "    header: {constant_buffers} cbuffers at {constant_buffers_at}, \
                 {resources} resources at {resources_at}, target {target:#010x}, flags {flags:#x}, \
                 creator {creator}, version {version:#x}, chunk {} bytes",
                chunk.len()
            );
            println!("    creator: {:?}", name(chunk, creator));
            for index in 0..constant_buffers as usize {
                let at = constant_buffers_at + index * 24;
                let (Some(name_at), Some(size), Some(variables)) =
                    (u32_at(chunk, at), u32_at(chunk, at + 12), u32_at(chunk, at + 4))
                else {
                    break;
                };
                println!(
                    "    cbuffer {}: {:?} size {size}, {variables} variables",
                    index,
                    name(chunk, name_at)
                );
            }
            // The bound resource record's stride is version-dependent; check
            // which one fits the chunk, then read the first records with it.
            let strides = [24usize, 28, 32];
            let fits = |stride: usize| resources_at + resources as usize * stride <= chunk.len();
            let stride = strides.iter().copied().find(|stride| fits(*stride)).unwrap_or(0);
            println!(
                "    strides that fit: {:?}; using {stride}",
                strides
                    .iter()
                    .copied()
                    .filter(|stride| fits(*stride))
                    .collect::<Vec<_>>()
            );
            let raw = chunk
                .get(resources_at..resources_at + 64)
                .unwrap_or_default()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!("    first bytes: {raw}");
            for index in 0..resources.min(8) as usize {
                let at = resources_at + index * stride;
                let Some(name_at) = u32_at(chunk, at) else {
                    break;
                };
                let kind = u32_at(chunk, at + 4).unwrap_or(0);
                let bind_point = u32_at(chunk, at + 8).unwrap_or(0);
                let bind_count = u32_at(chunk, at + 12).unwrap_or(0);
                let flags = u32_at(chunk, at + 16).unwrap_or(0);
                let space = u32_at(chunk, at + 20).unwrap_or(0);
                println!(
                    "    resource {}: {:?} kind {} ({}) bind {bind_point} count {bind_count} \
                     flags {flags:#x} space {space}",
                    index,
                    name(chunk, name_at),
                    kind,
                    kind_name(kind)
                );
                if stride >= 28 {
                    println!("      (words 6/7: {:08X} {:08X})", u32_at(chunk, at + 24).unwrap_or(0), u32_at(chunk, at + 28).unwrap_or(0));
                }
            }
            break; // one program per stage is enough to see the shape
        }
    }
    Ok(())
}
