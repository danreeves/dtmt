**WIP**

For the time being, here is a dump of a conversation in the Modders Discord:


> As for workflow:
> 
> Conceptually Stingray's serialization is relatively simple: The structs (or C++ classes, doesn't matter much) are read or written simply by their memory layout. I.e. a `Vector` class with `length` and `data` members would be written out as one unsigned 32-bit integer, directly followed by `length` times of whatever type the element is. Could be simple numbers, could be entire structs.
> As such, reverse engineering the binary formats essentially boils down to figuring out these struct definitions.
> 
> There are several avenues to, ideally, follow at the same time, primarily "staring at hex" and "staring at C-ish".
> 
> The "hex" part would be a Hex editor to look at the actual binary format on disk, try to find patterns, discern individual fields, and hopefully figure out what each field is used for.
> My current editor of choice is 010 Editor (https://www.sweetscape.com/010editor/), as its _Binary Templates_ feature is well suited for this.
> 
> To get data to analyze, DTMT (https://git.sclu1034.dev/bitsquid_dt/-/packages/generic/dtmt/dtmm-v1.0.0-rc3-99-g7a1727f) can both list bundle contents (so you can figure out which bundles to extract from) and extract individual files in their binary format. Generally, I would look for one or more bundles that contain files of the type I'm looking at, extract them, and load them in the editor.
> You'll want both files that vary only a little or not at all in size, which aids in spotting small differences in individual fields, and also files that vary a lot in size, which aids in spotting differences in over all structure (e.g. the smaller files might contain empty arrays). The "File Diff" feature of 010 also helps with the former.
> I would then start writing a template file, assigned to all files of that type and slowly build up the struct definition(s). Though field names would usually just start out as `unknown1`, `unknown2`, ...
> 
> Stingray does have certain commonly used structures, a non-exhaustive list off the top of my head:
> - unsigned 32-bit integers: commonly spotted by trailing zeroes. often a length, index or offset for a collection type
> - murmur hashes: 8 (or sometimes 4) bytes of complete gibberish
> - vector/array: an u32 `length` followed by a data stream of `length` elements
> - hash map: an u32 `length` followed by two consecutive data streams of `length` elements, the first being the keys, the second being the values. The keys often are murmur hashes
> - string: can be a C-style `0`-terminated string, i.e. readable ASCII, followed by a `00`, or a u32 `length` followed by that many ASCII characters
> - fixed width containers: especially strings or arrays could also appear as just a sequence of bytes/elements without a `length` value, if the engine uses a fixed, per-determined length. A sign for this would be a section that always has the same overall size, but varying amounts of `0` at the end
> - raw binary: many file types (e.g. textures, materials or sounds) will include "external" binary, i.e. data that is produced by library rather than the engine itself, such as Wwise sound banks.
> 
> In tandem, the "C-ish" part should be looked at as well, which is loading the game's executable in a tool like IDA, which can decompile that into a C-like syntax, and within that attempt to locate functions that operate on the struct(s) corresponding to what file type we're looking at. A starting point is usually found by just searching for the file type as string, e.g. `unit` and `.unit`.
> But this requires a lot of somewhat unrelated work, as in order to make sense of the decompiled code and to navigate through, you actually want to figure out names and struct layouts for as much as possible, not just the individual thing you're ultimately looking for.
> 
> I guess one good first step by someone wanting to help out could also just be coordinating collaboration. Between my stuff, limn and the handful things we have in VT2, there is very little shared effort so far. Which at best just means duplicate work, at worst could mean one person has knowledge that someone else is stuck on before a breakthrough.


## Useful links

- 010 Editor: https://www.sweetscape.com/
- IDA Free: https://hex-rays.com/ida-free/
- XeNTaX Wiki kept alive at https://forum.xen-tax.com
- UnknownCheats: https://www.unknowncheats.me
- https://github.com/tylerha97/awesome-reversing

## Shader in-game test scripts

`scripts/` next to this page holds the PowerShell used to deploy a built
material and read the result on screen. The game path comes from the Steam
install (`launch.bat` under `Warhammer 40,000 DARKTIDE`); everything else is a
parameter or an environment default, so no machine-specific path is baked in.

- `slice-sections.ps1` - slices a material data file's `shader43` section into
  `header/contexts/conditions/dependencies/group/device/default` `.bin` files,
  which is how the test fixtures are extracted.
- `set-shader.ps1` - writes a compiled section's `shader_size` and `shader_data`
  back into a material SJSON.
- `title-tint-demo.ps1` - the end-to-end harness: launch the game, wait for the
  title material line in the newest console log, screenshot and average a screen
  region a few times, then grep the log for the mod.
- `capture-title.ps1`, `shot-window.ps1` - title-screen captures;
  `shot-window.ps1` uses `PrintWindow`, so a borderless-fullscreen window is
  captured rather than the desktop.
- `lua-tint-demo.ps1`, `restore_title_material.ps1` - drive a material from Lua
  and restore the original title material afterwards.