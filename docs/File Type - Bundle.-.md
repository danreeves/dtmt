```
//------------------------------------------------
//--- 010 Editor v12.0.1 Binary Template
//
//      File: dt_bundle_decompressed.bt
//   Authors: Lucas Schwiderski
//   Version: 
//   Purpose: 
//  Category: Darktide
// File Mask: 
//  ID Bytes: 
//   History: 
//------------------------------------------------

typedef struct {
    uint64 extension <format=hex>;
    uint64 name <format=hex>;
    // One of these bitflags might designate a deleted file, i.e. the ability for a patch bundle
    // to make the engine assume that a file from the original bundle doesn't exist anymore.
    // But since Fatshark doesn't use patch bundles for DT, we won't have examples for this.
    // The decompiled code would be the only place to figure this out.
    uint32 flags <format=binary>;
} FileMeta;

typedef struct {
    uint32 lang <format=binary>;
    byte is_data_file <comment="If 1, data chunk will be a file name in data/">;
    uint32 size;
    // May designate a deleted file (see `FileMeta.flags`). Though less likely because when the meta flags are used
    // this while header could be omitted.
    byte unknown_2;
    // If > 0, data chunk will be followed by a string of that length, which is an auxiliary/extension file
    // to the data in the data chunk.
    // Example: `.wwise_stream` only contains 12 bytes of data in the bundle, which includes an offset and a length.
    // The extra file then contains the actual data.
    uint32 len_data_file_name;

    if (unknown_2 != 1) {
        Warning("Expected '1' for 'unknown_2', got '%x'", unknown_2);
    }
} FileHeader;

typedef struct {
    uint64 extension <format=hex>;
    uint64 name <format=hex>;

    uint32 header_count;
    uint32 unknown_1;

    FileHeader headers[header_count];

    local int i = 0;
    for (i = 0; i < header_count; i++) {
        if (headers[i].is_data_file) {
            char data_file[headers[i].size];
        } else {
            byte data[headers[i].size];
            char data_file[headers[i].len_data_file_name];
        }
    }
} File;

uint32 version <format=hex>;
uint32 unknown_1;
Assert(unknown_1 == 3);

uint32 num_entries;
uint64 properties[32] <format=hex>;

FileMeta file_meta[num_entries];

uint32 num_chunks;
uint32 chunk_sizes[num_chunks];

local int padding_size = 16 - (FTell() % 16);
if (padding_size < 16) {
    byte padding[padding_size];
}

uint32 unpacked_size <bgcolor=0x0015FF>;
uint32 unknown_3;

File files[num_entries] <optimize=false>;
```

## Confirmed header constants (2026-10-12)

Across **3,000** shipped bundles (`bundle/<name>`, 200 B – 3 MB):

| Field | Value | Occurrence |
|-------|-------|------------|
| `version` | `0xF0000008` | 3,000 / 3,000 |
| `unknown_1` | `3` | 3,000 / 3,000 |
| `unknown_3` (the word after `unpacked_size`) | `0` | 3,000 / 3,000 |

So both "unknown" words are **constants**, not content-dependent — they carry no
information, and DTMT's writer already emits `0` for `unknown_3` while its reader
skips the word.

### `FileMeta.flags` is the external-data-file bit (2026-10-12)

The doc above speculated that one of the `FileMeta.flags` bits "might designate a
deleted file". It does not: over **3,000** shipped bundles / **67,165** file
entries the word takes exactly two values,

| Value | Entries |
|-------|---------|
| `0x00000004` | 37,138 |
| `0x00000000` | 30,027 |

and correlating it with the parsed entries shows what bit 2 means:

| `props` | `external` | data file name | Entries |
|---------|-----------|----------------|---------|
| `00000004` | true | present | 13,483 |
| `00000000` | false | present | 5,814 |
| `00000000` | false | none | 2,191 |

So **`flags & 4` = "this entry's data lives in an external `data/` file"** — it is
always set when the variant is external and never when it is not. A data-file name
*without* the flag is a **stream** reference (the resource's own body is inline and
the named file is auxiliary) — which is why a streamed texture is `external=false`
*and* carries a `data/xx/<16 hex>.stream` name.

`FileHeader.unknown_2` (always `1`) is the only header field left without a tested
meaning.