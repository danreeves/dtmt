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

## The compiler is `dxc`, and the pipeline is already whole

There is no need for a new compile step. `dtmt build` finds `dxc.exe` (the
`dxc` config option, then `DTMT_DXC`, then the newest Windows SDK), compiles
`vs_main`/`ps_main` from `<name>.hlsl` or `<name>.vs.hlsl`/`<name>.ps.hlsl` next
to a material, and splices the container into every program of that stage
(`crates/dtmt/src/cmd/build.rs`). The section's programs are therefore mod-owned
today; what is still copied from a template is everything *around* them. The
group data's `cbuffer_offset` values are the one input that needs a source, and
the compiled container already carries them: the SDK's DXBC reflection gives the
slot of every variable, which is what `shader43 --slots` prints.

## Pairing a declaration with its own section

To settle the group count, a declaration has to be read next to the section it
was compiled into. The pieces are all present:

- `C:\dev\core_diff\shader_nodes` holds 146 of the **game's own** declarations.
- The compiled sections live in `bundle\data\XX\<hash>` in the install. The
  files come in two shapes; the unextensioned ones are a material whose payload
  *is* the section, and the layout is regular: the section size is a `u32` at
  offset `0x14`, and the section is `data[len - 4 - size .. len - 4]`. Verified
  against `bundle\data\00\0028686adad0c743`, a 35732-byte file whose 35408-byte
  section starts at 320.
- `db-list.txt` (in the notes directory) is the bundle database: a stream hash
  per line, then the file hashes it holds. A file's hash is the murmur64 of its
  **file name** alone - `bundle::get_name_from_path` hashes `path.file_name()`
  and looks it up in a filename dictionary - so a candidate material's stream is
  a lookup away.
- The extracted section is checked against the declaration by its channel names:
  each block channel record's murmur32 must be a channel the declaration
  declares. That is a strong enough pairing signal not to need the material's own
  name.
