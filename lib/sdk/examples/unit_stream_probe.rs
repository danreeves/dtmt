//! Find `unit` variants whose data lives in an external data file.
//!
//! A streamed unit keeps its vertex bytes outside the payload; if the bundle
//! records them as an external `data/` file then DTMT's existing streamed-data
//! path can carry them, and the geometry's "a vertex stream has no data" guard
//! can be lifted. Header-only parse, so it is cheap over a whole bundle directory.
//!
//! Usage: `unit_stream_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let dir = std::env::args().nth(1).expect("usage: unit_stream_probe <dir> [max]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let ctx = Context::new();
    let mut bundles = 0usize;
    let mut units = 0usize;
    let mut external_units = 0usize;
    let mut unit_names = 0usize;
    let mut slots: BTreeMap<usize, usize> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    for path in entries.into_iter().take(limit) {
        let Ok(bin) = std::fs::read(&path) else { continue };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Unit {
                continue;
            }
            units += 1;
            for variant in file.variants() {
                if !variant.external() {
                    continue;
                }
                external_units += 1;
                let name = variant.data_file_name();
                if name.is_some() {
                    unit_names += 1;
                }
                *slots.entry(variant.data_file_name_len()).or_default() += 1;
                if examples.len() < 8 {
                    examples.push(format!(
                        "{}  dfn={:?} size={}",
                        file.base_name().display(),
                        name,
                        variant.size()
                    ));
                }
            }
        }
    }

    println!("bundles parsed           {bundles}");
    println!("unit entries             {units}");
    println!("unit variants external   {external_units}");
    println!("  of which named a file  {unit_names}");
    println!("data-file-name slot lengths: {slots:?}");
    for example in &examples {
        println!("  {example}");
    }
    Ok(())
}
