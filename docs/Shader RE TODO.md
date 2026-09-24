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

1. **Group descriptors** (`{name_hash, flags, X, Y}`): `X` is the resource's
   byte offset in the per-draw binding table, allocated in descriptor-list order
   (24 bytes per constant buffer, 8 bytes per other resource) - decoded and
   reproducible. `flags` keeps the space in bits 16+ and a small kind in the low
   bits (0 material cbuffer, 1 engine cbuffer, 3 texture, 5 UAV). Still open:
   what `Y` measures (0 for cbuffers, a small count for resources;
   `global_texture2D` is 5 in the UI base's first six groups and 1 in the last
   six, `41B1CFF8` 10 and 2) and the compact copies' exact record order.
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

   Related finding: the engine's `name -> cbuffer offset` upload set is fixed by
   its own compilation of the shipped shader. Moving a known variable's record to
   new space (offset 240 in all 36 copies of the table, tails patched to 256, the
   pixel shader reading the new slot) leaves the slot at zero, so our shaders can
   only consume the values the engine already writes; pass extra values by
   packing them into known `float4` slots. A new family can render our programs,
   but its own new parameter slots will not be uploaded.
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
verify a name against Darktide-only sources (`C:\dev\Darktide-Source-Code` +
the game bundle) before trusting it.

## Toolbox

- `shader43` example: `--section <name>`, `--preamble`, `--tail <n>`,
  `--slots --hlsl <dir>`, `--variables <dict>`, `--decompile <dir>`,
  `--rebuild <dir>`.
- `generate_shader` example: `--preset <out.txt> <material data file>`,
  `--generate <preset> <base.material> <out.material> --vs/--ps`.
- `mine_materials` example: `--dict <csv> --out <dir> <game data dir>`.
- Helper scripts live in `%TEMP%\opencode\dtmt-mat` (cargo/rustfmt wrappers,
  `make-bounties.ps1`, `capture-title.ps1`, `parse-conditions-tree.ps1`,
  `decode-descriptors.ps1`, `unit-headers.ps1`, `words.ps1`, ...).
- Game data: `E:\SteamLibrary\steamapps\common\Warhammer 40,000 DARKTIDE\bundle\data`.
- Dump: `C:\Users\Dan\dtmt-dump` (materials, variables, groups, defaults,
  contexts, conditions, tails, known.txt, unknown.txt).
