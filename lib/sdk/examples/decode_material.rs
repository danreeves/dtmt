//! Prints the declarations of a material data file (everything the decompiled
//! SJSON holds before `shader_data`) next to what the shader section's shared
//! block carries for its channels.
//!
//! Usage: `cargo run -p sdk --example decode_material -- <material data file>...`

use sdk::filetype::{material, shader};
use sdk::murmur::Murmur32;
use sdk::{BundleFileVariant, Context};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let ctx = Context::new();

    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path)?;
        let mut variant = BundleFileVariant::new();
        variant.set_data(data.clone());

        println!("=== {path} ===");
        let files = material::decompile(&ctx, "decode.material".to_string(), &variant).await?;
        let sjson = String::from_utf8(files[0].data().to_vec())?;

        let mut channels: Vec<String> = Vec::new();
        let mut in_channels = false;
        for line in sjson.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("shader_data") {
                println!("  shader_data = <{} hex chars omitted>", trimmed.len());
                continue;
            }
            if trimmed.len() > 200 {
                println!("  <{} char line omitted>", trimmed.len());
                continue;
            }
            if trimmed.starts_with("channels = [") {
                in_channels = true;
                continue;
            }
            if in_channels {
                if trimmed.starts_with(']') {
                    in_channels = false;
                    continue;
                }
                channels.push(trimmed.trim_matches(|c| c == '"' || c == ',').to_string());
            }
            println!("  {trimmed}");
        }

        let Some(section) = shader_section(&data) else {
            continue;
        };
        let device_offset = u32_at(section, 40) as usize;
        let device_size = u32_at(section, 44) as usize;
        let Some(device) = section.get(device_offset..device_offset + device_size) else {
            continue;
        };
        let programs = shader::parse_programs(device)?;
        let preamble_end = programs.first().map(|program| program.pos).unwrap_or(0);
        let preamble = &device[..preamble_end];
        println!(
            "  preamble {} bytes, channels {:?}",
            preamble.len(),
            channels
        );
        println!(
            "  preamble_hex {}",
            preamble
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        );

        for channel in &channels {
            let hash_hex = format!("{:08X}", Murmur32::hash(channel));
            let hash = u32::from_str_radix(&hash_hex, 16).unwrap_or(0);
            let needle = hash.to_le_bytes();
            let mut found = 0;
            for at in 0..preamble.len().saturating_sub(3) {
                if preamble[at..at + 4] != needle {
                    continue;
                }
                found += 1;
                let start = at.saturating_sub(12);
                let end = (at + 16).min(preamble.len());
                let words: Vec<String> = preamble[start..end]
                    .chunks_exact(4)
                    .map(|word| format!("{:08X}", u32::from_le_bytes(word.try_into().unwrap())))
                    .collect();
                let word_of_hash = (at - start) / 4;
                let marked: Vec<String> = words
                    .iter()
                    .enumerate()
                    .map(|(i, word)| {
                        if i == word_of_hash {
                            format!("[{word}]")
                        } else {
                            word.clone()
                        }
                    })
                    .collect();
                println!("    {channel} 0x{hash:08X} at byte {at}: {}", marked.join(" "));
            }
            if found == 0 {
                println!("    {channel} 0x{hash:08X}: not in the block");
            }
        }
    }

    Ok(())
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Returns the shader section of a material stream.
fn shader_section(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 28 {
        return None;
    }
    let offset = u32_at(data, 12) as usize;
    let size = u32_at(data, 16) as usize;
    data.get(offset..offset + size)
}
