**Legend:**
* *Complete*: The binary format is fully implemented/documented.
* *Partial*: Enough of the binary format is known to create usable results, but some things are still not known or not implemented.
* *Limited*: Some work has begun, but it's not enough for *Partial*, yet.
* *None*: Nothing about this binary format is known.

| File Type       | Binary Format | Compilation | Decompilation | Comment                                                                                                   |
|-----------------|---------------|-------------|---------------|-----------------------------------------------------------------------|
| Bundle          | Partial       | ✓           | ✓             | A few fields in file headers and their interaction with external `data/` files is unknown.
| Bundle Database | [Partial](File+Type+-+Bundle+Database.-) | ✓           | ✓             |
| Package         | [Complete](File+Type+-+Package.-)      | ✓           | ✓             |
| Lua             | Complete      | ✓           | ✓             |
| Texture         | [Partial](File+Type+-+Texture.-) | [WIP](https://git.sclu1034.dev/bitsquid_dt/dtmt/issues/2) | [WIP](https://git.sclu1034.dev/bitsquid_dt/dtmt/issues/2) | While we do know the layout of the fields, we don't fully know what each of them does
| Material | [Limited](File+Type+-+Material.-) | ✗           | ✗      |
| Strings         | [Partial](File+Type+-+Strings.-)       | ✓           | ✓             | The file format itself is done, but the interaction with bundle properties and file variants is not properly implemented, yet. See https://git.sclu1034.dev/bitsquid_dt/dtmt/issues/3.
| Sound (Wwise)   | Limited          | ✗           | ✗             | Wwise does the heavy lifting anyways, so we "only" need to figure out the version and the post-processing. VT2's pipeline should work as a reference.<br>It also seems that banks are split up into smaller pieces with the new `.wwise_event` file type
| Wwise Event | [Partial](File+Type+-+Wwise+Event.-) | ✗ | ✗ |
| Wwise Stream | [Full](File+Type+-+Wwise+Stream.-) | ✗ | ✗ |
| Wwise Bank | [Partial](File+Type+-+Wwise+Bank.-) | ✗ | ✗ |
| Unit            | None          | ✗           | ✗             |
| Level           | None          | ✗           | ✗             |
| Particles       | None          | ✗           | ✗             |

Anything not mentioned explicitly is to be considered unknown.