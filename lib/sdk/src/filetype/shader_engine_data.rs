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
//! The text form is the same Stingray source dialect the `core/` files use -
//! `key = value`, `{}` tables, `[]` arrays, quoted hex blobs - so an engine data
//! file is a source file beside `.shader_node`, `.shader_source` and
//! `.material`, not a format of its own. What the build derives from those
//! sources is not stored: the group data's variable tables come from the
//! material, the tail inputs from the compiled containers, and the bytes a
//! field would repeat are omitted.
//!
//! ```text
//! let engine_data = EngineData::from_material(&data)?;          // extract once per shader
//! std::fs::write("ui.engine_data", engine_data.to_text())?;
//!
//! let engine_data = EngineData::from_text(&text)?;              // generate from it
//! let section = engine_data.generate(&containers, "materials/mods/x/base")?;  // Stage -> DXBC
//! ```

use std::collections::HashMap;
use std::fs;

use color_eyre::eyre::{Context, Result, bail};
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

/// The engine's standard config records: the 30 (index, value) records every
/// rich preamble starts with, captured from the current game build. Like the
/// engine table, this is engine-side data, so the toolchain carries it once and
/// a file only stores what it has beyond it.
const CONFIG_BASE_HEX: &str = include_str!("../../data/config_base.hex");

/// The toolchain's config base, parsed.
fn config_base() -> Result<Vec<u8>> {
    from_hex(CONFIG_BASE_HEX.trim())
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

/// A record table as its packed bytes: five little-endian words per record.
fn records_bytes(records: &[Record]) -> Vec<u8> {
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
    bytes
}

/// A record table as hex: five little-endian words per record.
fn records_hex(records: &[Record]) -> String {
    to_hex(&records_bytes(records))
}

/// Reads a record table written by [`records_bytes`].
fn records_from_bytes(bytes: &[u8]) -> Result<Vec<Record>> {
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

/// Reads a record table written by [`records_hex`].
fn records_from_hex(text: &str) -> Result<Vec<Record>> {
    records_from_bytes(&from_hex(text)?)
}

/// The index of a value in a deduplicated list, adding it when it is new.
fn dedup_index(list: &mut Vec<Hex>, value: Vec<u8>) -> usize {
    match list.iter().position(|other| other.0 == value) {
        Some(index) => index,
        None => {
            list.push(value.into());
            list.len() - 1
        }
    }
}

/// The engine data's text model, in the Stingray source dialect the `core/`
/// files use: `key = value`, `{}` tables, `[]` arrays and quoted blobs.
///
/// The section's `material_hash` word is not here: it is murmur32 of the
/// generating material's resource path, which [`EngineData::generate`] takes as
/// an argument.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    context_count: u32,
    /// Only written when it is not the engine's one dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency_count: Option<u32>,
    contexts: Hex,
    /// Empty blobs are not written.
    #[serde(default, skip_serializing_if = "Hex::is_empty")]
    conditions: Hex,
    /// Only written when it is not the engine's one dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependencies: Option<Hex>,
    preamble: PreambleText,
    /// The fallback when the group data has no template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_data: Option<Hex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_template: Option<GroupTemplateText>,
    /// Distinct compiled containers, referenced by `programs[].container`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    containers: Vec<Hex>,
    /// Distinct program tails, referenced by `programs[].tail`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tails: Vec<TailText>,
    programs: Vec<ProgramText>,
}

/// A quoted hex blob. Always quoted, so a hex string cannot read back as a
/// number or an identifier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Hex(Vec<u8>);

impl Hex {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Hex {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Serialize for Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(self.0.len() * 2 + 2);
        out.push('"');
        for byte in &self.0 {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02X}");
        }
        out.push('"');
        serializer.serialize_bytes(out.as_bytes())
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct HexVisitor;
        impl Visitor<'_> for HexVisitor {
            type Value = Hex;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a quoted hex blob")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                from_hex(v).map(Hex).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(HexVisitor)
    }
}

/// A string written quoted, the way the source files write names and kinds.
fn serialize_quoted<S: Serializer>(value: &str, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    serializer.serialize_bytes(out.as_bytes())
}

/// The device preamble: its head and rest around the engine's config base, or
/// its whole bytes when it does not start with the base.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreambleText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head: Option<Hex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rest: Option<Hex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bytes: Option<Hex>,
}

/// The group data's template: the shared prefix, the engine's table (only when
/// it is not the toolchain's), the distinct material tables and group parts,
/// and one entry per group.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupTemplateText {
    prefix: Hex,
    /// The engine's table as packed records, only when it is not the
    /// toolchain's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine: Option<Hex>,
    /// Distinct material record tables, as packed records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    materials: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    heads: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    betweens: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mids: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tails: Vec<Hex>,
    groups: Vec<GroupText>,
}

/// One group of the template: its query id and the parts it uses.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupText {
    query: Hex,
    #[serde(default)]
    head: usize,
    #[serde(default)]
    between: usize,
    #[serde(default)]
    mid: usize,
    #[serde(default)]
    tail: usize,
    #[serde(default)]
    material: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    material_first: bool,
}

/// One distinct program tail: the bytes before its block.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailText {
    lists: Hex,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    block: Option<BlockText>,
}

/// A block: its own bytes, or the preamble's body with a header in front and
/// the bytes that differ patched in.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum BlockText {
    Hex(Hex),
    Body(BlockDiff),
}

/// A block written as a diff against the preamble's body.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockDiff {
    /// The bytes before the body; absent when the block *is* the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head: Option<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patches: Vec<PatchText>,
}

/// One patched byte of a block: its offset into the body and the value.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchText {
    offset: u32,
    value: u32,
}

/// One program: its stage and the tail and container it uses.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramText {
    #[serde(serialize_with = "serialize_quoted")]
    stage: String,
    tail: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container: Option<usize>,
}

/// Writes a block as its own bytes, or - when that is smaller - as the
/// preamble's body with the bytes that differ patched in.
///
/// The block is the body with a per-program header in front and a few fields
/// patched (the UI shader's blocks are a 0- or 4-byte header and one patched
/// byte), so the diff is usually a few bytes against the 550-byte body.
fn block_text(block: &[u8], preamble: &[u8]) -> BlockText {
    let body = preamble.get(12..).unwrap_or_default();
    if body.is_empty() || block.len() < body.len() || block.len() - body.len() > 64 {
        return BlockText::Hex(block.to_vec().into());
    }
    let header = block.len() - body.len();
    let patches: Vec<PatchText> = (0..body.len())
        .filter(|at| block[header + at] != body[*at])
        .map(|at| PatchText {
            offset: at as u32,
            value: block[header + at] as u32,
        })
        .collect();
    // Each patch is a few characters, so a diff only pays off when it is small
    // against the block.
    if 5 * patches.len() + header >= block.len() {
        return BlockText::Hex(block.to_vec().into());
    }
    BlockText::Body(BlockDiff {
        head: (header != 0).then(|| block[..header].to_vec().into()),
        patches,
    })
}

/// Reads a block written by [`block_text`].
fn block_bytes(spec: &BlockText, preamble: &[u8]) -> Result<Vec<u8>> {
    match spec {
        BlockText::Hex(hex) => Ok(hex.0.clone()),
        BlockText::Body(diff) => {
            let body = preamble
                .get(12..)
                .ok_or_else(|| color_eyre::eyre::eyre!("the preamble is too short to hold a body"))?;
            let mut block = diff.head.clone().map(Hex::into_bytes).unwrap_or_default();
            let header_len = block.len();
            block.extend_from_slice(body);
            for patch in &diff.patches {
                let value = u8::try_from(patch.value)
                    .map_err(|_| color_eyre::eyre::eyre!("a patch value is out of range"))?;
                *block
                    .get_mut(header_len + patch.offset as usize)
                    .ok_or_else(|| color_eyre::eyre::eyre!("a patch is out of range"))? = value;
            }
            Ok(block)
        }
    }
}

/// The engine-side data of a shipped section: the parts the generator cannot
/// currently derive, captured once from a shipped material.
#[derive(Debug)]
pub struct EngineData {
    pub material_hash: u32,
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
            material_hash: u32_at(section, 4),
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
        let text: Text =
            serde_sjson::from_str(text).wrap_err("Failed to parse the engine data text")?;

        let mut engine_data = Self {
            // The section's identity comes from the material at `generate`
            // time, not from the file.
            material_hash: 0,
            context_count: text.context_count,
            dependency_count: text.dependency_count.unwrap_or(0),
            contexts: text.contexts.into_bytes(),
            conditions: text.conditions.into_bytes(),
            dependencies: text.dependencies.map(Hex::into_bytes).unwrap_or_default(),
            group_data: Vec::new(),
            group_template: None,
            material_tables: Vec::new(),
            device_preamble: Vec::new(),
            programs: Vec::new(),
            tails: Vec::new(),
            containers: Vec::new(),
            program_containers: Vec::new(),
        };

        // The preamble: the engine's config base is a toolchain constant, so a
        // file writes only what lies around it.
        engine_data.device_preamble =
            match (&text.preamble.head, &text.preamble.rest, &text.preamble.bytes) {
                (Some(head), Some(rest), None) => {
                    let mut preamble = head.0.clone();
                    preamble.extend_from_slice(&config_base()?);
                    preamble.extend_from_slice(&rest.0);
                    preamble
                }
                (None, None, Some(bytes)) => bytes.0.clone(),
                _ => bail!("a preamble is either its `head` and `rest` or its `bytes`"),
            };

        for container in text.containers {
            engine_data.containers.push(container.into_bytes());
        }

        // The group data: the carried bytes, or the template they are rebuilt
        // from.
        engine_data.group_data = text.group_data.map(Hex::into_bytes).unwrap_or_default();
        if let Some(template) = text.group_template {
            let engine = match &template.engine {
                Some(engine) => records_from_bytes(&engine.0)?,
                None => tool_engine_table()?,
            };
            let part = |list: &[Hex], index: usize, what: &str| -> Result<Vec<u8>> {
                list.get(index).map(|hex| hex.0.clone()).ok_or_else(|| {
                    color_eyre::eyre::eyre!("a group names a {what} at #{index}, which is missing")
                })
            };
            let mut groups = Vec::with_capacity(template.groups.len());
            let mut tables = Vec::with_capacity(template.groups.len());
            for group in &template.groups {
                // A group starts with its query id, which the contexts also
                // carry, so the head stores only what follows it.
                let mut head = group.query.0.clone();
                head.extend_from_slice(&part(&template.heads, group.head, "head")?);
                groups.push(GroupParts {
                    head,
                    material_first: group.material_first,
                    between: part(&template.betweens, group.between, "between")?,
                    mid: part(&template.mids, group.mid, "mid")?,
                    tail: part(&template.tails, group.tail, "tail")?,
                });
                tables.push(records_from_bytes(&part(
                    &template.materials,
                    group.material,
                    "material",
                )?)?);
            }
            engine_data.group_template = Some(GroupTemplate {
                prefix: template.prefix.into_bytes(),
                groups,
                engine,
            });
            engine_data.material_tables = tables;
        }

        // The tails: their lists, then the block that closes them.
        for tail in text.tails {
            let mut bytes = tail.lists.into_bytes();
            if let Some(block) = &tail.block {
                bytes.extend_from_slice(&block_bytes(block, &engine_data.device_preamble)?);
            }
            engine_data.tails.push(bytes);
        }

        for program in text.programs {
            let stage = match program.stage.as_str() {
                "Vertex" => Stage::Vertex,
                "Pixel" => Stage::Pixel,
                "Geometry" => Stage::Geometry,
                "Hull" => Stage::Hull,
                "Domain" => Stage::Domain,
                "Compute" => Stage::Compute,
                "Other" => Stage::Other,
                other => bail!("Unsupported program stage '{other}'"),
            };
            let tail = engine_data.tails.get(program.tail).cloned().ok_or_else(|| {
                color_eyre::eyre::eyre!("a program names tail #{}, which is missing", program.tail)
            })?;
            if let Some(container) = program.container
                && container >= engine_data.containers.len()
            {
                bail!("a program names container #{container}, which is missing");
            }
            engine_data.programs.push((stage, tail));
            engine_data.program_containers.push(program.container);
        }

        Ok(engine_data)
    }

    /// Serializes the engine data to its text form.
    pub fn to_text(&self) -> String {
        serde_sjson::to_string(&self.text_model()).expect("the engine data text serializes")
    }

    /// The text model for the current data: the fields laid out and
    /// deduplicated, in the Stingray source dialect (see the module docs).
    fn text_model(&self) -> Text {
        // The preamble's config records start with the engine's standard base, a
        // toolchain constant, so only what lies around it is stored.
        let base = config_base().unwrap_or_default();
        let starts_with_base = !base.is_empty()
            && self
                .device_preamble
                .get(16..16 + base.len())
                .is_some_and(|configs| configs == base.as_slice());
        let preamble = if starts_with_base {
            PreambleText {
                head: Some(self.device_preamble[..16].to_vec().into()),
                rest: Some(self.device_preamble[16 + base.len()..].to_vec().into()),
                bytes: None,
            }
        } else {
            PreambleText {
                head: None,
                rest: None,
                bytes: Some(self.device_preamble.clone().into()),
            }
        };

        let group_template = self.group_template.as_ref().map(|template| {
            // The engine's table is a toolchain constant, so it is only written
            // when the file carries a different one.
            let tool_table = tool_engine_table().unwrap_or_default();
            let engine = (template.engine != tool_table).then(|| records_bytes(&template.engine).into());

            // Distinct material tables, referenced by the groups.
            let mut materials: Vec<Hex> = Vec::new();
            let mut material_indexes = Vec::with_capacity(self.material_tables.len());
            for table in &self.material_tables {
                let bytes = records_bytes(table);
                material_indexes.push(match materials.iter().position(|other| other.0 == bytes) {
                    Some(index) => index,
                    None => {
                        materials.push(bytes.into());
                        materials.len() - 1
                    }
                });
            }

            // The group parts dedupe independently: the heads (once their query
            // id is off), the bytes between the tables, the packed runs and the
            // condition-header tails all repeat across a shader's groups.
            let mut heads: Vec<Hex> = Vec::new();
            let mut betweens: Vec<Hex> = Vec::new();
            let mut mids: Vec<Hex> = Vec::new();
            let mut tails: Vec<Hex> = Vec::new();
            let mut groups = Vec::with_capacity(template.groups.len());
            for (index, parts) in template.groups.iter().enumerate() {
                let query = parts
                    .head
                    .get(..4)
                    .map(|bytes| Hex(bytes.to_vec()))
                    .unwrap_or_default();
                groups.push(GroupText {
                    query,
                    head: dedup_index(&mut heads, parts.head.get(4..).unwrap_or_default().to_vec()),
                    between: dedup_index(&mut betweens, parts.between.clone()),
                    mid: dedup_index(&mut mids, parts.mid.clone()),
                    tail: dedup_index(&mut tails, parts.tail.clone()),
                    material: material_indexes[index],
                    material_first: parts.material_first,
                });
            }

            GroupTemplateText {
                prefix: template.prefix.clone().into(),
                engine,
                materials,
                heads,
                betweens,
                mids,
                tails,
                groups,
            }
        });

        // Deduplicate the tails: programs that share one reference the same
        // entry, which shrinks engine data files a lot (the UI shader has 96
        // programs but only about 20 distinct tails).
        let mut tails: Vec<&Vec<u8>> = Vec::new();
        let mut tail_indexes = Vec::with_capacity(self.programs.len());
        for (_, tail) in &self.programs {
            match tails.iter().position(|other| **other == *tail) {
                Some(index) => tail_indexes.push(index),
                None => {
                    tails.push(tail);
                    tail_indexes.push(tails.len() - 1);
                }
            }
        }
        let tails: Vec<TailText> = tails
            .iter()
            .map(|tail| {
                // A tail is written as its lists plus its block, and a block that
                // is the preamble's body with a few bytes patched - which is what
                // the UI shader's blocks are - is written as that diff instead of
                // as its own 550 bytes.
                let split = shader::Tail::parse(tail)
                    .and_then(|parsed| {
                        shader::TailLists::parse(&parsed.rest)
                            .map(|lists| tail.len() - lists.block.len())
                    })
                    .unwrap_or(tail.len());
                let (lists, block) = tail.split_at(split);
                TailText {
                    lists: lists.to_vec().into(),
                    block: (!block.is_empty()).then(|| block_text(block, &self.device_preamble)),
                }
            })
            .collect();

        // The same for the containers: the UI base's 96 programs carry two
        // distinct payloads, so the dedup keeps the file small.
        let mut containers: Vec<&Vec<u8>> = Vec::new();
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
                    match containers.iter().position(|other| **other == *container) {
                        Some(index) => container_indexes.push(Some(index)),
                        None => {
                            containers.push(container);
                            container_indexes.push(Some(containers.len() - 1));
                        }
                    }
                }
                None => container_indexes.push(None),
            }
        }

        let programs = self
            .programs
            .iter()
            .zip(tail_indexes.iter().zip(&container_indexes))
            .map(|((stage, _), (tail, container))| ProgramText {
                stage: match stage {
                    Stage::Vertex => "Vertex".to_string(),
                    Stage::Pixel => "Pixel".to_string(),
                    Stage::Geometry => "Geometry".to_string(),
                    Stage::Hull => "Hull".to_string(),
                    Stage::Domain => "Domain".to_string(),
                    Stage::Compute => "Compute".to_string(),
                    Stage::Other => "Other".to_string(),
                },
                tail: *tail,
                container: *container,
            })
            .collect();

        Text {
            context_count: self.context_count,
            // The dependency is the engine's one library, so it is only written
            // when the file carries something else.
            dependency_count: (self.dependency_count != 0 && self.dependency_count != 1)
                .then_some(self.dependency_count),
            contexts: self.contexts.clone().into(),
            conditions: self.conditions.clone().into(),
            dependencies: {
                let default_dependency = Dependency::of().write();
                (!self.dependencies.is_empty()
                    && self.dependencies[..] != default_dependency[..])
                    .then(|| self.dependencies.clone().into())
            },
            preamble,
            group_data: (self.group_template.is_none() && !self.group_data.is_empty())
                .then(|| self.group_data.clone().into()),
            group_template,
            containers: containers.iter().map(|bytes| Hex((*bytes).clone())).collect(),
            tails,
            programs,
        }
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

    /// Assembles a complete shader section from the engine data and our
    /// programs. `material` is the generating material's resource path: the
    /// section's `material_hash` word is murmur32 of it, the section's identity
    /// (measured: the UI base's section is murmur32 of its own material path,
    /// `content/ui/materials/backgrounds/splash_screen_partner_logos`; the
    /// shipped data file is that material's, named by murmur64 of its *file*
    /// path, which the resource-path dictionaries do not key on). Verified in
    /// game with snoopy-mod's own path.
    pub fn generate(
        &self,
        containers: &HashMap<Stage, Vec<u8>>,
        material: &str,
    ) -> Result<Vec<u8>> {
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

        // The section's identity: murmur32 of the base material's resource path.
        let material_hash = u32::from(murmur::Murmur32::hash(material.as_bytes()));

        let header = [
            shader::VERSION,
            material_hash,
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

    /// Whether `text` parses as an engine data file. The tooling uses it to tell
    /// engine data from material data files.
    pub fn looks_like_text(text: &str) -> bool {
        serde_sjson::from_str::<Text>(text).is_ok()
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
            material_hash: 0,
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
        assert!(text.contains("lists = \"01020304\""), "{text}");
        assert!(text.contains("lists = \"05060708\""), "{text}");
        assert!(text.contains("stage = \"Vertex\""), "{text}");
        assert!(text.contains("stage = \"Pixel\""), "{text}");
        assert_eq!(text.matches("stage = \"Vertex\"").count(), 2);

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
        assert!(text.contains("container = 0"), "{text}");
        assert!(text.contains("container = 1"), "{text}");
        assert!(text.contains("\"44584243"), "{text}");

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

        let section = parsed
            .generate(&HashMap::new(), "materials/test/base")
            .unwrap();
        // The section's identity word is murmur32 of the material path it was
        // generated for.
        assert_eq!(
            u32::from_le_bytes(section[4..8].try_into().unwrap()),
            u32::from(murmur::Murmur32::hash(b"materials/test/base"))
        );
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
            let spec = block_text(&block, &preamble);
            let parsed = block_bytes(&spec, &preamble).unwrap();
            assert_eq!(parsed, block, "spec {spec:?}");
        }
    }

    #[test]
    fn a_preamble_that_starts_with_the_base_stores_only_its_remainder() {
        let mut engine_data = empty_engine_data();
        let mut preamble: Vec<u8> = (0..16u8).collect();
        preamble.extend_from_slice(&config_base().unwrap());
        preamble.extend_from_slice(&[0xAB; 40]);
        engine_data.device_preamble = preamble.clone();

        let text = engine_data.to_text();
        assert!(text.contains("head = \""), "{text}");
        assert!(text.contains("rest = \""), "{text}");
        assert!(!text.contains("bytes = \""), "{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        assert_eq!(parsed.device_preamble, preamble);
    }

    #[test]
    fn a_rewrite_line_is_refused() {
        // The engine data used to carry DTMT-only `variable`/`clone` lines; they
        // are gone, and a file that still has one must fail rather than silently
        // ignore it.
        let err = EngineData::from_text("variable dev_wireframe_color mod_tint 224 16\n")
            .expect_err("a rewrite line is not engine data");
        assert!(
            err.to_string().contains("Failed to parse the engine data text"),
            "{err}"
        );
    }
}