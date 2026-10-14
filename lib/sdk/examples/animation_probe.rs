//! Probe the `animation` resources.
//!
//! Walks a directory of extracted `.animation` files, decodes each through
//! `filetype::animation`, and re-encodes it: a byte-identical result means the
//! derived layout accounts for every byte. A rejection prints the reason, which is
//! the profile frontier rather than a guess.
//!
//! Usage: `animation_probe <dir>`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sdk::filetype::animation::Animation;

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "animation") {
            out.push(path);
        }
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let root = std::env::args()
        .nth(1)
        .expect("usage: animation_probe <dir of .animation files>");

    let mut files = Vec::new();
    collect(Path::new(&root), &mut files);
    files.sort();

    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut joints: BTreeMap<usize, usize> = BTreeMap::new();
    let mut markers: BTreeMap<u16, usize> = BTreeMap::new();
    let mut tracks = 0usize;
    let mut shown = 0usize;

    for path in &files {
        let bytes = std::fs::read(path)?;
        let animation = match Animation::from_binary(&bytes) {
            Ok(animation) => animation,
            Err(e) => {
                failed += 1;
                *reasons.entry(e.to_string()).or_default() += 1;
                continue;
            }
        };
        let rebuilt = animation.to_binary()?;
        if rebuilt == bytes {
            ok += 1;
            *joints.entry(animation.joints.len()).or_default() += 1;
            *markers.entry(animation.marker).or_default() += 1;
            tracks += animation.tracks.len();
        } else {
            failed += 1;
            if shown < 8 {
                shown += 1;
                println!(
                    "NOT-IDENTICAL {}  {} B -> {} B, joints={} tracks={}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    bytes.len(),
                    rebuilt.len(),
                    animation.joints.len(),
                    animation.tracks.len()
                );
            }
            *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
        }
    }

    println!("animation: {} files, {ok} byte-identical, {failed} not", files.len());
    if !files.is_empty() {
        println!(
            "  {:.1}% identical",
            100.0 * ok as f64 / files.len() as f64
        );
    }
    println!("  distinct joint counts: {:?}", joints.keys().take(6).collect::<Vec<_>>());
    println!("  markers: {markers:?}   control points round-tripped: {tracks}");
    if !reasons.is_empty() {
        println!("--- reasons ({}) :", reasons.len());
        for (reason, count) in reasons.iter().take(12) {
            println!("  {count:5}  {reason}");
        }
    }
    Ok(())
}
