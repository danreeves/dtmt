//! EngineDatas: generating a `shader43` section from our own programs.
//!
//! A shader (the `gui` UI shader, the entity shader, ...) shares an
//! engine-side wrapper: the contexts the engine queries, the conditions tree
//! that selects a group, the group data with its variable tables, the packed
//! device preamble and one metadata tail per program. A [`EngineData`] captures
//! that wrapper once from a shipped base material, so a mod can generate a
//! complete section from its own compiled programs instead of shipping a
//! shipped shader blob.
//!
//! ```text
//! let engine_data = EngineData::from_material(&data)?;          // extract once per shader
//! std::fs::write("ui.engine_data", engine_data.to_text())?;
//!
//! let engine_data = EngineData::from_text(&text)?;              // generate from it
//! let section = engine_data.generate(&containers)?;         // Stage -> DXBC container
//! ```

use std::collections::HashMap;
use std::fs;

use color_eyre::eyre::{Context, Result, bail};

use super::shader::{self, Stage};
use crate::filetype::group_data::{Dependency, GroupData, GroupParts, GroupTemplate, Record};
use crate::murmur;

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// The engine's `global_viewport` record table, captured from the current game
/// build. It is engine-side data - the cbuffer schema the config records index
/// and the group data's engine table is written from - so the toolchain carries
/// it once rather than every mod.
const ENGINE_TABLE_HEX: &str = include_str!("../../data/global_viewport.hex");

/// The toolchain's engine table, parsed.
fn tool_engine_table() -> Result<Vec<Record>> {
    records_from_hex(ENGINE_TABLE_HEX.trim())
}

/// The length of a block channel record, by its `kind`: texture channels are
/// 60 bytes (kind 4) or 73 bytes (kind 5). Other kinds are only known to exist
/// (kind 2 is `global_texture2D`), not how long they are.
pub fn channel_record_len(kind: u32) -> Option<usize> {
    match kind {
        4 => Some(60),
        5 => Some(73),
        _ => None,
    }
}

fn from_hex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("hex string has an odd length");
    }

    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        bytes.push(
            u8::from_str_radix(std::str::from_utf8(pair)?, 16).wrap_err("invalid hex string")?,
        );
    }
    Ok(bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02X}");
    }
    text
}

/// A record table as hex: five little-endian words per record.
fn records_hex(records: &[Record]) -> String {
    let mut bytes = Vec::with_capacity(records.len() * 20);
    for record in records {
        for word in [
            record.kind,
            record.flags,
            record.hash,
            record.offset,
            record.size,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    to_hex(&bytes)
}

/// Reads a record table written by [`records_hex`].
fn records_from_hex(text: &str) -> Result<Vec<Record>> {
    let bytes = from_hex(text)?;
    if bytes.len() % 20 != 0 {
        bail!("a record table needs a multiple of 20 bytes");
    }
    Ok(bytes
        .chunks(20)
        .map(|chunk| Record {
            kind: u32::from_le_bytes(chunk[0..4].try_into().unwrap()),
            flags: u32::from_le_bytes(chunk[4..8].try_into().unwrap()),
            hash: u32::from_le_bytes(chunk[8..12].try_into().unwrap()),
            offset: u32::from_le_bytes(chunk[12..16].try_into().unwrap()),
            size: u32::from_le_bytes(chunk[16..20].try_into().unwrap()),
        })
        .collect())
}

/// The index of a value in a deduplicated list, adding it when it is new.
fn dedup_index(list: &mut Vec<Vec<u8>>, value: Vec<u8>) -> usize {
    match list.iter().position(|other| *other == value) {
        Some(index) => index,
        None => {
            list.push(value);
            list.len() - 1
        }
    }
}

/// Writes a block: its own bytes, or - when that is smaller - the preamble's
/// body with the bytes that differ patched in.
///
/// The block is the body with a per-program header in front and a few fields
/// patched (the UI shader's blocks are a 0- or 4-byte header and one patched
/// byte), so the diff is usually a few bytes against the 550-byte body.
fn block_spec(block: &[u8], preamble: &[u8]) -> String {
    let body = preamble.get(12..).unwrap_or_default();
    if body.is_empty() || block.len() < body.len() || block.len() - body.len() > 64 {
        return to_hex(block);
    }
    let header = block.len() - body.len();
    let patches: Vec<(usize, u8)> = (0..body.len())
        .filter(|at| block[header + at] != body[*at])
        .map(|at| (at, block[header + at]))
        .collect();
    // Each patch is five characters or more, so a diff only pays off when it is
    // small against the block.
    if 5 * patches.len() + header >= block.len() {
        return to_hex(block);
    }
    // `-` stands in for an empty header, so the patch list is never mistaken for
    // one.
    let header = if header == 0 {
        "-".to_string()
    } else {
        to_hex(&block[..header])
    };
    let mut out = format!("@body {header}");
    for (offset, value) in patches {
        out.push_str(&format!(" {offset:X}:{value:02X}"));
    }
    out
}

/// Reads a block written by [`block_spec`].
fn block_from_spec(spec: &str, preamble: &[u8]) -> Result<Vec<u8>> {
    // `@body` alone is the block that *is* the body: no header, no patches.
    let Some(rest) = spec.strip_prefix("@body") else {
        return from_hex(spec);
    };
    let mut fields = rest.trim_start().split_whitespace();
    let header_field = fields.next().unwrap_or("");
    let header = if header_field.is_empty() || header_field == "-" {
        Vec::new()
    } else {
        from_hex(header_field)?
    };
    let body = preamble
        .get(12..)
        .ok_or_else(|| color_eyre::eyre::eyre!("the preamble is too short to hold a body"))?;
    let header_len = header.len();
    let mut block = header;
    block.extend_from_slice(body);
    for patch in fields {
        let (offset, value) = patch
            .split_once(':')
            .ok_or_else(|| color_eyre::eyre::eyre!("malformed patch '{patch}'"))?;
        let offset = usize::from_str_radix(offset, 16)?;
        let value = u8::from_str_radix(value, 16)?;
        *block
            .get_mut(header_len + offset)
            .ok_or_else(|| color_eyre::eyre::eyre!("a patch is out of range"))? = value;
    }
    Ok(block)
}

/// The engine-side data of a shipped section: the parts the generator cannot
/// currently derive, captured once from a shipped material.
#[derive(Debug)]
pub struct EngineData {
    pub opaque: u32,
    pub context_count: u32,
    pub dependency_count: u32,
    pub contexts: Vec<u8>,
    pub conditions: Vec<u8>,
    pub dependencies: Vec<u8>,
    pub group_data: Vec<u8>,
    /// The group data's carried template, when its groups walk: the bytes a
    /// generator keeps while the material's tables are written. `None` when the
    /// groups do not walk, in which case `group_data` is the fallback.
    pub group_template: Option<GroupTemplate>,
    /// The material's own record tables, one per group, as the section wrote
    /// them. A build writes them back; a material that renames or re-sizes a
    /// variable writes the new records in their place.
    pub material_tables: Vec<Vec<Record>>,
    /// The packed table before the first program record.
    pub device_preamble: Vec<u8>,
    /// One entry per program of the template's device data, in order.
    pub programs: Vec<(Stage, Vec<u8>)>,
    /// Distinct program tails in the order `program` lines reference them.
    pub tails: Vec<Vec<u8>>,
    /// Distinct compiled containers, in first-use order. A build with sibling
    /// shader sources replaces these with freshly compiled ones; a build
    /// without sources carries them, so a reconstructed section keeps the
    /// shipped programs (which is what a mod that only edits the conditions or
    /// group data wants, and what keeps a variant-rich section's variants).
    pub containers: Vec<Vec<u8>>,
    /// The container index for each program, parallel to `programs`; `None`
    /// when the program carries no container (an engine data file from before
    /// the containers were captured).
    pub program_containers: Vec<Option<usize>>,
}

impl EngineData {
    /// Extracts the wrapper of a raw material data file's shader section.
    pub fn from_material(data: &[u8]) -> Result<Self> {
        if data.len() < 28 {
            bail!("material is too small");
        }

        let shader_offset = u32_at(data, 12) as usize;
        let shader_size = u32_at(data, 16) as usize;
        let section = data
            .get(shader_offset..shader_offset + shader_size)
            .ok_or_else(|| color_eyre::eyre::eyre!("shader section is out of range"))?;

        let slice = |start: usize, end: usize| -> Result<Vec<u8>> {
            section
                .get(start..end)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("section slice {start:#x}..{end:#x} out of range")
                })
        };

        let contexts = slice(u32_at(section, 8) as usize, u32_at(section, 16) as usize)?;
        let conditions = slice(u32_at(section, 16) as usize, u32_at(section, 24) as usize)?;
        let dependencies = slice(u32_at(section, 24) as usize, u32_at(section, 32) as usize)?;
        let group_data = slice(
            u32_at(section, 32) as usize,
            u32_at(section, 32) as usize + u32_at(section, 36) as usize,
        )?;

        // The group data's carried template and the material's own tables, when
        // the groups walk. A build writes the material's records back into the
        // template, so the engine's table and the group headers are carried once
        // instead of the whole group data.
        let parsed = shader::Section::parse(section)?;
        let query_ids: Vec<u32> = parsed
            .contexts()
            .iter()
            .flat_map(|context| context.queries.iter().map(|query| query.id))
            .collect();
        let group = GroupData::new(group_data.clone());
        let (group_template, material_tables) = match group.template(&query_ids) {
            Some(template) => {
                let tables = group
                    .object_tables(&query_ids)
                    .map(|tables| tables.into_iter().map(|(_, records)| records).collect())
                    .unwrap_or_default();
                (Some(template), tables)
            }
            None => (None, Vec::new()),
        };

        let device_offset = u32_at(section, 40) as usize;
        let device_size = u32_at(section, 44) as usize;
        let device = section
            .get(device_offset..device_offset + device_size)
            .ok_or_else(|| color_eyre::eyre::eyre!("device data is out of range"))?;
        let programs = shader::parse_programs(device)?;

        let first_program = programs.first().map(|program| program.pos).unwrap_or(0);
        let device_preamble = device
            .get(..first_program)
            .ok_or_else(|| color_eyre::eyre::eyre!("device preamble is out of range"))?
            .to_vec();

        // The compiled containers, deduplicated: the UI base's 96 programs
        // carry two distinct payloads, so this is what keeps the engine data
        // small enough to stay text.
        let mut containers: Vec<Vec<u8>> = Vec::new();
        let mut program_containers = Vec::with_capacity(programs.len());
        for program in &programs {
            let index = match containers.iter().position(|other| *other == program.container) {
                Some(index) => index,
                None => {
                    containers.push(program.container.clone());
                    containers.len() - 1
                }
            };
            program_containers.push(Some(index));
        }

        let programs = programs
            .iter()
            .map(|program| {
                let tail_start = program.meta_pos + 16;
                let tail_end = programs
                    .iter()
                    .find(|next| next.pos > tail_start)
                    .map(|next| next.pos)
                    .unwrap_or(device.len());
                let tail = device
                    .get(tail_start..tail_end)
                    .ok_or_else(|| color_eyre::eyre::eyre!("program tail is out of range"))?
                    .to_vec();
                Ok((program.stage, tail))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            opaque: u32_at(section, 4),
            context_count: u32_at(section, 12),
            dependency_count: u32_at(section, 28),
            contexts,
            conditions,
            dependencies,
            group_data,
            group_template,
            material_tables,
            device_preamble,
            programs,
            tails: Vec::new(),
            containers,
            program_containers,
        })
    }

    /// Parses the engine data from its text form.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut engine_data = Self {
            opaque: 0,
            context_count: 0,
            dependency_count: 0,
            contexts: Vec::new(),
            conditions: Vec::new(),
            dependencies: Vec::new(),
            group_data: Vec::new(),
            group_template: None,
            material_tables: Vec::new(),
            device_preamble: Vec::new(),
            programs: Vec::new(),
            tails: Vec::new(),
            containers: Vec::new(),
            program_containers: Vec::new(),
        };

        // The group data's template, when the groups walk. The lines may come in
        // any order, so they are collected first and assembled after the loop.
        let mut group_prefix: Option<Vec<u8>> = None;
        let mut engine_table: Option<Vec<Record>> = None;
        let mut materials: Vec<Vec<Record>> = Vec::new();
        let mut group_heads: Vec<Vec<u8>> = Vec::new();
        let mut group_betweens: Vec<Vec<u8>> = Vec::new();
        let mut group_mids: Vec<Vec<u8>> = Vec::new();
        let mut group_tails: Vec<Vec<u8>> = Vec::new();
        let mut groups: Vec<(GroupParts, usize)> = Vec::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Program lines carry the stage, a tail (hex or `#n`) and, when the
            // containers were captured, a container (hex or `#n`).
            if let Some(rest) = line.strip_prefix("program ") {
                let mut fields = rest.split_whitespace();
                let stage = fields
                    .next()
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed program line"))?;
                let tail = fields
                    .next()
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed program line"))?;
                let container = fields.next();
                let stage = match stage {
                    "Vertex" => Stage::Vertex,
                    "Pixel" => Stage::Pixel,
                    other => bail!("unsupported program stage '{other}'"),
                };
                let tail = match tail.strip_prefix('#') {
                    Some(index) => engine_data
                        .tails
                        .get(index.parse::<usize>()?)
                        .cloned()
                        .ok_or_else(|| color_eyre::eyre::eyre!("unknown tail #{index}"))?,
                    None => from_hex(tail)?,
                };
                let container = match container {
                    Some(field) => Some(match field.strip_prefix('#') {
                        Some(index) => engine_data
                            .containers
                            .get(index.parse::<usize>()?)
                            .cloned()
                            .ok_or_else(|| color_eyre::eyre::eyre!("unknown container #{index}"))?,
                        None => from_hex(field)?,
                    }),
                    None => None,
                };
                let container = match container {
                    Some(container) => {
                        let index = match engine_data
                            .containers
                            .iter()
                            .position(|other| *other == container)
                        {
                            Some(index) => index,
                            None => {
                                engine_data.containers.push(container);
                                engine_data.containers.len() - 1
                            }
                        };
                        Some(index)
                    }
                    None => None,
                };
                engine_data.programs.push((stage, tail));
                engine_data.program_containers.push(container);
                continue;
            }

            // Deduplicated tails, referenced by `program <stage> #n`.
            if let Some(rest) = line.strip_prefix("tail ") {
                let (index, hex) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed tail line"))?;
                let index = index.parse::<usize>()?;
                if engine_data.tails.len() <= index {
                    engine_data.tails.resize(index + 1, Vec::new());
                }
                engine_data.tails[index] = from_hex(hex)?;
                continue;
            }

            // The block that belongs to a tail, when it is written separately.
            // The tail line comes first, so the block is appended to it.
            if let Some(rest) = line.strip_prefix("block ") {
                let (index, spec) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                let block = block_from_spec(spec, &engine_data.device_preamble)?;
                let tail = engine_data
                    .tails
                    .get_mut(index)
                    .ok_or_else(|| color_eyre::eyre::eyre!("block #{index} has no tail"))?;
                tail.extend_from_slice(&block);
                continue;
            }

            // Deduplicated containers, referenced by `program <stage> #n #m`.
            if let Some(rest) = line.strip_prefix("container ") {
                let (index, hex) = rest
                    .split_once(' ')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed container line"))?;
                let index = index.parse::<usize>()?;
                if engine_data.containers.len() <= index {
                    engine_data.containers.resize(index + 1, Vec::new());
                }
                engine_data.containers[index] = from_hex(hex)?;
                continue;
            }

            // The group data's template: the prefix, the engine's table, the
            // distinct material tables and one `group` line per group, in group
            // order.
            if let Some(rest) = line.strip_prefix("group_prefix ") {
                group_prefix = Some(from_hex(rest)?);
                continue;
            }
            if let Some(rest) = line.strip_prefix("engine_table ") {
                engine_table = Some(records_from_hex(rest)?);
                continue;
            }
            if let Some(rest) = line.strip_prefix("material ") {
                // An empty table writes no hex, and the line trim eats the
                // trailing space with it.
                let (index, hex) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                if materials.len() <= index {
                    materials.resize(index + 1, Vec::new());
                }
                materials[index] = records_from_hex(hex)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("ghead ") {
                let (index, hex) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                if group_heads.len() <= index {
                    group_heads.resize(index + 1, Vec::new());
                }
                group_heads[index] = from_hex(hex)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("gbetween ") {
                let (index, hex) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                if group_betweens.len() <= index {
                    group_betweens.resize(index + 1, Vec::new());
                }
                group_betweens[index] = from_hex(hex)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("gmid ") {
                let (index, hex) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                if group_mids.len() <= index {
                    group_mids.resize(index + 1, Vec::new());
                }
                group_mids[index] = from_hex(hex)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("gtail ") {
                let (index, hex) = rest.split_once(' ').unwrap_or((rest, ""));
                let index = index.parse::<usize>()?;
                if group_tails.len() <= index {
                    group_tails.resize(index + 1, Vec::new());
                }
                group_tails[index] = from_hex(hex)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("group ") {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                // Six fields is the form that carries the four parts inline;
                // seven is the deduplicated form: query id, four part indexes,
                // the table order and the material reference.
                let (parts, index) = match fields.len() {
                    6 => (
                        GroupParts {
                            head: from_hex(fields[0])?,
                            material_first: fields[4] == "1",
                            between: from_hex(fields[1])?,
                            mid: from_hex(fields[2])?,
                            tail: from_hex(fields[3])?,
                        },
                        fields[5],
                    ),
                    7 => {
                        let query = u32::from_str_radix(fields[0], 16)?.to_le_bytes();
                        let part = |list: &Vec<Vec<u8>>, field: &str| -> Result<Vec<u8>> {
                            let index = field.parse::<usize>()?;
                            list.get(index)
                                .cloned()
                                .ok_or_else(|| color_eyre::eyre::eyre!("unknown group part #{index}"))
                        };
                        let mut head = query.to_vec();
                        head.extend_from_slice(&part(&group_heads, fields[1])?);
                        (
                            GroupParts {
                                head,
                                material_first: fields[5] == "1",
                                between: part(&group_betweens, fields[2])?,
                                mid: part(&group_mids, fields[3])?,
                                tail: part(&group_tails, fields[4])?,
                            },
                            fields[6],
                        )
                    }
                    _ => bail!("malformed group line"),
                };
                let index = index
                    .strip_prefix('#')
                    .ok_or_else(|| color_eyre::eyre::eyre!("malformed group line"))?
                    .parse::<usize>()?;
                groups.push((parts, index));
                continue;
            }

            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "opaque" => engine_data.opaque = value.parse()?,
                "context_count" => engine_data.context_count = value.parse()?,
                "dependency_count" => engine_data.dependency_count = value.parse()?,
                "contexts" => engine_data.contexts = from_hex(value)?,
                "conditions" => engine_data.conditions = from_hex(value)?,
                "dependencies" => engine_data.dependencies = from_hex(value)?,
                "group_data" => engine_data.group_data = from_hex(value)?,
                "device_preamble" => engine_data.device_preamble = from_hex(value)?,
                other => bail!("unknown engine data key '{other}'"),
            }
        }

        // Assemble the template once every line is in. The engine's table is the
        // toolchain's when the file does not carry one.
        if let Some(prefix) = group_prefix {
            let engine = match engine_table {
                Some(engine) => engine,
                None => tool_engine_table()?,
            };
            let mut parts = Vec::with_capacity(groups.len());
            let mut tables = Vec::with_capacity(groups.len());
            for (entry, index) in groups {
                tables.push(materials.get(index).cloned().ok_or_else(|| {
                    color_eyre::eyre::eyre!("group references material #{index}, which is missing")
                })?);
                parts.push(entry);
            }
            engine_data.group_template = Some(GroupTemplate {
                prefix,
                groups: parts,
                engine,
            });
            engine_data.material_tables = tables;
        }

        Ok(engine_data)
    }

    /// Serializes the engine data to its text form.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("opaque {}\n", self.opaque));
        text.push_str(&format!("context_count {}\n", self.context_count));
        // The dependency is the engine's one library (the renderer path's
        // hash), so it is only written when the file carries something else.
        if self.dependency_count != 0 && self.dependency_count != 1 {
            text.push_str(&format!("dependency_count {}\n", self.dependency_count));
        }
        text.push_str(&format!("contexts {}\n", to_hex(&self.contexts)));
        text.push_str(&format!("conditions {}\n", to_hex(&self.conditions)));
        let default_dependency = Dependency::of().write();
        if !self.dependencies.is_empty() && self.dependencies[..] != default_dependency[..] {
            text.push_str(&format!("dependencies {}\n", to_hex(&self.dependencies)));
        }
        match &self.group_template {
            Some(template) => {
                // The template: the prefix, the engine's table once, the distinct
                // material tables, the distinct group parts and one `group` line
                // per group.
                text.push_str(&format!("group_prefix {}\n", to_hex(&template.prefix)));
                // The engine's table is a toolchain constant, so it is only
                // written when the file carries a different one.
                let tool_table = tool_engine_table().unwrap_or_default();
                if template.engine != tool_table {
                    text.push_str(&format!(
                        "engine_table {}\n",
                        records_hex(&template.engine)
                    ));
                }
                let mut distinct_tables: Vec<&Vec<Record>> = Vec::new();
                let mut table_indexes = Vec::with_capacity(self.material_tables.len());
                for table in &self.material_tables {
                    match distinct_tables.iter().position(|other| **other == *table) {
                        Some(index) => table_indexes.push(index),
                        None => {
                            distinct_tables.push(table);
                            table_indexes.push(distinct_tables.len() - 1);
                        }
                    }
                }
                for (index, table) in distinct_tables.iter().enumerate() {
                    text.push_str(&format!("material {index} {}\n", records_hex(table)));
                }

                // The group parts dedupe independently: the heads (once their
                // query id is off), the bytes between the tables, the packed runs
                // and the condition-header tails all repeat across a shader's
                // groups.
                let mut heads: Vec<Vec<u8>> = Vec::new();
                let mut betweens: Vec<Vec<u8>> = Vec::new();
                let mut mids: Vec<Vec<u8>> = Vec::new();
                let mut tails: Vec<Vec<u8>> = Vec::new();
                let mut lines = Vec::with_capacity(template.groups.len());
                for parts in &template.groups {
                    // A group starts with its query id, which the contexts also
                    // carry, so it moves to the group line and the head keeps the
                    // rest.
                    let query = parts
                        .head
                        .get(..4)
                        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                        .unwrap_or(0);
                    let head = parts.head.get(4..).unwrap_or_default().to_vec();
                    let h = dedup_index(&mut heads, head);
                    let b = dedup_index(&mut betweens, parts.between.clone());
                    let m = dedup_index(&mut mids, parts.mid.clone());
                    let t = dedup_index(&mut tails, parts.tail.clone());
                    lines.push((query, h, b, m, t, parts.material_first));
                }
                for (name, list) in [
                    ("ghead", &heads),
                    ("gbetween", &betweens),
                    ("gmid", &mids),
                    ("gtail", &tails),
                ] {
                    for (index, bytes) in list.iter().enumerate() {
                        text.push_str(&format!("{name} {index} {}\n", to_hex(bytes)));
                    }
                }
                for ((query, h, b, m, t, first), material) in lines.iter().zip(&table_indexes) {
                    text.push_str(&format!(
                        "group {query:08X} {h} {b} {m} {t} {} #{material}\n",
                        if *first { 1 } else { 0 },
                    ));
                }
            }
            None => text.push_str(&format!("group_data {}\n", to_hex(&self.group_data))),
        }
        text.push_str(&format!(
            "device_preamble {}\n",
            to_hex(&self.device_preamble)
        ));

        // Deduplicate the tails: programs that share one reference the same
        // `tail` line, which shrinks engine data files a lot (the UI shader has 96
        // programs but only about 20 distinct tails).
        let mut distinct: Vec<&Vec<u8>> = Vec::new();
        let mut indexes = Vec::with_capacity(self.programs.len());
        for (_, tail) in &self.programs {
            match distinct.iter().position(|other| **other == *tail) {
                Some(index) => indexes.push(index),
                None => {
                    distinct.push(tail);
                    indexes.push(distinct.len() - 1);
                }
            }
        }
        for (index, tail) in distinct.iter().enumerate() {
            // A tail is written as its lists plus its block, and a block that is
            // the preamble's body with a few bytes patched - which is what the
            // UI shader's blocks are - is written as that diff instead of as its
            // own 550 bytes.
            let split = shader::Tail::parse(tail)
                .and_then(|parsed| {
                    shader::TailLists::parse(&parsed.rest)
                        .map(|lists| tail.len() - lists.block.len())
                })
                .unwrap_or(tail.len());
            let (lists, block) = tail.split_at(split);
            text.push_str(&format!("tail {index} {}\n", to_hex(lists)));
            text.push_str(&format!(
                "block {index} {}\n",
                block_spec(block, &self.device_preamble)
            ));
        }

        // The same for the containers: the UI base's 96 programs carry two
        // distinct payloads, so the dedup keeps the file small.
        let mut distinct_containers: Vec<&Vec<u8>> = Vec::new();
        let mut container_indexes = Vec::with_capacity(self.programs.len());
        for index in 0..self.programs.len() {
            let container = self
                .program_containers
                .get(index)
                .copied()
                .flatten()
                .and_then(|container| self.containers.get(container));
            match container {
                Some(container) => {
                    match distinct_containers
                        .iter()
                        .position(|other| **other == *container)
                    {
                        Some(index) => container_indexes.push(Some(index)),
                        None => {
                            distinct_containers.push(container);
                            container_indexes.push(Some(distinct_containers.len() - 1));
                        }
                    }
                }
                None => container_indexes.push(None),
            }
        }
        for (index, container) in distinct_containers.iter().enumerate() {
            text.push_str(&format!("container {index} {}\n", to_hex(container)));
        }

        for ((stage, _), (tail, container)) in self
            .programs
            .iter()
            .zip(indexes.iter().zip(&container_indexes))
        {
            match container {
                Some(container) => {
                    text.push_str(&format!("program {stage:?} #{tail} #{container}\n"))
                }
                None => text.push_str(&format!("program {stage:?} #{tail}\n")),
            }
        }

        text
    }

    /// Builds the device data: the engine data's preamble, then one framed program
    /// record per engine data program, using `containers[stage]` when the caller
    /// compiled one and the carried container otherwise, plus that program's tail.
    pub fn build_device(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        let mut device = self.device_preamble.clone();

        for (index, (stage, tail)) in self.programs.iter().enumerate() {
            let carried = self
                .program_containers
                .get(index)
                .copied()
                .flatten()
                .and_then(|container| self.containers.get(container));
            let container = containers.get(stage).or(carried).ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "no container given for program {index} ({stage:?}) and none carried in \
                     the engine data"
                )
            })?;

            let frame = shader::encode_frame(container)?;
            let key = murmur::hash(&frame, 0);

            // The input list describes the container's signature, so rebuild it
            // for whichever container this program gets: a mod shader with
            // different IO then still gets a tail that matches its programs.
            let tail = shader::Tail::parse(tail)
                .and_then(|parsed| parsed.with_inputs(container))
                .map(|rebuilt| rebuilt.bytes())
                .unwrap_or_else(|| tail.clone());

            device.extend_from_slice(&1u32.to_le_bytes());
            device.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            device.extend_from_slice(&frame);
            device.extend_from_slice(&5u32.to_le_bytes());
            device.extend_from_slice(&(container.len() as u32).to_le_bytes());
            device.extend_from_slice(&key.to_le_bytes());
            device.extend_from_slice(&tail);
        }

        Ok(device)
    }

    /// Assembles a complete shader section from the engine data and our programs.
    pub fn generate(&self, containers: &HashMap<Stage, Vec<u8>>) -> Result<Vec<u8>> {
        let device = self.build_device(containers)?;

        // The group data: written from the template and the material's tables
        // when the template is there, carried otherwise.
        let group_data = match &self.group_template {
            Some(template) => GroupData::build(template, &self.material_tables, &template.engine)?,
            None => self.group_data.clone(),
        };

        // The dependency is the engine's one library, so a file that does not
        // carry it gets the constant.
        let dependencies = if self.dependencies.is_empty() {
            Dependency::of().write().to_vec()
        } else {
            self.dependencies.clone()
        };
        let dependency_count = if self.dependency_count == 0 {
            1
        } else {
            self.dependency_count
        };

        let contexts_offset = 48usize;
        let conditions_offset = contexts_offset + self.contexts.len();
        let dependencies_offset = conditions_offset + self.conditions.len();
        let group_offset = dependencies_offset + dependencies.len();
        let device_offset = group_offset + group_data.len();
        let default_offset = device_offset + device.len();

        let header = [
            shader::VERSION,
            self.opaque,
            contexts_offset as u32,
            self.context_count,
            conditions_offset as u32,
            default_offset as u32,
            dependencies_offset as u32,
            dependency_count,
            group_offset as u32,
            group_data.len() as u32,
            device_offset as u32,
            device.len() as u32,
        ];

        let mut section = Vec::with_capacity(default_offset + 32);
        for word in header {
            section.extend_from_slice(&word.to_le_bytes());
        }
        section.extend_from_slice(&self.contexts);
        section.extend_from_slice(&self.conditions);
        section.extend_from_slice(&dependencies);
        section.extend_from_slice(&group_data);
        section.extend_from_slice(&device);
        section.extend_from_slice(&[0u8; 16]);
        while section.len() % 16 != 0 {
            section.push(0);
        }

        Ok(section)
    }

    /// Extracts the engine data straight from a material data file path.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let data = fs::read(path)?;
        Self::from_material(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_engine_data() -> EngineData {
        EngineData {
            opaque: 0,
            context_count: 0,
            dependency_count: 0,
            contexts: Vec::new(),
            conditions: Vec::new(),
            dependencies: Vec::new(),
            group_data: Vec::new(),
            group_template: None,
            material_tables: Vec::new(),
            device_preamble: Vec::new(),
            programs: Vec::new(),
            tails: Vec::new(),
            containers: Vec::new(),
            program_containers: Vec::new(),
        }
    }

    #[test]
    fn tails_round_trip_through_dedup() {
        let mut engine_data = empty_engine_data();
        let tail_a = vec![1u8, 2, 3, 4];
        let tail_b = vec![5u8, 6, 7, 8];
        engine_data.programs = vec![
            (Stage::Vertex, tail_a.clone()),
            (Stage::Pixel, tail_b.clone()),
            (Stage::Vertex, tail_a.clone()),
        ];

        let text = engine_data.to_text();
        assert!(text.contains("tail 0 01020304"));
        assert!(text.contains("tail 1 05060708"));
        assert!(text.contains("program Vertex #0"));
        assert!(text.contains("program Pixel #1"));
        assert_eq!(text.matches("program Vertex #0").count(), 2);

        let parsed = EngineData::from_text(&text).unwrap();
        assert_eq!(parsed.programs.len(), 3);
        assert_eq!(parsed.programs[0].1, tail_a);
        assert_eq!(parsed.programs[1].1, tail_b);
        assert_eq!(parsed.programs[2].1, tail_a);
    }

    #[test]
    fn containers_round_trip_and_carry_into_the_device() {
        let mut engine_data = empty_engine_data();
        let tail = vec![1u8, 2, 3, 4];
        let mut container_a = b"DXBC".to_vec();
        container_a.extend([0xAA; 60]);
        let mut container_b = b"DXBC".to_vec();
        container_b.extend([0xBB; 60]);
        engine_data.programs = vec![
            (Stage::Vertex, tail.clone()),
            (Stage::Pixel, tail.clone()),
            (Stage::Vertex, tail.clone()),
        ];
        engine_data.containers = vec![container_a.clone(), container_b.clone()];
        engine_data.program_containers = vec![Some(0), Some(1), Some(0)];

        let text = engine_data.to_text();
        assert!(text.contains("container 0 "), "{text}");
        assert!(text.contains("container 1 "), "{text}");
        assert!(text.contains("program Vertex #0 #0"), "{text}");
        assert!(text.contains("program Pixel #0 #1"), "{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        assert_eq!(parsed.containers, vec![container_a.clone(), container_b.clone()]);
        assert_eq!(parsed.program_containers, vec![Some(0), Some(1), Some(0)]);

        // With no compiled overrides the programs carry their own containers.
        let decode_all = |device: &[u8]| -> Vec<Vec<u8>> {
            let mut at = 0usize;
            let mut decoded = Vec::new();
            for _ in 0..3 {
                assert_eq!(
                    u32::from_le_bytes(device[at..at + 4].try_into().unwrap()),
                    1
                );
                let frame_length =
                    u32::from_le_bytes(device[at + 4..at + 8].try_into().unwrap()) as usize;
                let frame = &device[at + 8..at + 8 + frame_length];
                let decoded_length = u32::from_le_bytes(
                    device[at + 8 + frame_length + 4..at + 8 + frame_length + 8]
                        .try_into()
                        .unwrap(),
                ) as usize;
                decoded.push(shader::decode_frame(frame, decoded_length).unwrap());
                at += 8 + frame_length + 16 + 4; // the test's tails are four bytes
            }
            decoded
        };

        let device = parsed.build_device(&HashMap::new()).unwrap();
        assert_eq!(
            decode_all(&device),
            vec![container_a.clone(), container_b.clone(), container_a.clone()]
        );

        // A compiled override wins over the carried container.
        let mut overrides = HashMap::new();
        overrides.insert(Stage::Vertex, container_b.clone());
        let device = parsed.build_device(&overrides).unwrap();
        assert_eq!(
            decode_all(&device),
            vec![container_b.clone(), container_b.clone(), container_b.clone()]
        );
    }

    #[test]
    fn a_group_template_round_trips_and_rebuilds() {
        // A synthetic template: two groups, one material record each and one
        // engine record. The text form must carry the template back and the
        // generated section must hold exactly the bytes `build` produced.
        use crate::filetype::group_data::{GroupData, GroupParts, GroupTemplate, Record};

        let engine = vec![Record {
            kind: 2,
            flags: 0,
            hash: 0x6BC9_1D73,
            offset: 0,
            size: 12,
        }];
        let group = |material_first: bool| GroupParts {
            // The head ends with the first table's count word.
            head: vec![0u8; 40],
            material_first,
            between: vec![0u8; 12],
            mid: vec![0u8; 28],
            tail: vec![0u8; 8],
        };
        let template = GroupTemplate {
            prefix: 2u32.to_le_bytes().to_vec(),
            groups: vec![group(true), group(false)],
            engine: engine.clone(),
        };
        let material = vec![
            vec![Record {
                kind: 5,
                flags: 0,
                hash: 0xE503_152C,
                offset: 0,
                size: 4,
            }],
            Vec::new(),
        ];
        let built = GroupData::build(&template, &material, &engine).unwrap();

        let mut engine_data = empty_engine_data();
        engine_data.group_data = built.clone();
        engine_data.group_template = Some(template);
        engine_data.material_tables = material.clone();

        let parsed = EngineData::from_text(&engine_data.to_text()).unwrap();
        assert!(parsed.group_template.is_some());
        assert_eq!(parsed.material_tables, material);

        let section = parsed.generate(&HashMap::new()).unwrap();
        let offset = u32::from_le_bytes(section[32..36].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(section[36..40].try_into().unwrap()) as usize;
        assert_eq!(&section[offset..offset + size], &built[..]);
    }

    #[test]
    fn a_block_that_is_the_body_round_trips() {
        // The diff form degenerates to a bare `@body` when the block *is* the
        // body, and the parser has to read that back - the synthesized minimal
        // wrapper's blocks are exactly the body.
        let preamble: Vec<u8> = (0..60u8).collect();
        let body = preamble[12..].to_vec();
        let mut patched = body.clone();
        patched[5] = 0xFF;
        let mut headed = vec![2u8, 0, 0, 0];
        headed.extend_from_slice(&body);
        for block in [body.clone(), patched, headed] {
            let spec = block_spec(&block, &preamble);
            let parsed = block_from_spec(&spec, &preamble).unwrap();
            assert_eq!(parsed, block, "spec '{spec}'");
        }
    }

    #[test]
    fn a_rewrite_line_is_refused() {
        // The engine data used to carry DTMT-only `variable`/`clone` lines; they are
        // gone, and a file that still has one must fail rather than silently
        // ignore it.
        let err = EngineData::from_text("variable dev_wireframe_color mod_tint 224 16\n")
            .expect_err("a rewrite line is not an engine data key");
        assert!(err.to_string().contains("unknown engine data key"), "{err}");
    }
}