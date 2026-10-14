//! Probe the physics scenes units carry.
//!
//! A unit's `physics` field is the native physics scene. This decompiles units,
//! pulls that field out of the resulting SJSON, decodes it with
//! `filetype::physics_scene` and re-encodes it, so the headline number is how many
//! shipped scenes round-trip byte-for-byte.
//!
//! Usage: `physics_probe <bundle dir> [max bundles]`  (default 120 bundles, since
//! decompiling units is the expensive part)

use std::collections::BTreeMap;

use sdk::filetype::physics_scene::PhysicsScene;
use sdk::{Bundle, BundleFileType, Context};

/// The `physics = "<hex>"` line a decompiled unit carries.
fn physics_hex(text: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(text);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("physics = \"") {
            return rest.strip_suffix('"').map(str::to_string);
        }
    }
    None
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let arg = std::env::args()
        .nth(1)
        .expect("usage: physics_probe <bundle dir> [max bundles]");
    let limit: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);

    let ctx = Context::new();
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&arg)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.len() == 16 && n.chars().all(|c| c.is_ascii_hexdigit())
                })
        })
        .collect();
    entries.sort();
    entries.truncate(limit);

    let mut bundles = 0usize;
    let mut units = 0usize;
    let mut with_physics = 0usize;
    let mut ok = 0usize;
    let mut sjson_ok = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut bodies = 0usize;
    let mut shapes = 0usize;
    let mut shown = 0usize;

    for path in &entries {
        let Ok(bin) = std::fs::read(path) else {
            continue;
        };
        let Ok(bundle) = Bundle::from_binary(&ctx, "probe".to_string(), bin) else {
            continue;
        };
        bundles += 1;
        for file in bundle.files() {
            if file.file_type() != BundleFileType::Unit {
                continue;
            }
            units += 1;
            let Ok((unit_sjson, _bsi)) = sdk::filetype::unit::decompile(file.variants()[0].data()) else {
                continue;
            };
            let Some(hex) = physics_hex(unit_sjson.as_bytes()) else {
                continue;
            };
            with_physics += 1;
            let Some(bytes) = unhex(&hex) else {
                *reasons.entry("the physics field is not hex".to_string()).or_default() += 1;
                continue;
            };
            let scene = match PhysicsScene::from_binary(&bytes) {
                Ok(scene) => scene,
                Err(e) => {
                    if shown < 5 {
                        shown += 1;
                        println!("DECODE-FAIL {}  {} B  {e}", file.base_name().display(), bytes.len());
                    }
                    *reasons.entry(e.to_string()).or_default() += 1;
                    continue;
                }
            };
            bodies += scene.bodies.len();
            shapes += scene.shapes.len();
            if scene.to_binary()? == bytes {
                ok += 1;
            } else {
                *reasons.entry("re-encoded differently".to_string()).or_default() += 1;
            }
            if scene
                .to_sjson()
                .and_then(|text| PhysicsScene::from_sjson(&text))
                .and_then(|read| read.to_binary())
                .is_ok_and(|round| round == bytes)
            {
                sjson_ok += 1;
            }
        }
    }

    println!("physics: {with_physics} units with a physics scene, out of {units} units in {bundles} bundles");
    println!("  {ok} byte-identical, {sjson_ok} byte-identical through SJSON");
    println!("  {bodies} bodies, {shapes} shape bindings");
    for (reason, count) in reasons.iter().take(6) {
        println!("  {count:5}  {reason}");
    }
    Ok(())
}
