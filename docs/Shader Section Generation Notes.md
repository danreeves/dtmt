# Generating a shader section (plan)

This is the working plan for emitting a complete `shader43` section from our own
data instead of splicing a shipped one. It collects what each section needs and
marks what is still unknown. See `File Type - Material.-.md` for the field-level
notes.

## The `generate_shader` example

`lib/sdk/examples/generate_shader.rs` implements the template route:

```text
# One-off: extract the wrapper (contexts, conditions, dependencies, group data,
# one metadata tail per program) of a shipped base material.
generate_shader --preset ui.preset.txt <material data file>

# Generate a base material SJSON: our compiled containers replace every
# program, using that program's tail; everything else comes from the preset.
generate_shader --generate ui.preset.txt <base.material> <out.material> \
    --vs <container.dxbc> --ps <container.dxbc>
```

The preset is a small text file (a few hundred KB because the group data is
hex): the family's engine-side wrapper. The generated material keeps the
material-side fields of the file given as `<base.material>` (parent, textures,
channels, `unk3`, ...) and replaces its `shader_data`/`shader_size`. No shipped
shader blob is needed at generation time; the preset is the only input derived
from the game.

Status: **verified in game**. Generating from the UI base's preset with the mod's
shaders produces a working section (96 programs, ~431 KB) that builds, deploys,
and renders: the title screen tint follows the Lua-driven material value, and no
shipped shader blob is present in the mod. The first attempt without the
device-data preamble reached the title and then hit the engine's out-of-memory
error, which is how the preamble's importance was found.

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
| Group data | Generate per group. Each group needs a header (`{u32, query_id_of_the_group, descriptor words, …}`), the variable tables for that group's cbuffers, and the compact copy of those tables that follows. The variable records are `{type, flags, name_hash, cbuffer_offset, size}` runs with a count word; copies must all be consistent | Partly known: how the compact copy and the descriptor words relate is still open |
| Device data | Generate: a packed preamble followed by framed DXBC programs. Each program record is `envelope=1`, `frame_length`, Oodle frame, `metadata_kind=5`, decoded length, frame key, then the metadata tail. **The preamble matters**: a generated section without it makes the engine run out of memory as soon as a material using it is drawn (verified in game - the packed table is read as a lookup and garbage sizes follow), so `--generate` writes the preset's preamble before the records | The preamble's field layout (it is a packed table with increasing indices and small values, plus variable name hashes in places); decoding it is required for families whose program set differs from the template's |
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
2. Group data: the descriptor words after the group's query id, the compact copy
   of the variable tables, and whether one group can replace a permutation set.
3. Conditions: the payload encoding and what a leaf selects.
4. How a material chooses a context at runtime (which context query a material
   parameter answers), which decides whether a generated shader can ship a
   single context.
