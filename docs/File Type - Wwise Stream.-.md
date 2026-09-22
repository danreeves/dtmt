Seems to be just a pointer into a corresponding `.stream` file in `data/`.

A file header will look like this
```
uint32 lang = 0x0;
byte external = 0x0;
uint32 size = 12;
byte unknown = 1;
uint32 len_data_file_name = 31;
```

`extra_file` will then contain the path of the `.stream` file.

```101editor
//------------------------------------------------
//--- 010 Editor v12.0.1 Binary Template
//
//      File: dt_wwise_stream.bt
//   Authors: Lucas Schwiderski
//   Version: 
//   Purpose: 
//  Category: Darktide
// File Mask: *.wwise_stream
//  ID Bytes: 
//   History: 
//------------------------------------------------

// Could be the version, but 27 different versions of a file that only contains an offset seems unlikely
uint32 unknown1;
Assert(unknown1 == 0x1b);

// Offset and length of the section within the `.stream` file.
// Since all `.wwise_stream` files seem to get their own `.stream` file, `offset` always ends up being `0`,
// and `length` always ends up matching the `.stream` file's size.
uint32 offset;
uint32 length;
```

---

The `.stream` file contains a raw RIFF container, no extraneous fields. However, while the contained data is WAVE as per the type field, it uses an audio format that even ffmpeg cannot understand.
The `wFormatTag` of the `struct WAVEFORMATEX` is set to `0xFFFF` which indicates an experimental/in-development format that has not been standardized or registered with Microsoft, yet.

[ww2ogg](https://github.com/hcs64/ww2ogg) can be used to convert these into a usable format.