//! The declaration's parts: the types a `.shader_node` normalizes to.
//!
//! This module is not a file format. A mod describes a shader the way the
//! Stingray toolchain does, in a `.shader_node` file; the reader in
//! [`super::shader_node`] parses it and fills the normalized parts on the
//! [`super::shader_node::ShaderNode`] itself. These are those parts: the
//! interface ([`VariableDef`], [`ChannelDef`]), the permutation space
//! ([`PermutationSet`], [`Choice`], [`Permutation`], [`ShaderContext`],
//! [`Pass`]) and the programs. Each is its own type so it can be read and used
//! on its own.
//!
//! [`BlockTemplate`] still lives here while it is split out: it is the carried
//! engine preamble, not part of a declaration.

use std::collections::BTreeMap;

use color_eyre::eyre::{Context, Result};

use super::condition::{Condition, Defines};

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

/// The macros a choice or a pass defines, in the three forms the declarations
/// write them.
#[derive(Clone, Debug, PartialEq)]
pub enum Define {
    /// `["SKINNED_4WEIGHTS"]`
    Macros(Vec<String>),
    /// `"SKINNED_4WEIGHTS"`, a list of one.
    Macro(String),
    /// `{ "macros": [...], stages: [...] }`
    Table(DefineTable),
}

impl Define {
    /// The macros defined.
    pub fn macros(&self) -> &[String] {
        match self {
            Self::Macros(macros) => macros,
            Self::Macro(macro_name) => std::slice::from_ref(macro_name),
            Self::Table(table) => &table.macros,
        }
    }

    /// The stages the macros apply to. Empty means every stage.
    pub fn stages(&self) -> &[String] {
        match self {
            Self::Macros(_) | Self::Macro(_) => &[],
            Self::Table(table) => &table.stages,
        }
    }
}

impl Default for Define {
    fn default() -> Self {
        Self::Macros(Vec::new())
    }
}

/// The table form of a [`Define`].
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct DefineTable {
    /// The macros to define.
    #[serde(default, alias = "macro")]
    pub macros: Vec<String>,
    /// The stages the macros apply to. Empty means every stage.
    #[serde(default, alias = "stage")]
    pub stages: Vec<String>,
}

/// A shader context: one named set of passes the declaration compiles and draws. Named
/// for the section's contexts, and not for anything to do with errors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderContext {
    /// The context's name, which is what the engine asks for.
    pub name: String,
    /// How the passes are sorted: `immediate` or `deferred`.
    pub sort_mode: Option<String>,
    /// When the context compiles, and over which permutation sets.
    pub compile_with: Vec<CompileWith>,
    /// The passes, in declaration order, chosen by condition.
    pub passes: Vec<PassEntry>,
}

/// One `compile_with` entry: when the context compiles, and over which
/// permutation sets. An empty `permute_with` means every set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompileWith {
    /// The condition, if the entry has one.
    pub condition: Option<String>,
    /// The names of the permutation sets to permute over.
    pub permute_with: Vec<String>,
}

impl CompileWith {
    /// Whether the context compiles under `defines`, or `None` when the
    /// condition reaches a fact only the engine can answer.
    pub fn holds(&self, defines: &Defines) -> Result<Option<bool>> {
        let Some(text) = &self.condition else {
            return Ok(Some(true));
        };
        let condition = Condition::parse(text)
            .wrap_err_with(|| format!("an unparsable compile_with condition {text:?}"))?;
        Ok(condition.holds(defines))
    }
}

/// An entry of a context's `passes`: a pass, or a branch that chooses between
/// two lists of them.
#[derive(Clone, Debug, PartialEq)]
pub enum PassEntry {
    /// One pass, drawn as it is.
    Pass(Pass),
    /// `if <condition> then [...] else [...]`, with the branches themselves
    /// pass entries.
    Branch {
        /// The condition, as it is written.
        condition: String,
        /// The entries to use when the condition holds.
        then: Vec<PassEntry>,
        /// The entries to use when it does not.
        otherwise: Vec<PassEntry>,
    },
}

impl PassEntry {
    /// Collects the passes that may apply under `defines`.
    ///
    /// A branch whose condition the defines cannot answer contributes *both* of
    /// its sides, because the engine decides it per material at runtime: a pass
    /// drawn when it should not be is wasted, a pass missing when it was needed
    /// is a hole.
    pub fn select<'a>(&'a self, defines: &Defines, out: &mut Vec<&'a Pass>) -> Result<()> {
        match self {
            Self::Pass(pass) => out.push(pass),
            Self::Branch {
                condition,
                then,
                otherwise,
            } => {
                let condition = Condition::parse(condition)
                    .wrap_err_with(|| format!("an unparsable pass condition {condition:?}"))?;
                let selected = match condition.holds(defines) {
                    Some(true) => then,
                    Some(false) => otherwise,
                    None => {
                        collect_passes(then, defines, out)?;
                        otherwise
                    }
                };
                collect_passes(selected, defines, out)?;
            }
        }
        Ok(())
    }
}

/// Collects the passes of a list of entries.
fn collect_passes<'a>(
    entries: &'a [PassEntry],
    defines: &Defines,
    out: &mut Vec<&'a Pass>,
) -> Result<()> {
    for entry in entries {
        entry.select(defines, out)?;
    }
    Ok(())
}

/// One pass: a draw of a code block, with the macros it adds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pass {
    /// The layer it is drawn on, when it names one.
    pub layer: Option<String>,
    /// The code block the pass compiles - the HLSL entry point pair.
    pub code_block: String,
    /// The macros the pass defines.
    pub defines: Define,
    /// The render state the pass draws with.
    pub render_state: Option<String>,
    /// The key the engine sorts variants by, when the pass names one.
    pub branch_key: Option<String>,
}

impl Pass {
    /// The macros the pass defines.
    pub fn macros(&self) -> &[String] {
        self.defines.macros()
    }
}

impl ShaderContext {
    /// The passes that may apply under `defines`.
    pub fn passes_of(&self, defines: &Defines) -> Result<Vec<&Pass>> {
        let mut passes = Vec::new();
        collect_passes(&self.passes, defines, &mut passes)?;
        Ok(passes)
    }
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
    /// The permutation sets this choice delegates to, if it names any. The
    /// enumeration expands them under this choice, which is how a set is built
    /// out of other sets.
    pub permute_with: Vec<String>,
    /// Whether this is the set's `default` choice, taken when no `if` holds.
    pub is_default: bool,
}

/// Whether a stage-limited macro applies to a channel of `domain`. An empty
/// stage list applies everywhere; a named stage that is not the channel's does
/// not apply.
pub(crate) fn stage_applies(stages: &[String], domain: Domain) -> bool {
    stages.is_empty()
        || stages.iter().any(|stage| match (stage.as_str(), domain) {
            ("vertex", Domain::Vertex) => true,
            ("pixel", Domain::Pixel) => true,
            _ => false,
        })
}

/// One runtime interface: the flags a material's inputs enable and the variables
/// and channels they expose. `mask` is the bit set over the declaration's flags
/// and is what the conditions tree indexes on.
#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    /// The bit set over the declaration's flags.
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Permutation {
    /// The choice taken from each set, as `(set name, choice index)`.
    pub choices: Vec<(String, usize)>,
    /// The macros the permutation defines.
    pub macros: Vec<String>,
    /// The stages each macro applies to, for the macros a choice limited to some
    /// stages. A macro absent here applies to every stage.
    pub macro_stages: BTreeMap<String, Vec<String>>,
}

impl Permutation {
    /// Whether the permutation defines `macro`.
    pub fn defines(&self, macro_name: &str) -> bool {
        self.macros.iter().any(|m| m == macro_name)
    }

    /// Adds one choice's macros, remembering the stages each applies to.
    pub fn add_defines(&mut self, macros: &[String], stages: &[String]) {
        for name in macros {
            if !self.macros.contains(name) {
                self.macros.push(name.clone());
            }
            if !stages.is_empty() {
                self.macro_stages
                    .entry(name.clone())
                    .or_insert_with(|| stages.to_vec());
            }
        }
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
    use crate::filetype::shader_node::ShaderNode;

    /// A declaration as the `.shader_node` reader hands it over: two required
    /// channels and one gated by an optional variable.
    fn sample() -> ShaderNode {
        let mut node = ShaderNode::default();
        for name in ["texture_map", "vertex_position"] {
            node.channels.push(ChannelDef {
                name: name.to_string(),
                kind: ValueType::Texture2D,
                required: true,
                ..ChannelDef::default()
            });
        }
        node.channels.push(ChannelDef {
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
            node.variables.insert(
                name.to_string(),
                VariableDef {
                    kind: ValueType::Float3,
                    flag: flag.map(String::from),
                    default: vec![1.0, 1.0, 1.0],
                    ..VariableDef::default()
                },
            );
        }
        node.variables.insert(
            "normal_strength".to_string(),
            VariableDef {
                kind: ValueType::Float,
                flag: Some("HAS_NORMAL_MAP".to_string()),
                ..VariableDef::default()
            },
        );
        node.variables.insert(
            "mod_tint".to_string(),
            VariableDef {
                kind: ValueType::Float4,
                ..VariableDef::default()
            },
        );
        node
    }

    /// The texture channel of the sample declaration.
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
    fn an_interface_per_declared_input() {
        let node = sample();
        assert_eq!(
            node.flags(),
            vec!["HAS_BASE_COLOR", "HAS_NORMAL_MAP", "HAS_OPACITY"]
        );

        // A material that declares no optional input gets the always-present
        // variable and the required channels.
        let bare = node.interface(&[]);
        assert_eq!(bare.mask, 0);
        assert!(bare.flags.is_empty());
        assert_eq!(bare.variables, vec!["mod_tint"]);
        assert_eq!(bare.channels, vec!["texture_map", "vertex_position"]);

        // One input enables its flag, and with it the variable and the channel
        // that follows it.
        let base_color = node.interface(&["base_color".to_string()]);
        assert_eq!(base_color.mask, 0b001);
        assert!(base_color.defines("HAS_BASE_COLOR"));
        assert!(!base_color.defines("HAS_OPACITY"));
        assert_eq!(base_color.variables, vec!["base_color", "mod_tint"]);
        assert!(!base_color.channels.contains(&"normal_map".to_string()));

        // The gating variable's flag is bit 1, and it brings the channel with it.
        let normal = node.interface(&["normal_strength".to_string()]);
        assert_eq!(normal.mask, 0b010);
        assert!(normal.channels.contains(&"normal_map".to_string()));

        // Every optional input at once.
        let all = node.interface(&[
            "base_color".to_string(),
            "normal_strength".to_string(),
            "opacity".to_string(),
        ]);
        assert_eq!(all.mask, 0b111);
        assert_eq!(all.variables.len(), 4);

        // A mask gives the same answer as the names behind it.
        assert_eq!(node.interface_of(0b111), all);

        // An input the declaration does not declare is ignored, the way the engine
        // ignores a name it cannot bind.
        let unknown = node.interface(&["not_a_variable".to_string(), "opacity".to_string()]);
        assert_eq!(unknown.mask, 0b100);
    }

    #[test]
    fn one_permutation_per_choice_combination() {
        let mut node = sample();
        assert_eq!(node.group_count(), 1);
        node.permutation_sets = vec![
            PermutationSet {
                name: "vertex_modifiers".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("num_skin_weights() == 4".to_string()),
                        macros: vec!["SKINNED_4WEIGHTS".to_string()],
                        stages: vec!["vertex".to_string()],
                        permute_with: Vec::new(),
                        is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        permute_with: Vec::new(),
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
                        permute_with: Vec::new(),
                        is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        permute_with: Vec::new(),
                        is_default: true,
                    },
                ],
            },
        ];
        // Two sets of two choices: four permutations, the first set slowest.
        assert_eq!(node.group_count(), 4);
        let permutations = node.permutations();
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

    #[test]
    fn a_choice_permutes_over_another_set() {
        // The real files build sets out of sets: a choice names another set and
        // the enumeration expands it under the choice, so the referenced set is
        // not a second root and the product is not squared.
        let mut node = sample();
        node.permutation_sets = vec![
            PermutationSet {
                name: "inner".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("defined(A)".to_string()),
                        macros: vec!["A".to_string()],
                        stages: vec![],
                        permute_with: Vec::new(),
                        is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        permute_with: Vec::new(),
                        is_default: true,
                    },
                ],
            },
            PermutationSet {
                name: "outer".to_string(),
                choices: vec![Choice {
                    condition: None,
                    macros: vec![],
                    stages: vec![],
                    permute_with: vec!["inner".to_string()],
                    is_default: true,
                }],
            },
        ];
        let permutations = node.permutations();
        assert_eq!(
            permutations.len(),
            2,
            "outer expands inner once instead of being a second root"
        );
        assert_eq!(
            permutations[0].choices,
            vec![
                ("outer".to_string(), 0),
                ("inner".to_string(), 0),
            ]
        );
        assert!(permutations[0].defines("A"));
        assert!(permutations[1].macros.is_empty());
    }

    #[test]
    fn a_stage_limited_macro_does_not_decide_another_stage() {
        // SKINNED_4WEIGHTS is a vertex macro: a pixel channel gated on it must
        // not be included by a permutation that defines it for the vertex stage
        // only.
        let mut node = sample();
        node.permutation_sets = vec![PermutationSet {
            name: "vertex_modifiers".to_string(),
            choices: vec![Choice {
                condition: None,
                macros: vec!["SKINNED_4WEIGHTS".to_string()],
                stages: vec!["vertex".to_string()],
                permute_with: Vec::new(),
                is_default: true,
            }],
        }];
        node.channels.push(ChannelDef {
            name: "skinned".to_string(),
            kind: ValueType::Float,
            domain: Domain::Pixel,
            required: false,
            conditions: vec!["defined(SKINNED_4WEIGHTS)".to_string()],
            ..ChannelDef::default()
        });
        let permutation = node.permutations().remove(0);
        let names: Vec<String> = node.channel_names_of(&permutation).expect("names");
        assert!(
            !names.iter().any(|name| name == "skinned"),
            "a vertex-only macro does not apply to a pixel channel"
        );
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
    fn a_declaration_writes_one_default_context_for_the_single_group_path() {
        // The bridge from the declaration to the section. A declaration may name
        // shadow_caster and material_transfer too, but their query ids are not
        // derivable yet, so the one-group path writes the only context whose id is
        // known: default, pointing at the group data's hash. A multi-group
        // declaration against carried group data fails the section's query/group
        // count check rather than writing placeholder ids.
        let node = ShaderNode {
            contexts: vec![
                ShaderContext {
                    name: "default".to_string(),
                    ..Default::default()
                },
                ShaderContext {
                    name: "shadow_caster".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let records = node.context_records(0x8BE2_82AA);
        assert_eq!(records.len(), 1, "only the default context is written");
        assert_eq!(
            records[0].queries[0].id, 0x8BE2_82AA,
            "the query is the group data's hash"
        );
        assert_eq!(
            records[0].queries[0].conditions,
            crate::filetype::shader::NO_CONDITIONS
        );
        assert_eq!(records[0].len(), 20, "12 bytes plus one query");
        assert_eq!(
            records[0].name,
            crate::murmur::Murmur32::hash("default".as_bytes()).into()
        );
    }

    #[test]
    fn a_group_has_the_channels_its_conditions_allow() {
        let mut node = sample();
        // Two channels under a condition each, as a real declaration writes
        // them: one on a macro a set defines, one on a macro nothing defines.
        node.channels.push(ChannelDef {
            name: "tsm0".to_string(),
            kind: ValueType::Float3,
            required: false,
            conditions: vec!["defined(NEEDS_TANGENT_SPACE)".to_string()],
            ..ChannelDef::default()
        });
        node.channels.push(ChannelDef {
            name: "pixel_depth".to_string(),
            kind: ValueType::Float,
            required: false,
            conditions: vec!["defined(NEEDS_PIXEL_DEPTH)".to_string()],
            ..ChannelDef::default()
        });
        // A condition that reaches an engine query: no group can say.
        node.channels.push(ChannelDef {
            name: "skinned".to_string(),
            kind: ValueType::Float,
            required: false,
            conditions: vec!["num_skin_weights() == 4".to_string()],
            ..ChannelDef::default()
        });
        // Two conditions, both of which have to hold.
        node.channels.push(ChannelDef {
            name: "uv".to_string(),
            kind: ValueType::Float2,
            required: false,
            conditions: vec![
                "defined(NEEDS_UV_SCALE)".to_string(),
                "!defined(NEEDS_UV_ANIMATION)".to_string(),
            ],
            ..ChannelDef::default()
        });

        // A declaration with no sets has the one group with no macros, so only the
        // channels no condition gates are in it. A channel an *optional input's*
        // flag gates is here too: that flag is a runtime thing, and the group is
        // a compile permutation. The interface is where the flag applies.
        let groups = node.permutations();
        assert_eq!(groups.len(), 1);
        let names = node.channel_names_of(&groups[0]).expect("names");
        assert_eq!(names, vec!["texture_map", "vertex_position", "normal_map"]);

        // Now a set that defines the two macros one of the channels needs.
        node.permutation_sets = vec![PermutationSet {
            name: "passes".to_string(),
            choices: vec![
                Choice {
                    condition: Some("defined(PASS)".to_string()),
                    macros: vec![
                        "NEEDS_TANGENT_SPACE".to_string(),
                        "NEEDS_UV_SCALE".to_string(),
                    ],
                    stages: vec![],
                    permute_with: Vec::new(),
                    is_default: false,
                },
                Choice {
                    condition: None,
                    macros: vec![],
                    stages: vec![],
                    permute_with: Vec::new(),
                    is_default: true,
                },
            ],
        }];
        let permutations = node.permutations();
        assert_eq!(permutations.len(), 2);

        let with_macros = node.channel_names_of(&permutations[0]).expect("names");
        assert_eq!(
            with_macros,
            vec!["texture_map", "vertex_position", "normal_map", "tsm0", "uv"],
            "the two unconditional channels, the tangent basis, and the uv pair"
        );

        // The other group defines nothing, so the conditional channels drop out.
        let without = node.channel_names_of(&permutations[1]).expect("names");
        assert_eq!(
            without,
            vec!["texture_map", "vertex_position", "normal_map"]
        );
    }

    #[test]
    fn an_unparsable_condition_is_reported() {
        let mut node = sample();
        node.channels.push(ChannelDef {
            name: "broken".to_string(),
            required: false,
            conditions: vec!["defined(".to_string()],
            ..ChannelDef::default()
        });
        let permutations = node.permutations();
        let err = node
            .channel_names_of(&permutations[0])
            .expect_err("unparsable");
        assert!(err.to_string().contains("broken"), "{err}");
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
