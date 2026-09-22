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