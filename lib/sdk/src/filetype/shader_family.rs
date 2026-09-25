//! The intermediate representation of a shader family: what the emitters need to
//! write a `shader43` section, and nothing else.
//!
//! This module is deliberately *not* a mod-facing file format. A mod describes a
//! family the way the Stingray toolchain does, in a `.shader_node` file; the
//! reader in [`super::shader_node`] turns that into the [`Family`] here, and the
//! emitters turn a [`Family`] into bytes. Anything the mod writes that does not
//! reach the emitters does not belong in these types.
//!
//! Two enumerations live here, and they answer different questions:
//!
//! - [`Family::interfaces`] enumerates the *runtime* interfaces: one per
//!   combination of the family's optional variables, each with the flags it
//!   defines and the variables and channels it exposes. This is what the
//!   conditions tree indexes on when it picks a group's interface.
//! - [`Family::permutations`] enumerates the *compile* permutations: one per
//!   combination of a choice from each [`PermutationSet`], with the macros the
//!   programs are compiled with. The family's group count is this product.
//!
//! Everything the section still needs from the engine - the block header blob,
//! the engine's variable registry, the bindless conventions - is not part of a
//! family; it is carried by the toolchain as a [`BlockTemplate`].

use std::collections::BTreeMap;

/// The stage a channel or variable belongs to. A `vertex` channel is written by
/// the vertex program and interpolated into the pixel program; a `pixel` one is
/// a pixel-program-only value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    /// Written by the vertex stage and read by the pixel stage.
    Vertex,
    /// Pixel stage only.
    #[default]
    Pixel,
}

/// The type of a channel or variable. The sizes match the group data's record
/// sizes: 4, 8, 12 and 16 bytes, and 64 for a 4x4 matrix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueType {
    /// A scalar (4 bytes).
    #[default]
    Float,
    /// A two-component vector (8 bytes).
    Float2,
    /// A three-component vector (12 bytes).
    Float3,
    /// A four-component vector (16 bytes).
    Float4,
    /// A 4x4 matrix (64 bytes).
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

/// The type spellings a `.shader_node` uses, in either of the two dialects the
/// toolchain files mix: HLSL (`float3`) and the material vocabulary
/// (`vector3`).
impl ValueType {
    /// Parses a declared type name, or `None` when the name is not one the
    /// group data can size.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "float" | "scalar" => Self::Float,
            "float2" | "vector2" => Self::Float2,
            "float3" | "vector3" => Self::Float3,
            "float4" | "vector4" => Self::Float4,
            "float4x4" | "matrix" | "float3x3" => Self::Float4x4,
            "texture2d" | "texture" => Self::Texture2D,
            _ => return None,
        })
    }
}

/// A channel: a named value the vertex program hands to the pixel program, or a
/// texture the material binds.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelDef {
    /// The channel's name, which is what the group data hashes.
    pub name: String,
    /// The channel's type.
    pub kind: ValueType,
    /// Which stage produces it.
    pub domain: Domain,
    /// Whether the material must provide it. A required channel is in every
    /// interface; an optional one joins only the interfaces whose mask defines
    /// its gating variable's flag.
    pub required: bool,
    /// The DXBC semantic to bind the channel to, if the declaration names one.
    pub semantic: Option<String>,
    /// The variable whose flag gates this channel, when it is not the channel's
    /// own name.
    pub variable: Option<String>,
    /// The conditions over permutation macros that have to hold for the channel
    /// to exist, as the keys of the declaration's `channels` table it sits under.
    /// Empty means the channel is unconditional.
    pub conditions: Vec<String>,
}

impl Default for ChannelDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ValueType::Float4,
            domain: Domain::Pixel,
            required: true,
            semantic: None,
            variable: None,
            conditions: Vec::new(),
        }
    }
}

/// A material variable: a value the material may set by name, like
/// `dev_wireframe_color` in the shipped families. A variable with a flag is
/// optional - it belongs to the interface only where that flag is defined.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariableDef {
    /// The variable's type.
    pub kind: ValueType,
    /// Which stage reads it.
    pub domain: Domain,
    /// The permutation flag that makes the variable part of the interface. A
    /// variable without a flag is always present.
    pub flag: Option<String>,
    /// The default value written into the section's default data.
    pub default: Vec<f32>,
}

/// One program of a stage, given by the source the toolchain compiles.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramDef {
    /// The HLSL source, relative to the mod root.
    pub source: String,
    /// `vertex` or `pixel`.
    pub stage: String,
}

/// A set of mutually exclusive compile choices, from a `permutation_sets` entry
/// of a `.shader_node` file. Each choice is taken when its `if` expression holds
/// - or always, for the `default` choice.
#[derive(Clone, Debug, PartialEq)]
pub struct PermutationSet {
    /// The set's name, as the declaration spells it.
    pub name: String,
    /// The set's choices, in declaration order.
    pub choices: Vec<Choice>,
}

/// One choice of a [`PermutationSet`].
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// The `if` expression that selects the choice, if it has one.
    pub condition: Option<String>,
    /// The macros the choice defines.
    pub macros: Vec<String>,
    /// The stages those macros apply to. Empty means every stage.
    pub stages: Vec<String>,
    /// Whether this is the set's `default` choice, taken when no `if` holds.
    pub is_default: bool,
}

/// A whole family: what the emitters need, gathered from a `.shader_node` file
/// and its companion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Family {
    /// The programs to compile, keyed by entry point name.
    pub programs: BTreeMap<String, ProgramDef>,
    /// The channels the family exchanges, in the order the declaration lists
    /// them: the order of the conditions table decides the conditions section.
    pub channels: Vec<ChannelDef>,
    /// The material variables the family accepts, keyed by name. A declaration's
    /// own inputs are keyed by uuid, so this is in name order instead.
    pub variables: BTreeMap<String, VariableDef>,
    /// The compile-time permutation sets, in name order.
    pub permutation_sets: Vec<PermutationSet>,
}

impl Family {
    /// The channel of that name, when the family declares it.
    pub fn channel(&self, name: &str) -> Option<&ChannelDef> {
        self.channels.iter().find(|channel| channel.name == name)
    }

    /// The flags the family's variables can gate on, in name order.
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

    /// Enumerates the runtime interfaces: one per combination of the optional
    /// variables, in the order the flags are named (bit 0 first). Each carries
    /// the flags it defines, the variables and channels it exposes, and the mask
    /// the conditions tree keys on.
    pub fn interfaces(&self) -> Vec<Interface> {
        let flags = self.flags();
        let count = 1u32 << flags.len();
        (0..count)
            .map(|mask| Interface {
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
                    .filter(|channel| {
                        channel.required
                            || self
                                .gating_variable(&channel.name, channel)
                                .and_then(|variable| variable.flag.as_ref())
                                .is_some_and(|flag| self.enabled(flag, mask, &flags))
                    })
                    .map(|channel| channel.name.clone())
                    .collect(),
            })
            .collect()
    }

    /// Enumerates the compile permutations: one per combination of a choice from
    /// each permutation set, the first set varying slowest. Each carries the
    /// choices that make it up and the macros the programs compile with. A
    /// family with no sets has the one empty permutation.
    pub fn permutations(&self) -> Vec<Permutation> {
        let mut permutations = vec![Permutation {
            choices: Vec::new(),
            macros: Vec::new(),
        }];
        for set in &self.permutation_sets {
            let mut next = Vec::with_capacity(permutations.len() * set.choices.len());
            for permutation in permutations {
                for (index, choice) in set.choices.iter().enumerate() {
                    let mut permutation = permutation.clone();
                    permutation.choices.push((set.name.clone(), index));
                    permutation.macros.extend(choice.macros.iter().cloned());
                    next.push(permutation);
                }
            }
            permutations = next;
        }
        permutations
    }

    /// The number of groups the family generates: the product of the sets'
    /// choice counts, or one when the family declares no sets.
    pub fn group_count(&self) -> usize {
        self.permutations().len()
    }

    /// The number of runtime interfaces: one per combination of the optional
    /// variables.
    pub fn interface_count(&self) -> usize {
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

/// One runtime interface: the flags a material's inputs enable and the variables
/// and channels they expose. `mask` is the bit set over [`Family::flags`] and is
/// what the conditions tree indexes on.
#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    /// The bit set over [`Family::flags`].
    pub mask: u32,
    /// The permutation flags, in name order.
    pub flags: Vec<String>,
    /// The variable names in the interface.
    pub variables: Vec<String>,
    /// The channel names in the interface.
    pub channels: Vec<String>,
}

impl Interface {
    /// Whether the interface defines `flag`.
    pub fn defines(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// One compile permutation: the choice it takes from each permutation set and
/// the macros those choices define.
#[derive(Clone, Debug, PartialEq)]
pub struct Permutation {
    /// The choice taken from each set, as `(set name, choice index)`.
    pub choices: Vec<(String, usize)>,
    /// The macros the permutation defines.
    pub macros: Vec<String>,
}

impl Permutation {
    /// Whether the permutation defines `macro`.
    pub fn defines(&self, macro_name: &str) -> bool {
        self.macros.iter().any(|m| m == macro_name)
    }
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
    pub fn from_preamble(preamble: &[u8]) -> Result<Self, color_eyre::Report> {
        use color_eyre::eyre::bail;
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
        // Trim the stream to its records: walk the count and the per-kind lengths
        // so a template with a trailing pad still parses.
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
        let mut stream = preamble[table_end..].to_vec();
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
) -> Result<Vec<u8>, color_eyre::Report> {
    use color_eyre::eyre::bail;
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
        // two texture kinds have a decoded record length, and the texture record
        // is the shape the verified channel clone used.
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

    /// A family as the `.shader_node` reader hands it over: two required
    /// channels and one gated by an optional variable.
    fn sample() -> Family {
        let mut family = Family::default();
        for name in ["texture_map", "vertex_position"] {
            family.channels.push(ChannelDef {
                name: name.to_string(),
                kind: ValueType::Texture2D,
                required: true,
                ..ChannelDef::default()
            });
        }
        family.channels.push(ChannelDef {
            name: "normal_map".to_string(),
            kind: ValueType::Texture2D,
            required: false,
            variable: Some("normal_strength".to_string()),
            ..ChannelDef::default()
        });
        for (name, flag) in [
            ("base_color", Some("HAS_BASE_COLOR")),
            ("opacity", Some("HAS_OPACITY")),
        ] {
            family.variables.insert(
                name.to_string(),
                VariableDef {
                    kind: ValueType::Float3,
                    flag: flag.map(String::from),
                    default: vec![1.0, 1.0, 1.0],
                    ..VariableDef::default()
                },
            );
        }
        family.variables.insert(
            "normal_strength".to_string(),
            VariableDef {
                kind: ValueType::Float,
                flag: Some("HAS_NORMAL_MAP".to_string()),
                ..VariableDef::default()
            },
        );
        family.variables.insert(
            "mod_tint".to_string(),
            VariableDef {
                kind: ValueType::Float4,
                ..VariableDef::default()
            },
        );
        family
    }

    /// The texture channel of the sample family.
    fn texture_channel() -> ChannelDef {
        ChannelDef {
            name: "texture_map".to_string(),
            kind: ValueType::Texture2D,
            required: true,
            ..ChannelDef::default()
        }
    }

    #[test]
    fn parses_either_type_spelling() {
        assert_eq!(ValueType::parse("vector3"), Some(ValueType::Float3));
        assert_eq!(ValueType::parse("float3"), Some(ValueType::Float3));
        assert_eq!(ValueType::parse("scalar"), Some(ValueType::Float));
        assert_eq!(ValueType::parse("texture2d"), Some(ValueType::Texture2D));
        assert_eq!(ValueType::parse("texture_cube"), None);
    }

    #[test]
    fn one_interface_per_optional_variable_subset() {
        let family = sample();
        assert_eq!(
            family.flags(),
            vec!["HAS_BASE_COLOR", "HAS_NORMAL_MAP", "HAS_OPACITY"]
        );
        // Three optional variables: eight interfaces, bit 0 first.
        assert_eq!(family.interface_count(), 8);

        let interfaces = family.interfaces();
        assert!(interfaces[0].flags.is_empty());
        // Without the optional variables only the always-present one is left.
        assert_eq!(interfaces[0].variables, vec!["mod_tint"]);
        // The required channels ride along in every interface.
        assert_eq!(
            interfaces[0].channels,
            vec!["texture_map", "vertex_position"]
        );
        // A gated channel joins when its gating variable's flag is set:
        // HAS_NORMAL_MAP is bit 1, so interfaces 2 and 3 have it and 1 does not.
        assert!(!interfaces[1].channels.contains(&"normal_map".to_string()));
        assert_eq!(interfaces[1].channels.len(), 2);
        assert!(interfaces[2].channels.contains(&"normal_map".to_string()));
        assert!(interfaces[3].channels.contains(&"normal_map".to_string()));

        assert!(interfaces[1].defines("HAS_BASE_COLOR"));
        assert!(!interfaces[1].defines("HAS_OPACITY"));
        assert_eq!(interfaces[1].variables, vec!["base_color", "mod_tint"]);
        assert_eq!(interfaces[7].variables.len(), 4);
        // The masks are distinct, so the conditions tree can key on them.
        let masks: Vec<u32> = interfaces.iter().map(|i| i.mask).collect();
        assert_eq!(masks, (0..8).collect::<Vec<u32>>());
    }

    #[test]
    fn one_permutation_per_choice_combination() {
        let mut family = sample();
        assert_eq!(family.group_count(), 1);
        family.permutation_sets = vec![
            PermutationSet {
                name: "vertex_modifiers".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("num_skin_weights() == 4".to_string()),
                        macros: vec!["SKINNED_4WEIGHTS".to_string()],
                        stages: vec!["vertex".to_string()],
                        is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        is_default: true,
                    },
                ],
            },
            PermutationSet {
                name: "instanced_modifiers".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("instanced()".to_string()),
                        macros: vec!["INSTANCED".to_string()],
                        stages: vec![],
                        is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        is_default: true,
                    },
                ],
            },
        ];
        // Two sets of two choices: four permutations, the first set slowest.
        assert_eq!(family.group_count(), 4);
        let permutations = family.permutations();
        assert_eq!(
            permutations[0].choices,
            vec![
                ("vertex_modifiers".to_string(), 0),
                ("instanced_modifiers".to_string(), 0)
            ]
        );
        assert!(permutations[0].defines("SKINNED_4WEIGHTS"));
        assert!(permutations[0].defines("INSTANCED"));
        // The first set varies slowest, so taking the second choice of the
        // second set leaves the first set's macros alone.
        assert_eq!(permutations[1].macros, vec!["SKINNED_4WEIGHTS".to_string()]);
        assert_eq!(permutations[2].macros, vec!["INSTANCED".to_string()]);
        assert!(permutations[3].macros.is_empty());
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
