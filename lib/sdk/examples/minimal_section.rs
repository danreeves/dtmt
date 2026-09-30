//! Reduces an engine data to a minimal shape - one context, one query with a
//! chosen id, one group, two programs - so the engine's own log can say which
//! query id it demands. The point is to learn the rule behind the queries
//! instead of carrying them.
//!
//! ```text
//! minimal_section <engine_data> <out> [query-hex]
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::group_data::GroupTemplate;
use sdk::filetype::shader::Stage;
use sdk::filetype::shader_engine_data::EngineData;

/// murmur32 of "default".
const DEFAULT_CONTEXT: u32 = 0xF276_0503;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: minimal_section <engine_data> <out>")?;
    let output = args.next().ok_or("usage: minimal_section <engine_data> <out>")?;
    let query = args
        .next()
        .map(|value| u32::from_str_radix(value.trim_start_matches("0x"), 16))
        .transpose()?
        .unwrap_or(0x5A5A_5A5A);

    let text = fs::read_to_string(&input)?;
    let mut engine_data = EngineData::from_text(&text)?;
    let mode = args.next().unwrap_or_else(|| "min".to_string());
    if mode == "programs" {
        // Only the programs are reduced: the contexts and groups stay as
        // shipped, so the permutation slots the engine asks about are covered
        // by one vertex and one pixel program.
        let vertex = engine_data
            .programs
            .iter()
            .find(|(stage, _)| *stage == Stage::Vertex)
            .map(|(_, tail)| tail.clone())
            .ok_or("no vertex program")?;
        let pixel = engine_data
            .programs
            .iter()
            .find(|(stage, _)| *stage == Stage::Pixel)
            .map(|(_, tail)| tail.clone())
            .ok_or("no pixel program")?;
        engine_data.programs = vec![(Stage::Vertex, vertex), (Stage::Pixel, pixel)];
        fs::write(&output, engine_data.to_text())?;
        println!(
            "wrote {} bytes to {output}: the shipped contexts and groups, two programs",
            fs::metadata(&output)?.len()
        );
        return Ok(());
    }
    if mode == "groups" {
        // The contexts and groups are reduced to one each; the programs stay.
        let mut contexts = Vec::new();
        contexts.extend_from_slice(&DEFAULT_CONTEXT.to_le_bytes());
        contexts.extend_from_slice(&0u32.to_le_bytes());
        contexts.extend_from_slice(&1u32.to_le_bytes());
        contexts.extend_from_slice(&query.to_le_bytes());
        contexts.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        engine_data.contexts = contexts;
        engine_data.context_count = 1;
        let template = engine_data
            .group_template
            .as_ref()
            .ok_or("the engine data carries no group template")?;
        let group = template
            .groups
            .first()
            .ok_or("the template has no groups")?
            .clone();
        engine_data.group_template = Some(GroupTemplate {
            prefix: template.prefix.clone(),
            groups: vec![group],
            engine: template.engine.clone(),
        });
        engine_data.material_tables.truncate(1);
        fs::write(&output, engine_data.to_text())?;
        println!(
            "wrote {} bytes to {output}: one context (default), one query {query:08X}, one group, \
             the shipped programs",
            fs::metadata(&output)?.len()
        );
        return Ok(());
    }

    // One context, one query, no conditions.
    let mut contexts = Vec::new();
    contexts.extend_from_slice(&DEFAULT_CONTEXT.to_le_bytes());
    contexts.extend_from_slice(&0u32.to_le_bytes());
    contexts.extend_from_slice(&1u32.to_le_bytes());
    contexts.extend_from_slice(&query.to_le_bytes());
    contexts.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    engine_data.contexts = contexts;
    engine_data.context_count = 1;

    // One group: the template's first, with the first material table.
    let template = engine_data
        .group_template
        .as_ref()
        .ok_or("the engine data carries no group template")?;
    let group = template
        .groups
        .first()
        .ok_or("the template has no groups")?
        .clone();
    engine_data.group_template = Some(GroupTemplate {
        prefix: template.prefix.clone(),
        groups: vec![group],
        engine: template.engine.clone(),
    });
    engine_data.material_tables.truncate(1);

    // One program per stage: the first tail of each.
    let vertex = engine_data
        .programs
        .iter()
        .find(|(stage, _)| *stage == Stage::Vertex)
        .map(|(_, tail)| tail.clone())
        .ok_or("no vertex program")?;
    let pixel = engine_data
        .programs
        .iter()
        .find(|(stage, _)| *stage == Stage::Pixel)
        .map(|(_, tail)| tail.clone())
        .ok_or("no pixel program")?;
    engine_data.programs = vec![(Stage::Vertex, vertex), (Stage::Pixel, pixel)];

    fs::write(&output, engine_data.to_text())?;
    println!(
        "wrote {} bytes to {output}: one context (default), one query {query:08X}, one group, \
         two programs",
        fs::metadata(&output)?.len()
    );
    Ok(())
}
