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
use super::shader_node::{ResourceDecl, StageResources};
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

/// murmur32 of `c_instance_offset`: the engine's per-draw instance-buffer offset.
const C_INSTANCE_OFFSET: u32 = 0xE7DF_A2E1;

/// murmur32 of `bones`: the engine's skinning matrices.
const BONES: u32 = 0x18DE_AD01;

/// murmur32 of `last_bones`: the previous frame's skinning matrices.
const LAST_BONES: u32 = 0xF954_1801;

/// murmur32 of `idata`: the engine's per-instance data.
const IDATA: u32 = 0xF2DE_3925;

/// murmur32 of `global_uav_buffers`: the engine's raw UAV array.
const GLOBAL_UAV_BUFFERS: u32 = 0xA306_B24F;

/// murmur32 of `global_environment_settings`: the engine's environment cube /
/// settings constant buffer (a per-pass cbuffer, so flags `0x101`).
const GLOBAL_ENVIRONMENT_SETTINGS: u32 = 0xB3A2_EB88;

/// The group's descriptor list (`name -> index`). Every group must agree,
/// because the program-to-group mapping is not decoded.
fn descriptors(template: &GroupTemplate) -> Result<BTreeMap<u32, u32>> {
    let mut agreed: Option<BTreeMap<u32, u32>> = None;
    for parts in &template.groups {
        let list = group_descriptors(parts)?;
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

/// One group's descriptor list as a name-hash -> index map, from its head: the
/// query id, the group's two words, its descriptor list, then the first table's
/// header.
fn group_descriptors(parts: &crate::filetype::group_data::GroupParts) -> Result<BTreeMap<u32, u32>> {
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
    Ok(list)
}

/// The descriptors of one group, when a program's tail belongs to it - the
/// per-program form a family whose groups disagree on their lists needs.
fn descriptors_for_group(template: &GroupTemplate, group: usize) -> Result<BTreeMap<u32, u32>> {
    let parts = template
        .groups
        .get(group)
        .ok_or_else(|| color_eyre::eyre::eyre!("a program names group {group}, which is missing"))?;
    group_descriptors(parts)
}

/// The engine's spelling of a resource name. DXC names a texture / sampler
/// pair from a combined sampler object `__tex_<name>` / `__samp_<name>`; the
/// engine's own records use `<name>` (the same rule the
/// Darktide-Asset-Compiler's `id32` applies).
fn engine_name(name: &str) -> &str {
    for prefix in ["__tex_", "__samp_"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest;
        }
    }
    name
}

/// The lists (2, 3, 4, 5, 6 and 8) a stage's tail should hold, from its own
/// reflection: the non-UAV SRVs (bounded then unbounded), the UAVs (bounded
/// then unbounded), the static samplers, and the bindless sampler array.
///
/// The engine's own resources (`static_minlod_sampler`,
/// `global_feedback_buffers`, `global_texture2D`, `global_samplers`) are in the
/// container's reflection too, so they are written once, from it, rather than
/// as separate engine records.
///
/// The 7-word records carry the resource's index in the group's descriptor
/// list, so the descriptor table comes in with them. Their shape is
/// `{name, index, register, count, space, stride, 0}`, where `count` is
/// `0xFFFFFFFF` for an unbounded array and `stride` is 0 for a raw buffer, the
/// element stride for a structured buffer, or `0xFFFFFFFF` for a texture.
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
    let hash =
        |name: &str| -> u32 { u32::from(murmur::Murmur32::hash(engine_name(name).as_bytes())) };
    let resource = |decl: &ResourceDecl| -> Result<Vec<u32>> {
        let name = hash(&decl.name);
        let count = if decl.count == 0 {
            0xFFFF_FFFF
        } else {
            decl.count
        };
        Ok(vec![
            name,
            index(name)?,
            decl.register,
            count,
            decl.space,
            decl.stride,
            0,
        ])
    };

    // Lists 2 and 3: the non-UAV SRVs, bounded (srvs) and unbounded (arrays). A
    // structured buffer is one or the other by its bind count. A bindless
    // texture array is the engine's `global_texture2D` whatever the declaration
    // names it (`g_material_textures` in a from-scratch source).
    let mut srvs = Vec::new();
    let mut srv_arrays = Vec::new();
    for decl in &resources.textures {
        srvs.push(resource(decl)?);
    }
    for decl in &resources.texture_arrays {
        srv_arrays.push(vec![
            GLOBAL_TEXTURE2D,
            index(GLOBAL_TEXTURE2D)?,
            decl.register,
            0xFFFF_FFFF,
            decl.space,
            0xFFFF_FFFF,
            0,
        ]);
    }
    for decl in &resources.buffers {
        if decl.count == 0 {
            srv_arrays.push(resource(decl)?);
        } else {
            srvs.push(resource(decl)?);
        }
    }

    // Lists 4 and 5: the UAVs, bounded and unbounded. The engine's
    // `global_feedback_buffers` and `static_minlod_sampler` (list 6) are
    // sampler-side resources: a stage that binds the bindless sampler array
    // always carries them, whether or not the declaration names them. A
    // reconstructed container's reflection already has them, hence the dedupe; a
    // stage that merely reads a texture (`linear_depth`) without a sampler does
    // not (so the gate is the sampler array, not "samples a texture").
    let engine_sampler_resources = !resources.sampler_arrays.is_empty();
    let mut uavs = Vec::new();
    let mut uav_arrays = Vec::new();
    for decl in &resources.uavs {
        if decl.count == 0 {
            uav_arrays.push(resource(decl)?);
        } else {
            uavs.push(resource(decl)?);
        }
    }
    if engine_sampler_resources
        && descriptors.contains_key(&GLOBAL_FEEDBACK_BUFFERS)
        && !uav_arrays.iter().any(|record| record[0] == GLOBAL_FEEDBACK_BUFFERS)
    {
        uav_arrays.insert(
            0,
            vec![
                GLOBAL_FEEDBACK_BUFFERS,
                index(GLOBAL_FEEDBACK_BUFFERS)?,
                0,
                0xFFFF_FFFF,
                31,
                0xFFFF_FFFF,
                0,
            ],
        );
    }

    // List 6: the static samplers, `{name, register, count, space}`.
    let mut samplers: Vec<Vec<u32>> = resources
        .samplers
        .iter()
        .map(|decl| {
            vec![
                hash(&decl.name),
                decl.register,
                decl.count.max(1),
                decl.space,
            ]
        })
        .collect();
    if engine_sampler_resources && !samplers.iter().any(|record| record[0] == STATIC_MINLOD_SAMPLER)
    {
        samplers.insert(0, vec![STATIC_MINLOD_SAMPLER, 0, 1, 31]);
    }

    // List 8: the bindless sampler array, the engine's `global_samplers`. The
    // record is written in the engine's 3-word form; the codec reads a fourth
    // `space` word, which the block framing here absorbs (see `TailLists::SIZES`).
    let sampler_arrays = resources
        .sampler_arrays
        .iter()
        .map(|decl| vec![GLOBAL_SAMPLERS, decl.register, 0])
        .collect();

    lists.lists[2] = srvs;
    lists.lists[3] = srv_arrays;
    lists.lists[4] = uavs;
    lists.lists[5] = uav_arrays;
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

/// The number of contexts a contexts region carries: the `{name, word, count}`
/// records, walked so a variable-length record cannot be miscounted.
fn context_count_of(contexts: &[u8]) -> usize {
    let mut count = 0usize;
    let mut at = 0usize;
    while at + 12 <= contexts.len() {
        let queries = u32::from_le_bytes(contexts[at + 8..at + 12].try_into().unwrap()) as usize;
        let next = at + 12 + queries * 8;
        if next > contexts.len() {
            break;
        }
        count += 1;
        at = next;
    }
    count
}

/// The constant-buffer list a split tail should hold, from the container's own
/// reflection: the names in register order, the sizes from the compiled
/// container (it is the authority for every cbuffer, `c_per_object` included)
/// and the engine table (`global_viewport`), the indexes from the group's
/// descriptor list, and the constant 1 and 0 words.
///
/// A group's material table is only a fallback for `c_per_object`, when the
/// reflection gives no size: the table is our packing of the declaration's
/// inputs and can be narrower than the compiled buffer. A cbuffer with no size
/// source at all is an error rather than a guess.
fn derived_cbuffers(
    resources: &StageResources,
    template: &GroupTemplate,
    material: &[Record],
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
            // The compiled container's own size is authoritative: the group's
            // material table is our packing of the declaration's inputs, which
            // can be narrower than the compiled buffer (the standard's group 0
            // table is 16 bytes against a compiled 416), so the table is only a
            // fallback for a container whose size the reflection did not give.
            C_PER_OBJECT => resources
                .cbuffer_sizes
                .get(name)
                .copied()
                .filter(|size| *size > 0)
                .map(|size| size.div_ceil(16) * 16)
                .or_else(|| (!material.is_empty()).then(|| table_size(material)))
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("the cbuffer {hash:08X} ({name}) has no size")
                })?,
            GLOBAL_VIEWPORT => table_size(&template.engine),
            // The engine's other cbuffers take the compiled container's own
            // size, rounded to the 16-byte granularity the tail records use.
            _ => resources
                .cbuffer_sizes
                .get(name)
                .copied()
                .filter(|size| *size > 0)
                .map(|size| size.div_ceil(16) * 16)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("the cbuffer {hash:08X} ({name}) has no size source")
                })?,
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

/// The index of a tail in a deduplicated pool, adding it when new. Tails dedupe
/// as their semantic form (a condition set when it is one), so the written pool
/// is names rather than hex.
fn dedup_tail(list: &mut Vec<GroupTailText>, value: Vec<u8>) -> usize {
    let spec = GroupTailText::from_bytes(&value);
    match list.iter().position(|other| *other == spec) {
        Some(index) => index,
        None => {
            list.push(spec);
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
    /// The header's context count. Derived from `permutations` or the carried
    /// `contexts` blob when absent, so a file that names its plans does not
    /// repeat it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_count: Option<u32>,
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

/// A list of 32-bit hashes, written as quoted eight-digit words the way the file
/// writes every other name hash (a bare `0x…` token would read as neither an
/// integer nor an identifier, and a bare decimal loses the hex the docs use).
mod hash32_list {
    use serde::de::Error as _;
    use serde::ser::SerializeSeq as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::Hex;

    pub fn serialize<S: Serializer>(
        values: &[u32],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            seq.serialize_element(&Hex(value.to_be_bytes().to_vec()))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u32>, D::Error> {
        let values = Vec::<Hex>::deserialize(deserializer)?;
        values
            .into_iter()
            .map(|hex| {
                <[u8; 4]>::try_from(hex.0.as_slice())
                    .map(u32::from_be_bytes)
                    .map_err(|_| D::Error::custom("a condition hash needs four bytes"))
            })
            .collect()
    }
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
    /// The bytes before the first group. The common case is the group count
    /// alone, which is derived from the group list, so it is omitted then and
    /// written as hex only for a section with a longer prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prefix: Option<Hex>,
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
    tails: Vec<GroupTailText>,
    /// A repeating group list: when `groups` is empty and this is set, there is
    /// one group per query (in query order) and group `i` uses tail `i / repeat`
    /// with every other part at its zero index. The UI base writes 36 groups
    /// that are six repeats of its six tails, so the list is derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeat: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<GroupText>,
}

/// The bytes of a condition set: `u32 n`, then `n` 17-byte entries (`u32 hash`
/// plus thirteen zero bytes), then an eight-byte trailer (`01` when the set is
/// nonempty, zeros when it is empty). A group's tail is exactly this.
fn conditions_bytes(conditions: &[u32]) -> Vec<u8> {
    let nonempty = conditions.iter().any(|hash| *hash != 0);
    let mut out = (conditions.len() as u32).to_le_bytes().to_vec();
    for hash in conditions {
        out.extend_from_slice(&hash.to_le_bytes());
        out.extend_from_slice(&[0u8; 13]);
    }
    out.extend_from_slice(&if nonempty {
        [1, 0, 0, 0, 0, 0, 0, 0]
    } else {
        [0u8; 8]
    });
    out
}

/// A group's condition-header tail: a plain condition set, or raw bytes when it
/// is not one.
///
/// The UI base's six tails are all condition sets: `u32 n`, then `n` entries of
/// a condition hash plus thirteen zero bytes, then an eight-byte trailer (`01`
/// when the set is nonempty, zeros when it is empty). Written as the hash list,
/// the fixed bytes derive, so the family's tails are names rather than hex.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum GroupTailText {
    /// `{ conditions = [ "9FCFE126" "625D415E" ] }`.
    Conditions {
        #[serde(with = "hash32_list")]
        conditions: Vec<u32>,
    },
    /// `{ context = "default" pass = 0 }`: the condition set of one of the
    /// declaration's compile jobs - the pass at `pass` in `context`, its
    /// `layer`/`render_state`/`defines` hashed with `Murmur32`. Resolved by the
    /// build, which holds the declaration; the reader keeps it unresolved.
    Pass {
        context: String,
        pass: usize,
    },
    /// The whole tail as bytes, when it is not a plain condition set.
    Hex(Hex),
}

impl GroupTailText {
    /// The tail's bytes. A `Pass` is resolved by the caller (which holds the
    /// declaration) before the bytes are needed, so it contributes nothing on
    /// its own - see [`GroupTailText::pass_names`].
    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Hex(hex) => hex.0.clone(),
            Self::Conditions { conditions } => conditions_bytes(conditions),
            // Unresolved: the build replaces it via `resolved`.
            Self::Pass { .. } => Vec::new(),
        }
    }

    /// The condition set, when the bytes are exactly one.
    fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() >= 4 {
            let n = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            let expected = 4 + n * 17 + 8;
            if bytes.len() == expected {
                let mut conditions = Vec::with_capacity(n);
                let mut ok = true;
                for index in 0..n {
                    let at = 4 + index * 17;
                    conditions.push(u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()));
                    if bytes[at + 4..at + 17].iter().any(|byte| *byte != 0) {
                        ok = false;
                    }
                }
                if ok {
                    return Self::Conditions { conditions };
                }
            }
        }
        Self::Hex(Hex(bytes.to_vec()))
    }
}

/// The number of conditions in a condition-header tail, when its bytes are one:
/// `u32 n`, `n` 17-byte entries, then an eight-byte trailer.
fn condition_count(bytes: &[u8]) -> Option<usize> {
    let n = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    (bytes.len() == 4 + n * 17 + 8).then_some(n)
}

/// Fills the program-to-group mapping when the file carries none: a group owns
/// `2 × n` consecutive programs, where `n` is the number of conditions in the
/// group's tail, in group order. The rule was measured across six families
/// (`examples/program_groups.rs`); a family whose total does not match its
/// program count is left unmapped, and its tails stay carried.
fn derive_program_groups(engine_data: &mut EngineData, group_conditions: &[usize]) {
    if engine_data.program_groups.iter().any(Option::is_some) {
        return;
    }
    let mut groups = Vec::new();
    for (group, count) in group_conditions.iter().enumerate() {
        groups.extend(std::iter::repeat_n(Some(group), count * 2));
    }
    if !groups.is_empty() && groups.len() == engine_data.programs.len() {
        engine_data.program_groups = groups;
    }
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
    /// Whether the group carries the shared engine table. Defaults to true; a
    /// group with no engine table writes `engine = false`.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    engine: bool,
}

/// The default for `GroupText.engine`.
fn default_true() -> bool {
    true
}

/// Whether a group carries the shared engine table.
fn is_true(value: &bool) -> bool {
    *value
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

/// A group's head descriptors, from the group's programs' reflected bindings
/// (in program order, vertex/pixel pairs): each distinct binding once, in
/// **first-binding order**, with samplers dropped (they live in the tail lists,
/// not the head). Flags are the engine's mined ones, or - for a name the engine
/// table does not know - the binding's own kind. `y` is two bits per pass: bit
/// `2k` when the resource is bound as an SRV in pass `k` (the program pair
/// `2k`/`2k+1`), bit `2k+1` as a UAV; a cbuffer takes no bit. Measured against
/// the standard: this reproduces group 0's shipped descriptor list and its
/// `y` values (`0x155` for an SRV bound in all five passes, `0x2AA` for a UAV).
pub fn head_descriptors(programs: &[Vec<dxc::BoundResource>]) -> Vec<(u32, u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    for (index, bindings) in programs.iter().enumerate() {
        let pass = index / 2;
        for binding in bindings {
            // A sampler is not a head descriptor.
            if binding.kind == 3 {
                continue;
            }
            let hash = u32::from(murmur::Murmur32::hash(binding.name.as_bytes()));
            let slot = match out.iter().position(|(name, ..)| *name == hash) {
                Some(slot) => slot,
                None => {
                    let flags = match engine_descriptor_flags(hash) {
                        0 => binding.kind,
                        flags => flags,
                    };
                    out.push((hash, flags, 0));
                    out.len() - 1
                }
            };
            // A cbuffer takes no pass bit.
            if binding.kind == 0 {
                continue;
            }
            let bit = if matches!(binding.kind, 4 | 6..=11) {
                pass * 2 + 1
            } else {
                pass * 2
            };
            out[slot].2 |= 1 << bit;
        }
    }
    out
}

/// The engine's group head after the query: the header word, the descriptor
/// list and the two trailing words. The descriptors are the engine's own
/// resources (their flags mined from the shipped groups) and the stage's own
/// cbuffers and structured buffers, taken from the container's reflection.
/// `X` is the per-draw byte offset (24 per constant buffer, 8 per other) and
/// `Y` the engine's packed usage counts (measured: `{0, 0, 5, 10}` for a
/// texture and a feedback buffer; only a zero `Y` breaks the render).
fn derived_head(resources: &HashMap<Stage, StageResources>) -> Vec<u8> {
    let samples = resources.values().any(StageResources::samples);

    // The compiled cbuffer sizes, by the hash a descriptor carries: the header
    // word needs its material (kind-0) cbuffers' sizes.
    let mut sizes: HashMap<u32, u32> = HashMap::new();
    for stage in resources.values() {
        for (name, size) in &stage.cbuffer_sizes {
            sizes.insert(u32::from(murmur::Murmur32::hash(name.as_bytes())), *size);
        }
    }

    let mut descriptors: Vec<(u32, u32, u32)> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    let mut push = |name: u32, flags: u32, y: u32| {
        if !seen.contains(&name) {
            seen.push(name);
            descriptors.push((name, flags, y));
        }
    };

    // The engine's own descriptors first, then the stages' resources. The
    // engine's cbuffers and buffers are the shader's own binding order, so the
    // reflection supplies them; an unknown name falls back to its kind.
    push(C_PER_OBJECT, 0x0000, 0);
    push(GLOBAL_VIEWPORT, 0x0101, 0);
    for stage in [Stage::Vertex, Stage::Pixel] {
        let Some(stage_resources) = resources.get(&stage) else {
            continue;
        };
        for name in &stage_resources.cbuffers {
            let hash = u32::from(murmur::Murmur32::hash(name.as_bytes()));
            push(hash, engine_descriptor_flags(hash), 0);
        }
        for decl in &stage_resources.buffers {
            let hash = u32::from(murmur::Murmur32::hash(decl.name.as_bytes()));
            push(hash, engine_descriptor_flags(hash), 0);
        }
    }
    if samples {
        push(GLOBAL_TEXTURE2D, 0x0103, 5);
        push(GLOBAL_FEEDBACK_BUFFERS, 0x0105, 10);
    }

    // The header word: the per-draw total (24 per cbuffer, 8 per other)
    // rounded up to 16, plus the sizes of the material (kind-0) cbuffers -
    // measured exact on the standard's five shipped groups.
    let per_draw: u32 = descriptors
        .iter()
        .map(|(_, flags, _)| if flags & 0xFF <= 1 { 24 } else { 8 })
        .sum();
    let material: u32 = descriptors
        .iter()
        .filter(|(_, flags, _)| flags & 0xFF == 0)
        .filter_map(|(name, _, _)| sizes.get(name).copied())
        .sum();
    let header = per_draw.div_ceil(16) * 16 + material;

    let mut out = Vec::new();
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(&(descriptors.len() as u32).to_le_bytes());
    let mut x = 0u32;
    for (name, flags, y) in &descriptors {
        for word in [*name, *flags, x, *y] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        x += if flags & 0xFF <= 1 { 24 } else { 8 };
    }
    // The trailer: the number of cbuffer descriptors, then the first table's
    // record count (here the material table's, which `GroupData::build`
    // rewrites; a group with an engine table takes the engine pool's).
    let cbuffers = descriptors
        .iter()
        .filter(|(_, flags, _)| flags & 0xFF <= 1)
        .count() as u32;
    out.extend_from_slice(&cbuffers.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// A group's head, derived from its own programs: the descriptors are the
/// group's programs' reflected bindings in first-binding order (samplers
/// dropped) with the engine's flags and the per-pass `y` bits; the header word
/// is the per-draw total rounded up to 16 plus the material cbuffers' compiled
/// sizes; the trailer is the cbuffer-descriptor count and the first table's
/// record count. Measured against the standard's five shipped groups.
fn derived_head_for_group(
    programs: &[StageResources],
    material_count: usize,
    engine_count: usize,
    has_engine: bool,
) -> Vec<u8> {
    let bindings: Vec<Vec<dxc::BoundResource>> = programs
        .iter()
        .map(|resources| resources.bindings.clone())
        .collect();
    let descriptors = head_descriptors(&bindings);

    // The material cbuffers' compiled sizes, by the hash a descriptor carries.
    let mut sizes: HashMap<u32, u32> = HashMap::new();
    for resources in programs {
        for (name, size) in &resources.cbuffer_sizes {
            sizes.insert(u32::from(murmur::Murmur32::hash(name.as_bytes())), *size);
        }
    }

    let step = |flags: u32| if flags & 0xFF <= 1 { 24 } else { 8 };
    let per_draw: u32 = descriptors.iter().map(|(_, flags, _)| step(*flags)).sum();
    let material: u32 = descriptors
        .iter()
        .filter(|(_, flags, _)| flags & 0xFF == 0)
        .filter_map(|(name, _, _)| sizes.get(name).copied())
        .sum();
    let header = per_draw.div_ceil(16) * 16 + material;
    let cbuffers = descriptors
        .iter()
        .filter(|(_, flags, _)| flags & 0xFF <= 1)
        .count();

    let mut out = Vec::new();
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(&(descriptors.len() as u32).to_le_bytes());
    let mut x = 0u32;
    for (name, flags, y) in &descriptors {
        for word in [*name, *flags, x, *y] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        x += step(*flags);
    }
    out.extend_from_slice(&(cbuffers as u32).to_le_bytes());
    let first_table = if has_engine { engine_count } else { material_count };
    out.extend_from_slice(&(first_table as u32).to_le_bytes());
    out
}

/// The descriptor flags of the engine's own resources, mined from the shipped
/// groups (`descriptor-table.ps1`). The low three bits are the kind (0 material
/// cbuffer, 1 engine cbuffer, 3 texture, 4 structured SRV, 5 UAV, 7 raw UAV);
/// the high bits are per-resource and are not in the DXIL reflection, so an
/// unknown name takes only its kind's low bits.
fn engine_descriptor_flags(name: u32) -> u32 {
    match name {
        GLOBAL_VIEWPORT => 0x0000_0101,
        GLOBAL_ENVIRONMENT_SETTINGS => 0x0000_0101,
        C_INSTANCE_OFFSET | C_PER_OBJECT => 0x0000_0000,
        BONES | LAST_BONES => 0x0040_0004,
        IDATA => 0x0010_0004,
        GLOBAL_UAV_BUFFERS => 0x0000_0107,
        GLOBAL_TEXTURE2D => 0x0000_0103,
        GLOBAL_FEEDBACK_BUFFERS => 0x0000_0105,
        _ => 0x0000_0000,
    }
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
    /// The declaration compile job this program's container comes from, when
    /// the file carries no container bytes: the build compiles the job and
    /// frames the program with its container. It is the source-tree form of
    /// `container`, which a captured file uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    job: Option<usize>,
    /// The group this program's tail belongs to, when a split tail derives its
    /// lists against that group's descriptors (a family whose groups disagree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<usize>,
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
    /// The declaration compile job whose container each program uses, parallel
    /// to `programs`, for a file that carries no container bytes. The build
    /// compiles the job and frames the program with its container, so the
    /// source declaration is the only source.
    pub program_jobs: Vec<Option<usize>>,
    /// The group each program's tail belongs to, parallel to `programs`. A
    /// split tail derives its lists against that group's descriptor list; a
    /// family whose groups disagree on it needs this, since one map cannot
    /// stand for all groups.
    pub program_groups: Vec<Option<usize>>,
    /// Whether each program's tail is a **split** tail - an empty constant
    /// buffer list and empty resource lists that the build fills from the
    /// container's reflection - parallel to `programs`. A carried tail, even one
    /// with no constant buffers, is not split; an empty vector (a from-scratch
    /// file whose programs come from the declaration) means every program is
    /// split.
    pub program_split: Vec<bool>,
    /// Group tails written as a `pass = N` reference (tail slot, context, pass
    /// index), for [`EngineData::resolve_pass_tails`].
    pub pass_tail_refs: Vec<(usize, String, usize)>,
    /// The tail slot each template group uses, parallel to the groups, so a
    /// resolved pass tail can be written back to the groups that reference it.
    pub group_tail_slots: Vec<usize>,
    /// The `repeat` the file wrote (groups per tail), retained so a file that
    /// carries no tails can build its group list from the derived pool.
    pub group_repeat: usize,
}

/// The UI base family's per-pass channel sets, in pixel-program order: each
/// pass's `render_states` `write_mask0`, a `|`-joined channel list. The block's
/// mask byte is [`write_mask`] of one of these, so the mask is derived from the
/// pass's declared channels rather than recorded as a byte. The engine's own
/// `gui_gradient` declares these branches (`WRITE_MASK1` -> green, `WRITE_MASK2`
/// -> blue, else red; `07`/`0F` the default and full-alpha cases).
const UI_BASE_PASS_CHANNELS: [&str; 48] = [
    "red|green|blue",
    "red",
    "red",
    "green",
    "green",
    "blue",
    "blue",
    "alpha",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue|alpha",
    "red|green|blue",
    "red|green|blue",
    "red",
    "red",
    "green",
    "green",
    "blue",
    "blue",
    "alpha",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue|alpha",
    "red|green|blue",
    "red",
    "green",
    "blue",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue",
    "red",
    "green",
    "blue",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue",
    "red",
    "green",
    "blue",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue",
    "red",
    "green",
    "blue",
    "alpha",
    "red|green|blue|alpha",
    "red|green|blue",
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

/// Packs a pass's channel set into the block's channel-write mask byte.
///
/// The mask is the engine's `render_states` `write_mask0`, a `|`-joined channel
/// list; one bit per channel (`red` 1, `green` 2, `blue` 4, `alpha` 8). A pass
/// that names no channel keeps the preamble body's default `07`
/// (`red|green|blue`). Measured 48/48 against the shipped UI base: the 48
/// per-pass masks are exactly their `write_mask0` (see `Shader RE TODO.md`).
pub fn write_mask(channels: &[&str]) -> u8 {
    let mut mask = 0u8;
    let mut named = false;
    for channel in channels {
        let channel = channel.trim().to_ascii_lowercase();
        if channel.is_empty() || channel == "none" {
            named = true;
            continue;
        }
        for part in channel.split('|') {
            let bit = match part.trim() {
                "red" => 0x01,
                "green" => 0x02,
                "blue" => 0x04,
                "alpha" => 0x08,
                _ => continue,
            };
            mask |= bit;
            named = true;
        }
    }
    if named { mask } else { 0x07 }
}

/// Packs a query's macro tokens into the channel-write mask: a token is read by
/// its last `_`-separated word, so `CHANNEL_RED` names red. A query whose tokens
/// name no channel keeps the body's default `07`.
pub fn write_mask_tokens(tokens: &[String]) -> u8 {
    let names: Vec<&str> = tokens
        .iter()
        .filter_map(|token| token.rsplit('_').next())
        .collect();
    write_mask(&names)
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
            program_split: vec![false; programs.len()],
            programs,
            tails: Vec::new(),
            containers,
            program_containers,
            program_jobs: Vec::new(),
            program_groups: Vec::new(),
            permutations: None,
            pass_tail_refs: Vec::new(),
            group_tail_slots: Vec::new(),
            group_repeat: 0,
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
            // The count is the plans' contexts when the file names them, the
            // carried blob's contexts otherwise.
            context_count: text.context_count.unwrap_or_else(|| {
                text.permutations
                    .as_ref()
                    .map(|plans| plans.len() as u32)
                    .unwrap_or_else(|| context_count_of(&text.contexts.0) as u32)
            }),
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
            program_jobs: Vec::new(),
            program_groups: Vec::new(),
            program_split: Vec::new(),
            permutations: text
                .permutations
                .map(|plans| plans.into_iter().map(Into::into).collect()),
            pass_tail_refs: Vec::new(),
            group_tail_slots: Vec::new(),
            group_repeat: 0,
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
        // The condition count of each group's tail, for the program-to-group
        // mapping when the file carries no `group` tags.
        let mut group_conditions: Vec<usize> = Vec::new();
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
                            engine: true,
                        })
                        .collect()
                }
                _ => template.groups.clone(),
            };
            let mut group_tail_slots: Vec<usize> = Vec::with_capacity(expanded.len());
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
                    has_engine: group.engine,
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
                    tail: template
                        .tails
                        .get(group.tail)
                        .map(GroupTailText::bytes)
                        .ok_or_else(|| {
                            color_eyre::eyre::eyre!(
                                "a group names a tail at #{}, which is missing",
                                group.tail
                            )
                        })?,
                });
                group_tail_slots.push(group.tail);
                // An empty `materials` list means the table is derived - the
                // declaration names it - so the caller fills `material_tables`.
                if !template.materials.is_empty() {
                    tables.push(records_from_bytes(&part(
                        &template.materials,
                        group.material,
                        "material",
                    )?)?);
                }
            }
            // The prefix is the group count alone in the common case; the text
            // omits it then, and the reader writes the count its group list
            // implies.
            let prefix = match &template.prefix {
                Some(prefix) => prefix.clone().into_bytes(),
                None => (groups.len() as u32).to_le_bytes().to_vec(),
            };
            engine_data.group_template = Some(GroupTemplate {
                prefix,
                groups,
                engine,
            });
            engine_data.material_tables = tables;
            engine_data.group_tail_slots = group_tail_slots.clone();
            engine_data.group_repeat = template.repeat.unwrap_or(0);
            group_conditions = group_tail_slots
                .iter()
                .map(|slot| match template.tails.get(*slot) {
                    Some(GroupTailText::Conditions { conditions }) => conditions.len(),
                    Some(GroupTailText::Hex(hex)) => condition_count(&hex.0).unwrap_or(0),
                    _ => 0,
                })
                .collect();
            for (slot, tail) in template.tails.iter().enumerate() {
                if let GroupTailText::Pass { context, pass } = tail {
                    engine_data.pass_tail_refs.push((slot, context.clone(), *pass));
                }
            }
        }

        // The tails: a split tail's constant-buffer list and resource lists are
        // derived at build time, so the reader writes an empty prefix and empty
        // lists for it to fill; a carried tail is the whole tail. A tail written
        // with `lists` (even an all-zero one) is carried whole, and its program
        // must not be derived.
        let split_flags: Vec<bool> = text.tails.iter().map(|tail| tail.lists.is_none()).collect();
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
            engine_data.program_jobs.push(program.job);
            engine_data.program_groups.push(program.group);
            engine_data
                .program_split
                .push(split_flags.get(program.tail).copied().unwrap_or(false));
        }

        // A file that carries the group data as a blob but whose groups walk is
        // read as its template, so a write-back drops the blob (the groups'
        // heads/descriptors/headers dedupe). The template is only adopted when
        // it rebuilds the blob byte for byte - a family whose packed run or
        // condition framing the reader does not model losslessly keeps its blob
        // rather than risk a different section on the next build.
        if engine_data.group_template.is_none() && !engine_data.group_data.is_empty() {
            if let Ok(query_ids) = context_queries(&engine_data.contexts) {
                let group =
                    crate::filetype::group_data::GroupData::new(engine_data.group_data.clone());
                if let Some(template) = group.template(&query_ids)
                    && let Some(tables) = group.object_tables(&query_ids)
                {
                    let material: Vec<Vec<crate::filetype::group_data::Record>> =
                        tables.iter().map(|(_, records)| records.clone()).collect();
                    let rebuilt = crate::filetype::group_data::GroupData::build(
                        &template,
                        &material,
                        &template.engine,
                    );
                    if rebuilt.ok().as_deref() == Some(engine_data.group_data.as_slice()) {
                        engine_data.group_template = Some(template);
                        engine_data.material_tables = material;
                        engine_data.group_data = Vec::new();
                    }
                }
            }
        }

        derive_program_groups(&mut engine_data, &group_conditions);

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
            let mut tails: Vec<GroupTailText> = Vec::new();
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
                    tail: dedup_tail(&mut tails, parts.tail.clone()),
                    // The material table is derived when the file carries
                    // none (a from-scratch file names it), so the index is the
                    // zero placeholder either way.
                    material: material_indexes.get(index).copied().unwrap_or(0),
                    material_first: parts.material_first,
                    engine: parts.has_engine,
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

            // The prefix is the group count alone in the common case, so it
            // derives from the group list and the text omits it; a section with
            // a longer prefix keeps it.
            let prefix = (template.prefix != (template.groups.len() as u32).to_le_bytes())
                .then(|| template.prefix.clone().into());
            GroupTemplateText {
                prefix,
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
        // A split tail's resource records index the group's descriptor list. A
        // family whose groups agree on it (or whose programs carry their group
        // via `program_groups`, derived or written) can split; one that does
        // neither keeps its tails carried.
        let descriptors_agree = self
            .group_template
            .as_ref()
            .is_some_and(|template| descriptors(template).is_ok());
        let per_program_groups = self.program_groups.iter().any(Option::is_some);
        let tails: Vec<TailText> = tails
            .iter()
            .map(|tail| {
                // A tail is split - and stores only its block - when its
                // constant buffers are all the engine's own, the groups agree on
                // their descriptor lists (or each program names its group) and
                // its lists parse: the build derives both from the container's
                // own reflection. Anything else is carried whole.
                let parsed = shader::Tail::parse(tail);
                let lists = parsed
                    .as_ref()
                    .and_then(|parsed| shader::TailLists::parse(&parsed.rest));
                let split = match (&parsed, &lists) {
                    (Some(_), Some(_)) => descriptors_agree || per_program_groups,
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
            .enumerate()
            .map(|(index, (stage, _))| ProgramText {
                stage: match stage {
                    Stage::Vertex => "Vertex".to_string(),
                    Stage::Pixel => "Pixel".to_string(),
                    Stage::Geometry => "Geometry".to_string(),
                    Stage::Hull => "Hull".to_string(),
                    Stage::Domain => "Domain".to_string(),
                    Stage::Compute => "Compute".to_string(),
                    Stage::Other => "Other".to_string(),
                },
                tail: tail_indexes[index],
                container: container_indexes[index],
                job: self.program_jobs.get(index).copied().flatten(),
                group: self.program_groups.get(index).copied().flatten(),
            })
            .collect();

        // The count is the contexts' own when they are carried, and the plans'
        // when they are not; it is written only when it does not follow from
        // whichever of the two the file holds.
        let context_count = match (&self.permutations, self.contexts.is_empty()) {
            (Some(plans), _) if plans.len() != self.context_count as usize => {
                Some(self.context_count)
            }
            (Some(_), _) => None,
            (None, false) if context_count_of(&self.contexts) != self.context_count as usize => {
                Some(self.context_count)
            }
            (None, false) => None,
            (None, true) => Some(self.context_count),
        };
        Text {
            context_count,
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
            let mask = write_mask(&[UI_BASE_PASS_CHANNELS[pass]]);
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
        containers: &HashMap<Stage, Vec<u8>>,
        resources: &HashMap<Stage, StageResources>,
    ) -> Option<GroupTemplate> {
        let mut template = self.group_template.as_ref()?.clone();
        let engine_count = template.engine.len();
        for (index, group) in template.groups.iter_mut().enumerate() {
            // A head that is just the query waits for its derived rest. A
            // group with no engine table legitimately has an empty `between`,
            // so it is only derived when the group carries the engine table.
            if group.head.len() == 4 {
                // A group with its own programs derives its head from their
                // reflections; a declaration-only group falls back to the
                // shader-wide resources.
                let programs = self.group_programs(containers, index);
                let head = if programs.is_empty() {
                    derived_head(resources)
                } else {
                    let material = self.material_tables.get(index).map_or(0, Vec::len);
                    derived_head_for_group(&programs, material, engine_count, group.has_engine)
                };
                group.head.extend_from_slice(&head);
            }
            if group.between.is_empty() && group.has_engine {
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

    /// The reflections of a group's programs, in program order (vertex/pixel
    /// pairs), picking each container the same way `build_device` does.
    fn group_programs(
        &self,
        containers: &HashMap<Stage, Vec<u8>>,
        group: usize,
    ) -> Vec<StageResources> {
        let mut out = Vec::new();
        for (index, (stage, _)) in self.programs.iter().enumerate() {
            if self.program_groups.get(index).copied().flatten() != Some(group) {
                continue;
            }
            let carried = self
                .program_containers
                .get(index)
                .copied()
                .flatten()
                .and_then(|container| self.containers.get(container));
            let Some(container) = containers.get(stage).or(carried) else {
                continue;
            };
            if let Ok(reflected) = StageResources::from_container(container) {
                out.push(reflected);
            }
        }
        out
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
            // own reflection; a carried tail is left alone. A carried tail can
            // itself have no constant buffers (a vertex program's, or a family's
            // phase-prefixed tail), so the split form is the one with **empty
            // lists** too.
            let derived = match shader::Tail::parse(tail) {
                Some(parsed)
                    if self.program_split.get(index).copied().unwrap_or(true)
                        && parsed.cbuffers.is_empty()
                        && shader::TailLists::parse(&parsed.rest).is_some_and(|lists| {
                            lists.lists.iter().all(|records| records.is_empty())
                        }) =>
                {
                    // The program's own container's reflection: a multi-job
                    // family's programs bind different resources, so the first
                    // job's per-stage reflection cannot stand in. Fall back to
                    // the passed-in per-stage resources when reflection fails.
                    let reflected = crate::filetype::shader_node::StageResources::from_container(container).ok();
                    let resources = reflected.as_ref().or_else(|| resources.get(stage)).ok_or_else(
                        || {
                            color_eyre::eyre::eyre!(
                                "no reflection for program {index} ({stage:?}): its constant buffers \
                                 and resource lists are derived from the compiled container"
                            )
                        },
                    )?;
                    let template = template.ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "program {index} ({stage:?}) needs a group template to index its \
                             records"
                        )
                    })?;
                    let descriptors = if self.program_groups.is_empty() {
                        descriptors(template)?
                    } else {
                        // A program that names no group has a tail whose records
                        // are common to every group (a vertex tail's), so the
                        // first group stands in.
                        let group = self.program_groups.get(index).copied().flatten();
                        descriptors_for_group(template, group.unwrap_or(0))?
                    };
                    let mut lists = shader::TailLists::parse(&parsed.rest).ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "program {index} ({stage:?}): a split tail's lists do not parse"
                        )
                    })?;
                    set_derived_lists(&mut lists, resources, &descriptors)?;
                    let material: &[Record] = match self.material_tables.len() {
                        0 => &[],
                        1 => &self.material_tables[0],
                        _ => {
                            let group = self
                                .program_groups
                                .get(index)
                                .copied()
                                .flatten()
                                .unwrap_or(0);
                            self.material_tables.get(group).map(Vec::as_slice).unwrap_or(&[])
                        }
                    };
                    let mut bytes =
                        derived_cbuffers(resources, template, material, &descriptors)?;
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

    /// Resolves the group tails written as a `pass = N` reference against the
    /// declaration's compile jobs.
    ///
    /// A tail names a context and the pass index within it; its condition set is
    /// that pass's `layer`/`render_state`/`defines`, hashed with `Murmur32`. The
    /// build calls this once the declaration is compiled and before `generate`;
    /// a file whose tails are concrete condition sets has nothing to resolve.
    pub fn resolve_pass_tails(
        &mut self,
        jobs: &[crate::filetype::shader_node::CompileJob],
    ) -> Result<()> {
        if self.pass_tail_refs.is_empty() {
            return Ok(());
        }
        // The condition bytes of each referenced tail slot: the `pass`-th job of
        // `context`, in order.
        let mut resolved: std::collections::HashMap<usize, Vec<u8>> =
            std::collections::HashMap::new();
        for (slot, context, pass) in &self.pass_tail_refs {
            let job = jobs
                .iter()
                .filter(|job| &job.context == context)
                .nth(*pass)
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "a tail names {context}'s pass {pass}, which the declaration \
                         does not compile"
                    )
                })?;
            resolved.insert(*slot, conditions_bytes(&job.condition_ids()));
        }

        let template = self
            .group_template
            .as_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("a pass tail needs a group template"))?;
        for (index, group) in template.groups.iter_mut().enumerate() {
            let slot = self
                .group_tail_slots
                .get(index)
                .copied()
                .ok_or_else(|| color_eyre::eyre::eyre!("a group has no tail slot"))?;
            if let Some(bytes) = resolved.get(&slot) {
                group.tail = bytes.clone();
            }
        }
        Ok(())
    }

    /// The context query ids, in context order - the walk that pairs a group
    /// with the query that selects it.
    pub fn query_ids(&self) -> Result<Vec<u32>> {
        context_queries(&self.contexts)
    }

    /// Fills `material_tables` from the carried `group_data` blob when the file
    /// has no template: the groups walk by the contexts' query ids and each
    /// group's material table is read out. This is what a family whose groups do
    /// not template needs, so its split tails still have a `c_per_object` size
    /// source (`--reconstruct` writes the blob, not the template, there).
    pub fn fill_material_tables(&mut self) -> Result<()> {
        if !self.material_tables.is_empty() || self.group_data.is_empty() {
            return Ok(());
        }
        let query_ids = context_queries(&self.contexts)?;
        let group = crate::filetype::group_data::GroupData::new(self.group_data.clone());
        if let Some(tables) = group.object_tables(&query_ids) {
            self.material_tables = tables.into_iter().map(|(_, records)| records).collect();
        }
        Ok(())
    }

    /// Derives the group tails from the declaration's compile jobs, when the
    /// file carries none: each job's condition set is `conditions_bytes` of its
    /// `condition_ids` (its `layer`/`render_state`, hashed).
    ///
    /// A declaration that tags its passes with a `group` carries the
    /// group→tail assignment: one group per distinct index, in the order the
    /// groups first appear, each with that group's condition set. A declaration
    /// that names no group falls back to the repeating model (group `i` uses
    /// tail `i / repeat`), which is the UI base's shape.
    ///
    /// The UI base's six tails are exactly six passes' condition sets (see
    /// `docs/Shader Permutations.md`), so this replaces the carried pool.
    pub fn derive_tails(
        &mut self,
        jobs: &[crate::filetype::shader_node::CompileJob],
    ) -> Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }
        // A file that carries its own group data (a blob, not a template) keeps
        // it: its groups are the family's own, not the declaration's single pass.
        if !self.group_data.is_empty() {
            return Ok(());
        }
        // The distinct condition sets, in job order.
        let mut pool: Vec<Vec<u8>> = Vec::new();
        for job in jobs {
            let bytes = conditions_bytes(&job.condition_ids());
            if !pool.contains(&bytes) {
                pool.push(bytes);
            }
        }
        if pool.is_empty() {
            return Ok(());
        }
        // A file that carried a template with tails keeps them.
        if self
            .group_template
            .as_ref()
            .is_some_and(|template| template.groups.iter().any(|group| !group.tail.is_empty()))
        {
            return Ok(());
        }

        // The declaration can name the group each pass belongs to. A group owns
        // several passes (one per code block), so the group→tail assignment is
        // carried per group rather than recovered from a uniform `repeat`: one
        // group per distinct index, its tail the condition set of that group's
        // passes, the groups in the order their first pass appears. A
        // declaration that names no group falls back to the repeating model.
        let tagged = jobs.iter().any(|job| job.group.is_some());
        let (groups, slots): (
            Vec<crate::filetype::group_data::GroupParts>,
            Vec<usize>,
        ) = if tagged {
            let mut order: Vec<usize> = Vec::new();
            let mut slots: Vec<usize> = Vec::new();
            let mut groups: Vec<crate::filetype::group_data::GroupParts> = Vec::new();
            for job in jobs {
                let group = job.group.unwrap_or(0);
                if order.contains(&group) {
                    continue;
                }
                let bytes = conditions_bytes(&job.condition_ids());
                slots.push(pool.iter().position(|other| *other == bytes).unwrap_or(0));
                order.push(group);
                groups.push(crate::filetype::group_data::GroupParts {
                    head: 0u32.to_le_bytes().to_vec(),
                    material_first: true,
                    has_engine: true,
                    between: Vec::new(),
                    mid: Vec::new(),
                    tail: bytes,
                });
            }
            (groups, slots)
        } else {
            // The groups per tail: the file's `repeat`, or the query count over
            // the tail count (36 queries / 6 tails = 6).
            let repeat = if self.group_repeat > 0 {
                self.group_repeat
            } else {
                jobs.len().div_ceil(pool.len()).max(1)
            };
            let groups: Vec<crate::filetype::group_data::GroupParts> = (0..repeat * pool.len())
                .map(|index| crate::filetype::group_data::GroupParts {
                    head: 0u32.to_le_bytes().to_vec(),
                    material_first: true,
                    has_engine: true,
                    between: Vec::new(),
                    mid: Vec::new(),
                    tail: pool[index / repeat].clone(),
                })
                .collect();
            let slots = (0..groups.len()).map(|index| index / repeat).collect();
            (groups, slots)
        };
        let count = groups.len() as u32;
        self.group_tail_slots = slots;
        match self.group_template.as_mut() {
            Some(template) => {
                template.groups = groups;
                template.prefix = count.to_le_bytes().to_vec();
            }
            None => {
                self.group_template = Some(crate::filetype::group_data::GroupTemplate {
                    prefix: count.to_le_bytes().to_vec(),
                    groups,
                    engine: tool_engine_table()?,
                });
            }
        }
        Ok(())
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
        // shipped blob still carries its original ids). The header's
        // `context_count` is the number of context records - not the queries.
        let (contexts, context_count) = match &self.permutations {
            Some(plans) if !plans.is_empty() => {
                (permutations_contexts(material, plans)?, plans.len() as u32)
            }
            _ => (self.contexts.clone(), self.context_count),
        };
        // The template first: the device's tails read its descriptor list, so
        // the derived head has to exist before the device is built.
        let mut template = self.derived_template(containers, resources);

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

    /// The head's header word: the per-draw total (24 per cbuffer, 8 per other)
    /// rounded up to 16, plus the material (kind-0) cbuffers' compiled sizes.
    #[test]
    fn the_head_header_word_grows_with_the_material_cbuffers() {
        let mut pixel = StageResources::default();
        pixel.cbuffers = vec![
            "global_viewport".to_string(),
            "c_instance_offset".to_string(),
            "c_depth_only".to_string(),
        ];
        pixel
            .cbuffer_sizes
            .insert("global_viewport".to_string(), 1776);
        pixel
            .cbuffer_sizes
            .insert("c_instance_offset".to_string(), 16);
        pixel.cbuffer_sizes.insert("c_depth_only".to_string(), 304);
        let mut resources = HashMap::new();
        resources.insert(Stage::Pixel, pixel);

        let head = derived_head(&resources);
        let header = u32::from_le_bytes(head[0..4].try_into().unwrap());
        // The four descriptors (c_per_object, global_viewport, c_instance_offset,
        // c_depth_only) are 24 per draw = 96, already 16-aligned; the material
        // cbuffers are c_instance_offset (16) + c_depth_only (304) = 320.
        assert_eq!(header, 96 + 320);
    }

    /// A group's head descriptors: the reflected bindings in first-binding
    /// order, samplers dropped, flags from the engine table or the kind, and the
    /// per-pass `y` bits.
    #[test]
    fn head_descriptors_are_the_bindings_without_samplers() {
        let binding = |name: &str, kind: u32| dxc::BoundResource {
            name: name.into(),
            kind,
            bind_point: 0,
            bind_count: 1,
            flags: 0,
            space: 0,
            samples: 0,
            size: 0,
            variables: Vec::new(),
        };
        // Two passes (four programs): a cbuffer, an SRV bound in both passes, a
        // sampler, an unknown SRV in pass 1, and a UAV in pass 1.
        let programs = vec![
            vec![
                binding("global_viewport", 0),
                binding("idata", 5),
                binding("global_samplers", 3),
            ],
            vec![binding("global_viewport", 0), binding("idata", 5)],
            vec![binding("idata", 5), binding("tex", 2)],
            vec![binding("idata", 5), binding("uav", 7)],
        ];
        let descriptors = head_descriptors(&programs);
        let hash = |name: &str| u32::from(murmur::Murmur32::hash(name.as_bytes()));
        let names: Vec<u32> = descriptors.iter().map(|(name, ..)| *name).collect();
        assert_eq!(
            names,
            vec![
                hash("global_viewport"),
                hash("idata"),
                hash("tex"),
                hash("uav")
            ]
        );
        // A cbuffer takes no bit; `idata` is an SRV in passes 0 and 1 (bits 0 and
        // 2); `tex` is an SRV only in pass 1 (bit 2); `uav` a UAV only in pass 1
        // (bit 3).
        assert_eq!(descriptors[0].2, 0);
        assert_eq!(descriptors[1].2, 0b0101);
        assert_eq!(descriptors[2].2, 0b0100);
        assert_eq!(descriptors[3].2, 0b1000);
        assert_eq!(descriptors[0].1, 0x0101); // the engine's mined flags
        assert_eq!(descriptors[2].1, 2); // an unknown name takes its kind
    }

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
            program_jobs: Vec::new(),
            program_groups: Vec::new(),
            program_split: Vec::new(),
            pass_tail_refs: Vec::new(),
            group_tail_slots: Vec::new(),
            group_repeat: 0,
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
        assert!(
            !plans[0].queries.is_empty(),
            "queries: {:?}",
            plans[0].queries
        );
        assert_eq!(plans[0].queries.len(), 1);
        assert_eq!(
            plans[0].queries[0]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["SINGLE"],
            "queries: {:?}",
            plans[0].queries
        );
        // The count follows from the plans, so the file does not write it.
        assert_eq!(parsed.context_count, 1);
        let text_out = parsed.to_text();
        assert!(
            !text_out.contains("context_count"),
            "the count is repeated: {text_out}"
        );
        let round_tripped = EngineData::from_text(&text_out).unwrap();
        assert_eq!(round_tripped.context_count, 1);
        assert_eq!(round_tripped.permutations.expect("plans survive"), plans);
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
    fn a_program_round_trips_a_job_reference() {
        // The source-tree form: the file names the declaration's compile job
        // for a program and carries no container bytes.
        let mut engine_data = empty_engine_data();
        let tail = vec![1u8, 2, 3, 4];
        engine_data.programs = vec![
            (Stage::Vertex, tail.clone()),
            (Stage::Pixel, tail.clone()),
        ];
        engine_data.program_jobs = vec![Some(2), Some(2)];

        let text = engine_data.to_text();
        assert!(text.contains("job = 2"), "{text}");
        assert!(!text.contains("containers = ["), "{text}");
        assert!(!text.contains("container = "), "{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        assert_eq!(parsed.program_jobs, vec![Some(2), Some(2)]);
        assert_eq!(parsed.program_containers, vec![None, None]);
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
            has_engine: true,
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
            has_engine: true,
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
        // The prefix is the group count alone, so it derives from the list.
        assert!(!text.contains("prefix ="), "the count is still written:\n{text}");

        let parsed = EngineData::from_text(&text).unwrap();
        let template = parsed.group_template.as_ref().expect("template");
        assert_eq!(template.prefix, 6u32.to_le_bytes());
        let groups = &template.groups;
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
    fn a_pass_tail_resolves_from_the_declaration() {
        // A group tail may name one of the declaration's compile jobs instead of
        // carrying hashes: `{ context = "default" pass = 0 }`. The build
        // resolves it to that pass's layer/render_state/defines, hashed with
        // Murmur32 - here `gui` (9FCFE126), `gui_hdr` (BC4EE226) and `SINGLE`
        // (407AB909).
        use crate::filetype::shader_node::{CompileJob, JobMacro};

        let text = r#"
group_template = {
    repeat = 1
    tails = [ { context = "default" pass = 0 } ]
}
contexts = ""
preamble = { bytes = "" }
programs = []
"#;
        let mut engine_data = EngineData::from_text(text).unwrap();
        // One group referencing tail 0. The reader leaves it empty until the
        // declaration is known.
        assert_eq!(engine_data.group_tail_slots, vec![0]);
        assert_eq!(engine_data.pass_tail_refs.len(), 1);

        let jobs = vec![CompileJob {
            context: "default".into(),
            permutation: 0,
            code_block: "b".into(),
            layer: Some("gui".into()),
            render_state: Some("gui_hdr".into()),
            condition_terms: vec![
                crate::filetype::shader_node::ConditionTerm::Name("gui".into()),
                crate::filetype::shader_node::ConditionTerm::Name("gui_hdr".into()),
            ],
            group: None,
            macros: vec![JobMacro {
                name: "SINGLE".into(),
                stages: Vec::new(),
            }],
        }];
        engine_data.resolve_pass_tails(&jobs).unwrap();

        let expected = conditions_bytes(&[0x9FCF_E126, 0xBC4E_E226]);
        assert_eq!(
            engine_data.group_template.as_ref().unwrap().groups[0].tail,
            expected
        );
    }

    /// A pass's `group` tag carries the group→tail assignment: one group per
    /// distinct index, in first-appearance order, so a family whose passes
    /// outnumber its groups (the standard family's 11 passes over 5 groups)
    /// derives its own list instead of a uniform `repeat`.
    #[test]
    fn a_passes_group_tag_carries_the_group_to_tail_assignment() {
        let job = |group: usize, layer: &str| crate::filetype::shader_node::CompileJob {
            context: "default".into(),
            permutation: 0,
            code_block: "b".into(),
            layer: Some(layer.into()),
            render_state: None,
            condition_terms: vec![crate::filetype::shader_node::ConditionTerm::Name(
                layer.into(),
            )],
            group: Some(group),
            macros: Vec::new(),
        };
        // Two passes share group 0, one pass is group 1.
        let jobs = vec![job(0, "gui"), job(0, "gui"), job(1, "gui_hdr")];

        let mut engine_data = empty_engine_data();
        engine_data.derive_tails(&jobs).unwrap();

        let template = engine_data.group_template.as_ref().unwrap();
        assert_eq!(template.groups.len(), 2, "one group per distinct index");
        assert_eq!(
            template.groups[0].tail,
            conditions_bytes(&[0x9FCF_E126])
        );
        assert_eq!(
            template.groups[1].tail,
            conditions_bytes(&[0xBC4E_E226])
        );
        assert_eq!(engine_data.group_tail_slots, vec![0, 1]);
    }

    #[test]
    fn the_ui_bases_masks_are_the_passes_write_masks() {
        // The UI base's 48 shipped pixel masks are exactly each pass's
        // `write_mask0` channel set, packed one bit per channel (`red` 1,
        // `green` 2, `blue` 4, `alpha` 8). Measured 48/48 in `Shader RE TODO.md`.
        let masks: [u8; 48] = [
            0x07, 0x01, 0x01, 0x02, 0x02, 0x04, 0x04, 0x08, 0x08, 0x0F, 0x0F, 0x07, 0x07, 0x01,
            0x01, 0x02, 0x02, 0x04, 0x04, 0x08, 0x08, 0x0F, 0x0F, 0x07, 0x01, 0x02, 0x04, 0x08,
            0x0F, 0x07, 0x01, 0x02, 0x04, 0x08, 0x0F, 0x07, 0x01, 0x02, 0x04, 0x08, 0x0F, 0x07,
            0x01, 0x02, 0x04, 0x08, 0x0F, 0x07,
        ];
        let channels = [
            "red|green|blue", "red", "red", "green", "green", "blue", "blue", "alpha", "alpha",
            "red|green|blue|alpha", "red|green|blue|alpha", "red|green|blue",
            "red|green|blue", "red", "red", "green", "green", "blue", "blue", "alpha", "alpha",
            "red|green|blue|alpha", "red|green|blue|alpha", "red|green|blue",
            "red", "green", "blue", "alpha", "red|green|blue|alpha", "red|green|blue",
            "red", "green", "blue", "alpha", "red|green|blue|alpha", "red|green|blue",
            "red", "green", "blue", "alpha", "red|green|blue|alpha", "red|green|blue",
            "red", "green", "blue", "alpha", "red|green|blue|alpha", "red|green|blue",
        ];
        assert_eq!(channels.len(), 48);
        for (pass, mask) in masks.iter().enumerate() {
            assert_eq!(write_mask(&[channels[pass]]), *mask, "pass {pass}");
        }
        // A pass naming no channel keeps the body's default.
        assert_eq!(write_mask(&[]), 0x07);
        // A token is read by its last word, so `CHANNEL_RED` names red.
        assert_eq!(write_mask_tokens(&["TAIL_0".into(), "CHANNEL_RED".into()]), 0x01);
    }

    #[test]
    fn a_condition_tail_round_trips_as_its_hash_list() {
        // The UI base's tails are condition sets: `u32 n`, `n` entries of a hash
        // plus thirteen zeros, an eight-byte trailer. The text writes the hash
        // list, and the reader rebuilds the same bytes.
        let tail = |hashes: &[u32], nonempty: bool| {
            let mut bytes = (hashes.len() as u32).to_le_bytes().to_vec();
            for hash in hashes {
                bytes.extend_from_slice(&hash.to_le_bytes());
                bytes.extend_from_slice(&[0u8; 13]);
            }
            bytes.extend_from_slice(&if nonempty {
                [1, 0, 0, 0, 0, 0, 0, 0]
            } else {
                [0u8; 8]
            });
            bytes
        };
        let gui_transparent = tail(&[0x9FCF_E126, 0x625D_415E], true);
        let empty = tail(&[0], false);

        // The recognizer reads each back to its condition list and rebuilds the
        // identical bytes.
        for bytes in [&gui_transparent, &empty] {
            match GroupTailText::from_bytes(bytes) {
                GroupTailText::Conditions { .. } => {}
                other => panic!("a condition set was not recognised: {other:?}"),
            }
            assert_eq!(&GroupTailText::from_bytes(bytes).bytes(), bytes);
        }
        let GroupTailText::Conditions { conditions } =
            GroupTailText::from_bytes(&gui_transparent)
        else {
            panic!("expected conditions");
        };
        assert_eq!(conditions, vec![0x9FCF_E126, 0x625D_415E]);

        // The text form writes each hash as a quoted word, the way the file
        // writes every other name hash; a bare `0x…` token would read as
        // neither an integer nor an identifier. The text must round trip to
        // the same bytes.
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrap {
            tail: GroupTailText,
        }
        let text = serde_sjson::to_string(&Wrap {
            tail: GroupTailText::from_bytes(&gui_transparent),
        })
        .unwrap();
        assert!(
            text.contains("\"9FCFE126\""),
            "unexpected tail text: {text}"
        );
        assert!(
            text.contains("\"625D415E\""),
            "unexpected tail text: {text}"
        );
        let back = serde_sjson::from_str::<Wrap>(&text).unwrap().tail;
        assert_eq!(back, GroupTailText::from_bytes(&gui_transparent));
        assert_eq!(back.bytes(), gui_transparent);

        // A tail that is not a plain condition set stays bytes.
        let odd = vec![0x03, 0x00, 0x00, 0x00, 0xFF];
        assert!(matches!(
            GroupTailText::from_bytes(&odd),
            GroupTailText::Hex(_)
        ));
        assert_eq!(GroupTailText::from_bytes(&odd).bytes(), odd);
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
                has_engine: true,
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
            derived_cbuffers(&resources, &template, &material_table, &descriptors).unwrap();
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
        // The engine's own resource names, and the engine's spelling of the
        // DXC-generated texture/sampler pair names (`__tex_`/`__samp_` stripped:
        // the Darktide-Asset-Compiler's `id32` rule).
        assert_eq!(
            GLOBAL_TEXTURE2D,
            u32::from(murmur::Murmur32::hash(b"global_texture2D"))
        );
        assert_eq!(
            GLOBAL_FEEDBACK_BUFFERS,
            u32::from(murmur::Murmur32::hash(b"global_feedback_buffers"))
        );
        assert_eq!(
            GLOBAL_SAMPLERS,
            u32::from(murmur::Murmur32::hash(b"global_samplers"))
        );
        assert_eq!(engine_name("__tex__mi_texcoord0"), "_mi_texcoord0");
        assert_eq!(engine_name("__samp__mi_coords"), "_mi_coords");
        assert_eq!(engine_name("linear_depth"), "linear_depth");

        fn binding(
            name: &str,
            kind: u32,
            bind_point: u32,
            bind_count: u32,
            space: u32,
            samples: u32,
        ) -> dxc::BoundResource {
            dxc::BoundResource {
                name: name.into(),
                kind,
                bind_point,
                bind_count,
                flags: 0,
                space,
                samples,
                size: 0,
                variables: Vec::new(),
            }
        }

        // A pixel stage's reflection, as the standard family's containers report
        // it: two cbuffers, the engine's sampler/texture/UAV resources, one
        // material texture and one bounded structured buffer.
        let bindings = vec![
            binding("global_viewport", 0, 0, 1, 0, 0),
            binding("c_per_object", 0, 1, 1, 0, 0),
            binding("global_samplers", 3, 0, 0, 2, 0),
            binding("static_minlod_sampler", 3, 0, 1, 31, 0),
            binding("__samp__mi_coords", 3, 0, 1, 0, 0),
            binding("global_texture2D", 2, 0, 0, 2, 0),
            binding("__tex__mi_texcoord0", 2, 0, 1, 0, 0),
            binding("bones", 5, 0, 0, 12, 64),
            binding("idata", 5, 0, 0, 9, 16),
            binding("bounded_sbuffer", 5, 0, 1, 0, 32),
            binding("global_feedback_buffers", 4, 0, 0, 31, 0),
            binding("global_uav_buffers", 8, 0, 0, 10, 0),
            binding("uav_scratch", 4, 0, 1, 0, 0),
        ];
        let resources = StageResources::from_bindings(&bindings);
        assert_eq!(resources.cbuffers, vec!["global_viewport", "c_per_object"]);
        assert_eq!(resources.textures.len(), 1);
        assert_eq!(resources.texture_arrays.len(), 1);
        assert_eq!(resources.samplers.len(), 2);
        assert_eq!(resources.sampler_arrays.len(), 1);
        assert_eq!(resources.buffers.len(), 3);
        assert_eq!(resources.uavs.len(), 3);
        // A structured buffer's stride is the reflection's `NumSamples`.
        assert_eq!(resources.buffers[0].stride, 64);
        assert_eq!(resources.buffers[1].stride, 16);

        // The descriptor (root parameter) indices the records carry.
        let mi_texcoord0 = u32::from(murmur::Murmur32::hash(b"_mi_texcoord0"));
        let mi_coords = u32::from(murmur::Murmur32::hash(b"_mi_coords"));
        let bounded_sbuffer = u32::from(murmur::Murmur32::hash(b"bounded_sbuffer"));
        let uav_scratch = u32::from(murmur::Murmur32::hash(b"uav_scratch"));
        let descriptors = std::collections::BTreeMap::from([
            (GLOBAL_VIEWPORT, 0),
            (C_PER_OBJECT, 1),
            (GLOBAL_TEXTURE2D, 2),
            (mi_texcoord0, 3),
            (BONES, 4),
            (IDATA, 5),
            (GLOBAL_UAV_BUFFERS, 6),
            (GLOBAL_FEEDBACK_BUFFERS, 7),
            (GLOBAL_SAMPLERS, 8),
            (uav_scratch, 9),
            (bounded_sbuffer, 10),
        ]);

        let mut lists = shader::TailLists {
            lists: vec![Vec::new(); 9],
            block: Vec::new(),
        };
        set_derived_lists(&mut lists, &resources, &descriptors).unwrap();

        // Bounded SRVs (list 2): the bounded texture and the bounded structured
        // buffer, `{name, index, register, count, space, stride, 0}`.
        assert_eq!(
            lists.lists[2],
            vec![
                vec![mi_texcoord0, 3, 0, 1, 0, 0xFFFF_FFFF, 0],
                vec![bounded_sbuffer, 10, 0, 1, 0, 32, 0],
            ]
        );
        // Unbounded SRVs (list 3): the bindless texture array, then the
        // unbounded structured buffers.
        assert_eq!(
            lists.lists[3],
            vec![
                vec![GLOBAL_TEXTURE2D, 2, 0, 0xFFFF_FFFF, 2, 0xFFFF_FFFF, 0],
                vec![BONES, 4, 0, 0xFFFF_FFFF, 12, 64, 0],
                vec![IDATA, 5, 0, 0xFFFF_FFFF, 9, 16, 0],
            ]
        );
        // Bounded UAVs (list 4) and unbounded UAVs (list 5).
        assert_eq!(
            lists.lists[4],
            vec![vec![uav_scratch, 9, 0, 1, 0, 0xFFFF_FFFF, 0]]
        );
        assert_eq!(
            lists.lists[5],
            vec![
                vec![GLOBAL_FEEDBACK_BUFFERS, 7, 0, 0xFFFF_FFFF, 31, 0xFFFF_FFFF, 0],
                vec![GLOBAL_UAV_BUFFERS, 6, 0, 0xFFFF_FFFF, 10, 0, 0],
            ]
        );
        // Static samplers (list 6), `{name, register, count, space}`.
        assert_eq!(
            lists.lists[6],
            vec![
                vec![STATIC_MINLOD_SAMPLER, 0, 1, 31],
                vec![mi_coords, 0, 1, 0],
            ]
        );
        // The bindless sampler array (list 8).
        assert_eq!(lists.lists[8], vec![vec![GLOBAL_SAMPLERS, 0, 0]]);
        // The codec's unused second constant-buffer list and `resources[0]`.
        assert!(lists.lists[0].is_empty());
        assert!(lists.lists[1].is_empty());
    }

    /// Diffs the derived resource lists against a built material's carried
    /// tails. `DTMT_ENGINE_DATA` is the engine-data text, `DTMT_MATERIAL` the
    /// material file the build produced from it; with neither set the test is a
    /// no-op. Every program should derive the lists its tail carries, which is
    /// what the split needs.
    #[test]
    #[ignore = "needs DTMT_ENGINE_DATA and DTMT_MATERIAL"]
    fn the_derived_lists_match_a_built_section() {
        let (Ok(engine_text), Ok(material_path)) = (
            std::env::var("DTMT_ENGINE_DATA"),
            std::env::var("DTMT_MATERIAL"),
        ) else {
            return;
        };
        let engine = EngineData::from_text(&std::fs::read_to_string(engine_text).unwrap()).unwrap();
        let data = std::fs::read(&material_path).unwrap();
        let shader_offset = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let shader_size = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
        let section = &data[shader_offset..shader_offset + shader_size];
        let device_offset = u32::from_le_bytes(section[40..44].try_into().unwrap()) as usize;
        let device_size = u32::from_le_bytes(section[44..48].try_into().unwrap()) as usize;
        let device = &section[device_offset..device_offset + device_size];
        let programs = shader::parse_programs(device).unwrap();
        let template = engine.group_template.clone().expect("a group template");
        assert_eq!(programs.len(), engine.programs.len());

        let mut mismatches = 0;
        for (index, program) in programs.iter().enumerate() {
            let carried = &engine.programs[index].1;
            let (Some(parsed), Ok(resources)) = (
                shader::Tail::parse(carried),
                StageResources::from_container(&program.container),
            ) else {
                continue;
            };
            let Some(carried_lists) = shader::TailLists::parse(&parsed.rest) else {
                continue;
            };
            let group = engine
                .program_groups
                .get(index)
                .copied()
                .flatten()
                .unwrap_or(0);
            let descriptors = descriptors_for_group(&template, group).unwrap();
            let mut derived = shader::TailLists {
                lists: vec![Vec::new(); 9],
                block: Vec::new(),
            };
            set_derived_lists(&mut derived, &resources, &descriptors).unwrap();
            for list in [2usize, 3, 4, 5, 6, 8] {
                if derived.lists[list] != carried_lists.lists[list] {
                    mismatches += 1;
                    println!(
                        "program {index} ({:?}, group {group}) list {list}:\n  carried {:?}\n  derived {:?}",
                        program.stage, carried_lists.lists[list], derived.lists[list]
                    );
                }
            }
            // The constant-buffer prefix too.
            let material = engine
                .material_tables
                .get(group)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            match derived_cbuffers(&resources, &template, material, &descriptors) {
                Ok(derived_cbuffers) => {
                    let carried_prefix = &carried[..4 + parsed.cbuffers.len() * 24];
                    if derived_cbuffers != carried_prefix {
                        mismatches += 1;
                        println!(
                            "program {index} ({:?}, group {group}) cbuffers:\n  carried {:?}\n  derived {:?}",
                            program.stage, carried_prefix, derived_cbuffers
                        );
                    }
                }
                Err(err) => {
                    mismatches += 1;
                    println!("program {index} ({:?}, group {group}) cbuffers: {err}", program.stage);
                }
            }
        }

        // Per-group head: the descriptors (names), their flags, the x offset and
        // the y usage mask, from the group's programs' bindings.
        let mut derived_groups: Vec<Vec<u32>> = vec![Vec::new(); template.groups.len()];
        let mut derived_y: Vec<std::collections::BTreeMap<u32, u32>> =
            (0..template.groups.len())
                .map(|_| std::collections::BTreeMap::new())
                .collect();
        let mut derived_kind: Vec<std::collections::BTreeMap<u32, u32>> =
            (0..template.groups.len())
                .map(|_| std::collections::BTreeMap::new())
                .collect();
        let mut current = usize::MAX;
        let mut position = 0u32;
        for (index, program) in programs.iter().enumerate() {
            let group = engine
                .program_groups
                .get(index)
                .copied()
                .flatten()
                .unwrap_or(0);
            if group != current {
                current = group;
                position = 0;
            }
            let Ok(bindings) = dxc::reflect(&program.container) else {
                position += 1;
                continue;
            };
            let Some(list) = derived_groups.get_mut(group) else {
                continue;
            };
            for binding in &bindings {
                if !matches!(binding.kind, 0 | 2 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11) {
                    continue;
                }
                let name =
                    u32::from(murmur::Murmur32::hash(engine_name(&binding.name).as_bytes()));
                if !list.contains(&name) {
                    list.push(name);
                }
                derived_kind[group].entry(name).or_insert(binding.kind);
                // The y mask: two bits per pass, bit 2k when the resource is
                // bound as an SRV in pass k and bit 2k+1 when as a UAV (a
                // cbuffer takes no bit).
                if binding.kind != 0 {
                    let uav = matches!(binding.kind, 4 | 6 | 8 | 9 | 10 | 11) as u32;
                    *derived_y[group].entry(name).or_default() |=
                        1 << (2 * (position / 2) + uav);
                }
            }
            position += 1;
        }
        let mut fields = [0usize; 3];
        for (index, parts) in template.groups.iter().enumerate() {
            let shipped = head_records(&parts.head);
            let names: Vec<u32> = shipped.iter().map(|r| r[0]).collect();
            if derived_groups.get(index) != Some(&names) {
                mismatches += 1;
                println!("group {index} descriptor order differs");
            }
            let mut x = 0u32;
            for record in &shipped {
                let kind = derived_kind[index].get(&record[0]).copied().unwrap_or(0);
                let flags = engine_descriptor_flags(record[0]) | kind_low(kind);
                if flags != record[1] {
                    if fields[0] < 6 {
                        println!(
                            "group {index} {:08X} flags shipped {:08X} derived {:08X}",
                            record[0], record[1], flags
                        );
                    }
                    fields[0] += 1;
                }
                if x != record[2] {
                    fields[1] += 1;
                }
                let y = derived_y[index].get(&record[0]).copied().unwrap_or(0);
                if y != record[3] {
                    if fields[2] < 6 {
                        println!(
                            "group {index} {:08X} y shipped {:X} derived {:X}",
                            record[0], record[3], y
                        );
                    }
                    fields[2] += 1;
                }
                x += if flags & 0xFF <= 1 { 24 } else { 8 };
            }
        }
        println!(
            "head field mismatches: flags {} x {} y {} (of {} descriptors)",
            fields[0],
            fields[1],
            fields[2],
            template.groups.iter().map(|g| head_records(&g.head).len()).sum::<usize>()
        );

        // The `between` part: `{global_viewport size, 0, material record count}`
        // for a group that carries the engine table, empty for one that does not.
        let viewport_size = programs
            .iter()
            .find_map(|program| {
                StageResources::from_container(&program.container)
                    .ok()
                    .and_then(|r| r.cbuffer_sizes.get("global_viewport").copied())
            })
            .unwrap_or(0);
        for (index, parts) in template.groups.iter().enumerate() {
            let derived_between = if parts.has_engine {
                let mut between = viewport_size.to_le_bytes().to_vec();
                between.extend_from_slice(&0u32.to_le_bytes());
                let materials = engine
                    .material_tables
                    .get(index)
                    .map(Vec::len)
                    .unwrap_or(0) as u32;
                between.extend_from_slice(&materials.to_le_bytes());
                between
            } else {
                Vec::new()
            };
            if derived_between != parts.between {
                mismatches += 1;
                println!(
                    "group {index} between: shipped {:02X?} derived {:02X?}",
                    parts.between, derived_between
                );
            }
        }

        // The `mid` part is not derived yet: report the shipped and the
        // `derived_mid` sizes, and how many leading bytes agree.
        for (index, parts) in template.groups.iter().enumerate() {
            let records = engine
                .material_tables
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let candidate = derived_mid(records);
            let agree = parts
                .mid
                .iter()
                .zip(&candidate)
                .take_while(|(a, b)| a == b)
                .count();
            println!(
                "group {index} mid: shipped {} derived_mid {} ({} leading bytes agree)",
                parts.mid.len(),
                candidate.len(),
                agree
            );
        }

        assert_eq!(mismatches, 0, "{mismatches} list mismatches");
    }

    /// A resource kind's low descriptor-flag bits (the engine's kind class: 0
    /// material cbuffer, 3 texture, 4 structured SRV, 5 UAV, 7 raw UAV).
    fn kind_low(kind: u32) -> u32 {
        match kind {
            1 => 0x1,
            2 => 0x3,
            4 | 6 | 9 | 10 | 11 => 0x5,
            5 | 7 => 0x4,
            8 => 0x7,
            _ => 0x0,
        }
    }

    /// A group head's descriptor records: after the query id and one word, a u32
    /// count then `count` 16-byte `{name, flags, x, y}` records.
    fn head_records(head: &[u8]) -> Vec<[u32; 4]> {
        let Some(tail) = head.get(8..) else {
            return Vec::new();
        };
        let Some(n) = tail
            .get(..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
        else {
            return Vec::new();
        };
        let Some(bytes) = tail.get(4..4 + n * 16) else {
            return Vec::new();
        };
        bytes
            .chunks_exact(16)
            .map(|r| std::array::from_fn(|w| u32::from_le_bytes(r[w * 4..w * 4 + 4].try_into().unwrap())))
            .collect()
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
            has_engine: true,
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


