**Legend:**
* *Complete*: The binary format is fully implemented/documented.
* *Partial*: Enough of the binary format is known to create usable results, but some things are still not known or not implemented.
* *Limited*: Some work has begun, but it's not enough for *Partial*, yet.
* *None*: Nothing about this binary format is known.

| File Type       | Binary Format | Compilation | Decompilation | Comment                                                                                                   |
|-----------------|---------------|-------------|---------------|-----------------------------------------------------------------------|
| Bundle          | Partial       | ✓           | ✓             | A few fields in file headers and their interaction with external `data/` files is unknown.
| Bundle Database | [Partial](File%20Type%20-%20Bundle%20Database.-.md) | ✓           | ✓             |
| Package         | [Complete](File%20Type%20-%20Package.-.md)      | ✓           | ✓             |
| Lua             | Complete      | ✓           | ✓             |
| Texture         | [Partial](File%20Type%20-%20Texture.-.md) | ✓ | ✓ | Wrapper, texture header and DDS layouts are known and a decompile -> compile round trip reproduces the original blob byte-for-byte. A few `flags` bits and the full set of `category` values are still unverified.
| Material | [Partial](File%20Type%20-%20Material.-.md) | ✓           | ✓             | Instance and base materials compile and round trip byte-for-byte. Base materials can carry custom shaders compiled from HLSL with `dxc`. `unk1`/`unk2`/`unk3` and most of the shader43 wrapper (contexts, conditions, group data, program reflection tails, default data) are preserved but not understood. |
| Strings         | [Partial](File%20Type%20-%20Strings.-.md)       | ✓           | ✓             | The file format itself is done, but the interaction with bundle properties and file variants is not properly implemented, yet. See https://git.sclu1034.dev/bitsquid_dt/dtmt/issues/3.
| Sound (Wwise)   | Limited          | ✗           | ✗             | Wwise does the heavy lifting anyways, so we "only" need to figure out the version and the post-processing. VT2's pipeline should work as a reference.<br>It also seems that banks are split up into smaller pieces with the new `.wwise_event` file type
| Wwise Event | [Partial](File%20Type%20-%20Wwise%20Event.-.md) | ✗ | ✗ |
| Wwise Stream | [Full](File%20Type%20-%20Wwise%20Stream.-.md) | ✗ | ✗ |
| Wwise Bank | [Partial](File%20Type%20-%20Wwise%20Bank.-.md) | ✗ | ✗ |
| Unit            | None          | ✗           | ✗             |
| Level           | None          | ✗           | ✗             |
| Particles       | None          | ✗           | ✗             |

Anything not mentioned explicitly is to be considered unknown.
