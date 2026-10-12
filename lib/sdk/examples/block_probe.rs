//! Reports how a group's device block relates to the preamble body: how many of
//! the body's `{id, flags, 0}` records appear in the block, whole or as their
//! `{id, flags}` pair.
//!
//! ```text
//! block_probe <engine_data>
//! ```

use std::error::Error;
use std::fs;

use sdk::filetype::shader_engine_data::EngineData;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: block_probe <engine_data>")?;
    let engine_data = EngineData::from_text(&fs::read_to_string(&path)?)?;

    // The preamble: a 16-byte head, then the body - a count word and that many
    // 12-byte `{id, flags, 0}` records.
    let preamble = &engine_data.device_preamble;
    let body = preamble.get(16..).unwrap_or_default();
    let count = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let records: Vec<[u8; 12]> = body
        .get(4..4 + count * 12)
        .ok_or("the body is shorter than its count")?
        .chunks_exact(12)
        .map(|record| record.try_into().unwrap())
        .collect();
    println!(
        "preamble {} B; body {} B; {count} 12-byte records",
        preamble.len(),
        body.len()
    );

    for (index, (stage, block)) in engine_data.programs.iter().enumerate() {
        let whole = records
            .iter()
            .filter(|record| block.windows(12).any(|window| window == &record[..]))
            .count();
        let pair = records
            .iter()
            .filter(|record| block.windows(8).any(|window| window[..8] == record[..8]))
            .count();
        println!(
            "  program {index:2} {stage:?}: block {} B; whole records {whole:2}/{count}, \
             {{id,flags}} pairs {pair:2}/{count}",
            block.len()
        );
    }

    // Where the body's records sit in the first pixel block, in block order.
    let Some((_, block)) = engine_data
        .programs
        .iter()
        .find(|(stage, block)| *stage == sdk::filetype::shader::Stage::Pixel && block.len() > 128)
    else {
        return Ok(());
    };

    // For every pixel block, the head's length (the first record's offset) and
    // the trailing length after the last record.
    println!("pixel blocks: head / records / trailing");
    for (index, (stage, block)) in engine_data.programs.iter().enumerate() {
        if *stage != sdk::filetype::shader::Stage::Pixel || block.len() <= 128 {
            continue;
        }
        let offsets: Vec<usize> = records
            .iter()
            .filter_map(|record| {
                block
                    .windows(12)
                    .position(|window| window == &record[..])
            })
            .collect();
        let head = offsets.iter().min().copied().unwrap_or(0);
        let end = offsets.iter().map(|offset| offset + 12).max().unwrap_or(0);
        println!(
            "  program {index:2}: {:4} / {} records at {head}..{end} / trailing {}",
            block.len(),
            offsets.len(),
            block.len() - end,
        );
    }
    let mut at: Vec<(usize, usize)> = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            block
                .windows(12)
                .position(|window| window == &record[..])
                .map(|offset| (offset, index))
        })
        .collect();
    at.sort();
    println!("the body's records in the block, in block order (offset, record):");
    for (offset, index) in &at {
        let record = records[*index];
        let words: Vec<u32> = record
            .chunks(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect();
        println!(
            "  {offset:5}  record {index:2}  {words:08X?}  gap-before {}",
            offset
                - at
                    .iter()
                    .filter(|(other, _)| other < offset)
                    .map(|(other, _)| other + 12)
                    .max()
                    .unwrap_or(0)
        );
    }
    Ok(())
}
