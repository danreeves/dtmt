//! Probe the Wwise engine-layer resources.
//!
//! For every `wwise_dep` found it checks the rule derived from the two the boot
//! bundle ships (`size == 8 + len`, and the leading word constant), and for
//! `wwise_event` / `wwise_metadata` it dumps the first bytes so their layouts can be
//! derived the same way.
//!
//! Usage: `wwise_probe <bundle file or dir> [max bundles]`

use std::collections::{BTreeMap, BTreeSet};

use sdk::{Bundle, BundleFileType, Context};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let path_arg = std::env::args().nth(1).expect("usage: wwise_probe <bundle|dir> [max]");
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

    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    let mut deps = 0usize;
    let mut good = 0usize;
    let mut events = 0usize;
    let mut metadata = 0usize;
    let mut samples: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else { continue };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        for file in bundle.files() {
            let kind = match file.file_type() {
                BundleFileType::WwiseDep => "dep",
                BundleFileType::WwiseEvent => "event",
                BundleFileType::WwiseMetadata => "metadata",
                _ => continue,
            };
            let data = file.variants()[0].data();
            match kind {
                "dep" => {
                    deps += 1;
                    if data.len() < 8 {
                        continue;
                    }
                    let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
                    let len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
                    *versions.entry(version).or_default() += 1;
                    if 8 + len == data.len() {
                        good += 1;
                    } else if good + 5 > deps {
                        println!(
                            "DEP-MISMATCH {}  bytes={} version={version} len={len}",
                            file.base_name().display(),
                            data.len()
                        );
                    }
                }
                "event" => events += 1,
                _ => metadata += 1,
            }
            if kind != "dep" {
                let sample: String = data
                    .iter()
                    .take(64)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                samples
                    .entry(if kind == "event" { "event" } else { "metadata" })
                    .or_default()
                    .insert(sample);
            }
        }
    }

    println!("wwise_dep: {deps} found, {good} satisfy size == 8 + len");
    println!("versions seen: {versions:?}");
    println!("wwise_event: {events}   wwise_metadata: {metadata}");
    for (kind, set) in &samples {
        println!("--- {kind} samples ({} distinct):", set.len());
        for sample in set.iter().take(4) {
            println!("  {sample}");
        }
    }
    Ok(())
}
