//! Round-trip Darktide unit payloads: decompile a compiled payload into its
//! `.unit`/`.bsi` pair, compile the pair back and decompile the result again.
//! The `.unit` text and every `.bsi` line must match exactly, except the decoded
//! NORMAL stream data: octahedral normals are quantized, so their decimal text
//! can drift while everything structural (scene graph, geometry, indices,
//! materials, other streams) must survive the round trip.
//!
//! Usage: `cargo run -p sdk --example unit_roundtrip -- <payload>...`

use sdk::filetype::unit;
use sdk::murmur::{IdString64, Murmur64};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut failures = 0;
    for path in std::env::args().skip(1) {
        let original = std::fs::read(&path)?;
        let (unit_first, bsi_first) = match unit::decompile(&original) {
            Ok(pair) => pair,
            Err(error) => {
                println!("SKIP {path} ({error})");
                continue;
            }
        };

        let stem = std::path::Path::new(&path)
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let name = IdString64::from(Murmur64::hash(&stem));

        let file = match unit::compile(name, &unit_first, bsi_first.as_bytes()) {
            Ok(file) => file,
            Err(error) => {
                failures += 1;
                println!("FAIL {path} (recompile failed: {error})");
                continue;
            }
        };
        let recompiled = file.variants()[0].data().to_vec();
        let (unit_second, bsi_second) = match unit::decompile(&recompiled) {
            Ok(pair) => pair,
            Err(error) => {
                failures += 1;
                println!("FAIL {path} (second decompile failed: {error})");
                continue;
            }
        };

        let unit_ok = unit_first == unit_second;
        let bsi_ok =
            bsi_first == bsi_second || lines_match_ignoring_normals(&bsi_first, &bsi_second);
        if unit_ok && bsi_ok {
            println!(
                "OK   {path} ({} -> {} payload bytes{})",
                original.len(),
                recompiled.len(),
                if bsi_first == bsi_second {
                    ", text exact"
                } else {
                    ", normals re-encoded"
                }
            );
        } else {
            failures += 1;
            println!(
                "FAIL {path} (original {} bytes, recompiled {} bytes)",
                original.len(),
                recompiled.len()
            );
            if !unit_ok {
                println!("  .unit text differs");
            }
            if !bsi_ok {
                println!("  .bsi text differs");
                report_first_difference(&bsi_first, &bsi_second);
            }
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Compares `.bsi` texts line by line, treating the decoded data of NORMAL
/// streams as equal (its octahedral quantization is lossy).
fn lines_match_ignoring_normals(left: &str, right: &str) -> bool {
    let left: Vec<&str> = left.lines().collect();
    let right: Vec<&str> = right.lines().collect();
    left.len() == right.len()
        && left.iter().zip(&right).all(|(a, b)| {
            if a.contains("name = \"NORMAL\"") {
                stream_head(a) == stream_head(b)
            } else {
                a == b
            }
        })
}

/// The channel and size/stride parts of a stream line, without its `data` list.
fn stream_head(line: &str) -> (&str, &str) {
    let data = line.find(" data = [").unwrap_or(line.len());
    let tail = line[data..]
        .find(" ] size =")
        .map(|offset| data + offset + 3)
        .unwrap_or(line.len());
    (&line[..data], &line[tail..])
}

/// Prints where two texts first differ, with a little context.
fn report_first_difference(left: &str, right: &str) {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let at = left
        .iter()
        .zip(&right)
        .position(|(a, b)| a != b)
        .unwrap_or(left.len().min(right.len()));
    let start = at.saturating_sub(60);
    let end = (at + 60).min(left.len().min(right.len()));
    println!("  first difference at character {at}:");
    println!(
        "    first:  ...{}...",
        left[start..end].iter().collect::<String>()
    );
    println!(
        "    second: ...{}...",
        right[start..end].iter().collect::<String>()
    );
}
