//! Probe the `mouse_cursor` resources.
//!
//! Only two ship, so this checks the byte-exact round trip and reports the header
//! fields - in particular whether `byte count == width * height * 4` holds, which is
//! the relation the module deliberately does *not* require.
//!
//! Usage: `mouse_cursor_probe <bundle dir> [max bundles]`

use std::collections::BTreeMap;

use sdk::filetype::mouse_cursor::MouseCursor;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: mouse_cursor_probe <bundle dir> [max bundles]");
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
    let mut product_holds = 0usize;
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::MouseCursor {
                continue;
            }
            found += 1;
            let data = file.variants()[0].data();
            let cursor = match MouseCursor::from_binary(data) {
                Ok(cursor) => cursor,
                Err(e) => {
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            *shapes
                .entry(format!(
                    "{}x{} unknown_8={} unknown_12={} bytes={}",
                    cursor.width,
                    cursor.height,
                    cursor.unknown_8,
                    cursor.unknown_12,
                    cursor.pixels.len()
                ))
                .or_default() += 1;
            if cursor.pixels.len() as u64 == u64::from(cursor.width) * u64::from(cursor.height) * 4 {
                product_holds += 1;
            }
            if cursor.to_binary()? == data {
                ok += 1;
            }
            if cursor
                .to_sjson()
                .and_then(|text| MouseCursor::from_sjson(&text))
                .and_then(|read| read.to_binary())
                .is_ok_and(|round| round == data)
            {
                sjson_ok += 1;
            }
        }
    }

    println!("mouse_cursor: {found} found in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} byte-identical through SJSON");
    println!("  {product_holds} satisfy byte count == width * height * 4");
    for (shape, n) in &shapes {
        println!("  {n:5}  {shape}");
    }
    for (reason, count) in reasons.iter().take(5) {
        println!("  {count:5}  {reason}");
    }
    Ok(())
}
