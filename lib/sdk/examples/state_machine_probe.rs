//! Probe the `state_machine` resources.
//!
//! Decodes every shipped machine and re-encodes it. Walking the whole body and
//! landing exactly on the end is the check that the field order is right, so the
//! headline number here is the number of byte-identical round trips out of 4,006.
//!
//! The SJSON check is done only for smaller machines: one is 3.6 MB, whose SJSON
//! would be enormous, and the point of that check is the reader, not the size.
//!
//! Usage: `state_machine_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::state_machine::StateMachine;
use sdk::{Bundle, BundleFileType, Context};

/// Above this many bytes the machine is decoded and re-encoded but not put through
/// SJSON.
const SJSON_LIMIT: usize = 64 * 1024;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: state_machine_probe <bundle dir> [max bundles]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let ctx = Context::new();
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&arg)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.len() == 16 && n.chars().all(|c| c.is_ascii_hexdigit())
                })
        })
        .collect();
    entries.sort();
    entries.truncate(limit);

    let mut bundles = 0usize;
    let mut found = 0usize;
    let mut ok = 0usize;
    let mut sjson_ok = 0usize;
    let mut sjson_skipped = 0usize;
    let mut sjson_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut sjson_shown = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut sizes: Vec<usize> = Vec::new();
    let mut states = 0usize;
    let mut layers = 0usize;
    let mut shown = 0usize;

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::StateMachine {
                continue;
            }
            found += 1;
            let data = file.variants()[0].data();
            sizes.push(data.len());
            let machine = match StateMachine::from_binary(data) {
                Ok(machine) => machine,
                Err(e) => {
                    if shown < 6 {
                        shown += 1;
                        println!("DECODE-FAIL {}  {} B  {e}", file.base_name().display(), data.len());
                    }
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            layers += machine.layers.len();
            states += machine.layers.iter().map(|l| l.states.len()).sum::<usize>();
            if machine.to_binary()? == data {
                ok += 1;
            } else {
                *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
            }
            if data.len() <= SJSON_LIMIT {
                match machine
                    .to_sjson()
                    .and_then(|text| StateMachine::from_sjson(&text))
                    .and_then(|read| read.to_binary())
                {
                    Ok(round) if round == data => sjson_ok += 1,
                    Ok(_) => {
                        *sjson_reasons
                            .entry("re-encoded differently through SJSON".to_string())
                            .or_default() += 1;
                    }
                    Err(e) => {
                        if sjson_shown < 4 {
                            sjson_shown += 1;
                            println!("SJSON-FAIL {}  {} B  {e}", file.base_name().display(), data.len());
                        }
                        *sjson_reasons.entry(e.to_string()).or_default() += 1;
                    }
                }
            } else {
                sjson_skipped += 1;
            }
        }
    }

    sizes.sort_unstable();
    println!("state_machine: {found} found in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} of the small ones byte-identical through SJSON ({sjson_skipped} too large to try)");
    println!("  layers {layers}, states {states}");
    if let (Some(min), Some(max)) = (sizes.first(), sizes.last()) {
        println!("  sizes {min} B .. {max} B");
    }
    for (reason, count) in reasons.iter().take(6) {
        println!("  {count:5}  {reason}");
    }
    for (reason, count) in sjson_reasons.iter().take(4) {
        println!("  {count:5}  (sjson) {reason}");
    }
    Ok(())
}
