# File Type - Font (`slug`)

A font in Darktide is a **`slug` bundle entry** (`0xa27b4d04a9ba6f9e`), compiled by the commercial
[Slug library](https://sluglibrary.com/) - not a material and not a texture. The game ships **14**
in `packages/boot` (`content/ui/fonts/*` plus `core/performance_hud/debug`) and 8 more in
`packages/ui/fonts/slug_{ja,ko,zh_cn,zh_tw}`.

## The layout

```text
0   4CC   "guls"
4   u32   version     0x0700 shipped, 0x0760 in the current toolchain
8   4CC   kind        "fnt" for a font, "alb" for an album
12  [16]  16 zero bytes
0x20 4 x {4CC tag, u32 base, u32 count}    the index: HLF4 (kerning) and 16U2 (glyphs)
     then the tables, laid out consecutively from each record's base:
       HLF4: 16-byte {u32, f32, u32, f32} kerning pairs
       16U2: 64-byte glyph records - a box (four f32), a u32, a packed pair,
             two advances (f32) and further words
```

The glyph table's base differs between the two versions: the record's base plus this file's `HLF4`
table for `0x0760`, and a strided scan finds it for `0x0700`.

The `16U2` table interleaves two kinds of record, and the `u32` in the middle is a **codepoint**:
even indices carry a full-height box (`0, 0, w, 1`) and a *float* in the `u32` slot (`0.89` as
`f32`), while odd indices carry a real glyph box and a small ascending number - `140, 229, 278,
388, 670, 1040, 1291, 1315, 1340, 1365` … So the odd records are glyphs with their codepoints, the
even ones are markers (the tall box is the line height), and the discriminator is `word2 == 0`
versus non-zero. That halves the count: `16U2`'s 425 records are about 212 glyphs and 213 markers.

## A custom font under a **new** name

Read from the decompiled tree (`C:\dev\Darktide-Source-Code`), not guessed:

- `scripts/managers/ui/ui_font_settings.lua` gives every text style a `font_type` (a font name), a
  `font_size` and often a `material` - the gradient the glyphs are filled with;
- `scripts/managers/ui/ui_fonts_definitions.lua` is the registry: `base_path =
  "content/ui/fonts/"`, `custom_font = "darktide_custom_regular"`, the `fonts` table and
  `locale_specific_fonts`;
- `scripts/managers/ui/ui_font_manager.lua` (`Managers.font`) builds per name a `path =
  {base_path..name, <locale fallback>, base_path..custom_font}` - **a fallback chain**, so a name
  whose resource is missing renders silently in `darktide_custom_regular`;
- `scripts/managers/ui/ui_fonts.lua` reads `style.font_type` live; its `font_options_by_style`
  cache keys only alignment, not the font;
- the title screen's text style is `table.clone(UIFontSettings.body)`
  (`title_view_definitions.lua`) and the widget is `title_text`.

So four things, in order:

1. the slug at its own path - `content/ui/fonts/jokerman.slug`, no rename and no `name_overrides` -
   listed as `slug = [...]`;
2. the name added to `FontDefinitions.fonts`;
3. the manager's definitions **rebuilt**: they are built once in `init`, before a mod loads, so
   `Managers.font:data_by_type(name)` is nil until `_setup_font_definitions` runs again;
4. a style pointed at it - and this is the part that costs attempts: the draw goes through the
   widget's **passes**, so the font belongs on the pass' style (`widget.style[pass.style_id]`), not
   on `widget.style` alone. Same place the background material goes.

Verified in game: the title screen's `Press [SPACE] to continue` renders in Jokerman, with
`PUMPKIN_OK` and the font deployed - see `docs/evidence/custom-font-title-jokerman.png`.

## Shipping a font needs no writer

The user compiles the font with the Slug library's own tool (`slugfont.exe`, or its DLL), and
`dtmt build` **packs the resulting `.slug` verbatim** - a font is binary, not SJSON, so it has a
byte pass-through beside the Unit/Bones branches. Verified end to end: a compiled font lands in the
built bundle byte-identical and inline, exactly the shape of a shipped font.

Deployment needs the bundle database to map the resource name to the mod's bundle, which
`dtmm --deploy` does and `dtmt build --deploy` does not. Check with
`dtmt bundle db find-file <game>/bundle/bundle_database.data <path>`.

## Related

[Slug Album](File%20Type%20-%20Slug%20Album.-.md) is the same `guls` container with the kind `alb`.

## Open

The album payload is unread (see that page). The reader is read-only: `slugfont.exe` is the writer,
so nothing here needs to re-encode a font.
