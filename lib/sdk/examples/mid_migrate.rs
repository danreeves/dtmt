//! Migrates an engine data file to carry each group's channels instead of its
//! `mid`: the build derives the `mid` from them.
//!
//! A group's channels are the names its packed copies reference, in first-seen
//! order - the copies name channels and nothing else, so they are the reliable
//! source (a group's `mid` may carry only one of them).
//!
//! ```text
//! mid_migrate <engine_data>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::mid_sections;
use sdk::filetype::shader_engine_data::EngineData;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: mid_migrate <engine_data>")?;
    let mut engine_data = EngineData::from_text(&fs::read_to_string(&path)?)?;
    let template = engine_data
        .group_template
        .as_mut()
        .ok_or("no template in the file")?;

    for (index, parts) in template.groups.iter_mut().enumerate() {
        let mut channels: Vec<u32> = Vec::new();
        if let Some(copies) = mid_sections(&parts.mid).and_then(|mut sections| sections.pop()) {
            for row in copies.records.chunks_exact(28) {
                let hash = u32::from_le_bytes(row[0..4].try_into().unwrap());
                if !channels.contains(&hash) {
                    channels.push(hash);
                }
            }
        }
        println!("group {index}: channels {channels:08X?}");
        parts.channels = channels;
        parts.mid = Vec::new();
    }

    fs::write(&path, engine_data.to_text())?;
    println!("wrote {path}");
    Ok(())
}
