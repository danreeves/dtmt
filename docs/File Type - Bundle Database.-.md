**010Editor Template:**

```bt

uint32 format <format=hex>;
uint32 num_entries;

typedef struct {
    uint32 version;
    Assert(version == 0x4);

    uint32 len_name;
    char name[len_name];

    uint32 len_stream;
    char stream[len_stream];

    byte platform_specific; // boolean

    // byte buffer[20] <bgcolor=0x3399ff>;
    byte buffer[20];

    local int is_0 = true;
    local int i = 0;

    for (i = 0; i < 15; i++) {
        if (buffer[i] != 0) {
            is_0 = false;
        }
    }

    if (!is_0) {
        Warning("Unknown 20-byte buffer is not 0");
    }

    uint64 file_time; // win32 FILETIME
} File;

typedef struct {
    uint64 hash <format=hex>;
    uint32 num_files;

    File files[num_files] <optimize=false>;
} Entry;

Entry entries[num_entries] <optimize=false>;

uint32 num_hashes;

typedef struct {
    uint64 bundle <format=hex>;
    uint64 resource_hash <format=hex>;
} ResourceHash;

ResourceHash hashes[num_hashes];

if (format <= 5) {
    return;
}

uint32 num_contents;

typedef struct {
    uint64 extension <format=hex>;
    uint64 name <format=hex>;
} Filename;

typedef struct {
    uint64 hash <format=hex>;
    uint32 num_files;

    Filename files[num_files];
} BundleContents;

BundleContents contents[num_contents] <optimize=false>;
```

Algorithm to calculate resource hashes by WhiteGoat:
```cpp
uint64_t calc_hash(const std::vector<uint64_t>& list) {
    const uint64_t m = 0xc6a4a7935bd1e995ULL;
    const int r = 47;

    uint64_t k = 0;

    for (auto&& name : list) {
        uint64_t h = name;

        k *= m;
        k ^= k >> r;
        k *= m;
        
        h ^= k;
        k = m * h;
    }
    return k;
}
```
## What the shipped database actually holds (measured 2026-10-13)

`examples/db_probe.rs` reads the game's `bundle_database.data` and reports what DTMT parses. The reader agrees with the template above - every file-list key is also a bundle key, so the two sections line up - and the `contents` section turns out to be **partial**:

| | |
|---|---|
| bundles | **16,102** |
| bundles with a file list (`contents`) | **11,017** |
| bundles **without** a file list | **5,085** |
| file entries in the index | **957,805** |
| file-list keys not in the bundle list | 0 |

So about a third of the bundles have no file list at all, and `find-file` / `list-files` are only conclusive for the 11,017 that do. That is data, not a parsing fault - and it is why a name can look missing while the bundle itself is present:

```
content/ui/fonts/noto_sans_jp_black (1470e02569a61694) -> not in the file index
content/ui/fonts/mono_tide_regular   (780d2c1d6198922c) -> 9ba626afa44a3aa3
packages/ui/fonts/slug_ja            (e108d1bab1b6fd74) -> 9ba626afa44a3aa3
```

The CJK fonts' own bundle (`packages/ui/fonts/slug_ja`) is one of the 5,085 with no list, so its resources cannot be found there; the Latin fonts sit in `packages/boot`, which *is* listed. Both CLI commands now say which of the two cases they hit instead of reporting a flat "not found".