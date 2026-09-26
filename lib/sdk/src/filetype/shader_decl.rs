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





}
