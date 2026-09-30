//! Checks the structured tail reader and rebuilds each program's input
//! signature list (list 7) from its compiled container, diffing the result
//! against the list the tail carries.
//!
//! ```text
//! tail_inputs <engine_data text | material data>...
//! ```

use std::fs;
use std::path::Path;

use sdk::filetype::shader::{Tail, TailLists};
use sdk::filetype::shader_engine_data::EngineData;

fn record(words: &[u32]) -> String {
    words
        .iter()
        .map(|word| format!("{word:08X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() -> color_eyre::Result<()> {
    let verbose = std::env::args().any(|arg| arg == "-v");
    for arg in std::env::args().skip(1).filter(|arg| arg != "-v") {
        let bytes = fs::read(&arg)?;
        let engine = match String::from_utf8(bytes.clone()) {
            Ok(text) if EngineData::looks_like_text(&text) => EngineData::from_text(&text)?,
            _ => EngineData::from_material(&bytes)?,
        };
        let name = Path::new(&arg)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| arg.clone());
        let (mut round_tripped, mut rebuilt, mut failed, mut skipped) = (0usize, 0usize, 0usize, 0usize);
        for (index, (stage, tail_bytes)) in engine.programs.iter().enumerate() {
            let Some(tail) = Tail::parse(tail_bytes) else {
                failed += 1;
                continue;
            };
            let Some(mut lists) = TailLists::parse(&tail.rest) else {
                println!("  program {index} {stage:?}: the lists do not parse");
                failed += 1;
                continue;
            };
            if lists.bytes() != tail.rest {
                println!("  program {index} {stage:?}: the lists do not round trip");
                failed += 1;
                continue;
            }
            round_tripped += 1;
            let Some(container) = engine
                .program_containers
                .get(index)
                .copied()
                .flatten()
                .and_then(|at| engine.containers.get(at))
            else {
                skipped += 1;
                continue;
            };
            let original = lists.list(7).to_vec();
            if lists.set_inputs(container).is_none() {
                println!("  program {index} {stage:?}: no input signature");
                failed += 1;
                continue;
            }
            // The public entry point must reproduce the tail byte for byte.
            let api_matches =
                tail.with_inputs(container).map(|rebuilt| rebuilt.bytes()) == Some(tail.bytes());
            if lists.list(7) == original.as_slice() && api_matches {
                rebuilt += 1;
            } else {
                println!("  program {index} {stage:?}: list 7 differs");
                if !api_matches {
                    println!("    Tail::with_inputs does not reproduce the tail");
                }
                for (record_index, (tail_record, rebuilt_record)) in
                    original.iter().zip(lists.list(7)).enumerate()
                {
                    if tail_record != rebuilt_record {
                        println!("    [{record_index}] tail {}  rebuilt {}", record(tail_record), record(rebuilt_record));
                    }
                }
                if original.len() != lists.list(7).len() {
                    println!("    count {} -> {}", original.len(), lists.list(7).len());
                }
                if verbose {
                    println!("    tail:");
                    for (record_index, tail_record) in original.iter().enumerate() {
                        println!("      [{record_index:>2}] {}", record(tail_record));
                    }
                    println!("    rebuilt:");
                    for (record_index, rebuilt_record) in lists.list(7).iter().enumerate() {
                        println!("      [{record_index:>2}] {}", record(rebuilt_record));
                    }
                    if let Some((inputs, _)) = sdk::filetype::shader::signatures(container) {
                        println!("    container ISG1:");
                        for element in &inputs {
                            println!(
                                "      {:>16} idx {} reg {} mask {:08X}",
                                element.name, element.index, element.register, element.mask
                            );
                        }
                    }
                }
                failed += 1;
            }
        }
        println!(
            "{name}: {round_tripped} tails round trip, {rebuilt} input lists rebuilt identically, {failed} failed, {skipped} without containers"
        );
    }
    Ok(())
}
