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

use super::shader_family::{
    ChannelDef, Choice, Domain, Family, PermutationSet, ValueType, VariableDef,
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
    pub define: Option<Define>,
    /// Whether this is the set's default choice, written default = true.
    #[serde(rename = "default", default)]
    pub is_default: Option<bool>,
}

/// The `define` of a choice. The toolchain writes it either way: a bare list of
/// macros, or a table when the macros are limited to some stages.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Define {
    /// `define: ["SKINNED_4WEIGHTS"]`
    Macros(Vec<String>),
    /// `define: { "macros": [...], stages: [...] }`
    Table(DefineTable),
}

impl Define {
    /// The macros the choice defines.
    pub fn macros(&self) -> &[String] {
        match self {
            Self::Macros(macros) => macros,
            Self::Table(table) => &table.macros,
        }
    }

    /// The stages the macros apply to. Empty means every stage.
    pub fn stages(&self) -> &[String] {
        match self {
            Self::Macros(_) => &[],
            Self::Table(table) => &table.stages,
        }
    }
}

/// The table form of a `define`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DefineTable {
    /// The macros to define.
    #[serde(default, alias = "macro")]
    pub macros: Vec<String>,
    /// The stages the macros apply to. Empty means every stage.
    #[serde(default, alias = "stage")]
    pub stages: Vec<String>,
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
                let define = entry.define.clone().unwrap_or(Define::Macros(Vec::new()));
                choices.push(Choice {
                    condition: entry.condition.clone(),
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
        Ok(family)
    }
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
        // The interface it joins is the one that defines the input's flag.
        let with_flag = family
            .interfaces()
            .into_iter()
            .find(|interface| interface.variables.contains(&"texture_map".to_string()))
            .expect("an interface with the texture");
        assert!(with_flag.channels.contains(&"texture_map".to_string()));
        // An interface without it has neither.
        let without = family
            .interfaces()
            .into_iter()
            .find(|interface| !interface.variables.contains(&"texture_map".to_string()))
            .expect("an interface without the texture");
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

        // Two optional variables gate the interfaces, and the two sets gate the
        // four groups: the two counts are independent.
        assert_eq!(family.interface_count(), 4);
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

