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

`dtmt build` performs these steps:

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
| Contexts | A sequence of `{name_hash, u32, count, count × {query_hash, value_hash}}` records filling `[contexts_offset, conditions_offset)`. The first context is `default` (`F2760503`) in every material observed. The pixel program's group data header carries the first query hash of the group it belongs to as its second word, so the pairs specialise groups for material conditions. Sizes check out: the UI base's `(3 + 30×2) + (3 + 6×2)` words = 312 bytes, and a single-context material is `3 + 1×2` words. `shader43 --section contexts <material data file>` dumps it | What the query/value hashes name (they do not resolve through the dictionary), and how the engine picks a context at runtime |
| Conditions | Per-group bitmask expressions over the context queries. The UI base's section starts `{u32 1}{u32 12}{u32 36}{u32 7}` followed by 7 condition hashes, then bit-packed words with 2-bit and 4-bit fields (`02 20 01 20 02 10 0B 70 …`) | The encoding of the packed words and how a bitmask selects a group |
| Dependencies | 8 bytes on the UI base = a single u64 (`A4C3A8C0C3B89F20`), likely one 64-bit dependency id; the header's `dependency_count` is 1 | What a dependency is and when shaders carry more than one |
| Group data | Variable tables: runs of `{type, flags, name_hash, cbuffer_offset, size}` records preceded by their count, one table per cbuffer per group, stored several times per group (the UI base has 36 copies of its per-object table). **Gap slots are dead space**: inserting a record into uncovered bytes (the UI base's 8..16 / 24..32 gaps) is validated in the build, Lua drives it, the shader reads the right slot inside a working 240-byte `c_per_object`, but the upload renders nothing. A variable at a *shipped* slot (e.g. `dev_wireframe_color` @224, even renamed) is rendered; one at an uncovered byte range is either silently dropped or never sent, so the write path is narrower than the record table my insert-based experiments showed | The rest of the structure; needed to change the binding layout |
| Device data / programs | Record layout, Oodle frames, frame key, stage from `PSV0` | Nothing for reading and rebuilding |
| Program metadata tails | The engine's per-program resource map, as murmur32 name hashes. Cbuffer entries carry the cbuffer's name (`global_viewport`, `c_per_object`, `material_variable`, `bones`) and its size in bytes; the VS tail lists its input signature names (`POSITION`, `COLOR`, `TEXCOORD`) as `(name_hash, semantic index, ordinal)`. Resource names like `global_texture2D`, `global_samplers` and `static_minlod_sampler` appear next to what look like space/register fields (`31` sits by `static_minlod_sampler`, which renders from space31). Resolvable with `dtmt murmur hash <name> --half` or the dictionary. Framing observed: cbuffers are listed as 24 byte entries whose `{name_hash, size}` sit at `+4`/`+12`, in register order (`{global_viewport, 1776}` then `{c_per_object, 240}` on the UI base's pixel program; only `{c_per_object, 240}` on its vertex program, which uses one cbuffer). `shader43 --slots --hlsl <decompile dir>` uses this to print which decompiled array and slot a variable lands in. Tails are per program and can be all zero (the minimal HUD shader writes none). The per-object cbuffer size differs per shader family (240 bytes on the UI base, 352 on the entity base); growing the UI shader's to 256 with the shader, all tails and all group data headers in agreement still reads zeros in the new slot, so a further structure (contexts/conditions or a derived root signature) gates the per-object buffer size | Full record layout, fields around the hash lists, what an entry means for the engine's root signature and where the register/space pairs are encoded |
| Default data | A table after the device data: `{u32 zero}{u32 count}` then `count` entries of `{name_hash, element_count, blob_offset}` (12 bytes each), then a blob holding the values at their `blob_offset`. Worked example (entity base): `particle_color` has 4 elements at blob `+32` = `1.0 1.0 1.0 1.0`; `material_variable` has 1 element at blob `+48` = `0.0`. The UI base's default data is all zeros (no entries). Mind that `material_variable` is a *variable* of `c_per_object`, not a cbuffer: the entity base's group data has records `particle_color` @320 (float4) and `material_variable` @336 (scalar) inside its 352 byte per-object buffer | What a zero/default value means for the engine (whether it uploads these at all); structure beyond the observed shapes |

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
