## General

Sounds, both sound effects and music, seem to be split across multiple file types. So far, we know about three:

- [`.wwise_bank`](File+Type+-+Wwise+Bank.-): A full Wwise Soundbank that contains an arbitrary amount of sound and music events, as well as all the extra data to use them
- [`.wwise_event`](File+Type+-+Wwise+Event.-): A small soundbank that contains only a single HIRC event, though it may contain multiple `.wem` files.
- [`.wwise_stream`](File+Type+-+Wwise+Stream.-): A marker pointing to a file in `data/` containing a single, raw `.wem` file (based on the `RIFF` header).

The relationship between these is still unknown.
There are only two `.wwise_bank` files in total, so these must be setting up the entire sound environment for the game. I would expect to find some form of link/pointer from these banks to the `.wwise_event` and `.wwise_stream` files.

## Decompilation

Decompilation should be possible with the existing information. The Wwise formats are well known across modding scenes in other games, and VT2's unpacker can serve as prior art as well.

## Compilation

Until the relationship between the different file types is figured out, we will be limited. But what should already be possible is to generate `.wem` files and insert them into existing files, which would allow replacing existing sounds. Though it would require a lot of manual effort to map files to the in-game sound events.

As such, a tool would need to be written that can read a community-maintained mapping list from sound event names to murmur hashes of file names and can use that to replace `.wem` files in binary.