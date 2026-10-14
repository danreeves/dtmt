//! The `flow` resource: a compiled flow (visual-script) graph.
//!
//! 1,303 of these ship, and the layout comes from the community compiler's DT reader
//! and writer (`C:\dev\Darktide-Asset-Compiler`, `stingray/flow/flow_resource.cpp`),
//! whose `decode` and `encode` are documented as exact inverses for every flow the
//! game ships. That reference works on a *unit*'s flow, which the unit carries as two
//! arrays; a `flow` resource is those two concatenated, and the arithmetic says so:
//! the sampled 352-byte resource has a flow part of 272 (`header[21]`) and a dynamic
//! part of 80 (`header[22]`), and `272 + 80 = 352`.
//!
//! ```text
//! header: 23 u32 words
//!   0  always 0
//!   1  node count              2  node index offset
//!   3  event count             4  events offset
//!   5  extra pair count        6  variable count        7  variables offset
//!   8  query offset            9  preload pair count    10 preload offset
//!   11 subroutine count        12 subroutines offset
//!   13..16 the four list counts       17..20 their offsets
//!   21 the flow part's size    22 the dynamic part's size
//! then, in this order and 8-aligned where noted:
//!   node index (u32 per node) | events {u32 id, node | in << 16} | extra pairs |
//!   variables {u32 x 3} | query {count, u32..} [pad 8] | preload pairs |
//!   subroutines | four u32 lists [pad 8] | nodes {type, query, payload}
//! ```
//!
//! The writer rebuilds that order exactly, deriving every count and offset, so a
//! round trip is byte-exact; the sweep in `examples/flow_probe.rs` is what says so.
//! A node whose index entry repeats the previous offset is an **index entry without a
//! record** - the reference counts 328 of those across the game's flows - and is
//! carried as `empty` rather than as a zero-length node.
//!
//! The dynamic part is carried as hex, as the reference does: it is variable storage
//! and constants whose meaning lives in the nodes, and guessing at it would be
//! invention. Every node word is likewise carried; the reference names the node types
//! but that is a description, not a layout.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type id, for callers that need to recognise the payload.
pub const FLOW_TYPE: u64 = 0x92d3_ee03_8eeb_610d;

/// The header is 23 `u32` words.
const HEADER_WORDS: usize = 23;
const HEADER: usize = HEADER_WORDS * 4;

/// The header's word indices, named so the reader below reads as prose.
const NODES: usize = 1;
const NODE_INDEX: usize = 2;
const EVENTS: usize = 3;
const EVENTS_OFFSET: usize = 4;
const EXTRA: usize = 5;
const VARIABLES: usize = 6;
const VARIABLES_OFFSET: usize = 7;
const QUERY_OFFSET: usize = 8;
const PRELOAD: usize = 9;
const PRELOAD_OFFSET: usize = 10;
const SUBROUTINES: usize = 11;
const SUBROUTINES_OFFSET: usize = 12;
const LIST_COUNTS: usize = 13;
const LIST_OFFSETS: usize = 17;
const FLOW_SIZE: usize = 21;
const DYNAMIC_SIZE: usize = 22;

/// An external event: an id, the node it enters and which input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvent {
    pub event: u32,
    pub node: u32,
    pub input: u32,
}

/// One node: a type, a query offset, and its payload words.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// An index entry with no record of its own.
    #[serde(default, skip_serializing_if = "is_false")]
    pub empty: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub node_type: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub query: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<u32>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("a flow's dynamic data has an odd number of hex digits");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| color_eyre::eyre::eyre!("a flow's dynamic data is not hex"))
        })
        .collect()
}

/// A compiled flow graph.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    /// The size of the dynamic part, which the header declares.
    pub dynamic_size: u32,
    pub external_events: Vec<ExternalEvent>,
    pub extra: Vec<[u32; 2]>,
    pub variables: Vec<[u32; 3]>,
    pub query: Vec<u32>,
    pub preload: Vec<[u32; 2]>,
    pub subroutines: Vec<u32>,
    /// The four list tables.
    pub lists: [Vec<u32>; 4],
    pub nodes: Vec<Node>,
    /// The dynamic part, hex-encoded.
    pub dynamic_data: String,
}

fn word(b: &[u8], at: usize) -> Result<u32> {
    let bytes = b
        .get(at..at + 4)
        .ok_or_else(|| color_eyre::eyre::eyre!("a flow ends inside the word at {at}"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

impl Flow {
    /// Reads a flow from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER {
            bail!("a flow is shorter than its {HEADER}-byte header");
        }
        let mut h = [0u32; HEADER_WORDS];
        for (i, word) in h.iter_mut().enumerate() {
            *word = word_of(bytes, i)?;
        }
        let inside = |offset: usize, size: usize| offset <= bytes.len() && size <= bytes.len() - offset;

        if h[0] != 0 {
            bail!("a flow's first header word is {}, not zero", h[0]);
        }
        let flow_size = h[FLOW_SIZE] as usize;
        let dynamic_size = h[DYNAMIC_SIZE] as usize;
        // The resource is the flow part followed by the dynamic part, and this is the
        // equation that says so.
        if flow_size + dynamic_size != bytes.len() {
            bail!(
                "a flow declares {flow_size} flow bytes and {dynamic_size} dynamic bytes, which is not its {}",
                bytes.len()
            );
        }
        let nodes = h[NODES] as usize;
        let events = h[EVENTS] as usize;
        let extra = h[EXTRA] as usize;
        let variables = h[VARIABLES] as usize;
        let preload = h[PRELOAD] as usize;
        let subroutines = h[SUBROUTINES] as usize;
        if !inside(h[NODE_INDEX] as usize, 4 * nodes)
            || !inside(h[EVENTS_OFFSET] as usize, 8 * (events + extra))
            || !inside(h[VARIABLES_OFFSET] as usize, 12 * variables)
            || !inside(h[QUERY_OFFSET] as usize, 4)
            || !inside(h[PRELOAD_OFFSET] as usize, 8 * preload)
            || !inside(h[SUBROUTINES_OFFSET] as usize, 4 * subroutines)
        {
            bail!("a flow table lies outside the blob");
        }
        let query_offset = h[QUERY_OFFSET] as usize;
        let query_count = word(bytes, query_offset)? as usize;
        if !inside(query_offset + 4, 4 * query_count) {
            bail!("a flow query table lies outside the blob");
        }

        let mut external_events = Vec::with_capacity(events);
        for i in 0..events {
            let at = h[EVENTS_OFFSET] as usize + 8 * i;
            let target = word(bytes, at + 4)?;
            external_events.push(ExternalEvent {
                event: word(bytes, at)?,
                node: target & 0xffff,
                input: target >> 16,
            });
        }
        let pairs = |offset: usize, i: usize| -> Result<[u32; 2]> {
            let at = offset + 8 * i;
            Ok([word(bytes, at)?, word(bytes, at + 4)?])
        };
        let mut extra_pairs = Vec::with_capacity(extra);
        for i in 0..extra {
            extra_pairs.push(pairs(h[EVENTS_OFFSET] as usize + 8 * events, i)?);
        }
        let mut variable_rows = Vec::with_capacity(variables);
        for i in 0..variables {
            let at = h[VARIABLES_OFFSET] as usize + 12 * i;
            variable_rows.push([word(bytes, at)?, word(bytes, at + 4)?, word(bytes, at + 8)?]);
        }
        let query = (0..query_count)
            .map(|i| word(bytes, query_offset + 4 + 4 * i))
            .collect::<Result<Vec<u32>>>()?;
        let mut preload_rows = Vec::with_capacity(preload);
        for i in 0..preload {
            preload_rows.push(pairs(h[PRELOAD_OFFSET] as usize, i)?);
        }
        let subroutine_rows = (0..subroutines)
            .map(|i| word(bytes, h[SUBROUTINES_OFFSET] as usize + 4 * i))
            .collect::<Result<Vec<u32>>>()?;
        let mut lists: [Vec<u32>; 4] = Default::default();
        for k in 0..4 {
            let count = h[LIST_COUNTS + k] as usize;
            let offset = h[LIST_OFFSETS + k] as usize;
            if !inside(offset, 4 * count) {
                bail!("a flow list lies outside the blob");
            }
            lists[k] = (0..count)
                .map(|i| word(bytes, offset + 4 * i))
                .collect::<Result<Vec<u32>>>()?;
        }

        let mut node_list = Vec::with_capacity(nodes);
        for i in 0..nodes {
            let start = word(bytes, h[NODE_INDEX] as usize + 4 * i)? as usize;
            let end = if i + 1 < nodes {
                word(bytes, h[NODE_INDEX] as usize + 4 * (i + 1))? as usize
            } else {
                flow_size
            };
            if end == start {
                node_list.push(Node {
                    empty: true,
                    node_type: 0,
                    query: 0,
                    words: Vec::new(),
                });
                continue;
            }
            if end < start + 8 || end > flow_size || (end - start) % 4 != 0 {
                bail!("a flow node {i} has no valid extent");
            }
            let count = (end - start - 8) / 4;
            node_list.push(Node {
                empty: false,
                node_type: word(bytes, start)?,
                query: word(bytes, start + 4)?,
                words: (0..count)
                    .map(|k| word(bytes, start + 8 + 4 * k))
                    .collect::<Result<Vec<u32>>>()?,
            });
        }

        Ok(Self {
            dynamic_size: h[DYNAMIC_SIZE],
            external_events,
            extra: extra_pairs,
            variables: variable_rows,
            query,
            preload: preload_rows,
            subroutines: subroutine_rows,
            lists,
            nodes: node_list,
            dynamic_data: hex_encode(&bytes[flow_size..]),
        })
    }

    /// Writes the flow back, rebuilding the layout and deriving every count and
    /// offset.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let dynamic = hex_decode(&self.dynamic_data)?;
        if dynamic.len() != self.dynamic_size as usize {
            bail!(
                "a flow declares {} dynamic bytes but carries {}",
                self.dynamic_size,
                dynamic.len()
            );
        }
        let mut out = vec![0u8; HEADER];
        let node_index = out.len();
        out.resize(out.len() + 4 * self.nodes.len(), 0);
        let events_offset = out.len();
        for event in &self.external_events {
            push(&mut out, event.event);
            push(&mut out, event.node | (event.input << 16));
        }
        for pair in &self.extra {
            push(&mut out, pair[0]);
            push(&mut out, pair[1]);
        }
        let variables_offset = out.len();
        for row in &self.variables {
            for value in row {
                push(&mut out, *value);
            }
        }
        let query_offset = out.len();
        push(&mut out, self.query.len() as u32);
        for value in &self.query {
            push(&mut out, *value);
        }
        pad8(&mut out);
        let preload_offset = out.len();
        for pair in &self.preload {
            push(&mut out, pair[0]);
            push(&mut out, pair[1]);
        }
        let subroutines_offset = out.len();
        for value in &self.subroutines {
            push(&mut out, *value);
        }
        let mut list_offsets = [0u32; 4];
        for (k, list) in self.lists.iter().enumerate() {
            list_offsets[k] = out.len() as u32;
            for value in list {
                push(&mut out, *value);
            }
        }
        pad8(&mut out);
        for (i, node) in self.nodes.iter().enumerate() {
            if node.empty {
                let offset = out.len() as u32;
                out[node_index + 4 * i..node_index + 4 * i + 4].copy_from_slice(&offset.to_le_bytes());
                continue;
            }
            pad8(&mut out);
            let offset = out.len() as u32;
            out[node_index + 4 * i..node_index + 4 * i + 4].copy_from_slice(&offset.to_le_bytes());
            push(&mut out, node.node_type);
            push(&mut out, node.query);
            for value in &node.words {
                push(&mut out, *value);
            }
        }

        let flow_size = out.len() as u32;
        let header: [u32; HEADER_WORDS] = [
            0,
            self.nodes.len() as u32,
            node_index as u32,
            self.external_events.len() as u32,
            events_offset as u32,
            self.extra.len() as u32,
            self.variables.len() as u32,
            variables_offset as u32,
            query_offset as u32,
            self.preload.len() as u32,
            preload_offset as u32,
            self.subroutines.len() as u32,
            subroutines_offset as u32,
            self.lists[0].len() as u32,
            self.lists[1].len() as u32,
            self.lists[2].len() as u32,
            self.lists[3].len() as u32,
            list_offsets[0],
            list_offsets[1],
            list_offsets[2],
            list_offsets[3],
            flow_size,
            self.dynamic_size,
        ];
        for (i, value) in header.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&dynamic);
        Ok(out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

fn word_of(b: &[u8], index: usize) -> Result<u32> {
    word(b, index * 4)
}

fn push(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pad8(out: &mut Vec<u8>) {
    out.resize((out.len() + 7) & !7, 0);
}

/// Decompiles a `flow` variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a flow resource needs a variant");
    };
    let flow = Flow::from_binary(variant.data())?;
    Ok(vec![UserFile::new(flow.to_sjson()?.into_bytes())])
}

/// Compiles a `flow` SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let flow = Flow::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(flow.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::Flow);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A flow with two events, one variable, one query word, one of each list, two
    /// nodes (one of them an index entry without a record) and 8 dynamic bytes.
    fn sample() -> Flow {
        Flow {
            dynamic_size: 8,
            external_events: vec![
                ExternalEvent {
                    event: 0x7b4a514a,
                    node: 4,
                    input: 0,
                },
                ExternalEvent {
                    event: 0x85918a60,
                    node: 5,
                    input: 1,
                },
            ],
            extra: vec![[1, 2]],
            variables: vec![[3, 4, 5]],
            query: vec![6, 7],
            preload: vec![[8, 9]],
            subroutines: vec![10],
            lists: [vec![11], vec![], vec![12], vec![]],
            nodes: vec![
                Node {
                    empty: true,
                    node_type: 0,
                    query: 0,
                    words: vec![],
                },
                Node {
                    empty: false,
                    node_type: 13,
                    query: 14,
                    words: vec![15, 16],
                },
            ],
            dynamic_data: "0102030405060708".to_string(),
        }
    }

    #[test]
    fn a_sample_flow_round_trips() {
        let flow = sample();
        let bytes = flow.to_binary().unwrap();
        let read = Flow::from_binary(&bytes).expect("the sample parses");
        assert_eq!(read, flow, "the model round-trips");
        assert_eq!(read.to_binary().unwrap(), bytes, "byte-for-byte");
        // The header's own arithmetic: the flow part plus the dynamic part is the file.
        let flow_size = word(&bytes, FLOW_SIZE * 4).unwrap() as usize;
        let dynamic_size = word(&bytes, DYNAMIC_SIZE * 4).unwrap() as usize;
        assert_eq!(flow_size + dynamic_size, bytes.len());
        assert_eq!(word(&bytes, NODE_INDEX * 4).unwrap(), HEADER as u32);
    }

    #[test]
    fn a_sample_flow_survives_sjson() {
        let flow = sample();
        let sjson = flow.to_sjson().unwrap();
        assert_eq!(Flow::from_sjson(&sjson).unwrap(), flow);
    }

    #[test]
    fn a_dynamic_size_that_disagrees_with_the_data_is_rejected() {
        let mut flow = sample();
        flow.dynamic_size = 9;
        assert!(flow.to_binary().is_err());
    }

    #[test]
    fn a_header_that_does_not_describe_the_blob_is_rejected() {
        let bytes = sample().to_binary().unwrap();
        let mut broken = bytes.clone();
        broken[FLOW_SIZE * 4] ^= 0xFF;
        assert!(Flow::from_binary(&broken).is_err());
        let mut short = bytes;
        short.truncate(HEADER - 4);
        assert!(Flow::from_binary(&short).is_err());
    }
}
