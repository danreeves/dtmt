```010editor
uint32 header <hidden=true>;
uint32 count;

typedef struct {
    uint32 name_hash <format=hex>;
    uint32 address <format=hex>;
} Entry;

Entry entries[count];
```