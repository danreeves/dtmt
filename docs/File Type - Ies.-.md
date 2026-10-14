# File Type - Ies

An `ies` (`0x8f7d5a2c0f967655`) is a baked photometric distribution - the falloff a light uses.
Despite the type name it is **not** an IESNA LM-63 text file.

**1,297 of 1,297** shipped profiles re-encode byte for byte, binary and through SJSON.

## The layout

```text
0   u32   version     5 in all 1,297 shipped profiles
4   f32   scale       the one header field that varies
8   u32   unknown_8   16 in all 1,297, and the value count / 8
12  u32   unknown_12  256 in all 1,297, and twice the value count
16  [f32] the values, 128 of them, in a 528-byte profile
```

Every shipped profile is **exactly 528 bytes** and holds **128** values, and the curve falls
monotonically from its first value to its last, which is the shape of a candela-versus-angle table.
`version`, `unknown_8` and `unknown_12` are identical in all 1,297 - 5, 16 and 256 - while `scale`
is the field that actually varies.

The two count words are **carried** rather than derived or recomputed. With a single size in the
corpus, a derived value and a constant are indistinguishable, and writing a constant would be the
"read a field, drop it, write a default" fault this code base spends its time removing. They are
also consistent with the values either way: `128 / 8 = 16` and `2 * 128 = 256`.

## Envelope

No community tool writes this type, so unlike `bones` and `animation` the cooked-resource envelope
is not accepted: a shipped variant is the bare body.

## Open

What the curve's angle range is, and what the two count words mean when a profile is not 128 values
wide - both need a corpus with more than one size, which the game does not ship.
