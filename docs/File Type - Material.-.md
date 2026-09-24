Contains the layout of `stingray::MaterialResource` as used by Darktide, as
well as the SJSON format DTMT compiles from and decompiles to.

## Binary format

A material's bundle entry is a small resource header whose payload lives in an
external `data/<xx>/<hash>` file (property `DATA`), exactly like streamed
textures. The payload is the material stream:

```
u32 version                 // 60 or 61
u32 material_offset         // always 28
u32 material_size
u32 shader_offset           // u32::MAX if the material has no embedded shader
u32 shader_size
u32 unk2_offset             // u32::MAX if absent
u32 unk2_size
// at material_offset:
u32 name                    // IdString32, usually 0
MaterialTemplate...
// at shader_offset (if present): raw compiled shader blob
// at unk2_offset (if present): raw blob
```

`MaterialTemplate` uses the regular Stingray (little-endian) serialization:

| Type | Field | Meaning |
|------|-------|---------|
| `u64` | `material1` | primary parent material |
| `u64` | `material2` | secondary parent material |
| `IdString32[]` | `unk1` | shader texture channels (base materials) |
| `(IdString32, u64)[]` | `textures` | channel → texture resource |
| `(IdString32, IdString32)[]` | `material_contexts` | context → context material |
| `ShaderVariableReflection[]` | variables | `(u32 class, u32 elements, IdString32 name, u32 offset, u32 stride)` |
| `u8[]` | `variable_data` | packed variable values, indexed by `offset` |
| `(IdString32, bool)[]` | `unk2` | |
| `(u32, u32)[]` | `unk3` | |

`ShaderVariableReflection::class` is `0` scalar, `1` vector2, `2` vector3,
`3` vector4 and `12` for arbitrary data (where `elements * stride` gives the
byte size).

A material whose `shader_size > 0` ("base material") carries its own compiled
shader. Every other material is an *instance* that inherits the shader through
`material1`/`material2`. DTMT can compile instance materials only; shaders are
compiled DXBC blobs. Decompiling a base material emits its `shader_size` but
compiling it again is rejected.

`variable_data` may contain floats that are not described by any reflection
entry. They are preserved through the `extra_data` field so that
decompile → compile round trips stay byte-exact.

### Material contexts

`material_contexts` is *not* about resource paths. It maps a context name to a
context material name, both of which are 32-bit names. Darktide's
`surface_material` context is the surface type of the material, e.g. `cloth`,
`dirt` or `bone`, which is what decals, hit effects and footsteps use. The
Vermintide 2 source dump shows the same field with values such as `metal`,
`snow` or `flesh`.

## SJSON format

The SJSON format follows the Stingray source format (the same one used by
Vermintide 2's `.material` files). Values are resource paths; DTMT hashes them
the same way the game does. Decompilation writes the known name of a hash, or
an explicit `#`-prefixed hex hash for values that could not be named.

```sjson
parent_material = "content/ui/materials/base/ui_default_base"
material_contexts = {
  surface_material = "bone"
}
textures = {
  texture_map = "content/ui/textures/loading/loading_screen_background"
}
variables = {
  dirt = {
    type = "scalar"
    value = [0.5]
    offset = 0
  }
}
```

Fields:

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `name` | string | no | material name (`IdString32`), usually omitted |
| `parent_material` | string | no | `material1` |
| `parent_material_2` | string | no | `material2` |
| `material_contexts` | map | no | context name → context material name |
| `textures` | map | no | channel name → texture resource path |
| `variables` | map | no | variable name → `{ type, value, offset?, elements?, stride? }` |
| `channels` | string[] | no | `unk1`, only on base materials |
| `shader_size` | integer | no | set when decompiling a base material |
| `extra_data` | string | no | hex of `variable_data` bytes not covered by variables |
| `unk2` | map | no | unnamed `(name, bool)` pairs |
| `unk3` | array | no | `(a, b)` pairs |

Variable values are always arrays, even for `type = "scalar"`. The SJSON number
grammar parses a bare float like `0.5` as the integer `0` followed by `.5`, so
scalars cannot be represented as bare numbers without corrupting the file.

`offset` is optional. When omitted, the value is packed after the previous
variable. Decompiled materials always include it, because the game's instance
materials distribute their variables at offsets that are not reproducible from
the variable list alone.

### Base materials and new textures

To use an entirely new texture, create an instance material that references an
existing base material and point one of its texture channels at the new texture
resource:

```sjson
parent_material = "content/ui/materials/base/ui_default_base"
textures = {
  texture_map = "textures/mods/example/my_image"
}
```

Whether the reference resolves depends on the texture resource being present in
the bundle database, which DTMM takes care of when deploying the mod.

### Referencing a new material from Lua

A new material is a new resource, so nothing loads it until something asks for
it by name. The most reliable way to do that in a mod is to point a UI widget at
it at runtime. For example, to replace the loading screen background:

```lua
local LoadingView = require("scripts/ui/views/loading_view/loading_view")

local LOADING_MATERIAL = "materials/mods/example/loading_screen_background"

local original_on_enter = LoadingView.on_enter

LoadingView.on_enter = function (self)
    original_on_enter(self)

    local widget = self._widgets_by_name and self._widgets_by_name.background
    for _, pass in ipairs(widget.passes or {}) do
        if pass.pass_type == "texture" then
            widget.content[pass.value_id] = LOADING_MATERIAL
        end
    end
end
```

The widget is only built once the view's package has loaded, which is why this
runs in `on_enter` rather than `init`. `LoadingView.init` rebuilds the
background widget from `Views.loading_view.backgrounds`, so patching the view's
definitions file is not enough.

### Self-contained materials

Instance materials inherit their shader from a parent material. Referencing a
game material directly can stop the engine from unloading the mod package
cleanly, because the base material is shared with the game. A mod can instead
ship its own copy of a base material: decompiling one emits its compiled shader
as `shader_data`, and compiling that again reproduces the original blob
byte-for-byte. Pointing the new instance material at the mod's own base material
keeps the whole material graph owned by the mod.

### Custom shaders

A base material's `shader_data` is a `shader43` section containing framed DXBC
programs (Oodle-compressed containers with a per-program frame key). Shader
sources that sit next to a material are compiled and spliced in automatically:

```text
materials/mods/example/ui_base.material
materials/mods/example/ui_base.hlsl        // vs_main and/or ps_main
materials/mods/example/ui_base.ps.hlsl     // or a separate pixel shader
materials/mods/example/ui_base.vs.hlsl     // or a separate vertex shader
```

`dtmt build` compiles them with `dxc`, replaces every program of that stage,
re-compresses the frames with Oodle and updates the frame lengths, keys and the
section header. The replacement has to keep the shipped shader's interface; the
build checks the stage and both signature layouts and fails otherwise. See
`shaders/README.md` for the bindless resource layout and an example shader.

#### How the shader build works

`shader_data` is the compiled baseline: the whole `shader43` section with its
programs, per-program reflection, contexts, conditions, group data and default
data. The `.hlsl` files are build-time inputs and are not shipped in the bundle.

A material can also declare a **shader preset** and carry no `shader_data` at
all:

```sjson
// ui_default_base.material
shader_preset = "ui_default_base.preset"
```

`ui_default_base.preset` is a text file next to the material (or relative to the
mod root) that holds the family's engine-side wrapper: contexts, conditions,
dependencies, group data, the packed device preamble and one metadata tail per
program. When the declaration is present, `dtmt build` compiles the sibling
shader sources and generates the whole section from them and the preset, so the
material source stays a few hundred bytes and no shipped shader blob is needed.
The section is byte-identical to what the splice route produces for the same
sources. `generate_shader --preset <out.txt> <material data file>` extracts a
preset from a shipped base material once; shrinking the preset to only genuine
engine constants is an open item (see
[Shader Section Generation Notes](Shader%20Section%20Generation%20Notes.md)).

Otherwise, `dtmt build` performs these steps:

1. **Compile.** Each shader source is compiled with `dxc` to a DXBC/DXIL
   container: `<name>.hlsl` is probed for `vs_main` and `ps_main`, while
   `<name>.vs.hlsl` and `<name>.ps.hlsl` use those entry points directly.
2. **Parse.** `shader_data` is decoded, the device data is scanned for framed
   programs (`envelope = 1`, `frame_length`, an Oodle frame starting with
   `8c 06`, `metadata_kind = 5`), every frame is Oodle-decoded to a DXBC
   container, and the shader stage is read from the `PSV0` chunk.
3. **Check.** For every program whose stage has a replacement, the compiled
   container's stage and `ISG1`/`OSG1` semantics (name, index, register, mask)
   are compared with the original. A mismatch fails the build, because the other
   stages and the engine's input layouts are unchanged.
4. **Re-frame.** The new container is compressed with Oodle (Kraken) and the
   program record is rewritten: `frame_length`, `decoded_dxbc_length` and
   `frame_key = MurmurHash64A(frame, seed 0)`. The 16-byte metadata header is
   replaced and the rest of the program's metadata (the engine's reflection) is
   copied verbatim. Programs without a replacement are copied byte-for-byte.
5. **Relocate.** `device_data_size` is updated, the default-data block is moved
   after the device data with padding recomputed (4 bytes before the default
   data, 16 bytes at the end), and the material stream's `shader_size` and
   `unk2_offset` are updated.
6. The material is compiled into a bundle as usual.

At runtime the engine decodes the frames, creates pipeline states using its own
root signature, and binds the material's texture/sampler descriptors through
`c_per_object`; the shader samples them through the bindless arrays
(`global_texture2D[]` at `t0, space2`, `global_samplers[]` at `s0, space2`).

To modify an existing shader instead of writing one from scratch, its compiled
program can be translated back to editable HLSL; see
[Shader Decompilation](Shader%20Decompilation.md).

## Status and open questions

### Implemented

- Material streams (version 60/61) parse, decompile to SJSON and compile back.
  Instance materials and base materials (including their embedded `shader_data`)
  round trip byte-for-byte.
- Base materials can be built as long as they carry a `shader_data` blob; the
  shader section is preserved, or rebuilt when a shader override is applied.
- Custom shader sources next to a material are compiled with `dxc` and spliced
  into the shader section (`ShaderOverrides`). Programs of other stages and
  unreplaced programs are preserved byte-for-byte, and a replacement's stage and
  signature layout are checked against the original.

### Material unknowns

| Field | What is known | What is missing |
| --- | --- | --- |
| `unk1` | Shader texture channels, e.g. `texture_map` on the UI base | Whether values other than channel names appear |
| `unk2` | `(IdString32, bool)` pairs; empty in every material observed | Meaning; probably shader flags/defines |
| `unk3` | `(u32, u32)` pairs, e.g. `(6,0) (5,0)` on the UI base | Meaning; possibly program/variant selection |
| `material_contexts` | 32-bit context names such as `surface_material = "bone"` | The full value set and how consumers use it |
| Header `unk2_offset`/`unk2_size` | A small trailing blob (4 bytes on the UI base) | Contents |

### Shader43 unknowns

| Section | What is known | What is missing |
| --- | --- | --- |
| Header | All 12 words, and how to relocate device/default data | - |
| Contexts | A sequence of `{name_hash, u32, count, count × {query_id, conditions_offset}}` records filling `[contexts_offset, conditions_offset)`. The first context is `default` (`F2760503`) in every material observed. The second word of each pair is a **byte offset into the conditions section**: the UI base's `default` context lists 29 pairs whose offsets (`0, 0x3C, 0x6C, 0x9C, 0xCC, 0xFC, 0x11A … 0x48C`) are exactly the 29 condition records, so a context is a list of condition-tree entry points selected per material parameter. `0xFFFFFFFF` means **no conditions**: the minimal two program material `7c5d7bb7cce6b77c` has an empty conditions section, one `default` context with a single pair `{35D5E6D8, 0xFFFFFFFF}`, and its group header carries that same query id, so a material that does not permute anything needs no conditions at all. Sizes check out: the UI base's `(3 + 30×2) + (3 + 6×2)` words = 312 bytes, a single-context material is `3 + 1×2` words. `shader43 --section contexts <material data file>` dumps it | What the query ids name (they do not resolve through the dictionary), how the engine evaluates a conditions record, and which group its leaf ends up selecting |
| Conditions | A sequence of records `{u16 tag=1, u16 b, u16 c, u16 count}` + `count` condition hashes (u32) + a u16 payload, filling `[conditions_offset, dependencies_offset)`; the UI base's section is 1436 bytes and 35 records parse out of it. The condition names resolve through the enriched dictionary: the root set is `gui` (`9FCFE126`), `red`, `green`, `blue`, `alpha` and more (`BDF72706 B5F45768 8FB860CF`), plus the variants `BC4EE226` and `E2C8865F` - it is the shader library's permutation tree over the material's texture channels. Records are subsets of their parent (7 → 5 → 4 → 2), contexts list every node, and the payload is a list of u16s per node (`2000 2001 2002 1002 700B 2003 … 9000`) whose fields are still to decode (the high and low bytes look like a target and a condition bitmask) | The payload encoding and what a leaf selects (a group index or a query id) |
| Dependencies | 8 bytes on the UI base = a single u64 (`A4C3A8C0C3B89F20`), likely one 64-bit dependency id; the header's `dependency_count` is 1 | What a dependency is and when shaders carry more than one |
| Group data | Variable tables: runs of `{type, flags, name_hash, cbuffer_offset, size}` records preceded by their count, one table per cbuffer per group, stored several times per group (the UI base has 36 copies of its per-object table, including a compact serialization after the first tables). Each group's header starts `{u32, query_id, byte_size?, descriptor_count, descriptor...}` where `query_id` is the context entry that selects the group (the UI base's 12 groups reference every second query id of the `default` context), followed by a resource descriptor list `{name_hash, flags, X, Y}` (`c_per_object`, `global_viewport` 0x101, `global_texture2D` 0x103, …) and then the group's variable tables. **Descriptor `X` is the resource's byte offset in the per-draw binding table, allocated in descriptor-list order: 24 bytes per constant buffer, 8 bytes per other resource** (verified in the UI base: `c_per_object` 0 → 24, `global_viewport` 24 → 48, `global_texture2D` 48 → 56, `41B1CFF8` 56; and in a small two-program material where the descriptors lay out as 0/24/32/40/48/56/80/88/96/104/112 exactly). `flags` keeps the resource's space in bits 16+ and a small kind in the low bits (0 = material cbuffer, 1 = engine cbuffer, 3 = texture, 5 = UAV; `c_per_object` 0x0, `global_viewport` 0x101, `fog_volume` 0x10003 = space 1, `global_diffuse_map` 0x20103 = space 2 + bit 8, `reflection_map` 0x50003 = space 5). Across the corpus (2037 materials, 80465 table headers) `flags` is stable per resource name while `X` varies per family. The compact serialization stores the descriptor list again as 16-byte records `{name_hash (4 bytes in display order), flags, X, Y}` and the variables as `{cbuffer_hash, …}` records; in-place renames and appended records patched across all copies were accepted by the build and rendered correctly, so the copies can be edited by hash without understanding every header word. **Gap slots are dead space**: a record put into uncovered bytes (the UI base's 8..16 / 24..32 gaps) is accepted, Lua drives it and the shader reads the right slot, but the engine never uploads it, so only slots the engine itself fills are writable. `shader43 --variables/--slots` read the tables | What descriptor `Y` measures exactly (0 for constant buffers; small counts for other resources, e.g. `global_texture2D` 5 vs 1 and `41B1CFF8` 10 vs 2 in the UI base's two group halves) and the compact variable record's exact field order |
| Device data / programs | Record layout, Oodle frames, frame key, stage from `PSV0`. The device data starts with a packed preamble (a lookup table with increasing indices, small values and variable name hashes; 561 bytes for the UI base's 96 programs) before the first program record; a generated section without it makes the engine run out of memory when a material using it is drawn. What the preamble is: a shared table that two same-shader base materials match on byte for byte apart from one list entry (`count 1 -> 2` plus a name hash) and a 60 byte suffix, and whose long middle section is identical across *different* shaders too, so it holds engine-side variable reflection plus the material's own entries; the header starts `{1, A, B, C, 30, 0, 0, 768}`. Its first three words are `{1, query_count, 2}` (the minimal one-query material reads `{1, 1, 2, 0}` and the UI base, 30 + 6 queries, reads `{1, 36, 2, 37, 30, …}`), and the block after them is the *same* block that shows up at the end of every pixel program's tail (the UI pixel tails are ~805 bytes because the tail region extends over it before the next program record; `texture_map` sits at `+0x1F4` inside it), so it is the per-pipeline piece rather than a purely global one. `shader43 --preamble --dump <dir> <material data file>` dumps it | The field layout of the header and records, and which part a new family has to synthesise versus inherit from the engine's shared table |
| Program metadata tails | The engine's per-program resource map, as murmur32 name hashes. Cbuffer entries carry the cbuffer's name (`global_viewport`, `c_per_object`, `material_variable`, `bones`) and its size in bytes; the VS tail lists its input signature names (`POSITION`, `COLOR`, `TEXCOORD`) as `(name_hash, semantic index, ordinal)`. Resource names like `global_texture2D`, `global_samplers` and `static_minlod_sampler` appear next to what look like space/register fields (`31` sits by `static_minlod_sampler`, which renders from space31). Framing: the tail opens with `{u32 cbuffer_count}` then 24 byte cbuffer entries whose `{name_hash, size}` sit at `+4`/`+12`, in register order (`{global_viewport, 1776}` then `{c_per_object, 240}` on the UI base's pixel program; only `{c_per_object, 240}` on its vertex program). After them comes a sequence of counted lists, empty ones taking a single `0` word: **list 3 holds SRVs, list 5 holds UAVs, list 8 holds the input signature semantics** (3 word `{name_hash, 0, ordinal}` entries, `POSITION` on the UI base), and lists 4/6/7 sit empty in every sample; the order is stable across stages, which is what the leading zero runs measure (a pixel program with four UAVs and nothing else reads `0, 0, 0, 0, 4`; the same program's sibling with two SRVs and four UAVs reads `0, 0, 2, …entries…, 0, 4, …entries…`; the UI base's vertex program reads seven `0`s then `3, POSITION…`). Entries are 7 words for plain buffers (`{name_hash, binding_index, register_index, 1, 0, 0, 0}`, the binding index continuing across the material's programs and reused by repeat bindings, e.g. `5E4B0706` is 1 in both programs of a two program material) and 7 words for texture arrays/UAVs too (`{name_hash, kind, 0, 0xFFFFFFFF, space, 0xFFFFFFFF, 0}`; kind 3 = UAV, `0xFFFFFFFF` = bindless, space 31 on the UI base's bindless UAV). Pixel programs can carry further 3 word `{name_hash, i, i+1}` entries after the signature list (`96B9600E` ×3 on the UI base) and the list before them (`4B457E26`, space 31, kind 1) looks like the static samplers; that tail region is also where the engine had put `static_minlod_sampler`. A pixel program's tail then ends with the *shared block*: the preamble's own tail matches the tail's tail word for word (the UI base's preamble is 561 bytes = `{1, 36, 2}` + a 549 byte block, and the last 549 bytes of every pixel tail are that same block, so a pixel tail is `[program records][block]`; the UI base's pixel program records use 252 bytes of the 801; only the trailing *bytes* coincide, so align on words - a byte-wise suffix match finds three extra coincidental bytes). That is why `texture_map` shows up in both. The input signature list is confirmed to be `{u32 count}` + `count × {semantic name hash, semantic index, ordinal}`: `POSITION` `3FFEABD6`, `COLOR` `FCDCBA12`, `TEXCOORD` `B77A0F36`, `NORMAL` `7668E94B`, `TANGENT` `E32E5A9D`, `BLENDINDICES` `5F29E5B9`, `BLENDWEIGHT` `98405AD1`, `CUSTOM` `96B9600E` (`dtmt murmur hash <name> --half`), e.g. the UI base's vertex program lists `{3, POSITION@0:0, COLOR@0:1, TEXCOORD@0:2}` and many materials list `CUSTOM0..4`. Tails are per program, identical for programs with the same interface (all 48 UI vertex programs share one 112 byte tail) and can be all zero (the minimal HUD shader writes none). **The tails are load-bearing**: rebuilding the UI base with every tail truncated to its cbuffer entries (same byte length, resource lists zeroed) makes the game crash while loading the shader (`stingray::D3D12RenderDevice::dispatch_loadtime`, `shader #ID[31b9724d]`, the group's query id), while restoring the original tails renders the title screen again. The per-object cbuffer size differs per shader family (240 bytes on the UI base, 352 on the entity base), and the size in the entry is **patchable**: growing `c_per_object` in the mod's own HLSL (240 -> 256 bytes) and rewriting that size in every program tail with `Tail::parse`/`Tail::bytes` still renders the title screen, so a generator can adapt a family's tails to a mod's own constant buffer layouts instead of demanding a byte-identical interface. `shader43 --slots --hlsl <decompile dir>` uses the cbuffer entries to print which decompiled array and slot a variable lands in; `shader43 --tails` dumps every program's tail | The lists after the input signature (their count words and the `{name_hash, i, i+1}` entries), the trailing shared block, and generating a tail from a compiled container's reflection |
| Default data | A table after the device data: `{u32 zero}{u32 count}` then `count` entries of `{name_hash, element_count, blob_offset}` (12 bytes each), then a blob holding the values at their `blob_offset`. Worked example (entity base): `particle_color` has 4 elements at blob `+32` = `1.0 1.0 1.0 1.0`; `material_variable` has 1 element at blob `+48` = `0.0`. The UI base's default data is all zeros (no entries). Mind that `material_variable` is a *variable* of `c_per_object`, not a cbuffer: the entity base's group data has records `particle_color` @320 (float4) and `material_variable` @336 (scalar) inside its 352 byte per-object buffer | What a zero/default value means for the engine |

### Open work

1. **Decode the program metadata tails.** Compare tails across shipped shaders
   with different interfaces and correlate them with the DXIL `!dx.resources`
   metadata, so replacements no longer have to keep the original interface.
2. **Decode contexts and conditions.** Needed for a shader library that holds
   several of our own programs and selects them per permutation.
3. **Decode group data / the binding layout.** The engine supplies the root
   signature, so a shader can only use bindings the cloned layout provides until
   this is understood.
4. ~~**Extend the shader variable table.**~~ Researched: variable records are
   `{type, flags, name_hash, cbuffer_offset, size}` runs preceded by a count,
   stored in many copies per group; a prototype that appended a record to every
   copy and relocated the section was validated in game. Deliberately kept out
   of the tooling: aliasing an existing slot is the safer modding path (the mod
   uses the shipped `dev_wireframe_color`), and growing the per-object cbuffer
   for a genuinely new slot fails even with every encoded size patched, so the
   farmgate for that is the binding layout (item 3).
