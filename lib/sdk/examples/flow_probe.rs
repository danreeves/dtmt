//! Probe the `flow` resources.
//!
//! Decodes every shipped flow and re-encodes it. The writer rebuilds the layout
//! (section order, 8-alignment and node placement) rather than copying it, so a
//! byte-identical round trip is the evidence that the layout is right.
//!
//! Usage: `flow_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::flow::Flow;
use sdk::{Bundle, BundleFileType, Context};

/// Above this the SJSON path is skipped: the point of it is the reader, not the size.
const SJSON_LIMIT: usize = 64 * 1024;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: flow_probe <bundle dir> [max bundles]");
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
    let mut sjson_bad = 0usize;
    let mut sjson_skipped = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut sizes: Vec<usize> = Vec::new();
    let mut nodes = 0usize;
    let mut empty_nodes = 0usize;
    let mut events = 0usize;
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
            if file.file_type() != BundleFileType::Flow {
                continue;
            }
            found += 1;
            let data = file.variants()[0].data();
            sizes.push(data.len());
            let flow = match Flow::from_binary(data) {
                Ok(flow) => flow,
                Err(e) => {
                    if shown < 6 {
                        shown += 1;
                        println!("DECODE-FAIL {}  {} B  {e}", file.base_name().display(), data.len());
                    }
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            nodes += flow.nodes.len();
            empty_nodes += flow.nodes.iter().filter(|n| n.empty).count();
            events += flow.external_events.len();
            if flow.to_binary()? == data {
                ok += 1;
            } else {
                *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
            }
            if data.len() <= SJSON_LIMIT {
                match flow
                    .to_sjson()
                    .and_then(|text| Flow::from_sjson(&text))
                    .and_then(|read| read.to_binary())
                {
                    Ok(round) if round == data => sjson_ok += 1,
                    Ok(_) => sjson_bad += 1,
                    Err(e) => {
                        if shown < 6 {
                            shown += 1;
                            println!("SJSON-FAIL {}  {e}", file.base_name().display());
                        }
                        *reasons.entry(format!("sjson: {e}")).or_default() += 1;
                        sjson_bad += 1;
                    }
                }
            } else {
                sjson_skipped += 1;
            }
        }
    }

    sizes.sort_unstable();
    println!("flow: {found} found in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} byte-identical through SJSON ({sjson_bad} different, {sjson_skipped} too large to try)");
    println!("  nodes {nodes} ({empty_nodes} index entries without a record), external events {events}");
    if let (Some(min), Some(max)) = (sizes.first(), sizes.last()) {
        println!("  sizes {min} B .. {max} B");
    }
    for (reason, count) in reasons.iter().take(6) {
        println!("  {count:5}  {reason}");
    }
    Ok(())
}
