//! Checks an engine data file end to end: parses it, re-serializes it, generates
//! the section from the carried containers and compares the group data with the
//! source material's section. That covers both the text round trip of the
//! group-data template and the rebuild it feeds.
//!
//! ```text
//! engine_data_check <engine_data file> [<source material data file>]
//! ```

use std::collections::HashMap;
use std::error::Error;
use std::fs;

use sdk::filetype::shader_engine_data::EngineData;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: engine_data_check <file>")?;
    let source = std::env::args().nth(2);
    let text = fs::read_to_string(&path)?;

    let engine_data = EngineData::from_text(&text)?;
    println!(
        "{} bytes of text; group template: {}; material tables: {}",
        text.len(),
        if engine_data.group_template.is_some() {
            "yes"
        } else {
            "no (carried group data)"
        },
        engine_data.material_tables.len()
    );

    // The text round trip, then the section it generates.
    let again = EngineData::from_text(&engine_data.to_text())?;
    let section = again.generate(&HashMap::new(), "materials/test/base", &HashMap::new())?;
    let group_offset = u32_at(&section, 32) as usize;
    let group_size = u32_at(&section, 36) as usize;
    let generated = &section[group_offset..group_offset + group_size];
    println!("generated section {} bytes, group data {} bytes", section.len(), generated.len());

    if let Some(source) = source {
        let data = fs::read(&source)?;
        let offset = u32_at(&data, 12) as usize;
        let size = u32_at(&data, 16) as usize;
        let original_section = &data[offset..offset + size];
        let original_offset = u32_at(original_section, 32) as usize;
        let original_size = u32_at(original_section, 36) as usize;
        let original = &original_section[original_offset..original_offset + original_size];
        println!(
            "source group data {} bytes: {}",
            original.len(),
            if generated == original {
                "identical"
            } else {
                "DIFFERS"
            }
        );
        if generated != original {
            let diff = (0..generated.len().min(original.len()))
                .filter(|at| generated[*at] != original[*at])
                .count();
            return Err(format!("{diff} differing byte(s)").into());
        }
    }
    Ok(())
}
