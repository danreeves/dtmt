```0101editor
uint32 version;
uint32 num_entries;

typedef struct Entry {
    uint64 type <format=hex>;
    uint64 name <format=hex>;
};

Entry entries[num_entries];
```