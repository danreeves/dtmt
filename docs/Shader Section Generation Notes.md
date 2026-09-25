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
