//! Probe the `theme` resources.
//!
//! Decodes every shipped theme and checks the byte-exact round trip, and reports
//! the shapes it finds - the names, the header words and the record counts - so the
//! module's claims about the tail and the derived record count are measured rather
//! than assumed.
//!
//! Usage: `theme_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::theme::Theme;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: theme_probe <bundle dir> [max bundles]");
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
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut word_counts: BTreeMap<usize, usize> = BTreeMap::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Theme {
                continue;
            }
            found += 1;
            let data = file.variants()[0].data();
            let theme = match Theme::from_binary(data) {
                Ok(theme) => theme,
                Err(e) => {
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            *names.entry(theme.name.clone()).or_default() += 1;
            *word_counts.entry(theme.words.len()).or_default() += 1;
            *shapes
                .entry(format!(
                    "words={} records={} width={} markers {}",
                    theme.words.len(),
                    theme.records.len(),
                    theme.records.first().map(|r| r.values.len()).unwrap_or(0),
                    theme
                        .records
                        .iter()
                        .map(|r| r.marker)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                ))
                .or_default() += 1;
            if theme.to_binary()? == data {
                ok += 1;
            }
            if theme
                .to_sjson()
                .and_then(|text| Theme::from_sjson(&text))
                .and_then(|read| read.to_binary())
                .is_ok_and(|round| round == data)
            {
                sjson_ok += 1;
            }
        }
    }

    println!("theme: {found} found in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} byte-identical through SJSON");
    println!("  header word counts (exclusive of the record count): {word_counts:?}");
    println!("  names: {} distinct", names.len());
    for (name, n) in names.iter().take(12) {
        println!("    {n:4}  {name}");
    }
    println!("  shapes:");
    for (shape, n) in shapes.iter().take(10) {
        println!("    {n:4}  {shape}");
    }
    for (reason, count) in reasons.iter().take(5) {
        println!("  {count:5}  {reason}");
    }
    Ok(())
}
