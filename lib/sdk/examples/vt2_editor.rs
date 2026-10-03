//! The VT2 `.editor` shader registry: the compiler's flattened list of the
//! shader variants it builds, one entry per compiled `.shader_library`.
//!
//! Each `.editor` file is a table of entries keyed by `"#ID[hex]"`:
//!
//! ```text
//! "#ID[678af39167d568bf]" = {
//!     defines = ""                 // a bare string of space-separated tokens,
//!     semantics = []               // or an array, or [""] for one empty token
//!     shader = "apply_fog"
//!     variables = []
//! }
//! ```
//!
//! The compiled library name is `<shader>:<defines in declaration order>`:
//!
//! - `defines = ""` -> `<shader>` (no colon);
//! - `defines = [ "" ]` -> `<shader>:` (one empty token);
//! - `defines = "A B"` -> `<shader>:a:b`.
//!
//! This tool checks that rule against `debug_file_index.sjson` (the compiler's
//! own list of what it built), then reports how much of that set the
//! `.shader_source` declarations alone can reproduce.
//!
//! ```text
//! vt2_editor <sdk root> [--sources <core dir>]
//! ```

use std::collections::BTreeSet;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One `"#ID[...]"` entry of a `.editor` registry.
#[derive(Clone, Debug, Deserialize)]
struct EditorEntry {
    /// The tokens, as a bare space-separated string or an array of strings.
    #[serde(default)]
    defines: Defines,
    /// The shader the entry builds.
    shader: String,
}

/// The `defines` field: a string, an array, or absent.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
enum Defines {
    /// `defines = "A B C"`.
    Text(String),
    /// `defines = [ "A" "B" ]`.
    List(Vec<String>),
    /// No `defines` key at all.
    #[default]
    None,
}

impl Defines {
    /// The tokens in declaration order. A string is split on whitespace; an
    /// array keeps its items (including an empty string, which is a real
    /// empty token); an absent key has no tokens.
    fn tokens(&self) -> Vec<String> {
        match self {
            Self::Text(text) => text.split_whitespace().map(str::to_string).collect(),
            Self::List(items) => items.clone(),
            Self::None => Vec::new(),
        }
    }
}

/// The key a `(shader, tokens)` pair compiles to.
fn key(shader: &str, tokens: &[String]) -> String {
    let mut out = shader.to_lowercase();
    for token in tokens {
        out.push(':');
        out.push_str(&token.to_lowercase());
    }
    out
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

/// Every editor entry across the `.editor` registry tree.
fn editor_entries(root: &Path) -> Result<Vec<EditorEntry>, Box<dyn Error>> {
    let mut paths = Vec::new();
    walk(root, "editor", &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path)?;
        let table: std::collections::BTreeMap<String, EditorEntry> = serde_sjson::from_str(&text)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        out.extend(table.into_values());
    }
    Ok(out)
}

/// The `.shader_library` names the compiler recorded, as keys.
fn compiled_keys(index: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut out = BTreeSet::new();
    for line in fs::read_to_string(index)?.lines() {
        let Some(name) = line.split(" = \"").nth(1).and_then(|r| r.split('"').next()) else {
            continue;
        };
        if let Some(base) = name.strip_suffix(".shader_library") {
            out.insert(base.to_string());
        }
    }
    Ok(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("usage: vt2_editor <sdk root> [--sources <core>]")?);
    let mut sources: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        if arg == "--sources" {
            sources = Some(PathBuf::from(args.next().ok_or("--sources needs a directory")?));
        }
    }

    let entries = editor_entries(&root.join("TEMP").join("editor"))?;
    let index = root
        .join("TEMP")
        .join("streamable_resources_compile")
        .join("debug_file_index.sjson");
    let compiled = compiled_keys(&index)?;

    let editor_keys: BTreeSet<String> = entries
        .iter()
        .map(|e| key(&e.shader, &e.defines.tokens()))
        .collect();

    println!("editor entries:   {}", entries.len());
    println!("editor keys:      {}", editor_keys.len());
    println!("compiled keys:    {}", compiled.len());
    println!("editor == compiled: {}", editor_keys == compiled);
    if editor_keys != compiled {
        println!("compiled not editor ({}):", compiled.difference(&editor_keys).count());
        for k in compiled.difference(&editor_keys).take(20) {
            println!("   {k}");
        }
        println!("editor not compiled ({}):", editor_keys.difference(&compiled).count());
        for k in editor_keys.difference(&compiled).take(20) {
            println!("   {k}");
        }
    }

    if let Some(core) = sources {
        let mut paths = Vec::new();
        walk(&core, "shader_source", &mut paths);
        println!("\nthe same set from the {} .shader_source files:", paths.len());
        let declared = declared_keys(&paths);
        println!("declared keys:    {}", declared.len());
        println!("covered by both:  {}", declared.intersection(&compiled).count());
        let missing: Vec<&String> = compiled.difference(&declared).collect();
        println!("compiled not declared: {}", missing.len());

        // For each missing key, is every one of its tokens available to that
        // shader as either a declared token or a condition name in the file?
        let vocab = shader_vocabulary(&paths);
        // The compiler's condition vocabulary appears to be global rather than
        // per-shader, so also collect every token name seen anywhere.
        let mut global: BTreeSet<String> = BTreeSet::new();
        for path in &paths {
            let Ok(text) = fs::read_to_string(path) else { continue };
            for (_, tokens) in declared_in(&text) {
                global.extend(tokens.into_iter().map(|t| t.to_lowercase()));
            }
            global.extend(condition_names(&text).into_iter().map(|t| t.to_lowercase()));
        }
        for k in &missing {
            let (shader, tokens) = split_key(k);
            if !vocab.contains_key(&shader.to_lowercase()) {
                println!("   {k}   [shader not declared anywhere]");
                continue;
            }
            let local = &vocab[&shader.to_lowercase()];
            let unknown_local: Vec<&str> = tokens
                .iter()
                .map(String::as_str)
                .filter(|t| !local.contains(*t))
                .collect();
            let unknown_global: Vec<&str> = tokens
                .iter()
                .map(String::as_str)
                .filter(|t| !global.contains(*t))
                .collect();
            if unknown_global.is_empty() {
                println!("   {k}   [tokens known (globally), combination not declared]");
            } else if unknown_local.is_empty() {
                println!("   {k}   [tokens known to the shader, combination not declared]");
            } else {
                println!(
                    "   {k}   [unknown tokens local: {} | global: {}]",
                    unknown_local.join(","),
                    unknown_global.join(",")
                );
            }
        }
    }
    Ok(())
}

/// Split a key into its shader and its tokens (the empty token preserved).
fn split_key(key: &str) -> (String, Vec<String>) {
    let mut parts = key.split(':');
    let shader = parts.next().unwrap_or("").to_string();
    let tokens = parts.map(str::to_string).collect();
    (shader, tokens)
}

/// For every shader named in the corpus, the set of tokens available to it:
/// its declared defines plus every condition name in the file that declares it.
fn shader_vocabulary(paths: &[PathBuf]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(path) else { continue };
        let conditions = condition_names(&text);
        for (shader, tokens) in declared_in(&text) {
            let entry = out.entry(shader.to_lowercase()).or_default();
            for token in tokens {
                entry.insert(token.to_lowercase());
            }
            for name in &conditions {
                entry.insert(name.to_lowercase());
            }
        }
    }
    out
}

/// Every `(shader, defines)` pair a `.shader_source` declares, as keys.
///
/// This reads the two declaration mechanisms the compiler flattens into the
/// `.editor` registries: `static_compile` entries, and `passes` inside the
/// `shaders` table (each pass names an `hlsl_shader` plus `defines`).
fn declared_keys(paths: &[PathBuf]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(path) else { continue };
        for (shader, tokens) in declared_in(&text) {
            out.insert(key(&shader, &tokens));
        }
    }
    out
}

/// The `(shader, tokens)` pairs one `.shader_source` text declares.
fn declared_in(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if !line.starts_with('{') {
            continue;
        }
        // `{ [if: "..."] [layer="..."] (shader|hlsl_shader) = "name" [defines=[...]] ... }`
        // The authoring dialect writes both `key="value"` and `key = "value"`.
        let Some(shader) = quoted_after(line, "shader").or_else(|| quoted_after(line, "hlsl_shader"))
        else {
            continue;
        };
        out.push((shader, defines_of(line)));
    }
    out
}

/// The condition names a `.shader_source` text introduces, whether by nesting a
/// table under `defined_X` / `ndefined_X`, testing `#if defined(X)` / `#ifdef X`
/// in code, or naming a material option's `define = "X"`.
fn condition_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let code = line.split("//").next().unwrap_or("");
        // A table key: `defined_X = {` / `ndefined_X = {`.
        for prefix in ["defined_", "ndefined_"] {
            if let Some(rest) = code.trim_start().strip_prefix(prefix)
                && let Some(name) = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .filter(|n| !n.is_empty())
            {
                out.insert(name.to_string());
            }
        }
        // A preprocessor test: `#if defined(X)`, `#ifdef X`, `#ifndef X`,
        // `#elif defined(X)`, or the bare `#elif X` / `#if X` form.
        for marker in ["#if defined(", "#ifdef ", "#ifndef ", "#elif defined("] {
            if let Some(rest) = code.split(marker).nth(1) {
                let name = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    out.insert(name.to_string());
                }
            }
        }
        // Bare `#if X` / `#elif X` where X is not a keyword form.
        for marker in ["#elif ", "#if "] {
            if let Some(rest) = code.split(marker).nth(1) {
                let name = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !name.is_empty() && !matches!(name, "defined" | "ifdef" | "ifndef") {
                    out.insert(name.to_string());
                }
            }
        }
        // A material option: `define="X"` / `define = "X"`.
        if let Some(name) = quoted_after(code, "define") {
            out.insert(name);
        }
    }
    out
}

/// The value of a `key` assignment on this line, up to the closing quote.
/// Accepts both `key="value"` and `key = "value"`.
fn quoted_after(line: &str, key: &str) -> Option<String> {
    let rest = line.split(key).nth(1)?;
    let rest = rest.trim_start().strip_prefix('=')?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let value = rest.split('"').next()?;
    Some(value.to_string())
}

/// The tokens inside a `defines = [ "A" "B" ]` or `defines = "A B"` on this
/// line. Accepts whitespace around the `=`.
fn defines_of(line: &str) -> Vec<String> {
    let Some(rest) = line.split("defines").nth(1) else {
        return Vec::new();
    };
    let Some(rest) = rest.trim_start().strip_prefix('=') else {
        return Vec::new();
    };
    let rest = rest.trim_start();
    if let Some(list) = rest.strip_prefix('[') {
        let list = list.split(']').next().unwrap_or("");
        return list
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
    }
    if let Some(text) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
        return text.split_whitespace().map(str::to_string).collect();
    }
    Vec::new()
}
