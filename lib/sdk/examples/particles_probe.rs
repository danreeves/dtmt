//! Shipped-sample check for the `particles` resource: decompile every effect in a
//! bundle (or a directory of bundles), recompile it and compare the bytes. This is
//! what proves the typed initializer fields are *editable* rather than merely
//! readable - the compile path now rebuilds the operands from `fields`.
//!
//! Usage: `particles_probe <bundle file or dir> [max bundles]`

use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path_arg = std::env::args()
        .nth(1)
        .expect("usage: particles_probe <bundle|dir> [max bundles]");
    let limit: usize = std::env::args()
        .nth(2)
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

    let mut bundles = 0usize;
    let mut effects = 0usize;
    let mut identical = 0usize;
    let mut differing = 0usize;
    let mut failed = 0usize;
    // Which visualizer types appear, and whether the view named their fields.
    let mut visualizers: std::collections::BTreeMap<(String, bool), usize> = Default::default();
    let mut simulators_named = 0usize;
    let mut simulators_total = 0usize;

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else { continue };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Particles {
                continue;
            }
            let variant = &file.variants()[0];
            let payload = variant.data();
            effects += 1;
            let files = match sdk::filetype::particles::decompile(&ctx, file.variants()) {
                Ok(files) => files,
                Err(error) => {
                    failed += 1;
                    println!("DECOMPILE-FAIL {}  {error}", file.base_name().display());
                    continue;
                }
            };
            let sjson = match std::str::from_utf8(files[0].data()) {
                Ok(text) => text,
                Err(_) => {
                    failed += 1;
                    println!("NOT-UTF8      {}", file.base_name().display());
                    continue;
                }
            };
            if let Ok(effect) = sdk::filetype::particles::Particles::from_sjson(sjson) {
                for system in &effect.systems {
                    for simulator in &system.simulators {
                        simulators_total += 1;
                        if simulator.fields.is_some() {
                            simulators_named += 1;
                        }
                    }
                    for visualizer in &system.visualizers {
                        *visualizers
                            .entry((visualizer.kind.clone(), visualizer.fields.is_some()))
                            .or_default() += 1;
                    }
                }
            }
            match sdk::filetype::particles::compile(file.base_name().clone(), sjson) {
                Ok(rebuilt) => {
                    let rebuilt = rebuilt.variants().first().map(|v| v.data()).unwrap_or(&[]);
                    if rebuilt == payload {
                        identical += 1;
                    } else {
                        differing += 1;
                        let at = payload.iter().zip(rebuilt.iter()).position(|(a, b)| a != b);
                        println!(
                            "DIFF {}  original={} rebuilt={} first_diff_at={at:?}",
                            file.base_name().display(),
                            payload.len(),
                            rebuilt.len()
                        );
                    }
                }
                Err(error) => {
                    failed += 1;
                    println!("RECOMPILE-FAIL {}  {error}", file.base_name().display());
                }
            }
        }
    }

    println!(
        "scanned {bundles} bundles: effects={effects} byte_identical={identical} \
         differing={differing} failed={failed}"
    );
    println!("simulators with a named view: {simulators_named} of {simulators_total}");
    println!("visualizers (type, fields named) -> count:");
    for ((kind, named), count) in &visualizers {
        println!("  {kind:12} named={named:<5} {count}");
    }
    Ok(())
}
