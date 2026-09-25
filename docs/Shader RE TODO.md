# Shader RE TODO

Working notes and next steps for the `shader43` reverse engineering. See
`File Type - Material.-.md` (field-level findings) and
`Shader Section Generation Notes.md` (the target and what each section needs).

## Goal

Everything a shader needs is defined in the mod (`<material>.hlsl` sources plus a
shader declaration); `dtmt build` compiles the sources and **generates the whole
`shader43` section**. No shipped shader blob, and ideally no preset either - the
only game-derived data allowed is genuinely engine-side constants, kept as small
as the decode allows.

## Status

**Custom shaders work as long as they are based on an existing shader
interface.** Verified in game, a mod can:

- ship its own programs (HLSL compiled with `dxc`, spliced into a generated
  section);
- bind its own textures through the library's channels;
- drive values for every variable the library knows, from the material SJSON or
  from Lua - variables bind **by name**, and a name the library does not know
  stays out of the layout (up to four floats per known vector4 slot).

What does not work yet:

- renaming or adding material variables, or adding channels - the name set,
  offsets and channel list are the library's compiled interface;
- a **custom interface** (our own cbuffers, resources and slots). The interface
  is carried by data we can only copy today: the device **block** (the preamble
  tail, repeated after each pixel program), the packed group-data copies, the
  group descriptors' `Y` field and the program tails' resource lists. They are
  understood well enough to patch consistently, not yet enough to write.

## Done

- `shader43` section codec: parse/rebuild, Oodle frames, stage from `PSV0`,
  interface check (`lib/sdk/src/filetype/shader.rs`).
- Material-side flows: sibling `.hlsl` compile + splice; `shader_preset`
  declaration in a material SJSON makes `dtmt build` generate the entire section
  from a preset + compiled sources (`crates/dtmt/src/cmd/build.rs`,
  `lib/sdk/src/filetype/shader_preset.rs`, example
  `lib/sdk/examples/generate_shader.rs`). snoopymod runs this way; its base
  material is ~427 bytes and the preset is the only game-derived file.
- Verified in game: a generated section renders (title screen tint driven by Lua
  material values); a generated section *without* the device preamble makes the
  engine run out of memory at the title.
- `mine_materials` example: dumps `materials/variables/groups/defaults/contexts/
  conditions/tails.csv` for the whole game (2037 shader materials) and writes
  `known.txt`/`unknown.txt` hash bounty lists.
- Decoded (partially): contexts (`{name_hash, u32, count, count × {query_id,
  conditions_offset}}`), conditions tree (records `{tag, b, c, count}` +
  hashes + u16 payload; names are channels: `gui`, `red`, `green`, `blue`,
  `alpha`, `fog_volume`, `linear_depth`, ...), group header descriptor shape
  `{name_hash, flags, X, Y}`, group variable tables, default data table,
  program tails (cbuffer entries + signature lists + resource records for
  textures/UAVs/samplers with space fields and a bindless sentinel).

## Next steps

**Next action (decides whether custom interfaces are a compiler problem or a
dead end): decode the block's record grammar.** The block is the one structure
whose authority is still unproven, and the name-to-slot map lives in it or
beside it. Method: dump the device preamble of families with known, different
variable sets (the miner's CSVs give the sets and hashes), diff them and locate
the record for a known variable (e.g. `dev_wireframe_color` = `795CF4A7` on the
UI base family). Then clone that record with a new name hash and offset, patch
it consistently (block, canonical records, packed copies, descriptors, tails)
and test in game whether a material declaring the new name binds. If the block
can be grown, generation is a matter of modelling its record stream; if not, the
interface is engine-compiled and custom base materials stay bound to shipped
families. Supporting decodes, as the experiment needs them: descriptor `Y`
semantics, the packed copies' generation grammar, and the tail resource-list
kinds.

1. **Group descriptors** (`{name_hash, flags, X, Y}`): `X` is the resource's
   byte offset in the per-draw binding table, allocated in descriptor-list order
   (24 bytes per constant buffer, 8 bytes per other resource) - decoded and
   reproducible. `flags` keeps the space in bits 16+ and a small kind in the low
   bits (0 material cbuffer, 1 engine cbuffer, 3 texture, 5 UAV). Still open:
   what `Y` measures - but its *shape* is now clear: it is a packed array of 16 two-bit fields (values 0..3). Vertex-data resources take exactly `1, 5, 21, 85, 341` = `sum(4^i)` (`bones`, `idata`, `hmap`), which is what "one count in each of 1, 2, 4, 5 slots" looks like, and the UI base's `41B1CFF8` reading 10 = `2 + 8` next to `global_texture2D`'s 5 = `1 + 4` is the same packing with the fields at 2. So a likely reading is "per program/pass, how many times the group binds this resource", saturating at 3 per field; the exact slot meaning is still to confirm. Also open: the compact copies' exact record order.
2. **Program tails from DXBC**: the tail is the per-program binding map and it
   is load-bearing - zeroing everything but the cbuffer entries crashes the game
   at shader load (`dispatch_loadtime`, `shader #ID[<the group's query id>]`),
   restoring them renders again. Decoded: `{u32 cbuffer_count}` + 24 byte
   cbuffer entries (`{name_hash, size}` at entry `+0`/`+8`, register order), then
   counted lists (empty = one `0` word) carrying 7 word resource records
   (textures, UAVs with kind 3 and `0xFFFFFFFF` bindless + space, samplers) and
   3 word signature runs (`POSITION`/`COLOR`/`TEXCOORD`/`CUSTOM`/... resolved
   hashes; the pixel program's interpolated-input run has no count of its own,
   its length matching the container's `ISG1`), ending with the shared block
   (the preamble's tail, 549 bytes on the UI base). `shader::Tail` parses and
   serialises the cbuffer list and round-trips every sampled tail byte for byte,
   `patch_tails` rewrites sizes in a preset (hygiene: a 256 byte shader cbuffer
   with 240 byte tails still renders, so the size is not load-bearing). The
   shared block is *byte packed*, not word aligned - `texture_map` sits at an odd
   offset (501) inside it with `{u32 size = 4, u32 count = 1}` after it, so
   modelling it needs the packed record stream, not u32 lists. Next step: model
   the resource/signature lists (their counts, kinds and how many words each
   kind uses) and the block, so a tail can be generated from the compiled
   container's reflection instead of the preset's bytes.

   Current understanding, verified in game: **material variables bind by name**
   to the shader library's own variable names (its compiled cbuffer members and
   offsets). A known name fills its slot wherever it sits in the material; a name
   the library does not know stays out of the layout (verified: unknown
   `zz_probe_a` declared first plus known `dev_wireframe_color` second has the
   shader reading the known one's colour, and only-unknown names leave the slot
   at zero; the known name driven from Lua gives the full hue rotation). The
   material's `offset` field is an offset into the material's `variable_data`,
   not a cbuffer offset. So a mod drives the library's known variable values and
   channels and ships its own programs, but cannot rename or add parameters.
2b. **Generator plan** - what a generated section is made of:

| Piece | Source |
| --- | --- |
| Header, section offsets | generated |
| Contexts | generated (one `default` query, `0xFFFFFFFF`) or the family's |
| Conditions | empty for a single group, the family's for permutations |
| Dependencies (8 bytes) | the family's |
| Group data: units, descriptors | generated (`X` = running 24/8 byte allocation, `flags` = space/kind) |
| Group data: canonical tables | **ours**, from the material's channels and variables - this is the upload layout |
| Group data: packed copies | the **library's** (required to parse, not used for uploads) |
| Device: programs | ours (built from our HLSL) |
| Device: tails | cbuffer list ours, resource lists the library's |
| Device: preamble/block | the **library's** compiled interface |
| Default data | ours (empty, or the material's defaults) |

   The library constants (block, packed copies, resource lists, engine cbuffer
   variable names) are a small per-family file; everything else the tool can
   write. New *channel names* still require the library's block to already list
   them, since the block is the library's own record set.

2c. **Mod-defined parameters: what works** - the library's known variable names,
   driven from Lua or the material (`dev_wireframe_color` on the UI base, a
   float4 at offset 224, is the one custom slot its tables expose), plus its
   texture channels and our own programs. Values can be packed four floats at a
   time into a known vector4. Renaming or adding parameters does not work: the
   name set and offsets are the library's compiled interface.
3. **Conditions payload**: decode the u16 list per node (structure, names and
   node bounds are known). A material that does not permute anything needs no
   conditions at all: the minimal two program material ships an empty conditions
   section and a single `default` context pair `{query_id, 0xFFFFFFFF}`, with
   the group header carrying that same query id. So this only has to be decoded
   to support several groups/permutations in one material.
4. **Device preamble**: split the engine-constant middle from the per-material
   suffix (two same-shader materials differ by one list entry). Lead: the
   preamble's first words track the material's contexts - `{1, query_count, 2, …}`
   reads `{1, 1, 2, 0, …}` on the minimal one context material and `{1, 36, 2,
   37, 30, …}` on the UI base (36 = 30 + 6 queries), and the tail of the
   preamble holds the material's variable list (`texture_map` sits at `+0x1F4`).
   Next step: dump the preamble of a few hundred varied materials next to their
   contexts/conditions/programs and fit the table, then generate it.
5. **Mod-side shader declaration**: a small file next to the material (entry
   points, channels, variables/defaults, which engine-constant file to use),
   wired into `dtmt build`; then a **new family** (new root shader material)
   generated end to end and verified in game.

## Bounties

`unknown.txt` (hashes seen in Darktide shader structures that the dictionary
cannot name) is generated by the miner. Caveat: 32-bit short hashes collide
(about 5 of ~2100 resolved names are wrong - VT2 leftovers like
`dwarf_cave_rock_*`, `weave_death_ground_264`, `pes_*`). The fix if needed:
verify a name against Darktide-only sources (a local dump of the game's decompiled Lua/scripts +
the game bundle) before trusting it.

## Toolbox

- `shader43` example: `--section <name>`, `--preamble`, `--tail <n>`,
  `--slots --hlsl <dir>`, `--variables <dict>`, `--decompile <dir>`,
  `--rebuild <dir>`.
- `generate_shader` example: `--preset <out.txt> <material data file>`,
  `--generate <preset> <base.material> <out.material> --vs/--ps`.
- `mine_materials` example: `--dict <csv> --out <dir> <game data dir>`.
- Helper scripts (cargo/rustfmt wrappers, capture and analysis scripts for the
  desktop, e.g. title-screen captures and condition/descriptor dumps).
- Game data: the install's `bundle/data` directory.
- Miner output: `materials.csv`, `variables.csv`, `groups.csv`, `defaults.csv`,
  `contexts.csv`, `conditions.csv`, `tails.csv`, `known.txt`, `unknown.txt`.
