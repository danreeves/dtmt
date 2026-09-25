//! A reader for the Stingray `.shader_node` declaration: the source-side
//! description of a shader family that mods already ship.
//!
//! The reader is deliberately partial. It takes `inputs`, `channels` and
//! `permutation_sets` and ignores the rest (`shader_contexts`, `render_state`,
//! `sampler_state`, `options`, ...), producing the [`Family`] the emitters
//! consume. What each key means here:
//!
//! - `inputs` are the material variables, keyed by a uuid and named by `name`.
//!   `is_required` says whether the material must always supply one; an optional
//!   input's type carries the permutation flags that enable it, as
//!   `type = { vector3: ["HAS_BASE_COLOR"] }`.
//! - `channels` are the values the stages exchange, keyed by name, with a `type`,
//!   an optional `semantic` and a `domain` or `domains`.
//! - `permutation_sets` are the compile-time choices: each set lists its choices
//!   as `{ if: "...", define: { "macros": [...], stages: [...] } }`, or
//!   `{ default = true }` for the fallback.
//!
//! The files are read with the vendored SJSON crate, which accepts the
//! toolchain's dialect: both `key = value` and `key: value` separate, and
//! entries do not have to be on their own lines.

use std::collections::BTreeMap;

use color_eyre::eyre;
use color_eyre::eyre::{Result, bail};
use serde::Deserialize;

use super::condition::Defines;
use super::shader_family::{
    ChannelDef, Choice, CompileWith, Define, DefineTable, Domain, Family, Pass, PassEntry,
    PermutationSet, ShaderContext, ValueType, VariableDef,
};

/// A parsed `.shader_node` file, as far as this reader cares: the three sections
/// it takes, keyed by the names the file uses. Every other key is ignored, so a
/// file this reader has not caught up with still parses.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderNode {
    /// The material variables, keyed by their uuid.
    #[serde(default)]
    pub inputs: BTreeMap<String, Input>,
    /// The channels the stages exchange, keyed by the expression over
    /// permutation macros that has to hold for them to exist. A key of
    /// `Channels::One` names a channel that always exists.
    #[serde(default)]
    pub channels: BTreeMap<String, Channels>,
    /// The compile-time permutation sets, keyed by name.
    #[serde(default)]
    pub permutation_sets: BTreeMap<String, Vec<ChoiceEntry>>,
    /// The shader contexts, keyed by name: what the family compiles and draws.
    #[serde(default)]
    pub shader_contexts: BTreeMap<String, NodeContext>,
}

/// The value of one entry of the `channels` table. The table nests: a condition
/// gates a set of channels, and a set can hold further conditions, so a channel
/// can sit under a path of them.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Channels {
    /// A set, keyed by another condition or by a channel name. A channel's `type`
    /// is a string, which is what tells a channel apart from a set.
    Set(BTreeMap<String, Channels>),
    /// One channel, named by its own key in the table.
    One(Channel),
}

/// One `inputs` entry: a material variable.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Input {
    /// The variable's name. This is what the material sets and what the group
    /// data hashes, not the uuid it is keyed by.
    #[serde(default)]
    pub name: String,
    /// Whether the material must supply the variable in every permutation.
    #[serde(default, alias = "required")]
    pub is_required: bool,
    /// The type, and the permutation flags that enable the variable when it is
    /// optional.
    #[serde(rename = "type", default)]
    pub kind: InputType,
    /// Which stage reads the variable.
    #[serde(default)]
    pub domain: Option<String>,
}

/// One `channels` entry.
#[derive(Clone, Debug, Deserialize)]
pub struct Channel {
    /// The channel's type. This is what tells a channel apart from a set of
    /// channels, so it is the one field a channel cannot leave out.
    #[serde(rename = "type")]
    pub kind: String,
    /// The DXBC semantic to bind the channel to.
    #[serde(default)]
    pub semantic: Option<String>,
    /// Which stage produces it. A channel in both domains is written by the
    /// vertex stage and interpolated.
    #[serde(default)]
    pub domain: Option<String>,
    /// The stages the channel belongs to, when the file lists them instead.
    #[serde(default)]
    pub domains: Vec<String>,
}

/// One `permutation_sets` choice.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ChoiceEntry {
    /// The expression that selects this choice.
    #[serde(rename = "if", default, alias = "condition")]
    pub condition: Option<String>,
    /// The macros the choice defines.
    #[serde(default, alias = "defines")]
    pub define: Option<DefinesValue>,
    /// Whether this is the set's default choice, written default = true.
    #[serde(rename = "default", default)]
    pub is_default: Option<bool>,
}

/// The `defines` of a choice or a pass, in the three forms the declarations
/// write them: a list of macros, a bare name, or a table when the macros are
/// limited to some stages.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum DefinesValue {
    /// `defines: ["SKINNED_4WEIGHTS", "MOTION_BLUR"]`
    List(Vec<String>),
    /// `defines: "MOTION_BLUR"`
    Name(String),
    /// `defines: { "macros": [...], stages: [...] }`
    Table(DefineTable),
}

impl From<DefinesValue> for Define {
    fn from(defines: DefinesValue) -> Self {
        match defines {
            DefinesValue::List(macros) => Self::Macros(macros),
            DefinesValue::Name(name) => Self::Macro(name),
            DefinesValue::Table(table) => Self::Table(DefineTable {
                macros: table.macros,
                stages: table.stages,
            }),
        }
    }
}

impl From<Option<DefinesValue>> for Define {
    fn from(defines: Option<DefinesValue>) -> Self {
        defines.map(Self::from).unwrap_or_default()
    }
}

/// One `shader_contexts` entry: a named set of passes.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeContext {
    /// How the passes are sorted.
    #[serde(default)]
    pub passes_sort_mode: Option<String>,
    /// When the context compiles, and over which permutation sets.
    #[serde(default)]
    pub compile_with: Vec<CompileWithEntry>,
    /// The passes, in declaration order.
    #[serde(default)]
    pub passes: Vec<PassEntryValue>,
}

/// One `compile_with` entry.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CompileWithEntry {
    /// When the context compiles.
    #[serde(rename = "if", default, alias = "condition")]
    pub condition: Option<String>,
    /// The permutation sets to permute over: one name or a list of them, and
    /// nothing at all for every set.
    #[serde(default, alias = "permute")]
    pub permute_with: Option<PermuteWith>,
}

/// The `permute_with` of a `compile_with` entry. The toolchain writes a set
/// name, a list of them, or a list of entries that each name one again - the
/// last so that a `permute_with` block can be commented and extended.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PermuteWith {
    /// One entry: a name, or a wrapper naming one again.
    Entry(PermuteEntry),
    /// A list of entries.
    List(Vec<PermuteEntry>),
}

impl Default for PermuteWith {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

/// One entry of a `permute_with`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PermuteEntry {
    /// `permute_with: "default"`
    Name(String),
    /// `permute_with: { permute_with: "default" }`
    Nested {
        /// The name, or names, again.
        #[serde(default, rename = "permute_with")]
        permute_with: Box<PermuteWith>,
    },
}

impl PermuteWith {
    /// The set names, flattened out of whatever nesting the entry used.
    pub fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect(&mut names);
        names
    }

    fn collect(&self, names: &mut Vec<String>) {
        match self {
            Self::Entry(entry) => entry.collect(names),
            Self::List(entries) => {
                for entry in entries {
                    entry.collect(names);
                }
            }
        }
    }
}

impl PermuteEntry {
    fn collect(&self, names: &mut Vec<String>) {
        match self {
            Self::Name(name) => names.push(name.clone()),
            Self::Nested { permute_with } => permute_with.collect(names),
        }
    }
}

/// An entry of a context's `passes`: a branch, or a pass. A branch is written
/// `if <condition> then: [...]`, and a pass names a `code_block`, which is what
/// tells the two apart.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PassEntryValue {
    /// A branch, whose `else` may be absent.
    Branch(Branch),
    /// One pass.
    Pass(PassValue),
}

/// A branch of a context's `passes`.
#[derive(Clone, Debug, Deserialize)]
pub struct Branch {
    /// The condition.
    #[serde(rename = "if", alias = "condition")]
    pub condition: String,
    /// What to use when it holds.
    #[serde(rename = "then")]
    pub then: Vec<PassEntryValue>,
    /// What to use when it does not.
    #[serde(default, rename = "else", alias = "otherwise")]
    pub otherwise: Vec<PassEntryValue>,
}

/// One pass of a context.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PassValue {
    /// The layer it is drawn on.
    #[serde(default)]
    pub layer: Option<String>,
    /// The code block the pass compiles.
    #[serde(default)]
    pub code_block: String,
    /// The macros the pass defines.
    #[serde(default)]
    pub defines: Option<DefinesValue>,
    /// The render state the pass draws with.
    #[serde(default)]
    pub render_state: Option<String>,
    /// The key the engine sorts variants by.
    #[serde(default)]
    pub branch_key: Option<String>,
}

impl ShaderNode {
    /// Parses a `.shader_node` file.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        serde_sjson::from_str(sjson)
            .map_err(|err| eyre::eyre!("failed to parse the shader node: {err}"))
    }

    /// The family the emitters consume: the variables, the channels, and the
    /// permutation sets in name order.
    ///
    /// An input's flag is the first macro of its type table, and only when the
    /// input is optional: a required input is in every interface, so it has no
    /// flag. A channel is required unless an input of the same name is optional,
    /// in which case the channel follows that input's flag.
    pub fn family(&self) -> Result<Family> {
        let mut family = Family::default();
        for (uuid, input) in &self.inputs {
            let (kind, flags) = input_type(uuid, input)?;
            family.variables.insert(
                input.name.clone(),
                VariableDef {
                    kind,
                    domain: domain(input.domain.as_deref())?,
                    flag: flags.into_iter().next().filter(|_| !input.is_required),
                    default: Vec::new(),
                },
            );
        }
        // The channels table nests conditions, so it is walked rather than read:
        // a channel collects the conditions it sits under.
        let mut channels = Vec::new();
        for (key, entry) in &self.channels {
            walk_channels(key, entry, &[], &family, &mut channels)?;
        }
        family.channels = channels;
        for (name, entries) in &self.permutation_sets {
            let mut choices = Vec::new();
            for entry in entries {
                let condition = entry.condition.clone();
                let define: Define = entry.define.clone().into();
                choices.push(Choice {
                    condition: condition.clone(),
                    macros: define.macros().to_vec(),
                    stages: define.stages().to_vec(),
                    // A choice with no `if` is the set's default.
                    is_default: entry.is_default.unwrap_or(entry.condition.is_none()),
                });
            }
            family.permutation_sets.push(PermutationSet {
                name: name.clone(),
                choices,
            });
        }
        // The sets are a product, so their order decides the group order. The
        // file's own order is the one the toolchain compiles in, so it is kept.
        family.permutation_sets.sort_by(|a, b| a.name.cmp(&b.name));
        for (name, context) in &self.shader_contexts {
            family.contexts.push(ShaderContext {
                name: name.clone(),
                sort_mode: context.passes_sort_mode.clone(),
                compile_with: context
                    .compile_with
                    .iter()
                    .map(|entry| CompileWith {
                        condition: entry.condition.clone(),
                        permute_with: entry.permute_with.as_ref().map(PermuteWith::names).unwrap_or_default(),
                    })
                    .collect(),
                passes: context
                    .passes
                    .iter()
                    .map(walk_pass_entry)
                    .collect::<Result<Vec<PassEntry>>>()?,
            });
        }
        Ok(family)
    }
}

/// One pass entry of a declaration, as the family carries it.
fn walk_pass_entry(entry: &PassEntryValue) -> Result<PassEntry> {
    Ok(match entry {
        PassEntryValue::Branch(branch) => PassEntry::Branch {
            condition: branch.condition.clone(),
            then: branch
                .then
                .iter()
                .map(walk_pass_entry)
                .collect::<Result<Vec<PassEntry>>>()?,
            otherwise: branch
                .otherwise
                .iter()
                .map(walk_pass_entry)
                .collect::<Result<Vec<PassEntry>>>()?,
        },
        PassEntryValue::Pass(pass) => PassEntry::Pass(Pass {
            layer: pass.layer.clone(),
            code_block: pass.code_block.clone(),
            defines: pass.defines.clone().into(),
            render_state: pass.render_state.clone(),
            branch_key: pass.branch_key.clone(),
        }),
    })
}

/// Walks the `channels` table, collecting each channel with the conditions it
/// sits under. A key of a set that is empty gates nothing.
fn walk_channels(
    key: &str,
    entry: &Channels,
    conditions: &[String],
    family: &Family,
    out: &mut Vec<ChannelDef>,
) -> Result<()> {
    match entry {
        Channels::One(channel) => {
            out.push(channel_def(key, channel, conditions, family)?);
        }
        Channels::Set(set) => {
            let mut conditions = conditions.to_vec();
            if !key.is_empty() {
                conditions.push(key.to_string());
            }
            for (key, entry) in set {
                walk_channels(key, entry, &conditions, family, out)?;
            }
        }
    }
    Ok(())
}

/// The `type` of an input. The toolchain writes it either as a table that ties
/// the type to the permutation flags that enable the variable, or - when the
/// variable carries no flags - as a bare type name.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum InputType {
    /// `type = { vector3: ["HAS_BASE_COLOR"] }`
    Flags(BTreeMap<String, Vec<String>>),
    /// `type = "vector3"`
    Plain(String),
}

impl Default for InputType {
    fn default() -> Self {
        Self::Plain(String::new())
    }
}

impl InputType {
    /// The type name and the flags that enable the variable.
    pub fn split(&self) -> Option<(&str, &[String])> {
        match self {
            Self::Flags(table) if table.len() == 1 => {
                let (name, flags) = table.iter().next()?;
                Some((name.as_str(), flags))
            }
            Self::Plain(name) => Some((name.as_str(), &[])),
            _ => None,
        }
    }
}

/// One declared channel, as the family carries it. `conditions` is the path of
/// conditions it sits under; the name of a bare channel is its own key in the
/// table, which is the first entry of that path.
fn channel_def(
    name: &str,
    channel: &Channel,
    conditions: &[String],
    family: &Family,
) -> Result<ChannelDef> {
    let Some(kind) = ValueType::parse(&channel.kind) else {
        bail!("channel {name} has the unknown type {}", channel.kind);
    };
    // A channel of the same name as an optional input follows that input's flag.
    let gated_by_input = family
        .variables
        .get(name)
        .is_some_and(|variable| variable.flag.is_some());
    let domain = if channel.domains.is_empty() {
        domain(channel.domain.as_deref())?
    } else {
        domains(&channel.domains)?
    };
    Ok(ChannelDef {
        name: name.to_string(),
        kind,
        domain,
        // A conditional channel is in no interface until its conditions are
        // evaluated against a permutation.
        required: conditions.is_empty() && !gated_by_input,
        semantic: channel.semantic.clone(),
        variable: None,
        conditions: conditions.to_vec(),
    })
}

/// An input's type and the permutation flags that enable it.
fn input_type(uuid: &str, input: &Input) -> Result<(ValueType, Vec<String>)> {
    if input.name.is_empty() {
        bail!("the input {uuid} has no name");
    }
    let Some((name, flags)) = input.kind.split() else {
        bail!(
            "the input {} has the type {:?}, which names no single type",
            input.name,
            input.kind
        );
    };
    if name.is_empty() {
        bail!("the input {} has no type", input.name);
    }
    let Some(kind) = ValueType::parse(name) else {
        bail!("the input {} has the unknown type {name}", input.name);
    };
    Ok((kind, flags.to_vec()))
}

/// The domain a name spells, defaulting to the pixel stage.
fn domain(name: Option<&str>) -> Result<Domain> {
    Ok(match name {
        None => Domain::Pixel,
        Some("vertex") => Domain::Vertex,
        Some("pixel") => Domain::Pixel,
        Some(other) => bail!("unknown domain {other}"),
    })
}

/// The domain a list of names spells. A channel in both is written by the vertex
/// stage and interpolated into the pixel one.
fn domains(names: &[String]) -> Result<Domain> {
    let mut stage = Domain::Pixel;
    for name in names {
        if domain(Some(name))? == Domain::Vertex {
            stage = Domain::Vertex;
        }
    }
    Ok(stage)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A declaration in the toolchain's own shape, trimmed to what this reader
    /// takes: an inputs table with a flag table per input, a channels table, and
    /// two permutation sets.
    const SAMPLE: &str = r#"
        // A trimmed anisotropic base node.
        inputs = {
            "aca690cb-6305-4a2f-bf3d-69183a493db3" = {
                name = "base_color"
                is_required = false
                display_name = "Base Color"
                type = { vector3: ["HAS_BASE_COLOR"] }
                domain = "pixel"
            }
            "aee6e47b-be7b-4d67-a123-2ab5d660b94e" = {
                name = "texture_map"
                is_required = true
                type = { texture2d: ["HAS_TEXTURE_MAP"] }
                domain = "pixel"
            }
            "7a9306c6-95ae-4cdb-9fef-0eedacce4e83" = {
                name = "opacity"
                is_required = false
                type = { scalar: ["HAS_OPACITY", "HAS_ALPHA"] }
                domain = "pixel"
            }
        }

        channels = {
            texture_map = { type = "texture2d" domain = "pixel" }
            vertex_position = { type = "float4" domain = "vertex" }
            vertex_normal = { type = "float3" semantic = "NORMAL" domain = "vertex" }
            eye_vector = { type = "float3" domains = ["vertex", "pixel"] }
        }

        permutation_sets = {
            vertex_modifiers = [
                { if: "num_skin_weights() == 4" define: { "macros": ["SKINNED_4WEIGHTS"] stages: ["vertex"] } }
                { default = true }
            ]
            instanced_modifiers = [
                { if: "instanced()" define = { "macros": ["INSTANCED"] } }
                { default = true }
            ]
        }
    "#;

    fn sample() -> Family {
        ShaderNode::from_sjson(SAMPLE)
            .expect("parse")
            .family()
            .expect("family")
    }

    #[test]
    fn reads_the_variables_of_a_declaration() {
        let family = sample();
        assert_eq!(family.variables.len(), 3);

        // An optional input's flags hang off its type, and the first is the one
        // that gates the interface.
        let base_color = &family.variables["base_color"];
        assert_eq!(base_color.kind, ValueType::Float3);
        assert_eq!(base_color.flag.as_deref(), Some("HAS_BASE_COLOR"));
        assert_eq!(base_color.domain, Domain::Pixel);

        // A required input is in every interface, so it carries no flag even
        // though its type table has no macros either.
        let texture_map = &family.variables["texture_map"];
        assert_eq!(texture_map.kind, ValueType::Texture2D);
        assert_eq!(texture_map.flag, None);

        // Both flags of a two-macro type are read; the first gates.
        let opacity = &family.variables["opacity"];
        assert_eq!(opacity.kind, ValueType::Float);
        assert_eq!(opacity.flag.as_deref(), Some("HAS_OPACITY"));
    }

    #[test]
    fn reads_the_channels_of_a_declaration() {
        let family = sample();
        assert_eq!(family.channels.len(), 4);

        let position = family.channel("vertex_position").expect("channel");
        assert_eq!(position.kind, ValueType::Float4);
        assert_eq!(position.domain, Domain::Vertex);
        assert_eq!(position.semantic, None);
        assert!(position.required);

        let normal = family.channel("vertex_normal").expect("channel");
        assert_eq!(normal.kind, ValueType::Float3);
        assert_eq!(normal.semantic.as_deref(), Some("NORMAL"));

        // A channel in both domains is written by the vertex stage.
        let eye = family.channel("eye_vector").expect("channel");
        assert_eq!(eye.domain, Domain::Vertex);
    }

    #[test]
    fn a_channel_follows_an_optional_input_of_the_same_name() {
        // texture_map is a required input here, so it has no flag of its own and
        // its channel is required.
        assert!(sample().channel("texture_map").expect("channel").required);
        assert_eq!(sample().variables["texture_map"].flag, None);
        // With the input optional, the flag of its type gates both.
        let text = SAMPLE.replace("is_required = true", "is_required = false");
        let family = ShaderNode::from_sjson(&text)
            .expect("parse")
            .family()
            .expect("family");
        let map = family.channel("texture_map").expect("channel");
        assert!(!map.required);
        assert_eq!(
            family.variables["texture_map"].flag.as_deref(),
            Some("HAS_TEXTURE_MAP")
        );
        // A material that declares the texture gets the channel with it, and one
        // that does not has neither.
        let with_flag = family.interface(&["texture_map".to_string()]);
        assert!(with_flag.variables.contains(&"texture_map".to_string()));
        assert!(with_flag.channels.contains(&"texture_map".to_string()));
        let without = family.interface(&[]);
        assert!(!without.variables.contains(&"texture_map".to_string()));
        assert!(!without.channels.contains(&"texture_map".to_string()));
    }

    #[test]
    fn reads_the_permutation_sets_of_a_declaration() {
        let family = sample();
        // Two sets of two choices each, in name order.
        assert_eq!(family.permutation_sets.len(), 2);
        assert_eq!(family.permutation_sets[0].name, "instanced_modifiers");
        assert_eq!(family.permutation_sets[1].name, "vertex_modifiers");
        assert_eq!(family.group_count(), 4);

        let instanced = &family.permutation_sets[0].choices;
        assert_eq!(instanced[0].condition.as_deref(), Some("instanced()"));
        assert_eq!(instanced[0].macros, vec!["INSTANCED"]);
        // A define with no stages means every stage, and the `=` spelling of the
        // separator reads the same as the `:` one.
        assert!(instanced[0].stages.is_empty());
        assert!(!instanced[0].is_default);
        assert!(instanced[1].is_default);

        let vertex = &family.permutation_sets[1].choices;
        assert_eq!(vertex[0].macros, vec!["SKINNED_4WEIGHTS"]);
        assert_eq!(vertex[0].stages, vec!["vertex"]);
        assert_eq!(
            vertex[0].condition.as_deref(),
            Some("num_skin_weights() == 4")
        );

        // The two optional variables and the two sets count separately: the
        // flags are what a material can ask for, the sets are what gets compiled.
    }

    #[test]
    fn ignores_the_keys_it_does_not_read() {
        let text = format!(
            "{SAMPLE}\nshader_contexts = {{\n\tbase = {{\n\t\tpasses_sort_mode = \"immediate\"\n\t}}\n}}\nsampler_state = {{\n}}\n"
        );
        let family = ShaderNode::from_sjson(&text)
            .expect("parse")
            .family()
            .expect("family");
        assert_eq!(family.group_count(), 4);
    }

    #[test]
    fn reads_a_channel_under_nested_conditions() {
        // The toolchain nests the channels table, and a channel keeps the whole
        // path of conditions it sits under.
        let text = r#"
            channels = {
                "defined(PARTICLE_LIGHTING)" = {
                    basis0 = { type = "float4" domains = ["vertex", "pixel"] }
                }
                "defined(NEEDS_UV_SCALE)" = {
                    "defined(NEEDS_UV_ANIMATION)" = {
                        vertex_uv_data = { type = "float3" }
                    }
                    "!defined(NEEDS_UV_ANIMATION)" = {
                        vertex_uv_data = { type = "float2" }
                    }
                }
                vertex_size = { type = "float2" }
            }
        "#;
        let family = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect("family");
        assert_eq!(family.channels.len(), 4);

        // One condition deep.
        let basis = family.channel("basis0").expect("basis0");
        assert_eq!(basis.conditions, vec!["defined(PARTICLE_LIGHTING)"]);
        assert!(!basis.required);

        // Two deep, and the same channel declared under both branches with a
        // different type each time. The conditions are in name order, so the
        // `!defined` branch comes first.
        let animated: Vec<&ChannelDef> = family
            .channels
            .iter()
            .filter(|channel| channel.name == "vertex_uv_data")
            .collect();
        assert_eq!(animated.len(), 2);
        assert_eq!(
            animated[0].conditions,
            vec!["defined(NEEDS_UV_SCALE)", "!defined(NEEDS_UV_ANIMATION)"]
        );
        assert_eq!(animated[0].kind, ValueType::Float2);
        assert_eq!(animated[1].kind, ValueType::Float3);
        assert_eq!(
            animated[1].conditions,
            vec!["defined(NEEDS_UV_SCALE)", "defined(NEEDS_UV_ANIMATION)"]
        );

        // A channel with no condition of its own is unconditional.
        let size = family.channel("vertex_size").expect("vertex_size");
        assert!(size.conditions.is_empty());
        assert!(size.required);
    }

    /// A context as the toolchain writes one: a `compile_with` that names the
    /// sets to permute, and a pass tree of nested `if`/`then`/`else` branches
    /// with the three spellings of `defines` between them.
    const CONTEXTS: &str = r#"
        shader_contexts = {
            shadow_caster = {
                passes_sort_mode = "immediate"
                compile_with = [
                    { if: "on_renderer(D3D11, D3D12, GNM, GL)" permute_with: "shadow_caster" }
                ]
                passes = [
                    { code_block="depth_only" render_state="shadow_caster" }
                ]
            }

            default = {
                passes_sort_mode = "deferred"
                compile_with = [
                    { if: "on_renderer(D3D11, D3D12, GNM, GL)" permute_with: ["default"] }
                ]
                passes = [
                    { if: "defined(HAS_OPACITY)" then: [
                        { layer="gbuffer_alpha_masked" code_block="gbuffer_base" defines="MOTION_BLUR" render_state="gbuffer_material" }
                    ] else: [
                        { if: "defined(HAS_BASE_COLOR)" then: [
                            { layer="gbuffer" code_block="gbuffer_base" defines=["MOTION_BLUR" "CALCULATE_LIGHTING"] render_state="gbuffer_material" }
                        ] else: [
                            { layer="gbuffer" code_block="gbuffer_base" defines={ macros: ["EMISSIVE_PASS"] stages: ["pixel"] } render_state="emissive" branch_key="dev_wireframe" }
                        ]}
                    ]}
                ]
            }
        }
    "#;

    #[test]
    fn reads_the_contexts_of_a_declaration() {
        let family = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse")
            .family()
            .expect("family");
        // Two contexts, in name order: `default` then `shadow_caster`.
        assert_eq!(family.contexts.len(), 2);
        let default = family.context("default").expect("the default context");
        assert_eq!(default.sort_mode.as_deref(), Some("deferred"));
        assert_eq!(default.compile_with.len(), 1);
        assert_eq!(
            default.compile_with[0].permute_with,
            vec!["default".to_string()]
        );
        assert!(
            default.compile_with[0]
                .holds(&Defines::default())
                .expect("holds")
                == Some(true)
                || default.compile_with[0]
                    .holds(&Defines::default())
                    .expect("holds")
                    .is_none()
        );
        let shadow = family.context("shadow_caster").expect("shadow");
        assert_eq!(shadow.sort_mode.as_deref(), Some("immediate"));
        // A single name, not a list.
        assert_eq!(
            shadow.compile_with[0].permute_with,
            vec!["shadow_caster".to_string()]
        );
        assert!(family.context("nope").is_none());
    }

    #[test]
    fn a_pass_tree_is_read_and_selected() {
        let family = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse")
            .family()
            .expect("family");
        let default = family.context("default").expect("the default context");
        // One entry, a branch over the whole tree.
        assert_eq!(default.passes.len(), 1);
        let PassEntry::Branch {
            then, otherwise, ..
        } = &default.passes[0]
        else {
            panic!("expected a branch, got {:?}", default.passes[0]);
        };
        assert_eq!(then.len(), 1);
        assert_eq!(otherwise.len(), 1);
        // The inner else is a branch of its own.
        let PassEntry::Branch {
            then: inner_then,
            otherwise: inner_otherwise,
            ..
        } = &otherwise[0]
        else {
            panic!("expected a nested branch");
        };
        assert_eq!(inner_then.len(), 1);
        assert_eq!(inner_otherwise.len(), 1);

        // The three spellings of `defines`.
        let PassEntry::Pass(opaque) = &then[0] else {
            panic!("expected a pass");
        };
        assert_eq!(opaque.layer.as_deref(), Some("gbuffer_alpha_masked"));
        assert_eq!(opaque.macros(), ["MOTION_BLUR".to_string()]);
        let PassEntry::Pass(base) = &inner_then[0] else {
            panic!("expected a pass");
        };
        assert_eq!(
            base.macros(),
            ["MOTION_BLUR".to_string(), "CALCULATE_LIGHTING".to_string()]
        );
        let PassEntry::Pass(emissive) = &inner_otherwise[0] else {
            panic!("expected a pass");
        };
        assert_eq!(emissive.macros(), ["EMISSIVE_PASS".to_string()]);
        assert_eq!(emissive.defines.stages(), ["pixel".to_string()]);
        assert_eq!(emissive.branch_key.as_deref(), Some("dev_wireframe"));
    }

    #[test]
    fn the_defines_choose_the_branch() {
        let family = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse")
            .family()
            .expect("family");
        let default = family.context("default").expect("the default context");
        let passes = |defines: &Defines| -> Vec<(String, String)> {
            default
                .passes_of(defines)
                .expect("passes")
                .iter()
                .map(|pass| {
                    (
                        pass.layer.clone().unwrap_or_default(),
                        pass.macros().join(" "),
                    )
                })
                .collect()
        };

        // The first branch, the second, and the innermost else: three different
        // passes, told apart by their macros.
        assert_eq!(
            passes(&defines(&["HAS_OPACITY"])),
            vec![(
                "gbuffer_alpha_masked".to_string(),
                "MOTION_BLUR".to_string()
            )]
        );
        assert_eq!(
            passes(&defines(&["HAS_BASE_COLOR"])),
            vec![(
                "gbuffer".to_string(),
                "MOTION_BLUR CALCULATE_LIGHTING".to_string()
            )]
        );
        assert_eq!(
            passes(&Defines::default()),
            vec![("gbuffer".to_string(), "EMISSIVE_PASS".to_string())]
        );
    }

    #[test]
    fn an_undecidable_branch_contributes_both_sides() {
        // A pass branch on an engine query: the engine decides it per material at
        // runtime, so a generated family has to carry both sides rather than
        // guess.
        let text = r#"
            shader_contexts = {
                default = {
                    passes = [
                        { if: "on_platform(GL)" then: [
                            { layer="gl" code_block="base" }
                        ] else: [
                            { layer="d3d" code_block="base" }
                        ]}
                    ]
                }
            }
        "#;
        let family = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect("family");
        let default = family.context("default").expect("the default context");
        let layers = |defines: &Defines| -> Vec<String> {
            default
                .passes_of(defines)
                .expect("passes")
                .iter()
                .filter_map(|pass| pass.layer.clone())
                .collect()
        };
        // Nothing decides it, so both sides are carried.
        assert_eq!(layers(&Defines::default()), vec!["gl", "d3d"]);
    }

    #[test]
    fn a_context_permutes_over_the_sets_it_names() {
        let mut family = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse")
            .family()
            .expect("family");
        // Two sets of two choices each.
        family.permutation_sets = vec![
            PermutationSet {
                name: "default".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("defined(A)".to_string()),
                        macros: vec!["A".to_string()],
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
            PermutationSet {
                name: "shadow_caster".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("defined(B)".to_string()),
                        macros: vec!["B".to_string()],
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
        let default = family.context("default").expect("the default context");
        let shadow = family.context("shadow_caster").expect("shadow");

        // Each context names one set, so each permutes over that one alone: two
        // groups each, not four.
        let default_groups = family.permutations_for(default);
        assert_eq!(default_groups.len(), 2);
        assert_eq!(default_groups[0].macros, vec!["A".to_string()]);
        let shadow_groups = family.permutations_for(shadow);
        assert_eq!(shadow_groups.len(), 2);
        assert_eq!(shadow_groups[0].macros, vec!["B".to_string()]);

        // The family's groups are the sum over its contexts.
        assert_eq!(family.context_group_count(), 4);
        // Without a name, a context permutes over every set.
        let all = family.permutation_sets.len();
        assert_eq!(all, 2);
        let unnamed = ShaderContext {
            name: "unnamed".to_string(),
            ..ShaderContext::default()
        };
        assert_eq!(family.permutations_for(&unnamed).len(), 4);
    }

    /// A set of macros to test conditions against.
    fn defines(macros: &[&str]) -> Defines {
        Defines::new(macros.iter().map(|name| name.to_string()))
    }

    #[test]
    fn reads_a_bare_input_type() {
        // A variable with no flags of its own writes its type as a bare name.
        let text = r#"
            inputs = {
                "1" = {
                    name = "distortion_normal"
                    is_required = true
                    domain = "pixel"
                    type = "vector3"
                }
            }
        "#;
        let family = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect("family");
        let input = &family.variables["distortion_normal"];
        assert_eq!(input.kind, ValueType::Float3);
        assert_eq!(input.flag, None);
    }

    #[test]
    fn rejects_an_input_with_no_type() {
        let text = "inputs = {\n\t\"1\" = { name = \"x\" }\n}";
        let err = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect_err("no type");
        assert!(err.to_string().contains("no type"), "{err}");
    }

    #[test]
    fn rejects_an_input_with_no_name() {
        let text = "inputs = {\n\t\"1\" = { type = { vector3: [] } }\n}";
        let err = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect_err("no name");
        assert!(err.to_string().contains("no name"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_type() {
        let text = "inputs = {\n\t\"1\" = { name = \"x\" type = { texture_cube: [] } }\n}";
        let err = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect_err("unknown type");
        assert!(err.to_string().contains("texture_cube"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_domain() {
        let text =
            "channels = {\n\tc = {\n\t\ttype = \"float3\"\n\t\tdomain = \"geometry\"\n\t}\n}";
        let err = ShaderNode::from_sjson(text)
            .expect("parse")
            .family()
            .expect_err("unknown domain");
        assert!(err.to_string().contains("geometry"), "{err}");
    }
}

