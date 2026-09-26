# Shader progress: where this stands and what to do next

This is the pick-up point. Read this before the other shader notes; it names
what is verified, with sample sizes, and what is still a reading.

## Repo state (2026-09-26)

`main` is the trusted baseline plus reviewed commits:

```
b69d8e9 condition_tree: the payload's branches, and the tests-1 result
4be58e5 condition_tree: the payload reads as guarded results
8b89277 condition_tree: the conditions framing is decoded
45bb351 docs: the shader notes, corrected and scoped
e0c2153 sdk: the shader declaration reader, section codec and group data, reviewed
ab4f385 build: vendor the SJSON dialect fork
2c79541 docs: tail signature runs and trailing run are family-independent   <- trusted baseline
```

`docs/Shader Section Generation Notes.md` and `docs/Shader RE TODO.md` are the
reference; this file is the pick-up point.

## Build and test

- `cargo test -p sdk` needs `E:\SteamLibrary\steamapps\common\Warhammer 40,000
  DARKTIDE\binaries` on `PATH` (links `oo2core_9_win64.dll`).
- Expected: 118 pass, 3 pre-existing `filetype::package` failures.
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
shader43 --group-data <section>
shader43 --dependencies <section>
shader43 --plan <declaration.shader_node> <section>
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
- **Conditions roots**: `gui` 9FCFE126, `red` 9B8DE7E4, `green` 4BA4BD58,
  `blue` 0977913D, `alpha` 3F697354; `BDF72706`, `B5F45768`, `8FB860CF`,
  `E2C8865F`, `BC4EE226` unnamed. [dictionary + 35 records]

## Open, in the order to attack

1. **Map the conditions payload's result indices.** The pattern is confirmed on
   all 35 UI-base records: every branch's result is `tests.len() - 1`, and
   `5007` is the fallback where a record has one. It is **not a group
   selector**: every query id appears exactly once in the group data, one group
   per query, on all seven sections [7/7]. So the result refines the interface
   within the group (the per-group variable table). Next experiment: compare
   each record's result set with its group's descriptor `Y` fields and the
   length of the group's material table (groups 0-11 have `Y` 5/10, groups 12+
   have 1/2); then confirm in game with a crafted tree.
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
3. **Group data constructor**: with the walk, a first constructor can carry each
   group's byte-packed header and descriptors from the template - they are the
   family's compiled interface metadata, like the block - generate the material
   and channel tables, and keep the engine table. Generating the header itself
   needs the grammar in (2). `rebuild`/`rebuild_channels` already write the
   tables correctly.
4. **Wire `Section::build` into `dtmt build`** with generated group data and a
   real conditions tree; verify with the round trip and the substitutions before
   any deploy.
5. **In-game test** via snoopy-mod: title screen only, no space at boot,
   screenshot the Darktide window (borderless fullscreen -> PrintWindow).
6. **`.shader_source` parsing** (`hlsl_shaders = { name = { code } }`) and the
   code_blocks -> programs path.

## In-game harness

`docs/scripts/` holds the scripts used to test in game. `title-tint-demo.ps1`
is the shape to copy: kill Darktide, launch `scripts/launch.bat` from the
install directory (the game ships no launcher), poll the newest console log for
the title material line (`material set: background_image`), screenshot and
average a region's colour a few times, and grep the log for the mod. `shot-window.ps1` uses `PrintWindow` so a borderless-fullscreen window is
captured rather than the desktop; `set-shader.ps1` and `slice-sections.ps1` move
a section in and out of a material. The deployable test mod is the snoopy-mod
checkout. The payload experiment that needs this: a generated family whose
groups render distinguishable colours and whose crafted conditions tree maps
channel sets to them, then drive the material from Lua and read which colour
appears.

## Open decode details worth keeping

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
