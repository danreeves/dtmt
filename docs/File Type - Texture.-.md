# File Type: Texture

A Darktide texture is a bundle entry whose payload (the "wrapper") carries a
compressed **DDS image**. The large mipmaps are split out into a separate
`bundle/data/.../<hash>.stream` file.

The mod-side equivalent is a **`.texture` SJSON file** plus the **`.dds` image**
it points at.

This page is written to be explicit about what is actually known. Every field
and rule is tagged:

| Tag | Meaning |
|-----|---------|
| **Verified** | Confirmed against shipped Darktide bundles (and, where noted, reproduced in game). |
| **Observed** | Present in the sampled game data, but a wider sample could reveal more cases. |
| **Guess** | Our inference. Plausible, but not verified. |
| **Unknown** | No reliable information. |

The sample used to verify this page is a shipped `content/` bundle containing
16 textures (mix of streamed and non-streamed).

---

## 0. At a glance

```
 bundle file entry, type hash CD4238C6A0C69E32 ("texture")
 │
 ├─ streamed      wrapper stored inline in the bundle
 │                + data/xx/<16 hex>.stream   (large mipmaps, Oodle-compressed)
 │
 └─ non-streamed  whole wrapper stored in     data/xx/<16 hex>   (all mipmaps inline)

 wrapper
 │
 └─ <compressed payload>  =  a DDS image (DDS header + inline mipmaps)

 source project
 │
 ├─ foo.texture   SJSON definition (input/output settings)
 └─ foo.dds       the image, referenced by input.filename
```

A texture is either **streamed** (big mipmaps in a `.stream` side file) or
**non-streamed** (everything in one `data/` side file). The two cases are
distinguished by flags in the bundle entry, not by the wrapper itself.

---

## 1. Binary layout in the game

### 1.1 Bundle entry

Textures are ordinary bundle files with the type hash `CD4238C6A0C69E32`.
The outer bundle structure is documented in
[File Type - Bundle](./File%20Type%20-%20Bundle.-.md). Only the parts that
change for textures are listed here.

The game stores textures in two ways, both confirmed against shipped bundles:

| | Non-streamed | Streamed |
|---|---|---|
| FileMeta `flags` | `0x4` (`DATA`) | `0x0` |
| variant `is_data_file` byte | `1` | `0` |
| variant `size` field | `30` — length of the data file name slot | length of the inline wrapper |
| variant `unknown_2` byte | `1` | `1` |
| variant `len_data_file_name` | `0` | `31` — length of the `.stream` name |
| inline data chunk | none; the `size` bytes **are** the data file name | the wrapper (`size` bytes) |
| side file | `bundle/data/xx/<16 hex>` (the whole wrapper) | `bundle/data/xx/<16 hex>.stream` (large mipmaps) |

Status: **Verified** for all columns and values.

Notes:

* The `is_data_file` byte is the thing the Rust code calls `external`.
  `1` means "the data chunk is a file name", not "the file is external".
  Status: **Verified**.
* Side-file names are derived from the resource path, not from the variant's own
  `name` hash. The rule (verified on 4 non-streamed and 3 streamed shipped
  textures) is:
  * `hash = murmur64("<resource path>.texture")`
  * **non-streamed**: use `hash`
  * **streamed**: use `hash | 1` (the low bit is set). Every sampled `.texture`
    hash happened to be even, so this is indistinguishable from `hash + 1`.
  * format as `data/<first 2 hex>/<remaining 14 hex>`, lowercase.
  * Example: `content/textures/ailment_masks/burn_mask_01` →
    `murmur64("…burn_mask_01.texture") = 317d32497fdba34c` →
    `data/31/317d32497fdba34d.stream`.
  * Status: **Verified**. DTMT's compiler instead derives the name from
    `murmur64("<resource path>")` (no `.texture` suffix, no low bit), so its
    names differ. See [§3.4](#34-known-discrepancies--open-questions).
* Non-streamed textures declare the name slot as `24 text bytes + 6 NUL`
  (`size == 30`). Streamed textures declare exactly the 31-byte name with no
  NUL padding. Status: **Verified**. (See
  [§3.4](#34-known-discrepancies--open-questions) — DTMT currently writes a
  different value for the streamed case.)

### 1.2 The wrapper

The wrapper is the bytes in the bundle's inline data chunk (streamed) or in the
whole `data/xx/<hash>` file (non-streamed).

Layout, from the start of the wrapper:

```
offset  size                       field
0x00    u32                        compression_type
0x04    u32                        compressed_size
0x08    u32                        uncompressed_size
0x0C    u8[compressed_size]        compressed payload (the inline DDS)
...     u32                        unknown            == 0x43
...     TextureHeader (148 bytes)  see §1.3
...     [stream chunk table]       only when TextureHeader.meta_size > 0 — see §1.4
...     u32                        category           see §1.7
```

| Field | Status | Notes |
|-------|--------|-------|
| `compression_type` | **Observed** | `1` for every texture inspected. `0` (raw) and `2` (zlib) are handled by DTMT but were not seen for textures. |
| `compressed_size` | **Verified** | Byte length of the compressed payload. |
| `uncompressed_size` | **Verified** | Byte length of the inline DDS after decompression. |
| `unknown` | **Verified** | Always `0x43`. The engine skips it. Purpose **Unknown**. |
| `category` | **Verified** | See §1.7. |

`compression_type` values:

* `1` — Oodle (the normal case).
* `2` — zlib.
* `0` — raw, not compressed. In this mode `compressed_size` and
  `uncompressed_size` are both `0`, the bytes up to the final `u32` are a raw
  DDS image, and there is **no** `0x43` field and **no** `TextureHeader`.
  Status: **Unknown** (implemented by DTMT, but not seen in texture data).

### 1.3 TextureHeader (148 bytes)

Immediately after the `0x43` field. All values little-endian.

```
0x00  u32  flags
0x04  u32  n_streamable_mipmaps
0x08  u32  width
0x0C  u32  height
0x10  MipInfo[16]   { u32 offset; u32 size; }   // 128 bytes
0x90  u32  meta_size
```

`4 u32 + 16 * 8 bytes + 4 u32 = 148` bytes.

Status: **Verified** (layout and values reproduced against shipped bundles).

`flags` bits:

| Bit | Name | Meaning | Status |
|-----|------|---------|--------|
| 0 | `STREAMABLE` | Texture has streamed mipmaps. Mirrors `n_streamable_mipmaps > 0`. | **Verified** |
| 1 | — | Never observed set. | **Unknown** |
| 8 | `SRGB` | Texture is sRGB. | **Verified** |

`width` / `height` are the dimensions of the **full** image (the largest mip),
even when that mip is stored in the `.stream` file. Status: **Verified**.

`mip_infos[16]`:

* One entry per mip level, largest first. Entries past the mip count are zero.
* For streamed mipmaps (indices `0 .. n_streamable_mipmaps`) `offset` is
  relative to the start of the **uncompressed stream region**.
* For the remaining (inline) mipmaps `offset` is relative to the start of the
  **inline mip data** (i.e. after the DDS header, so the first inline offset is
  `0`), not to the start of the DDS.
* `size` is the uncompressed byte size of that mip.

Status: **Verified** from shipped data (offsets restart at `0` at the first
inline mip; streamed offsets accumulate from `0`).

`meta_size`:

* `0` when there are no streamed mipmaps.
* Otherwise `8 + num_chunks * 4`, i.e. the byte size of the chunk table in §1.4.

Status: **Verified**.

### 1.4 Stream chunk table

Present only when `meta_size > 0` (i.e. the texture is streamed). Follows the
`TextureHeader`.

```
u32  num_chunks
u16  0                 // purpose Unknown
u16  num_chunks        // same value as the u32
u32  chunk_ends[num_chunks]
```

`chunk_ends[i]` is the cumulative **compressed** end offset of chunk `i` in the
`.stream` file. The size of chunk `i` is `chunk_ends[i] - chunk_ends[i-1]`
(with `chunk_ends[-1] = 0`), and the last entry equals the `.stream` file size.

Status: **Verified** (e.g. 10 chunks → `meta_size == 48`, last end `655380` ==
the file's byte size).

### 1.5 Inline DDS

The decompressed payload is a standard DirectDraw Surface image:

* `DDS ` magic + 124-byte `DDS_HEADER` + 20-byte `DDS_HEADER_DXT10`
  (the `pixel_format.four_cc` is always `DX10` in DTMT output), for
  **148 bytes** of header in total.
* Followed by the inline mipmap levels, largest first, tightly packed.

Important: the inline DDS describes **only the mipmaps kept inline**, not the
whole chain. For a streamed texture its header width/height/mipmap count are
the dimensions of the first *inline* mip. The full dimensions live in the
[TextureHeader](#13-textureheader-148-bytes). Status: **Verified**.

Formats observed, all via `DX10`:

| DXGI value | Name | Notes |
|-----------|------|-------|
| 71 | `BC1_UNORM` | 8 bytes / 4×4 block |
| 80 | `BC4_UNORM` | 8 bytes / 4×4 block |
| 83 | `BC5_UNORM` | 16 bytes / 4×4 block |
| 98 | `BC7_UNORM` | 16 bytes / 4×4 block |

Status: **Observed**. Non-square and tiny textures (e.g. `4×4`, `512×8`) occur.
Other DXGI formats are probably possible — **Unknown**.

### 1.6 Streamed mipmap data (`.stream`)

The `.stream` file is a plain concatenation of Oodle-compressed chunks. There is
no header; the chunk table in the bundle wrapper is the index.

* Every chunk decompresses to exactly **65536 bytes** (`0x10000`).
* A chunk is `64` block rows tall. One block row is `1024` bytes, so a chunk
  covers `64 × 1024 = 65536` bytes.
* How wide a chunk is depends on the block size:
  * 16-byte formats (BC5/BC7): 64 blocks = 256 px wide.
  * 8-byte formats (BC1/BC4): 128 blocks = 512 px wide.
* Chunks are stored **mip by mip**, largest mip first, and within a mip in
  **column-major** order: `band = i / chunks_wide`, `column = i % chunks_wide`.
* To reassemble a mip, place chunk `i` at column `i % chunks_wide` and row band
  `i / chunks_wide`. A chunk fills `chunk_width_blocks` blocks wide and `64`
  block rows tall, at a destination row pitch of `blocks_wide * block_bytes`.

Status: **Verified**. The number of chunks computed this way matches every
sampled wrapper, and each `.stream` file size matches the last `chunk_ends`
entry. The reassembly is additionally exercised in game.

### 1.7 Category

A `u32` murmur32 hash of an engine id, normally a
`content/texture_categories/*` string. DTMT looks unknown hashes up in the
dictionary; known ones are printed as strings, unknown ones as 8 uppercase hex
digits.

Known values (from observed data and the engine's memory tree):

| Hash | String |
|------|--------|
| `BECF238C` | `content/texture_categories/ui` |
| `EB8156DA` | `content/texture_categories/loading` |
| `E4378318` | `content/texture_categories/character_nm` |
| `8B36AA40` | `content/texture_categories/character_orm` |

Status: the four mappings above are **Observed**. The complete set of categories
is **Unknown**.

---

## 2. Source files

### 2.1 `.texture` (SJSON)

A texture definition is SJSON. Only the `common` (PC) section is used.

```sjson
common = {
  input = {
    filename = foo.dds
  }
  output = {
    category = content/texture_categories/character_nm
    srgb = true
    streamable = true
  }
}
```

Schema:

| Path | Type | Default | Meaning | Status |
|------|------|---------|---------|--------|
| `common.input.filename` | string | — (required) | Path to the `.dds`, relative to the project root. The `.dds` extension may be omitted. | **Verified** |
| `common.output.category` | string | `""` | Texture category. Either a name (hashed with murmur32) or an 8-hex-digit hash (used as-is). | **Verified** |
| `common.output.srgb` | bool | `false` | Sets the `SRGB` flag. | **Verified** |
| `common.output.streamable` | bool | `false` | Stream the large mipmaps into a `.stream` side file. Sets the `STREAMABLE` flag. | **Verified** |
| `common.output.mipmap_num_largest_steps_to_discard` | u32 | `0` | Drop this many of the largest mipmaps before packing. | **Verified in code**, not exercised against shipped data |
| `common.output.mipmap_num_smallest_steps_to_discard` | u32 | `0` | Drop this many of the smallest mipmaps before packing. | **Verified in code** |

Serialization rules (what a decompiled file looks like):

* Fields are written only when they differ from their default: `srgb` is omitted
  when `false`, `streamable` when `false`, the discard counts when `0`.
* `category` is always written.
* Indentation is two spaces. Status: **Verified** (matches decompiled output).

Stingray allows additional `output` options (`format`, `apply_processing`,
`cut_alpha_threshold`, `mipmap_filter`, `mipmap_filter_wrap_mode`,
`mipmap_keep_original`, …). DTMT **accepts but ignores** them, because unknown
SJSON fields are dropped while parsing. Status: **Verified in code**.

Stingray also allows per-platform override sections (e.g. `windows { … }`).
DTMT only supports `common`. Status: **Verified in code**.

### 2.2 `.dds`

A normal DDS file. It is the actual image data and is referenced by
`common.input.filename`.

* For **streamed** textures it must be a block-compressed format (BC1/BC4/BC5/
  BC7). Uncompressed streamed textures are rejected.
* For **non-streamed** textures an uncompressed DDS is accepted and stored
  as-is, but it is not parsed, so it cannot be discarded/streamed.
* On compilation DTMT parses the DDS, normalizes the header to `DX10`, and
  rewrites it. Legacy `DXT1`/`DXT3`/`DXT5` are mapped to `BC1`/`BC2`/`BC3`.
* The mip chain is taken from the DDS; the header's mip count is used.

Status: **Verified**.

---

## 3. Mapping

### 3.1 Decompile (game → source)

For a texture named `foo`:

1. Read the wrapper (inline, or the whole `data/xx/<hash>` file).
2. Decompress the payload → inline DDS.
3. If a `.stream` file exists, decompress every chunk, reassemble each streamed
   mip (§1.6), and prepend them to the inline mips.
4. Write `foo.dds` with a header describing the **full** mip chain.
5. Write `foo.texture` with:

| Game value | Source field |
|------------|--------------|
| `flags & STREAMABLE` | `output.streamable` |
| `flags & SRGB` | `output.srgb` |
| `category` hash | `output.category` (string if known, else 8-hex) |

`width`, `height`, `n_streamable_mipmaps` and `mip_infos` are used to rebuild
the DDS but are **not** written to the `.texture` — they are recomputed on
compile. The discard counts are always decompiled as `0` (they are not
recoverable from the binary). Status: **Verified**.

### 3.2 Compile (source → game)

1. Parse `foo.texture`; read the DDS at `input.filename`.
2. Parse the DDS mip chain; apply the two discard counts (drop from the start /
   end of the chain).
3. Decide `n_streamable`: if `streamable`, mipmaps are streamed while their
   uncompressed size is `>= 65536` bytes; the smallest mip is always kept inline.
   If `streamable` is `false`, nothing is streamed.
4. Build the inline DDS (DX10 header) from the kept inline mips.
5. Fill the `TextureHeader`: `flags`, `n_streamable_mipmaps`, `width`, `height`,
   `mip_infos`, `meta_size`.
6. If anything is streamed, Oodle-compress each 64 KiB chunk into the `.stream`
   data and write the chunk table.
7. Write the wrapper. Streamed → inline in the bundle + `.stream` side file.
   Non-streamed → whole wrapper in a `data/xx/<hash>` side file.

Source → game field mapping:

| Source | Game |
|--------|------|
| `input.filename` | the DDS that is read; `root/filename`, appending `.dds` if it does not exist and has no extension |
| `output.category` | `category` — 8 hex digits are parsed as a hash, anything else is murmur32-hashed |
| `output.srgb` | `flags` bit 8 |
| `output.streamable` | `flags` bit 0, and selects inline-vs-side-file storage |
| `mipmap_num_largest_steps_to_discard` | dropped from the front of the mip chain |
| `mipmap_num_smallest_steps_to_discard` | dropped from the back of the mip chain |
| — | `width`/`height`/`n_streamable_mipmaps`/`mip_infos`/`meta_size`/chunk table are computed |

Status: **Verified** against the implementation; the wrapper layout matches
shipped data, apart from the side-file naming caveats in §3.4.

### 3.3 Worked example (shipped data)

`content/textures/ailment_masks/burn_mask_01` is a streamed texture. In the
bundle:

* FileMeta `flags = 0x0`, `is_data_file = 0`, `size = 40432`, `unknown_2 = 1`,
  `len_data_file_name = 31`.
* Side file: `data/31/317d32497fdba34d.stream`, 655380 bytes.

Wrapper (`data` = 40432 bytes):

| Field | Value |
|-------|-------|
| `compression_type` | `1` |
| `compressed_size` | `40216` |
| `uncompressed_size` | `43836` |
| `unknown` | `0x43` |
| `flags` | `0b1` (`STREAMABLE`) |
| `n_streamable_mipmaps` | `2` |
| `width` × `height` | `1024 × 1024` |
| `mip_infos` | `0/524288, 524288/131072, 0/32768, 32768/8192, 40960/2048, 43008/512, 43520/128, 43648/32, 43680/8, …` |
| `meta_size` | `48` |
| chunk table | `num_chunks = 10`, ends `65538, 131076, …, 655380` |
| `category` | `0xE5A8544A` |

Interpretation:

* Format is `BC4_UNORM` (8-byte blocks). Mip 0 = `1024×1024` = `524288` bytes,
  mip 1 = `512×512` = `131072` bytes. Both are `>= 65536`, so both are streamed.
* Inline mips start at `256×256` = `32768` bytes; the inline DDS is
  `148 + (32768 + 8192 + 2048 + 512 + 128 + 32 + 8) = 43836` bytes.
* Chunk count: mip 0 = `ceil(256/128) * ceil(256/64) = 2 * 4 = 8` chunks,
  mip 1 = `1 * 2 = 2` chunks → 10.

The decompiled source is:

```sjson
common = {
  input = {
    filename = 0400F8C315A60225.dds
  }
  output = {
    category = E5A8544A
    streamable = true
  }
}
```

together with a 699196-byte `0400F8C315A60225.dds` (all 9 mips,
`BC4_UNORM`, `DX10`).

### 3.4 Known discrepancies / open questions

* **Side-file names are regenerated, not preserved.** The game derives them from
  `murmur64("<resource path>.texture")` (low bit set for streamed). DTMT derives
  them from `murmur64("<resource path>")`. A decompile → compile cycle therefore
  produces a different `data/xx/<hash>[.stream]` name than the game used. It
  still loads in game (the engine reads the name from the bundle), but the bytes
  are not identical to shipped data. Status: **Observed discrepancy**.
* **Streamed data-file name length.** Shipped streamed textures declare
  `len_data_file_name = 31` (exactly the `data/xx/<hash>.stream` name, no NUL
  padding). DTMT's writer uses `name.len() + 6`, which yields `37` for the same
  name. Both are self-consistent because the reader consumes `len` bytes, and
  the compiled result reportedly works in game, but it is **not** byte-identical
  to shipped data. Status: **Observed discrepancy**.
* `flags` bit 1 — never seen set. Status: **Unknown**.
* The `0x43` field before the `TextureHeader` — constant, skipped by the engine,
  meaning **Unknown**.
* The two `u16` fields in the chunk table (`0` and `num_chunks`) — meaning
  **Unknown**.
* The full set of `category` hashes — **Unknown**.
* `compression_type` `0` and `2` — handled by DTMT, not observed for textures.
  Status: **Unknown**.
* `mip_infos` are read but not used when decompiling; the DDS header is used
  instead. Whether the engine relies on them at load time is **Unknown**.
* Whether the engine validates/recomputes the side-file name, or simply reads
  the name recorded in the bundle, is **Unknown** (the naming scheme itself is
  verified above).
* Stingray per-platform texture sections and the non-implemented `output`
  options are ignored.
