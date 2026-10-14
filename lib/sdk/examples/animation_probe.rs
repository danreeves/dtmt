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
    let mut sjson_ok = 0usize;
    let mut sjson_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut joints: BTreeMap<usize, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut beats = 0usize;
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
        // The authoring path, which is what `dtmt build` takes: decode, write
        // SJSON, read it back, encode.
        match animation
            .to_sjson()
            .and_then(|text| Animation::from_sjson(&text))
            .and_then(|read| read.to_binary())
        {
            Ok(round) if round == bytes => sjson_ok += 1,
            Ok(_) => {
                *sjson_reasons
                    .entry("re-encoded differently through SJSON".to_string())
                    .or_default() += 1;
            }
            Err(e) => {
                *sjson_reasons.entry(e.to_string()).or_default() += 1;
            }
        }
        let rebuilt = animation.to_binary()?;
        if rebuilt == bytes {
            ok += 1;
            *joints.entry(animation.joints.len()).or_default() += 1;
            beats += animation.beats.len();
            tracks += animation.tracks.len();
            for track in &animation.tracks {
                let name = if track.packed {
                    format!("{} (packed)", track.kind)
                } else {
                    track.kind.clone()
                };
                *kinds.entry(name).or_default() += 1;
            }
        } else {
            failed += 1;
            if shown < 8 {
                shown += 1;
                println!(
                    "NOT-IDENTICAL {}  {} B -> {} B, joints={} items={}",
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
        println!("  {:.1}% identical", 100.0 * ok as f64 / files.len() as f64);
    }
    println!("  distinct joint counts: {:?}", joints.keys().take(6).collect::<Vec<_>>());
    println!("  beats: {beats}   items round-tripped: {tracks}");
    println!("  item kinds: {kinds:?}");
    println!(
        "  through SJSON (the build path): {sjson_ok} byte-identical, {} not",
        files.len() - sjson_ok
    );
    for (reason, count) in sjson_reasons.iter().take(6) {
        println!("    {count:5}  {reason}");
    }
    if !reasons.is_empty() {
        println!("--- reasons ({}) :", reasons.len());
        for (reason, count) in reasons.iter().take(12) {
            println!("  {count:5}  {reason}");
        }
    }
    Ok(())
}
