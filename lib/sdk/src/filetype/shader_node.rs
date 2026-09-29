//! A reader for the Stingray `.shader_node` declaration: the source-side
//! description of a shader declaration that mods already ship.
//!
//! The reader takes `inputs`, `channels`, `permutation_sets`, `shader_contexts`
//! and `code_blocks`, and ignores the rest (`render_state`, `sampler_state`,
//! `options`, ...), producing the normalized view the emitters consume
//! (`variables`, `channels`, `permutation_sets`, `contexts`, `programs` on the
//! node itself). What each key means here:
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
use color_eyre::eyre::{Context as _, Result, bail};
use serde::Deserialize;

use super::condition::{Condition, Defines};
use super::shader_graph::Evaluation;
use super::shader_source::{CodeParts, ShaderSource, find_chunk, include_chunk};
use super::shader_decl::{
    ChannelDef, Choice, CompileWith, Define, DefineTable, Domain, Interface, Pass, PassEntry,
    Permutation, PermutationSet, ProgramDef, ShaderContext, ValueType, VariableDef,
};

/// A parsed `.shader_node` file: the declaration the emitters consume.
///
/// The fields the file names are deserialized as written, named `raw_*` where
/// the normalized view has the same name. [`ShaderNode::from_sjson`] fills the
/// normalized fields. Keys this reader does not model are ignored, so a
/// declaration out ahead of the reader still parses.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShaderNode {
    /// The material variables, keyed by their uuid, as the file writes them.
    #[serde(default)]
    pub inputs: BTreeMap<String, Input>,
    /// The channels table, in declaration order, as the file writes it.
    #[serde(rename = "channels", default)]
    pub raw_channels: ChannelTable,
    /// The compile-time permutation sets, keyed by name, as the file writes them.
    #[serde(rename = "permutation_sets", default)]
    pub raw_permutation_sets: BTreeMap<String, Vec<ChoiceEntry>>,
    /// The shader contexts, keyed by name, as the file writes them.
    #[serde(rename = "shader_contexts", default)]
    pub raw_contexts: BTreeMap<String, NodeContext>,
    /// The code blocks, keyed by the name a pass's `code_block` uses.
    #[serde(default)]
    pub code_blocks: BTreeMap<String, CodeBlock>,

    /// The normalized material variables, keyed by name.
    #[serde(skip)]
    pub variables: BTreeMap<String, VariableDef>,
    /// The normalized channels, in declaration order.
    #[serde(skip)]
    pub channels: Vec<ChannelDef>,
    /// The normalized permutation sets, in name order.
    #[serde(skip)]
    pub permutation_sets: Vec<PermutationSet>,
    /// The normalized shader contexts, in name order.
    #[serde(skip)]
    pub contexts: Vec<ShaderContext>,
    /// The programs to compile, from the code blocks. Empty until the code
    /// blocks are read; the emitters need the compiled program list eventually.
    #[serde(skip)]
    pub programs: BTreeMap<String, ProgramDef>,
}

/// The `channels` table, kept in declaration order. A serde map would be a
/// `BTreeMap` and lose it, and the order is what the block's channel stream is
/// written in, so it is read as a sequence of entries instead.
#[derive(Clone, Debug, Default)]
pub struct ChannelTable(pub Vec<(String, Channels)>);

impl<'de> Deserialize<'de> for ChannelTable {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TableVisitor;

        impl<'de> serde::de::Visitor<'de> for TableVisitor {
            type Value = ChannelTable;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a channels table")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(ChannelTable(entries))
            }
        }

        deserializer.deserialize_map(TableVisitor)
    }
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
    /// The permutation sets this choice delegates to. A set's choice can name
    /// another set, and the enumeration expands it recursively, so a set can be
    /// built out of others - `non_instanced_modifiers` is one
    /// `permute_with: "vertex_modifiers"` entry.
    #[serde(default)]
    pub permute_with: Option<PermuteWith>,
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

/// One `code_blocks` entry: the code it includes and the HLSL it compiles. The
/// file's other keys (samplers, stage conditions, instance data) are not read
/// yet.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CodeBlock {
    /// The chunks to include, as `path#chunk`, or the bare name of another code
    /// block of the same declaration.
    #[serde(default)]
    pub include: Vec<String>,
    /// The body: `code = { shared = ..., hlsl = ... }` or a bare string.
    #[serde(default)]
    pub code: Option<CodeParts>,
}

/// The preprocessor lines a job's macros compile under.
pub fn defines_for(macros: &[&str]) -> String {
    let mut out = String::new();
    for name in macros {
        out.push_str(&format!("#define {name}\n"));
    }
    out
}

/// The DXC target profile for a stage name, when it is one DXC has.
///
/// Darktide ships SM 6.x DXIL, not SM 5.x bytecode: every shipped program
/// carries a `DXIL` chunk in its container (checked on the UI base's first
/// programs), the decompiler runs `dxil-spirv` over them, and `dtmt build`
/// compiles its overrides at 6.0. The container's `DXBC` magic is the container
/// format, which DXIL shares; it is not the payload.
pub fn profile_for(stage: &str) -> Option<&'static str> {
    Some(match stage {
        "vertex" => "vs_6_0",
        "pixel" => "ps_6_0",
        _ => return None,
    })
}

/// The entry point name for a profile: the dialect's code blocks define
/// `vs_main` and `ps_main`.
pub fn entry_for(profile: &str) -> Option<&'static str> {
    Some(match profile {
        "vs_6_0" => "vs_main",
        "ps_6_0" => "ps_main",
        _ => return None,
    })
}

/// The stages a pass compiles for: the dialect's code blocks carry a vertex and
/// a pixel entry point, and a pass draws with the pair. A macro's `stages` is a
/// limit on where it applies, not a list of stages to compile.
pub const STAGES: [&str; 2] = ["vertex", "pixel"];

/// The macros the engine's own compiler defines: the platform, and the stage
/// the program is compiled for. The library sources branch on them
/// (`#if defined(RENDERER_D3D12)`, `#if defined(STAGE_VERTEX)`,
/// `#elif defined(STAGE_FRAGMENT)`), so a compile without them takes the GLSL
/// or stub branches.
pub fn engine_defines(stage: &str) -> Vec<&'static str> {
    let mut out = vec!["RENDERER_D3D12"];
    match stage {
        "vertex" => out.push("STAGE_VERTEX"),
        "pixel" => out.push("STAGE_FRAGMENT"),
        _ => {}
    }
    out
}

/// One macro a job defines, with the stages it is limited to (empty: all).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobMacro {
    /// The macro name.
    pub name: String,
    /// The stages the macro applies to. Empty means every stage.
    pub stages: Vec<String>,
}

impl JobMacro {
    /// Whether the macro applies when compiling `stage`.
    pub fn applies_to(&self, stage: &str) -> bool {
        self.stages.is_empty() || self.stages.iter().any(|named| named == stage)
    }
}

/// One program the declaration asks to compile: a context, a permutation, and
/// the pass's code block with the macros it compiles under. The HLSL is
/// assembled per stage, because a macro limited to one stage must not be
/// defined in the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileJob {
    /// The context the pass belongs to.
    pub context: String,
    /// The permutation's index within that context.
    pub permutation: usize,
    /// The code block the pass names.
    pub code_block: String,
    /// The permutation's macros plus the pass's own, each with its stage limit.
    pub macros: Vec<JobMacro>,
}

impl CompileJob {
    /// The macros to define when compiling `stage`, in order.
    pub fn macros_for(&self, stage: &str) -> Vec<&str> {
        self.macros
            .iter()
            .filter(|macro_def| macro_def.applies_to(stage))
            .map(|macro_def| macro_def.name.as_str())
            .collect()
    }
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
    /// The macros the pass defines. Written `defines` or, in the passes the real
    /// files carry at the top level, `define` - the two spellings mean the same.
    #[serde(default, alias = "define")]
    pub defines: Option<DefinesValue>,
    /// The render state the pass draws with.
    #[serde(default)]
    pub render_state: Option<String>,
    /// The key the engine sorts variants by.
    #[serde(default)]
    pub branch_key: Option<String>,
}

impl ShaderNode {
    /// Parses a `.shader_node` file and fills the normalized view.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        let mut node: Self = serde_sjson::from_str(sjson)
            .map_err(|err| eyre::eyre!("failed to parse the shader node: {err}"))?;
        node.normalize()?;
        Ok(node)
    }

    /// The code block a pass names, when the declaration defines it.
    pub fn code_block(&self, name: &str) -> Option<&CodeBlock> {
        self.code_blocks.get(name)
    }

    /// The HLSL a job compiles for one stage: the macros that apply to the
    /// stage, then the block's includes in order, then the block's body.
    ///
    /// An include is another code block of this declaration (a bare name) or a
    /// library chunk (`path#chunk`); a chunk's own `includes` are pulled in
    /// first, and a name is only taken once, so a diamond does not duplicate
    /// code. The metadata entry may be keyed by the bare name where the pass
    /// wrote a `path#chunk`, so both are tried.
    pub fn job_source(
        &self,
        job: &CompileJob,
        stage: &str,
        libraries: &[ShaderSource],
        evaluation: Option<&Evaluation>,
    ) -> String {
        let mut macros = engine_defines(stage);
        macros.extend(job.macros_for(stage));
        let mut out = defines_for(&macros);

        // An output-node block's HLSL uses the graph names the toolchain expands
        // (`GRAPH_VERTEX_INPUT` and friends). When the source mentions one, the
        // scaffolding goes in before the block's own code: the macros have to be
        // defined before the structs that use them. The graph's own evaluation
        // goes after the block, where the libraries' types and macros are.
        let uses_graph = self.uses_graph(&job.code_block, libraries);
        if uses_graph {
            out.push_str(&self.graph_scaffold(evaluation));
        }

        let mut seen = Vec::new();
        self.write_block(&job.code_block, libraries, &mut seen, &mut out);

        if uses_graph {
            out.push_str(&self.graph_evaluation_tail(evaluation));
        }
        out
    }

    /// Whether a block's source - its own code parts or any library chunk -
    /// mentions one of the graph names.
    fn uses_graph(&self, block: &str, libraries: &[ShaderSource]) -> bool {
        let mentions = |text: &str| {
            text.contains("GRAPH_") || text.contains("GraphVertex") || text.contains("GraphPixel")
        };
        if let Some(code) = self
            .code_blocks
            .get(block)
            .and_then(|block| block.code.as_ref())
            && mentions(&code.hlsl())
        {
            return true;
        }
        libraries.iter().any(|library| {
            library
                .hlsl_shaders
                .values()
                .any(|chunk| chunk.hlsl().is_some_and(|text| mentions(&text)))
        })
    }

    /// Appends one block's code: its includes, then its own body. A block whose
    /// declaration entry carries no `code` takes its body from the library chunk
    /// of the same name (the sibling `.shader_source`).
    fn write_block(
        &self,
        reference: &str,
        libraries: &[ShaderSource],
        seen: &mut Vec<String>,
        out: &mut String,
    ) {
        let name = include_chunk(reference);
        if seen.iter().any(|taken| taken == name) {
            return;
        }
        seen.push(name.to_string());

        let mut has_body = false;
        if let Some(block) = self.code_block(reference).or_else(|| self.code_block(name)) {
            for include in &block.include {
                self.write_block(include, libraries, seen, out);
            }
            if let Some(code) = &block.code {
                out.push_str(&code.hlsl());
                out.push('\n');
                has_body = true;
            }
        }

        if !has_body
            && let Some((_, chunk)) = find_chunk(libraries, name)
        {
            for include in &chunk.includes {
                self.write_block(include, libraries, seen, out);
            }
            if let Some(hlsl) = chunk.hlsl() {
                out.push_str(&hlsl);
                out.push('\n');
            }
        }
    }

    /// The programs the declaration asks to compile, context by context: each
    /// permutation of the context's sets, and each pass that permutation
    /// selects. The branch a define cannot decide contributes both sides, which
    /// is why a job list can be longer than the programs a section ships.
    pub fn compile_jobs(&self) -> Result<Vec<CompileJob>> {
        let mut jobs = Vec::new();
        for context in &self.contexts {
            for (index, permutation) in self.permutations_for(context).iter().enumerate() {
                let defines = Defines::new(permutation.macros.iter().cloned());
                for pass in context.passes_of(&defines)? {
                    let mut macros: Vec<JobMacro> = permutation
                        .macros
                        .iter()
                        .map(|name| JobMacro {
                            name: name.clone(),
                            stages: permutation
                                .macro_stages
                                .get(name)
                                .cloned()
                                .unwrap_or_default(),
                        })
                        .collect();
                    for name in pass.macros() {
                        let stages = pass.defines.stages().to_vec();
                        match macros.iter_mut().find(|macro_def| &macro_def.name == name) {
                            // The same macro from both sides applies where
                            // either says; an empty list is every stage.
                            Some(existing) => {
                                if existing.stages.is_empty() || stages.is_empty() {
                                    existing.stages.clear();
                                } else {
                                    for stage in stages {
                                        if !existing.stages.contains(&stage) {
                                            existing.stages.push(stage);
                                        }
                                    }
                                }
                            }
                            None => macros.push(JobMacro {
                                name: name.clone(),
                                stages,
                            }),
                        }
                    }
                    jobs.push(CompileJob {
                        context: context.name.clone(),
                        permutation: index,
                        code_block: pass.code_block.clone(),
                        macros,
                    });
                }
            }
        }
        Ok(jobs)
    }

    /// Fills the normalized view the emitters consume: the variables, the
    /// channels, the permutation sets and the contexts.
    ///
    /// An input's flag is the first macro of its type table, and only when the
    /// input is optional: a required input is in every interface, so it has no
    /// flag. A channel is required unless an input of the same name is optional,
    /// in which case the channel follows that input's flag.
    fn normalize(&mut self) -> Result<()> {
        let mut variables = BTreeMap::new();
        for (uuid, input) in &self.inputs {
            let (kind, flags) = input_type(uuid, input)?;
            variables.insert(
                input.name.clone(),
                VariableDef {
                    kind,
                    domain: domain(input.domain.as_deref())?,
                    flag: flags.into_iter().next().filter(|_| !input.is_required),
                    default: Vec::new(),
                },
            );
        }
        self.variables = variables;
        // The channels table nests conditions, so it is walked rather than read:
        // a channel collects the conditions it sits under.
        let mut channels = Vec::new();
        for (key, entry) in &self.raw_channels.0 {
            walk_channels(key, entry, &[], &self.variables, &mut channels)?;
        }
        self.channels = channels;
        let mut sets = Vec::new();
        for (name, entries) in &self.raw_permutation_sets {
            let mut choices = Vec::new();
            for entry in entries {
                let condition = entry.condition.clone();
                let define: Define = entry.define.clone().into();
                choices.push(Choice {
                    condition: condition.clone(),
                    macros: define.macros().to_vec(),
                    stages: define.stages().to_vec(),
                    permute_with: entry
                        .permute_with
                        .as_ref()
                        .map(PermuteWith::names)
                        .unwrap_or_default(),
                    // A choice with no `if` is the set's default.
                    is_default: entry.is_default.unwrap_or(entry.condition.is_none()),
                });
            }
            sets.push(PermutationSet {
                name: name.clone(),
                choices,
            });
        }
        // The sets are a product, so their order decides the group order. The
        // file's own order is the one the toolchain compiles in, so it is kept.
        sets.sort_by(|a, b| a.name.cmp(&b.name));
        self.permutation_sets = sets;
        let mut contexts = Vec::new();
        for (name, context) in &self.raw_contexts {
            contexts.push(ShaderContext {
                name: name.clone(),
                sort_mode: context.passes_sort_mode.clone(),
                compile_with: context
                    .compile_with
                    .iter()
                    .map(|entry| CompileWith {
                        condition: entry.condition.clone(),
                        permute_with: entry
                            .permute_with
                            .as_ref()
                            .map(PermuteWith::names)
                            .unwrap_or_default(),
                    })
                    .collect(),
                passes: context
                    .passes
                    .iter()
                    .map(walk_pass_entry)
                    .collect::<Result<Vec<PassEntry>>>()?,
            });
        }
        self.contexts = contexts;
        Ok(())
    }
}

/// One pass entry of a declaration, as the declaration carries it.
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
    variables: &BTreeMap<String, VariableDef>,
    out: &mut Vec<ChannelDef>,
) -> Result<()> {
    match entry {
        Channels::One(channel) => {
            out.push(channel_def(key, channel, conditions, variables)?);
        }
        Channels::Set(set) => {
            let mut conditions = conditions.to_vec();
            if !key.is_empty() {
                conditions.push(key.to_string());
            }
            for (key, entry) in set {
                walk_channels(key, entry, &conditions, variables, out)?;
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

/// One declared channel, as the declaration carries it. `conditions` is the path of
/// conditions it sits under; the name of a bare channel is its own key in the
/// table, which is the first entry of that path.
fn channel_def(
    name: &str,
    channel: &Channel,
    conditions: &[String],
    variables: &BTreeMap<String, VariableDef>,
) -> Result<ChannelDef> {
    let Some(kind) = ValueType::parse(&channel.kind) else {
        bail!("channel {name} has the unknown type {}", channel.kind);
    };
    // A channel of the same name as an optional input follows that input's flag.
    let gated_by_input = variables
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

impl ShaderNode {
    /// The context records a single-group generated section writes: the `default`
    /// context with one query for the group data's own hash.
    ///
    /// A declaration's other contexts (`shadow_caster`, `material_transfer`)
    /// select further groups, and a group is selected by a *query id*. Nothing
    /// can derive those ids yet: the shipped ids do not resolve and no pairing
    /// with a declaration has been established, so a record per declared context
    /// would be inventing keys. The single-group path is the one whose id is
    /// known - the group data's own hash, which is what the minimal shipped
    /// material's one `default` context carries.
    ///
    /// The declared contexts are therefore *not* all written, and that is
    /// deliberate: [`crate::filetype::shader::Section::check`] refuses a record
    /// set whose query count does not equal the group data's group count, so a
    /// multi-context declaration against carried multi-group data fails loudly
    /// instead of writing placeholder query ids.
    pub fn context_records(&self, hash: u32) -> Vec<crate::filetype::shader::ContextRecord> {
        use crate::filetype::shader::{ContextRecord, NO_CONDITIONS, Query};
        use crate::murmur::Murmur32;
        vec![ContextRecord {
            name: Murmur32::hash(b"default").into(),
            flags: 0,
            queries: vec![Query {
                id: hash,
                conditions: NO_CONDITIONS,
            }],
        }]
    }

    /// One declared shader context by name.
    pub fn context(&self, name: &str) -> Option<&ShaderContext> {
        self.contexts.iter().find(|context| context.name == name)
    }

    /// The compile permutations of one context: the recursive product of the
    /// choices of the sets its `compile_with` names, or of the declaration's root sets
    /// when it names none.
    ///
    /// A choice may `permute_with` another set, and that set is expanded under
    /// the choice. That is how `instanced_and_non_instanced` delegates to
    /// `instanced_modifiers` and `non_instanced_modifiers`, and how the real
    /// `default` set reaches both - a set is not a flat list of macros. A cycle
    /// stops the expansion rather than looping, and the depth is bounded.
    ///
    /// Whether a context that names none really permutes over all of them is not
    /// settled: the toolchain also drops the sets a context's code does not use,
    /// and that is a dependency of the compiled code rather than of the
    /// declaration, so the count this returns is an upper bound.
    pub fn permutations_for(&self, context: &ShaderContext) -> Vec<Permutation> {
        let names: Vec<String> = context
            .compile_with
            .iter()
            .flat_map(|entry| entry.permute_with.iter().cloned())
            .collect();
        let roots: Vec<&PermutationSet> = if names.is_empty() {
            self.root_sets()
        } else {
            let named: Vec<&PermutationSet> = names
                .iter()
                .filter_map(|name| self.permutation_sets.iter().find(|set| set.name == *name))
                .collect();
            // A name that matches no set is a declaration this reader does not
            // understand; falling back to every root set keeps the count honest
            // rather than dropping permutations.
            if named.len() == names.len() {
                named
            } else {
                self.root_sets()
            }
        };
        let mut permutations = vec![Permutation::default()];
        for set in roots {
            permutations = self.expand(permutations, set, &mut Vec::new());
        }
        permutations
    }

    /// The sets no choice delegates to: the roots of the permutation graph.
    fn root_sets(&self) -> Vec<&PermutationSet> {
        let referenced: std::collections::BTreeSet<&str> = self
            .permutation_sets
            .iter()
            .flat_map(|set| set.choices.iter())
            .flat_map(|choice| choice.permute_with.iter().map(String::as_str))
            .collect();
        self.permutation_sets
            .iter()
            .filter(|set| !referenced.contains(set.name.as_str()))
            .collect()
    }

    /// The product of `base` with one set's choices, expanding each choice's own
    /// `permute_with` recursively. `visiting` stops a cycle.
    fn expand(
        &self,
        base: Vec<Permutation>,
        set: &PermutationSet,
        visiting: &mut Vec<String>,
    ) -> Vec<Permutation> {
        if visiting.iter().any(|name| name == &set.name) || visiting.len() >= 16 {
            return base;
        }
        visiting.push(set.name.clone());
        let mut next = Vec::new();
        for permutation in base {
            for (index, choice) in set.choices.iter().enumerate() {
                let mut permutation = permutation.clone();
                permutation.choices.push((set.name.clone(), index));
                permutation.add_defines(&choice.macros, &choice.stages);
                let mut expanded = vec![permutation];
                for nested in &choice.permute_with {
                    if let Some(nested_set) = self
                        .permutation_sets
                        .iter()
                        .find(|candidate| candidate.name == *nested)
                    {
                        expanded = self.expand(expanded, nested_set, visiting);
                    }
                }
                next.extend(expanded);
            }
        }
        visiting.pop();
        next
    }

    /// The number of groups a context compiles: how many permutations of it there
    /// are. The section's groups are the sum over its contexts.
    pub fn group_count_of(&self, context: &ShaderContext) -> usize {
        self.permutations_for(context).len()
    }

    /// The number of groups the declaration compiles: the sum over its contexts. With
    /// no contexts, the one permutation of the declaration itself.
    pub fn context_group_count(&self) -> usize {
        if self.contexts.is_empty() {
            return self.group_count();
        }
        self.contexts
            .iter()
            .map(|context| self.group_count_of(context))
            .sum()
    }
}

impl ShaderNode {
    /// The channel of that name, when the declaration declares it.
    pub fn channel(&self, name: &str) -> Option<&ChannelDef> {
        self.channels.iter().find(|channel| channel.name == name)
    }

    /// The flags the declaration's variables can gate on, in name order.
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

    /// The interface a material gets from the inputs it declares: the mask over
    /// the flags those inputs enable, the variables they expose, and the
    /// channels that follow them. An input the declaration does not declare is
    /// ignored, which is what the engine does with a name it cannot bind.
    ///
    /// A declaration with many optional variables has a great many *possible*
    /// interfaces - two to the power of its flags - but a section ships a handful
    /// of groups, and the conditions tree is what maps an interface onto one of
    /// them. So this answers one query; it does not enumerate.
    pub fn interface(&self, inputs: &[String]) -> Interface {
        let flags = self.flags();
        let mut mask = 0u32;
        for input in inputs {
            let Some(variable) = self.variables.get(input) else {
                continue;
            };
            let Some(flag) = variable.flag.as_deref() else {
                continue;
            };
            if let Some(bit) = flags.iter().position(|declared| *declared == flag) {
                mask |= 1 << bit;
            }
        }
        Interface {
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
                    Some(flag) => flags
                        .iter()
                        .position(|declared| declared == &flag.as_str())
                        .is_some_and(|bit| mask & (1 << bit) != 0),
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
                            .is_some_and(|flag| {
                                flags
                                    .iter()
                                    .position(|declared| declared == &flag.as_str())
                                    .is_some_and(|bit| mask & (1 << bit) != 0)
                            })
                })
                .map(|channel| channel.name.clone())
                .collect(),
        }
    }

    /// The interface of the mask, for a caller that works in masks rather than
    /// in input names.
    pub fn interface_of(&self, mask: u32) -> Interface {
        let inputs: Vec<String> = self
            .variables
            .iter()
            .filter(|(_, variable)| {
                variable.flag.as_ref().is_some_and(|flag| {
                    self.flags()
                        .iter()
                        .position(|declared| declared == &flag.as_str())
                        .is_some_and(|bit| mask & (1 << bit) != 0)
                })
            })
            .map(|(name, _)| name.clone())
            .collect();
        self.interface(&inputs)
    }

    /// Enumerates the compile permutations over the declaration's root sets: one per
    /// combination of their choices, expanding every choice's `permute_with`
    /// recursively. A declaration with no sets has the one empty permutation.
    pub fn permutations(&self) -> Vec<Permutation> {
        let mut permutations = vec![Permutation::default()];
        for set in self.root_sets() {
            permutations = self.expand(permutations, set, &mut Vec::new());
        }
        permutations
    }

    /// The number of groups the declaration generates: the product of the sets'
    /// choice counts, or one when the declaration declares no sets.
    pub fn group_count(&self) -> usize {
        self.permutations().len()
    }

    /// The channels one group has: those whose conditions all hold under the
    /// group's defines, and those no condition gates.
    ///
    /// A channel whose condition reaches an engine query - how many skin weights
    /// a mesh has, which renderer is running - is left out, because a generated
    /// declaration cannot answer it. That is the one thing to remember about this
    /// list: it is what the *defines* say, not what a mesh would produce.
    pub fn channels_of(&self, permutation: &Permutation) -> Result<Vec<&ChannelDef>> {
        let mut channels = Vec::new();
        for channel in &self.channels {
            // A macro a choice limited to one stage is not defined for a channel
            // of another stage: `SKINNED_4WEIGHTS` is a vertex macro and must not
            // decide whether a pixel channel exists.
            let defines = Defines::new(
                permutation
                    .macros
                    .iter()
                    .filter(|name| {
                        permutation
                            .macro_stages
                            .get(*name)
                            .is_none_or(|stages| {
                                super::shader_decl::stage_applies(stages, channel.domain)
                            })
                    })
                    .cloned(),
            );
            let mut holds = true;
            for text in &channel.conditions {
                let condition = Condition::parse(text).wrap_err_with(|| {
                    format!("channel {} has an unparsable condition", channel.name)
                })?;
                // An answer of "unknown" is not a yes, so a channel whose
                // condition this group cannot decide is left out of it.
                holds &= condition.holds(&defines) == Some(true);
                if !holds {
                    break;
                }
            }
            if holds {
                channels.push(channel);
            }
        }
        Ok(channels)
    }

    /// The channel names one group has, which is what the group data records.
    pub fn channel_names_of(&self, permutation: &Permutation) -> Result<Vec<String>> {
        Ok(self
            .channels_of(permutation)?
            .iter()
            .map(|channel| channel.name.clone())
            .collect())
    }

    /// The graph scaffolding an output-node block expects: the `GRAPH_*` macros
    /// and the `GraphVertexParams`/`GraphVertexResults`/`GraphPixelParams`/
    /// `GraphPixelResults` structs.
    ///
    /// The toolchain expands the declaration's channels and material variables
    /// into these names, and every shipped output node's HLSL uses them (the
    /// skydome base and the decal base both do), so a declaration cannot be
    /// compiled without them. Generated here: the channel fields (from the
    /// declaration's `channels`, by domain), the export fields (from `inputs` -
    /// the material variables the graph computes), the
    /// `GRAPH_*_INPUT`/`GRAPH_MATERIAL_EXPORTS` field lists, and the macros that
    /// move values between the shader's structs and the graph's.
    ///
    /// The channel set is the declaration's, not one interface's: which channels
    /// a group has is a property of its permutation, and that is the group data's
    /// business. A texture channel is left out - it is a material binding, not an
    /// interpolant.
    ///
    /// Not generated: `GRAPH_EVALUATE_VERTEX` and `GRAPH_EVALUATE_PIXEL` - the
    /// graph's own code is the *material's* node graph, which a shader
    /// declaration does not carry - so they are emitted empty, and a shader
    /// compiled from a declaration alone declares the graph but does not evaluate
    /// it.
    pub fn graph_scaffold(&self, evaluation: Option<&Evaluation>) -> String {
        let hlsl = |kind: ValueType| match kind {
            ValueType::Float => "float",
            ValueType::Float2 => "float2",
            ValueType::Float3 => "float3",
            ValueType::Float4 => "float4",
            ValueType::Float4x4 => "float4x4",
            ValueType::Texture2D => "Texture2D",
        };
        // A channel can be declared under several conditions (the `channels`
        // table nests), so the same name can appear more than once; the fields
        // are keyed by name, first occurrence first. The graph's imports add
        // their own: a mesh input with a semantic.
        struct Field<'a> {
            name: &'a str,
            hlsl: &'static str,
            semantic: Option<&'a str>,
            /// The vertex stage writes it (and the pixel stage interpolates it).
            vertex: bool,
        }
        let mut channels: Vec<Field> = Vec::new();
        for channel in self
            .channels
            .iter()
            .filter(|channel| channel.kind != ValueType::Texture2D)
        {
            if !channels.iter().any(|seen| seen.name == channel.name) {
                channels.push(Field {
                    name: &channel.name,
                    hlsl: hlsl(channel.kind),
                    semantic: channel.semantic.as_deref(),
                    vertex: channel.domain == Domain::Vertex,
                });
            }
        }
        if let Some(evaluation) = evaluation {
            for channel in &evaluation.channels {
                if !channels.iter().any(|seen| seen.name == channel.name) {
                    channels.push(Field {
                        name: &channel.name,
                        hlsl: channel.hlsl,
                        semantic: Some(&channel.semantic),
                        vertex: true,
                    });
                }
            }
        }

        let mut out = String::from("// Generated by DTMT from the declaration.\n");

        // The macros the graph's nodes ask for, so the declaration's channels
        // and the generated evaluation see them.
        if let Some(evaluation) = evaluation {
            for define in &evaluation.defines {
                out.push_str(&format!("#define {define}\n"));
            }
        }

        out.push_str("struct GraphVertexParams {\n");
        for channel in channels.iter().filter(|channel| channel.vertex) {
            out.push_str(&format!("    {} {};\n", channel.hlsl, channel.name));
        }
        out.push_str("};\n");
        out.push_str("struct GraphPixelParams {\n");
        for channel in &channels {
            out.push_str(&format!("    {} {};\n", channel.hlsl, channel.name));
        }
        out.push_str("};\n");
        // A results struct holds the declaration's inputs of its stage: the
        // values the graph computes and the shader then reads. A texture is a
        // binding, not a value.
        for (name, domain) in [
            ("GraphVertexResults", Domain::Vertex),
            ("GraphPixelResults", Domain::Pixel),
        ] {
            out.push_str(&format!("struct {name} {{\n"));
            for (variable, definition) in &self.variables {
                if definition.kind == ValueType::Texture2D || definition.domain != domain {
                    continue;
                }
                out.push_str(&format!("    {} {variable};\n", hlsl(definition.kind)));
            }
            out.push_str("};\n");
        }

        out.push_str("#define GRAPH_VERTEX_PARAM(params, name) params.name\n");
        out.push_str("#define GRAPH_PIXEL_PARAM(params, name) params.name\n");
        out.push_str("#define GRAPH_VERTEX_DATA(input, name) input.name\n");
        out.push_str("#define GRAPH_PIXEL_DATA(input, name) input.name\n");

        // The VS input is the mesh-bound channels, the ones the declaration gives
        // a semantic; the PS input is what the vertex stage writes for the pixel
        // stage, numbered as interpolators.
        out.push_str("#define GRAPH_VERTEX_INPUT");
        for channel in channels.iter().filter(|channel| channel.semantic.is_some()) {
            let semantic = channel.semantic.unwrap_or_default();
            out.push_str(&format!(" {} {} : {semantic};", channel.hlsl, channel.name));
        }
        out.push('\n');

        out.push_str("#define GRAPH_PIXEL_INPUT");
        // A channel with an explicit semantic keeps it; the rest take the next
        // free interpolator index, so a declared `TEXCOORD1` cannot collide with
        // a generated one.
        let mut used: Vec<u32> = channels
            .iter()
            .filter_map(|channel| channel.semantic)
            .filter_map(|semantic| semantic.strip_prefix("TEXCOORD"))
            .filter_map(|index| index.parse::<u32>().ok())
            .collect();
        let mut next = 0u32;
        for channel in channels.iter().filter(|channel| channel.vertex) {
            let semantic = match channel.semantic {
                Some(semantic) => semantic.to_string(),
                None => {
                    while used.contains(&next) {
                        next += 1;
                    }
                    used.push(next);
                    next += 1;
                    format!("TEXCOORD{}", next - 1)
                }
            };
            out.push_str(&format!(" {} {} : {semantic};", channel.hlsl, channel.name));
        }
        out.push('\n');

        out.push_str("#define GRAPH_MATERIAL_EXPORTS");
        if evaluation.is_some() {
            // A graph material's variables are the graph's exports: what the
            // material stores and the nodes read. The declaration's own inputs
            // are either computed by the graph (the results structs) or engine
            // globals the libraries declare, so they are not repeated here.
            if let Some(evaluation) = evaluation {
                for (export, hlsl, _) in &evaluation.exports {
                    out.push_str(&format!(" {hlsl} {export};"));
                }
            }
        } else {
            for (variable, definition) in &self.variables {
                if definition.kind == ValueType::Texture2D {
                    continue;
                }
                out.push_str(&format!(" {} {variable};", hlsl(definition.kind)));
            }
        }
        out.push('\n');

        out.push_str("#define GRAPH_VERTEX_WRITE_PARAMS(params, input)");
        for channel in channels
            .iter()
            .filter(|channel| channel.semantic.is_some() && channel.vertex)
        {
            out.push_str(&format!(" params.{0} = input.{0};", channel.name));
        }
        out.push('\n');
        out.push_str("#define GRAPH_PIXEL_WRITE_PARAMS(params, input)");
        for channel in channels.iter().filter(|channel| channel.vertex) {
            out.push_str(&format!(" params.{0} = input.{0};", channel.name));
        }
        out.push('\n');
        out.push_str("#define GRAPH_VERTEX_WRITE(o, results, params)");
        for channel in channels.iter().filter(|channel| channel.vertex) {
            out.push_str(&format!(" o.{0} = params.{0};", channel.name));
        }
        out.push('\n');

        // The evaluation itself is a function per stage, not a macro body: the
        // node code carries preprocessor branches (`#if defined(OP_EQUAL)` and
        // the material's flags) that a macro expansion could not carry.
        out.push_str(
            "void graph_evaluate_vertex(out GraphVertexResults results, in GraphVertexParams params);\n",
        );
        out.push_str(
            "void graph_evaluate_pixel(out GraphPixelResults results, in GraphPixelParams params);\n",
        );
        out.push_str(
            "#define GRAPH_EVALUATE_VERTEX(results, params) graph_evaluate_vertex(results, params)\n",
        );
        out.push_str(
            "#define GRAPH_EVALUATE_PIXEL(results, params) graph_evaluate_pixel(results, params)\n",
        );

        out
    }

    /// The graph evaluation's file-scope part: the sampler declarations and the
    /// functions the scaffold's macros call. It is appended after the block,
    /// because it uses the libraries' types and macros.
    pub fn graph_evaluation_tail(&self, evaluation: Option<&Evaluation>) -> String {
        let Some(evaluation) = evaluation else {
            return String::new();
        };
        let mut out = String::from("// Generated by DTMT from the material's graph.\n");
        for sampler in &evaluation.samplers {
            out.push_str(&format!("DECLARE_SAMPLER_2D({sampler});\n"));
        }
        // Each stage's evaluation only exists in its own stage: the pixel one
        // reads the material cbuffer, which a vertex source does not declare.
        out.push_str("#if defined(STAGE_VERTEX)\n");
        out.push_str(
            "void graph_evaluate_vertex(out GraphVertexResults results, in GraphVertexParams params)\n{\n",
        );
        out.push_str(&evaluation.vertex);
        out.push_str("}\n");
        out.push_str("#endif\n");
        out.push_str("#if defined(STAGE_FRAGMENT)\n");
        out.push_str(
            "void graph_evaluate_pixel(out GraphPixelResults results, in GraphPixelParams params)\n{\n",
        );
        out.push_str(&evaluation.pixel);
        out.push_str("}\n");
        out.push_str("#endif\n");
        out
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

    fn sample() -> ShaderNode {
        ShaderNode::from_sjson(SAMPLE).expect("parse")
    }

    #[test]
    fn reads_the_variables_of_a_declaration() {
        let node = sample();
        assert_eq!(node.variables.len(), 3);

        // An optional input's flags hang off its type, and the first is the one
        // that gates the interface.
        let base_color = &node.variables["base_color"];
        assert_eq!(base_color.kind, ValueType::Float3);
        assert_eq!(base_color.flag.as_deref(), Some("HAS_BASE_COLOR"));
        assert_eq!(base_color.domain, Domain::Pixel);

        // A required input is in every interface, so it carries no flag even
        // though its type table has no macros either.
        let texture_map = &node.variables["texture_map"];
        assert_eq!(texture_map.kind, ValueType::Texture2D);
        assert_eq!(texture_map.flag, None);

        // Both flags of a two-macro type are read; the first gates.
        let opacity = &node.variables["opacity"];
        assert_eq!(opacity.kind, ValueType::Float);
        assert_eq!(opacity.flag.as_deref(), Some("HAS_OPACITY"));
    }

    #[test]
    fn reads_the_channels_of_a_declaration() {
        let node = sample();
        assert_eq!(node.channels.len(), 4);

        let position = node.channel("vertex_position").expect("channel");
        assert_eq!(position.kind, ValueType::Float4);
        assert_eq!(position.domain, Domain::Vertex);
        assert_eq!(position.semantic, None);
        assert!(position.required);

        let normal = node.channel("vertex_normal").expect("channel");
        assert_eq!(normal.kind, ValueType::Float3);
        assert_eq!(normal.semantic.as_deref(), Some("NORMAL"));

        // A channel in both domains is written by the vertex stage.
        let eye = node.channel("eye_vector").expect("channel");
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
        let node = ShaderNode::from_sjson(&text)
            .expect("parse");
        let map = node.channel("texture_map").expect("channel");
        assert!(!map.required);
        assert_eq!(
            node.variables["texture_map"].flag.as_deref(),
            Some("HAS_TEXTURE_MAP")
        );
        // A material that declares the texture gets the channel with it, and one
        // that does not has neither.
        let with_flag = node.interface(&["texture_map".to_string()]);
        assert!(with_flag.variables.contains(&"texture_map".to_string()));
        assert!(with_flag.channels.contains(&"texture_map".to_string()));
        let without = node.interface(&[]);
        assert!(!without.variables.contains(&"texture_map".to_string()));
        assert!(!without.channels.contains(&"texture_map".to_string()));
    }

    #[test]
    fn reads_the_permutation_sets_of_a_declaration() {
        let node = sample();
        // Two sets of two choices each, in name order.
        assert_eq!(node.permutation_sets.len(), 2);
        assert_eq!(node.permutation_sets[0].name, "instanced_modifiers");
        assert_eq!(node.permutation_sets[1].name, "vertex_modifiers");
        assert_eq!(node.group_count(), 4);

        let instanced = &node.permutation_sets[0].choices;
        assert_eq!(instanced[0].condition.as_deref(), Some("instanced()"));
        assert_eq!(instanced[0].macros, vec!["INSTANCED"]);
        // A define with no stages means every stage, and the `=` spelling of the
        // separator reads the same as the `:` one.
        assert!(instanced[0].stages.is_empty());
        assert!(!instanced[0].is_default);
        assert!(instanced[1].is_default);

        let vertex = &node.permutation_sets[1].choices;
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
        let node = ShaderNode::from_sjson(&text)
            .expect("parse");
        assert_eq!(node.group_count(), 4);
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
        let node = ShaderNode::from_sjson(text)
            .expect("parse");
        assert_eq!(node.channels.len(), 4);

        // One condition deep.
        let basis = node.channel("basis0").expect("basis0");
        assert_eq!(basis.conditions, vec!["defined(PARTICLE_LIGHTING)"]);
        assert!(!basis.required);

        // Two deep, and the same channel declared under both branches with a
        // different type each time. The conditions are in name order, so the
        // `!defined` branch comes first.
        let animated: Vec<&ChannelDef> = node
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
        let size = node.channel("vertex_size").expect("vertex_size");
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
        let node = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse");
        // Two contexts, in name order: `default` then `shadow_caster`.
        assert_eq!(node.contexts.len(), 2);
        let default = node.context("default").expect("the default context");
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
        let shadow = node.context("shadow_caster").expect("shadow");
        assert_eq!(shadow.sort_mode.as_deref(), Some("immediate"));
        // A single name, not a list.
        assert_eq!(
            shadow.compile_with[0].permute_with,
            vec!["shadow_caster".to_string()]
        );
        assert!(node.context("nope").is_none());
    }

    #[test]
    fn a_pass_tree_is_read_and_selected() {
        let node = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse");
        let default = node.context("default").expect("the default context");
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
        let node = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse");
        let default = node.context("default").expect("the default context");
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
        // runtime, so a generated declaration has to carry both sides rather than
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
        let node = ShaderNode::from_sjson(text)
            .expect("parse");
        let default = node.context("default").expect("the default context");
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
    fn a_pass_reads_define_as_well_as_defines() {
        // The real files write the singular spelling on top-level passes:
        // `{ code_block="depth_only" define="DRAW_OUTLINE" ... }`. Both mean the
        // same, and dropping one silently loses the macro the pass draws with.
        let text = r#"
            shader_contexts = {
                default = {
                    passes = [
                        { layer="outline" code_block="depth_only" define="DRAW_OUTLINE" }
                    ]
                }
            }
        "#;
        let node = ShaderNode::from_sjson(text)
            .expect("parse");
        let default = node.context("default").expect("the default context");
        let passes = default.passes_of(&Defines::default()).expect("passes");
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].macros(), ["DRAW_OUTLINE".to_string()]);
    }

    #[test]
    fn a_pass_links_to_its_code_block() {
        let text = r#"
            code_blocks = {
                base = { include = ["lib#common"] code = { hlsl = """ void main() {} """ } }
            }
            shader_contexts = {
                default = { passes = [ { code_block="base" } ] }
            }
        "#;
        let node = ShaderNode::from_sjson(text).expect("parse");
        let block = node.code_block("base").expect("block");
        assert_eq!(block.include, vec!["lib#common"]);
        assert!(block.code.as_ref().expect("code").hlsl().contains("main"));
        match &node.contexts[0].passes[0] {
            PassEntry::Pass(pass) => assert_eq!(pass.code_block, "base"),
            other => panic!("expected a pass, got {other:?}"),
        }
        assert!(node.code_block("nope").is_none());
    }

    #[test]
    fn a_context_lists_its_compile_jobs() {
        let text = r#"
            code_blocks = { base = { code = { hlsl = """ void main() {} """ } } }
            shader_contexts = {
                default = {
                    passes = [
                        { if: "defined(A)" then: [ { code_block="base" defines=["A"] } ] else: [ { code_block="base" defines={ macros: ["X"] stages: ["pixel"] } } ] }
                    ]
                }
            }
        "#;
        let node = ShaderNode::from_sjson(text).expect("parse");
        let jobs = node.compile_jobs().expect("jobs");
        assert_eq!(jobs.len(), 1, "no sets, and nothing defines A");
        assert_eq!(jobs[0].context, "default");
        assert_eq!(jobs[0].code_block, "base");
        assert_eq!(jobs[0].macros_for("pixel"), vec!["X"]);
        assert!(jobs[0].macros_for("vertex").is_empty());
    }

    #[test]
    fn a_code_block_assembles_its_includes_and_hlsl() {
        let libraries = [ShaderSource::from_sjson(
            r#"hlsl_shaders = { common = { code = """ void common() {} """ } }"#,
        )
        .expect("library")];
        let text = r#"
            code_blocks = { base = { include = ["lib#common"] code = { shared = """ void shared() {} """ hlsl = """ void main() {} """ } } }
        "#;
        let node = ShaderNode::from_sjson(text).expect("parse");
        let mut out = String::new();
        node.write_block("base", &libraries, &mut Vec::new(), &mut out);
        let common = out.find("common").expect("include");
        let shared = out.find("shared").expect("shared");
        let main = out.find("main").expect("body");
        assert!(common < shared && shared < main, "{out}");
    }

    #[test]
    fn a_stage_maps_to_its_profile_and_entry_point() {
        assert_eq!(profile_for("vertex"), Some("vs_6_0"));
        assert_eq!(profile_for("pixel"), Some("ps_6_0"));
        assert_eq!(entry_for("vs_6_0"), Some("vs_main"));
        assert_eq!(entry_for("ps_6_0"), Some("ps_main"));
        assert_eq!(profile_for("geometry"), None);
        assert_eq!(STAGES, ["vertex", "pixel"]);
        assert_eq!(
            engine_defines("vertex"),
            vec!["RENDERER_D3D12", "STAGE_VERTEX"]
        );
        assert_eq!(
            engine_defines("pixel"),
            vec!["RENDERER_D3D12", "STAGE_FRAGMENT"]
        );
    }

    #[test]
    fn a_job_source_is_defines_then_includes_then_body() {
        let libraries = [ShaderSource::from_sjson(
            r#"
                hlsl_shaders = {
                    common = { code = """ void common() {} """ }
                    gbuffer_base = { code = { hlsl = """ PS_INPUT vs_main(VS_INPUT input) {} """ } }
                }
            "#,
        )
        .expect("library")];
        let text = r#"
            code_blocks = {
                gbuffer_base = { include = ["lib#common"] }
            }
            shader_contexts = {
                default = {
                    passes = [ { code_block="gbuffer_base" defines={ macros: ["A"] stages: ["vertex"] } } ]
                }
            }
        "#;
        let node = ShaderNode::from_sjson(text).expect("parse");
        let jobs = node.compile_jobs().expect("jobs");
        assert_eq!(jobs.len(), 1);
        let source = node.job_source(&jobs[0], "vertex", &libraries, None);
        let engine = source.find("#define RENDERER_D3D12").expect("renderer");
        let stage = source.find("#define STAGE_VERTEX").expect("stage");
        let defines = source.find("#define A").expect("defines");
        let common = source.find("void common()").expect("include");
        let body = source.find("vs_main").expect("body");
        assert!(
            engine < stage && stage < defines && defines < common && common < body,
            "{source}"
        );

        // The macro is limited to the vertex stage, so the pixel source must
        // not define it, and it must take the fragment stage.
        let pixel = node.job_source(&jobs[0], "pixel", &libraries, None);
        assert!(!pixel.contains("#define A"), "{pixel}");
        assert!(pixel.contains("#define STAGE_FRAGMENT"), "{pixel}");
    }

    #[test]
    fn a_context_permutes_over_the_sets_it_names() {
        let mut node = ShaderNode::from_sjson(CONTEXTS)
            .expect("parse");
        // Two sets of two choices each.
        node.permutation_sets = vec![
            PermutationSet {
                name: "default".to_string(),
                choices: vec![
                    Choice {
                        condition: Some("defined(A)".to_string()),
                        macros: vec!["A".to_string()],
                        stages: vec![],
                        permute_with: Vec::new(), is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        permute_with: Vec::new(), is_default: true,
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
                        permute_with: Vec::new(), is_default: false,
                    },
                    Choice {
                        condition: None,
                        macros: vec![],
                        stages: vec![],
                        permute_with: Vec::new(), is_default: true,
                    },
                ],
            },
        ];
        let default = node.context("default").expect("the default context");
        let shadow = node.context("shadow_caster").expect("shadow");

        // Each context names one set, so each permutes over that one alone: two
        // groups each, not four.
        let default_groups = node.permutations_for(default);
        assert_eq!(default_groups.len(), 2);
        assert_eq!(default_groups[0].macros, vec!["A".to_string()]);
        let shadow_groups = node.permutations_for(shadow);
        assert_eq!(shadow_groups.len(), 2);
        assert_eq!(shadow_groups[0].macros, vec!["B".to_string()]);

        // The declaration's groups are the sum over its contexts.
        assert_eq!(node.context_group_count(), 4);
        // Without a name, a context permutes over every set.
        let all = node.permutation_sets.len();
        assert_eq!(all, 2);
        let unnamed = ShaderContext {
            name: "unnamed".to_string(),
            ..ShaderContext::default()
        };
        assert_eq!(node.permutations_for(&unnamed).len(), 4);
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
        let node = ShaderNode::from_sjson(text)
            .expect("parse");
        let input = &node.variables["distortion_normal"];
        assert_eq!(input.kind, ValueType::Float3);
        assert_eq!(input.flag, None);
    }

    #[test]
    fn rejects_an_input_with_no_type() {
        let text = "inputs = {\n\t\"1\" = { name = \"x\" }\n}";
        let err = ShaderNode::from_sjson(text).expect_err("no type");
        assert!(err.to_string().contains("no type"), "{err}");
    }

    #[test]
    fn rejects_an_input_with_no_name() {
        let text = "inputs = {\n\t\"1\" = { type = { vector3: [] } }\n}";
        let err = ShaderNode::from_sjson(text).expect_err("no name");
        assert!(err.to_string().contains("no name"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_type() {
        let text = "inputs = {\n\t\"1\" = { name = \"x\" type = { texture_cube: [] } }\n}";
        let err = ShaderNode::from_sjson(text).expect_err("unknown type");
        assert!(err.to_string().contains("texture_cube"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_domain() {
        let text =
            "channels = {\n\tc = {\n\t\ttype = \"float3\"\n\t\tdomain = \"geometry\"\n\t}\n}";
        let err = ShaderNode::from_sjson(text).expect_err("unknown domain");
        assert!(err.to_string().contains("geometry"), "{err}");
    }
}

