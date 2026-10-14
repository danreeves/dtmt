//! Probe the `ies` resources.
//!
//! Decodes every shipped profile, re-encodes it and checks the bytes, and tallies
//! the header words so the module's claims about them are measured rather than
//! assumed: if `version`, the two count words or the size turn out to vary, this
//! says so.
//!
//! Usage: `ies_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::ies::Ies;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: ies_probe <bundle dir> [max bundles]");
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
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    let mut eights: BTreeMap<u32, usize> = BTreeMap::new();
    let mut twelves: BTreeMap<u32, usize> = BTreeMap::new();
    let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut scales: BTreeMap<String, usize> = BTreeMap::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Ies {
                continue;
            }
            found += 1;
            let data = file.variants()[0].data();
            *sizes.entry(data.len()).or_default() += 1;
            let ies = match Ies::from_binary(data) {
                Ok(ies) => ies,
                Err(e) => {
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            *versions.entry(ies.version).or_default() += 1;
            *eights.entry(ies.unknown_8).or_default() += 1;
            *twelves.entry(ies.unknown_12).or_default() += 1;
            *counts.entry(ies.values.len()).or_default() += 1;
            *scales.entry(format!("{:.4}", ies.scale)).or_default() += 1;
            if ies.to_binary()? == data {
                ok += 1;
            } else {
                *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
            }
            if ies
                .to_sjson()
                .and_then(|text| Ies::from_sjson(&text))
                .and_then(|read| read.to_binary())
                .is_ok_and(|round| round == data)
            {
                sjson_ok += 1;
            }
        }
    }

    println!("ies: {found} found in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} byte-identical through SJSON");
    println!("  sizes: {sizes:?}");
    println!("  value counts: {counts:?}");
    println!("  version: {versions:?}");
    println!("  unknown_8: {eights:?}");
    println!("  unknown_12: {twelves:?}");
    println!("  distinct scales (up to 8): {:?}", scales.keys().take(8).collect::<Vec<_>>());
    if !reasons.is_empty() {
        println!("--- reasons ({}) :", reasons.len());
        for (reason, count) in reasons.iter().take(8) {
            println!("  {count:5}  {reason}");
        }
    }
    Ok(())
}
