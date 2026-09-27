# Shader progress: where this stands and what to do next

This is the pick-up point. Read this before the other shader notes; it names
what is verified, with sample sizes, and what is still a reading.

Two goals, in order: **build a shader from source** (declaration + HLSL ->
section) and **bundle -> source -> bundle** (extract, decompile to editable
source, build it back). Both directions are goals. Byte-for-byte round trips are
the codec's correctness oracle, not the goal: a reconstruction is useful even
where it cannot reproduce bytes, and a build is useful even where some engine
constants are carried.

## Repo state (2026-09-27)

`main` is the trusted baseline plus reviewed commits:

```
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
- Expected: 129 pass, 3 pre-existing `filetype::package` failures.
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
  `{query_id, 0x130, 4, c_per_object, 0, 0, 0, descriptor[3] {name_hash, flags,
  X, Y}, ...}`. UI base: `4 + 12 x 1758 + 24 x 1741 = 62884`, the whole region.
  `GroupData::descriptors` reads `+32`; the old `+8` reading is corrected.
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
   carries no information beyond the branch's shape. The next step is the
   in-game crafted tree: a record whose branch tests two distinguishable
   conditions, to see which one drives the outcome and what the caller does
   with the value.
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
3. **Group data constructor**: with the walk, a first constructor can carry each
   group's byte-packed header and descriptors from the template - they are the
   family's compiled interface metadata, like the block - generate the material
   and channel tables, and keep the engine table. Generating the header itself
   needs the grammar in (2). `rebuild`/`rebuild_channels` already write the
   tables correctly.
4. **In-process DXC (`dxcompiler.dll` + `dxil.dll`)** - **done** (see the
   verified list). The validator library is not optional: it is what signs the
   DXIL, and D3D12 refuses an unsigned container. Next, if wanted: ship both
   libraries beside the tool so no SDK install is needed.
5. **Graph code generation** for the real game declarations. The Stingray
   toolchain expands the declaration's channels and graph nodes into
   `GRAPH_VERTEX_INPUT`, `GRAPH_PIXEL_INPUT`, `GRAPH_MATERIAL_EXPORTS`,
   `GraphVertexParams`, `GraphVertexResults`; without them every output-node
   block fails to compile (only these names are left in `decal_base`). The
   channel table is read, so this is code generation from data that is already
   parsed, not a decode. Mod-authored blocks that avoid the graph macros compile
   today.
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

Next: graph code generation for the real output-node declarations (see open
item 4), the in-game title test of the migrated mod, and then the multi-job
mapping once the conditions decode lands.

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
