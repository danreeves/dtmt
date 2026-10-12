//! Shipped-sample check for the `bones` resource: walk a directory of shipped
//! bundles, decode every `Bones` entry with `Bones::from_binary` and report how
//! many validate (type hash, envelope sizes, joint ids, null-terminated names).
//!
//! Usage: `bones_probe <bundle dir> [max files]`

use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: bones_probe <bundle dir> [max files]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let ctx = Context::new();
    let mut examined = 0usize;
    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut shown = 0usize;

    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    entries.truncate(limit);

    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(bundle) = Bundle::from_binary(&ctx, name.clone(), &bytes[..]) else {
            continue;
        };
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Bones {
                continue;
            }
            for variant in file.variants() {
                examined += 1;
                match sdk::filetype::bones::Bones::from_binary(variant.data()) {
                    Ok(bones) => {
                        ok += 1;
                        if shown < 5 {
                            shown += 1;
                            let round = bones
                                .to_binary()
                                .map(|b| b == variant.data())
                                .unwrap_or(false);
                            println!(
                                "OK   {name}  joints={} lods={} first={:?} round_trip={round}",
                                bones.names.len(),
                                bones.lods.len(),
                                bones.names.first()
                            );
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        if failed <= 6 {
                            let d = variant.data();
                            let head: String = d
                                .iter()
                                .take(20)
                                .map(|b| format!("{b:02x}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            println!(
                                "FAIL {name}  len={} external={} dfn={:?}  {e}\n     head: {head}",
                                d.len(),
                                variant.external(),
                                variant.data_file_name(),
                            );
                        }
                    }
                }
            }
        }
    }

    println!(
        "scanned {} bundles: bones variants={examined} decoded_ok={ok} failed={failed}",
        entries.len()
    );
    Ok(())
}
