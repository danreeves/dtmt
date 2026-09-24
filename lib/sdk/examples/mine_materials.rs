//! Dumps the shader tables of raw Darktide material data files, for reverse
//! engineering. Useful to mine the whole game's materials and see how the
//! `shader43` structures vary across shader families.
//!
//! Usage:
//!   cargo run -p sdk --example mine_materials -- [--dict <dictionary.csv>] \
//!       [--out <dir>] <file or directory>...
//!
//! For every material it writes:
//!
//! - `materials.csv`: one row of section statistics per material,
//! - `variables.csv`: one row per group data variable record,
//! - `groups.csv`: one row per variable table, with the header words before it,
//! - `defaults.csv`: one row per default data entry,
//! - `contexts.csv`: one row per context entry (query id and condition node),
//! - `conditions.csv`: one row per condition hash,
//! - `tails.csv`: hash-like values in the program metadata tails, unique per
//!   material, with their first position.
//!
//! Names are resolved through the dictionary when one is given; otherwise the
//! short hashes are printed as they are.

use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use sdk::filetype::shader;
use sdk::murmur::Dictionary;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// The material stream header (version 60/61) points at the shader section.
fn shader_section(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 28 {
        return None;
    }

    let version = u32_at(data, 0);
    if version != 60 && version != 61 {
        return None;
    }
    if u32_at(data, 4) != 28 {
        return None;
    }

    let offset = u32_at(data, 12) as usize;
    let size = u32_at(data, 16) as usize;
    if size == 0 {
        return None;
    }

    data.get(offset..offset + size)
}

/// One group data variable record, as stored in the shader's group data.
#[derive(Clone, Copy, Debug)]
struct Variable {
    kind: u32,
    flags: u32,
    name_hash: u32,
    offset: u32,
    size: u32,
}

impl Variable {
    fn read(group: &[u8], at: usize) -> Option<Self> {
        const SIZES: [(u32, u32); 5] = [(0, 4), (1, 8), (2, 12), (3, 16), (4, 64)];

        if at + 20 > group.len() {
            return None;
        }

        let variable = Variable {
            kind: u32_at(group, at),
            flags: u32_at(group, at + 4),
            name_hash: u32_at(group, at + 8),
            offset: u32_at(group, at + 12),
            size: u32_at(group, at + 16),
        };

        if variable.kind > 12 || variable.flags > 3 || variable.offset > 4096 {
            return None;
        }
        if let Some(&(.., expected)) = SIZES.iter().find(|(code, ..)| *code == variable.kind)
            && variable.size != expected
        {
            return None;
        }

        Some(variable)
    }
}

/// A run of variable records whose preceding word is their count.
#[derive(Debug)]
struct VariableTable {
    /// Byte offset of the first record, relative to the group data.
    start: usize,
    variables: Vec<Variable>,
}

/// Finds every variable table in a group data blob.
///
/// Records are 20 bytes and tables sit at arbitrary alignments (some copies are
/// only byte aligned), so every offset is checked.
fn variable_tables(group: &[u8]) -> Vec<VariableTable> {
    let mut tables = Vec::new();
    let mut at = 0;

    while at + 20 <= group.len() {
        let mut count = 0;
        while Variable::read(group, at + count * 20).is_some() {
            count += 1;
        }

        if count >= 3 && at >= 4 && u32_at(group, at - 4) as usize == count {
            tables.push(VariableTable {
                start: at,
                variables: (0..count)
                    .filter_map(|index| Variable::read(group, at + index * 20))
                    .collect(),
            });
        }

        at += 1;
    }

    tables
}

/// Walks the contexts section: `(context name hash, query hash, node offset)`
/// per entry.
fn context_entries(shader: &[u8]) -> Vec<(u32, u32, u32)> {
    let mut entries = Vec::new();
    let contexts_offset = u32_at(shader, 8) as usize;
    let conditions_offset = u32_at(shader, 16) as usize;

    let mut at = contexts_offset;
    while at + 12 <= conditions_offset {
        let context = u32_at(shader, at);
        let count = u32_at(shader, at + 8) as usize;
        if count > 512 || at + 12 + count * 8 > conditions_offset {
            break;
        }

        for index in 0..count {
            let entry = at + 12 + index * 8;
            entries.push((context, u32_at(shader, entry), u32_at(shader, entry + 4)));
        }

        at += 12 + count * 8;
    }

    entries
}

/// Reads the condition hashes of the node at `offset` in the conditions section.
fn condition_hashes(shader: &[u8], offset: usize) -> Option<Vec<u32>> {
    let dependencies_offset = u32_at(shader, 24) as usize;
    if offset + 8 > dependencies_offset {
        return None;
    }

    let count = u16::from_le_bytes(shader[offset + 6..offset + 8].try_into().unwrap()) as usize;
    if count > 64 || offset + 8 + count * 4 > dependencies_offset {
        return None;
    }

    Some(
        (0..count)
            .map(|index| u32_at(shader, offset + 8 + index * 4))
            .collect(),
    )
}

/// One entry of the default data table.
#[derive(Clone, Copy, Debug)]
struct DefaultEntry {    name_hash: u32,
    element_count: u32,
    blob_offset: u32,
}

/// Decodes the default data block: a zero word, a count, `{name_hash,
/// element_count, blob_offset}` entries and a value blob.
fn default_data(block: &[u8]) -> Option<(Vec<DefaultEntry>, usize)> {
    if block.len() < 8 || u32_at(block, 0) != 0 {
        return None;
    }

    let count = u32_at(block, 4) as usize;
    if count > 256 || 8 + count * 12 > block.len() {
        return None;
    }

    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let at = 8 + index * 12;
        let entry = DefaultEntry {
            name_hash: u32_at(block, at),
            element_count: u32_at(block, at + 4),
            blob_offset: u32_at(block, at + 8),
        };

        if entry.element_count == 0 || entry.element_count > 64 {
            return None;
        }
        if entry.blob_offset as usize >= block.len() {
            return None;
        }

        entries.push(entry);
    }

    let blob = block.len() - (8 + count * 12);
    Some((entries, blob))
}

/// Stats of one material's shader section, accumulated while walking a file.
#[derive(Default)]
struct SectionStats {
    version: u32,
    shader_size: usize,
    contexts: u32,
    conditions_offset: u32,
    dependency_offset: u32,
    dependency_count: u32,
    group_size: u32,
    device_size: u32,
    default_offset: u32,
    programs: usize,
    vertex: usize,
    pixel: usize,
    tables: usize,
}

/// Collects the section statistics of a parsed shader section.
fn inspect(shader: &[u8], version: u32) -> SectionStats {
    let mut stats = SectionStats {
        version,
        shader_size: shader.len(),
        contexts: u32_at(shader, 12),
        conditions_offset: u32_at(shader, 16),
        dependency_offset: u32_at(shader, 24),
        dependency_count: u32_at(shader, 28),
        group_size: u32_at(shader, 36),
        device_size: u32_at(shader, 44),
        default_offset: u32_at(shader, 20),
        ..Default::default()
    };

    if let Some(group) = {
        let offset = u32_at(shader, 32) as usize;
        let size = stats.group_size as usize;
        shader.get(offset..offset + size)
    } {
        stats.tables = variable_tables(group).len();
    }

    if let Some(device) = {
        let offset = u32_at(shader, 40) as usize;
        let size = stats.device_size as usize;
        shader.get(offset..offset + size)
    } {
        if let Ok(programs) = shader::parse_programs(device) {
            stats.programs = programs.len();
            for program in &programs {
                match program.stage {
                    shader::Stage::Vertex => stats.vertex += 1,
                    shader::Stage::Pixel => stats.pixel += 1,
                    _ => {}
                }
            }
        }
    }

    stats
}

/// Walks `path` and calls `visit` for every file, without collecting them all
/// first (the game's data directory has hundreds of thousands of files).
fn walk(path: &Path, visit: &mut impl FnMut(&Path)) {
    if path.is_file() {
        visit(path);
        return;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else if let Some(size) = entry.metadata().ok().map(|meta| meta.len())
            && size >= 300
        {
            visit(&path);
        }
    }
}

/// Collects the shader hash bounty lists while mining: names that resolved a
/// hash seen in the shader data, and hashes that did not.
#[derive(Default)]
struct Bounties {
    known: std::collections::BTreeSet<String>,
    unknown: std::collections::BTreeMap<u32, std::collections::BTreeSet<&'static str>>,
}

impl Bounties {
    fn add(&mut self, names: &HashMap<u32, String>, hash: u32, area: &'static str) {
        match names.get(&hash) {
            Some(name) => {
                self.known.insert(name.clone());
            }
            None => {
                self.unknown.entry(hash).or_default().insert(area);
            }
        }
    }

    /// Writes `known.txt` and `unknown.txt` next to the dump.
    fn write(&self, dir: &Path) -> std::io::Result<()> {
        use std::fmt::Write;

        let mut known = String::new();
        for name in &self.known {
            let _ = writeln!(known, "{name}");
        }
        fs::write(dir.join("known.txt"), known)?;

        let mut unknown = String::new();
        let _ = writeln!(
            unknown,
            "# Hashes that occur in Darktide shader43 structures (contexts, conditions,"
        );
        let _ = writeln!(
            unknown,
            "# group data, program tails, variables, defaults) and that the dictionary"
        );
        let _ = writeln!(unknown, "# cannot name. One hash per line.");
        let _ = writeln!(unknown, "#");

        let mut areas: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
        for areas_of_hash in self.unknown.values() {
            for area in areas_of_hash {
                *areas.entry(area).or_default() += 1;
            }
        }
        let summary = areas
            .into_iter()
            .map(|(area, count)| format!("{area} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(unknown, "# Areas: {summary}");
        let _ = writeln!(unknown, "#");

        for hash in self.unknown.keys() {
            let _ = writeln!(unknown, "{hash:08X}");
        }
        fs::write(dir.join("unknown.txt"), unknown)?;

        Ok(())
    }
}

fn name_of(names: &HashMap<u32, String>, hash: u32) -> String {    names
        .get(&hash)
        .cloned()
        .unwrap_or_else(|| format!("{hash:08X}"))
}

/// Writes one CSV row, quoting the string fields.
fn row(out: &mut impl Write, fields: &[String]) -> std::io::Result<()> {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.write_all(b",")?;
        }
        if field.contains([',', '"']) {
            write!(out, "\"{}\"", field.replace('"', "\"\""))?;
        } else {
            out.write_all(field.as_bytes())?;
        }
    }
    out.write_all(b"\n")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut dictionary: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from(".");
    let mut paths: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dict" => {
                i += 1;
                dictionary = Some(PathBuf::from(
                    args.get(i).expect("--dict needs a dictionary"),
                ));
            }
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).expect("--out needs a directory"));
            }
            other => paths.push(PathBuf::from(other)),
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!(
            "usage: mine_materials [--dict <dictionary.csv>] [--out <dir>] \
             <file or directory>..."
        );
        std::process::exit(1);
    }

    let names: HashMap<u32, String> = match &dictionary {
        Some(path) => {
            let bytes = fs::read(path)?;
            let rt = tokio::runtime::Runtime::new()?;
            let dictionary = rt.block_on(Dictionary::from_csv(&bytes[..]))?;
            dictionary
                .entries()
                .iter()
                .map(|entry| (u32::from(entry.short()), entry.value().clone()))
                .collect()
        }
        None => HashMap::new(),
    };

    fs::create_dir_all(&out_dir)?;
    let mut materials = BufWriter::new(fs::File::create(out_dir.join("materials.csv"))?);
    let mut variables = BufWriter::new(fs::File::create(out_dir.join("variables.csv"))?);
    let mut groups = BufWriter::new(fs::File::create(out_dir.join("groups.csv"))?);
    let mut defaults = BufWriter::new(fs::File::create(out_dir.join("defaults.csv"))?);
    let mut contexts = BufWriter::new(fs::File::create(out_dir.join("contexts.csv"))?);
    let mut conditions = BufWriter::new(fs::File::create(out_dir.join("conditions.csv"))?);
    let mut tails = BufWriter::new(fs::File::create(out_dir.join("tails.csv"))?);

    row(
        &mut materials,
        &[
            "file".into(),
            "version".into(),
            "shader_size".into(),
            "contexts".into(),
            "conditions_offset".into(),
            "dependency_offset".into(),
            "dependency_count".into(),
            "group_size".into(),
            "device_size".into(),
            "default_offset".into(),
            "programs".into(),
            "vertex".into(),
            "pixel".into(),
            "tables".into(),
            "variables".into(),
            "defaults".into(),
        ],
    )?;
    row(
        &mut variables,
        &[
            "file".into(),
            "table".into(),
            "kind".into(),
            "flags".into(),
            "name_hash".into(),
            "name".into(),
            "offset".into(),
            "size".into(),
        ],
    )?;
    row(
        &mut groups,
        &[
            "file".into(),
            "table".into(),
            "start".into(),
            "count".into(),
            "before".into(),
        ],
    )?;
    row(
        &mut contexts,
        &[
            "file".into(),
            "context_hash".into(),
            "context_name".into(),
            "query_hash".into(),
            "query_name".into(),
            "node".into(),
        ],
    )?;
    row(
        &mut conditions,
        &[
            "file".into(),
            "node".into(),
            "hash".into(),
            "name".into(),
        ],
    )?;
    row(
        &mut tails,
        &[
            "file".into(),
            "program".into(),
            "stage".into(),
            "position".into(),
            "hash".into(),
            "name".into(),
        ],
    )?;
    row(
        &mut defaults,
        &[
            "file".into(),
            "name_hash".into(),
            "name".into(),
            "element_count".into(),
            "blob_offset".into(),
        ],
    )?;

    let mut files = 0usize;
    let mut seen = 0usize;
    let mut bounties = Bounties::default();

    for path in &paths {
        walk(path, &mut |file| {
            files += 1;

            match process(
                file,
                &mut materials,
                &mut variables,
                &mut groups,
                &mut defaults,
                &mut contexts,
                &mut conditions,
                &mut tails,
                &names,
                &mut bounties,
            ) {
                Ok(true) => seen += 1,
                Ok(false) => {}
                Err(err) => eprintln!("{}: {err}", file.display()),
            }

            if files % 2000 == 0 {
                eprintln!("{files} files read, {seen} material(s) so far");
                let _ = materials.flush();
                let _ = variables.flush();
                let _ = groups.flush();
                let _ = defaults.flush();
                let _ = contexts.flush();
                let _ = conditions.flush();
                let _ = tails.flush();
            }
        });
    }

    materials.flush()?;
    variables.flush()?;
    groups.flush()?;
    defaults.flush()?;
    contexts.flush()?;
    conditions.flush()?;
    tails.flush()?;

    bounties.write(&out_dir)?;

    println!(
        "wrote the dump of {seen} material(s) from {files} file(s) to {} ({} known, {} unknown hashes)",
        out_dir.display(),
        bounties.known.len(),
        bounties.unknown.len()
    );

    Ok(())
}

/// Writes the three CSV rows of one material data file. Returns whether the
/// file was a material with a shader section.
fn process(
    file: &Path,
    materials: &mut impl Write,
    variables: &mut impl Write,
    groups: &mut impl Write,
    defaults: &mut impl Write,
    contexts: &mut impl Write,
    conditions: &mut impl Write,
    tails: &mut impl Write,
    names: &HashMap<u32, String>,
    bounties: &mut Bounties,
) -> std::io::Result<bool> {
    let Some(data) = fs::read(file).ok() else {
        return Ok(false);
    };
    let Some(shader) = shader_section(&data) else {
        return Ok(false);
    };
    let stats = inspect(shader, u32_at(&data, 0));

    let group = {
        let offset = u32_at(shader, 32) as usize;
        let size = u32_at(shader, 36) as usize;
        shader.get(offset..offset + size)
    };

    let mut variable_rows = 0usize;
    if let Some(group) = group {
        for (index, table) in variable_tables(group).iter().enumerate() {
            // The words before the table's count word: the group header
            // descriptors for a group's first table, the scaffolding around the
            // compact copies for the others.
            let before = table
                .start
                .checked_sub(4 + 24 * 4)
                .map(|before_start| {
                    group[before_start..table.start - 4]
                        .chunks_exact(4)
                        .map(|chunk| {
                            let word = u32::from_le_bytes(chunk.try_into().unwrap());
                            if word >= 0x0001_0000 && word != u32::MAX {
                                bounties.add(names, word, "groups");
                            }
                            format!("{word:08X}")
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();

            row(
                groups,
                &[
                    file.display().to_string(),
                    index.to_string(),
                    table.start.to_string(),
                    table.variables.len().to_string(),
                    before,
                ],
            )?;

            for variable in &table.variables {
                variable_rows += 1;
                bounties.add(names, variable.name_hash, "variables");
                row(
                    variables,
                    &[
                        file.display().to_string(),
                        index.to_string(),
                        variable.kind.to_string(),
                        variable.flags.to_string(),
                        format!("{:08X}", variable.name_hash),
                        name_of(names, variable.name_hash),
                        variable.offset.to_string(),
                        variable.size.to_string(),
                    ],
                )?;
            }
        }
    }

    let mut default_rows = 0usize;
    if let Some(block) = {
        let offset = u32_at(shader, 20) as usize;
        shader.get(offset..)
    } && let Some((entries, _blob)) = default_data(block)
    {
        for entry in &entries {
            default_rows += 1;
            bounties.add(names, entry.name_hash, "defaults");
            row(
                defaults,
                &[
                    file.display().to_string(),
                    format!("{:08X}", entry.name_hash),
                    name_of(names, entry.name_hash),
                    entry.element_count.to_string(),
                    entry.blob_offset.to_string(),
                ],
            )?;
        }
    }

    // Contexts and their condition nodes.
    let entries = context_entries(shader);
    let mut seen_contexts = std::collections::HashSet::new();
    let mut seen_nodes = std::collections::HashSet::new();
    for (context, query, node) in &entries {
        bounties.add(names, *context, "contexts");
        bounties.add(names, *query, "contexts");
        if seen_contexts.insert((*context, *query, *node)) {
            row(
                contexts,
                &[
                    file.display().to_string(),
                    format!("{context:08X}"),
                    name_of(names, *context),
                    format!("{query:08X}"),
                    name_of(names, *query),
                    node.to_string(),
                ],
            )?;
        }
        if seen_nodes.insert(*node)
            && let Some(hashes) = condition_hashes(shader, *node as usize)
        {
            for hash in hashes {
                row(
                    conditions,
                    &[
                        file.display().to_string(),
                        node.to_string(),
                        format!("{hash:08X}"),
                        name_of(names, hash),
                    ],
                )?;
            }
        }
    }

    // Hash-like values anywhere in a program's metadata tail, one row per
    // unique value per material.
    let mut seen_tail = std::collections::HashSet::new();
    if let Some(device) = {
        let offset = u32_at(shader, 40) as usize;
        let size = u32_at(shader, 44) as usize;
        shader.get(offset..offset + size)
    } && let Ok(programs) = shader::parse_programs(device)
    {
        for program in &programs {
            let tail_start = program.meta_pos + 16;
            let tail_end = programs
                .iter()
                .find(|next| next.pos > tail_start)
                .map(|next| next.pos)
                .unwrap_or(device.len());
            let Some(tail) = device.get(tail_start..tail_end) else {
                continue;
            };

            for (index, chunk) in tail.chunks_exact(4).enumerate() {
                let value = u32::from_le_bytes(chunk.try_into().unwrap());
                if value < 0x0001_0000 || value == u32::MAX || !seen_tail.insert(value) {
                    continue;
                }
                row(
                    tails,
                    &[
                        file.display().to_string(),
                        program.index.to_string(),
                        format!("{:?}", program.stage),
                        (index * 4).to_string(),
                        format!("{value:08X}"),
                        name_of(names, value),
                    ],
                )?;
            }
        }
    }

    row(
        materials,
        &[
            file.display().to_string(),
            format!("{:08X}", stats.version),
            stats.shader_size.to_string(),
            stats.contexts.to_string(),
            stats.conditions_offset.to_string(),
            stats.dependency_offset.to_string(),
            stats.dependency_count.to_string(),
            stats.group_size.to_string(),
            stats.device_size.to_string(),
            stats.default_offset.to_string(),
            stats.programs.to_string(),
            stats.vertex.to_string(),
            stats.pixel.to_string(),
            stats.tables.to_string(),
            variable_rows.to_string(),
            default_rows.to_string(),
        ],
    )?;

    Ok(true)
}
