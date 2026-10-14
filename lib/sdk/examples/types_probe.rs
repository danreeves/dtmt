//! Census the file types a Darktide install actually ships, and dump samples.
//!
//! Every type DTMT names is checked against every file in every bundle, so the
//! output says both what exists and - more usefully - what does not. A type with a
//! real id and a real name but **zero** shipped instances is a mechanism the engine
//! knows and Darktide never uses, and it cannot be derived from data that does not
//! exist; that is a different verdict from "not read yet".
//!
//! An optional comma-separated list of type ids dumps the first samples of each, so
//! several small types can be surveyed in one pass.
//!
//! Usage: `types_probe <bundle dir> [max bundles] [id,id,...]`

use std::collections::{BTreeMap, BTreeSet};

use sdk::{Bundle, BundleFileType, Context};

fn dump(label: &str, data: &[u8]) {
    println!("--- {label}  ({} B)", data.len());
    for row in (0..data.len().min(64)).step_by(16) {
        let end = (row + 16).min(data.len());
        let bytes = &data[row..end];
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let words: Vec<String> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()).to_string())
            .collect();
        let floats: Vec<String> = bytes
            .chunks_exact(4)
            .map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap())))
            .collect();
        let text: String = bytes
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  {row:04x}  {}  |{text}|", hex.join(" "));
        if !words.is_empty() {
            println!("        u32 [{}]  f32 [{}]", words.join(", "), floats.join(", "));
        }
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: types_probe <bundle dir> [max bundles] [id,id,...]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let wanted: Vec<u64> = std::env::args()
        .nth(3)
        .map(|s| {
            s.split(',')
                .map(|part| {
                    u64::from_str_radix(part.trim().trim_start_matches("0x"), 16)
                        .expect("the ids must be hex")
                })
                .collect()
        })
        .unwrap_or_default();

    let ctx = Context::new();
    // A bundle's name is its 16-hex-digit hash; everything else in the directory is
    // a sibling index or a stream.
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
    let mut files = 0usize;
    let mut variants = 0usize;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut dumped: BTreeSet<u64> = BTreeSet::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            files += 1;
            variants += file.variants().len();
            let kind = file.file_type();
            let id = u64::from(kind);
            match kind {
                BundleFileType::Unknown(hash) => {
                    *unknown.entry(format!("{hash:016X}")).or_default() += 1;
                }
                known => {
                    *counts.entry(known.ext_name()).or_default() += 1;
                }
            }
            if wanted.contains(&id) && dumped.insert(id) {
                dump(
                    &format!("{} ({id:016X})", kind.ext_name()),
                    file.variants()[0].data(),
                );
            }
        }
    }

    println!(
        "swept {bundles} bundles: {files} files, {variants} variants, {} distinct known types",
        counts.len()
    );

    // Every named type, most shipped first, so the zeroes are visible in place.
    let mut rows: Vec<(String, usize)> = BundleFileType::ALL
        .iter()
        .map(|kind| (kind.ext_name(), counts.get(&kind.ext_name()).copied().unwrap_or(0)))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let absent: Vec<&str> = rows
        .iter()
        .filter(|(_, n)| *n == 0)
        .map(|(name, _)| name.as_str())
        .collect();
    println!("--- named types, most shipped first:");
    for (name, n) in rows.iter().filter(|(_, n)| *n > 0) {
        println!("  {n:8}  {name}");
    }
    println!(
        "--- {} named types with ZERO shipped instances: {}",
        absent.len(),
        absent.join(", ")
    );
    println!("--- {} unnamed ids shipped:", unknown.len());
    for (hash, n) in unknown.iter().take(20) {
        println!("  {n:8}  {hash}");
    }
    Ok(())
}
