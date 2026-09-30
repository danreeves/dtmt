//! Counts a section's contexts, queries, groups and programs, per stage, to see
//! whether programs come in fixed-size groups (RainbowFlame's profiles show 32
//! programs over 8 resource groups with the colour pixel shaders at 1, 5, 9...).
//!
//! ```text
//! program_census <material data file | .raw section>...
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader::{self, Section, Stage};

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

fn main() -> Result<(), Box<dyn Error>> {
    let mut ratios: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for path in std::env::args().skip(1) {
        let data = fs::read(&path)?;
        let Some(bytes) = section_bytes(&data) else {
            continue;
        };
        let Ok(section) = Section::parse(bytes) else {
            continue;
        };
        let queries: usize = section
            .contexts()
            .iter()
            .map(|context| context.queries.len())
            .sum();
        let contexts = section.contexts().len();
        #[allow(clippy::redundant_closure_for_method_calls)]
        let queries_with_contexts = section
            .contexts()
            .iter()
            .filter(|context| !context.queries.is_empty())
            .count();
        let programs = shader::parse_programs(section.device_data())?;
        let vertex = programs.iter().filter(|p| p.stage == Stage::Vertex).count();
        let pixel = programs.iter().filter(|p| p.stage == Stage::Pixel).count();
        let others = programs.len() - vertex - pixel;
        let ratio = if queries == 0 {
            format!("{}/0", programs.len())
        } else {
            format!(
                "{:.2}",
                programs.len() as f64 / queries as f64
            )
        };
        *ratios.entry(ratio).or_default() += 1;
        println!(
            "{}: contexts {contexts} ({queries_with_contexts} with queries), queries {queries}, programs {} (V {vertex} P {pixel} other {others}), programs/group {:.2}",
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default(),
            programs.len(),
            programs.len() as f64 / queries.max(1) as f64,
        );
    }
    println!("== programs-per-group distribution:");
    let mut list: Vec<_> = ratios.into_iter().collect();
    list.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (ratio, count) in list.iter().take(12) {
        println!("  {ratio} x{count}");
    }
    Ok(())
}
