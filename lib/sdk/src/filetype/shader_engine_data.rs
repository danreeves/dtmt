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

use std::collections::{BTreeMap, HashMap};
use std::fs;

use color_eyre::eyre::{Context, Result, bail};
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::shader::{self, Stage};
use super::shader_node::StageResources;
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

/// murmur32 of `c_per_object`: the engine's per-material constant buffer, whose
/// size the group data's material table gives.
const C_PER_OBJECT: u32 = 0xB563_9618;

/// murmur32 of `global_viewport`: the engine's viewport constant buffer, whose
/// size the engine table gives.
const GLOBAL_VIEWPORT: u32 = 0x516D_5CCD;

/// murmur32 of `static_minlod_sampler`: the engine's static sampler, which a
/// stage takes when it declares a non-array sampler at set 31.
const STATIC_MINLOD_SAMPLER: u32 = 0x4B42_C5E6;

/// murmur32 of `global_samplers`: the engine's bindless sampler array, which a
/// stage takes when it declares a sampler array.
const GLOBAL_SAMPLERS: u32 = 0xDA56_0F03;

/// murmur32 of `global_texture2D`: the engine's bindless texture array.
const GLOBAL_TEXTURE2D: u32 = 0x3AFC_636C;

/// murmur32 of `global_feedback_buffers`: the engine's feedback UAV.
const GLOBAL_FEEDBACK_BUFFERS: u32 = 0x41B1_CFF8;

/// The group's descriptor list (`name -> index`). Every group must agree,
/// because the program-to-group mapping is not decoded.
fn descriptors(template: &GroupTemplate) -> Result<BTreeMap<u32, u32>> {
    let mut agreed: Option<BTreeMap<u32, u32>> = None;
    for parts in &template.groups {
        // The head is the query id, the group's two words, its descriptor
        // list, then the first table's header: skip the query and the header
        // word, then the count opens the list.
        let head = parts
            .head
            .get(8..)
            .ok_or_else(|| color_eyre::eyre::eyre!("a group head is too short"))?;
        let count = u32::from_le_bytes(
            head[..4]
                .try_into()
                .map_err(|_| color_eyre::eyre::eyre!("a group head is too short"))?,
        ) as usize;
        let bytes = head
            .get(4..4 + count * 16)
            .ok_or_else(|| color_eyre::eyre::eyre!("a group's descriptor list is short"))?;
        let mut list = BTreeMap::new();
        for index in 0..count {
            let name = u32::from_le_bytes(bytes[index * 16..index * 16 + 4].try_into().unwrap());
            list.insert(name, index as u32);
        }
        match &agreed {
            None => agreed = Some(list),
            Some(previous) if *previous != list => {
                bail!("the groups disagree on their descriptor lists")
            }
            Some(_) => {}
        }
    }
    agreed.ok_or_else(|| color_eyre::eyre::eyre!("the template has no groups"))
}

/// The lists (3, 5, 6 and 8) a stage's tail should hold, from its own
/// reflection and the engine's conventions:
///
/// - a texture array becomes the engine's `global_texture2D` record and an
///   unbounded sampler array its `global_samplers` one;
/// - a stage that samples gets the engine's `static_minlod_sampler` record and,
///   as the UI base shows, its `global_feedback_buffers` one;
/// - any other binding becomes its own record, named by its HLSL name.
///
/// The 7-word records carry the resource's index in the group's descriptor
/// list, so the descriptor table comes in with them.
fn set_derived_lists(
    lists: &mut shader::TailLists,
    resources: &StageResources,
    descriptors: &BTreeMap<u32, u32>,
) -> Result<()> {
    let index = |name: u32| -> Result<u32> {
        descriptors.get(&name).copied().ok_or_else(|| {
            color_eyre::eyre::eyre!("{name:08X} is not in the group's descriptor list")
        })
    };

    let mut textures = Vec::new();
    for decl in &resources.textures {
        // The bounded-texture record's shape is not measured yet; fail rather
        // than write a guess.
        bail!(
            "the bounded texture binding '{}' has no measured record shape yet",
            decl.name
        );
    }
    for decl in &resources.texture_arrays {
        textures.push(vec![
            GLOBAL_TEXTURE2D,
            index(GLOBAL_TEXTURE2D)?,
            decl.register,
            0xFFFF_FFFF,
            decl.space,
            0xFFFF_FFFF,
            0,
        ]);
    }

    let mut buffers = Vec::new();
    if resources.samples() {
        buffers.push(vec![
            GLOBAL_FEEDBACK_BUFFERS,
            index(GLOBAL_FEEDBACK_BUFFERS)?,
            0,
            0xFFFF_FFFF,
            31,
            0xFFFF_FFFF,
            0,
        ]);
    }

    let mut samplers = Vec::new();
    if resources.samples() {
        samplers.push(vec![STATIC_MINLOD_SAMPLER, 0, 1, 31]);
    }
    for decl in &resources.samplers {
        samplers.push(vec![
            u32::from(murmur::Murmur32::hash(decl.name.as_bytes())),
            decl.register,
            1,
            decl.space,
        ]);
    }

    let sampler_arrays = resources
        .sampler_arrays
        .iter()
        .map(|_| vec![GLOBAL_SAMPLERS, 0, 0])
        .collect();

    lists.lists[3] = textures;
    lists.lists[5] = buffers;
    lists.lists[6] = samplers;
    lists.lists[8] = sampler_arrays;
    Ok(())
}

/// A record table's size: the largest `offset + size`, rounded up to the
/// 16-byte granularity the tail entries record.
fn table_size(records: &[Record]) -> u32 {
    let end = records
        .iter()
        .map(|record| record.offset + record.size)
        .max()
        .unwrap_or(0);
    end.div_ceil(16) * 16
}

/// The query ids a contexts region carries, in order: each context is
/// `{u32 name, u32 word, u32 count}` followed by `count` `{u32 query, u32
/// conditions}` records.
fn context_queries(contexts: &[u8]) -> Result<Vec<u32>> {
    let mut queries = Vec::new();
    let mut at = 0usize;
    while at < contexts.len() {
        let head = contexts
            .get(at..at + 12)
            .ok_or_else(|| color_eyre::eyre::eyre!("a context record is short"))?;
        let count = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
        let records = contexts
            .get(at + 12..at + 12 + count * 8)
            .ok_or_else(|| color_eyre::eyre::eyre!("a context's queries are short"))?;
        for record in records.chunks_exact(8) {
            queries.push(u32::from_le_bytes(record[..4].try_into().unwrap()));
        }
        at += 12 + count * 8;
    }
    Ok(queries)
}

/// The constant-buffer list a split tail should hold, from the container's own
/// reflection: the names in register order, with the sizes from the group data's
/// material table (`c_per_object`) and the engine table (`global_viewport`), the
/// indexes from the group's descriptor list, and the constant 1 and 0 words.
///
/// Every group must agree on a size, because the program-to-group mapping is not
/// decoded; a cbuffer with no size source is an error rather than a guess.
fn derived_cbuffers(
    resources: &StageResources,
    template: &GroupTemplate,
    material_tables: &[Vec<Record>],
    descriptors: &BTreeMap<u32, u32>,
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(4 + resources.cbuffers.len() * 24);
    out.extend_from_slice(&(resources.cbuffers.len() as u32).to_le_bytes());
    for (register, name) in resources.cbuffers.iter().enumerate() {
        let hash = u32::from(murmur::Murmur32::hash(name.as_bytes()));
        let index = descriptors.get(&hash).copied().ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "the cbuffer {hash:08X} ({name}) is not a descriptor of the shader"
            )
        })?;
        let size = match hash {
            C_PER_OBJECT => {
                let mut sizes = material_tables.iter().map(|table| table_size(table));
                let first = sizes
                    .next()
                    .ok_or_else(|| color_eyre::eyre::eyre!("the file has no material table"))?;
                if sizes.any(|size| size != first) {
                    bail!(
                        "the groups disagree about c_per_object's size; the program-to-group \
                         mapping is not decoded"
                    );
                }
                first
            }
            GLOBAL_VIEWPORT => table_size(&template.engine),
            other => bail!("the cbuffer {other:08X} ({name}) has no size source"),
        };
        for word in [hash, index, size, register as u32, 1, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
    }
    Ok(out)
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
    /// The permutations to derive the contexts' ids from, instead of carrying
    /// the `contexts` blob itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    permutations: Option<Vec<PermutationPlanText>>,
    /// The concrete ids, carried by a section rebuilt from a shipped blob;
    /// empty when the ids come from `permutations`.
    #[serde(default, skip_serializing_if = "Hex::is_empty")]
    contexts: Hex,
    /// Empty blobs are not written.
    #[serde(default, skip_serializing_if = "Hex::is_empty")]
    conditions: Hex,
    /// Only written when it is not the engine's one dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependencies: Option<Hex>,
    /// The device preamble. Absent means the engine's block template
    /// ([`crate::filetype::shader_engine_block`]) - the one engine constant of
    /// the device data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preamble: Option<PreambleText>,
    /// The fallback when the group data has no template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_data: Option<Hex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_template: Option<GroupTemplateText>,
    /// Distinct compiled containers, referenced by `programs[].container`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    containers: Vec<Hex>,
    /// The distinct bytes a block puts in front of the preamble body, which the
    /// blocks' `head` indexes name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    block_heads: Vec<Hex>,
    /// Distinct program tails, referenced by `programs[].tail`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tails: Vec<TailText>,
    /// The program list. Absent means the build derives it from the permutation
    /// plan (one `(Vertex, Pixel)` pair per permutation slot).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    /// A repeating group list: when `groups` is empty and this is set, there is
    /// one group per query (in query order) and group `i` uses tail `i / repeat`
    /// with every other part at its zero index. The UI base writes 36 groups
    /// that are six repeats of its six tails, so the list is derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeat: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<GroupText>,
}

/// One group of the template: its query id (absent when the contexts carry it -
/// they do, one to one and in the same order) and the parts it uses.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<Hex>,
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

/// One distinct program tail: the block that closes it, and the lists only when
/// they have to be carried.
///
/// Everything else derives from the compiled container's own reflection at
/// build time: the constant-buffer list (names in register order, with the
/// sizes from the group data's material table and the engine table, and the
/// indexes from the group's descriptor list) and the resource lists (the
/// texture/sampler arrays, the static sampler and the feedback buffers). A tail
/// this reader cannot split is carried whole in `lists`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lists: Option<ListsText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    block: Option<BlockText>,
}

/// A tail's lists: by role, or the raw bytes of the whole region for a tail
/// this reader cannot split.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum ListsText {
    Roles(RoleLists),
    Raw(Hex),
}

/// The tail's lists by role, one record per hex string (the record's words,
/// little-endian).
///
/// The lists that are always empty in every sample (0, 1 and 4) and the input
/// list (7) are not stored at all: the reader writes their empty counts and the
/// build rebuilds the inputs from the compiled container's signature.
/// One context's permutation plan in its binary form for `to_text`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermutationPlanText {
    /// The context's name, hashed into the header.
    pub context: String,
    /// One key-defining token list per query, in the section's order.
    pub queries: Vec<Vec<String>>,
}

impl From<PermutationPlanText> for PermutationPlan {
    fn from(text: PermutationPlanText) -> Self {
        Self {
            context: text.context,
            queries: text.queries,
        }
    }
}

impl From<&PermutationPlan> for PermutationPlanText {
    fn from(plan: &PermutationPlan) -> Self {
        Self {
            context: plan.context.clone(),
            queries: plan.queries.clone(),
        }
    }
}

/// The binary form of a permutation plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermutationPlan {
    pub context: String,
    pub queries: Vec<Vec<String>>,
}

/// Builds the contexts blob: each plan becomes a context whose query ids are
/// the permutation rule's values over that context's key token sets.
fn permutations_contexts(
    material: &str,
    plans: &[PermutationPlan],
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for plan in plans {
        let context_hash =
            u32::from(murmur::Murmur32::hash(plan.context.as_bytes()));
        out.extend_from_slice(&context_hash.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(plan.queries.len() as u32).to_le_bytes());
        for tokens in &plan.queries {
            // The key carries the material path, the define tokens and the
            // environment tail — not the context name (the context lives in
            // the header hash; the verified in-game key had no context token).
            let key = crate::filetype::permutation::key(
                material,
                "",
                &tokens.iter().map(String::as_str).collect::<Vec<_>>(),
                "WIN32",
                "D3D12",
            );
            let id = crate::filetype::permutation::id(&key);
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        }
    }
    Ok(out)
}

/// The `c_per_object` cbuffer hash the packed copies are keyed by.
const PACKED_CBUFFER: u32 = 0xB563_9618;

/// The engine's group head after the query: the header word, the descriptor
/// list and the two trailing words. The descriptors are the section's own
/// resources under the engine's names and flags; `X` is the per-draw byte offset
/// (24 per constant buffer, 8 per other) and `Y` the engine's packed usage
/// counts (measured: `{0, 0, 5, 10}` for a texture and a feedback buffer; only a
/// zero `Y` breaks the render).
fn derived_head(resources: &HashMap<Stage, StageResources>) -> Vec<u8> {
    let samples = resources.values().any(StageResources::samples);
    let mut descriptors: Vec<(u32, u32, u32)> = vec![(C_PER_OBJECT, 0x0000, 0), (GLOBAL_VIEWPORT, 0x0101, 0)];
    if samples {
        descriptors.push((GLOBAL_TEXTURE2D, 0x0103, 5));
        descriptors.push((GLOBAL_FEEDBACK_BUFFERS, 0x0105, 10));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0x130u32.to_le_bytes());
    out.extend_from_slice(&(descriptors.len() as u32).to_le_bytes());
    let mut x = 0u32;
    for (name, flags, y) in &descriptors {
        for word in [*name, *flags, x, *y] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        x += if flags & 0xFF <= 1 { 24 } else { 8 };
    }
    out.extend_from_slice(&0x02u32.to_le_bytes());
    // The material table's count, which `GroupData::build` rewrites.
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// The engine's `between` bytes: two constants then the engine table's count
/// word, which [`GroupData::build`] rewrites.
fn derived_between() -> Vec<u8> {    let mut out = vec![0xF0, 0, 0, 0, 0x40, 0, 0, 0];
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// The engine's packed copies for a group, from its material table: one
/// three-record template per texture slot (a kind 5 row), the slot's channel
/// name hashed into every copy and `c_per_object` as the cbuffer. Measured on
/// the UI base: the three `texture_map` rows re-emitted with their own offsets
/// and kinds.
fn derived_mid(records: &[Record]) -> Vec<u8> {
    let slots = records.iter().filter(|record| record.kind == 5).count();
    let mut out = Vec::new();
    out.extend_from_slice(&0x700u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((slots * 3) as u32).to_le_bytes());
    for record in records.iter().filter(|record| record.kind == 5) {
        for (a, b, offset, kind) in [(5u32, 0u32, 0u32, 5u32), (0, 0, 4, 1), (8, 0, 16, 1)] {
            for word in [record.hash, a, b, offset, kind, PACKED_CBUFFER, 0] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
    }
    out
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleLists {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    engine: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    textures: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    buffers: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    samplers: Vec<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sampler_arrays: Vec<Hex>,
}

/// Reads a list record of `size` words.
fn words_from_hex(hex: &Hex, size: usize) -> Result<Vec<u32>> {
    if hex.0.len() != size * 4 {
        bail!("a list record needs {} bytes, got {}", size * 4, hex.0.len());
    }
    Ok(hex
        .0
        .chunks(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

/// Writes a tail's lists back out: the nine counted lists, with 0, 1, 4 and the
/// inputs empty.
fn lists_bytes(text: &RoleLists) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for index in 0..shader::TailLists::SIZES.len() {
        let (size, records): (usize, &[Hex]) = match index {
            2 => (7, &text.engine),
            3 => (7, &text.textures),
            5 => (7, &text.buffers),
            6 => (4, &text.samplers),
            8 => (3, &text.sampler_arrays),
            _ => (0, &[]),
        };
        out.extend_from_slice(&(records.len() as u32).to_le_bytes());
        for record in records {
            for word in words_from_hex(record, size)? {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
    }
    Ok(out)
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
    /// The index of the bytes before the body in the file's shared head pool;
    /// absent when the block *is* the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head: Option<usize>,
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
fn block_text(block: &[u8], preamble: &[u8], heads: &mut Vec<Hex>) -> BlockText {
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
    // The head goes into the file's shared pool: the tails repeat a handful of
    // them.
    let head = (header != 0).then(|| match heads.iter().position(|other| other.0 == block[..header])
    {
        Some(index) => index,
        None => {
            heads.push(block[..header].to_vec().into());
            heads.len() - 1
        }
    });
    BlockText::Body(BlockDiff { head, patches })
}

/// Reads a block written by [`block_text`].
fn block_bytes(spec: &BlockText, preamble: &[u8], heads: &[Hex]) -> Result<Vec<u8>> {
    match spec {
        BlockText::Hex(hex) => Ok(hex.0.clone()),
        BlockText::Body(diff) => {
            let body = preamble
                .get(12..)
                .ok_or_else(|| color_eyre::eyre::eyre!("the preamble is too short to hold a body"))?;
            let mut block = match diff.head {
                Some(index) => heads
                    .get(index)
                    .map(|head| head.0.clone())
                    .ok_or_else(|| color_eyre::eyre::eyre!("a block head is out of range"))?,
                None => Vec::new(),
            };
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
    /// The permutation plans the file carries instead of concrete ids: one per
    /// context, each listing the queries the engine hashes its keys into. When
    /// present, `generate` derives the ids rather than carrying them.
    pub permutations: Option<Vec<PermutationPlan>>,
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

/// The UI base family's pass table: the mask byte each pixel program writes into
/// its tail block, in program order (48 pixel programs).
///
/// Read from a shipped UI-base section (`family_probe`), and the family's own
/// data rather than anything our declaration describes: the engine's UI-base
/// shader declares these 48 passes. The values are channel sets - `01 02 04 08`
/// the single channel bits, `0F` all four, `07` the body's default (a tail with
/// no patch). A different engine family has its own table (and its own tail
/// shape - see `docs/Shader RE TODO.md`).
const UI_BASE_PASS_MASKS: [u8; 48] = [
    0x07, 0x01, 0x01, 0x02, 0x02, 0x04, 0x04, 0x08, 0x08, 0x0F, 0x0F, 0x07, 0x07, 0x01, 0x01,
    0x02, 0x02, 0x04, 0x04, 0x08, 0x08, 0x0F, 0x0F, 0x07, 0x01, 0x02, 0x04, 0x08, 0x0F, 0x07,
    0x01, 0x02, 0x04, 0x08, 0x0F, 0x07, 0x01, 0x02, 0x04, 0x08, 0x0F, 0x07, 0x01, 0x02, 0x04,
    0x08, 0x0F, 0x07,
];

/// The UI base family's head sizes, one per pixel program, in slot order: the
/// 32-byte head is `02`+zeros, the 36-byte one adds a `02 00 00 00` word at the
/// seam. Measured from a shipped section (`program_diff`): slots 0-22 alternate
/// 32/36, slots 23-46 are 36, the last is 32.
const UI_BASE_PASS_HEADS: [u8; 48] = [
    32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32, 36, 32,
    36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36, 36,
    36, 32,
];

/// The UI base family's 36-byte head seam word's low byte, per pixel slot (0
/// where the head is 32 bytes). Measured from a shipped section.
const UI_BASE_PASS_SEAMS: [u8; 48] = [
    0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x02,
    0x00, 0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00,
];

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
            permutations: None,
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
            permutations: text
                .permutations
                .map(|plans| plans.into_iter().map(Into::into).collect()),
        };

        // The preamble: the engine's config base is a toolchain constant, so a
        // file writes only what lies around it.
        // The preamble: the engine's block template when the file carries none,
        // or the file's own head/rest around the toolchain's config base.
        engine_data.device_preamble = match &text.preamble {
            None => crate::filetype::shader_engine_block::engine_block(),
            Some(preamble) => match (&preamble.head, &preamble.rest, &preamble.bytes) {
                (Some(head), Some(rest), None) => {
                    let mut block = head.0.clone();
                    block.extend_from_slice(&config_base()?);
                    block.extend_from_slice(&rest.0);
                    block
                }
                (None, None, Some(bytes)) => bytes.0.clone(),
                _ => bail!("a preamble is either its `head` and `rest` or its `bytes`"),
            },
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
            let queries = context_queries(&engine_data.contexts)?;
            // A repeating list stands for `repeat` groups per tail: group `i`
            // uses tail `i / repeat`, everything else at its zero index. The
            // group count is the family's (`repeat x tails`), not the queries' -
            // a from-scratch file declares one query but ships the family's
            // groups, so the count comes from the tails, and each group's head
            // leaves its query id as a zero placeholder for `generate` to fill
            // from the contexts it derives.
            let expanded: Vec<GroupText> = match (template.groups.is_empty(), template.repeat) {
                (true, Some(repeat)) if repeat > 0 && !template.tails.is_empty() => {
                    let count = repeat * template.tails.len();
                    (0..count)
                        .map(|index| GroupText {
                            query: None,
                            head: 0,
                            between: 0,
                            mid: 0,
                            tail: index / repeat,
                            material: 0,
                            material_first: true,
                        })
                        .collect()
                }
                _ => template.groups.clone(),
            };
            for (index, group) in expanded.iter().enumerate() {
                // A group starts with its query id: the file's own, or the
                // contexts' by position (they are one to one, same order), so
                // the head stores only what follows it. A file that carries
                // neither yet (a from-scratch file whose contexts come from the
                // declaration at build time) leaves a zero placeholder that
                // `generate` fills from the contexts it derives.
                let query = match &group.query {
                    Some(query) => query.0.clone(),
                    None => queries
                        .get(index)
                        .map(|query| query.to_le_bytes().to_vec())
                        .unwrap_or_else(|| 0u32.to_le_bytes().to_vec()),
                };
                let mut head = query;
                if template.heads.is_empty() {
                    // Derived in `generate`, which has the stage resources.
                } else {
                    head.extend_from_slice(&part(&template.heads, group.head, "head")?);
                }
                groups.push(GroupParts {
                    head,
                    material_first: group.material_first,
                    // An empty pool means the part is derived: the engine's
                    // constants for `between`, and the material table's texture
                    // slots for `mid` (filled in `generate`).
                    between: if template.betweens.is_empty() {
                        Vec::new()
                    } else {
                        part(&template.betweens, group.between, "between")?
                    },
                    mid: if template.mids.is_empty() {
                        Vec::new()
                    } else {
                        part(&template.mids, group.mid, "mid")?
                    },
                    tail: part(&template.tails, group.tail, "tail")?,
                });
                // An empty `materials` list means the table is derived - the
                // declaration names it - so the caller fills `material_tables`.
                if !template.materials.is_empty() {
                    tables.push(records_from_bytes(&part(
                        &template.materials,
                        group.material,
                        "material",
                    )?)?);
                }            }
            engine_data.group_template = Some(GroupTemplate {
                prefix: template.prefix.into_bytes(),
                groups,
                engine,
            });
            engine_data.material_tables = tables;
        }

        // The tails: a split tail's constant-buffer list and resource lists are
        // derived at build time, so the reader writes an empty prefix and empty
        // lists for it to fill; a carried tail is the whole tail.
        for tail in text.tails {
            let mut bytes = match &tail.lists {
                Some(ListsText::Roles(roles)) => lists_bytes(roles)?,
                Some(ListsText::Raw(raw)) => raw.0.clone(),
                None => {
                    let mut bytes = 0u32.to_le_bytes().to_vec();
                    bytes.extend_from_slice(&lists_bytes(&RoleLists::default())?);
                    bytes
                }
            };
            if let Some(block) = &tail.block {
                bytes.extend_from_slice(&block_bytes(
                    block,
                    &engine_data.device_preamble,
                    &text.block_heads,
                )?);
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
            let context_queries = context_queries(&self.contexts).unwrap_or_default();
            for (index, parts) in template.groups.iter().enumerate() {
                // The query id is the contexts' own, one per group and in the
                // same order, so it is only written when they do not carry it.
                let head_query = parts.head.get(..4).unwrap_or_default();
                // The query id is omitted when the contexts carry it (one per
                // group, in order), or when they carry none at all - a
                // from-scratch file whose contexts come from the declaration, so
                // the head's first word is a placeholder `generate` fills.
                let derived = match context_queries.get(index) {
                    Some(query) => head_query == query.to_le_bytes().as_slice(),
                    None => context_queries.is_empty(),
                };
                groups.push(GroupText {
                    query: (!derived).then(|| Hex(head_query.to_vec())),
                    head: dedup_index(&mut heads, parts.head.get(4..).unwrap_or_default().to_vec()),
                    between: dedup_index(&mut betweens, parts.between.clone()),
                    mid: dedup_index(&mut mids, parts.mid.clone()),
                    tail: dedup_index(&mut tails, parts.tail.clone()),
                    material: material_indexes[index],
                    material_first: parts.material_first,
                });
            }

            // A group list that is a plain repetition of the tails (group `i`
            // uses tail `i / repeat`, every other part at its zero index - the UI
            // base's 36 = six repeats of six) is written as the repeat count
            // instead of the list, so it is derived from the queries at read
            // time.
            let plain = groups.iter().all(|group| {
                group.query.is_none()
                    && group.head == 0
                    && group.between == 0
                    && group.mid == 0
                    && group.material == 0
                    && group.material_first
            });
            let repeat = (plain && !groups.is_empty() && !tails.is_empty())
                .then(|| groups.len() / tails.len())
                .filter(|repeat| {
                    *repeat > 0
                        && groups
                            .iter()
                            .enumerate()
                            .all(|(index, group)| group.tail == index / repeat)
                });
            let (repeat, groups) = match repeat {
                Some(repeat) => (Some(repeat), Vec::new()),
                None => (None, groups),
            };

            GroupTemplateText {
                prefix: template.prefix.clone().into(),
                engine,
                materials,
                heads,
                betweens,
                mids,
                tails,
                repeat,
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
        let mut block_heads: Vec<Hex> = Vec::new();
        let tails: Vec<TailText> = tails
            .iter()
            .map(|tail| {
                // A tail is split - and stores only its block - when its
                // constant buffers are all the engine's own and its lists
                // parse: the build derives both from the container's own
                // reflection. Anything else is carried whole.
                let parsed = shader::Tail::parse(tail);
                let lists = parsed
                    .as_ref()
                    .and_then(|parsed| shader::TailLists::parse(&parsed.rest));
                let split = match (&parsed, &lists) {
                    (Some(parsed), Some(_)) => {
                        self.group_template.is_some()
                            && parsed.cbuffers.iter().all(|entry| {
                                matches!(entry.name_hash(), C_PER_OBJECT | GLOBAL_VIEWPORT)
                            })
                    }
                    _ => false,
                };
                match (split, lists) {
                    (true, Some(lists)) => TailText {
                        lists: None,
                        block: (!lists.block.is_empty()).then(|| {
                            block_text(&lists.block, &self.device_preamble, &mut block_heads)
                        }),
                    },
                    _ => TailText {
                        lists: Some(ListsText::Raw(tail.to_vec().into())),
                        block: None,
                    },
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
            // The ids are written as they are carried; plans (when present)
            // would let a later build re-derive them, but the round trip keeps
            // the concrete blob.
            permutations: self
                .permutations
                .as_ref()
                .map(|plans| plans.iter().map(PermutationPlanText::from).collect()),
            contexts: self.contexts.clone().into(),
            conditions: self.conditions.clone().into(),
            dependencies: {
                let default_dependency = Dependency::of().write();
                (!self.dependencies.is_empty()
                    && self.dependencies[..] != default_dependency[..])
                    .then(|| self.dependencies.clone().into())
            },
            // The engine block template is not written: an absent preamble
            // means exactly it.
            preamble: (self.device_preamble
                != crate::filetype::shader_engine_block::engine_block())
            .then_some(preamble),
            group_data: (self.group_template.is_none() && !self.group_data.is_empty())
                .then(|| self.group_data.clone().into()),
            group_template,
            containers: containers.iter().map(|bytes| Hex((*bytes).clone())).collect(),
            block_heads,
            tails,
            programs,
        }
    }

    /// Builds the device data: the engine data's preamble, then one framed program
    /// record per engine data program, using `containers[stage]` when the caller
    /// compiled one and the carried container otherwise, plus that program's tail.
    ///
    /// `resources`, when it has an entry for a stage, replaces that stage's
    /// sampler lists (6 and 8) with the ones its HLSL declares.
    /// The group template with its derived parts filled in: the head (the
    /// descriptor list) when the file carries only the query, `between`, and
    /// `mid`. A part the file carried wins.
    /// The program list and its tails, when the file does not carry them and a
    /// permutation plan does: one `(Vertex, Pixel)` pair per permutation slot,
    /// the vertex program the minimal block and the pixel program a head plus
    /// the permutation's channel mask.
    ///
    /// Measured on the UI base (96 programs): every vertex program is the
    /// 12-byte zero block; the pixel programs walk a small tail set whose blocks
    /// are `head + preamble body + one patch at body offset 477`, the patch
    /// value a channel bitmask (`1, 2, 4, 8, 15`), and the pattern repeats every
    /// six permutations as the context alternates.
    pub fn derived_programs(&self, _plans: &[PermutationPlan]) -> Option<Vec<(Stage, Vec<u8>)>> {
        let body = self.device_preamble.get(12..)?;
        // A split tail's base: an empty prefix (so the build fills its lists
        // from the container's reflection) followed by empty lists.
        let split = shader::TailLists {
            lists: vec![Vec::new(); shader::TailLists::SIZES.len()],
            block: Vec::new(),
        }
        .bytes();
        let mut split_tail = 0u32.to_le_bytes().to_vec();
        split_tail.extend_from_slice(&split);
        // The vertex program's whole tail is the split base plus the minimal
        // 12-byte block: `4 + 36 + 12 = 52` bytes.
        let mut vertex = split_tail.clone();
        vertex.extend_from_slice(&[0u8; 12]);

        // The program list is the **family's**, not one pair per query: the UI
        // base's section declares a single context query yet carries 48 program
        // pairs. So the family table (heads, seams, masks) is walked whole, one
        // pair per entry.
        let mut programs = Vec::new();
        let passes = UI_BASE_PASS_HEADS.len();
        for pass in 0..passes {
            let head = UI_BASE_PASS_HEADS[pass] as usize;
            let seam = UI_BASE_PASS_SEAMS[pass];
            let mask = UI_BASE_PASS_MASKS[pass];
            let last = pass == passes - 1;
            let mut pixel = split_tail.clone();
            let mut block = vec![0u8; head];
            block[0] = 0x02;
            if head == 36 {
                block[32..36].copy_from_slice(&[seam, 0, 0, 0]);
            }
            if last {
                // The family's last block is a head alone: `02` + 35 zeros, no
                // body.
                let mut block = vec![0u8; 36];
                block[0] = 0x02;
                pixel.extend_from_slice(&block);
            } else {
                block.extend_from_slice(body);
                // The body's default is `07`; only a pass that overrides it
                // patches the byte.
                if mask != 0x07
                    && let Some(byte) = block.get_mut(head + 477)
                {
                    *byte = mask;
                }
                pixel.extend_from_slice(&block);
            }
            programs.push((Stage::Vertex, vertex.clone()));
            programs.push((Stage::Pixel, pixel));
        }
        Some(programs)
    }

    fn derived_template(
        &self,
        resources: &HashMap<Stage, StageResources>,
    ) -> Option<GroupTemplate> {
        let mut template = self.group_template.as_ref()?.clone();
        for (index, group) in template.groups.iter_mut().enumerate() {
            // A head that is just the query waits for its derived rest.
            if group.head.len() == 4 {
                group.head.extend_from_slice(&derived_head(resources));
            }
            if group.between.is_empty() {
                group.between = derived_between();
            }
            if group.mid.is_empty() {
                let records = self
                    .material_tables
                    .get(index)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                group.mid = derived_mid(records);
            }
        }
        Some(template)
    }

    pub fn build_device(
        &self,
        containers: &HashMap<Stage, Vec<u8>>,
        resources: &HashMap<Stage, StageResources>,
        template: Option<&GroupTemplate>,
    ) -> Result<Vec<u8>> {
        // The program list: the file's own, or - for a from-scratch file with a
        // permutation plan - the list the plan stands for (one (Vertex, Pixel)
        // pair per permutation slot, the vertex the minimal block and the pixel
        // the head, preamble body and channel mask).
        let derived;
        let programs: &[(Stage, Vec<u8>)] = match &self.permutations {
            Some(plans) if self.programs.is_empty() && !plans.is_empty() => {
                derived = self
                    .derived_programs(plans)
                    .ok_or_else(|| color_eyre::eyre::eyre!("the preamble has no body"))?;
                &derived
            }
            _ => &self.programs,
        };

        let mut device = self.device_preamble.clone();

        for (index, (stage, tail)) in programs.iter().enumerate() {
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

            // A split tail (the reader wrote it an empty prefix) derives its
            // constant-buffer list and its resource lists from the container's
            // own reflection; a carried tail is left alone.
            let derived = match shader::Tail::parse(tail) {
                Some(parsed) if parsed.cbuffers.is_empty() => {
                    let resources = resources.get(stage).ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "no reflection for program {index} ({stage:?}): its constant buffers \
                             and resource lists are derived from the compiled container"
                        )
                    })?;
                    let template = template.ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "program {index} ({stage:?}) needs a group template to index its \
                             records"
                        )
                    })?;
                    let descriptors = descriptors(template)?;
                    let mut lists = shader::TailLists::parse(&parsed.rest).ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "program {index} ({stage:?}): a split tail's lists do not parse"
                        )
                    })?;
                    set_derived_lists(&mut lists, resources, &descriptors)?;
                    let mut bytes =
                        derived_cbuffers(resources, template, &self.material_tables, &descriptors)?;
                    bytes.extend_from_slice(&lists.bytes());
                    Some(bytes)
                }
                _ => None,
            };
            let tail: &[u8] = derived.as_deref().unwrap_or(tail);

            // The input list describes the container's signature, so rebuild it
            // for whichever container this program gets: a mod shader with
            // different IO then still gets a tail that matches its programs.
            // A tail whose input list is *empty* is one whose inputs are
            // derived from the signature, so without one it must fail rather
            // than write a program with no inputs.
            let tail = match shader::Tail::parse(tail)
                .and_then(|parsed| parsed.with_inputs(container))
            {
                Some(rebuilt) => rebuilt.bytes(),
                None => {
                    let derived_inputs = shader::Tail::parse(tail)
                        .and_then(|parsed| shader::TailLists::parse(&parsed.rest))
                        .is_some_and(|lists| lists.list(7).is_empty());
                    if derived_inputs {
                        bail!(
                            "the container for program {index} ({stage:?}) has no signature to \
                             rebuild its inputs from"
                        );
                    }
                    tail.to_vec()
                }
            };

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
        resources: &HashMap<Stage, StageResources>,
    ) -> Result<Vec<u8>> {
        // The contexts: derived from the declaration when the file carries the
        // permutation plans, carried otherwise (a section rebuilt from a
        // shipped blob still carries its original ids).
        let (contexts, context_count) = match &self.permutations {
            Some(plans) if !plans.is_empty() => (permutations_contexts(material, plans)?, plans.iter().map(|p| p.queries.len() as u32).sum::<u32>()),
            _ => (self.contexts.clone(), self.context_count),
        };
        // The template first: the device's tails read its descriptor list, so
        // the derived head has to exist before the device is built.
        let mut template = self.derived_template(resources);

        // A group's query id comes from the contexts (the engine looks a group
        // up by it), so a from-scratch file - whose groups carry a zero
        // placeholder because the file had no contexts - takes it here from the
        // contexts the file derived. Setting a group node to the context query
        // is what the shipped sections do.
        if let Some(template) = template.as_mut() {
            let ids = context_queries(&contexts)?;
            for (index, group) in template.groups.iter_mut().enumerate() {
                let placeholder = group
                    .head
                    .get(..4)
                    .is_some_and(|word| word == [0u8; 4]);
                if placeholder
                    && let Some(id) = ids.get(index)
                    && let Some(word) = group.head.get_mut(..4)
                {
                    word.copy_from_slice(&id.to_le_bytes());
                }
            }
        }

        let device = self.build_device(containers, resources, template.as_ref())?;

        let group_data = match &template {
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
        let conditions_offset = contexts_offset + contexts.len();
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
            context_count,
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
        section.extend_from_slice(&contexts);
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
            permutations: None,
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
    fn permutation_plans_derive_the_verified_ids() {
        // The blob the engine accepted in game: one context, two word pairs
        // (id, conditions) whose first id was computed by the permutation rule.
        let plans = vec![PermutationPlan {
            context: "default".into(),
            queries: vec![vec!["SINGLE".into()]],
        }];
        let blob = permutations_contexts("materials/mods/snoopymod/ui_default_base", &plans)
            .expect("derive");

        // murmur32("default"), a zero word, one query, then the id and the
        // wildcard conditions marker.
        assert_eq!(blob.len(), 5 * 4);
        let word = |at: usize| u32::from_le_bytes(blob[at..at + 4].try_into().unwrap());
        assert_eq!(word(0), u32::from(murmur::Murmur32::hash(b"default")));
        assert_eq!(word(8), 1);
        let id = word(12);
        assert_eq!(word(16), 0xFFFFFFFF);

        // The exact id the engine accepted for
        // `material:SINGLE:PLATFORM_WIN32:RENDERER_D3D12` (6FA3FCCF).
        let verified_key =
            "materials/mods/snoopymod/ui_default_base:SINGLE:PLATFORM_WIN32:RENDERER_D3D12";
        assert_eq!(id, crate::filetype::permutation::id(verified_key));
        assert_eq!(id, 0x6FA3FCCF);
    }

    #[test]
    fn permutation_text_round_trip() {
        let text = r#"
context_count = 1
permutations = [
    {
        context = "default"
        queries = [ ["SINGLE"] ]
    }
]
contexts = ""
preamble = { bytes = "" }
programs = []
"#;
        let parsed = EngineData::from_text(text)
            .map_err(|error| panic!("text parse failed: {error:#}"))
            .unwrap();
        let plans = parsed.permutations.clone().expect("plans");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].context, "default");
        assert!(!plans[0].queries.is_empty(), "queries: {:?}", plans[0].queries);
        assert_eq!(plans[0].queries.len(), 1);
        assert_eq!(
            plans[0].queries[0]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["SINGLE"],
            "queries: {:?}", plans[0].queries
        );
        let text_out = parsed.to_text();
        let plans_round_tripped = EngineData::from_text(&text_out)
            .unwrap()
            .permutations
            .expect("plans survive the round trip");
        assert_eq!(plans_round_tripped, plans);
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

        let device = parsed
            .build_device(&HashMap::new(), &HashMap::new(), parsed.group_template.as_ref())
            .unwrap();
        assert_eq!(
            decode_all(&device),
            vec![container_a.clone(), container_b.clone(), container_a.clone()]
        );

        // A compiled override wins over the carried container.
        let mut overrides = HashMap::new();
        overrides.insert(Stage::Vertex, container_b.clone());
        let device = parsed
            .build_device(&overrides, &HashMap::new(), parsed.group_template.as_ref())
            .unwrap();
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
            .generate(&HashMap::new(), "materials/test/base", &HashMap::new())
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
    fn a_repeating_group_list_round_trips_as_a_repeat() {
        // The UI base's shape: 36 groups that are six repeats of six tails. The
        // text form writes the repeat count, and the reader expands it back to
        // one group per query, each with `tail = index / repeat`.
        use crate::filetype::group_data::{GroupParts, GroupTemplate, Record};

        let engine = vec![Record {
            kind: 2,
            flags: 0,
            hash: 0x6BC9_1D73,
            offset: 0,
            size: 12,
        }];
        // Six queries: the head is the query id alone, as the text writes it,
        // so the ids must be the contexts' own, derived the same way.
        let plans = vec![PermutationPlan {
            context: "default".into(),
            queries: (0..6)
                .map(|index| vec![format!("Q{index}")])
                .collect::<Vec<_>>(),
        }];
        let contexts = permutations_contexts("materials/test/base", &plans).expect("contexts");
        let query_ids = context_queries(&contexts).expect("queries");
        assert_eq!(query_ids.len(), 6);
        let query = |index: usize| {
            let mut head = query_ids[index].to_le_bytes().to_vec();
            head.extend_from_slice(&[0u8; 8]);
            head
        };
        let tail = |byte: u8| vec![byte; 12];
        let group = |index: usize, tail: Vec<u8>| GroupParts {
            head: query(index),
            material_first: true,
            between: vec![0u8; 12],
            mid: vec![0u8; 8],
            tail,
        };
        let template = GroupTemplate {
            prefix: 6u32.to_le_bytes().to_vec(),
            groups: vec![
                group(0, tail(0xA1)),
                group(1, tail(0xA1)),
                group(2, tail(0xA1)),
                group(3, tail(0xB2)),
                group(4, tail(0xB2)),
                group(5, tail(0xB2)),
            ],
            engine: engine.clone(),
        };

        let mut engine_data = empty_engine_data();
        engine_data.contexts = contexts;
        engine_data.group_template = Some(template);
        engine_data.material_tables = vec![Vec::new(); 6];

        let text = engine_data.to_text();
        assert!(text.contains("repeat = 3"), "the list is not collapsed:\n{text}");
        assert!(!text.contains("groups ="), "the list is still written:\n{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        let groups = &parsed.group_template.as_ref().expect("template").groups;
        assert_eq!(groups.len(), 6);
        // Tail index is `index / repeat`: 0,0,0,1,1,1.
        assert_eq!(groups[0].tail, groups[2].tail);
        assert_eq!(groups[3].tail, groups[5].tail);
        assert_ne!(groups[2].tail, groups[3].tail);
        // Each tail is the twelve-byte run it was.
        assert_eq!(groups[0].tail, vec![0xA1; 12]);
        assert_eq!(groups[5].tail, vec![0xB2; 12]);
    }

    #[test]
    fn a_cbuffer_tail_derives_from_the_reflection() {
        // The names, their sizes and their indexes, from a stage's reflection
        // and the group template.
        use crate::filetype::group_data::{GroupParts, GroupTemplate, Record};

        assert_eq!(
            C_PER_OBJECT,
            u32::from(murmur::Murmur32::hash(b"c_per_object"))
        );
        assert_eq!(
            GLOBAL_VIEWPORT,
            u32::from(murmur::Murmur32::hash(b"global_viewport"))
        );

        // A one-group template whose descriptors are the UI base's four.
        let mut head = vec![0u8; 4]; // the query id, filled by the test
        head.extend_from_slice(&[0u8; 4]); // the header word
        head.extend_from_slice(&4u32.to_le_bytes());
        for name in [
            C_PER_OBJECT,
            GLOBAL_VIEWPORT,
            GLOBAL_TEXTURE2D,
            GLOBAL_FEEDBACK_BUFFERS,
        ] {
            head.extend_from_slice(&name.to_le_bytes());
            head.extend_from_slice(&[0u8; 12]);
        }
        let template = GroupTemplate {
            prefix: 1u32.to_le_bytes().to_vec(),
            groups: vec![GroupParts {
                head,
                material_first: true,
                between: Vec::new(),
                mid: Vec::new(),
                tail: Vec::new(),
            }],
            // The engine table's extent gives global_viewport 1792 (1788
            // rounded up to 16).
            engine: vec![Record {
                kind: 2,
                flags: 0,
                hash: 0x6BC9_1D73,
                offset: 0,
                size: 1788,
            }],
        };
        // The material table's extent gives c_per_object 240.
        let material_table = vec![Record {
            kind: 3,
            flags: 0,
            hash: 0x795C_F4A7,
            offset: 224,
            size: 16,
        }];
        let resources = StageResources {
            cbuffers: vec!["global_viewport".to_string(), "c_per_object".to_string()],
            ..Default::default()
        };

        let descriptors = descriptors(&template).unwrap();
        let prefix =
            derived_cbuffers(&resources, &template, &[material_table], &descriptors).unwrap();
        assert_eq!(prefix.len(), 4 + 2 * 24);
        assert_eq!(u32::from_le_bytes(prefix[0..4].try_into().unwrap()), 2);
        let entry = |index: usize| -> [u32; 6] {
            let at = 4 + index * 24;
            std::array::from_fn(|word| {
                u32::from_le_bytes(prefix[at + word * 4..at + word * 4 + 4].try_into().unwrap())
            })
        };
        assert_eq!(entry(0), [GLOBAL_VIEWPORT, 1, 1792, 0, 1, 0]);
        assert_eq!(entry(1), [C_PER_OBJECT, 0, 240, 1, 1, 0]);
    }

    #[test]
    fn the_resource_lists_come_from_the_container_reflection() {
        // The engine names the derived lists use.
        assert_eq!(
            STATIC_MINLOD_SAMPLER,
            u32::from(murmur::Murmur32::hash(b"static_minlod_sampler"))
        );
        assert_eq!(
            GLOBAL_SAMPLERS,
            u32::from(murmur::Murmur32::hash(b"global_samplers"))
        );
        assert_eq!(
            GLOBAL_TEXTURE2D,
            u32::from(murmur::Murmur32::hash(b"global_texture2D"))
        );
        assert_eq!(
            GLOBAL_FEEDBACK_BUFFERS,
            u32::from(murmur::Murmur32::hash(b"global_feedback_buffers"))
        );

        // A pixel stage's reflection, as the UI base's container reports it.
        let bindings = vec![
            dxc::BoundResource {
                name: "global_viewport".into(),
                kind: 0,
                bind_point: 0,
                bind_count: 1,
                flags: 1,
                space: 0,
            },
            dxc::BoundResource {
                name: "c_per_object".into(),
                kind: 0,
                bind_point: 1,
                bind_count: 1,
                flags: 1,
                space: 0,
            },
            dxc::BoundResource {
                name: "g_material_samplers".into(),
                kind: 3,
                bind_point: 0,
                bind_count: 0,
                flags: 0,
                space: 2,
            },
            dxc::BoundResource {
                name: "g_material_textures".into(),
                kind: 2,
                bind_point: 0,
                bind_count: 0,
                flags: 0xC,
                space: 2,
            },
        ];
        let resources = StageResources::from_bindings(&bindings);
        assert_eq!(resources.cbuffers, vec!["global_viewport", "c_per_object"]);
        assert_eq!(resources.texture_arrays.len(), 1);
        assert_eq!(resources.sampler_arrays.len(), 1);
        assert!(resources.samples());

        // The descriptors the records index into, in the UI base's order.
        let descriptors = std::collections::BTreeMap::from([
            (C_PER_OBJECT, 0),
            (GLOBAL_VIEWPORT, 1),
            (GLOBAL_TEXTURE2D, 2),
            (GLOBAL_FEEDBACK_BUFFERS, 3),
        ]);
        let mut lists = shader::TailLists {
            lists: vec![Vec::new(); 9],
            block: Vec::new(),
        };
        set_derived_lists(&mut lists, &resources, &descriptors).unwrap();
        assert_eq!(
            lists.lists[3],
            vec![vec![GLOBAL_TEXTURE2D, 2, 0, 0xFFFF_FFFF, 2, 0xFFFF_FFFF, 0]]
        );
        assert_eq!(
            lists.lists[5],
            vec![vec![GLOBAL_FEEDBACK_BUFFERS, 3, 0, 0xFFFF_FFFF, 31, 0xFFFF_FFFF, 0]]
        );
        assert_eq!(lists.lists[6], vec![vec![STATIC_MINLOD_SAMPLER, 0, 1, 31]]);
        assert_eq!(lists.lists[8], vec![vec![GLOBAL_SAMPLERS, 0, 0]]);
    }

    #[test]
    fn a_group_query_comes_from_the_contexts() {
        // The contexts carry the query ids, one per group and in the same
        // order, so the file does not store them.
        use crate::filetype::group_data::{GroupParts, GroupTemplate};

        // Two contexts, three queries: {name, word, count} then {query,
        // conditions} per query.
        let mut contexts = Vec::new();
        for (name, queries) in [
            (0xAAAA_AAAAu32, vec![0x1111_1111u32, 0x2222_2222]),
            (0xBBBB_BBBB, vec![0x3333_3333]),
        ] {
            contexts.extend_from_slice(&name.to_le_bytes());
            contexts.extend_from_slice(&[0u8; 4]);
            contexts.extend_from_slice(&(queries.len() as u32).to_le_bytes());
            for query in queries {
                contexts.extend_from_slice(&query.to_le_bytes());
                contexts.extend_from_slice(&0u32.to_le_bytes());
            }
        }

        let group = |query: u32| GroupParts {
            head: {
                let mut head = query.to_le_bytes().to_vec();
                head.extend_from_slice(&[0u8; 8]); // the header word and a zero count
                head
            },
            material_first: true,
            between: Vec::new(),
            mid: Vec::new(),
            tail: Vec::new(),
        };
        let template = GroupTemplate {
            prefix: 1u32.to_le_bytes().to_vec(),
            groups: vec![group(0x1111_1111), group(0x2222_2222), group(0x3333_3333)],
            engine: Vec::new(),
        };

        let mut engine_data = empty_engine_data();
        engine_data.contexts = contexts;
        engine_data.group_template = Some(template);
        engine_data.material_tables = vec![Vec::new(); 3];

        let text = engine_data.to_text();
        assert!(!text.contains("query = "), "{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        let template = parsed.group_template.unwrap();
        let queries: Vec<u32> = template
            .groups
            .iter()
            .map(|group| u32::from_le_bytes(group.head[..4].try_into().unwrap()))
            .collect();
        assert_eq!(queries, vec![0x1111_1111, 0x2222_2222, 0x3333_3333]);
        assert_eq!(parsed.material_tables.len(), 3);
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
            let mut heads = Vec::new();
            let spec = block_text(&block, &preamble, &mut heads);
            let parsed = block_bytes(&spec, &preamble, &heads).unwrap();
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