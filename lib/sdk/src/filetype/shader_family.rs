//! Declarative shader-family definitions: the mod-side input to a generated
//! `shader43` section.
//!
//! A [`Family`] is what a mod writes instead of a shipped shader blob: the
//! programs (by source file), the channels the family exchanges between its
//! stages, and the material variables it accepts. [`Family::permutations`]
//! enumerates the runtime permutations - one per combination of the declared
//! optional variables - and each becomes a group in the section, compiled with
//! the flags its variables enable.
//!
//! The format is SJSON, the dialect the material and shader node files use, with
//! the names as map keys so they can be hashed straight to murmur32:
//!
//! ```sjson
//! // snoopy_ui.shader_family
//! channels = {
//!     vertex_position = {
//!         type = "float4"
//!         domain = "vertex"
//!         required = true
//!     }
//!     texture_map = {
//!         type = "texture2d"
//!         domain = "pixel"
//!         required = true
//!     }
//! }
//!
//! variables = {
//!     base_color = {
//!         type = "vector3"
//!         domain = "pixel"
//!         flag = "HAS_BASE_COLOR"
//!         default = [ 1, 1, 1 ]
//!     }
//!     opacity = {
//!         type = "scalar"
//!         domain = "pixel"
//!         flag = "HAS_OPACITY"
//!     }
//!     mod_tint = {
//!         type = "vector4"
//!         domain = "pixel"
//!     }
//! }
//!
//! programs = {
//!     vs_main = {
//!         source = "snoopymod/ui.vs.hlsl"
//!         stage = "vertex"
//!     }
//!     ps_main = {
//!         source = "snoopymod/ui.ps.hlsl"
//!         stage = "pixel"
//!     }
//! }
//! ```
//!
//! Everything the section still needs from the engine - the block header blob,
//! the engine's variable registry, the bindless conventions - is deliberately
//! not part of this file; it is carried by the toolchain instead.

use std::collections::BTreeMap;

use color_eyre::eyre;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The stage a channel or variable belongs to. A `vertex` channel is written by
/// the vertex program and interpolated into the pixel program; a `pixel` one is
/// a pixel-program-only value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// Written by the vertex stage and read by the pixel stage.
    Vertex,
    /// Pixel stage only.
    #[default]
    Pixel,
}

/// The type of a channel or variable. The sizes match the group data's record
/// sizes: 4, 8, 12 and 16 bytes, and 64 for a 4x4 matrix. The material's own
/// spelling (`scalar`, `vector2`, ...) is accepted as an alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// A scalar (4 bytes).
    #[serde(alias = "scalar")]
    Float,
    /// A two-component vector (8 bytes).
    #[serde(alias = "vector2")]
    Float2,
    /// A three-component vector (12 bytes).
    #[serde(alias = "vector3")]
    Float3,
    /// A four-component vector (16 bytes).
    #[serde(alias = "vector4")]
    Float4,
    /// A 4x4 matrix (64 bytes).
    #[serde(alias = "matrix", alias = "float3x3")]
    Float4x4,
    /// A 2D texture channel.
    Texture2D,
}

impl ValueType {
    /// The number of bytes one element of this type occupies, as the group data
    /// records it.
    pub fn size(self) -> u32 {
        match self {
            Self::Float => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 | Self::Texture2D => 16,
            Self::Float4x4 => 64,
        }
    }
}

/// A channel: a named value the vertex program hands to the pixel program, or a
/// texture the material binds.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelDef {
    /// The channel's type.
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// Which stage produces it.
    #[serde(default)]
    pub domain: Domain,
    /// Whether the material must provide it. A required channel is in every
    /// interface; an optional one joins only the permutations that define its
    /// variable's flag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// The variable whose flag gates this channel, when it is not the channel's
    /// own name (a `normal_map` texture can follow `normal_strength`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    /// The DXBC semantic to bind the channel to, if not the default for its
    /// type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
}

/// A material variable: a value the material may set by name, like
/// `dev_wireframe_color` in the shipped families. A variable with a `flag` is
/// optional - it belongs to the interface only in the permutations that define
/// that flag.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDef {
    /// The variable's type.
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// Which stage reads it.
    #[serde(default)]
    pub domain: Domain,
    /// The permutation flag that makes the variable part of the interface. A
    /// variable without a flag is always present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    /// The default value written into the section's default data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default: Vec<f32>,
}

/// One program of a stage, given by the source file `dtmt build` compiles.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDef {
    /// The HLSL source, relative to the mod root.
    pub source: String,
    /// `vertex` or `pixel`.
    pub stage: String,
}

/// A whole family declaration: the programs to compile, the channels the family
/// exchanges and the material variables it accepts, keyed by name.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// The programs to compile, keyed by entry point name.
    #[serde(default)]
    pub programs: BTreeMap<String, ProgramDef>,
    /// The channels the family exchanges.
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelDef>,
    /// The material variables the family accepts.
    #[serde(default)]
    pub variables: BTreeMap<String, VariableDef>,
}

impl Family {
    /// Parses a declaration from SJSON.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        serde_sjson::from_str(sjson)
            .map_err(|err| eyre::eyre!("failed to parse the shader family: {err}"))
    }

    /// Serialises the declaration back to SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).wrap_err("failed to write the shader family")
    }

    /// The flags the declaration can define, in name order.
    pub fn flags(&self) -> Vec<&str> {
        let mut flags: Vec<&str> = Vec::new();
        for variable in self.variables.values() {
            if let Some(flag) = &variable.flag
                && !flags.contains(&flag.as_str())
            {
                flags.push(flag);
            }
        }
        flags
    }

    /// Enumerates the runtime permutations: one per combination of the declared
    /// optional variables, in the order the flags are named (bit 0 first). Each
    /// becomes one group in the section and one program pair compiled with the
    /// flags its variables enable; the variables and channels that ride along
    /// are that group's interface.
    pub fn permutations(&self) -> Vec<Permutation> {
        let flags = self.flags();
        let count = 1u32 << flags.len();
        (0..count)
            .map(|mask| Permutation {
                mask,
                flags: flags
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| mask & (1 << bit) != 0)
                    .map(|(_, flag)| (*flag).to_string())
                    .collect(),
                variables: self
                    .variables
                    .iter()
                    .filter(|(_, variable)| match &variable.flag {
                        None => true,
                        Some(flag) => self.enabled(flag, mask, &flags),
                    })
                    .map(|(name, _)| name.clone())
                    .collect(),
                channels: self
                    .channels
                    .iter()
                    .filter(|(name, channel)| {
                        channel.required
                            || self
                                .gating_variable(name, channel)
                                .and_then(|variable| variable.flag.as_ref())
                                .is_some_and(|flag| self.enabled(flag, mask, &flags))
                    })
                    .map(|(name, _)| name.clone())
                    .collect(),
            })
            .collect()
    }

    /// The number of groups the declaration generates.
    pub fn group_count(&self) -> usize {
        1usize << self.flags().len()
    }

    /// Whether `flag` is set in `mask`.
    fn enabled(&self, flag: &str, mask: u32, flags: &[&str]) -> bool {
        flags
            .iter()
            .position(|f| *f == flag)
            .is_some_and(|bit| mask & (1 << bit) != 0)
    }

    /// The variable that gates the channel `name`: the one it names, or the
    /// variable of the same name.
    fn gating_variable<'a>(
        &'a self,
        name: &'a str,
        channel: &'a ChannelDef,
    ) -> Option<&'a VariableDef> {
        self.variables
            .get(channel.variable.as_deref().unwrap_or(name))
    }
}

/// One enumerated permutation: the flags it defines and the interface it
/// exposes. `mask` is the bit set over [`Family::flags`] and is what the
/// conditions tree indexes on.
#[derive(Clone, Debug, PartialEq)]
pub struct Permutation {
    /// The bit set over [`Family::flags`].
    pub mask: u32,
    /// The permutation flags, in declaration order.
    pub flags: Vec<String>,
    /// The variable names in the interface.
    pub variables: Vec<String>,
    /// The channel names in the interface.
    pub channels: Vec<String>,
}

impl Permutation {
    /// Whether the permutation defines `flag`.
    pub fn defines(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The record length the engine uses for a channel record, by its kind.
fn record_len(kind: u32) -> Option<usize> {
    match kind {
        4 => Some(60),
        5 => Some(73),
        _ => None,
    }
}

/// The engine-side constant a generated block starts from: a shipped preamble
/// carrying the header, the engine-variable records and one channel record per
/// kind to clone from. It is the only piece of the block a mod cannot derive.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockTemplate {
    preamble: Vec<u8>,
    /// The engine-variable records, as `[index, value]` pairs.
    records: Vec<(u32, u32)>,
    /// The channel record stream, count word first.
    stream: Vec<u8>,
}

impl BlockTemplate {
    /// Reads a shipped preamble, checking the header/table/stream framing: a
    /// 120-byte header, `count - 8` byte-packed 13-byte records, the stream's
    /// count word, then the records themselves.
    pub fn from_preamble(preamble: &[u8]) -> Result<Self> {
        if preamble.len() < 0x78 {
            bail!("the block template is too small ({} bytes)", preamble.len());
        }
        let count = u32_at(preamble, 12);
        if count < 8 {
            bail!("the block template's record count {count} is too small");
        }
        let records = (count - 8) as usize;
        let table_end = 0x78 + records * 13;
        if table_end + 4 > preamble.len() {
            bail!(
                "the block template's record table runs past the preamble ({} > {})",
                table_end + 4,
                preamble.len()
            );
        }
        let stream_count = u32_at(preamble, table_end) as usize;
        let mut records_data = Vec::new();
        let mut at = 0x78;
        for _ in 0..records {
            records_data.push((u32_at(preamble, at), u32_at(preamble, at + 5)));
            at += 13;
        }
        let mut stream = preamble[table_end..].to_vec();
        // Trim the stream to its records: walk the count and the per-kind
        // lengths so a template with a trailing pad still parses.
        let mut kept = 4;
        at = table_end + 4;
        for _ in 0..stream_count {
            let kind = u32_at(preamble, at + 4);
            let Some(len) = record_len(kind) else {
                bail!("unknown channel record kind {kind}");
            };
            kept += len;
            at += len;
        }
        if at > preamble.len() {
            bail!(
                "the block template's stream runs past the preamble ({} > {})",
                at,
                preamble.len()
            );
        }
        stream.truncate(kept);
        Ok(Self {
            preamble: preamble.to_vec(),
            records: records_data,
            stream,
        })
    }

    /// The group count the template carries, which a generated section repeats.
    pub fn groups(&self) -> u32 {
        u32_at(&self.preamble, 4)
    }

    /// The cbuffer count the template carries.
    pub fn cbuffers(&self) -> u32 {
        u32_at(&self.preamble, 8)
    }

    /// The engine-variable records, which a generated block copies verbatim.
    pub fn records(&self) -> &[(u32, u32)] {
        &self.records
    }

    /// The channel record stream (count word first).
    pub fn stream(&self) -> &[u8] {
        &self.stream
    }

    /// The channel record of the given kind, for a clone to copy.
    pub fn channel_record(&self, kind: u32) -> Option<Vec<u8>> {
        let count = u32_at(&self.stream, 0) as usize;
        let mut at = 4;
        for _ in 0..count {
            let this = u32_at(&self.stream, at + 4);
            let len = record_len(this)?;
            if this == kind {
                return Some(self.stream[at..at + len].to_vec());
            }
            at += len;
        }
        None
    }

    /// The channel names the template's stream carries.
    pub fn channel_names(&self) -> Vec<u32> {
        let count = u32_at(&self.stream, 0) as usize;
        let mut names = Vec::new();
        let mut at = 4;
        for _ in 0..count {
            names.push(u32_at(&self.stream, at));
            let len = record_len(u32_at(&self.stream, at + 4)).unwrap_or(60);
            at += len;
        }
        names
    }
}

/// Builds a block for `channels`: the template's header with the given counts,
/// its engine-variable records, and one channel record per declared channel -
/// the template's record of the matching kind with the name hash substituted.
/// A channel whose name the template already carries keeps its record.
pub fn build_block(
    template: &BlockTemplate,
    channels: &[(String, ChannelDef)],
    groups: u32,
    cbuffers: u32,
) -> Result<Vec<u8>> {
    let mut block = Vec::new();
    // The header: the template's bytes with the three varying words rewritten.
    block.extend_from_slice(&template.preamble[..0x78]);
    let records = template.records.len();
    for (index, value) in &template.records {
        block.extend_from_slice(&index.to_le_bytes());
        block.push(0);
        block.extend_from_slice(&value.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut stream = Vec::new();
    let mut written: Vec<u32> = Vec::new();
    for (name, channel) in channels {
        let hash = u32::from(crate::murmur::Murmur32::hash(name.as_bytes()));
        if let Some(at) = template
            .channel_names()
            .iter()
            .position(|existing| *existing == hash)
        {
            // The template already carries it; copy its record unchanged.
            let mut skip = 4;
            for _ in 0..at {
                skip += record_len(u32_at(template.stream(), skip + 4)).unwrap_or(60);
            }
            let len = record_len(u32_at(template.stream(), skip + 4)).unwrap_or(60);
            stream.extend_from_slice(&template.stream()[skip..skip + len]);
            written.push(hash);
            continue;
        }
        // A new channel: clone the template's record of the same kind. Only the
        // two texture kinds have a decoded record length, and the texture
        // record is the shape the verified channel clone used.
        if channel.kind != ValueType::Texture2D {
            bail!(
                "channel {name} is a {:?} and no block record of that kind is known",
                channel.kind
            );
        }
        let Some(record) = template
            .channel_record(4)
            .or_else(|| template.channel_record(5))
        else {
            bail!("the block template has no channel record to clone");
        };
        let mut clone = record;
        clone[0..4].copy_from_slice(&hash.to_le_bytes());
        stream.extend_from_slice(&clone);
        written.push(hash);
    }
    let mut count_bytes = (written.len() as u32).to_le_bytes().to_vec();
    count_bytes.extend_from_slice(&stream);
    block.extend_from_slice(&count_bytes);
    // The three varying header words: groups, cbuffers, records + 8.
    block[4..8].copy_from_slice(&groups.to_le_bytes());
    block[8..12].copy_from_slice(&cbuffers.to_le_bytes());
    block[12..16].copy_from_slice(&((records as u32) + 8).to_le_bytes());
    Ok(block)
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        // The family a mod would ship.
        channels = {
            vertex_position = {
                type = "float4"
                domain = "vertex"
                required = true
            }
            texture_map = {
                type = "texture2d"
                domain = "pixel"
                required = true
            }
        }

        variables = {
            base_color = {
                type = "vector3"
                domain = "pixel"
                flag = "HAS_BASE_COLOR"
                default = [ 1, 1, 1 ]
            }
            opacity = {
                type = "scalar"
                domain = "pixel"
                flag = "HAS_OPACITY"
            }
            mod_tint = {
                type = "vector4"
                domain = "pixel"
            }
        }

        programs = {
            vs_main = {
                source = "snoopymod/ui.vs.hlsl"
                stage = "vertex"
            }
            ps_main = {
                source = "snoopymod/ui.ps.hlsl"
                stage = "pixel"
            }
        }
    "#;

    #[test]
    fn parses_a_declaration() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        assert_eq!(family.programs.len(), 2);
        assert_eq!(family.programs["vs_main"].stage, "vertex");
        assert_eq!(family.channels.len(), 2);
        assert_eq!(family.channels["texture_map"].kind, ValueType::Texture2D);
        assert_eq!(family.channels["vertex_position"].domain, Domain::Vertex);
        assert!(family.channels["texture_map"].required);
        assert_eq!(family.variables.len(), 3);
        assert_eq!(family.variables["base_color"].default, vec![1.0, 1.0, 1.0]);
        // The types agree with the group data's record sizes.
        assert_eq!(family.channels["vertex_position"].kind.size(), 16);
        assert_eq!(family.variables["mod_tint"].kind.size(), 16);
    }

    #[test]
    fn enumerates_one_group_per_optional_variable_subset() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        assert_eq!(family.flags(), vec!["HAS_BASE_COLOR", "HAS_OPACITY"]);
        // Two optional variables: four interfaces, so four groups.
        assert_eq!(family.group_count(), 4);

        let permutations = family.permutations();
        assert!(permutations[0].flags.is_empty());
        // Without the optional variables only the always-present one is left.
        assert_eq!(permutations[0].variables, vec!["mod_tint"]);
        // The required channels ride along in every permutation.
        assert_eq!(
            permutations[0].channels,
            vec!["texture_map", "vertex_position"]
        );

        assert!(permutations[1].defines("HAS_BASE_COLOR"));
        assert!(!permutations[1].defines("HAS_OPACITY"));
        assert_eq!(permutations[1].variables, vec!["base_color", "mod_tint"]);

        assert!(permutations[2].defines("HAS_OPACITY"));
        assert!(!permutations[2].defines("HAS_BASE_COLOR"));

        assert!(permutations[3].defines("HAS_BASE_COLOR"));
        assert!(permutations[3].defines("HAS_OPACITY"));
        assert_eq!(
            permutations[3].variables,
            vec!["base_color", "mod_tint", "opacity"]
        );
        // The masks are distinct, so the conditions tree can key on them.
        let masks: Vec<u32> = permutations.iter().map(|p| p.mask).collect();
        assert_eq!(masks, vec![0, 1, 2, 3]);
    }

    #[test]
    fn an_optional_channel_follows_its_variable() {
        let text = r#"
            channels = {
                texture_map = {
                    type = "texture2d"
                    required = true
                }
                normal_map = {
                    type = "texture2d"
                    variable = "normal_strength"
                }
            }
            variables = {
                normal_strength = {
                    type = "scalar"
                    flag = "HAS_NORMAL_MAP"
                }
            }
        "#;
        let family = Family::from_sjson(text).expect("parse");
        let permutations = family.permutations();
        assert_eq!(permutations.len(), 2);
        assert_eq!(permutations[0].channels, vec!["texture_map"]);
        assert_eq!(permutations[1].channels, vec!["normal_map", "texture_map"]);
    }

    #[test]
    fn round_trips_through_sjson() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        let text = family.to_sjson().expect("write");
        let again = Family::from_sjson(&text).expect("re-parse");
        assert_eq!(family, again);
    }

    #[test]
    fn an_empty_declaration_makes_one_group() {
        let family = Family::from_sjson("channels = {}").expect("parse");
        assert_eq!(family.group_count(), 1);
        assert_eq!(family.permutations()[0].mask, 0);
    }

    #[test]
    fn rejects_an_unknown_key() {
        let err = Family::from_sjson("programz = {}").expect_err("must fail");
        assert!(err.to_string().contains("programz"), "{err}");
    }

    /// The murmur32 the engine hashes channel names with, pinned against a name
    /// the shipped families carry.
    fn hash(name: &str) -> u32 {
        u32::from(crate::murmur::Murmur32::hash(name.as_bytes()))
    }

    /// A synthetic preamble in the shipped framing: a 120-byte header, one
    /// engine-variable record, then a one-channel stream whose record is the
    /// kind-4 60-byte shape.
    fn template_preamble() -> Vec<u8> {
        let mut block = vec![0u8; 0x78];
        block[4..8].copy_from_slice(&1u32.to_le_bytes());
        block[8..12].copy_from_slice(&2u32.to_le_bytes());
        // One engine record, so the count word is 8 + 1.
        block[12..16].copy_from_slice(&9u32.to_le_bytes());
        // The record table: {index, pad, value, pad}.
        block.extend_from_slice(&7u32.to_le_bytes());
        block.push(0);
        block.extend_from_slice(&4242u32.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        // The stream: its count word, then the channel records.
        block.extend_from_slice(&1u32.to_le_bytes());
        let mut record = vec![0u8; 60];
        record[0..4].copy_from_slice(&hash("texture_map").to_le_bytes());
        record[4..8].copy_from_slice(&4u32.to_le_bytes());
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record[12..16].copy_from_slice(&0x1234u32.to_le_bytes());
        block.extend_from_slice(&record);
        block
    }

    fn texture_channel() -> ChannelDef {
        ChannelDef {
            kind: ValueType::Texture2D,
            domain: Domain::Pixel,
            required: true,
            variable: None,
            semantic: None,
        }
    }

    #[test]
    fn hashes_channel_names_the_way_the_engine_does() {
        assert_eq!(hash("texture_map"), 0xE503152C);
    }

    #[test]
    fn reads_a_template_preamble() {
        let preamble = template_preamble();
        let template = BlockTemplate::from_preamble(&preamble).expect("template");
        assert_eq!(template.groups(), 1);
        assert_eq!(template.cbuffers(), 2);
        assert_eq!(template.records(), &[(7, 4242)]);
        assert_eq!(template.channel_names(), vec![hash("texture_map")]);
        let record = template.channel_record(4).expect("kind 4");
        assert_eq!(record.len(), 60);
        assert_eq!(u32_at(&record, 8), 1);
        // The stream is the count word plus the record.
        assert_eq!(template.stream().len(), 4 + 60);
        assert!(template.channel_record(5).is_none());
    }

    #[test]
    fn rejects_a_malformed_template() {
        assert!(BlockTemplate::from_preamble(&[0u8; 16]).is_err());
        let mut preamble = template_preamble();
        preamble[12..16].copy_from_slice(&4u32.to_le_bytes());
        let err = BlockTemplate::from_preamble(&preamble).expect_err("count too small");
        assert!(err.to_string().contains("too small"), "{err}");
        // A record count that outruns the preamble is caught, not a panic.
        preamble.copy_from_slice(&template_preamble());
        preamble[12..16].copy_from_slice(&900u32.to_le_bytes());
        assert!(BlockTemplate::from_preamble(&preamble).is_err());
    }

    #[test]
    fn builds_a_block_from_a_declaration() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let channels = [
            ("texture_map".to_string(), texture_channel()),
            ("mod_map".to_string(), texture_channel()),
        ];
        let block = build_block(&template, &channels, 4, 3).expect("build");
        // The rebuilt block reads back as a template of its own: one group, three
        // cbuffers, the engine record copied verbatim.
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert_eq!(built.groups(), 4);
        assert_eq!(built.cbuffers(), 3);
        assert_eq!(built.records(), &[(7, 4242)]);
        // Both channels are present, in declaration order, hashed by name.
        assert_eq!(
            built.channel_names(),
            vec![hash("texture_map"), hash("mod_map")]
        );
        // A channel the template already carried keeps its record byte for byte.
        let first = built.channel_record(4).expect("record");
        assert_eq!(u32_at(&first, 0), hash("texture_map"));
        assert_eq!(u32_at(&first, 12), 0x1234);
    }

    #[test]
    fn a_cloned_channel_differs_only_in_its_name() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let channels = [("mod_map".to_string(), texture_channel())];
        let block = build_block(&template, &channels, 1, 2).expect("build");
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert_eq!(built.channel_names(), vec![hash("mod_map")]);
        let record = built.channel_record(4).expect("record");
        assert_eq!(record.len(), 60);
        // The body is the template's, which is what the verified clone did.
        assert_eq!(u32_at(&record, 4), 4);
        assert_eq!(u32_at(&record, 8), 1);
        assert_eq!(u32_at(&record, 12), 0x1234);
    }

    #[test]
    fn a_block_with_no_channels_keeps_an_empty_stream() {
        let template = BlockTemplate::from_preamble(&template_preamble()).expect("template");
        let block = build_block(&template, &[], 1, 2).expect("build");
        assert_eq!(block.len(), 0x78 + 13 + 4);
        let built = BlockTemplate::from_preamble(&block).expect("re-read");
        assert!(built.channel_names().is_empty());
    }
}
