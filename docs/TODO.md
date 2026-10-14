# TODO

The project-wide worklist: what every Stingray file type still needs, what the
current focus is, and where community work has already solved something.

Two focused lists sit under it:

- [`File-Type-Status.md`](File-Type-Status) - the per-type decode / compile /
  decompile status table.
- [`Shader RE TODO.md`](Shader+RE+TODO) - the material-shader (`shader43`)
  workstream, which is the current focus.

This page is the umbrella. Update it when a file type moves, or when a
community project is found that already knows part of a format.

**Legend** (the same as `File-Type-Status.md`):
*Complete* - the format is fully implemented/documented; *Partial* - enough is
known for usable results, gaps remain; *Limited* - some work begun; *None* -
nothing known.

## The shader goal (top priority)

**Every shader should compile from its own `.material` + `.shader_node` +
`.shader_source`, with no carried `shader_engine_data` side file.** Per family,
as of 2026-10-12:

- **`ui_default_base` (GUI / title): done.** No engine data - it builds from the
  declaration alone (section `CD9854D69AC9A5B7`), verified in game.
- **`standard_base`: the `mid` derives and the blocks share their body and tail.**
  `standard_base.engine_data` is now **9,578 B** - down from 375,410 when the
  section was captured, and from 42,091 - a **77% cut**. The programs' cbuffer and
  resource lists derive; the group **`mid`** does too (the file carries each
  group's **channel list**); a program's **group** is derived from the group
  tails' condition counts; the group **heads** derive from the groups' programs'
  reflections; a group's **`between`** derives from the engine table's size; the
  device **blocks** reference the slice of the preamble body they
  carry verbatim and pool their **body slice**, head and trailing bytes (shared
  fronts and backs); zero-valued indices are omitted; and the engine table is
  carried as the count of records it drops from the toolchain's captured table
  (4), not the whole 2.7 KB. Every step is confirmed by the built section
  matching the shipped one (with two pre-existing material-size bytes still
  open) and rendering in game. Still carried: the group template's materials, the
  **contexts/conditions/preamble**, the per-program **block head and the distinct
  trailings** and the **program list** (`stage`/`tail`/`job`). The remaining
  decode is the per-program block grammar, plus the enumerator and the material
  table's derivation.
- **Particle shaders: not started.** The `particles` file type decodes the
  particle **effect resource** (`filetype/particles.rs`), *not* a shader; no
  particle `.shader_node`/`.shader_source` work exists.

Also outstanding for **any** custom shader: our own **interface** (our own
cbuffers, resources and channels) is still blocked on the device block's record
grammar - a custom base material can only use a shipped interface today.

## Where things stand

- **Compile + decompile work today:** Bundle, Bundle Database, Package, Lua,
  Strings, Texture (partial), Material (partial), Unit (partial).
- **Active workstream (top priority):** the material **shader** - deleting
  `.engine_data` and deriving the `shader43` section from `.material` +
  `.shader_node` + `.shader_source`. See "The shader goal" above and
  `Shader RE TODO.md`.
- **Formats with real work left:** the shader section, Particles, Level, Sound,
  and the resource side a Unit pulls in (Bones, Animation, State Machine, Flow,
  Physics).
- Anything not listed is unknown.

## File types

| File type | In dtmt | Next step |
|-----------|---------|-----------|
| Bundle | Partial | The header's "unknown" words are pinned: `version = 0xF0000008`, `unknown_1 = 3`, `unknown_3 = 0` in all 3,000 sampled bundles, and **`FileMeta.flags` is the external-data-file bit** (`flags & 4` = the data lives in a `data/` file - 67,165 entries take only `0`/`4`, so it is *not* a deleted-file bitflag). A data-file name without the flag is a stream reference. The last header field, the per-variant byte `FileHeader.unknown_2`, is **`1` in all 176,465 variants** of a 3,000-bundle sample, and the writer now emits the variant's own value (a fresh variant starts at `1`) instead of a hard `1` - byte-neutral on a rebuilt mod (all 8 data files hash-identical). Nothing is left unexplained in the header *except the `PropertyArray`*: upstream issue #3 gives it completely - the 256 bytes near the start of a bundle are **32 `IdString64`s**, the properties file variants are named by, shared across all bundles (the engine copies the database's shared array into each, zero-filling past its length). A file's `variant` is an **index into that array**, and the entry is the variant's hashed name, so a localised file's name can be resolved instead of shown as `#ID[...]` - which is what the Strings row means by "the bundle-property / variant interaction". Implementing it is: read and keep the 32 ids (the reader already *reads* all 32 into a local and drops it, and `BundleFileVariant.property` already holds the variant and is written back), map a file's variant through them when naming it, and add a property hash group to the dictionary (the issue's own suggestion, since the names are generic and need not be languages). **Measured on shipped bundles (2026-10-14):** the array sits at offset 20 (`format`, 3 skipped words, `num_entries`), and four different bundles - three hash-named plus `packages/boot` - carry the *same* **12 non-zero of 32**, which is the "shared across all bundles" claim exactly; three resolve in the existing dictionary as **`ja`**, **`es`** and **`dev`** (so a new group is a labelling nicety, not needed for resolution)  **But the variant index is not where DTMT keeps it.** `bundle_dump` now prints each variant's `property` and the name it resolves to, and across the boot bundle's 3,707 files plus 40 random bundles **every one is `property = 0`** - so `BundleFileVariant.property` is not the issue's "variant bit field" (or Darktide's bundles simply do not use variants, which an issue written in 2023 against VT2 would not have known). The index is most likely the file header's u32 that `file.rs` reads with `r.skip_u32(0)` and `to_binary` writes as a hardcoded `0x0` under a literal `// TODO: Figure out what this is` - the same read-then-drop shape as the per-variant byte.  **Measured: Darktide does not use variants at all.** With `bundle_dump` printing both, the file-header word is `0` in **391 of 391** files across 25 random bundles (the field is now *kept* rather than dropped and written as `0`, which is a round-trip fix regardless), and **no file in 40 random bundles has more than one variant** - which also explains why every variant's `property` read as 0 earlier: they are all *single*-variant files. So the `PropertyArray` is present and populated (the shared engine data still carries `ja`, `es`, `dev`) but nothing in a Darktide bundle indexes it, and wiring the naming to it would be dead code. Darktide localises by **package** instead - the `packages/ui/fonts/slug_ja`/`_ko`/`_zh_tw`/`_zh_cn` bundles are the pattern - so issue #3, written in 2023 against Vermintide 2, describes a mechanism this game does not exercise. The remaining value is the round-trip fix above. See [`File Type - Bundle.-.md`](File+Type+-+Bundle.-). |
| Bundle Database | Partial | See [`File Type - Bundle Database.-.md`](File+Type+-+Bundle+Database.-). The reader parses every section the community template names, and the data is the limit: the file index (`contents`) covers **11,017 of the 16,102 bundles** (957,805 file entries, measured with `examples/db_probe.rs`), so `find-file`/`list-files` are conclusive only for those. The ~5,085 bundles without a list include `packages/ui/fonts/slug_ja`, which is why its CJK fonts look absent while the Latin ones (in the listed `packages/boot`) resolve. Both commands now say which of the two cases they hit. |
| Package | Complete | Nothing. The community writer's `package_v43` corroborates the layout. |
| Lua | Complete | Nothing. |
| Texture | Partial | Derive/compile + in-game load work. Side-file **names are arbitrary** (the game's 186,955 shipped `.stream` files are `data/<name[..2]>/<16 hex>.stream`, a counter+hash — only the recorded path matters), and the `.stream` **slot length** is now exact (`31` for a `.stream` name, `30` for `data/ab/<16 hex>`; verified across ~2,400 shipped entries). The community profiles pin the body flags (`0x101` sRGB / `0x001` linear = `SRGB\|STREAMABLE`), and the **category** is `murmur32("content/texture_categories/<leaf>")` (verified against 10,897 shipped bodies; DTMT now derives and resolves those). Open: ~20 category names — including the three most common after `ui` — are not in the dictionary and cannot be enumerated from the game's files. See [`Reverse Engineering - Textures.-.md`](Reverse+Engineering+-+Textures.-). |
| Material | Partial | Base + instance round-trip byte-for-byte. Custom shaders work **against a shipped interface**; adding channels / our own cbuffers is blocked on the device block's record grammar. The community `material_v61` writer is a v61 reference. |
| Shader section (`shader43`) | Partial (generating) | The `standard_base` family derives from source: its engine data is **9,556 B** (was 375,410), the resource lists, `mid`, groups, group heads, `between`, device-block body slices and heads/trailings all derive or pool, and the built material matches the shipped one but for two pre-existing material-size bytes (verified in game). Remaining: the per-program device **block grammar** (the head and trailing, 49% of the file), the carried **preamble**/`programs`/`materials`, and the enumerator. See `Shader RE TODO.md`. The community `shader_section` (version `0x2b`) decodes/encodes shipped sections exactly, and its `shader_compiler` gives the root-parameter numbering. A decompiler symbol map of `Darktide.exe` names the engine/D3D12 symbols; the device pass reader `stingray::D3D12ShaderPass::init` is at `0x1407f5f4x`–`0x1407f64xx` (found from its assert string; the map's own address for it is mis-attributed to ray tracing). |
| Strings | Complete | The format is done, and the bundle-property / variant interaction it pointed at (upstream issue #3) is **answered**: Darktide's bundles carry no variant index at all - every file has a single variant, its `property` and the file-header word are `0` in every sample, so nothing indexes the `PropertyArray`, and localisation is by **package** instead (`packages/ui/fonts/slug_ja`/`_ko`/`_zh_tw`/`_zh_cn`). See the Bundle row for the measurements. |
| Unit | Partial | Static payloads spawn, and skins / animations / streamed meshes / actor-camera-light units are rejected with clear errors. A shipped sweep (`examples/unit_probe.rs`; 2,666 units in 400 bundles) measures the gap: **168 decode** (2,498 rejected) and **168 round-trip byte-for-byte - every decodable shipped unit** (0 before this week's fixes). The 2,498 rejections were **four walkable sections plus one unbeatable class**, and the walkable ones are now carried verbatim: skins (**116**, `skins_raw`), the simple-animation blob and its groups (**1,464**, `simple_animation` + `animation_groups_raw`), actors (**833**, `actors_raw`), cameras and lights (`cameras_raw`) and the unknown device blob (**7**, `device_blob`) - `bitsquid-blender-tools`' `stingray/unit/dt.py` gives every shape, and each section is either length-prefixed or fixed-size per record, so it can be walked and carried. That took the sample from **168 to 508 decoded**; the 17 that then did not round-trip were **not** carry bugs but two gaps in the vendored SJSON reader, both fixed: a plain `u64` above `i64::MAX` (any 64-bit hash with the top bit set) and the word `inf`. The class that stays is **streamed geometry** and it is not a parsing gap. A fresh 120-bundle sweep on the tree with the actor and camera carries reads **130 decoded / 130 byte-identical / 0 rebuilt-but-different / 0 recompile failures**, and its rejection list is *only* streamed geometry (273) plus one `(9, 8)` channel unit - the actor and camera sections no longer appear as rejections at all. The 76 streamed geometries are **not** a parsing gap - their vertex bytes are in no bundle at all (across 3,000 bundles and 19,133 unit entries *no* unit variant is external or names a data file: `examples/unit_stream_probe.rs`), so the guard is deliberate rather than a TODO. The dominant loss was the **bounding volumes**: they are baked at cook time, so recomputing them from the stored half-float vertices always drifted. The geometry's `bounds` are now carried (optional, decompiled-only) and reused by the geometry, the mesh objects and the LOD union. The scene graph carries the payload's own `index`/`rotation`/`scale`/`world`, and the `.unit` maps are `IndexMap`. The last class was the LOD-object section: its records carry an `unk2` hash, `unk4`/`unk5` and a stream mesh order that the compiler did not model, costing exactly four bytes per unit. Carrying it verbatim closed the sweep. The half-float stream path is bypassed (a decompiled geometry carries its record verbatim) and the mesh-object render flags are carried too. The community `unit_v115` writer and `bitsquid-blender-tools` (`stingray/unit/`, `vertex_stream.py`) are the references. |
| Bones | Complete | The cooked **body** is decoded and compiled (`filetype/bones.rs`): a bundle variant stores the body alone — the community writer's 38-byte envelope is its *standalone* blob form, which the reader still accepts. **Verified against shipped data**: all 723 `bones` variants in 2,500 sampled game bundles decode, pass every joint-id check and round-trip byte-for-byte (`examples/bones_probe.rs`). |
| Animation | None | **Two types, both opaque**: `animation` (`0x931e336d7646cc26`) and **`animation_curves`** (`0xdcfb9e18fff13984`), which DTMT recognises and copies but does not read. Clips ship in **dedicated packages** - `bundle/9f535ee1e90a6439` holds ~40 named clips (`content/characters/player/human/first_person/animations/grenade_tox_equipable/equip.animation`, 24,292 B) *plus* ~56 hash-named ones of the same sizes, so the same clips exist under both the package path and a cooked hash name, and DTMT already reports the named ones. A clip opens with two zero words, counts (146, 7, ...) and floats - the container is visible in a hexdump and not yet derived. The community reference is substantial and includes an *evaluator*, not just a writer (`animation_writer.cpp` 26.8 KB, `animation_fitter.cpp` 21.4 KB, `animation_evaluator.{h,cpp}`), so it can generate cooked clips from glTF. **The container's opening is derived (2026-10-14), with two invariants that hold exactly.** Four named first-person clips (`to_cover` 31,028 B, `equip` 18,038, `dodge_bwd` 14,408, `dodge_left` 14,796): `u32 0`, `u32 0`, `u32 146` (constant across all four), then a **float that reads as a duration in seconds** - 0.69995117, 1.2998, 0.5, 0.5, which is exactly the right shape for a 0.7 s cover move, a 1.3 s equip and two 0.5 s dodges - then **`u32` equal to the file's own byte length** (31,028 / 18,038 / 14,408 / 14,796, matching on all four), `u32 0`, `u32 7` (a count), `u32 0`. So two fields are pinned by arithmetic or by meaning rather than by eye: a self-size that closes exactly, and a duration whose values are plausible for what each clip does. **Sampled across families (18 clips: locomotion, idle, hit-stun, fire, inspect) and one anomaly explained the rest.** `w08 = 146` on **all 18** and `w10 == size` on **all 18**, so the version and the self-size are constants of the format; the float at `0x0c` fits every clip by name (`hit_stun` 0.267 s, `hit_stun_medium` 0.633, `dodge` 0.5, `equip` 1.167, `idle_aim_underhand` 3.333, `idle` 5.833), which is 18-for-18. Then `grenade_tox_equipable/equip` (24,292 B) read `w18 = 1059201564` where the others read `7`, and the hexdump showed why: **`0x14` is a count**, `1` there instead of `0`, and one 8-byte entry (`f32` 0.6329 plus a word) follows before the `7`. So the opening is `u32 0, u32 0, u32 146 (version), f32 duration, u32 size, u32 N, N x 8-byte entries, u32 tracks, u32 0` - four fields pinned by arithmetic or by meaning rather than by eye, and the exception is what proves the count is real. **What follows the count is a float block, not a table of names.** From `0x18` the words are `7, 0, 0, 16320`, then runs of `16256` and `0` - i.e. `f32` **1.5** and **1.0** (`0x3FC00000` / `0x3F800000`) with zeros between, which reads as an identity-like matrix carrying a scale or translation rather than a list of records, and **not one word resolves in the dictionary**. So the tracks are almost certainly **index-based**, referring to a sibling `.bones` resource - which the same bundle ships (`content/characters/player/human/first_person/base.bones`, 3,162 B) - exactly as the unit work found animations pairing with bones. That gives the next check for free: `bones.rs` already decodes the body, so its **joint count** can bound the indices the clip uses, and a track index at or above it would be a mis-read. **Measured: `0x08` is the rig's joint count, not a version (corrected 2026-10-14).** `base.bones` has **146** joints, and the clip's `0x08` is **146** - the same number the four-clip and 18-clip samples all showed and which I read as a format version. It is not: it is the size of the rig the clip animates, constant across the samples *because all 18 are first-person human clips on the same `base.bones`*. That is a much better reading, and it is a warning about the evidence: "constant across samples" looked like a format constant, and only a *cross-resource* check (decoding the sibling bones) revealed it as a per-rig quantity.  `base.bones` decompiles through `filetype/bones.rs` to a readable **names list** - `root_point`, `ap_aim`, `ap_camera_node`, `ep_mainhand`, `j_spine2`, `j_leftshoulder`, `j_lefthandthumb1`, ... - so the rig the clip animates is *named*, which is what makes the tracks checkable at all. But the count is far above `7`, so the clip's `7` is not a joint count: it is most likely a **track or curve count** (seven animated things), and the joint indices, if the clip carries them, still need locating. **Tested both candidate layouts, and the test found the payload's shape instead (2026-10-14).** There is no plain index list: the `u16` scan is drowned in zero padding and no ascending byte run exists. But the scan exposed a **grid** - zero-filled runs of thirteen `u16` recur at exact multiples of **0x50 (80) bytes** (`0x5ba`, `0x60a`, `0x65a`, ..., `0x100a`, `0x105a`), and where the spacing is larger it is always a multiple (0x10aa -> 0x11ea is 320 = 4 x 80). So the curve payload is a sequence of records on an **80-byte grid**, some spanning several units, and `idle` is mostly zeros because most of the rig has no curve. Next: derive the 80-byte record (a curve header plus keyframes is the shape to test) - and the track indices remain unlocated. **The payload is half-float, not `f32` (2026-10-14).** Every 80-byte block of `idle` is non-zero, and reading its densest region as `u32`/`f32` yields garbage - 1e28, NaN, 7.5e31 - which is the signature of **16-bit halves read as 32-bit floats**. The same bytes as `u16` are `0x3FB6` = 1.858 and `0x8062` = -0.000000, i.e. plausible rotation components, the magnitude a joint curve should carry. It also fits the grid: 80 bytes is 40 halves, and the "thirteen `u16`" zero runs are 16-bit units rather than byte padding. `f32_from_f16` already exists in the unit reader, which reads half-float vertex streams the same way, so the curve payload should be read as halves too. Next: interpret the 80-byte record as halves and check the values against a rotation. |
| State Machine | None | The community `state_machine_writer` writes states, blends, layers and ragdoll states. |
| Flow | None | The community `flow_resource` decodes/encodes a unit's compiled flow graph. |
| Physics | None | The community compiler cooks PhysX collections/scenes. |
| Particles | Limited | The effect header, the effect variables and each system header (name, capacity, channel layout, component counts) are decoded and compiled (`filetype/particles.rs`); a shipped effect round-trips byte-for-byte. The component bodies are still opaque hex; the community `particles_resource` (version 102) decodes them. |
| Level | None | Nothing started. |
| Font (`.slug`) | Partial | **Custom fonts.** **Implemented (read-only)**: `filetype/slug.rs` parses a `slug` bundle entry - `guls`, version, kind, the four `{tag, base, count}` records, the `HLF4` kerning pairs and the `16U2` glyph records (box, words, two advances) - and `dtmt bundle extract --decompile` now reads a shipped font (`packages/ui/fonts/slug_*`; one entry resolves to `noto_sans_jp_black`)
**Caveat:** a *fresh* `slugfont.exe` compile of `consola.ttf` (2,173,136 bytes, also version `0x0760`) does **not** match the demo's shape - at `base + 64` its table holds **ascending `u32` pairs** (`737 738`, `753 754`, `769 770` …), a codepoint list rather than 64-byte glyph records. So the table order and strides vary between compiles, and the reader's default base is tuned to the demo and the shipped entry rather than being general; it should locate the glyph records structurally before being trusted on arbitrary fonts. **Confirmed on shipped data (2026-10-13):** Darktide's own Latin fonts hit this - the reader locates no table in `content/ui/fonts/arial` (992,496 B raw) and now reports `glyphs 0, glyph table None` for it, where it used to print the ascending-id run as codepoints `386, 387, 400, …`; those fonts remain a gap.
**The glyph table''s records are of two kinds, and the `u32` is a codepoint.** Dumping 20 records of a shipped entry shows them alternating: even indices carry a **full-height box** (`0, 0, w, 1`) and a *float* in the `u32` slot (`1063339950`/`1063339951` = `0.89` as `f32`), while odd indices carry a real glyph box and - in that same slot - a small **ascending** number: `140, 229, 278, 388, 670, 1040, 1291, 1315, 1340, 1365` … So the odd records are the **glyphs and their codepoints**, the even ones are markers (the tall box is the line height), and the discriminator is `word2 == 0` (markers) versus non-zero (glyphs). That halves the count: `16U2`''s 425 records are about 212 glyphs and 213 markers.. **Shipping a custom font needs no `slug` writer**: the user compiles the font with the Slug library's own tool (`slugfont.exe`, or its DLL), and DTMT **packs the resulting `.slug` verbatim** - `crates/dtmt/src/cmd/build.rs` has a byte pass-through beside the Unit/Bones branches, because every other type is read as SJSON and a binary font cannot be. Verified end-to-end: `content/ui/fonts/snoopytest.slug` (135,604 B) lands in the built mod bundle as `a27b4d04a9ba6f9e 18faafe3b4f9359c size=135604`, byte-identical and inline, exactly the shape of a shipped font. **The recipe, proven in game (2026-10-13):** the UI resolves fonts *by name* - `scripts/ui/boot_ui.lua` renders through the `slug_text` shader with `font_name = content/ui/fonts/mono_tide_regular`, and `scripts/managers/ui/ui_fonts` maps styles to names - so a mod needs no Lua at all to swap a font: **drop your `.slug` at `content/ui/fonts/<name>` and list it in the package** (`slug = [ "content/ui/fonts/mono_tide_regular" ]`). Built and deployed, the mod's bundle carries `a27b4d04a9ba6f9e 780d2c1d6198922c size=272064` - the exact hash the UI asks for - and the boot, title screen and gameplay all still pass (`PUMPKIN_OK` twice), with the UI's default font visibly coming from a different family. Still open: an *unlocated* table for Darktide's Latin fonts and the fresh `slugfont` output - the reader now reports `glyph_table` when it locates one (`92,288` for the CJK entries, with 218 glyphs + 221 markers = the `16U2` count of 439) and **no glyphs at all when it cannot**, rather than the fabricated ascending-id codepoints it used to print - plus the `COMP` regions, and registering a font under a *new* name (which needs the UI's own font table). See below for the format and the reference compiler. **A Darktide font is a plain `Slug` bundle entry (`A27B4D04A9BA6F9E`), not a material.** The earlier note here claimed the opposite ("no `slug` entry appears in ~1,700 sampled bundles, so the data lives in the material's payload") - that sample simply did not include the boot/UI bundles. Measured directly (`examples/bundle_dump.rs`, 2026-10-13): Darktide's Latin fonts are **14 `slug` entries in `packages/boot` (bundle `9ba626afa44a3aa3`)** - `content/ui/fonts/{arial, darktide_custom_regular, friz_quadrata, itc_novarese_bold, itc_novarese_medium, machine_medium, mono_tide_light, mono_tide_medium, mono_tide_regular, proxima_nova_bold, proxima_nova_light, proxima_nova_medium, rexlia}` plus `core/performance_hud/debug` (135 KB to 1.0 MB each) - and the CJK ones in `packages/ui/fonts/slug_{ja,ko,zh_cn,zh_tw}` (5.6 MB each). Every one of them is `guls`, version **`0x0700`** (the reference compilers emit `0x0760`). No material takes part: the VT2 SDK's `slug_text*.material` are generic text-rendering materials with an empty `variables` block, and the pairing lives in a **config** resource instead (`font = core/performance_hud/debug` beside `font_material = debug`), which is where the "fonts are materials" idea came from. The format is the commercial **Slug** library (<https://sluglibrary.com/>); its demo ships the compiler and reference files at `C:\Users\Dan\Downloads\SlugDemo\SlugDemo` - `slugfont.exe -o <out> <in.ttf>` compiles a font into a `.slug` (verified: `C:\Windows\Fonts\consola.ttf` yields a 2,173,136-byte file), so there is a reproducible ground truth to compare a shipped entry against, and a `.slug` opens with the magic `guls`, a version (`0x0760`) and a four-character kind (`fnt` for a font, `alb` for an album), with compiled examples in `Fonts/*.slug` and `Albums/*.slug`. **Found (2026-10-13):** Darktide's fonts are the packages `packages/ui/fonts/slug_ja`, `slug_zh_tw`, `slug_ko` and `slug_zh_cn` (from `dtmt bundle db list-bundles` over the game's `bundle/bundle_database.data`), and each is a bundle of **two `Slug` entries** - bundle file type `a27b4d04a9ba6f9e` (`Slug` in `bundle/filetype.rs`, ~5.6 MB each in `slug_ja`). So a Darktide font is the Slug library's compiled data stored as a `slug` bundle file, and DTMT has no `slug` filetype: `bundle extract` errors on it and there is no decompiler. Next step: implement the `slug` filetype. `examples/slug_probe.rs` already dumps a bundle's `Slug` entries, and the shipped bytes confirm the format: magic `guls`, version `0x0700` (the demo files are `0x0760`), kind `fnt`, then a **4CC chunk directory** - `{tag, u32 length, payload}` records with tags like `COMP` and `HLF4`. Read that directory and the metric/atlas tables (using `slugfont.exe` from `C:\Users\Dan\Downloads\SlugDemo\SlugDemo` and its `Fonts/*.slug` as the reference), then compile one back and drive a UI text element with the pumpkin harness.

The directory is a list of **12-byte records** (`{4CC tag, u32, u32}`) at offset 32, and a shipped entry and the demo's `consola.slug` line up entry for entry:

| # | tag | shipped (`slug_ja`) | demo (`consola.slug`) |
|---|-----|---------------------|-----------------------|
| 1 | `HLF4` | `0x1000`, `0x4B` | `0x1000`, `0x2` |
| 2 | `COMP` | `0x1DAF9C`, `0xB9D40` | `0xAF60`, `0xC000` |
| 3 | `16U2` | `0x1000`, `0x1A9` | `0x1000`, `0xC` |
| 4 | `COMP` | `0x2BF168`, `0x294D00` | `0x14750`, `0x16F80` |

The two `COMP` chunks are the bulk (~1.9 M + ~2.9 M of the 5.6 M file), so the font body is **compressed inside the `slug`** - Oodle, which DTMT already links - and the other two chunks are a fraction of that. `examples/slug_oodle.rs` walks the directory and tries Oodle on the `COMP` chunks.

Two details confirmed with it: the 4CC tags are stored **little-endian** (`COMP` reads back as `PMOC`), and the last record's `offset + size` equals the file size exactly (`2,879,848 + 2,706,688 = 5,586,536`), so `{tag, u32 offset, u32 size}` is right - and the directory is just those **four** records (`32..80`); the 12-byte stride past that is already the next structure, which is a table of **11 values** starting at `80`: `112, 256, 368, 880, 4,978, 92,016, 729,200, 749,232, 756,464, 759,152, 0`. The `HLF4`/`16U2` regions at offset `4,096` are almost entirely zero (a lone `0xCA` at `4,168`), so those two chunks look like small tables rather than data.

The `oodle::decompress` wrapper needs the **exact** raw size and that table's values are all below the 2.7 MB chunk, so the size is elsewhere. A first attempt with a generous 32 MB buffer **fails**, and **zlib, plain deflate and gzip fail too** - because the chunks are **not compressed at all**. Their contents are structured tables: the demo's first `PMOC` chunk holds clean `f32` values (`0x3F0CC000` = `0.549805`, `0x3F31E000` = `0.694824`), i.e. Slug metrics. So - as `slug_probe` now confirms by printing the tags in their readable order (`HLF4`, `COMP`, `16U2`) - the 4CC reading was right and **`COMP` is a table name, not "compressed"**; no decompression is needed, the tables read directly.

A three-way header comparison (`slugfont`'s fresh output, the demo file, and a shipped entry) settles several fields - and shows one important version difference:

| field | fresh `slugfont` run | demo `consola` | shipped `slug_ja` |
|-------|------|------|------|
| version | `0x0760` | `0x0760` | **`0x0700`** |
| `4FLH` | `(4096, 11)` | `(4096, 2)` | `(4096, 75)` |
| `PMOC` #1 | `(287944, 1180096)` | `(44896, 49152)` | `(1945500, 761152)` |
| `2U61` | `(4096, 111)` | `(4096, 12)` | `(4096, 425)` |
| `PMOC` #2 | `(705040, 1468096)` | `(83792, 94080)` | `(2879848, 2706688)` |
| at 80 | count 14: `112, 256, 432, 944, 2911, 25520` | 14: `112, 1, 432, 496, 357, 1520` | 11: `112, 256, 368, 880, 4978, 92016` |
| at 144 | `1068016, 1177392, 1179952` | `47216, 47280, 49008` | `749232, 756464, 759152` |

What that gives: the two tiny records' **second word is a count, not a size** (`4FLH` 2/11/75, `2U61` 12/111/425 - they scale with the font), the first word `4096` is a fixed payload base; the **last** record's two words are an offset and a size whose sum is the file size exactly in all three files; `112` is constant across all three (a structural value); and **Darktide ships Slug version `0x0700` while the reference compiler emits `0x0760`**, so a shipped entry may not decode with the current library's layout - the demo file's `0x0760` is the closer reference for *structure*, and the shipped file for *fields*.

Reading the region at that base (`4096`) shows the per-glyph tables directly:

- the **demo** file holds **16-byte records**, `{u32, f32, u32, f32}` - e.g. `00 00 00 00 | 00 C0 0C 3F | 00 00 00 00 | 00 A0 25 3F` = two metrics (`0.549805`, `0.647461`) per glyph - so `4FLH` is a two-metric-per-glyph table with stride 16;
- the **fresh and shipped** files instead open with a **4-byte ascending run** (`0x2CD, 0x2CE, 0x2CF …` = 717, 718, 719 … in the fresh; `0x20F, 0x210 …` = 527, 528 … in the shipped) - a **supported-codepoint list**, stride 4.
- The shipped file is all-zero at `4096` and starts its run at `8192`, so its tables sit at a different base within the same region.

Together with the field reading above, the record is `{tag, u32 base, u32 count}` with that tag's table laid out consecutively from the base - the two records sharing `4096` are one region, first table then second.

Measured run lengths: the **longest ascending `u32` run** in that region is **280 entries** in the fresh compile (first value `0x613` = 1,555) and **109** in the shipped entry (first `0x139` = 313).

**The two record counts are the table's entry counts and the tables are now read.** Walking the demo from `4096`:

- `4FLH` count **2** sits at `4096..4128` - two **16-byte** records `{u32, f32, u32, f32}` (the two metric pairs `0.549805`/`0.647461`), i.e. two **kerning/ligature pairs**;
- `2U61` count **12** follows at `4128` - twelve **64-byte glyph records**, each a glyph box (four `f32`: `0.0800781, -0.00878906, 0.479981, 0.646973`), then a `u32` and a packed pair (`0x001C000E` = 14 and 28), then two advances (`35.0085`, `42.6984`), and so on.

So `examples/slugfont`'s `consola.slug` is a **12-glyph subset build** (hence 177 KB), `2U61` is the **glyph count** and `4FLH` the **kerning count**, and a glyph is a 64-byte record - which is the shape to read in a shipped entry, where the counts are 75 and 425. `slug_probe` reads the demo's table (`base + 64`, 64-byte records); the shipped `0x0700` entry's glyph table instead starts at **offset 92,288** (found by scanning for a 64-byte-strided run of plausible boxes - `(0, 0, 0.229, 1)`, `(0.093, -0.014, 0.304, 0.767)`, ...), so the records share the shape and only the base differs. Note those do **not** equal the records' counts (`4FLH`/`2U61` = 11/111 fresh, 75/425 shipped), so the counts are a different per-font quantity than the codepoint-run length - something to keep in mind, not an equality to rely on.

The demo's header (177,872 bytes) shows the shape after the directory:

```
 32  4FLH (4096, 2)   PMOC (44896, 49152)
 56  2U61 (4096, 12)  PMOC (83792, 94080)
 80  u32 count = 14, then 14 u32 values: 112, 1, 432, 496, 357, 1520, 0 ...
144  47216, 47280, 49008, 0   then zeros up to the first chunk at 4096
```

So `{magic, version, kind, flags, 16 zero bytes, 4 chunk records, u32 count + values, a small group}` and then the payload starts at offset `4096`. The shipped file has the same shape with `count = 11` and larger values (112, 256, 368, 880, 4,978, 92,016, 729,200, 749,232, 756,464, 759,152).

The two *tiny* records still look odd - `4FLH` and `2U61` share the offset `4096` in both files - so the first word is an offset for the `COMP` records (their ranges are consistent with the file size) but not obviously for those two. Next: settle one record's semantics by following the values (the demo's `47216/47280/49008` sit inside the first `COMP` range) and re-try decompression over the corrected spans.
| Sound (Wwise) | Limited | **Five types are recognised and none is decoded**: `WwiseBank` (`535a7bd3e650d799`, `.bnk`), `WwiseDep`, `WwiseEvent`, `WwiseMetadata` and `WwiseStream` (`.ogg`) are all handled as opaque blobs, which is already enough for a mod to replace a bank or a stream wholesale - so this is a decoding gap, not a shipping one. The tractable slice is the **engine-level** resources rather than the bank binary: `scripts/wwise_bank_reference.lua` in the game's own boot bundle shows the Lua side refers to banks by name, and a `.wwise_event`/`.wwise_dep`/`.wwise_metadata` is a small engine structure (a name plus the banks it references), so it can be decoded and re-emitted while the `.bnk` stays opaque. VT2's SDK ships `.wwise_bank`/`.wwise_dep` sources beside its compiled pipeline (`TEMP/streamable_resources_compile/`) as the reference. **The `.wwise_dep` layout is solved (2026-10-14)** from the two the boot bundle ships: `u32 version` (29 in both, so it is a constant, not a size), `u32 len` (the bank path's length *including* its NUL) and then that path - `world_sound_fx.wwise_dep` is `29, 21, "wwise/world_sound_fx\0"`, exactly its 29 bytes, and `Init.wwise_dep` is `29, 11, "wwise/Init\0"`, its 19. One bank reference per file. So a dep is a three-field resource - read, write and round-trip are trivial, and it needs no bank parsing at all. **But `wwise_event` is not a small engine structure** - the plan above was wrong. Probing 300 bundles found **606** of them (and no metadata), and every sample begins `1d 00 00 00` (version **29**, the *same* word the deps carry, which is now a third confirmation that it is the resource version) followed by a header and then the bytes `42 4b 48 44` = **`BKHD`**, Wwise's bank-header chunk. So an event resource *embeds a Wwise bank*: decoding it fully means decoding the `.bnk` format, which is the large proprietary part - only the Stingray wrapper around it is small. The dep remains the tractable one (version 29, a length, a bank path), and `wwise_metadata` did not appear at all in the sample. **Done: the dependency is implemented** (`filetype/wwise.rs`) - `version`, a NUL-inclusive `len` and the bank path, with `from_binary`/`to_binary`, SJSON, and `decompile`/`compile` wired beside Bones and Particles, so a mod can repoint a dependency without carrying a bank. The reader enforces `8 + len == size` and refuses a truncated resource, and the tests use the shipped files' exact bytes - re-verified field by field against `Init.wwise_dep` and `world_sound_fx.wwise_dep` (version 29, lengths 11/21, a trailing NUL, both satisfying the invariant). The event wrapper stays version + embedded bank rather than a structure to model field by field. The one unrun check is the CLI plumbing itself, which mirrors Bones' arm verbatim. |

## Community references (re-check these)

These move on their own schedule, so revisit them when a format's decode is
picked up - someone may already have cracked it.

### VT2 SDK (the writer's ground truth)

`C:\Program Files (x86)\Steam\steamapps\common\Vermintide 2 SDK` - the official
Vermintide 2 SDK. Under `TEMP/streamable_resources_compile/` it ships the
toolchain's **own** compiler output: 250 compiled `.shader_library` files (the
VT2 device data), `debug_file_index.sjson` naming every one, and
`shader_cache.db`. Its dev binaries carry RTTI/assert strings that name the
writer's types and functions (`stingray::shader_compiler::make_device_data`,
`stingray::D3DShaderPassData` and its `ResourceBindInfo`/`ConstantBufferBindInfo`
/`SamplerBindInfo`, `D3DShaderPass::initialize_shader_program`, ...), and the
source file names (`platform_d3d12.cpp`, `platform_interface.cpp`,
`library_compiler.cpp`). It is the closest thing to a spec for the `shader43`
writer. See `Shader RE TODO.md`, "The VT2 SDK is the writer's ground truth".

### Darktide-Asset-Compiler

`C:\dev\Darktide-Asset-Compiler` - <https://github.com/fviuff/Darktide-Asset-Compiler>

A Blender addon + C++ compiler that authors native Darktide resources (unit,
bones, animations, materials, textures, particles, physics, state machine,
flow). It is a **writer** for the same formats dtmt reads, and it is updated
often (commits through 2026-10; check `git -C <dir> log --oneline`). The parts
worth reading, under `src/stingray/`:

- `material/shader_section.{h,cpp}` - the material stream's shader section
  (version `0x2b`): the compiled shaders of every context and pass, with
  `decode()`/`encode()` exact inverses for shipped sections. Names are
  `IdString32`; a stage is Kraken-compressed DXBC plus the bindings the engine
  builds the root signature from.
- `material/shader_compiler.{h,cpp}` - compiles HLSL with dxc (SM 6.0 DXIL,
  validator 1.7) and builds the **root table**: the order a group's distinct
  constant-buffer / resource names are first bound across its passes and stages.
  That is the numbering our tail records index. It also documents the engine's
  resource-name rule (strip a DXC `__tex_` / `__samp_` prefix) and the per-kind
  tail record shapes (SRV / UAV / structured / raw / sampler / inputs).
- `material/material_v61.{h,cpp}` - a material version 61 writer (`payload
  offset 28`, absent-section `0xffffffff`, header 68, `surface_material` context
  `0x0254A04B`).
- `particles/particles_resource.{h,cpp}` - the particle effect resource
  (version 102) decode/encode plus a component schema (initializers,
  simulators, vertex writers, visualizers, curves/gradients).
- `unit/unit_v115.{h,cpp}`, `bones/bones_writer.*`, `animation/animation_writer.*`,
  `state_machine/state_machine_writer.*`, `flow/flow_resource.*`,
  `physics/*`, `texture/texture_writer.*`, `package/package_v43.*`.
- `murmur_hash.*` and `cooked_resource.*` - the hashing and the cooked-resource
  envelope.

Its `README.md` has an **"On the list"** section (custom shaders, chain-bone
physics, a second UV map, more particle preview) that marks what the community
has *not* cracked yet - a useful "still open" signal.

### Other references

- The VT2 SDK writer artifacts and the `.shader_library` decode - see
  `Shader RE TODO.md` ("The VT2 SDK is the writer's ground truth").
- `C:\dev\bitsquid-blender-tools` - a Python read/write library for the Stingray
  formats (`bitsquid/stingray/`: `unit/`, `bones.py`, `material.py`,
  `mesh_geometry.py`, `mesh_object.py`, `animation.py`, `state_machine`, `level`,
  …) plus its own `unpacking/dictionary.csv`. Its `stingray/resource_header.py`
  names the 38-byte wrapper **`DTHeader`** (`{resource_type, name, unk1..unk3,
  unk4, resource_len, unk5, stream_name_len}`) and says it is "found at the
  beginning of most (all?) resource files" - i.e. the *standalone* resource form,
  which is why a Darktide **bundle variant** carries only the body (see
  `File Type - Bones.-.md`). Useful as a second opinion on field names and on the
  formats DTMT has not started (skin data, scene graph, actor desc, camera, light,
  ragdoll, mover, vertex streams).
- Upstream issue tracker: <https://git.sclu1034.dev/bitsquid_dt/dtmt/issues>.
