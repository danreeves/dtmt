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
    if mode == "renew" {
        // Every query id is replaced with a fresh one, the structure otherwise
        // untouched: the engine reads the ids from the section, so generated
        // tags should serve as well as the shipped ones.
        let mut contexts = Vec::new();
        let mut at = 0usize;
        let mut index = 0u32;
        let old = engine_data.contexts.clone();
        while at + 12 <= old.len() {
            let count = u32::from_le_bytes(old[at + 8..at + 12].try_into().unwrap()) as usize;
            contexts.extend_from_slice(&old[at..at + 12]);
            for query in 0..count {
                let record = at + 12 + query * 8;
                let renewed = u32::from(sdk::murmur::Murmur32::hash(
                    format!("renew{index}").as_bytes(),
                ));
                contexts.extend_from_slice(&renewed.to_le_bytes());
                contexts.extend_from_slice(&old[record + 4..record + 8]);
                index += 1;
            }
            at += 12 + count * 8;
        }
        engine_data.contexts = contexts;
        fs::write(&output, engine_data.to_text())?;
        println!(
            "wrote {} bytes to {output}: {index} query ids renewed",
            fs::metadata(&output)?.len()
        );
        return Ok(());
    }
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

    if mode == "n" {
        // The first `count` queries and groups, everything else shipped: the
        // structural floor probe. Program list and all framing stay as they
        // are, so the only variable is how many permutation slots the section
        // declares.
        let count: usize = args
            .next()
            .ok_or("usage: minimal_section <in> <out> <query> n <count> [programs]")?
            .parse()?;

        // An optional program count: the first `programs` entries of the
        // program list, to probe what ShaderTemplate::initialize needs there.
        let programs: Option<usize> = match args.next() {
            Some(value) => Some(value.parse()?),
            None => None,
        };

        // The first `count` query ids, with their own conditions offsets, from
        // the shipped contexts blob.
        let mut queries: Vec<(u32, u32)> = Vec::new();
        let mut at = 0usize;
        while at + 12 <= engine_data.contexts.len() && queries.len() < count {
            let n = u32::from_le_bytes(engine_data.contexts[at + 8..at + 12].try_into().unwrap())
                as usize;
            for query in 0..n {
                if queries.len() == count {
                    break;
                }
                let record = at + 12 + query * 8;
                let id = u32::from_le_bytes(
                    engine_data.contexts[record..record + 4].try_into().unwrap(),
                );
                let conditions = u32::from_le_bytes(
                    engine_data.contexts[record + 4..record + 8].try_into().unwrap(),
                );
                queries.push((id, conditions));
            }
            at += 12 + n * 8;
        }
        let mut contexts = Vec::new();
        contexts.extend_from_slice(&DEFAULT_CONTEXT.to_le_bytes());
        contexts.extend_from_slice(&0u32.to_le_bytes());
        contexts.extend_from_slice(&(queries.len() as u32).to_le_bytes());
        for (id, conditions) in &queries {
            contexts.extend_from_slice(&id.to_le_bytes());
            contexts.extend_from_slice(&conditions.to_le_bytes());
        }
        engine_data.contexts = contexts;
        engine_data.context_count = 1;

        let template = engine_data
            .group_template
            .as_ref()
            .ok_or("the engine data carries no group template")?;
        let groups: Vec<_> = template.groups.iter().take(count).cloned().collect();
        engine_data.group_template = Some(GroupTemplate {
            prefix: (groups.len() as u32).to_le_bytes().to_vec(),
            groups,
            engine: template.engine.clone(),
        });
        engine_data.material_tables.truncate(count);
        if let Some(programs) = programs {
            // Keep an equal number of each stage, in the original order: the
            // list is 48 vertex + 48 pixel, so truncating the front would leave
            // only one stage.
            let per_stage = programs / 2;
            let (mut vertex, mut pixel) = (0usize, 0usize);
            engine_data.programs.retain(|(stage, _)| match stage {
                Stage::Vertex => {
                    vertex += 1;
                    vertex <= per_stage
                }
                Stage::Pixel => {
                    pixel += 1;
                    pixel <= per_stage
                }
                _ => true,
            });
        }

        fs::write(&output, engine_data.to_text())?;
        println!(
            "wrote {} bytes to {output}: one context, {count} quer(ies)/groups, {} programs",
            fs::metadata(&output)?.len(),
            engine_data.programs.len()
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
