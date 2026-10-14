//! Probe the physics scene a unit carries, through the unit's own round trip.
//!
//! The scene is a nested object in the unit SJSON now, so the check that matters is
//! the whole unit path rather than the scene alone: decompile a unit, recompile it
//! from that text, and compare the payload. A scene that does not survive its SJSON
//! spelling - or a spelling the parser does not accept - shows up here as a
//! difference or an error.
//!
//! Usage: `physics_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::unit;
use sdk::murmur::{IdString64, Murmur64};
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: physics_probe <bundle dir> [max bundles]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let ctx = Context::new();
    // The name only labels the compiled file; the payload is what is compared.
    let name = IdString64::from(Murmur64::hash(b"units/probe/physics"));

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
    let mut units = 0usize;
    let mut with_physics = 0usize;
    let mut with_flow = 0usize;
    let mut ok = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut bodies = 0usize;
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
            if file.file_type() != BundleFileType::Unit {
                continue;
            }
            units += 1;
            let data = file.variants()[0].data();
            let Ok((text, bsi)) = unit::decompile(data) else {
                continue;
            };
            if text.contains("flow = {") {
                with_flow += 1;
            }
            if !text.contains("physics = {") {
                continue;
            }
            with_physics += 1;
            bodies += text.matches("name_hash").count();
            match unit::compile(name.clone(), &text, bsi.as_bytes()) {
                Ok(compiled) => {
                    if compiled.variants()[0].data() == data {
                        ok += 1;
                    } else {
                        if shown < 4 {
                            shown += 1;
                            println!("DIFFERENT {}", file.base_name().display());
                        }
                        *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
                    }
                }
                Err(e) => {
                    if shown < 4 {
                        shown += 1;
                        println!("RECOMPILE-FAIL {}  {e}", file.base_name().display());
                    }
                    *reasons.entry(e.to_string()).or_default() += 1;
                }
            }
        }
    }

    println!(
        "units: {units} in {bundles} bundles; {with_flow} carry a flow graph, {with_physics} carry a physics scene"
    );
    println!("  {ok} byte-identical through the unit SJSON, {bodies} body records");
    for (reason, count) in reasons.iter().take(6) {
        println!("  {count:5}  {reason}");
    }
    Ok(())
}
