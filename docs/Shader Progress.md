# Shader progress: where this stands and what to do next

This is the pick-up point. Read this before the other shader notes; it names
what is verified, with sample sizes, and what is still a reading.

Two goals, in order: **build a shader from source** (declaration + HLSL ->
section) and **bundle -> source -> bundle** (extract, decompile to editable
source, build it back). Both directions are goals. Byte-for-byte round trips are
the codec's correctness oracle, not the goal: a reconstruction is useful even
where it cannot reproduce bytes, and a build is useful even where some engine
constants are carried.

## Repo state (2026-09-30)

`main` is the trusted baseline plus reviewed commits:

```
b9a5bc6 shader: the resource record index rule is exact - 1004/1004
3088c8e shader: the descriptor list starts at +16, word 1 is its entry index
1fb1c1b docs: the built material carries the compiled containers
ccdbe8c build: source material programs from a sibling .shader_node
899b6fe shader: read the real code shapes, compile with DXC
1657273 shader_node: map a job's stages to DXC profiles
cd18f54 shader_node: a compile job carries its stages
e0446a2 shader_node: assemble a code block's HLSL
96b9d2a shader_node: enumerate the compile jobs
2e93df5 docs: the authoring format is the Stingray dialect, HLSL only
2c79541 docs: tail signature runs and trailing run are family-independent   <- trusted baseline
```

`docs/Shader Section Generation Notes.md` and `docs/Shader RE TODO.md` are the
reference; this file is the pick-up point.

## Build and test

- `cargo test -p sdk` needs `E:\SteamLibrary\steamapps\common\Warhammer 40,000
  DARKTIDE\binaries` on `PATH` (links `oo2core_9_win64.dll`).
- Expected: 130 pass, 3 pre-existing `filetype::package` failures (133 tests).
- Fixtures: the six extracted sections and the UI base's parts live in a scratch
  directory outside the repository (each `.raw` is a 20-byte wrapper then the
  section, which the example handles). `docs/scripts/slice-sections.ps1`
  extracts them from a material data file, and the UI base can also be fed to
  the tooling as `uib.material`. The round-trip checks depend on them, so keep a
  copy.
- Dictionary: `dictionary.csv` in this repository; an enriched dictionary is
  available locally for the variable-name substitutions.

Useful commands:

```
shader43 --layout <six .raw>                 # section round trip, expect 6/6
shader43 --substitute --variables <dict> <six .raw>
shader43 --conditions --variables <dict> <section>
shader43 --conditions-map --variables <dict> <section>
shader43 --group-data <section>
shader43 --dependencies <section>
shader43 --plan <declaration.shader_node> <section>
shader43 --reconstruct <dir> [--variables <dict>] [--dxil-spirv <exe>]
    [--spirv-cross <exe>] <section>   # declaration + shader_source + engine_data
shader43 --compile <dir> <declaration.shader_node> <library.shader_source | dir>...
```

## Verified (sample size in brackets)

- **Section layout**: contexts are `{u32 name, u32 word2, u32 count, count x
  {u32 query_id, u32 conditions}}`, variable length, filling
  `[contexts_offset, conditions_offset)` exactly. `conditions_offset` is
  `48 + sum(record lengths)`. [7 sections: six small + UI base]
- **The queries are the groups**: `sum(count)` equals the group data's group
  count, and the first query of the first context is the group data's hash.
  [7/7] `Section::check` enforces it.
- **Queries and groups are one to one**: every query id appears exactly once in
  the group data, in its group's header. [7/7]
- **Group data header**: a 4-byte global count, then back-to-back groups each
  `{query_id, word, count}` - the word is 0x130 and 0x390 in the samples, the
  count is the number of 16-byte descriptors that follow at +12:
  `{name_hash, flags, X, Y}`. The count varies with the shader (4 on the UI
  base, 6 or 7 on `38ECBAD1`, 12 on `3F08AC44`, up to 36). `GroupData::descriptors`
  reads entries 1..3 (the +32/+48/+64 words: `global_viewport`, the texture and
  the UAV on the UI base); entry 0 at +16 is the material's `c_per_object` there.
  The old "one set of three descriptors at +8" and "+32 list" readings are
  corrected. [27 sections walked by `resource_table`]
- **A resource record's second word is its descriptor index**: a 7-word resource
  record's word 1 is the resource's index in the descriptor list of the group its
  program belongs to. [27 sections: 355 programs with 7-word records, 355 fully
  matched by one group, 1004/1004 records] The per-group program counts also
  partition programs among groups with distinct lists (`eb09dd77` 26/26/2/2/16/16,
  `3F08AC44` 7/7/1/2/2), a lead on the program-to-group mapping.
- **There is no link table and no node pool.** Those were 20-byte records read
  at the wrong length; `004F18EA`'s "link" is default's second query,
  `2A04418E`'s `0x1C` is a conditions byte offset. [7/7]
- **Round trip**: byte-identical on all seven sections, including the UI base's
  1436-byte conditions tree. Necessary, not sufficient.
- **Dependencies**: one little-endian u64, Murmur64 of
  `core/stingray_renderer/renderer` (`209FB8C3C0A8C3A4`). [7/7]
- **Group data walk**: a table is the run whose count word four bytes before it
  equals its length. The engine table is the one whose first record is
  `6BC91D73` (69 records, once per group). [7/7] The UI base's material table is
  at `+88` with 7 records; the old scan read `+76` with one.
- **Conditions framing**: `{u16 tag=1, u16 payload_words, u16 payload_offset =
  8 + 4 x count, u16 count} + count u32 hashes + payload_words u16 words`,
  self-delimiting. [UI base: 35 records, starts exactly the 29+6 context
  offsets; six small sections: 0/28/56 bytes]
- **Conditions payload, a reading**: guarded results. `0x20xx` is a test over
  the record's hashes, `0x10xx` the result, `0x70xx` a jump to the record's
  `0x9000` end after a taken result, `0x50xx` the fallback, `0x90xx` the end.
  Every jump target is the record's own end word, and every branch's result is
  its test count minus one. [35/35 records; `Node::branches` in the code]
- **Declaration front end**: reads the 15 real `.shader_node` files; `define=`
  and `defines=`; choice-level `permute_with` recursion (91 uses); stage-limited
  macros; declaration channel order. [15 files, manual run + unit tests]
- **Decompile to source, first slice**: `shader43 --reconstruct` writes a
  `.shader_node` skeleton from a section - the group data's named variables and
  channels become `inputs` and `channels`, the resolvable contexts become
  `shader_contexts` - and the result parses back through the declaration reader
  (`427B5E6E`: 32 inputs, 13 channels, 2 contexts; the UI base: 2 contexts).
  Permutation sets and HLSL are compiled away and named in the header comment.
- **Conditions roots**: `gui` 9FCFE126, `red` 9B8DE7E4, `green` 4BA4BD58,
  `blue` 0977913D, `alpha` 3F697354; `BDF72706`, `B5F45768`, `8FB860CF`,
  `E2C8865F`, `BC4EE226` unnamed. [dictionary + 35 records]
- **Code block shape**: a `.shader_node` code block's body is
  `code = { shared = ..., hlsl = ... }` or a bare string; `include` names
  library chunks (`path#chunk`) or other code blocks of the same declaration,
  and an include is taken once. The top-level `hlsl` field the reader first
  read occurs **0 times**; there are 14 `code` tables across the 15
  output-node declarations. [14 blocks, 15 files]
- **Library shape**: a `.shader_source` chunk carries `code` (a string, or the
  same `shared`/`hlsl`/`glsl` table) and `includes` (chunk names, resolved
  recursively); a file carries `includes` as full paths. Top-level chunk
  `hlsl`/`glsl` fields occur 0 times. [134 `code` chunks, 117 chunk
  `includes`, one table form: `skinning`]
- **Programs are DXIL**: every shipped program's container carries a `DXIL`
  chunk (SM 6.x), so the compile profiles are `vs_6_0`/`ps_6_0`. The `DXBC`
  magic is the container format, which DXIL shares; the earlier "Darktide ships
  DXBC" reading and the `vs_5_0` profiles built on it are retracted. [UI base
  programs 0-9 inspected; `dtmt build` compiles its own overrides at 6.0]
- **Engine defines**: the library sources branch on `RENDERER_D3D12` and
  `STAGE_VERTEX` / `STAGE_FRAGMENT`; without them they take the GLSL or stub
  branches (`#define CBUFFER_START` becomes a comment, `Sampler2D` becomes
  `sampler2D`). `job_source` prepends them. [common.shader_source guards;
  verified by compiling]
- **From source to containers**: `shader43 --compile <dir> <declaration>
  <libraries|dir>` enumerates the jobs, assembles each stage's source and
  compiles it with DXC (`filetype::shader_compile`, shared with `dtmt build`).
  A mod-authored blit block including the real `common#common` chunk produced
  a 2921-byte `vs_6_0` and a 2774-byte `ps_6_0` container, both `DXBC` with a
  `DXIL` chunk. The real `decal_base` declaration now fails only on the
  toolchain-generated graph macros (`GRAPH_VERTEX_INPUT`, `GRAPH_PIXEL_INPUT`,
  `GRAPH_MATERIAL_EXPORTS`, `GraphVertexParams`, `GraphVertexResults`).
- **`dtmt build` sources from the declaration**: a material with a sibling
  `<name>.shader_node` compiles its single job per stage against the
  `.shader_source` files under the mod root and feeds the engine data flow; the
  sibling HLSL layouts remain the fallback, and a declaration with more than one
  job is refused (one container per stage cannot stand in for several
  permutations yet). [verified on snoopy-mod: the `ui_default_base` title
  shader built from its declaration, vs 4533 / ps 6079 bytes, and generated the
  431056-byte section; the sibling `.vs.hlsl`/`.ps.hlsl` were renamed away for
  the test. The built material carries the new containers: every program's
  decoded length is 4533 (VS) or 6079 (PS), the frame keys check, and the
  section round trip is identical. The in-game title test has not been run on
  this build yet.] `shader43 --compile --against <material>` reports whether a
  compiled container's interface matches the material's program of its stage.
- **In game**: the declaration-built `ui_default_base` renders. The mod log
  shows the package loading, the material being set
  (`material set: background_image -> materials/mods/snoopymod/title_screen_background`)
  and the Lua-driven `mod_tint`; the title screen shows the tint cycling and the
  pixel-stage wave (observed 2026-09-27). The deployed data file is
  byte-identical to the build output (SHA256 `265C2062…`). Deployment is
  DTMM's, not the legacy `mods/` folder: sync `out/` into
  `%APPDATA%\dtmm\mods\snoopymod`, then `dtmm --reset` and `dtmm --deploy`.
  [one run]
- **In game, after the engine-data rename**: the same shader built from
  `ui_default_base.engine_data` and driving `dev_wireframe_color` renders too -
  the title background cycles blue -> teal -> green across 9 second samples
  (region averages `4,20,103` / `4,56,37` / `28,43,15`), so the removed rename
  is not needed to bind the variable. Note: `user_settings.config`'s
  `log_level` was `1` for these runs, which suppresses the `ModLoader` info
  lines; set it to `2` or higher to read them. [one run]
- **The mod carries no engine library sources** (2026-09-30): the whole
  `core/stingray_renderer/**` tree the migration extracted (66 decompiled
  library files, ~1.4 MB) is gone. `dtmt build` reads only the mod's own
  `.shader_source` and the four node definitions its material names
  (`core/shader_nodes/{texture_coordinate0,sample_texture,material_variable,mul}`);
  every built asset is byte-identical without the tree, which is the target
  shape in miniature: declaration + source + material + node definitions. In
  game after the cleanup: `[ModLoader] Loading package "packages/mods/snoopymod"`,
  `[snoopymod] init.lua loaded`, `[snoopymod] material set: background_image ->
  materials/mods/snoopymod/title_screen_background`,
  `[snoopymod] driving 'dev_wireframe_color' from Lua material_values`; the
  three title samples all read `40,47,43`, and the template pumpkin unit still
  renders (user-observed). The tree is backed up in the scratch directory
  (`snoopy-core-backup`); the template cube/pumpkin assets stay in the mod.
  [one run]
- **The engine data text form is the Stingray dialect** (2026-09-30): `to_text`
  writes `key = value`, `{}` tables, `[]` arrays and quoted hex blobs, the same
  dialect as `.shader_node`/`.shader_source`/`.material`, and `from_text` reads
  it back with `serde_sjson`; `looks_like_text` tells it from a material data
  file. Snoopy-mod's file was converted with
  `generate_shader --engine-data --no-containers` and **every built asset is
  byte-identical**. The dialect's tall layout (a table field or array element
  per line) costs bytes - 15605 to 24189 on the UI base's file - which the
  derivations take back. [one conversion, one build]
- **The build is reproducible** (2026-09-30): two consecutive `dtmt build` runs
  are byte-identical for every file under `out/data` (MD5 per file). Only the
  listing files (`files.sjson`, the bundle manifest) reorder between runs. An
  earlier "changing word" in the built title instance was its reflection
  record's variable name hash, and the compared builds had different sources
  (`mod_tint` before the rename, `dev_wireframe_color` after); murmur32 of each
  matches the two observed words. One gap: `engine_data_check` can no longer
  rebuild the mod's engine data as-is, because the mod compiles its containers -
  the check needs them passed, or a built material to compare against.
- **In-process DXC**: `lib/dxc` is a workspace crate (like `oodle`) that loads
  `dxcompiler.dll` at runtime (`libloading`, no import library) and compiles
  through a hand-written `extern "system"` vtable transcribed from the SDK's
  `dxcapi.h`. It also loads the **validator** `dxil.dll` beside it:
  `IDxcValidator` lives there, signs the DXIL and fills the container header's
  16-byte hash, and D3D12 refuses unsigned DXIL with `E_INVALIDARG` - found by
  an in-game crash (`shader '#ID[6e8c619d]'`) that an exe-validated rebuild of
  the same material fixed. There is no `dxc.exe` path any more. [verified:
  in-process containers are byte-identical to the exe-validated ones (header
  hash `6BEF2E28…` on the blit block), and snoopy-mod's material is
  byte-identical to the known-good build (`2639D7DA…`); no temporary files]
- **The declaration's `samplers` table is read** (`filetype::shader_node`): a
  code block's texture slots, nested by condition, with `source`
  (`material`/`resource_set`), `slot_name`, `type` and `sampler_state`. This is
  the table a material's `textures` keys bind; the declaration's `channels`
  table is the stage-exchange channels (`tsm0`, `texcoord`) instead, and
  `source = "resource_set"` is where the stream's recurring engine names come
  from. `shader43 --reconstruct` writes the section's texture slots there (the
  device stream's record names plus the group data's channel table), keeps them
  out of `inputs`, and quotes a hash-named code block key so the reconstructed
  declaration parses as SJSON. [verified: reconstructing `007bf44baec8ee9a`
  (the UI base family, whose channel table does not decode) and compiling it
  with `--compile --against` matches both interfaces exactly, where the old
  tree mismatched; the reader's `reads_a_code_blocks_samplers_table` test
  covers the nested table.]
- **The mod's engine data is the template form**: snoopy-mod's
  `ui_default_base.engine_data` was regenerated from a current UI base material
  (`e3370cb2107d8aca`, whose stream carries `texture_map` like the old source) and
  then trimmed to what is genuinely carried: **218950 -> 22178 bytes**. The
  template holds the group data's parts (the tables are written, the group
  headers/descriptors/packed runs deduplicated), the dependency is written from
  the engine constant, the captured containers are dropped (the mod compiles its
  own), and a tail's block is written as a diff against the preamble's body.
  `dtmt build` produces a byte-identical material data file and the deployed
  title material renders in game. The pre-update file is kept beside the miner
  dumps as `ui_default_base.engine_data.old`; the September update grew the
  engine table by four records, which is why no current section matches the old
  62884-byte group data.
- **Bundle -> source, one step**: `shader43 --reconstruct <dir> <material>` now
  writes the source tree `dtmt build` needs - `<name>.shader_node` (with a code
  block and a pass so it builds), `<name>.shader_source` (the first program of
  each stage decompiled with dxil-spirv/spirv-cross, merged under
  `STAGE_VERTEX`/`STAGE_FRAGMENT` guards), `<name>.engine_data`, and (for a full
  material data file; a sliced section has no material template) `<name>.material`
  with the section fields stripped and `shader_engine_data` pointing at the
  engine data. The old `.preamble.bin`/`.constants.txt` side files are gone.
  [verified on the UI base: `uib.shader_node` + `uib.shader_source` (4087 bytes)
  + `uib.engine_data` (162068 bytes); compiling the reconstruction with
  `--compile --against` reproduces both interfaces exactly ("interface matches
  program 0 Vertex" / "program 1 Pixel"). The decompiler tools are skipped with
  a note when they are not found.]
- **Source -> section from a reconstruction**: the four reconstructed files
  build in a scratch mod (`dtmt build`: vs 4533, ps 6115 bytes) into a section
  with the original's 2 contexts, 1436 condition bytes, 1 dependency and 62884
  group-data bytes, 368556 bytes of programs, and an identical round trip. So
  the loop shipped section -> source -> section closes end to end.
- **The engine data carries the compiled programs**: `container <n> <hex>`
  lines (deduplicated; the UI base's 96 programs are two containers) and
  `program <stage> #<tail> #<container>` references, written by `--reconstruct`
  and read back by `dtmt build`. A material with engine data but **no sibling
  shader sources** now *carries* the section: the engine data's own programs
  are framed verbatim, the conditions and group data are the file's. [verified:
  a carried `004F18EA` build reproduces all 14 shipped containers byte for
  byte (the Oodle frames and offsets are re-derived, so the section bytes are
  not identical); in game, a carried shader replaces the title background - the
  carried `38ECBAD1` and `2A04418E` both render the widget black, i.e. the
  material and its foreign programs are in effect. `lib/dxc`'s validator runs
  as usual for compiled sources; carried containers are the shipped, signed
  ones.]

## Open, in the order to attack

1. **Map the conditions payload's result indices.** The pattern is confirmed on
   all 35 UI-base records: every branch's result is `tests.len() - 1`, and
   `5007` is the fallback where a record has one. It is **not a group
   selector**: every query id appears exactly once in the group data, one group
   per query, on all seven sections [7/7]. The offline comparison the notes
   called for is done (`shader43 --conditions-map`): every UI-base group's
   material table is the same 7 records (`texture_map` x3, `view_proj`,
   `world_view_proj`, `world`, `dev_wireframe_color`), the descriptors read
   `Y` 0/5/10 in every group, the results are only 1..3 (fallback 7), and the
   section has 96 programs (48 pairs) - so the result is not a table length, a
   `Y` field or a program index, and since it equals `tests.len() - 1` it
   carries no information beyond the branch's shape.

   **The first in-game probe is a negative:** all 85 result/fallback words of
   the UI base's 35 records were rewritten to 64 in the engine data, the mod
   rebuilt and deployed, and the title screen renders identically to the
   baseline (same blue wavy title, tint still cycling, no crash, no new
   `[Shaders] could not find any pipeline` warnings). So on this path the
   engine does not index anything with the result. It also **cannot** be
   observed there: the UI base's 96 programs collapse to **two distinct DXIL
   payloads** (48 vertex + 48 pixel copies), on the shipped section and the
   rebuilt one alike, and the per-group tables are byte-identical, so all 36
   groups are functionally the same. A variant-rich shader is needed to see a
   selection at all: the six small families carry 9-17 distinct payloads across
   14-26 programs, but most of their queries select no conditions
   (`record@0xffffffff`) and their trees use a second opcode (`0x30xx`) the UI
   base does not. Next: craft the tree on a family whose variants differ (or
   author a declaration with two real code variants) and watch the log and the
   render.

   **The carry needed for that probe is in place** (see the verified list): the
   engine data carries the shipped programs, so a variant-rich family can be a
   mod material without recompiling it. The first attempt carried `38ECBAD1`
   (bounding-volume debug: 4 distinct programs) and `2A04418E` (noise/skydome:
   17 distinct across 26) onto the title widget: both replace the widget and
   render it black, so the material path works, but neither shader's *default*
   context draws anything a screenshot can distinguish (their bindings want
   per-instance or global values a UI widget does not set).

   **The probe's first positive result is on the cube unit** (3D, so the
   bindings are real): its material parents to `#3F08AC44`, one of the six
   variant-rich families, so a carried `3F08AC44` base material with the cube's
   `bca`/`nm`/`orm` channels puts the shipped variants on a visible object; with
   the shipped tree the cube renders in the Mourningstar hub (spawned with the
   mod's F6 handler in `StateGameplay`). Rewriting the tree's results crashes
   the engine with an access violation in the pipeline path (error context
   `shader #ID[7bc888ae]`, `resource_tag` the cube), in every variant of the
   patch: every result +1 (1/6/8), only the default context's record set to 5,
   only the shadow_caster record set to 5, or both set to 5. So the result is a
   **live selector used when the material is drawn**, `0` is the only valid
   value for this material in both contexts, and nothing validates it - the
   shipped fallbacks 5/7 are simply never taken in the shipped tree. Which index
   **The result's range is the branch's test count** (crafted-record probe, same
   cube): a crafted record with **two** hashes (the known-true `7F9E89FD`
   twice) and a branch testing both renders with result `0` *and* with result
   `1`, no crash; the shipped **one**-hash record crashes with result `1`. So
   the engine indexes with the result into the branch's tests - the compiler
   always emits `tests.len() - 1` (the last, most specific condition) - and
   picks the condition hash at that index.

   **The condition vocabulary is the engine's queries and context names**, mined
   by hashing every printable string of `Darktide.exe` (`7F9E89FD` =
   `num_skin_weights`, `9FCFE126` = `gui`, `E2C8865F` = `gui_render_pass`,
   `3100C3D2` = `shadow_caster`, `F2760503` = `default`; a throwaway scanner,
   ~511k strings, 5 hits). The remaining condition hashes (`BDF72706`,
   `B5F45768`, `8FB860CF`, `625D415E`, `BC4EE226`, `red`, `green`, `blue`,
   `alpha`) are not in the exe or the dictionary.

   **The final probe: the picked condition does not change the render.** A
   crafted default-context record with hashes [`num_skin_weights`, `default`]
   and result `0` (picks `num_skin_weights`) vs result `1` (picks `default`)
   renders the cube identically. The reason is in the data: `3F08AC44`'s
   default context has **two groups with byte-identical tables**
   (`78E25D0F@0+4 9A531871@4+4`), so every interface in that context binds the
   same thing - the visible differences live across *contexts* (the
   `5852A5B1` pass carries the `bca`/`nm`/`orm` interface), not within one.
   So for the shipped materials the result's selection is not observable in the
   render, and the tree's job is the interface bookkeeping the plan's notes
   describe, not a visible variant switch.
2. **The group walk and per-group tables are implemented; the byte-packed
   header's grammar is the remaining decode.** `GroupData::group_starts` walks
   the groups by the contexts' query ids, and `group_bounds` / `object_tables`
   find each group's material table inside its own bounds, so a rebuilder can
   rewrite every group rather than only the first. The constructor still has to
   write the byte-packed header: its bytes are dumped (groups 0-11: 74, groups
   12-35: 57) but its length is not self-delimiting at the end. The opening
   words are `E503152C 00000008 00000000 00000010 00000001 B5639618 00000000
   <n>` with `<n>` 2 then 1; groups 0-11 then carry a condition hash plus twelve
   zero bytes, and both kinds end in a common 25-byte zero-terminated tail.
   Next: the `<n>` word or the opening record grammar as the length determinant.
   Measured on all 37 group boundaries of the UI base: the header length is
   **57 + 17 x (n - 1)**, with `n` the word at header+28 (2 where the group
   carries a condition hash, 1 where it does not). Confirmed on the six small
   sections too: `004F18EA` 91/91/57 (n 3,3,1), `17A3DC01` 125/57/57 (5,1,1),
   `2A04418E` 125,125,57,57,57, `38ECBAD1` 74 (n 2), `3F08AC44`
   125,125,57,57,57, `427B5E6E` 125,57,57. So the header is self-delimiting
   once `n` is read, across four values of `n` and seven sections. Caveat: the
   check reads `n` at the candidate offset, so it is self-consistency; what `n`
   counts is still open (it is not simply the condition count). Then the
   constructor can use the formula or carry the header from the template.

   **Decoded on all 36 UI-base groups:** the header is the **last** `40 + 17 x
   n` bytes of the group (74 at `n` 2, 57 at `n` 1), ending exactly where the
   next group's hash word starts. It is a 28-byte packed record (the third of
   the packed table's three, `E503152C 00000008 00000000 00000010 00000001
   B5639618 00000000`), the `n` word, `n` **17-byte condition entries** (`u32
   condition hash` plus thirteen zero bytes), and an 8-byte `01 00 00 00 00 00
   00 00` trailer. So `n` is the number of condition entries - the old
   "condition hash plus twelve zero bytes" is one entry, not a special case.
   The entries, measured on all 36 groups: `9FCFE126` (`gui`) for groups 0-5
   and 24-29, `BC4EE226` for 6-11 and 18-23, `625D415E` for 12-17, and zero for
   the second context's groups 30-35; groups 0-11 carry two entries
   (`gui`+`625D415E` or `BC4EE226`+`625D415E`). A group also starts with its own
   hash: the group data is `{u32 count}` then the groups, and the first query's
   id is the global hash at +4 (group 0's id). What the entries select is still
   open; they are the group's condition set, and the group's tree record tests
   them among others.
3. **Group data constructor**: with the walk, a first constructor can carry each
   group's byte-packed header and descriptors from the template - they are the
   family's compiled interface metadata, like the block - generate the material
   and channel tables, and keep the engine table. Generating the header itself
   needs the grammar in (2). `rebuild`/`rebuild_channels` already write the
   tables correctly.

   **The condition header is now read and written** (`GroupData::condition_headers`
   and `rebuild_conditions`, plus `shader43 --group-conditions`): a group's last
   `40 + 17 x n` bytes, found by self-consistency - the `n` word at header + 28
   reads back as the candidate, and the header's 28-byte record is the last
   packed copy, so its first word matches the copy before it. Verified: the UI
   base's 36 groups all read with the notes' values (n 2 for groups 0-11 and 1
   for the rest, the entries `gui`/`BC4EE226`/`625D415E`/zero, the trailer
   `01 00 00 00 00 00 00 00` where the group carries an entry), and five of the
   six small families read every group (`004F18EA` 3,3,1; `17A3DC01` 5,1,1;
   `38ECBAD1` 2; `3F08AC44` 5,5,1,1,1; `427B5E6E` 5,1,1) - and every rebuild is
   byte-identical on all seven. `2A04418E`'s two shadow-context groups do not
   satisfy the rule and are refused rather than guessed. So the constructor can
   now *write* the header (and grow or shrink a group's entries) instead of
   carrying it, with the rest of the group data left alone.
4. **In-process DXC (`dxcompiler.dll` + `dxil.dll`)** - **done** (see the
   verified list). The validator library is not optional: it is what signs the
   DXIL, and D3D12 refuses an unsigned container. Next, if wanted: ship both
   libraries beside the tool so no SDK install is needed.
5. **Graph code generation** - **done** for the scaffolding and the evaluation
   (see the verified list). The Stingray toolchain expands the declaration's
   channels and graph nodes into `GRAPH_VERTEX_INPUT`, `GRAPH_PIXEL_INPUT`,
   `GRAPH_MATERIAL_EXPORTS`, `GraphVertexParams`, `GraphVertexResults` and the
   `GRAPH_EVALUATE_*` bodies; without them every output-node block fails to
   compile. What is left:
   - A mod-authored graph in game: `dtmt build` compiles a material whose SJSON
     carries a `shader` block - the declaration is the graph's output node and
     the definitions are read from the mod root at the path the graph writes -
     but no mod ships one yet, so the generated evaluation has not been drawn.
   - Type coercion where a node's `auto` inputs disagree, an `if` mixing
     `float3` and `float4`; what the toolchain does there is unmeasured.
   - The spurious permutations of undecidable branches (`render_setting(...)`),
     which make a declaration's own `#error` fire on jobs no section ships.

   **The scaffolding is generated** (`ShaderNode::graph_scaffold`, emitted by
   `job_source` when a block or a library chunk mentions a graph name): the
   `GraphVertexParams`/`GraphPixelParams` structs from the declaration's
   channels (a vertex-domain channel is a param and an interpolator, a
   pixel-only one a pixel param), the `Graph*Results` structs per domain, the
   `GRAPH_*_INPUT` field lists (interpolator indices de-duplicated against the
   declared semantics) and the param/data/write macros. The graph's own
   evaluation comes from the material, below: `GRAPH_EVALUATE_*` calls a
   generated function per stage. [verified by compiling the Vermintide 2 SDK's output
   with `shader43 --compile`: 1438 programs across seven declarations -
   `anisotropic_base` 696, `billboard_base` 516, `particle_gbuffer_base` 144,
   `unlit_base` 18, `terrain_base` 6, `skydome_base` 4, `decal_base` 2, plus 52
   programs of `standard_base_bitsquid`. The rest fail on the material graph's
   locals (`shadow`, `wire_aa_fade`), on the older Stingray HLSL in that folder
   (assigning to a `const` parameter), or on a declaration's own
   `#error "Probes are not supported"`. A failed compile leaves its assembled
   source as `<name>.failed.hlsl` beside the output.]

   **The material graph is read and resolved** (`filetype::shader_graph`, plus
   `shader43 --graph --core <folder>`): a material's `shader = { nodes,
   connections }` block, the node definitions under `shader_nodes/` (inputs by
   connector uuid with their names/types/domains, the output type, the option
   uuids, the `code`, the `exports`), and the wiring between them. One of the
   material's nodes *is* the output node - the shader declaration - so the
   connections into its connectors are the graph's outputs, and its connector
   uuids are the declaration's `inputs` uuids. The resolution reports, per node,
   what feeds each input (another node, an instance value, a sampler, or
   nothing), and the graph's outputs by shader-input name and stage. [verified
   on the Vermintide 2 SDK's `standard.material`: 28 nodes, 32 connections, the
   six outputs resolving to `base_color`, `metallic`, `normal`, `emissive`,
   `roughness` and `ambient_occlusion`, the switches' options reading back as
   `OP_EQUAL`, and the connector/option uuids matching across the material's
   lower-case and the definitions' upper-case spellings.]

   **The evaluation is generated** (`Resolution::evaluate`, verified): each
   stage's code is the node code with its inputs bound, `RESULT(x)` writing the
   node's variable, `<input>_type` replaced by the resolved type, the instance's
   options as `#define`s around the node, the definition's export names replaced
   by the instance's, and imports read from the channel they name
   (`output_channel`, or a mesh input's semantic, which the scaffold adds as a
   channel). Types are inferred from the definitions: a declared name, `typeof`
   within the node, `largestof`/`smallestof` among inputs, `auto` from the
   source, the widest input as the fallback. The scaffold splits the results
   structs by domain, emits the graph's `defines`, puts the graph's *exports*
   into `GRAPH_MATERIAL_EXPORTS` (a graph material's variables are its exports;
   the declaration's own inputs are the results or the libraries' engine
   globals, so they are not repeated), declares the samplers, and emits the
   evaluation as a function per stage that `GRAPH_EVALUATE_*` calls - the node
   code carries preprocessor branches (`#if defined(OP_EQUAL)`) a macro body
   could not. `shader43 --compile <dir> --core <core> <material> <libraries>`
   compiles a material's graph. [SDK materials: `chroma_cube` 18/18 programs
   compiled; `no_uvs` and `transparent` 54 compiled and 12 failed, all on the
   declaration's own `#error "LOW_RES_ENABLED and MOTION_BLUR should not be
   active simultaneously"` - a spurious permutation of an undecidable
   `render_setting(...)` branch, which no shipped section has - so the failures
   are not the generated code. `standard.material` fails only on its own `if`
   nodes mixing `float3` and `float4`, which the node's own comment leaves to
   the user.]

   Reading the graph also found a real bug in the vendored `serde_sjson`: its
   integer alternative matched the leading digits of a float, so the SDK's
   `value = [0.0 0.0 0.0]` parsed as six numbers. The integer parser now refuses
   a `.`/`e` continuation.
6. **Wire `Section::build` into `dtmt build`** with generated group data and a
   real conditions tree; verify with the round trip and the substitutions before
   any deploy. The compile side is in place (`shader_compile::compile`), the
   declaration path feeds the engine data flow, and snoopy-mod is migrated to it;
   what remains is programs -> device data without the engine data, and the mapping
   of several jobs to a section's programs (needs the conditions decode).
7. **In-game test**: **passed** for the declaration path (see the verified list).
   The harness's `title-tint-demo.ps1` captures with `CopyFromScreen`, which
   grabbed the desktop rather than the game window on the run that verified the
   tint; `shot-window.ps1`'s `PrintWindow` on the Darktide window is the fix for
   an automated sample. The next in-game experiments are the conditions payload
   (crafted tree) and the fully generated section.

## In-game harness

`docs/scripts/` holds the scripts used to test in game. `title-tint-demo.ps1`
is the shape to copy: kill Darktide, launch `scripts/launch.bat` from the
install directory (the game ships no launcher), poll the newest console log for
the title material line (`material set: background_image`), screenshot and
average a region's colour a few times, and grep the log for the mod. `shot-window.ps1` uses `PrintWindow` so a borderless-fullscreen window is
captured rather than the desktop; `set-shader.ps1` and `slice-sections.ps1` move
a section in and out of a material. The deployable test mod is the snoopy-mod
checkout, deployed through DTMM, not the legacy `mods/` folder: build it
(`dtmt build`), sync `out/` into `%APPDATA%\dtmm\mods\snoopymod`, then
`dtmm --reset` and `dtmm --deploy`. `title-tint-demo.ps1` samples with
`CopyFromScreen`, which can grab the desktop instead of the game window; use
`shot-window.ps1`'s `PrintWindow` when the sample must be the game. The payload
experiment that needs this: a generated shader whose groups render
distinguishable colours and whose crafted conditions tree maps channel sets to
them, then drive the material from Lua and read which colour appears.

## Authoring format decision

Mods author shaders in the Stingray dialect only: a `.shader_node` declaration
and `.shader_source` libraries (`hlsl_shaders = { <name> = { code } }`),
**HLSL only** - the `glsl` parts are portability scaffolding for the renderer's
other backends and are ignored. The sibling `.vs.hlsl` / `.ps.hlsl` convention
is dropped once this path builds; it exists only in the engine data flow and
snoopymod today.

The path is now complete up to the compiler:

- `filetype::shader_source` reads the real library shape: chunk `code` (a
  string or the `shared`/`hlsl`/`glsl` table), chunk `includes` (names) and
  file `includes` (paths); `glsl` is kept but never selected.
- `code_blocks` reads the real block shape (`include`, `code`), and a pass
  links to its block by name.
- `ShaderNode::job_source` assembles each stage's source: the engine defines
  (`RENDERER_D3D12`, `STAGE_VERTEX`/`STAGE_FRAGMENT`), the job's macros
  filtered by their stage limits, then the block's includes (recursively, once
  each) and its body.
- `filetype::shader_compile` finds DXC and invokes it; `dtmt build` shares it.
- `shader43 --compile <dir> <declaration> <libraries|dir>` runs the lot and
  writes a container per job and stage; a mod-authored block compiles today.
- `dtmt build` prefers a sibling `<name>.shader_node` and compiles its single
  job per stage into the engine data's overrides; the sibling HLSL layout is the
  fallback. Snoopy-mod's `ui_default_base` is migrated to it.
- The captured engine-side data is called **engine data** now
  (`filetype::shader_engine_data`, `EngineData`, `<name>.engine_data`,
  `shader_engine_data = "..."`), not "preset"; `version 43` is the SDK's
  `shader::VERSION`, not a field. The old `variable`/`clone`/`channel`/
  `clone_channel` rewrite lines are gone: the shader's own variable names are
  what a material addresses (snoopy-mod drives `dev_wireframe_color`).
- The compiled section is generated **in memory**: `material::compile_with_shader`
  takes the bytes and `dtmt build` never stringifies the section into the
  material SJSON, nor writes it to a side file. The source tree stays text
  (`.shader_node` + `.shader_source` + `.engine_data` + the material SJSON) and
  the binary lives only in the built bundle. Verified: snoopy-mod's compiled
  material data is byte-identical, with no section file in the tree.

Next: the `dtmt build` wiring for a mod-authored graph (declaration + node
definitions + material), then an in-game test of a fully generated shader, and
the multi-job mapping once the conditions decode lands.

## Open decode details worth keeping

- Block synthesis is **open work, not a proven impossibility**: a generated
  minimal block fails at shader load, and the record stream's grammar is partly
  fitted (record kinds, component flags, the three varying header words). The
  block is carried from the library's own section until that grammar is
  finished.
- UI base channels: the stride from the material table lands on the engine table
  and is refused; the declared channel table at `+1620` (`{1776, 0, 3}`, records
  at `+1632`) is not reached. Refusing beats reading 69 engine variables as
  channels, which is what the old code did.
- `38ECBAD1` has a channel table at `+1528` (20 records, first kind 5
  `20BCBF88`); the earlier "no channel table" claim is retracted. Its layout is
  not decoded.
- Packed run framing: 6 of 7 copies found on `427B5E6E`; `is_packed`'s constants
  are fitted to the small sections.
- `Section::build` refuses a multi-group declaration against carried group data
  by the query/group count check. That is the honest gap, not a bug.

## Standing rules

- After claiming a format fact, deliberately look for the section that would
  break it. Write the sample size into the note. Five claims in the previous
  session failed this way.
- A read-N-write-N round trip proves the writer did not move bytes; it proves
  nothing about the reader. Substitution tests and count/bounds invariants are
  the evidence.
- The shader docs in this repository are the reference; anything that
  contradicts them is superseded.
