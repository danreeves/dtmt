Case 1:

```
header {
    unknown_1 == 0,
    size > 0,
    unknown_2 == 1,
    len_data_file_name == 31
}
```

`data` is non-zero size. The `extra_file` points to a `.stream` in `data/`.

Case 2:

```
header {
    unknown_1 == 1,
    size == 30,
    unknown_2 == 1,
    len_data_file_name == 0
}
```

`data` is a file path in `data/`. The file name will be without extension, ends with `0x0` and will be filled up by `0x66`s until `header.size` (usually 5).

---

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

[WIP DDSImage::load](https://git.sclu1034.dev/bitsquid_dt/dtmt/wiki/WIP-DDSImage%3A%3Aload)