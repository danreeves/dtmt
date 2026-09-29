//! The material graph: a material's `shader = { nodes, connections }` block and
//! the node definitions under the renderer's `shader_nodes/`, read into the
//! wiring an output-node declaration's `GRAPH_EVALUATE_*` macros stand for.
//!
//! A node definition (a `.shader_node` under `shader_nodes/`) declares its
//! inputs - each with a connector uuid, a name, a type and a domain - its output
//! type, the option uuids it switches on, and its `code`: HLSL that reads its
//! input names and calls `RESULT(<expr>)` for its output.
//!
//! A material's `shader` block lists **nodes** and **connections**. A node
//! instance picks a definition by `type` (a resource path), selects options by
//! uuid, binds samplers by input name, and carries its own values for the inputs
//! it does not connect. A connection wires one instance's output into another
//! instance's connector by uuid. One of the nodes is the *output node* itself
//! (`core/stingray_renderer/output_nodes/...`, the shader declaration): the
//! connections into its connectors are the graph's outputs, and its connectors'
//! uuids are the uuids of the declaration's `inputs` table.
//!
//! This module reads that structure and resolves it: which instance feeds which
//! connector, what each input's value is when nothing feeds it, and what the
//! graph's outputs are. Generating the HLSL evaluation from the resolution is
//! the next step; the pieces the generator needs are the node definitions' code
//! and option names, which [`NodeDef`] carries.

use std::collections::BTreeMap;

use color_eyre::eyre::{Result, bail};
use serde::Deserialize;

use crate::filetype::shader_decl::Domain;
use crate::filetype::shader_source::CodeParts;

/// The `shader` block of a graph material.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Graph {
    /// The graph's nodes, the output node included.
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    /// The wires between the nodes' connectors.
    #[serde(default)]
    pub connections: Vec<Connection>,
}

/// One node of a graph: an instance of a node definition.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GraphNode {
    /// The instance's uuid, which the connections name.
    #[serde(default)]
    pub id: String,
    /// The definition's resource path, e.g.
    /// `core/shader_nodes/sample_texture` or an output node.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The editor title, which the material's UI shows.
    #[serde(default)]
    pub title: String,
    /// The option uuids the instance selects.
    #[serde(default)]
    pub options: Vec<String>,
    /// The samplers the instance binds, keyed by the input name they feed.
    #[serde(default)]
    pub samplers: BTreeMap<String, SamplerBinding>,
    /// The instance's export overrides, keyed by the definition's export name.
    #[serde(default)]
    pub export: BTreeMap<String, ExportOverride>,
    /// The instance's own values for inputs nothing feeds, keyed by input name.
    #[serde(default, flatten)]
    pub values: BTreeMap<String, NodeValue>,
}

/// An instance's override of one of its definition's exports: the material
/// variable it publishes, under the name the material knows it by.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ExportOverride {
    /// The variable's name in the material. Defaults to the definition's name.
    #[serde(default)]
    pub name: String,
    /// The variable's type, when the instance overrides the definition's.
    #[serde(rename = "type", default)]
    pub kind: NodeType,
    /// The value the material stores for it.
    #[serde(default)]
    pub value: Option<NodeValue>,
}


/// One sampler a node instance binds: the input name is the map key, and the
/// `slot_name` is the material texture channel it reads.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SamplerBinding {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub slot_name: String,
}

/// One wire: a source instance (its output) into a destination instance's
/// connector.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub source: Endpoint,
    #[serde(default)]
    pub destination: Endpoint,
}

/// One end of a wire. The destination end names the connector uuid; the source
/// end names the instance only, because a node has one output.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Endpoint {
    #[serde(default)]
    pub instance_id: String,
    #[serde(default)]
    pub connector_id: Option<String>,
}

/// A value a node instance carries for an unconnected input.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeValue {
    Text(String),
    Number(f64),
    Bool(bool),
    List(Vec<NodeValue>),
    Table(BTreeMap<String, NodeValue>),
}

/// The dialect writes list elements whitespace-separated, which the untagged
/// `Deserialize` derive mangles through serde's content buffering, so the value
/// is read through `deserialize_any` directly.
impl<'de> Deserialize<'de> for NodeValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = NodeValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a node value")
            }

            fn visit_f64<E>(self, value: f64) -> std::result::Result<NodeValue, E> {
                Ok(NodeValue::Number(value))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<NodeValue, E> {
                Ok(NodeValue::Number(value as f64))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<NodeValue, E> {
                Ok(NodeValue::Number(value as f64))
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<NodeValue, E> {
                Ok(NodeValue::Bool(value))
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<NodeValue, E> {
                Ok(NodeValue::Text(value.to_string()))
            }

            fn visit_seq<A>(self, mut seq: A) -> std::result::Result<NodeValue, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(NodeValue::List(values))
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<NodeValue, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut table = BTreeMap::new();
                while let Some((key, value)) = map.next_entry()? {
                    table.insert(key, value);
                }
                Ok(NodeValue::Table(table))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl NodeValue {
    /// The value as an HLSL literal, when it is one: a number, a bool, or a list
    /// of numbers (a vector). A text value is quoted only when the caller says
    /// so, because node text is a name, not a string.
    pub fn hlsl(&self, quote_text: bool) -> Option<String> {
        Some(match self {
            NodeValue::Number(number) => {
                let text = format!("{number}");
                if text.contains('.') { text } else { format!("{text}.0") }
            }
            NodeValue::Bool(value) => value.to_string(),
            NodeValue::List(values) => {
                let parts: Option<Vec<String>> =
                    values.iter().map(|value| value.hlsl(quote_text)).collect();
                format!("{{ {} }}", parts?.join(", "))
            }
            NodeValue::Text(text) => {
                if quote_text {
                    format!("\"{text}\"")
                } else {
                    text.clone()
                }
            }
            NodeValue::Table(_) => return None,
        })
    }
}

/// A node definition, read from a `.shader_node` under `shader_nodes/`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeDef {
    /// The definition's editor group and display name.
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub display_name: String,
    /// The inputs, keyed by connector uuid.
    #[serde(default)]
    pub inputs: BTreeMap<String, NodeInput>,
    /// The values the node reads from the engine or from a channel, keyed by
    /// the name its code uses.
    #[serde(default, deserialize_with = "flatten_imports")]
    pub imports: BTreeMap<String, NodeImport>,
    /// The macros the definition's code needs defined.
    #[serde(default)]
    pub defines: Vec<String>,
    /// The stage the definition's code belongs to, when it names one.
    #[serde(default)]
    pub domain: Option<String>,
    /// The output's type, usually `{ typeof: "<input name>" }`.
    #[serde(default)]
    pub output: NodeOutput,
    /// The option uuids the definition's code switches on, keyed by uuid.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The HLSL body: reads the input names, calls `RESULT(<expr>)`. Some
    /// definitions write it here; others put it in `code_blocks`.
    #[serde(default)]
    pub code: Option<CodeParts>,
    /// The definition's code blocks, when it writes them. The `default` block
    /// is the node's body; the rest are helpers.
    #[serde(default)]
    pub code_blocks: BTreeMap<String, NodeCode>,
    /// The values the node exports to the material, keyed by export name.
    #[serde(default)]
    pub exports: BTreeMap<String, NodeExport>,
}

/// One code block of a node definition.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeCode {
    /// The body: a bare string, or the `shared`/`hlsl`/`glsl` table.
    #[serde(default)]
    pub code: Option<CodeParts>,
    #[serde(default)]
    pub language: String,
}

/// A value a node definition reads from outside the graph: a channel the
/// declaration carries, or an engine global.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeImport {
    #[serde(rename = "type", default)]
    pub kind: NodeType,
    /// The stage the value comes from.
    #[serde(default)]
    pub domain: Option<String>,
    /// The declaration channel the value is, when it names one.
    #[serde(default)]
    pub output_channel: Option<String>,
    /// The vertex semantic the value binds to, when it is a mesh input.
    #[serde(default)]
    pub semantic: Option<String>,
    /// `engine` when the value is an engine global rather than a channel.
    #[serde(default)]
    pub source: Option<String>,
}

/// Reads an `imports` table. An entry is an import definition, but the table
/// may also nest entries under a condition - `"!defined(NO_VERTEX_NORMALS)": {
/// tsm0 = { ... } }` - so a table whose keys are not the fields of an import is
/// walked as one.
fn flatten_imports<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, NodeImport>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    fn is_import(table: &BTreeMap<String, NodeValue>) -> bool {
        !table.is_empty()
            && table.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type" | "domain" | "output_channel" | "semantic" | "source" | "display_name"
                )
            })
    }

    fn text(table: &BTreeMap<String, NodeValue>, key: &str) -> Option<String> {
        match table.get(key) {
            Some(NodeValue::Text(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn walk(table: &BTreeMap<String, NodeValue>, imports: &mut BTreeMap<String, NodeImport>) {
        for (name, value) in table {
            let NodeValue::Table(inner) = value else {
                continue;
            };
            if !is_import(inner) {
                walk(inner, imports);
                continue;
            }
            let mut import = NodeImport::default();
            import.kind = match inner.get("type") {
                Some(NodeValue::Text(name)) => NodeType::Name(name.clone()),
                Some(NodeValue::Table(kind)) => match text(kind, "typeof") {
                    Some(follows) => NodeType::TypeOf(TypeOf { r#typeof: follows }),
                    None => NodeType::Gated(
                        kind.iter()
                            .map(|(key, value)| {
                                let flags = match value {
                                    NodeValue::List(values) => values
                                        .iter()
                                        .filter_map(|value| match value {
                                            NodeValue::Text(flag) => Some(flag.clone()),
                                            _ => None,
                                        })
                                        .collect(),
                                    NodeValue::Text(flag) => vec![flag.clone()],
                                    _ => Vec::new(),
                                };
                                (key.clone(), flags)
                            })
                            .collect(),
                    ),
                },
                _ => NodeType::None,
            };
            import.domain = text(inner, "domain");
            import.output_channel = text(inner, "output_channel");
            import.semantic = text(inner, "semantic");
            import.source = text(inner, "source");
            imports.insert(name.clone(), import);
        }
    }

    let raw: BTreeMap<String, NodeValue> = BTreeMap::deserialize(deserializer)?;
    let mut imports = BTreeMap::new();
    walk(&raw, &mut imports);
    Ok(imports)
}

/// One input of a node definition.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    /// The declared type: a name like `float3`, or a table
    /// `{ scalar: ["HAS_X"] }` when a flag gates it.
    #[serde(rename = "type", default)]
    pub kind: NodeType,
    /// The stage the input belongs to, when the definition names one.
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub is_required: bool,
}

/// A node's output: its type, either a name or `{ typeof: "<input name>" }`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeOutput {
    #[serde(rename = "type", default)]
    pub kind: NodeType,
}

/// A type a node definition writes: a bare name, or a table keyed by the name
/// whose value is the flags that gate it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
pub enum NodeType {
    #[default]
    None,
    Name(String),
    Gated(BTreeMap<String, Vec<String>>),
    TypeOf(TypeOf),
}

/// The `{ typeof: "<input name>" }` spelling.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TypeOf {
    #[serde(default)]
    pub r#typeof: String,
}

impl NodeType {
    /// The type's name, when it is a plain name.
    pub fn name(&self) -> Option<&str> {
        match self {
            NodeType::Name(name) => Some(name),
            NodeType::Gated(table) => table.keys().next().map(String::as_str),
            _ => None,
        }
    }

    /// The input the type follows, for the `typeof` spelling.
    pub fn follows(&self) -> Option<&str> {
        match self {
            NodeType::TypeOf(inner) => Some(&inner.r#typeof),
            _ => None,
        }
    }

    /// The inputs a `largestof`/`smallestof` type picks among.
    pub fn among(&self) -> Option<(&str, &[String])> {
        let NodeType::Gated(table) = self else {
            return None;
        };
        let (key, names) = table.iter().next()?;
        matches!(key.as_str(), "largestof" | "smallestof").then_some((key.as_str(), names.as_slice()))
    }
}

/// One value a node definition exports to the material.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeExport {
    #[serde(rename = "type", default)]
    pub kind: NodeType,
    #[serde(default)]
    pub value: Option<NodeValue>,
}

impl Graph {
    /// Reads the graph out of a material's text.
    pub fn from_material(text: &str) -> Result<Option<Self>> {
        #[derive(Deserialize)]
        struct Material {
            #[serde(default)]
            shader: Option<Graph>,
        }
        let mut material: Material = serde_sjson::from_str(text)?;
        // The material writes connector and option uuids lower case where the
        // definitions write them upper case, so everything is normalized here.
        if let Some(graph) = &mut material.shader {
            for connection in &mut graph.connections {
                connection.source.instance_id = connection.source.instance_id.to_lowercase();
                connection.destination.instance_id = connection.destination.instance_id.to_lowercase();
                connection.destination.connector_id = connection
                    .destination
                    .connector_id
                    .take()
                    .map(|connector| connector.to_lowercase());
            }
            for node in &mut graph.nodes {
                node.id = node.id.to_lowercase();
                node.options = node.options.iter().map(|option| option.to_lowercase()).collect();
            }
        }
        Ok(material.shader)
    }

    /// The node with this id.
    pub fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The node that is the output node: the one whose definition is not under
    /// `shader_nodes/`, which is the shader declaration itself.
    pub fn output_node(&self) -> Option<&GraphNode> {
        self.nodes
            .iter()
            .find(|node| !node.kind.contains("shader_nodes/"))
    }

    /// The connections into a node's connectors, keyed by connector uuid: the
    /// source instance id that feeds each.
    pub fn feeds(&self, instance: &str) -> BTreeMap<String, String> {
        let mut feeds = BTreeMap::new();
        for connection in &self.connections {
            if connection.destination.instance_id != instance {
                continue;
            }
            if let Some(connector) = &connection.destination.connector_id {
                feeds.insert(connector.clone(), connection.source.instance_id.clone());
            }
        }
        feeds
    }
}

impl NodeDef {
    /// Reads a node definition out of a `.shader_node`'s text. The connector and
    /// option uuids are normalized to lower case, because a material writes them
    /// that way and the definitions write them upper case.
    pub fn from_text(text: &str) -> Result<Self> {
        let mut def: Self = serde_sjson::from_str(text)?;
        def.inputs = def
            .inputs
            .into_iter()
            .map(|(uuid, input)| (uuid.to_lowercase(), input))
            .collect();
        def.options = def
            .options
            .into_iter()
            .map(|(uuid, name)| (uuid.to_lowercase(), name))
            .collect();
        Ok(def)
    }

    /// The definition's input with this connector uuid.
    pub fn input(&self, connector: &str) -> Option<&NodeInput> {
        self.inputs.get(connector)
    }

    /// The node's HLSL body: the top-level `code`, or the `default` block of a
    /// `code_blocks` table. The definitions use both shapes.
    pub fn code(&self) -> String {
        self.code
            .as_ref()
            .map(CodeParts::hlsl)
            .or_else(|| {
                self.code_blocks
                    .get("default")
                    .and_then(|block| block.code.as_ref())
                    .map(CodeParts::hlsl)
            })
            .unwrap_or_default()
    }

    /// The option names the definition declares for the instance's selected
    /// option uuids, in the definition's order. A uuid the definition does not
    /// know is reported, because the code would not switch on it.
    pub fn option_names(&self, selected: &[String]) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for uuid in selected {
            match self.options.get(uuid) {
                Some(name) => names.push(name.clone()),
                None => bail!("the node definition has no option {uuid}"),
            }
        }
        Ok(names)
    }

    /// The definition's declared type for one of its inputs: a plain name, or
    /// the name a gated table keys on.
    pub fn input_type(&self, name: &str) -> Option<String> {
        self.inputs
            .values()
            .find(|input| input.name == name)
            .and_then(|input| input.kind.name().map(str::to_string))
    }
}

/// What feeds one input of a resolved node.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Another node's output.
    Node(String),
    /// The instance's own value.
    Value(NodeValue),
    /// A sampler the instance binds; the string is the material channel.
    Sampler(String),
    /// Nothing feeds it and the instance carries no value.
    Unbound,
}

/// One node of a resolved graph.
#[derive(Clone, Debug)]
pub struct ResolvedNode {
    /// The instance's uuid.
    pub id: String,
    /// The definition's resource path.
    pub kind: String,
    /// The editor title.
    pub title: String,
    /// The option names the instance's selected uuids map to.
    pub options: Vec<String>,
    /// The inputs, in the definition's order: `(input name, source)`.
    pub inputs: Vec<(String, Source)>,
    /// The samplers the instance binds: input name -> material channel.
    pub samplers: BTreeMap<String, String>,
    /// The instance's exports: `(definition name, instance name, type, value)`.
    pub exports: Vec<(String, String, NodeType, Option<NodeValue>)>,
    /// The definition's code.
    pub code: String,
    /// Whether the node is the output node (the shader declaration).
    pub output: bool,
}

/// One of the graph's outputs: a declaration input, and the node that feeds it.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphOutput {
    /// The declaration input's name.
    pub name: String,
    /// The node feeding it.
    pub source: Source,
    /// The stage the declaration input lives in.
    pub domain: Domain,
}

/// A resolved graph: every node's wiring and the graph's outputs.
#[derive(Clone, Debug)]
pub struct Resolution {
    /// The nodes, the output node last.
    pub nodes: Vec<ResolvedNode>,
    /// The output node's inputs: which node feeds each shader input.
    pub outputs: Vec<GraphOutput>,
}

impl Graph {
    /// Resolves the wiring: for every node, what feeds each of its inputs, and
    /// for the output node, which shader input each connector is.
    ///
    /// `defs` maps a definition's resource path to its definition, and
    /// `shader_inputs` maps the output node's connector uuids to the shader
    /// declaration's input names.
    pub fn resolve(
        &self,
        defs: &BTreeMap<String, NodeDef>,
        shader_inputs: &BTreeMap<String, String>,
    ) -> Result<Resolution> {
        let Some(output) = self.output_node() else {
            bail!("the graph has no output node");
        };
        let output_id = output.id.clone();

        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let def = defs
                .get(&node.kind)
                .ok_or_else(|| color_eyre::eyre::eyre!("no definition for '{}'", node.kind))?;
            let feeds = self.feeds(&node.id);
            let mut inputs = Vec::new();
            for (connector, input) in &def.inputs {
                let source = if let Some(instance) = feeds.get(connector) {
                    Source::Node(instance.clone())
                } else if let Some(slot) = node.samplers.get(&input.name) {
                    Source::Sampler(slot.slot_name.clone())
                } else if let Some(value) = node.values.get(&input.name) {
                    Source::Value(value.clone())
                } else {
                    Source::Unbound
                };
                inputs.push((input.name.clone(), source));
            }
            let options = def.option_names(&node.options)?;
            let samplers = node
                .samplers
                .iter()
                .map(|(name, binding)| (name.clone(), binding.slot_name.clone()))
                .collect();
            // The instance's export overrides: the definition's export name is
            // what the definition's code reads, the instance's name is what the
            // material knows the variable by.
            let mut exports = Vec::new();
            for (name, export) in &def.exports {
                let instance = node.export.get(name);
                let instance_name = instance
                    .map(|export| export.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| name.clone());
                let kind = instance
                    .map(|export| export.kind.clone())
                    .filter(|kind| !matches!(kind, NodeType::None))
                    .unwrap_or_else(|| export.kind.clone());
                let value = instance
                    .and_then(|export| export.value.clone())
                    .or_else(|| export.value.clone());
                exports.push((name.clone(), instance_name, kind, value));
            }
            nodes.push(ResolvedNode {
                id: node.id.clone(),
                kind: node.kind.clone(),
                title: node.title.clone(),
                options,
                inputs,
                samplers,
                exports,
                code: def.code(),
                output: node.id == output_id,
            });
        }

        // The graph's outputs: the output node's connectors, named by the
        // declaration's input table and living in the input's stage.
        let output_def = defs.get(&output.kind);
        let feeds = self.feeds(&output_id);
        let mut outputs = Vec::new();
        for (connector, instance) in &feeds {
            let name = shader_inputs
                .get(connector)
                .cloned()
                .unwrap_or_else(|| connector.clone());
            let domain = output_def
                .and_then(|def| def.inputs.get(connector))
                .and_then(|input| input.domain.as_deref())
                .filter(|domain| *domain == "vertex")
                .map(|_| Domain::Vertex)
                .unwrap_or(Domain::Pixel);
            outputs.push(GraphOutput {
                name,
                source: Source::Node(instance.clone()),
                domain,
            });
        }

        Ok(Resolution { nodes, outputs })
    }
}

/// The HLSL name of an engine value type, or `None` for `auto` and names the
/// generator does not know.
pub fn hlsl_type(name: &str) -> Option<&'static str> {
    Some(match name {
        "scalar" | "float" => "float",
        "vector2" | "float2" => "float2",
        "vector3" | "float3" => "float3",
        "vector4" | "float4" => "float4",
        "matrix4x4" | "float4x4" => "float4x4",
        "uint" => "uint",
        "bool" => "bool",
        "2d" | "texture2d" => "Texture2D",
        "cube" => "TextureCube",
        _ => return None,
    })
}

/// How wide a value is, for `largestof`/`smallestof`.
fn type_width(hlsl: &str) -> u8 {
    match hlsl {
        "float" => 1,
        "float2" => 2,
        "float3" => 3,
        "float4" => 4,
        "float4x4" => 16,
        _ => 0,
    }
}

/// A channel the graph's imports add to the declaration: a mesh input with a
/// semantic, which the generated vertex input declares and the pixel stage reads
/// interpolated.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphChannel {
    /// The channel's name, which is the import's name.
    pub name: String,
    /// The channel's HLSL type.
    pub hlsl: &'static str,
    /// The vertex semantic it binds to.
    pub semantic: String,
}

/// The generated graph evaluation: the HLSL of each stage, the defines the used
/// nodes ask for, the samplers the graph binds, the channels its imports add and
/// the material variables it exports.
#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    /// The vertex stage's evaluation, as the body of a function.
    pub vertex: String,
    /// The pixel stage's evaluation.
    pub pixel: String,
    /// The macros the used node definitions need defined, in first-use order.
    pub defines: Vec<String>,
    /// The material channels the graph samples, in first-use order. The sampler
    /// variable carries the channel's name.
    pub samplers: Vec<String>,
    /// The channels the graph's imports add to the declaration.
    pub channels: Vec<GraphChannel>,
    /// The material variables the graph exports: `(name, HLSL type, value)`.
    pub exports: Vec<(String, &'static str, Option<NodeValue>)>,
}

/// The resolved types of one node: its inputs and its output.
#[derive(Clone, Debug, Default)]
struct NodeTypes {
    inputs: BTreeMap<String, String>,
    output: String,
}

impl Resolution {
    /// Generates the graph's evaluation: for each stage, the HLSL that computes
    /// every node the stage's outputs need, writes them into the results struct
    /// and binds the inputs, imports and samplers the node definitions read.
    ///
    /// The generated code is the body of a function taking the stage's params
    /// and results, because the node code carries preprocessor branches
    /// (`#if defined(OP_EQUAL)` and the material's flags) that a macro
    /// definition could not.
    pub fn evaluate(&self, defs: &BTreeMap<String, NodeDef>) -> Result<Evaluation> {
        let def_of = |node: &ResolvedNode| -> Result<&NodeDef> {
            defs.get(&node.kind)
                .ok_or_else(|| color_eyre::eyre::eyre!("no definition for '{}'", node.kind))
        };

        // The graph's own nodes: the output node is the declaration itself.
        let nodes: Vec<&ResolvedNode> = self.nodes.iter().filter(|node| !node.output).collect();
        let position: BTreeMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();

        // What each node reads from other nodes.
        let mut feeds: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for node in &nodes {
            let sources = node
                .inputs
                .iter()
                .filter_map(|(_, source)| match source {
                    Source::Node(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            feeds.insert(node.id.clone(), sources);
        }

        // The graph's outputs, by stage, and the nodes each stage needs.
        let roots = |domain: Domain| -> Vec<String> {
            self.outputs
                .iter()
                .filter(|output| output.domain == domain)
                .filter_map(|output| match &output.source {
                    Source::Node(id) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        let vertex_order = dependency_order(&roots(Domain::Vertex), &feeds);
        let pixel_order = dependency_order(&roots(Domain::Pixel), &feeds);

        // A node the definitions put in one stage cannot be evaluated in the
        // other: its imports would not be there.
        for (order, stage) in [(&vertex_order, "vertex"), (&pixel_order, "pixel")] {
            for id in order {
                let node = &nodes[position[id]];
                let def = def_of(node)?;
                if let Some(domain) = &def.domain
                    && domain != stage
                {
                    bail!(
                        "the '{}' node '{}' is a {domain} node, but the {stage} stage needs it",
                        node.title,
                        node.kind
                    );
                }
            }
        }

        // The defines, samplers, channels and exports of the nodes in use.
        let mut evaluation = Evaluation::default();
        for node in &nodes {
            let def = def_of(node)?;
            for define in &def.defines {
                if !evaluation.defines.contains(define) {
                    evaluation.defines.push(define.clone());
                }
            }
            for slot in node.samplers.values() {
                if !slot.is_empty() && !evaluation.samplers.contains(slot) {
                    evaluation.samplers.push(slot.clone());
                }
            }
            for (name, instance, kind, value) in &node.exports {
                let _ = name;
                let hlsl = match kind {
                    NodeType::Name(name) => hlsl_type(name),
                    NodeType::Gated(_) => kind.name().and_then(hlsl_type),
                    _ => None,
                }
                .unwrap_or("float4");
                if !evaluation.exports.iter().any(|(seen, _, _)| seen == instance) {
                    evaluation.exports.push((instance.clone(), hlsl, value.clone()));
                }
            }
            for (name, import) in &def.imports {
                if import.source.as_deref() == Some("engine") {
                    continue;
                }
                let Some(semantic) = &import.semantic else {
                    continue;
                };
                if evaluation.channels.iter().any(|channel| &channel.name == name) {
                    continue;
                }
                let hlsl = import
                    .kind
                    .name()
                    .and_then(hlsl_type)
                    .unwrap_or("float4");
                evaluation.channels.push(GraphChannel {
                    name: name.clone(),
                    hlsl,
                    semantic: semantic.clone(),
                });
            }
        }

        // The types, in dependency order (the vertex and pixel closures together).
        let mut order: Vec<usize> = Vec::new();
        for id in vertex_order.iter().chain(pixel_order.iter()) {
            if !order.contains(&position[id]) {
                order.push(position[id]);
            }
        }
        let mut types: BTreeMap<String, NodeTypes> = BTreeMap::new();
        for index in &order {
            let node = nodes[*index];
            let def = def_of(node)?;
            let resolved = resolve_types(node, def, &types)?;
            types.insert(node.id.clone(), resolved);
        }

        evaluation.vertex =
            self.evaluate_stage(&nodes, &position, &vertex_order, &types, defs, Domain::Vertex)?;
        evaluation.pixel =
            self.evaluate_stage(&nodes, &position, &pixel_order, &types, defs, Domain::Pixel)?;
        Ok(evaluation)
    }

    /// Emits one stage's evaluation: the nodes in dependency order, each with
    /// its inputs bound, and the stage's outputs written into `results`.
    fn evaluate_stage(
        &self,
        nodes: &[&ResolvedNode],
        position: &BTreeMap<String, usize>,
        order: &[String],
        types: &BTreeMap<String, NodeTypes>,
        defs: &BTreeMap<String, NodeDef>,
        domain: Domain,
    ) -> Result<String> {
        let mut out = String::new();
        let mut variables: BTreeMap<&str, String> = BTreeMap::new();
        for id in order {
            let index = position[id];
            let node = nodes[index];
            let def = defs
                .get(&node.kind)
                .ok_or_else(|| color_eyre::eyre::eyre!("no definition for '{}'", node.kind))?;
            let node_types = types
                .get(id)
                .ok_or_else(|| color_eyre::eyre::eyre!("no types for '{}'", node.id))?;
            let variable = format!("node_{index}");
            variables.insert(id.as_str(), variable.clone());

            out.push_str(&format!("    // {}\n", node.title));
            for option in &node.options {
                out.push_str(&format!("    #define {option}\n"));
            }
            out.push_str(&format!("    {} {variable};\n", node_types.output));
            out.push_str("    {\n");

            // The inputs the connections and the instance's values feed.
            for (name, source) in &node.inputs {
                let hlsl = node_types
                    .inputs
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "float".to_string());
                match source {
                    Source::Node(upstream) => {
                        let source = variables
                            .get(upstream.as_str())
                            .ok_or_else(|| color_eyre::eyre::eyre!("{upstream} is not evaluated yet"))?;
                        out.push_str(&format!("        {hlsl} {name} = {source};\n"));
                    }
                    Source::Value(value) => {
                        let literal = value
                            .hlsl(false)
                            .ok_or_else(|| color_eyre::eyre::eyre!("the value of '{name}' is not a literal"))?;
                        out.push_str(&format!("        {hlsl} {name} = {literal};\n"));
                    }
                    // A sampler is a global variable, declared once per channel.
                    Source::Sampler(_) => {}
                    Source::Unbound => out.push_str(&format!("        {hlsl} {name} = 0;\n")),
                }
            }

            // The imports: engine globals keep their name; a channel import is
            // read from the channel it names, or from a channel of its own name
            // when it is a mesh input the scaffold adds.
            for (name, import) in &def.imports {
                if import.source.as_deref() == Some("engine") {
                    continue;
                }
                let hlsl = import.kind.name().and_then(hlsl_type).unwrap_or("float4");
                let channel = import.output_channel.as_deref().unwrap_or(name);
                out.push_str(&format!("        {hlsl} {name} = params.{channel};\n"));
            }

            // The code: RESULT writes the node's variable, `<input>_type` is the
            // resolved type, and the instance's names replace the definition's.
            let mut code = node.code.clone();
            for (definition, instance, _, _) in &node.exports {
                if definition != instance {
                    code = replace_identifier(&code, definition, instance);
                }
            }
            for (name, slot) in &node.samplers {
                if !slot.is_empty() && name != slot {
                    code = replace_identifier(&code, name, slot);
                }
            }
            for (name, hlsl) in &node_types.inputs {
                code = replace_identifier(&code, &format!("{name}_type"), hlsl);
            }
            code = replace_result(&code, &variable);
            out.push_str(&code);
            if !code.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("    }\n");
            for option in &node.options {
                out.push_str(&format!("    #undef {option}\n"));
            }
        }

        // The stage's outputs.
        for output in &self.outputs {
            if output.domain != domain {
                continue;
            }
            let Source::Node(id) = &output.source else {
                continue;
            };
            let Some(variable) = variables.get(id.as_str()) else {
                continue;
            };
            out.push_str(&format!("    results.{} = {variable};\n", output.name));
        }
        Ok(out)
    }
}

/// The nodes a set of roots depends on, in dependency order (a node after the
/// nodes it reads).
fn dependency_order(roots: &[String], feeds: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut visiting: Vec<String> = Vec::new();
    fn visit(
        id: &str,
        feeds: &BTreeMap<String, Vec<String>>,
        order: &mut Vec<String>,
        visiting: &mut Vec<String>,
    ) {
        if order.iter().any(|seen| seen == id) || visiting.iter().any(|seen| seen == id) {
            return;
        }
        visiting.push(id.to_string());
        for source in feeds.get(id).into_iter().flatten() {
            visit(source, feeds, order, visiting);
        }
        visiting.pop();
        order.push(id.to_string());
    }
    for root in roots {
        visit(root, feeds, &mut order, &mut visiting);
    }
    order
}

/// Resolves one node's input and output types.
fn resolve_types(
    node: &ResolvedNode,
    def: &NodeDef,
    known: &BTreeMap<String, NodeTypes>,
) -> Result<NodeTypes> {
    // The declared type of a name the node's code reads: an input, an import or
    // an export, following `typeof` to the name it follows.
    fn declared(
        name: &str,
        def: &NodeDef,
        node: &ResolvedNode,
        seen: &mut Vec<String>,
    ) -> Option<NodeType> {
        if seen.iter().any(|visited| visited == name) {
            return None;
        }
        seen.push(name.to_string());
        let kind = def
            .inputs
            .values()
            .find(|input| input.name == name)
            .map(|input| input.kind.clone())
            .or_else(|| def.imports.get(name).map(|import| import.kind.clone()))
            .or_else(|| {
                node.exports
                    .iter()
                    .find(|(definition, instance, _, _)| definition == name || instance == name)
                    .map(|(_, _, kind, _)| kind.clone())
            })?;
        match &kind {
            NodeType::TypeOf(inner) => declared(&inner.r#typeof, def, node, seen),
            _ => Some(kind),
        }
    }

    let mut inputs = BTreeMap::new();
    for (name, source) in &node.inputs {
        let mut seen = Vec::new();
        let hlsl = declared(name, def, node, &mut seen)
            .and_then(|kind| kind.name().and_then(hlsl_type).map(str::to_string))
            .or_else(|| match source {
                Source::Node(id) => known.get(id).map(|types| types.output.clone()),
                Source::Value(value) => value_type(value),
                Source::Sampler(_) | Source::Unbound => None,
            })
            .unwrap_or_else(|| "float".to_string());
        inputs.insert(name.clone(), hlsl);
    }
    for name in def.imports.keys() {
        let mut seen = Vec::new();
        let hlsl = declared(name, def, node, &mut seen)
            .and_then(|kind| kind.name().and_then(hlsl_type).map(str::to_string))
            .unwrap_or_else(|| "float".to_string());
        inputs.insert(name.clone(), hlsl);
    }
    for (definition, instance, _kind, _) in &node.exports {
        let mut seen = Vec::new();
        let hlsl = declared(instance, def, node, &mut seen)
            .or_else(|| declared(definition, def, node, &mut seen))
            .and_then(|kind| kind.name().and_then(hlsl_type).map(str::to_string))
            .unwrap_or_else(|| "float".to_string());
        inputs.insert(instance.clone(), hlsl.clone());
        inputs.insert(definition.clone(), hlsl);
    }

    let output = match &def.output.kind {
        NodeType::TypeOf(inner) => inputs.get(&inner.r#typeof).cloned(),
        NodeType::Gated(_) => match def.output.kind.among() {
            Some((which, names)) => {
                let mut best: Option<String> = None;
                for name in names {
                    let Some(ty) = inputs.get(name) else {
                        continue;
                    };
                    let take = best
                        .as_ref()
                        .map(|current| match which {
                            "largestof" => type_width(ty) > type_width(current),
                            _ => type_width(ty) < type_width(current),
                        })
                        .unwrap_or(true);
                    if take {
                        best = Some(ty.clone());
                    }
                }
                best
            }
            None => def.output.kind.name().and_then(hlsl_type).map(str::to_string),
        },
        NodeType::Name(name) => hlsl_type(name).map(str::to_string),
        NodeType::None => None,
    };
    let output = output
        .or_else(|| {
            inputs
                .values()
                .max_by_key(|ty| type_width(ty))
                .cloned()
        })
        .unwrap_or_else(|| "float4".to_string());

    Ok(NodeTypes { inputs, output })
}

/// The HLSL type of an instance value: a number is a scalar, a list a vector.
fn value_type(value: &NodeValue) -> Option<String> {
    Some(match value {
        NodeValue::Number(_) => "float".to_string(),
        NodeValue::Bool(_) => "bool".to_string(),
        NodeValue::List(values) => match values.len() {
            1 => "float".to_string(),
            2 => "float2".to_string(),
            3 => "float3".to_string(),
            4 => "float4".to_string(),
            _ => return None,
        },
        NodeValue::Text(_) | NodeValue::Table(_) => return None,
    })
}

/// Replaces whole identifiers in a code block.
fn replace_identifier(code: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return code.to_string();
    }
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(at) = rest.find(from) {
        let before = rest[..at].chars().last();
        let after = rest[at + from.len()..].chars().next();
        if before.map(is_word).unwrap_or(false) || after.map(is_word).unwrap_or(false) {
            out.push_str(&rest[..at + from.len()]);
            rest = &rest[at + from.len()..];
            continue;
        }
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// Replaces `RESULT(<expr>)` with an assignment to `value`, keeping the
/// expression. The expression may contain parentheses of its own.
fn replace_result(code: &str, value: &str) -> String {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(at) = rest.find("RESULT") {
        let before = rest[..at].chars().last();
        let after = &rest[at + "RESULT".len()..];
        let skipped = after.len() - after.trim_start().len();
        if before.map(is_word).unwrap_or(false) || !after[skipped..].starts_with('(') {
            out.push_str(&rest[..at + "RESULT".len()]);
            rest = &rest[at + "RESULT".len()..];
            continue;
        }
        out.push_str(&rest[..at]);
        let open = at + "RESULT".len() + skipped;
        let mut depth = 0usize;
        let mut end = None;
        for (offset, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(rest);
            return out;
        };
        let expression = &rest[open + 1..end];
        out.push_str(value);
        out.push_str(" = ");
        out.push_str(expression.trim());
        out.push_str(";\n");
        // The call's own semicolon is left behind by the replacement.
        rest = &rest[end + 1..];
        if let Some(after) = rest.strip_prefix(';') {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A switch node: two auto inputs, one option, `typeof` on an input.
    const SWITCH: &str = r#"
        group = "Utility"
        display_name = "If"
        inputs = {
            "CED7BBF3-0B48-4335-B933-095A41CA0294" = { name = "input_a" display_name = "A" type = "auto" }
            "39BC7619-2768-480B-ACFD-63FA66EF6905" = { name = "input_b" display_name = "B" type = "auto" }
        }
        output = {
            type = { typeof: "input_a" }
        }
        options = {
            "9A84282B-F1A2-46D4-9FC4-5A76FC9B30DD" = "OP_EQUAL"
        }
        code = """
            RESULT(input_a);
        """
    "#;

    /// A constant node: one typed input, no options.
    const CONSTANT: &str = r#"
        group = "Constant"
        display_name = "Constant Vector3"
        inputs = {
            "6ff26be7-68a1-4b89-b9dd-551d216086c2" = { name = "a" display_name = "RGB" type = "float3" }
        }
        output = {
            type = { typeof: "a" }
        }
        code = """
            RESULT(a);
        """
    "#;

    /// The output node: one input, the graph's own output.
    const OUTPUT: &str = r#"
        inputs = {
            "00000000-0000-0000-0000-000000000001" = { name = "base_color" type = "vector3" domain = "pixel" }
        }
        code_blocks = { }
        shader_contexts = { }
    "#;

    const MATERIAL: &str = r#"
        shader = {
            connections = [
                {
                    destination = {
                        connector_id = "CED7BBF3-0B48-4335-B933-095A41CA0294"
                        instance_id = "aaaa"
                    }
                    source = {
                        instance_id = "bbbb"
                    }
                }
                {
                    destination = {
                        connector_id = "00000000-0000-0000-0000-000000000001"
                        instance_id = "cccc"
                    }
                    source = {
                        instance_id = "aaaa"
                    }
                }
            ]
            nodes = [
                {
                    id = "bbbb"
                    type = "core/shader_nodes/constant_vector3"
                    title = "Tint"
                    a = [
                        1
                        0.5
                        0
                    ]
                }
                {
                    id = "aaaa"
                    type = "core/shader_nodes/if"
                    title = "Switch"
                    options = [ "9A84282B-F1A2-46D4-9FC4-5A76FC9B30DD" ]
                }
                {
                    id = "cccc"
                    type = "core/stingray_renderer/output_nodes/standard_base"
                    title = "Standard"
                }
            ]
            version = 2
        }
    "#;

    fn resolved() -> Resolution {
        let graph = Graph::from_material(MATERIAL).unwrap().unwrap();
        let mut defs = BTreeMap::new();
        for (path, text) in [
            ("core/shader_nodes/if", SWITCH),
            ("core/shader_nodes/constant_vector3", CONSTANT),
            ("core/stingray_renderer/output_nodes/standard_base", OUTPUT),
        ] {
            defs.insert(path.to_string(), NodeDef::from_text(text).unwrap());
        }
        let mut shader_inputs = BTreeMap::new();
        shader_inputs.insert(
            "00000000-0000-0000-0000-000000000001".to_string(),
            "base_color".to_string(),
        );
        graph.resolve(&defs, &shader_inputs).unwrap()
    }

    fn input(node: &ResolvedNode, name: &str) -> Source {
        node.inputs
            .iter()
            .find(|(input, _)| input == name)
            .map(|(_, source)| source.clone())
            .unwrap_or_else(|| panic!("{name} is not an input of {}", node.id))
    }

    #[test]
    fn a_graph_resolves_its_wiring() {
        let resolution = resolved();
        let switch = resolution
            .nodes
            .iter()
            .find(|node| node.id == "aaaa")
            .expect("the switch node");
        assert_eq!(switch.options, vec!["OP_EQUAL"]);
        assert_eq!(input(switch, "input_a"), Source::Node("bbbb".to_string()));
        assert_eq!(input(switch, "input_b"), Source::Unbound);
        assert!(switch.code.contains("RESULT(input_a)"));
        assert_eq!(
            resolution.outputs,
            vec![GraphOutput {
                name: "base_color".to_string(),
                source: Source::Node("aaaa".to_string()),
                domain: Domain::Pixel,
            }]
        );
        assert!(resolution.nodes.iter().any(|node| node.output));
    }

    #[test]
    fn a_graph_evaluates_its_nodes() {
        let graph = Graph::from_material(MATERIAL).unwrap().unwrap();
        let mut defs = BTreeMap::new();
        for (path, text) in [
            ("core/shader_nodes/if", SWITCH),
            ("core/shader_nodes/constant_vector3", CONSTANT),
            ("core/stingray_renderer/output_nodes/standard_base", OUTPUT),
        ] {
            defs.insert(path.to_string(), NodeDef::from_text(text).unwrap());
        }
        let mut shader_inputs = BTreeMap::new();
        shader_inputs.insert(
            "00000000-0000-0000-0000-000000000001".to_string(),
            "base_color".to_string(),
        );
        let resolution = graph.resolve(&defs, &shader_inputs).unwrap();
        let evaluation = resolution.evaluate(&defs).unwrap();

        // Nothing feeds a vertex input, so the vertex stage evaluates nothing.
        assert!(evaluation.vertex.is_empty(), "{}", evaluation.vertex);

        // The constant node declares its value and its input's literal.
        assert!(
            evaluation.pixel.contains("float3 node_0;"),
            "{}",
            evaluation.pixel
        );
        assert!(
            evaluation.pixel.contains("float3 a = { 1.0, 0.5, 0.0 };"),
            "{}",
            evaluation.pixel
        );
        // The switch takes its option as a define and writes its output.
        assert!(evaluation.pixel.contains("#define OP_EQUAL"));
        assert!(evaluation.pixel.contains("#undef OP_EQUAL"));
        assert!(
            evaluation.pixel.contains("node_1 = input_a;"),
            "{}",
            evaluation.pixel
        );
        // The graph's output lands in the results.
        assert!(
            evaluation.pixel.contains("results.base_color = node_1;"),
            "{}",
            evaluation.pixel
        );
    }

    #[test]
    fn an_unconnected_input_carries_the_instance_value() {
        let resolution = resolved();
        let constant = resolution
            .nodes
            .iter()
            .find(|node| node.id == "bbbb")
            .expect("the constant node");
        let value = input(constant, "a");
        let Source::Value(value) = &value else {
            panic!("expected a value, got {value:?}");
        };
        assert_eq!(value.hlsl(false).as_deref(), Some("{ 1.0, 0.5, 0.0 }"));
    }

    #[test]
    fn a_whitespace_separated_list_parses() {
        #[derive(Deserialize)]
        struct Sample {
            a: NodeValue,
        }
        let sample: Sample = serde_sjson::from_str("a = [\n1\n0.5\n0\n]\n").unwrap();
        assert_eq!(
            sample.a,
            NodeValue::List(vec![
                NodeValue::Number(1.0),
                NodeValue::Number(0.5),
                NodeValue::Number(0.0)
            ])
        );
    }
}
