//! Dumps a section's contexts and its conditions records together: each query's
//! group id, the record at its offset, the record's hashes and the payload's
//! branches with their result indices - so the results can be read against the
//! groups the contexts select.
//!
//! ```text
//! conditions_dump <material data file>
//! ```

use sdk::filetype::condition_tree::ConditionTree;
use sdk::filetype::shader::Section;

/// The condition names the UI base's roots resolve through the dictionary.
fn name(hash: u32) -> &'static str {
    match hash {
        0x9FCF_E126 => "gui",
        0x9B8D_E7E4 => "red",
        0x4BA4_BD58 => "green",
        0x0977_913D => "blue",
        0x3F69_7354 => "alpha",
        0x9B80_38E0 => "linear_depth",
        _ => "",
    }
}

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data = std::fs::read(&args[0])?;
    // A material data file carries the section as {offset, size} at +12/+16.
    let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    let (offset, size) = (word(12), word(16));
    let section = Section::parse(&data[offset..offset + size])?;

    let tree = ConditionTree::parse(section.conditions())?;
    let mut offsets = Vec::with_capacity(tree.len());
    let mut at = 0usize;
    for node in tree.nodes() {
        offsets.push(at);
        at += node.len();
    }
    let group = section.group_data();
    let group_count = group
        .get(0..4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .unwrap_or(0);
    println!(
        "conditions {} bytes, {} records; group data {} bytes, first word {}",
        section.conditions().len(),
        tree.len(),
        group.len(),
        group_count
    );

    let mut group = 0usize;
    for (index, context) in section.contexts().iter().enumerate() {
        println!(
            "context {index}: name {:08X} {} flags {:08X} queries {}",
            context.name,
            name(context.name),
            context.flags,
            context.queries.len()
        );
        for (query_index, query) in context.queries.iter().enumerate() {
            println!(
                "  query {query_index}: id {:08X} group {} conditions @{}",
                query.id, group, query.conditions
            );
            group += 1;
            let at = query.conditions as usize;
            let Some(record) = offsets.iter().position(|offset| *offset == at) else {
                println!("    (no record starts at that offset)");
                continue;
            };
            let node = &tree.nodes()[record];
            println!(
                "    record {record} @{at}: {} hashes, {} payload words",
                node.hashes.len(),
                node.payload.len()
            );
            for (hash_index, hash) in node.hashes.iter().enumerate() {
                let label = name(*hash);
                if label.is_empty() {
                    println!("      hash {hash_index}: {hash:08X}");
                } else {
                    println!("      hash {hash_index}: {hash:08X}  {label}");
                }
            }
            match node.branches() {
                Some((branches, fallback)) => {
                    for (branch_index, branch) in branches.iter().enumerate() {
                        println!(
                            "      branch {branch_index}: tests {:?} result {}",
                            branch.tests, branch.result
                        );
                    }
                    if let Some(fallback) = fallback {
                        println!("      fallback {fallback}");
                    }
                }
                None => println!("      payload: {:04X?}", node.payload),
            }
        }
    }
    Ok(())
}
