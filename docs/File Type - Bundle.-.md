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