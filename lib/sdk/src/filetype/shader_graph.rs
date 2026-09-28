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
    /// The instance's own values for inputs nothing feeds, keyed by input name.
    #[serde(default, flatten)]
    pub values: BTreeMap<String, NodeValue>,
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
    /// The output's type, usually `{ typeof: "<input name>" }`.
    #[serde(default)]
    pub output: NodeOutput,
    /// The option uuids the definition's code switches on, keyed by uuid.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The HLSL body: reads the input names, calls `RESULT(<expr>)`.
    #[serde(default)]
    pub code: String,
    /// The values the node exports to the material, keyed by export name.
    #[serde(default)]
    pub exports: BTreeMap<String, NodeExport>,
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
    /// The definition's code.
    pub code: String,
    /// Whether the node is the output node (the shader declaration).
    pub output: bool,
}

/// A resolved graph: every node's wiring and the graph's outputs.
#[derive(Clone, Debug)]
pub struct Resolution {
    /// The nodes, the output node last.
    pub nodes: Vec<ResolvedNode>,
    /// The output node's inputs: `(shader input name, source)`.
    pub outputs: Vec<(String, Source)>,
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
            nodes.push(ResolvedNode {
                id: node.id.clone(),
                kind: node.kind.clone(),
                title: node.title.clone(),
                options,
                inputs,
                code: def.code.clone(),
                output: node.id == output_id,
            });
        }

        // The graph's outputs: the output node's connectors, named by the
        // declaration's input table.
        let feeds = self.feeds(&output_id);
        let mut outputs = Vec::new();
        for (connector, instance) in &feeds {
            let name = shader_inputs
                .get(connector)
                .cloned()
                .unwrap_or_else(|| connector.clone());
            outputs.push((name, Source::Node(instance.clone())));
        }

        Ok(Resolution { nodes, outputs })
    }
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
            vec![("base_color".to_string(), Source::Node("aaaa".to_string()))]
        );
        assert!(resolution.nodes.iter().any(|node| node.output));
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
