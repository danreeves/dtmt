//! Compares the tails `EngineData::derived_programs` generates against the ones
//! a section actually carries: the same `(stage, tail)` list, diffed by length
//! and first differing byte, so a wrong block shape shows up immediately.
//!
//! ```text
//! program_diff <engine_data text with a plan>
//! ```

use std::collections::HashMap;
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
    println!("plans: {}", plans.len());
    for plan in plans {
        println!("  context {:?}: {} queries", plan.context, plan.queries.len());
        for tokens in &plan.queries {
            println!("    {tokens:?}");
        }
    }

    let derived = engine
        .derived_programs(plans)
        .expect("the generator produced nothing");
    println!("derived programs: {}", derived.len());

    // The carried programs in the same shape, for a difference.
    let carried: Vec<(Stage, Vec<u8>)> = engine.programs.clone();
    println!("carried programs: {}", carried.len());

    for (index, (stage, tail)) in derived.iter().enumerate().take(24) {
        let carried = carried.get(index);
        match carried {
            Some((carried_stage, carried_tail)) => {
                let same_len = carried_tail.len() == tail.len();
                let diff = if same_len {
                    carried_tail
                        .iter()
                        .zip(tail.iter())
                        .position(|(a, b)| a != b)
                } else {
                    None
                };
                println!(
                    "  {index:3} {stage:?} derived {} B, carried {} B ({carried_stage:?}), \
                     first diff {diff:?}",
                    tail.len(),
                    carried_tail.len()
                );
            }
            None => println!("  {index:3} {stage:?} derived {} B, no carried", tail.len()),
        }
    }

    // What each derived pixel block holds where the mask sits.
    let _ = HashMap::<u8, u8>::new();
    Ok(())
}
