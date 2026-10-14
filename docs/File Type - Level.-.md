# File Type - Level

A `level` (`0x2a690fd348fe9ac5`) is a level. Darktide ships **932**, and **nothing** about it is
decoded: it extracts as an opaque blob, which round-trips.

## What is known

- the resource begins with a header and then an **offset table into the file**, so a reader would be
  a container of sections addressed by that table rather than a flat structure;
- the reference for this type is a **stub**: `bitsquid-blender-tools`' `stingray/level/__init__.py`
  says "Mostly unknown" and does not decode it;
- the level is where the media/prop references live, so a decoded one would be the natural home for
  "which units and materials does this level place", which is why it stays on the list rather than
  being closed as not-worth-it.

## Why it is not derived

Every other type in this project was derived from a reference that *decodes* it, and this one has
none. Deriving it blind would mean guessing at a section table whose sections are themselves of
unknown types, and the failure mode of a wrong guess here is a level that loads and then behaves
differently rather than one that errors.

## Next

Needs a real reference - a decompiled level loader, or a shipped level with its sources - before a
reader is worth attempting. Until then the blob is carried, which is honest and round-trips.
