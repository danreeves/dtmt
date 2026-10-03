//! Verifies the VT2 shader-key rule against the official toolchain's own data:
//! every `static_compile` entry in the SDK's `.shader_source` files declares a
//! `shader` and a `defines` list; the rule says the compiled library key is
//! `<shader>:<defines sorted, lowercased>`. `debug_file_index.sjson` lists what
//! was actually compiled, so the rule is checked against it directly.
//!
//! ```text
//! vt2_key_check <sdk core dir> <debug_file_index.sjson>
//! ```

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "shader_source") {
            out.push(path);
        }
    }
}

/// Every `static_compile` entry's `(shader, defines)` in one source text.
fn static_compiles(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("static_compile") {
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        if line.starts_with(']') {
            in_block = false;
            continue;
        }
        // { if: "…#" shader="name" defines=["A" "B"] }  (or no defines)
        let Some(shader) = line
            .split("shader=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
        else {
            continue;
        };
        let mut defines = Vec::new();
        if let Some(rest) = line.split("defines=[").nth(1)
            && let Some(list) = rest.split(']').next()
        {
            for item in list.split('"') {
                let item = item.trim();
                if !item.is_empty() && item != "," && !item.starts_with(',') {
                    defines.push(item.trim_matches(|c| c == ',' || c == ' ').to_string());
                }
            }
        }
        out.push((shader.to_string(), defines));
    }
    out
}

fn key(shader: &str, defines: &[String]) -> String {
    let mut tokens: Vec<String> = defines
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| d.to_lowercase())
        .collect();
    tokens.sort();
    if tokens.is_empty() {
        shader.to_lowercase()
    } else {
        format!("{}:{}", shader.to_lowercase(), tokens.join(":"))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let core = std::env::args().nth(1).ok_or("usage: vt2_key_check <core> <index>")?;
    let index = std::env::args().nth(2).ok_or("usage: vt2_key_check <core> <index>")?;

    let mut paths = Vec::new();
    walk(Path::new(&core), &mut paths);
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for path in &paths {
        let text = fs::read_to_string(path).unwrap_or_default();
        for (shader, defines) in static_compiles(&text) {
            declared.insert(key(&shader, &defines));
        }
    }

    let mut compiled: BTreeSet<String> = BTreeSet::new();
    for line in fs::read_to_string(&index)?.lines() {
        let Some(name) = line.split(" = \"").nth(1).and_then(|rest| rest.split('"').next()) else {
            continue;
        };
        if let Some(base) = name.strip_suffix(".shader_library") {
            compiled.insert(base.to_string());
        }
    }

    let matched = declared.intersection(&compiled).count();
    println!("declared keys: {}", declared.len());
    println!("compiled keys: {}", compiled.len());
    println!("matched:       {matched}");
    println!("compiled not declared ({}):", compiled.difference(&declared).count());
    for key in compiled.difference(&declared).take(10) {
        println!("   {key}");
    }
    Ok(())
}
