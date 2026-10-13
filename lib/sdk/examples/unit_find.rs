//! Find a shipped `unit` by its name hash and dump its decompiled `.unit`/`.bsi`
//! SJSON, for inspecting a payload the sweep reports as a failure.
//!
//! Usage: `unit_find <bundle dir> <hex hash> [out dir]`

use sdk::filetype::unit;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let dir = std::env::args().nth(1).expect("usage: unit_find <dir> <hex hash> [out]");
    let wanted = std::env::args().nth(2).expect("the unit's name hash, in hex");
    let out = std::env::args().nth(3);
    let wanted = u64::from_str_radix(wanted.trim_start_matches("0x"), 16)?;

    let ctx = Context::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

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
            if name != wanted {
                continue;
            }
            println!("found in {}", path.display());
            for (i, variant) in file.variants().iter().enumerate() {
                let payload = variant.data();
                println!("  variant {i}: {} bytes", payload.len());
                match unit::decompile(payload) {
                    Ok((unit_sjson, bsi_sjson)) => {
                        println!(
                            "  decompiled: .unit {} chars, .bsi {} chars",
                            unit_sjson.len(),
                            bsi_sjson.len()
                        );
                        if let Some(out) = &out {
                            let dir = std::path::Path::new(out);
                            std::fs::create_dir_all(dir)?;
                            let stem = format!("{name:016x}");
                            std::fs::write(dir.join(format!("{stem}.unit")), &unit_sjson)?;
                            std::fs::write(dir.join(format!("{stem}.bsi")), &bsi_sjson)?;
                            println!("  wrote {}/{stem}.unit and .bsi", dir.display());
                        }
                        // The recompile path needs the encoded BSI bytes, which the
                        // sweep's own probe assembles; here the SJSON is the point.
                    }
                    Err(error) => println!("  decompile failed: {error:?}"),
                }
            }
        }
    }
    Ok(())
}
