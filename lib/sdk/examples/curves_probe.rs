//! Probe the `animation_curves` resources.
//!
//! `animation_curves` (`0xdcfb9e18fff13984`) is the one animation-family type DTMT
//! still ships opaquely, and neither reference on disk reads it, so its layout has
//! to come from shipped files. This sweep finds them, reports their sizes, and dumps
//! a few so the header can be read.
//!
//! Usage: `curves_probe <bundle dir or file> [max bundles]`

use std::collections::BTreeMap;

use sdk::{Bundle, BundleFileType, Context};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn dump(label: &str, data: &[u8]) {
    println!("--- {label}  ({} B)", data.len());
    for row in (0..data.len().min(96)).step_by(16) {
        let end = (row + 16).min(data.len());
        let bytes = &data[row..end];
        let words: Vec<String> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()).to_string())
            .collect();
        let floats: Vec<String> = bytes
            .chunks_exact(4)
            .map(|c| format!("{:.4}", f32::from_le_bytes(c.try_into().unwrap())))
            .collect();
        println!("  {row:04x}  {}", hex(bytes));
        println!("        u32 [{}]", words.join(", "));
        println!("        f32 [{}]", floats.join(", "));
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: curves_probe <bundle dir or file> [max bundles]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    // An optional type id, so the probe can look at any type and not just the one
    // that motivated it.
    let filter: Option<u64> = std::env::args().nth(3).map(|s| {
        u64::from_str_radix(s.trim_start_matches("0x"), 16)
            .expect("the type filter must be a hex id")
    });

    let ctx = Context::new();
    let root = std::path::Path::new(&arg);
    let mut entries: Vec<std::path::PathBuf> = if root.is_file() {
        vec![root.to_path_buf()]
    } else {
        std::fs::read_dir(root)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            // A bundle's name is its 16-hex-digit hash; everything else in the
            // directory is a sibling index or a stream.
            .filter(|p| {
                p.is_file()
                    && p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.len() == 16 && n.chars().all(|c| c.is_ascii_hexdigit())
                    })
            })
            .collect()
    };
    entries.sort();
    entries.truncate(limit);

    let mut bundles = 0usize;
    let mut found = 0usize;
    let mut named = 0usize;
    let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
    let mut samples: Vec<(String, Vec<u8>)> = Vec::new();

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            let kind = file.file_type();
            let wanted = match filter {
                Some(id) => u64::from(kind) == id,
                None => kind == BundleFileType::AnimationCurves,
            };
            if !wanted {
                continue;
            }
            found += 1;
            let name = file.base_name().display().to_string();
            if !name.chars().all(|c| c.is_ascii_hexdigit()) {
                named += 1;
            }
            let data = file.variants()[0].data();
            *sizes.entry(data.len()).or_default() += 1;
            if samples.len() < 5 {
                samples.push((name, data.to_vec()));
            }
        }
    }

    println!(
        "{found} files of the wanted type in {bundles} bundles ({named} with a real name)"
    );
    println!("  distinct sizes (up to 10): {:?}", sizes.keys().take(10).collect::<Vec<_>>());
    for (name, data) in &samples {
        dump(name, data);
    }
    Ok(())
}
