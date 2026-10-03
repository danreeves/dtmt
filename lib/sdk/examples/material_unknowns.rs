//! Correlates a material stream's trailing unknowns (`unk2`, `unk3`,
//! `unk2_data`) with the fields they might track, across a corpus of material
//! data files.
//!
//! The unknowns are carried, not modelled, so the stream is walked directly from
//! its documented layout (see `docs/File Type - Material.-.md`).
//!
//! ```text
//! material_unknowns <dir or file>...
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Returns `(channels, textures, contexts, reflections, variable_data, unk2, unk3, other)`.
#[allow(clippy::type_complexity)]
fn walk(data: &[u8]) -> Option<(u32, u32, u32, u32, u32, Vec<(u32, bool)>, Vec<(u32, u32)>, u32)> {
    let version = u32_at(data, 0)?;
    if !(60..=62).contains(&version) || u32_at(data, 4)? != 28 {
        return None;
    }
    let shader_offset = u32_at(data, 12)?;
    let shader_size = u32_at(data, 16)?;
    let unk2_offset = u32_at(data, 20)?;
    let unk2_size = u32_at(data, 24)?;
    let _ = (shader_offset, shader_size);

    // The template at offset 28: name, material1, material2, then counted lists.
    let mut at = 28usize + 4 + 8 + 8;
    let channels = u32_at(data, at)?;
    at += 4 + channels as usize * 4;
    let textures = u32_at(data, at)?;
    at += 4 + textures as usize * 12;
    let contexts = u32_at(data, at)?;
    at += 4 + contexts as usize * 8;
    let reflections = u32_at(data, at)?;
    at += 4 + reflections as usize * 20;
    let data_len = u32_at(data, at)?;
    at += 4 + data_len as usize;
    let unk2_count = u32_at(data, at)?;
    at += 4;
    let mut unk2 = Vec::new();
    for _ in 0..unk2_count {
        let name = u32_at(data, at)?;
        let value = *data.get(at + 4)? != 0;
        unk2.push((name, value));
        at += 5;
    }
    let unk3_count = u32_at(data, at)?;
    at += 4;
    let mut unk3 = Vec::new();
    for _ in 0..unk3_count {
        unk3.push((u32_at(data, at)?, u32_at(data, at + 4)?));
        at += 8;
    }
    let other = if unk2_size > 0 && unk2_offset != u32::MAX {
        u32_at(data, unk2_offset as usize).unwrap_or(0)
    } else {
        0
    };
    Some((
        channels,
        textures,
        contexts,
        reflections,
        data_len,
        unk2,
        unk3,
        other,
    ))
}

fn walk_dir(path: &Path, visit: &mut impl FnMut(&Path)) {
    if path.is_file() {
        visit(path);
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        walk_dir(&entry.path(), visit);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("usage: material_unknowns <dir or file>...".into());
    }

    // unk3 signature -> count, plus the correlated shapes.
    let mut unk3_shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut unk2_shapes: BTreeMap<String, usize> = BTreeMap::new();
    // (channels, textures) -> unk3 signature, to see whether a tracks a count.
    let mut correlation: BTreeMap<(u32, u32), BTreeMap<String, usize>> = BTreeMap::new();
    let mut total = 0usize;
    let mut parsed = 0usize;

    for arg in &args {
        walk_dir(Path::new(arg), &mut |path| {
            total += 1;
            let Ok(data) = fs::read(path) else { return };
            let Some((channels, textures, _ctx, _refl, _data, unk2, unk3, _other)) = walk(&data)
            else {
                return;
            };
            parsed += 1;
            let unk3_sig = unk3
                .iter()
                .map(|(a, b)| format!("{a}:{b}"))
                .collect::<Vec<_>>()
                .join(" ");
            let unk2_sig = unk2
                .iter()
                .map(|(a, b)| format!("{a}:{}", *b as u8))
                .collect::<Vec<_>>()
                .join(" ");
            *unk3_shapes.entry(unk3_sig.clone()).or_default() += 1;
            *unk2_shapes.entry(unk2_sig).or_default() += 1;
            *correlation
                .entry((channels, textures))
                .or_default()
                .entry(unk3_sig)
                .or_default() += 1;
        });
    }

    println!("scanned {total}, material streams parsed {parsed}");
    println!("\nunk3 signatures (top 20):");
    let mut shapes: Vec<_> = unk3_shapes.iter().collect();
    shapes.sort_by(|a, b| b.1.cmp(a.1));
    for (sig, n) in shapes.iter().take(20) {
        println!("  x{n:5}  {sig:60}");
    }
    println!("\nunk2 signatures (top 5):");
    let mut s2: Vec<_> = unk2_shapes.iter().collect();
    s2.sort_by(|a, b| b.1.cmp(a.1));
    for (sig, n) in s2.iter().take(5) {
        println!("  x{n:5}  {sig:60}");
    }
    println!("\n(channels, textures) -> unk3 (top 15):");
    let mut corr: Vec<_> = correlation.iter().collect();
    corr.sort_by_key(|((c, t), _)| (*c, *t));
    for ((c, t), sigs) in corr.iter().take(15) {
        let mut list: Vec<_> = sigs.iter().collect();
        list.sort_by(|a, b| b.1.cmp(a.1));
        let shown = list
            .iter()
            .take(2)
            .map(|(s, n)| format!("[{s}] x{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("  ch={c:2} tx={t:2}  {shown}");
    }
    Ok(())
}
