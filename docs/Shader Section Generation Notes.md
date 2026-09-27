# Generating a shader section (plan)

This is the working plan for emitting a complete `shader43` section from our own
data instead of splicing a shipped one. It collects what each section needs and
marks what is still unknown. See `File Type - Material.-.md` for the field-level
notes.

## Target

**Everything is defined in the mod and DTMT only compiles.** A mod should ship
shader sources (`.hlsl`), the shader's declarations (which cbuffers, resources,
variables and defaults it uses, and which material channels it answers), and the
material SJSON - with **no `shader_data` at all**. `dtmt build` compiles the
sources with `dxc` and generates the whole section: contexts, conditions, group
data, program tails, device data and default data.

Anything kept from a shipped shader must be reduced to genuinely engine-side
constants that cannot currently be derived from the shader itself. The working candidates
are:

| Piece | Where it should come from |
| --- | --- |
| Program tails | The compiled DXBC: cbuffer names/sizes and the signature names are all in the container, so the tails can be generated (the tail's remaining, undecoded fields are the open question) |
| Contexts / conditions | The mod's declared channels: the tree selects a group by which channels the material provides, so it can be generated for the mod's own channel set (the payload and the engine's query mechanism still have to be decoded) |
| Group data | The mod's declared cbuffers/variables: the variable tables are built from the declarations, the resource descriptors from the DXBC reflection |
| Default data | The mod's declared defaults (format decoded) |
| Device preamble | Generated from the mod's variables if it is the reflection table it looks like; otherwise reduced to an engine-constant blob kept in the toolchain |
| Header, pads, offsets | Recomputed |

Engine constants are only acceptable where the engine genuinely requires data
that cannot currently be derived from the shader - and even then the goal is to decode and
shrink them to the smallest possible form, not to grow them into a preset.

## Status of the intermediate route

The mod-defined build flow is in place: a material can declare
`shader_preset = "<name>.preset"` and ship no `shader_data` at all; `dtmt build`
compiles the sibling shader sources and generates the section from them and the
preset (byte-identical to the splice route), and snoopymod runs that way - its
base material is a few hundred bytes, the preset is the only game-derived file.
The preset holds the declaration's engine-side wrapper for now; shrinking it to only
genuine engine constants (and generating the rest from the shader itself) is the
remaining RE work listed above.

The SDK also has the template route
(`lib/sdk/examples/generate_shader.rs`, `shader_preset` module) used to extract
the preset and as the harness for testing each decoded piece. It proved the
pipeline end to end - a generated section (96 programs, ~431 KB, no shipped blob
in the mod) builds, deploys and renders in game (the title screen tint follows
Lua) - and the first attempt without the device-data preamble reached the title
and then hit the engine's out-of-memory error, which is how the preamble's
importance was found.

## What a material needs

A base material carries a `shader_data` blob (the `shader43` section) and a
`shader_size`. The section is:

```
header (12 words)
contexts        [contexts_offset, conditions_offset)
conditions      [conditions_offset, dependencies_offset)
dependencies    [dependency_offset, group_offset)
group data      [group_data_offset, +group_data_size)
device data     [device_data_offset, +device_data_size)
default data    [default_data_offset, section end)
```

The offsets are relative to the section start; the device and default blocks are
recomputable (that is how the splice flow already relocates them).

## Per section

| Section | Emit | Status |
| --- | --- | --- |
| Header | `{version=43, opaque, contexts_offset, context_count, conditions_offset, default_data_offset, dependency_offset, dependency_count, group_data_offset, group_data_size, device_data_offset, device_data_size}` | Known |
| Post-build | Recompute offsets and pads (4 bytes before the default data, 16 bytes at the end); update the material's `shader_size` | Known |
| Contexts | `{name, word2, count, count x {query_id, conditions_offset}}`, variable length, filling `[contexts_offset, conditions_offset)`; the queries are the groups. The ids are engine-side and copy from a template until a declaration-level pairing exists | Layout measured on seven sections; writable, ids copied |
| Conditions | Copy from a template: records are `{u16 tag=1, u16 payload_words, u16 payload_offset, u16 count}` + `count` hashes + `payload_words` payload words, and the framing is self-delimiting (`filetype::condition_tree`). The payload is a bytecode whose opcodes are mapped but not decoded | Framing decoded and writable; payload semantics open |
| Dependencies | Copy from the same template (8 bytes = one u64 id on the UI base) | Copyable only |
| Group data | Generate per group. Each group needs a header (`{u32, query_id_of_the_group, descriptor words, …}`), the variable tables for that group's cbuffers, and the compact copy of those tables that follows. The variable records are `{type, flags, name_hash, cbuffer_offset, size}` runs with a count word; copies must all be consistent | Structure mapped (see `Shader RE TODO.md`): a 32-byte global header then 36 groups; the channel table, variable table and packed run are byte-identical across all 36 groups, only the descriptors' `Y` and the byte-packed group header vary. The tables are identified: 69 records = the `global_viewport` engine cbuffer's variables, 7 records = the group's `c_per_object` variables (incl. `texture_map`). Generation = emit the tables once, replicate them across the template's group count, keep the template's descriptors/headers |
| Device data | Generate: a packed preamble followed by framed DXBC programs. Each program record is `envelope=1`, `frame_length`, Oodle frame, `metadata_kind=5`, decoded length, frame key, then the metadata tail. **The preamble matters**: a generated section without it makes the engine run out of memory as soon as a material using it is drawn (verified in game - the packed table is read as a lookup and garbage sizes follow), so `--generate` writes the preset's preamble before the records | The preamble's 120-byte header is decoded: only `+0x04` (group count), `+0x08` (cbuffer count) and `+0x0C` (record count + 8) vary across seven shipped declarations, the rest is constant; the byte-packed `{index, value}` records after it are engine-variable binding entries shared across declarations (22-record common prefix). Generation = copy the template's block and rewrite the three header words |
| Program tails | Still partly open. Cbuffer entries are 24-byte records whose `{name_hash, size}` sit at `+4`/`+12`, in register order; signature elements are listed by name hash with index/ordinal. What the engine does with the rest of the tail is unknown | Open |
| Default data | Generate: `{u32 zero}{u32 count}` then `count × {name_hash, element_count, blob_offset}` then the value blob, with `element_count` = 1/2/3/4 for scalar/vec2/vec3/vec4 and `blob_offset` = byte offset of the value in the blob | Known |

## Shape decisions for a first generated shader

- **Reuse one shader declaration per shader.** Pick the template base material whose
  resource model (bindless arrays, `c_per_object` layout, spaces) the shader
  needs, then emit our programs with the same interface. That is the current
  build flow's constraint and it stays until the tails' root-signature part is
  decoded.
- **Copy contexts + conditions + dependencies from the template.** They are
  engine-side permutation data: context query ids do not resolve through the
  dictionary, while the condition names are shader/channel names (`gui`, `red`,
  `green`, `blue`, `fog_volume`, `linear_depth`, ...). Copying them keeps the
  group selection working without decoding the tree fully.
- **Keep one group per generated shader**, using the template's first group and
  its `query_id` (the group header's second word), and emit that group's
  variable tables and compact copies.
- **Give the shader its own parameters where the template leaves room.** A
  variable at a slot the engine already fills can be renamed/reused; bytes in
  gaps are not uploaded, and a larger `c_per_object` is not honoured, so new
  per-shader parameters have to come either from replacing a shipped variable or
  from resources the engine owns (textures/samplers through the bindless
  arrays).

## Declarative target format (from the Stingray renderer mod)

`C:\dev\vmb\mods\Badgers\core\stingray_renderer` implements the Stingray renderer
and shows the shape a from-scratch declaration has to take. A `.shader_node`
declares a whole shader:

- `inputs` - the material interface: `name`, `type` (`scalar`, `vector3`, ...),
  `domain` (`vertex`/`pixel`) and the **permutation flag that enables the input**
  (`type = { vector3: ["HAS_BASE_COLOR"] }`). These are the `c_per_object`
  variables and, through the flags, the condition set.
- `channels` - the vertex -> pixel interpolants with `type`, `semantic` and
  `domain` (`vertex_position`, `vertex_normal : NORMAL`). These are the group's
  channel records and the tail's signature runs.
- `permutation_sets` - named sets of choices, each
  `{ if: <expression>, define: { macros, stages }, permute_with: <nested set> }`.
  The recursion enumerates the permutation space; the `if` expressions are over
  material properties (`is_any_material_variable_set(...)`,
  `lightmap_format() == ...`, `num_skin_weights() == 4`, `defined(TRANSPARENT)`).
- `shader_contexts` - per context: `compile_with` (the permutation set to
  enumerate) and `passes` (`code_block`, `defines`, `render_state`).
- `code_blocks` - the HLSL per pass, with `include`, `samplers`,
  `stage_conditions`.
- `render_state` / `sampler_state` - the fixed-function state.

Mapping to the `shader43` section:

| Declaration | Section |
| --- | --- |
| `shader_contexts` names | contexts |
| the `if` expressions of the permutation sets | the conditions decision tree - the condition hashes resolve to material properties (`gui`, `red`, `green`, `blue`, `alpha`), so the tree is derivable instead of copied |
| one permutation | one group (group count = permutations) + one program pair |
| `inputs` | the `c_per_object` variable table (name hash, type, domain) |
| `channels` | the channel/resource table, the block's channel records, the tail's signature runs |
| `code_blocks.code.hlsl` + defines | the programs |

So the conditions tree - the largest engine-side carry today - is *derivable* from
a declaration. What stays engine-side: the query mechanism (how the engine asks
about a material property), the bindless array/space conventions, the engine's
variable registry (name -> block index and cbuffer offset), the per-permutation
group headers, and the block header blob.

## Open items blocking a from-scratch shader

1. Program tails: the parts after the cbuffer/signature lists, and what the
   engine derives from them (root signature, register/space pairs).
2. Group data: the group header's own fields (the byte-packed words around the
   permutation hash) are still not decoded. The tables are mapped and
   byte-identical across the 36 groups, so generation can emit them once and keep
   the template's descriptors and headers; whether one group can replace a
   permutation set is then just a matter of the copied conditions agreeing.
3. Conditions: the payload encoding and what a leaf selects. (The *dependencies*
   section is now read and writable - see below.)
4. How a material chooses a context at runtime (which context query a material
   parameter answers), which decides whether a generated shader can ship a
   single context.

## The declaration front end (what the reader now covers)

`filetype::shader_node` reads a `.shader_node` into the normalized view the emitters
consume. It takes `inputs`, `channels`, `permutation_sets` and `shader_contexts`,
and ignores the rest, so a declaration out ahead of the reader still parses. All
fifteen real declarations in the Badgers mod read, and every condition in them
parses.

Three things the first reading got wrong, each found by running it against those
files:

- an input writes its type as a **flag table** (`type = { vector3: ["HAS_X"] }`)
  or as a **bare name** (`type = "vector3"`), depending on whether it has flags;
- the `channels` table **nests**: a condition gates a *set* of channels, and a
  set can hold further conditions, so a channel carries the whole path of
  conditions it sits under;
- `defines` comes in three spellings - a list, a bare name, or a table with
  `stages` - in both a permutation choice and a pass.

The toolchain's dialect is not SJSON, so `serde_sjson` is vendored
(`lib/serde_sjson`, a submodule of the fork) with four relaxations: `key: value`
separates as well as `key = value`, entries need not be on their own lines, a
key may be a quoted string, and a quoted string may run over several lines. The
third was a genuine upstream bug - `deserialize_identifier` took bare words only,
so any derived struct rejected `"macros":`. Every change only accepts more than
before, so the strict material files parse as they did.

### Groups are per context, not per declaration

`compile_with` names the permutation sets a context permutes over, and that is
what the group count is: `standard_base`'s two contexts permute two sets each,
four groups, where the product over every set of the declaration is sixteen. A
`permute_with` may nest - a list of entries that each name a set again, so the
block can be commented - and the names are flattened out of whatever nesting is
used.

Whether a context that names no set permutes over *all* of them is not settled.
The toolchain also drops the sets whose macros a context's compiled HLSL never
mentions, and that is a property of the code rather than of the declaration, so
`ShaderNode::permutations_for` is an upper bound until a declaration can be paired
with its own section and the real count measured.

### An interface is a query, not an enumeration

The first cut enumerated one interface per subset of the optional variables,
which looks right until a real declaration does it: `standard_base` has 22 gated
variables, so 4194304 interfaces, against the 16 groups a section ships. A
material's inputs pick an interface; the conditions tree is what maps one onto a
group. So `ShaderNode::interface(&names)` and `ShaderNode::interface_of(mask)` answer one
query and nothing enumerates.

### Conditions are three-valued

`filetype::condition` parses the `if` grammar the declarations use - `defined`,
`!`, `&&`, `||`, brackets, calls with or without arguments, comparisons - and
evaluates it against a permutation's defines. A macro test is answered; a *call*
is an engine query (`num_skin_weights()`, `on_platform(GL)`) that a generated
declaration cannot answer, so it evaluates to `None` rather than a guess, and the
combinators fold that through Kleene logic. The two consumers then differ on
purpose, because the costs differ:

- a **channel** whose condition is unknown is left out of the group - a value the
  group cannot supply is a hole;
- a **pass** whose branch is unknown contributes *both* sides - a pass drawn when
  it should not be is wasted, a pass missing when it was needed is a hole.

## The group data, measured (this is what the emitter walks)

The group data is `8 + 48` bytes of header followed by the tables, and **every
table is preceded by a 12-byte header whose third word is that table's record
count**. That is the run length the descriptor does not carry, and it is what
makes a deterministic walk possible instead of pattern-matching.

Measured on `427B5E6E72E72FD7` (3 groups), all offsets relative to the group data:

| offset | bytes | meaning |
| --- | --- | --- |
| +0 | `03 00 00 00 0D E8 35 2B` | group count, then a hash |
| +8 | 3 x 16 bytes | descriptors, `{offset, count, cbuffer hash, flags}` |
| +124 | `70 00 01 00 04 00 00 00 45 00 00 00` | the first table's header, ending in the count **69** |
| +136 | 69 x 20 bytes | the engine's `global_viewport` table |
| +1516 | `F0 06 00 00 00 00 00 00 3A 00 00 00` | header: size **1776**, 0, count **58** |
| +1528 | 58 x 20 bytes | the material's own variables |
| +2688 | `90 01 00 00 00 00 00 00 13 00 00 00` | header: size 400, 0, count **19** |
| +2700 | 19 x 20 bytes | the channels |
| +3080 | `C0 01 00 00 80 00 00 00 13 00 00 00` | header: size 448, `0x80`, count **19** |

`0x6F0` = 1776 is the size of the `global_viewport` cbuffer, which the
`global_viewport` variable table independently says, so the first word of the
header is the size of the cbuffer the table's offsets refer to. The second word
is a flag or a stride, and is not settled.

Two things follow, and both correct earlier assumptions:

- **The tables come one after another, each with its own header**, in the order
  engine, material, channels. The material's table is therefore the one whose
  header count is the second largest in a group, and its *length differs per
  group* (58 / 17 / 15 across these three), which is the per-group interface
  showing through the data.
- **Runs must start on a word boundary.** Scanning at every byte offset finds
  false runs - a table at 1528 also "reads" as 8 records from 1527, and the
  28-byte packed records read as 20-byte records one at a time, every 28 bytes.
  A rewrite that trusts such a run clobbers the bytes after it, which is why the
  round trip fails on four of the six sections while the two whose tables are
  found cleanly come back byte for byte.

So the emitter's open problem is not the byte layout - it is the walk. Given a
table header, the next table's header is 12 + 20 x count bytes away, and the
group's tables are the ones between the descriptors and the next group's. What is
still missing is where one group's tables end and the next group's begin, and the
header's second word. The per-group headers - the 74/57/29-byte structures noted
earlier - are what would settle both.

### The 12-byte table header, and what is still open in it

The header is confirmed twice over on the same declaration, and the count is always
its last word:

```
+124   70 00 01 00   04 00 00 00   45 00 00 00    256,    4, count 69  → table at +136
+1516  F0 06 00 00   00 00 00 00   3A 00 00 00   1776,    0, count 58  → table at +1528
```

The first word is **not** the cbuffer size in both cases. `1776` is the size of
`global_viewport` and the material table's offsets run to 1764 inside it, so
there the first word is the cbuffer; but the engine table's first word is `256`
and its own records run to 1764 as well. So the first word is something else that
happens to be 1776 for the second table - the previous table's end, a group's
slice of the cbuffer, or a count of something else. The second word is `4` and
`0`, which look like flags but could as easily be a sub-count.

Both of the material table's and the engine table's offsets land in the same
cbuffer, which is worth stating plainly: **a group has one `global_viewport`
cbuffer, and both tables describe slices of it.** That is why the engine table is
the same in every group and the material's is not - the material's records are
the group's own slice.

The region between the descriptors (+56) and the first table's header (+124) is
68 bytes of 16-byte entries whose hashes are the ones the tail's resource lists
already name - `3AFC636C` (list 4, a section texture) and `41B1CFF8` (list 6, a
UAV) among them - so the per-group header is a *resource* list, and the tables
follow it. That is the structure to decode next: the entry shape, and therefore
where a group's tables end and the next group's resources begin.

### The walk rule, measured on all six sections

The four words immediately before every table are a header whose **last word is
that table's record count**, and whose third word is `256`:

| section | groups | first table | the four words before it |
| --- | --- | --- | --- |
| `004F18EA` | 3 | +184, 69 records | `120, 0, 4, 69` |
| `17A3DC01` | 3 | +136, 69 records | `112, 256, 4, 69` |
| `2A04418E` | 5 | +216, 69 records | `168, 256, 5, 69` |
| `3F08AC44` | 5 | +216, 69 records | `168, 256, 5, 69` |
| `427B5E6E` | 3 | +136, 69 records | `112, 256, 4, 69` |
| `38ECBAD1` | 1 | +156, 68 records | different shape, not yet read |

Five of six line up exactly: `256` in the third position, a count in the fourth
that is also the table's record count, and a first word that scales with the
group (`112` at four, `168` at five). The engine's table is 69 records in five of
the six - the same table, the same cbuffer, in every section - and the sixth is a
one-group section whose header reads differently.

So the walk the emitter wants is: find `256` on a word boundary, read the count
after it, expect a valid table 16 bytes on, and continue 12/16 + 20 x count bytes
later. That is a specific signature rather than "any 20 bytes that look like a
record", which is what made the scanning approach rewrite bytes it should not
have. `38ECBAD1` is the declaration to read before trusting the rule everywhere: a
single group may lay its header out differently, and that is exactly the case a
rule fitted to five samples would get wrong.

## The dependencies entry (8 bytes, one 64-bit hash)

The header's seventh and eighth words are the offset and the count of the
dependencies table, and every shipped section has **one** entry of **8 bytes**:

```text
u64 dependency   // little-endian Murmur64 of the dependency's path
```

`209FB8C3C0A8C3A4` is the long hash of `core/stingray_renderer/renderer` in the
dictionary, and its two words in little-endian order are exactly the entry's
two words. The first reading of this entry gave it sixteen bytes by counting
the group data's first two words as part of it; the second gave it a `{tag,
name}` split by reading one hash's two halves as two fields. Both errors came
from the same habit: reading a fixed number of words and calling the window a
record. The check is arithmetic - `group_data_offset - dependency_offset` is 8
on all seven sections measured - and the dictionary is the confirmation.

So the dependency is one engine constant, computed from the path, and the
group count and hash are the group data's own first two words, which is where
they live and where a writer already has them.

## The channel table, and the stride that reaches it

A group's third table is its **channels**, and the stride to it is the count
rule the walk now implements:

- a table's **record count is the word four bytes before its first record**;
- the **next table's 12-byte header begins where this table's records end**.

So for a material table of `len` records starting at `start`, the channel table's
header is at `start + 20 x len`, its count at `+ 8`, and its records at `+ 12`. On
`427B5E6E` that is `1528 + 20 x 58 = 2688`, whose header is `{400, 0, 19}` and
whose records are the nineteen channels. `GroupData::channel_table` reads it and
`GroupData::channels` groups the records into channels.

A channel is however many records it is: a *texture* channel is three - a type 5
binding followed by two type 1 parameters, the UV scale and offset the sampler
takes - and a scalar channel is one. The first reading of this assumed three for
every channel and refused every section; the record count is not the channel
count, and the channels of the six sections are:

| section | groups | records | channels |
| --- | --- | --- | --- |
| `004F18EA` | 3 | - | 11 |
| `17A3DC01` | 3 | - | 18 |
| `2A04418E` | 5 | - | 17 |
| `38ECBAD1` | 1 | - | 0 |
| `3F08AC44` | 5 | - | 10 |
| `427B5E6E` | 3 | 19 | 15 |

`38ECBAD1` reads zero channels through this stride, and the earlier note called
that a property of the single-group section. Re-reading it for this correction
found a 20-record table at `+1528` whose first record is a kind 5 binding
(`20BCBF88`) with two kind 1 parameters - a channel table, not "no table". So the
section is not an exception to the channel rule; its layout is simply not decoded
yet, and `38ECBAD1` is the section to read on its own before trusting any
per-group rule.

The UI base breaks the *stride*: its engine table sits between the material's and
the channels', so the stride lands on the engine run. `channel_table` refuses
that (a 69-record engine table is not a channel table), which means the UI base's
channels are reported as none until the per-group headers are decoded. Refusing
is the right failure; reading 69 engine variables as channels was the old one.

One trap, in the fixture as much as the data: a header's tail can read as a
record, so a *scan* for runs needs the count word to reject it - the UI base's
material table was read at `+76` for exactly that reason. The count-validated
walk now rejects it; the trap is why the count rule is implemented rather than
documented.

## Channel names do not pair; context names do

The group's channel names resolve out of the game dictionary, and they are real
material channels: `noise_texture`, `bca`, `orm`, `detail_nm`, `view_proj`,
`world_view_proj`, `world`, `last_world`, `color`, `wind_speed`, `noise_color`,
`world_noise_size`, `wind_power`, `detail_scale`, `dirt_amount`, `noise_edge_fade`,
`noise_width`, `noise_scale`, `noise_str`, `sharpness`, `orm2`, `bc2`,
`roughness`, `bc_blend`.

None of the distinctive ones appear in any of the fifteen `.shader_node`
declarations. `world_noise_size`, `wind_power`, `dirt_amount`, `noise_edge_fade`,
`sharpness`, `detail_scale`, `bc_blend`, `noise_str` and `orm2` are each in **zero**
declarations, while `billboard` - the one name that is in the library - is in seven.

The earlier note generalised this to "the pairing oracle does not exist". That
was too wide. **Context names do pair**: `default` is `F2760503` and
`shadow_caster` is `3100C3D2` in the dictionary, and both appear in the
declarations and in the shipped contexts tables. What does not pair is the
channel names, and it is a declaration-level pairing that is still missing: the
shipped sections and the Stingray-library declarations share generic context
names (`default`, `shadow_caster`), not a declaration identity. The context name
`5852A5B1`, which appears in every shipped section with two or three contexts,
is not in any declaration and does not resolve.

So `permutations_for` is still an upper bound pending a declaration-level pairing, and
the honest position is that the group count is *carried*, not derived. The group
data header carries it; a from-scratch build takes the count and the hash from a
template, which is the same bargain the group data emitter already makes.

## The contexts table: variable length, and there is no link table

The header's third and fourth words are the contexts table's offset and its
context count. A record is **variable length**:

```text
u32 name       // murmur32 of the context's name: F2760503 = default,
               // 3100C3D2 = shadow_caster, 5852A5B1 unnamed
u32 word2      // 0 on every context of every section measured
u32 count      // the number of queries that follow
count x {
    u32 query_id         // selects a group; the first query of the first
                         // context is the group data's own hash
    u32 conditions       // a byte offset into the conditions blob, or
                         // FFFFFFFF for none
}
```

The records fill `[contexts_offset, conditions_offset)` **exactly** - there is
nothing between them. Measured on all seven sections (six small sections and the
UI base): 004F18EA's two records are 28 and 20 bytes, the UI base's two are 252
and 60 (30 and 6 queries), and the sum always lands on `conditions_offset`.

The invariant that settles the shape: **the queries across the contexts are the
groups, one query per group.** `sum(count)` equals the group count in the group
data header on all seven, and the first query of the first context is the group
data's own hash. `Section::check` enforces both, together with a conditions
offset that lands inside the blob.

The queries and the groups are also **one to one**: every query id appears
exactly once in the group data, in its group's header, on all seven sections.
[7/7] The group data opens with a **4-byte global header** - the group count -
and then the groups run back to back, each starting with its query id:

```text
u32 query_id
u32 0x130            // 304, the same in every group
u32 4                // a count, the same in every group
u32 c_per_object     // B5639618
u32 0, 0, 0
descriptor[3]        // {name_hash, flags, X, Y}, the documented rule
...                  // resources, then the group's tables
```

The UI base's 36 groups are `4 + 12 x 1758 + 24 x 1741 = 62884` bytes, which is
the whole region: the first twelve groups are 1758 bytes and the rest 1741. The
descriptors are the three at `+32`: `global_viewport` `{516D5CCD, 0x101, 24, 0}`,
the section texture `{3AFC636C, 0x103, 48, 5}` and the UAV
`{41B1CFF8, 0x105, 56, 10}`, with `X` the per-draw byte offset and `Y` the packed
usage counts. The words at `+8` are the header's own `{0x130, 4, c_per_object, 0, 0, 0}`; an
earlier reading took them for descriptors and has been corrected in
`GroupData::descriptors`.

So a query id selects a group, and the conditions tree does not: it refines the
interface *within* the group, which is why the payload's result indices are
small (0..7) and not group numbers.

What the earlier reading called a **link table** was the tail of the last context
record read through the wrong record length. `004F18EA`'s "link context"
`{99C09062, FFFFFFFF, 5852A5B1, 0, 1}` is `default`'s second query
`{99C09062, FFFFFFFF}` followed by the next record's header `{5852A5B1, 0, 1}`;
`2A04418E`'s "link second word 0x1C" is `shadow_caster`'s first query conditions
offset (0x1C = 28, the second 28-byte node); and `FFFFFFFF` in that position is
"no conditions". There is no link table, no link flag and no node-per-link rule:
a 20-byte fixed record fits only the declarations whose every context has one query,
which is why the model survived as long as it did.

## The conditions node is **not** a constant (correction)

> **This section previously said the opposite, and it was wrong.** It claimed every
> node on every section was the same 28 bytes, that the grammar therefore never had
> to be decoded, and that this "closed the section". The measurement was right and
> the inference was not.

Every node on the six sections that have one *is* the same 28 bytes - five nodes
across `004F18EA`, `2A04418E` and `3F08AC44`, and the three with no conditions have an
empty blob. But the conditions section is a **real permutation tree** over the
material's texture channels, and its framing is now decoded:

```text
u16 tag            // 1
u16 payload_words  // the u16 words that follow the hashes
u16 payload_offset // 8 + 4 x count, the payload's byte offset in the record
u16 count          // the number of condition hashes
u32 hashes[count]
u16 payload[payload_words]
```

The UI base's conditions section is **1436 bytes and 35 records**, and its record
starts are exactly the 29 + 6 conditions offsets of its two contexts. The
dictionary names the roots: `gui` (`9FCFE126`), `red` (`9B8DE7E4`), `green`
(`4BA4BD58`), `blue` (`0977913D`), `alpha` (`3F697354`); `BDF72706`, `B5F45768`,
`8FB860CF`, `E2C8865F` and `BC4EE226` are unnamed. Records are subsets of their
parent (7 -> 5 -> 4 -> 2). Because `payload_offset` is the formula above, the
framing is self-delimiting, and `filetype::condition_tree` reads and writes the
region byte for byte; `shader43 --conditions` dumps it.

The payload reads as a list of **guarded results**: a run of `0x20xx` words is a
conjunction of tests over the hashes (the operand indexes the record's hash
list), `0x10xx` is the result when the conjunction holds, `0x70xx` jumps to the
record's end when that result was taken, `0x50xx` is the fallback result, and
`0x90xx` ends the record. Every `0x70xx` target is the record's own `0x9000`
word (`700B` in a twelve-word payload, `7009` in ten, `7008` in nine); a
one-result record is `2000 .. 200(N-1) 10(N-1) 9000` (rec17, rec24); a two-result
record is `tests A, 10a, 70end, tests B, 10b, 5007, 9000` (rec0, rec1, rec12,
rec13), and the branches' tests are subsets of the record's hashes.

That is a reading from 35 records, not a decode: the result values are small
indices (0..7) whose mapping to groups or interfaces is not established. Across
all 35 records the result equals the conjunction's test count minus one, and the
fallback is 7 where a record has one. So the tree is still carried, but the
framing is no longer open work, and the payload's shape is.

The six sections measured here have condition sections of 0, 28 and 56 bytes
**because they are small sections** - one to five groups, one to three contexts -
not because the format is a constant. Reading their agreement as the format is the
same error made four times this session: an observation from a small sample
written down as a conclusion.

So the conditions region is carried as bytes, addressed by the offsets in the
contexts queries, and **the conditions tree is open work.** The `CONDITIONS_NODE`
constant and the node-per-link writer were deleted with the link model: they were
a guess about a format from six small samples, and the UI base refused them. A
new shader has to build its own tree.

A test pins the constant's length, its word count and both end words, and it
earned its place immediately: the constant was first written with each word
byte-reversed, and the end-word assertion is what caught it.

### The whole layout

The contexts fill their region exactly and the conditions blob follows them, so
the layout is arithmetic:

| offset | size | contents |
| --- | --- | --- |
| 0 | 48 | the 12-word header |
| 48 | sum of the context record lengths | the contexts table |
| `conditions_offset` | `dependencies_offset - conditions_offset` | the conditions blob (a byte-addressed pool or tree, carried) |
| `dependency_offset` | 8 x dependencies | the dependencies table (one 8-byte u64 entry) |
| `group_data_offset` | `group_data_size` | the group data |
| `group_data_offset + size` | 1-3 bytes | slack, carried |
| `device_data_offset` | `device_data_size` | the programs |
| after the programs | to the end | the default-data region, carried |

`conditions_offset` is `48 + the sum of the context record lengths`, not
`48 + 20 x contexts + 8 x links`: the second formula was fitted to records read
at the wrong length, and the "links" were the bytes of the last record. A writer
lays the region out rather than copying offsets, and `Section::parse` refuses a
section whose contexts do not end exactly where the conditions begin.

The old link table and node pool are gone from the code, the layout and this
note. `Section::check` replaces them with the three measured invariants: the
queries are the groups, every conditions offset lands inside the blob, and the
first query is the group data's hash.


## The whole section round trips, and the dependencies entry is a u64

`Section::parse` walks the layout above and `Section::into_bytes` recomputes it,
and **all seven sections measured - the six small sections and the real UI base -
come back byte for byte**, including the UI base with its 1436-byte conditions
tree, which the link model could not even parse. That is necessary but not
sufficient: a wrong reader and its writer can agree and still be wrong, which is
why `Section::check`, the substitution tests and the query/group count are there.

**The dependencies entry is 8 bytes, and it is one little-endian u64**: the
Murmur64 of `core/stingray_renderer/renderer`, whose value the dictionary shows
as `209FB8C3C0A8C3A4`. The first reading gave it sixteen bytes by counting the
group data's own first two words; the second gave it a `{tag, name}` pair by
reading the two halves of the one hash as fields. `group_data_offset -
dependency_offset` is 8 on all seven sections, and the dictionary is the
confirmation.

Two regions are carried rather than derived, and both are named as such:

- **The slack** between the group data and the programs. The header's group data
  *size* is 1, 1, 3, 2, 3 and 1 bytes short of the distance to the device data on
  the six, and not constant, so it is neither alignment nor a fixed header.
- **The default-data region** after the programs, which the header's sixth word
  points into. It lands at the end of the device data on three of the six and two
  to three bytes before it on the others.

Both are kept whole rather than laid out, for the same reason the context's
second word is: they are fields this does not know the meaning of, and a change
there is the one change in the section that could not be checked.

## The substitution test, which is the one the round trip cannot cover

A round trip proves nothing moves when nothing is meant to change. It says
nothing about what happens when something *is* meant to, which is the only case
that matters for a writer. Run against all six small sections:

**A renamed context: 4 bytes, at +48, and only those.** The name word, nothing
else. A rename is length-preserving, so it *should* leave every offset alone, and
it does.

**A renamed material variable: 4 bytes, one run, entirely inside the group data.**
The name hash of one record, reached through `GroupData::rebuild` rather than
through the section. Three of the six skip it - their material tables hold only
records the dictionary has no name for, which is a gap in the dictionary and not
a failure, and the tool says which it skipped and why.

**A query added: the offsets followed the sum of the records.** A query is eight
bytes and the group count has to rise with it, so the test raises both: the first
context gains a query and the group data header's count word gains one. The
rebuilt section reads back, `conditions_offset` lands at
`48 + sum(context record lengths)`, and the group data is what was written. That
is the half the round trip cannot reach, because it alters a length and so has to
move everything after the contexts table.

The first version of this test compared against `48 + 20 x contexts + 8 x links`
and added a whole context - both the wrong formula and an invalid section under
the query/group invariant. It passed on the declarations whose records happened to be
20 bytes and refused on the rest, which is how the wrong record length stayed
alive. A substitution test that encodes the model it is testing proves nothing.

## The channel table is the one table a from-scratch group data writes

The other two tables in a group are engine-side - `global_viewport` and the packed
run - so they are carried. The **channel table is the declaration's own**, so it
has to be writable rather than copied; `GroupData::rebuild_channels` keeps the
engine's kind, flags and size for every slot and takes only the new name and
offset, because a type 5 binding's width is the engine's and not something a
declaration's type can say.

A rename moves the name hash in **each** of a channel's records - twelve bytes for
a texture channel, four for a scalar one - and nothing else. The record count must
be the template's: adding or dropping a channel moves the count word and the next
table, and a new channel needs a cbuffer offset from the compiled program's
reflection, which is not this call's input. The first version assumed every
channel was three records and refused every section; the second wrote whatever fit
over existing slots and left the count stale. Both are gone.

On the UI base the channel table is not reached by the current stride (the engine
table sits between the material's and the channels'), so its channels are reported
as none rather than guessed at. `38ECBAD1` has a channel table at `+1528` that the
earlier note said did not exist; reading it is open work.

### What the section now is

| region | status |
| --- | --- |
| contexts table | **measured and writable** - variable length, `{name, word2, count, queries}`, filling the region exactly |
| conditions blob | **framing decoded and writable** - self-delimiting records, `payload_offset = 8 + 4 x count`; a query's offset addresses one. The payload bytecode is mapped, not decoded; the UI base is 1436 bytes / 35 records |
| dependencies entry | **measured and writable** - one little-endian u64: Murmur64 of the renderer path |
| group data | **measured and writable for the material's table** - the count word before every table is the walk rule; the engine table is the one whose first record is `6BC91D73` |
| channels | **measured for the six, open on the UI base** - a table is a count word, records and the next header; the UI base's stride lands on the engine table and is refused |
| packed run | **partly measured** - readable, not a projection of the material table; 6 of 7 copies found on `427B5E6E` |
| block | **carried** - the library's compiled preamble; three header words are rewritten, nothing else |
| programs | **carried** - Oodle-framed DXBC, rebuilt not constructed |

The carried list is: the opaque and default-data header words, each context's
second word (0 on every section measured), the conditions blob, the slack between
group data and programs, the programs, and the bytes after them. Everything else
is derived from the declaration or read off the group data being built.

## The compiler is DXC, reached through its DLL

`dtmt build` compiles a material's shader sources today, but by **spawning
`dxc.exe`** - found through the `dxc` config option, then `DTMT_DXC`, then the
newest `Windows Kits\10\bin\*\x64\dxc.exe`. `dxc.exe` is a thin command-line
wrapper: the compiler is `dxcompiler.dll`, reached through the COM interface in
`dxcapi.h` (`DxcCreateInstance` -> `IDxcUtils` / `IDxcCompiler3`). Binding that
directly is the better shape, and the SDK has everything it needs:

- `Windows Kits\10\Lib\*\um\x64\dxcompiler.lib` - the import library, in every
  installed SDK (17763, 19041, 22621).
- `Windows Kits\10\bin\*\x64\dxcompiler.dll` and `dxil.dll` next to `dxc.exe`.
- `Windows Kits\10\Include\*\um\dxcapi.h` for the interface declarations.

Two things to get right, both from how the `oodle` crate links `oo2core`:

- The DLL is not on `PATH` and not shipped with the game, so like `oo2core` a
  developer copies it over: link the import library and ship `dxcompiler.dll`
  beside the tool, the same arrangement the Oodle binding already needs. No
  delay-loading and no runtime path hunting - one DLL to copy, like Oodle.
- The interfaces are ABI-stable, so a hand-written `extern "system"` vtable for
  the handful of types needed (`IDxcBlob`, `IDxcBlobEncoding`, `IDxcUtils`,
  `IDxcCompiler3`, `DxcBuffer`) is enough. `dxcapi.h` is large and drags in the
  Windows headers, so bindgen over it would make the build depend on an SDK
  *include* tree where the import library alone would do.

This matters for the from-scratch path because the group data's `cbuffer_offset`
values come from the compiled container: the SDK's DXBC reflection already reports
the slot of every variable (`shader43 --slots`), and Darktide's programs are
DXIL - SM 6.x payloads inside `DXBC` containers (every shipped program carries a
`DXIL` chunk; the decompiler runs `dxil-spirv`) - so the `dxc -T vs_6_0` /
`ps_6_0` profiles `dtmt build` already passes are what the DLL call has to
reproduce exactly.

## No ground truth to pair against

This is the thing to know before planning any measurement: **the game ships no
shader declarations at all.** A search of the whole install for `*.shader_node`,
`*.shader_import` and `*.hlsl` returns nothing. What it ships is compiled
sections only, inside the `bundle\data\XX\<hash>` files, which are bundle
containers whose frames are Oodle-compressed - the SDK's `bundle` module plus the
`oodle` crate are what read them, and nothing hand-rolled will.

The declarations available on this machine are therefore *not* Darktide's:

- `C:\dev\core_diff\shader_nodes` is a **Stingray library source drop** (it is a
  git checkout, and its files are the classic Stingray node library).
- `C:\dev\vmb\...\stingray_renderer\output_nodes` is a mod's own
  Stingray-renderer declarations.

Both are the right dialect and the wrong declarations. Context names pair by identity
(`default` = `F2760503`, `shadow_caster` = `3100C3D2`), but that is a shared
vocabulary, not a declaration identity: the shipped context name `5852A5B1` is in no
declaration. Channel names do not pair at all. So no declaration's group count can
be checked against a shipped section's, and any number computed from a Stingray
declaration is a hypothesis about Darktide, never a measurement of it.

Three oracles remain, in order of strength:

1. **Substitution**, which isolates a field: rename a context, rename a variable,
   add a query and raise the group count. A round trip cannot do this - a wrong
   reader and its writer agree - which is why the substitutions are the evidence
   and the round trip only the regression check.
2. **The count and bounds invariants**: the queries are the groups, a table is the
   run its count word says it is, and a conditions offset lands inside the blob.
   These caught the UI base's material table at +88 rather than +76.
3. **Round-trip**, read-N-write-N, which is necessary and not sufficient.
