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

A bundle variant stores the **body alone**. None of **723** shipped `bones`
variants (decoded from 2,500 sampled game bundles) carries an envelope: the
resource's type and name live in the bundle file entry, and the variant's bytes
open straight with the body's `joint count`.

The body:

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
are non-increasing. Status: **Verified against shipped data** - all 723 samples
decode, every joint id checks out, and `to_binary` reproduces the input bytes
exactly.

The community writer's `stingray/cooked_resource.cpp` instead wraps the body in a
**standalone 38-byte envelope** (which is what made the earlier reading assume an
envelope):

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

`Bones::from_binary` accepts that form too - it strips the envelope when one is
present - so such a blob still loads, while `to_binary` always emits the bare body
a bundle wants.

## SJSON form

`DTMT decompiles` a variant to SJSON and `dtmt build` compiles it back:

```text
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

The joint ids are not stored in the SJSON - they are recomputed from the names -
and neither is the resource name, which is the bundle entry's.

## Tooling

- `dtmt bundle extract --decompile` writes the SJSON for a bones entry.
- `dtmt build` compiles a `.bones` SJSON listed in a package's `bones = [...]`
  into the cooked bundle.
- `dtmt bundle inject <bundle> replace <resource path> <file.bones>` compiles a
  source into an existing bundle.

## Gaps

- None outstanding on the format. It is verified against **723 shipped variants**
  from 2,500 sampled game bundles (`examples/bones_probe.rs`): all decode, pass
  every joint-id check and round-trip byte-for-byte. The earlier "38-byte
  envelope" reading came from the community's *standalone* writer and does not
  apply to a bundle variant - `Bones::from_binary` still accepts it so such blobs
  load.
