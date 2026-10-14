# File Type - Flow

A `flow` (`0x92d3ee038eeb610d`) is a compiled flow graph - the game's visual scripting.

**1,303 of 1,303** shipped flows re-encode byte for byte, and the same through SJSON, across
23,209 nodes.

## The layout

```text
header: 23 u32 words
  0  always 0
  1  node count              2  node index offset
  3  event count             4  events offset
  5  extra pair count        6  variable count        7  variables offset
  8  query offset            9  preload pair count    10 preload offset
  11 subroutine count        12 subroutines offset
  13..16 the four list counts       17..20 their offsets
  21 the flow part's size    22 the dynamic part's size
then, in this order and 8-aligned where noted:
  node index (u32 per node) | events {u32 id, node | in << 16} | extra pairs |
  variables {u32 x 3} | query {count, u32..} [pad 8] | preload pairs |
  subroutines | four u32 lists [pad 8] | nodes {type, query, payload}
```

The writer **rebuilds** that order - section order, alignment, node placement - and derives every
count and offset rather than copying the header, so a round trip is byte-exact by construction, and
`examples/flow_probe.rs` is what says so.

A node whose index entry repeats the previous offset is an **index entry without a record** - the
reference counts 328 of those across the game's flows, and the sweep finds **5,203** of them. It is
carried as `empty` rather than as a zero-length node.

## The unit's flow is the same structure

The reference works on a *unit*'s flow, which a unit carries as two arrays; a `flow` resource is
those two concatenated, and the arithmetic says so: the sampled 352-byte resource declares a
272-byte flow part (`header[21]`) and an 80-byte dynamic part (`header[22]`), and `272 + 80 = 352`.

## Grounding

The community compiler's DT reader and writer (`C:\dev\Darktide-Asset-Compiler`,
`stingray/flow/flow_resource.cpp`), whose `decode` and `encode` are documented as exact inverses
for every flow the game ships.

## Open

The **dynamic part** is carried as hex, as the reference does: it is variable storage and constants
whose meaning lives in the nodes, and guessing at it would be invention. Every node word is likewise
carried - the reference names the node types, but that is a description, not a layout.
