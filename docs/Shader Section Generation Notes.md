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
constants that cannot be derived from the shader itself. The working candidates
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
that cannot be derived from the shader - and even then the goal is to decode and
shrink them to the smallest possible form, not to grow them into a preset.

## Status of the intermediate route

The mod-defined build flow is in place: a material can declare
`shader_preset = "<name>.preset"` and ship no `shader_data` at all; `dtmt build`
compiles the sibling shader sources and generates the section from them and the
preset (byte-identical to the splice route), and snoopymod runs that way - its
base material is a few hundred bytes, the preset is the only game-derived file.
The preset holds the family's engine-side wrapper for now; shrinking it to only
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
| Contexts | Copy from a template with the same shader family. The query ids and their condition-entry offsets are engine-side constants tied to the shader's permutation space; the layout is `{name_hash, u32, count, count × {query_id, conditions_offset}}` | Layout known, ids only copyable |
| Conditions | Copy from the same template. Records are `{u16 tag=1, u16 b, u16 c, u16 count}` + `count` hashes + a packed payload; they form a decision tree whose leaves select a group | Layout known, payload decoding still open |
| Dependencies | Copy from the same template (8 bytes = one u64 id on the UI base) | Copyable only |
| Group data | Generate per group. Each group needs a header (`{u32, query_id_of_the_group, descriptor words, …}`), the variable tables for that group's cbuffers, and the compact copy of those tables that follows. The variable records are `{type, flags, name_hash, cbuffer_offset, size}` runs with a count word; copies must all be consistent | Structure mapped (see `Shader RE TODO.md`): a 32-byte global header then 36 groups; the channel table, variable table and packed run are byte-identical across all 36 groups, only the descriptors' `Y` and the byte-packed group header vary. The tables are identified: 69 records = the `global_viewport` engine cbuffer's variables, 7 records = the group's `c_per_object` variables (incl. `texture_map`). Generation = emit the tables once, replicate them across the template's group count, keep the template's descriptors/headers |
| Device data | Generate: a packed preamble followed by framed DXBC programs. Each program record is `envelope=1`, `frame_length`, Oodle frame, `metadata_kind=5`, decoded length, frame key, then the metadata tail. **The preamble matters**: a generated section without it makes the engine run out of memory as soon as a material using it is drawn (verified in game - the packed table is read as a lookup and garbage sizes follow), so `--generate` writes the preset's preamble before the records | The preamble's 120-byte header is decoded: only `+0x04` (group count), `+0x08` (cbuffer count) and `+0x0C` (record count + 8) vary across seven shipped families, the rest is constant; the byte-packed `{index, value}` records after it are engine-variable binding entries shared across families (22-record common prefix). Generation = copy the template's block and rewrite the three header words |
| Program tails | Still partly open. Cbuffer entries are 24-byte records whose `{name_hash, size}` sit at `+4`/`+12`, in register order; signature elements are listed by name hash with index/ordinal. What the engine does with the rest of the tail is unknown | Open |
| Default data | Generate: `{u32 zero}{u32 count}` then `count × {name_hash, element_count, blob_offset}` then the value blob, with `element_count` = 1/2/3/4 for scalar/vec2/vec3/vec4 and `blob_offset` = byte offset of the value in the blob | Known |

## Shape decisions for a first generated shader

- **Reuse one shader family per shader.** Pick the template base material whose
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
declares a whole family:

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
3. Conditions: the payload encoding and what a leaf selects.
4. How a material chooses a context at runtime (which context query a material
   parameter answers), which decides whether a generated shader can ship a
   single context.

## The declaration front end (what the reader now covers)

`filetype::shader_node` reads a `.shader_node` into the `Family` the emitters
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

### Groups are per context, not per family

`compile_with` names the permutation sets a context permutes over, and that is
what the group count is: `standard_base`'s two contexts permute two sets each,
four groups, where the product over every set of the family is sixteen. A
`permute_with` may nest - a list of entries that each name a set again, so the
block can be commented - and the names are flattened out of whatever nesting is
used.

Whether a context that names no set permutes over *all* of them is not settled.
The toolchain also drops the sets whose macros a context's compiled HLSL never
mentions, and that is a property of the code rather than of the declaration, so
`Family::permutations_for` is an upper bound until a declaration can be paired
with its own section and the real count measured.

### An interface is a query, not an enumeration

The first cut enumerated one interface per subset of the optional variables,
which looks right until a real family does it: `standard_base` has 22 gated
variables, so 4194304 interfaces, against the 16 groups a section ships. A
material's inputs pick an interface; the conditions tree is what maps one onto a
group. So `Family::interface(&names)` and `Family::interface_of(mask)` answer one
query and nothing enumerates.

### Conditions are three-valued

`filetype::condition` parses the `if` grammar the declarations use - `defined`,
`!`, `&&`, `||`, brackets, calls with or without arguments, comparisons - and
evaluates it against a permutation's defines. A macro test is answered; a *call*
is an engine query (`num_skin_weights()`, `on_platform(GL)`) that a generated
family cannot answer, so it evaluates to `None` rather than a guess, and the
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
  round trip fails on four of the six families while the two whose tables are
  found cleanly come back byte for byte.

So the emitter's open problem is not the byte layout - it is the walk. Given a
table header, the next table's header is 12 + 20 x count bytes away, and the
group's tables are the ones between the descriptors and the next group's. What is
still missing is where one group's tables end and the next group's begin, and the
header's second word. The per-group headers - the 74/57/29-byte structures noted
earlier - are what would settle both.

### The 12-byte table header, and what is still open in it

The header is confirmed twice over on the same family, and the count is always
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
already name - `3AFC636C` (list 4, a family texture) and `41B1CFF8` (list 6, a
UAV) among them - so the per-group header is a *resource* list, and the tables
follow it. That is the structure to decode next: the entry shape, and therefore
where a group's tables end and the next group's resources begin.

### The walk rule, measured on all six families

The four words immediately before every table are a header whose **last word is
that table's record count**, and whose third word is `256`:

| family | groups | first table | the four words before it |
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
the six - the same table, the same cbuffer, in every family - and the sixth is a
one-group family whose header reads differently.

So the walk the emitter wants is: find `256` on a word boundary, read the count
after it, expect a valid table 16 bytes on, and continue 12/16 + 20 x count bytes
later. That is a specific signature rather than "any 20 bytes that look like a
record", which is what made the scanning approach rewrite bytes it should not
have. `38ECBAD1` is the family to read before trusting the rule everywhere: a
single group may lay its header out differently, and that is exactly the case a
rule fitted to five samples would get wrong.

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
the slot of every variable (`shader43 --slots`), and Darktide needs DXBC, so the
`dxc -T vs_5_0`-style profile the current shell-out already passes is what the
DLL call has to reproduce exactly.

## No ground truth to pair against

This is the thing to know before planning any measurement: **the game ships no
shader declarations at all.** A search of the whole install for `*.shader_node`,
`*.shader_import` and `*.hlsl` returns nothing. What it ships is compiled
sections only, inside the `bundle\data\XX\<hash>` files, which are bundle
containers whose frames are Oodle-compressed - the SDK's `bundle` module plus the
`oodle` crate are what read them, and nothing hand-rolled will.

The declarations available on this machine are therefore *not* Darktide's:

- `C:\dev\core_diff\shader_nodes` is a **Stingray library source drop** (it is a
  git checkout, and its files are the classic Stingray node library - `group =
  "Math"`, `type = "auto"`). It is the right *dialect* and the wrong families.
- `C:\dev\vmb\...\stingray_renderer\output_nodes` is a mod's own
  Stingray-renderer declarations. Also the right dialect, also not Darktide's
  families.

Darktide is built on Stingray, so its declarations are Stingray-shaped and the
dialect work transfers. But there is no declaration whose section we also have, so
the group count cannot be settled by pairing. Two things can settle it instead:

1. **Round-trip.** Read a shipped section into the emitters and write it back
   byte-identically. This is what the block emitter does now (863/863 bytes on
   `427B5E6E72E72FD7`) and it is the only offline oracle available.
2. **The binary.** The `dependencies` and `conditions` sections record what the
   toolchain decided, so decoding them answers the group count directly - which
   is why those two sections are the remaining decode work rather than the
   emitters.

Treat any number computed from a Stingray declaration as a hypothesis about
Darktide, never as a measurement of it.

