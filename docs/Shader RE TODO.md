# Shader RE TODO

Working notes and next steps for the `shader43` reverse engineering. See
`File Type - Material.-.md` (field-level findings) and
`Shader Section Generation Notes.md` (the target and what each section needs).

## Priority order

1. **Group data channel records** (current): parse the canonical and packed
   framings precisely so a cloned channel can be inserted with correct counts -
   the last piece before a family can add its own texture channel.
2. **New-family path proper**: generate a whole family (block config, packed
   copies, tail resource lists, conditions), not just patch a shipped one.
3. **Unit workstream**: streamed meshes first, then skins/animations.
4. **Particles**: a new file type (compile/decompile) so particle-based mods
   like RainbowFlame can be replicated.

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

- **adding** material variables and **adding or renaming channels**. Renaming a
  variable works (verified in game, see below); a cloned variable and a renamed
  channel are implemented and awaiting their in-game observations;
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
- Verified in game: **renaming a library variable works**. `dev_wireframe_color`
  (b1 + 224) was renamed to `mod_tint` in the group data (216 occurrences), the
  preamble and all 36 tails; the material declares `mod_tint`, Lua drives it,
  and the title background cycles hue under Lua control (its UV ripple and
  brightness pulse are the demo shader's own effects). The engine resolves
  material variables by the library's compiled names, and the name set is
  patchable. Deployed next: a `clone dev_wireframe_color mod_extra 240 16`
  build (tails grown to 256 bytes, the shader reads `_25_m0[15]`) to test
  **adding** a variable; observation pending.
- `mine_materials` example: dumps `materials/variables/groups/defaults/contexts/
  conditions/tails.csv` for the whole game (2037 shader materials) and writes
  `known.txt`/`unknown.txt` hash bounty lists.
- Family census (from `variables.csv`): of 2036 mined materials and 3353
  distinct variable names, 129 names appear in at least half of the materials
  (the engine variables), and the material-specific remainder ranges from 0 to
  86 per material with a long tail (most materials have 4 to 40). The per-file
  ranking was joined with a dump of every bundle's entries (the entry's
  `dfn=...` field maps a data file to its resource name hash, which the
  dictionaries resolve). The richest families are FX materials:
  `content/fx/materials/abilities/cryptic_force_field_02` (86),
  `content/weapons/materials/weapon_power_sword/weapon_power_effect_cryptic`
  (62), `content/fx/materials/master/wind_render` (61). Many had no
  dictionary entry, so a mod that needs many existing parameter slots should
  prefer the named ones or use the `clone` preset lines.
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

Update: variables and cbuffers turned out not to be in the block (see the block
notes below), so the block's remaining authority is resources/channels - and its
record stream is now framed: record lengths follow the record's `kind` (4 -> 60
bytes, 5 -> 73 bytes), the engine prologue (`linear_depth`, `global_diffuse_map`,
`sun_shadow_map`, `fog_volume`) is at fixed offsets and the stream ends exactly
at the preamble's end. `shader43 --records` parses it end to end on three shipped
families, and a `clone_channel` preset line clones a record into the preamble,
every tail's block and the group data's variable records (unit-tested). In game
the cloned name **binds**: with the clone and a material naming it, the title
screen renders the mod texture with the cycling tint (a block-only clone left
the title black).

Block notes from a byte-precise dump of the chain base (863 byte preamble):

- The channel records are **byte packed**, not word aligned: the `bca` record
  starts at `+0x283` (`{u32 3, u32 bca_hash, u32 5, u32 1, ...}`) and its hash
  sits at `+0x287`. Record strides differ with content (`bca` -> `orm` is 74
  bytes, `orm` -> `nm` is 73), so records are variable length with the format
  flags from `texture_format_spec.config` in their tails.
- `orm` and `nm` are preceded by a different word than `bca` (`c5 0a 8d bd`
  before the `orm` hash), so the word before the hash is not a stable "kind".
- Channel records appear in the preamble *and* after every pixel program (the
  same block), so a channel rename has to patch every copy. The group data
  carries the same names in its canonical records and in the cbuffer-keyed
  packed form, so a rename must patch all three places. This is implemented as
  the `channel <shipped> <new>` preset line (`replace_hash`): it replaces the
  4 byte name hash in the group data, the preamble and every tail. Verified
  offline against the UI base (264 occurrences: 216 group, 48 device). In-game
  verification of a renamed channel is still pending.
- Section-level evidence from an external binary-patch mod (RainbowFlame),
  diffing a patched shipped material against the original: the programs are
  replaced, the **tails** change only in their per-program cbuffer lists
  (`c_material_exports` 80 -> 112 bytes, one program gains a `c_billboard:144`
  entry), the **group data** grows by 40 bytes (variable records and count
  words) and the **default data** by 44 bytes. The **preamble/block, contexts,
  conditions and dependencies are byte-identical**. Cbuffers and variables are
  therefore not part of the block: their interface lives in the group data's
  name-to-slot tables and the per-program tails, and both sizes and entries can
  change. The block stays the library's - the remaining authority for
  resources/channels. (Our earlier `clone` test that grew the UI base's cbuffer
  rendered black, so something else about that patch was wrong - most likely the
  base material's variable list, which RainbowFlame patches in its materials as
  well.)
- Channel records in the block: the record stream sits at the end of the
  preamble and parses cleanly. A record is `{u32 name_hash, u32 kind,
  u32 count = 1, ...}` and its length is fixed by `kind`: **kind 4 -> 60 bytes,
  kind 5 -> 73 bytes** (kind 2 is `global_texture2D`, which appears in the
  per-pixel blocks only). The stream parses end to end with `shader43
  --records`, exactly reaching the preamble's end (staff-49: 6 records from
  `+0x20F` to `+0x391`; the enemy warpfire material `bb79ba7a5b92d132`: 7
  records from `+0x20F` to `+0x3E7`; the UI base: 1 record at `+0x1F5`). Every
  family has the same engine prologue - `linear_depth` (kind 4, `+0x20F`),
  `global_diffuse_map` (kind 4, `+0x24B`), `sun_shadow_map` (kind 5, `+0x287`)
  and `fog_volume` (kind 4, `+0x2D0`) - and family channels follow (the first at
  `+0x30C`). The stream is preceded by its **record count** in the word right
  before the first record (1 for the UI base, 6 for staff-49), and a clone has
  to bump it. In-game channel-clone tests: cloning the block record **and** the
  group data variable record made the game fail with an out-of-memory fatal
  error at boot (the same signature as a malformed device preamble), and fixing
  the count word alone did not help; a **block-only** clone (preamble record +
  count, no group data, no tails) boots normally. The group data clone is
  therefore wrong and was dropped. The per-pixel blocks repeat the stream with
  kind 2 records mixed in, whose length is still unknown, so tail blocks are not
  cloned yet. Inspecting the failing group data shows an unrelated record
  changed (`camera_world` became `world`), i.e. the generic run heuristic
  inserted clones at false-positive record occurrences; cloning a channel's
  group data needs a precise per-table parser instead. In game, the block-only
  clone boots but the title texture stays black: the cloned channel does not
  bind, so the group data records (not the block record) are what the engine
  resolves when a material names a channel.
- The group data holds a channel in **two framings**, 3 records each per group
  unit (108 + 108 = 216 for the UI base's `texture_map`; `shader43 --channel
  <name>` dumps them). **Canonical** 20-byte records
  `{type, flags, name_hash, cbuffer_offset, size}`: type 5/offset 0/size 4,
  type 1/offset 4/size 8, type 1/offset 16/size 8. **Packed** copies sit in a
  run of their own: a `{u32 count = 3}` word, then 28-byte records
  `{name_hash, a, b, c, d, B5639618, 0}` (the `c_per_object` hash near the end),
  e.g. `{hash, 0, 0, 4, 1, B5639618, 0}`. A channel clone therefore needs 3
  canonical + 3 packed insertions per group unit (216 total for the UI base)
  with the run counts bumped. The records are byte-packed (any alignment): the
  UI base's canonical records sit at 4-, 2- and 1-byte-aligned offsets, so the
  scan probes every byte offset and the run/count check rejects the shifted
  views. `clone_channel_group_data` inserts all 216 records on the UI base
  (108 canonical + 108 packed, unit-tested) and is **verified in game**: with
  the block record, the group data clone and the material naming the new
  channel, the title screen renders the mod texture with the cycling tint,
  where the block-only clone stayed black. The flag words are byte-packed, and a record's tail carries the
  same packed 2-bit-per-slot usage counts as the descriptor `Y` field (`0x15` =
  21 = 1+4+16, `0x55` = 85 = 1+4+16+64). A channel can therefore be added by
  cloning a record of the same kind and substituting the name hash, the same
  mechanism as `clone_variable`, now with a known record length. The miner's
  dictionary CSV carries the murmur32 hash in its third column, which is how
  `sun_shadow_map` was resolved.

1. **Group descriptors** (`{name_hash, flags, X, Y}`): `X` is the resource's
   byte offset in the per-draw binding table, allocated in descriptor-list order
   (24 bytes per constant buffer, 8 bytes per other resource) - decoded and
   reproducible. `flags` keeps the space in bits 16+ and a small kind in the low
   bits (0 material cbuffer, 1 engine cbuffer, 3 texture, 5 UAV). Still open:
   what `Y` measures - but its *shape* is now clear: it is a packed array of 16 two-bit fields (values 0..3). Vertex-data resources take exactly `1, 5, 21, 85, 341` = `sum(4^i)` (`bones`, `idata`, `hmap`), which is what "one count in each of 1, 2, 4, 5 slots" looks like, and the UI base's `41B1CFF8` reading 10 = `2 + 8` next to `global_texture2D`'s 5 = `1 + 4` is the same packing with the fields at 2. So a likely reading is "per program/pass, how many times the group binds this resource", saturating at 3 per field; the exact slot meaning is still to confirm. Also open: the compact copies' exact record order.

   The group data's overall structure is mapped now (UI base, 62,884 bytes):
   a 32-byte global header (`{u32 group_count = 36, u32 library_hash,
   u32 0x130, u32 4, u32 c_per_object, 0, 0, 0}`), then **36 groups**, each
   holding its descriptors (3 x 16 bytes, above), the channel table (`{u32 2,
   u32 count = 7}` + 7 canonical 20-byte records), the variable table (`{u32 2,
   u32 count = 69}` + 69 canonical records), the packed copies (`{u32, u32 0,
   u32 count = 3}` + 3 packed 28-byte records) and a byte-packed group header
   (74 bytes in groups 0-11, 57 in 12-34, 29 in the last; it carries the
   permutation's hash, e.g. `9FCFE126` in 12 groups). The channel table, variable
   table and packed run are **byte-identical across all 36 groups**; only the
   descriptors' `Y` and the group header vary (`Y` = `{0, 5, 10}` in groups 0-11,
   `{0, 1, 2}` from group 12 on - the per-permutation binding counts). The
   "compact copies" are the same records re-emitted per group at whatever byte
   alignment the group lands on (the group size, 1758/1741 bytes, is not
   4-divisible), not a different encoding. Generation can therefore emit the
   tables once and replicate them across the template's group count, keeping the
   template's descriptors and headers.
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
   channels and ships its own programs; renaming a variable works, adding one is
   under test.
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
   time into a known vector4. Renaming a parameter works (verified in game:
   `dev_wireframe_color` -> `mod_tint`); adding one is under test (a clone at
   offset 240 is deployed). The set of offsets is still the library's.
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
- `compile_unit` example: `<unit> <bsi> <out payload>` compiles an authoring
  pair into the runtime payload (the same code the build uses).
- `decompile_unit` example: `<payload> <out dir>` writes the `.unit`/`.bsi`
  pair back out; static units only (skins, animations and actors are rejected
  with a clear error).
- `generate_shader` example: `--preset <out.txt> <material data file>`,
  `--generate <preset> <base.material> <out.material> --vs/--ps`.
- `mine_materials` example: `--dict <csv> --out <dir> <game data dir>`.
- Helper scripts (cargo/rustfmt wrappers, capture and analysis scripts for the
  desktop, e.g. title-screen captures and condition/descriptor dumps).
- Game data: the install's `bundle/data` directory.
- Miner output: `materials.csv`, `variables.csv`, `groups.csv`, `defaults.csv`,
  `contexts.csv`, `conditions.csv`, `tails.csv`, `known.txt`, `unknown.txt`.
