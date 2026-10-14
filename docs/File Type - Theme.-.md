# File Type - Theme

A `theme` (`0x38bb9442048a7fbd`) is a UI/environment theme. Darktide ships **191**, and the
reader decodes **96** of them; it is **deliberately not registered** in the bundle dispatch, because
it was an opaque blob before and registering a reader that fails on 95 of 191 would turn working
extractions into errors.

## The layout

```text
0                u32      how many words follow (8 in every shipped theme)
4                [u32]    those words; the last of them is the record count
4 + 4 * count    [N B]    the records: a u64 marker and a run of u32 values
after the records u32     carried: zero in `darkness`
                 u32      the length of the name run, terminators included
                 [u8]     NUL-separated names, ending on a terminator
```

Two things pin the shape:

- the **last header word is the record count** - 18 for `darkness`, and every theme where both
  readings agree;
- the **tail is a length-prefixed run of NUL-separated names**, and the run may hold several names
  or none.

Together they give the record width, `(records_end - header_end) / count`, which is **not** a
constant: `darkness` has 24-byte records.

## The tail, and why two readings failed first

The tail is a **name list, not one name**. The 6,483-byte theme ends with `u32 42` and then
`fog_backdrop\0darkness\0performance_armoury\0`, and `13 + 9 + 20 = 42` exactly. Two other files
end in **zeros**: a theme may carry *no* names at all, and those are record-less as well.

That explains both earlier attempts at once - a scan for printable bytes finds only the *last* name,
so the word before it is not that name's length; and a name-less theme has no printable run to find.
The run is now located by its **own declared length** and read as a list, and the word before that
length is carried. 69 of the 96 that read carry no names at all.

## Verified

`examples/theme_probe.rs` sweeps all 191: **96 byte-identical of 191**, binary and through SJSON -
up from 22 before the tail was understood.

## Open

The **record geometry** is unsettled. With the tail located correctly, the gap between the header
and the tail still fails to divide by the header word for some files - one theme has a 6,397-byte
gap over 6 records, and 6,397 is prime, so the header word cannot be a record count there. The
failures also show a theme carries **several** length-prefixed runs rather than one at the end:
one ends `toxic_gas\0OFF\0` `1e 00 00 00` `performance_cooling\0toxic_gas\0`, where `0x1e` is 30 and
`20 + 10 = 30` is exactly the last run.
