# File Type - Slug Album

A `slug_album` (`0xe9fc9ea7042e5ec0`) is the **same `guls` container as a font** with the kind `alb`
instead of `fnt`. Darktide ships **11**.

The type routes through the same reader as `slug`, and the reader index of four records is
**surfaced** (`SlugFont.records`) rather than read and dropped - which is what distinguishes an
album from a font.

## What is settled

Every shipped album carries the same four records, in the same order:

```text
HLF4   COMP   16U2   COMP
```

- the magic, the version `0x0700` and the four records at `0x20` are identical to a font's;
- the last `COMP` region **ends exactly on the file size** across all six distinct sizes - 576, 676,
  848, 868, 1,716 and 10,784 bytes;
- the field order of the two `COMP` records is **`{length, offset}`**, settled by hexdumping the
  smallest album outright rather than by a bundle lookup (the album hashes are not reliable through
  `bundle db find-file` for this type);
- the second word of the second `COMP` record is **320 in all 11 albums**, so it is a constant
  rather than a per-file length.

Reading a record as `{offset, count}` instead makes the two `COMP` regions *overlap* (e.g.
`[5072, 5392)` and `[5344, 10784)` in the 10,784-byte album), which is what rules that order out.

## Verified

All 25 `slug` and all 11 `slug_album` entries parse, and the surfaced records match the raw bytes
exactly (`examples/slug_album_probe.rs`).

## Open

The album **payload** is not read:

- its `16U2` count is `1`, so the glyph-table heuristic (four plausible records in a row) never
  locates a table and no glyphs are reported;
- the `COMP` regions are not decompressed - they are the payload, and they remain unread.
