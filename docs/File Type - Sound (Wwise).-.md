# File Type - Sound (Wwise)

Sound in Darktide is Wwise's, so "only" the version and the post-processing are Stingray's. Five
types are recognised:

| type | what it is | state |
|---|---|---|
| `wwise_bank` (`535a7bd3e650d799`, `.bnk`) | a Wwise bank | opaque |
| `wwise_dep` (`0xaf32095c82f2b070`) | the banks a package depends on | **read, written, round-tripped** |
| `wwise_event` (`0xaabdd317b58dfc8a`) | an *embedded* bank plus an event | wrapper unread |
| `wwise_metadata` (`0xd50a8b7e1c82b110`) | the events a bank provides | zero shipped instances |
| `wwise_stream` (`0x504b55235d21440e`, `.ogg`) | a streamed audio file | opaque |

Handling them as opaque blobs is already enough for a mod to replace a bank or a stream wholesale,
so this is a **decoding gap, not a shipping one**. See [Wwise Bank](File%20Type%20-%20Wwise%20Bank.-.md),
[Wwise Event](File%20Type%20-%20Wwise%20Event.-.md) and
[Wwise Stream](File%20Type%20-%20Wwise%20Stream.-.md).

## What is solved: `wwise_dep`

A dep is a three-field resource, derived from the two the boot bundle ships:

```text
0   u32   version    29 in every sample, so a constant rather than a size
4   u32   length     the bank path's length, including its NUL
8   [u8]  the path
```

`world_sound_fx.wwise_dep` is `29, 21, "wwise/world_sound_fx\0"` - exactly its 29 bytes - and
`Init.wwise_dep` is `29, 11, "wwise/Init\0"`, its 19. **One bank reference per file.**

## What was wrong about `wwise_event`

It is *not* a small engine structure. Probing 300 bundles found **606** of them and no metadata, and
every sample begins `1d 00 00 00` (version **29** again, a third confirmation of that constant)
followed by `42 4b 48 44` = `BKHD`, Wwise's bank-header chunk. So an event resource **embeds a
bank**: decoding it fully means decoding `.bnk`, which is the large proprietary part, and only the
Stingray wrapper around it is small.

## The VT2 source reference

A VT2 mod that ships all four shows what the *sources* are, where Darktide's are cooked
(`C:\dev\vmb\mods\CustomCursors\wwise\mods\CustomCursors\`):

| file | shape |
|---|---|
| `project.wwise_metadata` | text: `aux_buses = []`, `banks = { YoshiTongue = { … events = [ … ] } }` |
| `YoshiTongue.wwise_bank` | `u32 1`, `"win32"` padded, then `BKHD…` - a wrapper around the raw bank |
| `YoshiTongue.wwise_bank_metadata` | text: `events = [ "mlem" ]` |
| `YoshiTongue.wwise_dep` | text: `win32 = { banks = [ … ] }` |

That explains the three unresolved types at least as VT2 models them: a bank source is a small
wrapper around the raw bank, so the `.bnk` stays opaque and only the wrapper is Stingray's; a
bank-metadata source names the events a bank provides - the same fact Darktide's `wwise_event`
embeds as a whole bank, so the two games moved the same information into different types; and a dep
source is a per-platform bank list, cooked into the `{version, length, path}` form Darktide ships,
which is why a shipped dep carries exactly one path.

## Next

1. read the Darktide `wwise_event` wrapper around its embedded `BKHD` - the header between the
   version word and the chunk - and see whether it is the wrapper VT2's bank source carries;
2. decode `wwise_metadata` (a shipped type with zero instances, so this VT2 shape is its only
   reference);
3. leave the `.bnk` and the `.ogg` opaque, as Wwise's own.
