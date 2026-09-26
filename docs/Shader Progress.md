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
- Expected: 117 pass, 3 pre-existing `filetype::package` failures.
- The six extracted sections are in
  `C:\Users\Dan\AppData\Local\Temp\opencode\dtmt-mat\ui-mat\*.raw` (each is a
  20-byte wrapper then the section; the example handles the wrapper). The UI
  base is reconstructed from `uib.*.bin` parts or from `uib.material`, which the
  tooling can also read. These live in the temp directory: if they are gone,
  re-extract from the game or ask; the round-trip checks depend on them.
- Dictionary: `C:\Users\Dan\AppData\Local\Temp\opencode\dtmt-mat\dt-dictionary.csv`
  (or `C:\dev\dtmt\dictionary.csv`).

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
   `5007` is the fallback where a record has one. What the index selects (group
   or interface variant) is not established. Next experiment: for each record,
   take its context query id and the group headers' query ids, and see whether
   the result index maps to a group position or to a Y-field interface. The
   confirming test is in-game: a generated family with a crafted tree, observe
   which group the engine selects.
2. **Decode the per-group header** (the 74/57/29-byte structures between the
   descriptors and the first table, e.g. UI base groups 0-11 carry
   `9FCFE126`). It names each table's offset and is what unblocks a from-scratch
   group data constructor.
3. **Group data constructor**: with (2), generate the material and channel
   tables and the descriptors; carry the engine table, the group hash and the
   block. `rebuild`/`rebuild_channels` already write the tables correctly.
4. **Wire `Section::build` into `dtmt build`** with generated group data and a
   real conditions tree; verify with the round trip and the substitutions before
   any deploy.
5. **In-game test** via snoopy-mod: title screen only, no space at boot,
   screenshot the Darktide window (borderless fullscreen -> PrintWindow).
6. **`.shader_source` parsing** (`hlsl_shaders = { name = { code } }`) and the
   code_blocks -> programs path.

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
