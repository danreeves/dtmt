# File Type - Bones

A `bones` resource is a cooked **skeleton**: the joint names, their 32-bit ids
and the LOD joint counts. It is a small resource the engine loads beside a
`unit` to give a skinned model its joints.

Status of each statement:

| Tag | Meaning |
|-----|---------|
| **Verified** | Confirmed against the reader and the tests. |
| **Observed** | Seen in a file, but a wider sample could add cases. |
| **Guess** | Our inference; not verified. |

## Format

The resource is a **38-byte cooked-resource envelope** followed by the body:

```
+0x00  u64  type hash       # 0x18DEAD01056B72E9 for `bones`
+0x08  u64  name hash       # the resource path's own hash
+0x10  u8   1
+0x11  u8[12] zero
+0x1D  u32  body size
+0x21  u8   1
+0x22  u32  stream size     # 0 for bones (no external stream)
       body
       stream name          # `stream size` bytes
```

The body (`body size` bytes):

```
u32  joint count
u32  lod count
u32  joint id     x joint count
u32  lod count    x lod count
char joint name\0 x joint count
```

A joint's **id** is [`Murmur32`] of its name, which is the upper 32 bits of the
seed-zero `Murmur64` the engine uses for an `IdString32`. The LOD counts are the
number of joints in each LOD; the first is the full joint count and the counts
are non-increasing. Status: **Verified in code** (the reader checks every id
against its name, and the tests round-trip the body).

The layout is taken from the Darktide-Asset-Compiler's
`stingray/bones/bones_writer.cpp` (`build_bones_body`) and
`stingray/cooked_resource.cpp`, which write cooked bones the game loads.

## SJSON form

`DTMT decompiles` a variant to SJSON and `dtmt build` compiles it back:

```text
name = #ab2ea126a37d8be7
names = [
	j_root
	j_spine
	j_head
]
lods = [
	3
	2
]
```

`name` is the envelope's name hash; `dtmt build` replaces it with the resource
path's own hash, so a hand-authored source may omit it. The joint ids are not
stored in the SJSON - they are recomputed from the names.

## Tooling

- `dtmt bundle extract --decompile` writes the SJSON for a bones entry.
- `dtmt build` compiles a `.bones` SJSON listed in a package's `bones = [...]`
  into the cooked bundle.

## Gaps

- **Not yet checked against a game-shipped `.bones`.** The format is verified
  against the community writer and the tests, but no shipped sample has been
  decoded (the game's `bundle_database.data` listing is slow to scan).
