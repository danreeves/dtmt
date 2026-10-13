//! Tally the per-file-header byte DTMT calls `BundleFileVariant::unknown_1`
//! (the community template's `unknown_2`) across shipped bundles.
//!
//! `BundleFile::to_binary` writes a hard `1` for it while the reader keeps the
//! value it read, so any shipped value other than `1` is a silent round-trip
//! loss. Also tallies `Properties` and the external-data-file flag.
//!
//! Usage: `bundle_header_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::{Bundle, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let dir = std::env::args().nth(1).expect("usage: bundle_header_probe <dir> [max]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let ctx = Context::new();
    let mut bundles = 0usize;
    let mut variants = 0usize;
    let mut bytes: BTreeMap<u8, usize> = BTreeMap::new();
    let mut props: BTreeMap<u32, usize> = BTreeMap::new();
    let mut external: BTreeMap<bool, usize> = BTreeMap::new();
    let mut failures = 0usize;
    // One example per unexpected byte value, so a fix can be reproduced.
    let mut examples: BTreeMap<u8, String> = BTreeMap::new();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    for path in entries.into_iter().take(limit) {
        let Ok(bin) = std::fs::read(&path) else { continue };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            failures += 1;
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            for variant in file.variants() {
                variants += 1;
                *bytes.entry(variant.unknown_1()).or_default() += 1;
                *props.entry(file.props().bits()).or_default() += 1;
                *external.entry(variant.external()).or_default() += 1;
                if variant.unknown_1() != 1 {
                    examples.entry(variant.unknown_1()).or_insert_with(|| {
                        format!("{} ({})", path.display(), file.base_name().display())
                    });
                }
            }
        }
    }

    println!("bundles parsed {bundles} (parse failures {failures})");
    println!("variants       {variants}");
    println!("the per-variant header byte:");
    for (value, count) in &bytes {
        let note = if *value == 1 { " (what the writer emits)" } else { "" };
        println!("  {value:>3}  {count}{note}");
    }
    for (value, example) in &examples {
        println!("  value {value} seen at e.g. {example}");
    }
    println!("properties:");
    for (value, count) in &props {
        println!("  {value:08x}  {count}");
    }
    println!("external (the data-file flag):");
    for (value, count) in &external {
        println!("  {value}  {count}");
    }
    Ok(())
}
