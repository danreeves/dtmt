# File Type - Animation

An `animation` (`0x931e336d7646cc26`) is a cooked skeletal clip: a header, a **sync table** - one
rest pose per joint of the rig the clip animates - and then a flat stream of items, terminated by a
footer that must land on the very last byte.

**110 of 110** shipped clips re-encode byte for byte, binary and through the SJSON a build uses,
and one was shipped through the mod and loaded in game.

## The layout

```text
0       u32   unknown_0
4       u32   unknown_4
8       u32   joint count      the sync table is exactly count * 40 bytes
12      f32   duration
16      u32   body size        the resource's own length
20      u32   beat count N
24      N x (f32 time_slice, f32 quantisation)
24+8N   u16   sync marker
26+8N   joints * 40 bytes:     f32 position[3], f32 rotation[4], f32 scale[3]
        then items until a `u16 3` footer that must end the body
```

The joint count is a real cross-resource link, not a version word: for the first-person rig it is
**146**, which is the joint count of the sibling `base.bones`. It was first read as a format
version, and the correction came from decoding that sibling.

## The item stream

An item opens with a `u16`, whose top two bits choose the encoding:

```text
0x0000 | kind 0..7   unpacked: u16 bone, f32 time, then 3 or 4 f32
0xC000               packed:   see below
```

The unpacked kinds are `2` trigger, `3` footer, `4` translation, `5` rotation, `6` scale and `7`
the sync marker. The packed kinds are `0x8000` translation, `0xC000` rotation and `0x4000` scale.

A **packed** item fuses its type, joint and time into one `u32` -
`type (2 bits) | bone (10 bits) | milliseconds (20 bits)` - and then stores quantised values:
three `u16` for a translation or scale spanning `-10 .. 10` metres, and four `u16` for a rotation
spanning `-pi .. pi` before normalisation.

That fusion is what made the stream look broken. Reading `idle` as if every item were unpacked
works for 147 scale keys and then reads `0xC000`, which is not a kind at all - it is the rotation
item's type bits, with the joint and the time packed into the same word. The game's clips use the
packed items for animated channels and the unpacked form for static ones.

## Grounding

The header, the sync table and the unpacked item layout are also what the community's cooked writer
emits and validates (`C:\dev\Darktide-Asset-Compiler`,
`animation/animation_{writer,evaluator}.cpp`), and the packed item types and quantisers come from
`bitsquid-blender-tools` (`bitsquid/stingray/animation.py`, `bitsquid/deserializing.py`), which
reads shipped Darktide clips. So the grammar is those two readers' grammar, not an inference from
samples.

## Open

The sync table is the unpacked one in every shipped clip (marker `7`). A **packed** sync
(marker `1`) is rejected rather than mis-read, since it cannot round-trip through decoded values -
no shipped clip uses it, so there is nothing to derive it against.

`unknown_0` and `unknown_4` are carried; the reference does not name them.
