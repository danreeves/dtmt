//! Shipped-sample check for the `unit` resource: walk a directory of shipped
//! bundles, decompile every `Unit` payload and recompile it, reporting how many
//! decode, recompile and round-trip byte-for-byte, and why the rest fail.
//!
//! Usage: `unit_probe <bundle dir> [max files]`

use sdk::filetype::unit;
use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let dir = std::env::args().nth(1).expect("usage: unit_probe <bundle dir> [max files]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);

    let ctx = Context::new();
    let mut examined = 0usize;
    let mut decoded = 0usize;
    let mut round_trip = 0usize;
    let mut mismatch = 0usize;
    let mut recompiled_failed = 0usize;
    let mut decode_failed = 0usize;
    let mut shown_decode = 0usize;
    let mut shown_other = 0usize;
    let mut dumped = 0usize;
    let mut failures: Vec<(usize, usize, Option<usize>, String)> = Vec::new();
    let mut reasons: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let dump_dir = std::env::args().nth(3);

    // The first argument may be a single bundle file instead of a directory.
    let dir_path = std::path::Path::new(&dir);
    let mut entries: Vec<_> = if dir_path.is_file() {
        vec![dir_path.to_path_buf()]
    } else {
        std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect()
    };
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
            if file.file_type() != BundleFileType::Unit {
                continue;
            }
            for variant in file.variants() {
                examined += 1;
                let payload = variant.data();
                match unit::decompile(payload) {
                    Ok((unit_text, bsi_text)) => {
                        decoded += 1;
                        let rebuilt = unit::compile(
                            file.base_name().clone(),
                            &unit_text,
                            bsi_text.as_bytes(),
                        );
                        if let Some(out) = dump_dir.as_ref() {
                            if let Ok(rebuilt) = rebuilt.as_ref() {
                                let out = std::path::Path::new(out).join(dumped.to_string());
                                std::fs::create_dir_all(&out)?;
                                std::fs::write(out.join("orig.bin"), payload)?;
                                std::fs::write(
                                    out.join("rebuilt.bin"),
                                    rebuilt.variants().first().map(|v| v.data()).unwrap_or(&[]),
                                )?;
                                std::fs::write(out.join("unit.sjson"), &unit_text)?;
                                std::fs::write(out.join("bsi.sjson"), &bsi_text)?;
                                dumped += 1;
                            }
                        }
                        match rebuilt {
                            Ok(rebuilt) => {
                                let same = rebuilt
                                    .variants()
                                    .first()
                                    .map(|v| v.data() == payload)
                                    .unwrap_or(false);
                                if same {
                                    round_trip += 1;
                                } else {
                                    mismatch += 1;
                                    let other = rebuilt.variants().first().map(|v| v.data()).unwrap_or(&[]);
                                    let at = payload
                                        .iter()
                                        .zip(other.iter())
                                        .position(|(a, b)| a != b);
                                    failures.push((payload.len(), other.len(), at, name.clone()));
                                    if shown_other < 4 {
                                        shown_other += 1;
                                        println!(
                                            "DIFF {name}  original={} rebuilt={} first_diff_at={at:?}",
                                            payload.len(),
                                            other.len()
                                        );
                                        if let Some(at) = at {
                                            let lo = at.saturating_sub(8);
                                            let hi = (at + 12).min(payload.len().min(other.len()));
                                            println!(
                                                "     orig: {}",
                                                payload[lo..hi].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
                                            );
                                            println!(
                                                "     new : {}",
                                                other[lo..hi].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                recompiled_failed += 1;
                                if shown_other < 8 {
                                    shown_other += 1;
                                    println!("RECOMPILE-FAIL {name}  {e}");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        decode_failed += 1;
                        *reasons.entry(e.to_string()).or_insert(0usize) += 1;
                        if shown_decode < 8 {
                            shown_decode += 1;
                            println!("DECODE-FAIL {name}  {e}");
                        }
                    }
                }
            }
        }
    }

    println!(
        "scanned {} bundles: units={examined} decoded={decoded} round_trip={round_trip} \
         rebuilt_but_different={mismatch} recompile_failed={recompiled_failed} decode_failed={decode_failed}",
        entries.len()
    );
    failures.sort();
    println!("--- {} differing units, smallest first:", failures.len());
    for (original, rebuilt, at, bundle) in failures.iter().take(15) {
        println!("    original={original} rebuilt={rebuilt} first_diff_at={at:?} bundle={bundle}");
    }
    println!("--- decode rejections by reason:");
    let mut reasons: Vec<_> = reasons.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1));
    for (reason, count) in reasons {
        println!("    {count:5}  {reason}");
    }
    Ok(())
}
