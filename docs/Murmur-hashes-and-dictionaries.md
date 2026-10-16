Fatshark heavily utilizes hashing to turn arbitrary strings into fixed-size data types. Specifically, they use Murmur in its 64-bit variant, which is very fast to calculate, while still producing a decent uniformity (inverse likelihood of producing duplicates). Depending on the situation, they either use the full 64-bit output of the hash function, or, when they don't need as much data, they cut it and only use the first 32 bits. While Murmur does have a "proper" 32-bit variant, Fatshark does not seem to use that anywhere.

However, to be able to do things like loading packages or finding a specific file, we need to know the original string values. The Dictionary (`dictionary.csv` by default) is a collection of known (or guessed) strings. It maps these back to their hash values, so that whenever a hash value is found in the game files, it can be looked up and matched in the dictionary.

But the dictionary is far from complete, and likely never will be. Therefore, DTMT will fall back to producing the hash value itself whenever it encounters a Murmur hash that cannot be found in the dictionary. It is important to note that because of this, decompiling and re-compiling a file is not always idempotent, as the stringified hash value will produce a different hash than the original string.

CSV format for the dictionary: `string,murmur64,murmur32,group`.

The `group` is one of several (see `dtmt dictionary add --help`), which segment hashes by their usage in the engine. While uniformity for Murmur is decent, Fatshark's compilation pipeline only ensures unique hashes within their respective usage groups. E.g. hashes for file names are ensured to be unique among themselves, but may overlap with the ones for localization IDs in `.strings` files. Therefore, DTMT has to respect the same groups to avoid false-positive matches during decompilation.
## Merging a community dictionary (2026-10-16)

`C:\Users\Dan\Downloads\darktide-dictionary.csv` (235,447 entries) was merged in: **44,284 names
added**, 241 skipped as duplicates, taking this tree's dictionary from 1,780,864 lines to
**1,825,186 entries** (1,825,187 lines with the header).

The two formats differ, and that matters when comparing them:

| file | format |
|---|---|
| theirs | `hash,type,path` - lowercase hash, no group |
| this tree's | `value,long,short,group` |

So a comparison must match *their* field 1 against *our* field 2, upper-cased. Matching field 1
against field 1 - a first attempt - reports **100% different**, which is impossible and is the tell
that the columns are wrong.

Most of the additions are `content/...` (34,111) and `wwise/...` (10,036), with a long tail
(`scripts` 159, `base` 93, `dialogues` 31, `packages` 15, ...). The `wwise` ones are the most useful
of the tail, since the sound workstream's next steps hinge on resolving bank and event names.

`dtmt dictionary add -g filename <file>` does the whole job: names line by line, duplicates skipped in
O(n) (see the comment in `cmd/dictionary.rs`), and it **writes the dictionary itself** - a following
`dictionary save` is unnecessary and only re-serialises what is already on disk.

Two things to know before repeating it:

- `dictionary.csv` is **not tracked by git** - it is a local 124 MB artifact - so a merge needs no
  commit;
- the written line count is not `old + added`. The loader materialises entries the file does not spell
  out (39 here), so the file grows by `added + 39`. The arithmetic closes:
  `1,780,864 + 44,284 + 39 = 1,825,187`.

### What the merge resolved in this tree (2026-10-16)

Measured rather than assumed: every hex hash appearing in `docs/`, `lib/`, `crates/` and snoopy-mod
is **279 candidates**, of which **73 resolved before the merge and 77 after** - five set members,
four net entries, every one of them `content/texture_categories/...` (`weapon_nm`, `environment_nm`,
`character_nm`, `environment_hm`, `skydome`), all in the `texture_category` group.

So the honest answer to "what does this unlock?" is: **almost nothing in our own docs and code**. The
five are the 64-bit forms of categories this tree already had in 32-bit, they appear nowhere in the
tree, and the texture reader uses `IdString32` only. That matches the shape of the additions too -
they are overwhelmingly *paths* (`content/...` 34,111, `wwise/...` 10,036), which is what a community
dictionary from an asset dump mostly contains.

The merge's value is therefore in the **data**, not in the docs: resolving `#<hex>` names as
decompiled materials, shader channel records, unit sections and animation names are read. This
measurement does **not** cover that - it checked the repo and snoopy-mod, not the extraction corpus -
so the data side is still unmeasured.
