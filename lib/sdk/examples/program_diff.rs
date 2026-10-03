//! Compares the tails `EngineData::derived_programs` generates against the ones
//! a section actually carries, and prints the carried family's table as Rust
//! arrays (head sizes and mask bytes per pixel slot).
//!
//! ```text
//! program_diff <engine_data text with a plan>
//! ```

use std::fs;

use sdk::filetype::shader::Stage;
use sdk::filetype::shader_engine_data::EngineData;

fn main() -> color_eyre::Result<()> {
    let path = std::env::args().nth(1).expect("usage: program_diff <engine_data>");
    let text = fs::read_to_string(&path)?;
    let engine = EngineData::from_text(&text)?;

    let plans = engine
        .permutations
        .as_ref()
        .expect("the file carries no permutation plan");
    let derived = engine
        .derived_programs(plans)
        .expect("the generator produced nothing");
    let carried: Vec<(Stage, Vec<u8>)> = engine.programs.clone();
    println!("derived {} programs, carried {}", derived.len(), carried.len());

    let mut differing = 0;
    for (index, ((stage, tail), (_, carried_tail))) in
        derived.iter().zip(carried.iter()).enumerate()
    {
        let diff = tail
            .iter()
            .zip(carried_tail.iter())
            .position(|(a, b)| a != b)
            .or_else(|| (tail.len() != carried_tail.len()).then_some(usize::MAX));
        if diff.is_some() {
            differing += 1;
            let from = diff.unwrap_or(0).min(tail.len().saturating_sub(1));
            let to = (from + 24).min(tail.len().min(carried_tail.len()));
            let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join("");
            println!(
                "  {index:3} {stage:?} {} B, diff {diff:?}\n      derived[{from}]={}\n      carried[{from}]={}",
                tail.len(),
                hex(&tail[from..to]),
                hex(&carried_tail[from..to])
            );
        }
    }
    println!("{differing} differing");

    // The carried family table, as Rust arrays.
    let mut heads = Vec::new();
    let mut masks = Vec::new();
    let mut seams = Vec::new();
    for (stage, tail) in &carried {
        if *stage != Stage::Pixel {
            continue;
        }
        // Head 36 gives a 625-byte tail, head 32 gives 621; the mask follows at
        // `4 (prefix) + 36 (lists) + head + 477`.
        let head = if tail.len() == 625 { 36 } else { 32 };
        heads.push(head as u8);
        masks.push(tail.get(4 + 36 + head + 477).copied().unwrap_or(0x07));
        // The 36-byte head's seam word's low byte, at tail offset 72.
        seams.push(if head == 36 {
            tail.get(72).copied().unwrap_or(0)
        } else {
            0
        });
    }
    let dump = |name: &str, values: &[u8], hex: bool| {
        println!("const {name}: [u8; {}] = [", values.len());
        for chunk in values.chunks(16) {
            let row: Vec<String> = chunk
                .iter()
                .map(|v| if hex { format!("0x{v:02X}") } else { v.to_string() })
                .collect();
            println!("    {},", row.join(", "));
        }
        println!("];");
    };
    dump("UI_BASE_PASS_HEADS", &heads, false);
    dump("UI_BASE_PASS_MASKS", &masks, true);
    dump("UI_BASE_PASS_SEAMS", &seams, true);
    Ok(())
}
