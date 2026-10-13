//! Locate a shipped `unit` by its name hash (or every unit in a bundle) and
//! report the sweep's verdict while dumping its decompiled `.unit`/`.bsi` SJSON,
//! for inspecting a payload the sweep flags.
//!
//! Usage: `unit_find <bundle file or dir> [name hash] [out dir]`

use sdk::filetype::unit;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path_arg = std::env::args().nth(1).expect("usage: unit_find <bundle|dir> [hash] [out]");
    let wanted: Option<u64> = std::env::args()
        .nth(2)
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok());
    let out = std::env::args().nth(3);
    let limit: usize = std::env::args()
        .nth(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let ctx = Context::new();
    let root = std::path::Path::new(&path_arg);
    let mut entries: Vec<std::path::PathBuf> = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        std::fs::read_dir(root)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect()
    };
    entries.sort();
    entries.truncate(limit);

    for path in entries {
        let Ok(bin) = std::fs::read(&path) else { continue };
        let Ok(bundle) = Bundle::from_binary(&ctx, "find".to_string(), bin) else {
            continue;
        };
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Unit {
                continue;
            }
            let name = u64::from(file.base_name().to_murmur64());
            if wanted.is_some_and(|wanted| wanted != name) {
                continue;
            }
            println!("{}  unit {name:016x}", path.display());
            for (i, variant) in file.variants().iter().enumerate() {
                let payload = variant.data();
                let (unit_sjson, bsi_sjson) = match unit::decompile(payload) {
                    Ok(pair) => pair,
                    Err(error) => {
                        println!("  variant {i}: {} bytes - decode rejected: {error}", payload.len());
                        continue;
                    }
                };                let verdict = match unit::compile(
                    file.base_name().clone(),
                    &unit_sjson,
                    bsi_sjson.as_bytes(),
                ) {
                    Ok(rebuilt) => {
                        let rebuilt = rebuilt.variants().first().map(|v| v.data()).unwrap_or(&[]);
                        if rebuilt == payload {
                            "round-trips byte-for-byte".to_string()
                        } else {
                            let at = payload
                                .iter()
                                .zip(rebuilt.iter())
                                .position(|(a, b)| a != b);
                            format!(
                                "DIFFERENT: orig {} rebuilt {} first_diff_at {at:?}",
                                payload.len(),
                                rebuilt.len()
                            )
                        }
                    }
                    Err(error) => format!("RECOMPILE-FAIL: {error:?}"),
                };
                println!(
                    "  variant {i}: {} bytes, .unit {} chars, .bsi {} chars - {verdict}",
                    payload.len(),
                    unit_sjson.len(),
                    bsi_sjson.len()
                );
                if let Some(out) = &out {
                    let dir = std::path::Path::new(out);
                    std::fs::create_dir_all(dir)?;
                    let stem = format!("{name:016x}_{i}");
                    std::fs::write(dir.join(format!("{stem}.unit")), &unit_sjson)?;
                    std::fs::write(dir.join(format!("{stem}.bsi")), &bsi_sjson)?;
                }
            }
        }
    }
    Ok(())
}
