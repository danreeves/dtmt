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

### Custom material parameters

A base material's shader library decides which material variables exist and
where they live in the material constant buffer. Two preset lines adapt that
interface when a section is generated:

- `variable <shipped> <name> <offset> <size>` re-purposes a shipped slot: the
  shipped variable's 20 byte record is rewritten to the new name, offset and
  size everywhere it occurs (canonical records and the verbatim copies the
  packed serialization embeds), and the slot is no longer addressable under its
  old name.
- `clone <template> <name> <offset> <size>` adds a slot: a copy of the
  template's record is appended to every run of records that contains it, with
  the run's count word bumped. The template stays intact.
- `channel <shipped> <new-name>` renames a texture channel: the shipped channel's
  32 bit name hash is replaced wherever it occurs - the group data (canonical
  records and the cbuffer-keyed packed copies), the device preamble and every
  program tail's block - so the library, the material and the shader agree on
  the new name. Offsets, registers and texture formats are untouched, so the
  material declares the texture under the new name and the shader keeps sampling
  the same register. Verified against the UI base: `channel texture_map mod_map`
  replaced all 264 occurrences (216 in the group data, 48 in the device data).

Both are applied in `Preset::generate_with_report`, before the section is
assembled, and both grow the constant buffer in the program tails when the new
offset reaches past the shipped buffer end.

Three declarations have to agree for a slot to be readable:

1. the **shader** must declare the constant buffer large enough. On the UI base
   the material buffer is `b1` and the decompiled HLSL declares
   `float4 _25_m0[15]` (240 bytes); a slot at offset 240 needs
   `float4 _25_m0[16]` (256 bytes). Members are read as `_25_m0[offset / 16]`.
2. the **program tails** must mention the larger size. DTMT rewrites every tail
   entry whose size covered the old range and is smaller than the new one
   (`grow_tails`), so this is automatic.
3. the **group data** records must place the new member inside that buffer (the
   `variable`/`clone` lines do that).

For the mod's title screen the shipped `dev_wireframe_color` lives at offset
224 of the UI base's material buffer, so cloning it at offset 240 and reading
`_25_m0[15]` gives the mod a slot of its own: the material SJSON declares
`mod_extra = { type = "vector4" value = [1, 1, 1, 1] }` and Lua drives it
through `material_values`. Verified offline (36 records added to 36 runs, tails
grown to 256 in all 96 programs); in-game verification pending.

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
| Group data | Variable tables: runs of `{type, flags, name_hash, cbuffer_offset, size}` records preceded by their count, one table per cbuffer per group, stored several times per group (the UI base has 36 copies of its per-object table, including a compact serialization after the first tables). Each group's header starts `{u32, query_id, byte_size?, descriptor_count, descriptor...}` where `query_id` is the context entry that selects the group (the UI base's 12 groups reference every second query id of the `default` context), followed by a resource descriptor list `{name_hash, flags, X, Y}` (`c_per_object`, `global_viewport` 0x101, `global_texture2D` 0x103, …) and then the group's variable tables. **Descriptor `X` is the resource's byte offset in the per-draw binding table, allocated in descriptor-list order: 24 bytes per constant buffer, 8 bytes per other resource** (verified in the UI base: `c_per_object` 0 → 24, `global_viewport` 24 → 48, `global_texture2D` 48 → 56, `41B1CFF8` 56; and in a small two-program material where the descriptors lay out as 0/24/32/40/48/56/80/88/96/104/112 exactly). `flags` keeps the resource's space in bits 16+ and a small kind in the low bits (0 = material cbuffer, 1 = engine cbuffer, 3 = texture, 5 = UAV; `c_per_object` 0x0, `global_viewport` 0x101, `fog_volume` 0x10003 = space 1, `global_diffuse_map` 0x20103 = space 2 + bit 8, `reflection_map` 0x50003 = space 5). Across the corpus (2037 materials, 80465 table headers) `flags` is stable per resource name while `X` varies per family. The compact serialization stores the descriptor list again as 16-byte records `{name_hash (4 bytes in display order), flags, X, Y}` and the variables as `{cbuffer_hash, …}` records; in-place renames and appended records patched across all copies were accepted by the build and rendered correctly, so the copies can be edited by hash without understanding every header word. **Gap slots are dead space**: a record put into uncovered bytes (the UI base's 8..16 / 24..32 gaps) is accepted, Lua drives it and the shader reads the right slot, but the engine never uploads it, so only slots the engine itself fills are writable. Group data carries the tables **twice in two formats**: the canonical 20 byte records and, at the end of every unit (UI base: from unit + 1628), a *packed copy* that stores the same descriptor list with the hash in display byte order and `u16` fields (e.g. `global_viewport` as `51 6D 5C CD`, `0x101`, `0x18`, `0x0000`), plus variable records. Zeroing that packed region (or the block, or the tail resource lists) makes the game fail at load, so a generated unit has to emit both forms. The compact copy is a packed serialization of the unit's own data and is required (zeroing it fails load). Its opening on the UI base's unit 1 is `{1776, 0, 3}` and it then interleaves: the per-object table's records in a seven word shape that repeats the canonical fields (e.g. `texture_map, 5, 0, 0, c_per_object, texture_map, 0, 0, 4` for the table's first record - names little endian here), the permutation name `gui` (`9FCFE126`), and the descriptor list with big-endian `u16` fields instead of `u32`s (`global_viewport` as `5C CD 51 6D` then `01 01 00 18 00 00` for flags `0x101`, `X` 24, `Y` 0; `global_texture2D` as `63 6C 3A FC` then `01 03 00 30 00 05`; `41B1CFF8` as `CF F8 41 B1` then `01 05 00 38 00 0A`) - the same 16-bit-pair byte swap the block uses at odd offsets. Record boundaries inside it are the remaining fitting target. Where the engine gets the upload layout from is **solved: material variables bind by name**. The material's `variables` block maps each name to the shader library's own variable names (the cbuffer members as the engine compiled them, offsets included); a name the library knows fills its slot no matter where it sits in the material, and a name it does not know stays out of the layout entirely. Verified in game: a material declaring an unknown `zz_probe_a` first and the known `dev_wireframe_color` second has the shader (reading offset 224) show the known one's colour; declaring only unknown names leaves the slot at zero; and the known name driven from Lua produces the full hue rotation. The material's own `offset` field is an offset into the material's `variable_data`, not a cbuffer offset. So a mod can drive the library's known variable values (from Lua or the material), bind its textures through the library's channels and ship its own programs, but it cannot rename or add parameters - the name set and offsets are the library's compiled interface. The material's byte-packed *block* (the preamble's tail, repeated after each pixel program) is the **compiled material interface**: it holds records `{name_hash, u32 a, u32 count}` plus flag/space-looking words (`0x100`, `0x200`, `0x10000`), and names that are *authored* resource names like `curve_texture_43db3c3b` - the kind the engine editor writes into a material/shader node when a curve texture is set up, so they live in authoring data we can produce - in materials that declare no channels of their own. It is *validated at load*: zeroing everything after the 12 byte header (`{1, query_count, 2}`) makes the game fail to load the resource, while keeping only the first 64 bytes loads and then runs out of memory when the material is drawn. So it must be generated for a new layout, and its record grammar is the last big unknown. The block is *not* where material variable uploads come from: a fresh base material with a new name (`mod_probe`) at a new offset (240) was driven from Lua and reached the shader while the block was still the shipped one - the group data's variable tables are the upload layout's source. Its skeleton is now legible from small samples: `{u32 1, u32 query_count, u32 2, u32 0, u32 record_count}` then `record_count` records and a byte tail (87 bytes = 84 + 3, 100 = 96 + 4, i.e. sizes are not word multiples). A texture/channel record is 15 words: `{name_hash, u32 components (4 = RGBA, 3 = RGB), u32 1, ...}` followed by flag words (`0x100`/`0x300`, `0x200`, `0x10000`/`0x30000`, `0x30000`/`0x10000`, `0x1000000`/`0x3000000`, a constant `0x15`) whose values track the component count - matching `texture_format_spec.config`'s format rules on the authoring side. Other records start with small type words (`{3, 0, 2, 0x300, ...}`) and can carry long (64 bit) hashes, so they look like the material's non-texture variables (curve maps and friends). The tail's cbuffer size is not load-bearing either: a 256 byte `c_per_object` in the HLSL with the tails left at 240 still renders, so `patch_tails` is hygiene rather than a requirement. The material's own `offset` field is not authoritative (the working mod declares `dev_wireframe_color` at offset 0 while the engine writes it at the table's 224). `shader43 --variables/--slots` read the tables | What descriptor `Y` measures exactly - though its *shape* is a packed array of 16 two-bit fields (values 0..3): the vertex-data resources take exactly `1, 5, 21, 85, 341` = `sum(4^i)`, and `41B1CFF8` reading 10 = `2 + 8` next to `global_texture2D`'s 5 = `1 + 4` fits the same packing, so it reads like "per program/pass, how many times this group binds the resource", saturating at 3 per field - and the compact variable record's exact field order |
| Device data / programs | Record layout, Oodle frames, frame key, stage from `PSV0`. The device data starts with a packed preamble (a lookup table with increasing indices, small values and variable name hashes; 561 bytes for the UI base's 96 programs) before the first program record; a generated section without it makes the engine run out of memory when a material using it is drawn. What the preamble is: a shared table that two same-shader base materials match on byte for byte apart from one list entry (`count 1 -> 2` plus a name hash) and a 60 byte suffix, and whose long middle section is identical across *different* shaders too, so it holds engine-side variable reflection plus the material's own entries; the header starts `{1, A, B, C, 30, 0, 0, 768}`. Its first three words are `{1, query_count, 2}` (the minimal one-query material reads `{1, 1, 2, 0}` and the UI base, 30 + 6 queries, reads `{1, 36, 2, 37, 30, …}`), and the block after them is the *same* block that shows up at the end of every pixel program's tail (the UI pixel tails are ~805 bytes because the tail region extends over it before the next program record; `texture_map` sits at `+0x1F5` inside it), so it is the per-pipeline piece rather than a purely global one. The block is the material's **compiled interface**. On the smaller materials the model is exact: `{u32 1, u32 query_count, u32 2, u32 0, u32 record_count}` then `record_count` 15 word records and a 7 or 20 byte tail (validated across the sampled materials), where a texture/channel record is `{name_hash, u32 components (4 = RGBA, 3 = RGB), u32 1, ...}` followed by flag words (`0x100`/`0x300`, `0x200`, `0x10000`/`0x30000`, `0x30000`/`0x10000`, `0x1000000`/`0x3000000`, a constant `0x15`) that track the component count - the same rules the authoring side's `texture_format_spec.config` states - and the other records start with small type words (`{3, 0, 2, 0x300, ...}`) and can carry 64 bit hashes. The UI family's block is much richer (549 bytes) and packs fields at odd byte offsets, so it is the same kind of record stream at a finer level of detail and is still to be fitted. The block is validated at load: zeroing everything after the header makes the game fail to load the resource, keeping only the first 64 bytes loads and then runs out of memory when the material is drawn, and the full block works. Replacing it with a *generated* minimal block (one `texture_map` record shaped like the sampled families', plus a zero tail) also fails at shader load (`dispatch_loadtime`, `shader #ID[<the group's query id>]`), so a block is the *shader library's* compiled interface: it can be recognised and modelled (below) but for a given library it must be that library's own - a genuine engine constant to carry per family, not something a material can synthesise. `shader43 --preamble --dump <dir> <material data file>` dumps it | The richer layout's field widths, and how a block is generated for a new channel set |
| Program metadata tails | The engine's per-program resource map, as murmur32 name hashes. The tail opens with `{u32 cbuffer_count}` then 24 byte cbuffer entries (`{name_hash, size}` at entry `+0`/`+8`, i.e. bytes `+4`/`+12`, in register order: `{global_viewport, 1776}` then `{c_per_object, 240}` on the UI base's pixel program, only `{c_per_object, 240}` on its vertex program), then a sequence of counted lists in which an empty list is a single `0` word. Observed pixel layout (UI base): `0, 0, 0, 1, {global_texture2D, 2, 0, 0xFFFFFFFF, 2, 0xFFFFFFFF, 0}, 0, 1, {41B1CFF8, 3, 0, 0xFFFFFFFF, 31, 0xFFFFFFFF, 0}, 1, {static_minlod_sampler, 0, 1, 31, 4, DC0548BC, 0}, 0`: a 7 word texture array record, a 7 word UAV record (kind 3, `0xFFFFFFFF` bindless, `31` space) and a 7 word sampler record in three separate lists. The vertex program instead reads seven `0`s then `3, POSITION@0:0, COLOR@0:1, TEXCOORD@0:2` - its counted input list. Pixel programs then carry the *interpolated inputs* as 3 word `{name_hash, semantic index, ordinal}` runs whose length follows the container's `ISG1` rather than a count of their own, followed by more counted lists such as `{1, global_samplers@0:0}`. The semantic name hashes resolve: `POSITION` `3FFEABD6`, `COLOR` `FCDCBA12`, `TEXCOORD` `B77A0F36`, `NORMAL` `7668E94B`, `TANGENT` `E32E5A9D`, `BLENDINDICES` `5F29E5B9`, `BLENDWEIGHT` `98405AD1`, `CUSTOM` `96B9600E`, `global_texture2D` `3AFC636C`, `static_minlod_sampler` `4B42C5E6`, `global_samplers` `DA560F03` (`dtmt murmur hash <name> --half`). A pixel program's tail ends with the *shared block* (the preamble's tail: the UI base's 561 byte preamble is `{1, 36, 2}` + a 549 byte block, and every pixel tail ends with those 549 bytes, so a pixel tail is `[program record][block]`; the record is 252 of the 801 bytes). Tails are per program, identical for programs with the same interface (all 48 UI vertex programs share one 112 byte tail) and can be all zero (the minimal HUD shader writes none). `shader::Tail` parses and serialises the cbuffer list and round-trips every sampled tail byte for byte. **The tails are load-bearing**: rebuilding the UI base with every tail truncated to its cbuffer entries (same byte length, resource lists zeroed) makes the game crash while loading the shader (`stingray::D3D12RenderDevice::dispatch_loadtime`, `shader #ID[31b9724d]`, the group's query id), while restoring the original tails renders the title screen again. The per-object cbuffer size differs per shader family (240 bytes on the UI base, 352 on the entity base), and the size in the entry is **patchable**: growing `c_per_object` in the mod's own HLSL (240 -> 256 bytes) and rewriting that size in every program tail with `Tail::parse`/`Tail::bytes` still renders the title screen, so a generator can adapt a family's tails to a mod's own constant buffer layouts instead of demanding a byte-identical interface. `shader43 --slots --hlsl <decompile dir>` uses the cbuffer entries to print which decompiled array and slot a variable lands in; `shader43 --tails` dumps every program's tail | The lists after the interpolated inputs, the block's internals, and generating a tail from a compiled container's reflection |
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

   Follow-up (verified offline): the UI family's group data is a repeated unit
   structure - 36 units of five tables each, stride 2142 bytes - and the
   interface table (records of kinds 1/3/4/5 at byte offsets up to 304) holds
   the channels *and* the variables. Channels (`texture_map`, `bca`, `nm`,
   `orm`) also appear in the device block; scalar/vector variables
   (`dev_wireframe_color`, `outline_color`, `view_proj`) appear **only** in the
   group data, so the group data is the name-to-slot map. The preset rewrite
   (`variable <slot> <new-name> <offset> <size>`, implemented as
   `patch_variable`: it replaces the exact 20 byte record wherever it occurs)
   was exercised in game as a title-screen test: the shipped
   `dev_wireframe_color` record was renamed to `mod_tint` in all 36 copies while
   keeping offset 224/size 16, the instance material declared `mod_tint`, and
   Lua drove it. **Verified in game**: the title background cycles hue under Lua control, so a material variable binds by its renamed name. DTMT now also supports `clone <template> <name> <offset> <size>` preset lines (`clone_variable`): a copy of the template record is appended to every run of records that contains it, with the run's count word bumped when one is found in the 16 bytes before the run. Unit-tested, and verified against the shipped UI base: all 36 runs grew from 7 to 8 records with the clone at offset 240 and their count words bumped, while the tails were grown to 256 by the same path as a rewrite. Canonical runs only; packed copies of channel records are not cloned yet. The clone is deployed and awaiting its in-game observation.
