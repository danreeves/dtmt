# Reverse Engineering: Textures

The confirmed layout lives in
[File Type - Texture.-](File%20Type%20-%20Texture.-.md). This page keeps the
raw notes and the historical draft that led there, and lists what is still
unresolved.

## General

There are two ways a texture is stored in a bundle, "data file" vs "stream
file". Confirmed against shipped bundles:

- **data file** (non-streamed):
  - points to a file in `data/` without a `.stream` extension
  - `is_data_file == 1` (the data chunk is the file name), no data in the bundle
  - the whole wrapper (all mipmaps inline) lives in that file
- **stream file** (streamed):
  - points to a `.stream` file in `data/`
  - `is_data_file == 0`; the bundle always contains the wrapper
  - the bundle holds the DDS header and the small inline mipmaps, the `.stream`
    file holds the large mipmaps as Oodle chunks

Other observations:

- `BundleFileVariant.unknown_2` (called `unknown_1` in one draft) is always `1`.
- Textures can be duplicated across bundles.
- Non-streamed data file names are `data/xx/<16 hex>` in a 30-byte slot
  (24 text bytes + 6 NUL). Streamed names are `data/xx/<16 hex>.stream` and the
  declared length is exactly `31`.
- The name itself is `murmur64("<resource path>.texture")`, with the low bit
  set for streamed files (`… | 1`).

## Formats

Formats observed so far, all via a `DX10` FourCC:

| DXGI | Name |
|------|------|
| 71 | `BC1_UNORM` |
| 80 | `BC4_UNORM` |
| 83 | `BC5_UNORM` |
| 98 | `BC7_UNORM` |

See [File Type - Texture.-](File%20Type%20-%20Texture.-.md) for the full layout
and the stream chunking rules.

## Decompiling

Decompilation now extracts the inline DDS and reassembles every streamed mipmap
into a complete DDS, and emits the matching `.texture` SJSON. (An earlier
implementation only extracted the largest mipmap.)

## Historical draft (010 Editor template)

This was the first working interpretation of the wrapper, kept for reference.
Some comments are guesses that have since been confirmed or corrected.

```010editor
uint32 compression_type;
Assert(compression_type == 1 || compression_type == 0);

if (compression_type == 1) {
    uint32 compressed_size;
    uint32 uncompressed_size;

    byte data[compressed_size] <format=hex, bgcolor=0x888545>;

    // Seems to be skipped by the decompiled binary's code.
    // Doesn't fit to be a length of a later data section. It doesn't match anything, and it's an uneven number.
    uint32 unknown_1 <comment="Always 67 (0x43), so maybe a format/type specifier.">;
    //Assert(unknown_1 == 0x43);

    // At this point, the code reads an array of 37 uint32 fields.
    // This will be slowly split into individual named fields as I figure them out.

    typedef struct TextureHeader {
        // Checked bits:
        // >> 1: Some extra calculations are done with the next three fields around mipmaps
        //       Also determines if `unknown_2` is saved into `TextureImage`
        //       => Presumable means "has mipmaps" or VT2's "is streamable"
        // >> 2: So far, I haven't found a line of code that actually uses this bit. But there is a line that does `flags & 3`,
        //       indicating that this bit does get used somewhere else.
        // >> 8: TextureFormat.srgb
        uint32 flags <format=binary, comment="Some bitflags.">;

        uint32 n_streamable_mips;
        uint32 width;
        uint32 height;

        // At some point, the code will move 68 bytes from an offset to the start of this array.
        uint32 unknown_2[(68 + 60) / 4] <bgcolor=0x9b86d3, fgcolor=0xb2dbeb>;

    };

    TextureHeader texture_header;

    uint32 meta_size;

    // According to limn, this is something that happens inside `meta_data`.
    // But so far, I haven't found the part in the decompiled binary that handles this data.
    if (meta_size > 0) {
        uint32 num_chunks <comment="Connected to `largest_{width|height}`">;
        Assert((num_chunks * 4) + 8 == meta_size);

        uint16 unknown_6 <hidden=true>;
        Assert(unknown_6 == 0);

        uint16 num_chunks_2 <hidden=true, comment="Always the same as `num_chunks`">;
        Assert(num_chunks == num_chunks_2);

        uint32 chunks[num_chunks];
    }

    // A key to look up a value in the engine's internal render quality settings.
    // Specifically, a value that VT2 calls `mip_drop`.
    // TODO: Check how many unique values there are in the game files.
    // Ideally, there would only be a handful that we can simply hard code.
    uint32 category <format=hex, comment="Assumed, since VT2 has the same thing.">;

    // Shortcuts to get to compile this:
    // - `flags = 0` -> skips most of the mipmap stuff in the bundle file
    // - `width = 0`, `height = 0` -> also skips some of the mipmap stuff
    // - `n_streamable_mipmaps = 0` -> seems to be an intended value to only rely on DDS mipmaps
    //                                 It does force `srgb` on through setting a special value for `flags`, though. So if that produces issues, I might
    //                                 need to fake it s
} else if (compression_type == 0) {
    uint32 unknown_1;
    uint32 unknown_2;

    Assert(unknown_1 == 0);
    Assert(unknown_2 == 0);
}
```

`compressed_data` once uncompressed:

```010editor
byte unknown_1[8];

uint32 flags;

uint32 unknown_2;

uint32 base_width;
uint32 base_pitch;

byte unknown_3[60];


uint32 four_cc;
Assert(four_cc == 0x44583130) // Or reverse?

byte unknown_4[40];

uint32 dxt10_encoding_kind;
uint32 dxt10_dimension;
Assert(dxt10_dimension == 3);
uint32 dxt10_misc_flags;
uint32 dxt10_array_size;
Assert(dxt10_array_size == 1);
uint32 dxt10_misc_flags_2;
```

The `TextureHeader.unknown_2[(68 + 60) / 4]` array turned out to be sixteen
`{ u32 offset; u32 size; }` mip entries. The `compressed_data` block is an
ordinary DDS image.

## Open questions

See [§3.4 of the format page](File%20Type%20-%20Texture.-.md#34-known-discrepancies--open-questions).
The main ones:

* Shipped streamed textures declare the `.stream` name length as `31`, while
  DTMT's writer uses `name.len() + 6 = 37`.
* The game derives side-file names from `murmur64("<resource path>.texture")`
  (low bit set for streamed). DTMT derives them from the bare resource hash, so
  its names differ.

## Cross-check with the community writer (2026-10-12)

`Darktide-Asset-Compiler`'s `stingray/texture/texture_writer.cpp` gives its
profiles and its side-file rule:

- `TextureProfile{id, dxgi_format, block_bytes, srgb, body_flags, footer_word,
  filtering}` - e.g. `substance_basic_bc_static_control_bc1_srgb` = BC1 sRGB
  (`dxgi_format 72`, 8 B/block, `body_flags 0x101`, `footer_word 0xa88c22b7`) and
  `substance_basic_orm_bc1` = BC1 linear (`71`, 8, `0x001`, `0xe514d1dc`). So the
  body's flags word is **`0x101` for an sRGB family and `0x001` for a linear one**,
  the footer is a per-family hash, and there are five families (bc, bca, nm, orm,
  pbr_emissive_em).
- its side-file name is
  `data/am/<hex16(id64("texture-stream:" + resource))>.stream`, with `id64` =
  `murmur64(s, 0)`.

The *shipped game* agrees with DTMT's **shape**, not the community's hash: of the
**186,955** `bundle/data/**/*.stream` files, every path is
`data/<the name's first two hex>/<16 hex>.stream` - the directory **is** the name's
first byte pair - and the names are a toolchain scheme (a counter in the high u16
plus a hash), not a name hash. So the name is arbitrary and only the *path the
resource records* matters; DTMT's form is right. The real discrepancies stay the
two "Open questions": the `.stream` **slot length** (shipped `31`, DTMT
`name.len() + 6 = 37` at `file.rs:112`; the non-streamed slot is `30 = 24 + 6`, so
the `+6` looks like a non-streamed convention) and whether the game ever recomputes
the name. Verifying either means parsing a *shipped* bundle's texture variant and
reading `data_file_name_len` against the loose `.stream` beside it.
