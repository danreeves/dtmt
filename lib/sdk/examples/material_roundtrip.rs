//! Round-trip Darktide material data files: parse a material stream, decompile
//! it to SJSON, compile it back and compare the result with the input.
//!
//! Usage: `cargo run -p sdk --example material_roundtrip -- <material data file>...`
//!
//! The files are the external `data/xx/<hash>` payloads referenced by a
//! material's bundle entry (not the `.material` resource itself).

use sdk::filetype::material;
use sdk::murmur::IdString64;
use sdk::{BundleFileVariant, Context};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let ctx = Context::new();
    let mut failures = 0;

    for path in std::env::args().skip(1) {
        let original = std::fs::read(&path)?;

        let mut variant = BundleFileVariant::new();
        variant.set_data(original.clone());

        let files = material::decompile(&ctx, "roundtrip.material".to_string(), &variant).await?;
        let sjson = String::from_utf8(files[0].data().to_vec())?;

        if sjson.contains("shader_size") {
            println!("SKIP (embedded shader): {path}");
            continue;
        }

        let file = material::compile(IdString64::from("roundtrip".to_string()), &sjson)?;
        let variant = &file.variants()[0];
        let recompiled = variant.external_data().expect("external data");

        if recompiled == &original {
            println!("OK   {} ({} bytes)", path, original.len());
        } else {
            failures += 1;
            println!(
                "FAIL {} (original {} bytes, recompiled {} bytes)",
                path,
                original.len(),
                recompiled.len()
            );

            let len = original.len().min(recompiled.len());
            for i in 0..len {
                if original[i] != recompiled[i] {
                    println!("  first difference at byte {i:#X}");
                    println!(
                        "  original:   {}",
                        hex(&original[i.saturating_sub(8)..(i + 8).min(len)])
                    );
                    println!(
                        "  recompiled: {}",
                        hex(&recompiled[i.saturating_sub(8)..(i + 8).min(len)])
                    );
                    break;
                }
            }
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X} "))
        .collect::<String>()
}
