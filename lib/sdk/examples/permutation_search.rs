//! Searches for the permutation keys behind a section's query ids - or behind
//! a list of compiled shader library file hashes - over a vocabulary of shader
//! names, contexts and define tokens.
//!
//! ```text
//! permutation_search <targets> <vocabulary> <shaders> <contexts> [max defines] [platform] [renderer]
//! ```
//!
//! `targets` is a file with one hex value per line: 32-bit query ids (as a
//! section's `contexts` blob carries them) or 64-bit compiled library file
//! hashes (as a bundle database knows them). `vocabulary` is one define token
//! per line; `shaders` and `contexts` are comma-separated lists. For every
//! shader x context x sorted define subset up to `max defines` (default 2) the
//! tool builds the canonical key and hashes it both as a permutation id (high
//! 32 bits of murmur64a of the full key) and as a library file name
//! (murmur64a of the lowercased key plus `.shader_library`), printing every
//! key whose value is in the target set.

use std::collections::HashSet;
use std::error::Error;
use std::fs;

use sdk::filetype::permutation::{id, key, library_hash, library_key};

fn hex_set(path: &str) -> Result<(HashSet<u32>, HashSet<u64>), Box<dyn Error>> {
    let mut ids = HashSet::new();
    let mut hashes = HashSet::new();
    for line in fs::read_to_string(path)?.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        let v = u64::from_str_radix(t, 16)?;
        if t.len() <= 8 {
            ids.insert(v as u32);
        } else {
            hashes.insert(v);
        }
    }
    Ok((ids, hashes))
}

/// Calls `f` for every subset of `vocab` (sorted, in lexicographic order) with
/// at most `max` elements, `cur` holding the subset while it runs.
fn each_combo<'a>(
    vocab: &[&'a str],
    start: usize,
    max: usize,
    cur: &mut Vec<&'a str>,
    f: &mut impl FnMut(&[&'a str]),
) {
    f(cur);
    if cur.len() == max {
        return;
    }
    for i in start..vocab.len() {
        cur.push(vocab[i]);
        each_combo(vocab, i + 1, max, cur, f);
        cur.pop();
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: permutation_search <targets> <vocabulary> <shaders> <contexts> \
             [max defines] [platform] [renderer]"
        );
        return Ok(());
    }

    let (ids, hashes) = hex_set(&args[1])?;
    let mut vocab: Vec<String> = fs::read_to_string(&args[2])?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(str::to_string)
        .collect();
    vocab.sort_unstable();

    let shaders: Vec<&str> = args[3].split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let contexts: Vec<&str> = args[4].split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let max_defines: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(2);
    let platform = args.get(6).map(String::as_str).unwrap_or("WIN32");
    let renderer = args.get(7).map(String::as_str).unwrap_or("D3D12");

    println!(
        "targets: {} ids, {} hashes | vocabulary: {} tokens | shaders: {} | contexts: {} | up to {} defines",
        ids.len(),
        hashes.len(),
        vocab.len(),
        shaders.len(),
        contexts.len(),
        max_defines
    );

    let vocab_refs: Vec<&str> = vocab.iter().map(String::as_str).collect();
    let mut hits = 0usize;
    let mut tested = 0usize;

    for shader in &shaders {
        for context in &contexts {
            let mut cur: Vec<&str> = Vec::new();
            each_combo(&vocab_refs, 0, max_defines, &mut cur, &mut |defines| {
                tested += 1;
                let base = library_key(shader, context, defines);
                let full = key(shader, context, defines, platform, renderer);

                let file = library_hash(&base);
                if hashes.contains(&file) {
                    println!("library hit  {file:016x}  {base}.shader_library");
                    hits += 1;
                }
                let perm = id(&full);
                if ids.contains(&perm) {
                    println!("id hit       {perm:08x}  {full}");
                    hits += 1;
                }
            });
        }
    }

    println!("tested {tested} keys, {hits} hits");
    Ok(())
}
