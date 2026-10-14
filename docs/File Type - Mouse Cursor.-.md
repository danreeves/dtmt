# File Type - Mouse Cursor

A `mouse_cursor` (`0xb277b11fe4a61d37`) is the engine's own pointer: a small bitmap with a
hotspot. The game sets it from Lua, by the window:

```lua
-- scripts/game_states/boot/boot_state_resolve_parameters.lua
Window.set_cursor("content/ui/textures/cursors/mouse_cursor_idle")
```

Darktide ships exactly **two**, which is why they come in a pair:

```
content/ui/textures/cursors/mouse_cursor_idle    -> bundle 9ba626afa44a3aa3
content/ui/textures/cursors/mouse_cursor_click   -> bundle 9ba626afa44a3aa3
```

Note that a mod does not have to override them: a cursor may be shipped under a **new** name and
the game pointed at it, which is what snoopymod does.

## The cooked resource

```text
0   u32   width
4   u32   height
8   u32   hotspot_x      9 in both shipped cursors
12  u32   hotspot_y      9 in both shipped cursors
16  u32   byte count     the pixel data's length
20  [u8]  the pixels, four bytes each, BGRA, bottom row first
```

`20 + byte count` is the whole resource, and that is the check the reader makes - exactly as
`wwise_dep` does with its name length. `byte count == width * height * 4` holds for both shipped
cursors and for every cooked one, but it is *not* enforced on read: two files cannot establish it
as an invariant, so the probe reports it instead.

The two middle words were carried as `unknown_8`/`unknown_12` until the community runtime
`darktide-simple-assets-runtime` named them: its `compileMouseCursor`
writes exactly `{width, height, hotspotX, hotspotY, byteCount}` over a PNG decoded **BGRA with the
bottom row first**, and `PrepareMouseCursor` registers the result as a `mouse_cursor`. The shipped
9,9 on a 40x40 cursor is a plausible arrow tip, so the names are not a guess.

There is no cooked-resource envelope: a shipped variant is the bare body, and no community tool
writes this type.

## The source

A source names a **texture**, and the pixels and the size both come from that texture's image. It
has no field for the pixels - they are only ever linked, never written out:

```text
texture = "textures/mods/snoopymod/cursor"
hotspot_x = 0
hotspot_y = 0
```

`hotspot_x`/`hotspot_y` default to zero when the source does not say. `width` and `height` may be
written as **assertions** - they must match the image, and the build refuses the cursor if they do
not - but the image is the truth, so they are normally left out.

`dtmt build` resolves the texture through its `.texture` source's `input.filename` to a **PNG**,
decodes it with the SDK's own PNG reader (see below), and writes the cooked layout above. A `.dds`
source is refused with a message rather than mis-read: the texture module parses DDS *headers* but
there is no DDS *pixel* decoder yet.

## PNG sources

`lib/sdk/src/png.rs` reads what a source image needs - 8-bit greyscale, RGB, greyscale+alpha, RGBA
and palette with `tRNS`, non-interlaced, and all five row filters - and refuses anything else by
name rather than decoding it wrongly. The inflation is `flate2`, which the texture reader already
used, so this costs no new dependency. Output is tightly packed RGBA.

## Converting a Windows `.cur`

A `.cur` is `{reserved, type = 2, count}` followed by `count` 16-byte directory entries, each
`{width, height, colours, reserved, hotspot_x, hotspot_y, bytes, offset}`, and then the images - a
32-bit BMP/DIB, or a PNG in newer files. The two things to carry over are the **hotspot** and the
size; the image becomes a PNG and the source names it.

## Verified

- the reader round-trips **2 of 2** shipped cursors, byte for byte, binary and through SJSON;
- a 32x32 PNG cooks to `20 + 32*32*4 = 4116`, and a test pins the row order and the channel order
  (`a_texture_backed_cursor_matches_the_written_pixels` became
  `a_texture_backed_cursor_writes_bgra_bottom_row_first` when the written-out form was removed);
- in game, `Window.set_cursor("content/ui/cursors/link_select")` renders that cursor, with the mod
  printing `cursor 'content/ui/cursors/link_select' set` and the pumpkin test passing.

## Not this resource: the pointer the UI *draws*

The in-game pointer is drawn by a UI element made of **materials** -
`constant_element_software_cursor_definitions.lua` names
`content/ui/materials/cursors/cursor_idle` and `cursor_click` - so a mod that wants the pointer
visibly changed overrides those two materials. This resource is the one the engine uses, and
`Window.set_cursor` is what selects it.
