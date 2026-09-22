Darktide ships with a `.dll` for Oodle 2.8, and DTMT could link to that at runtime. However, that creates two problems:

* It's only available on Windows. Linux users would have to run everything through Wine.
* DTMT needs to know the path to the library file. It's hard to make automatic detection reliable, and it's confusing for people to specify it manually.

Therefore, linking the libraries statically is the better option, but requires additional files on both systems.
To compile for Windows (native or cross), a `oo2core_win64.lib` file is needed. Compiling for Linux requires an `liboo2corelinux64.a` archive file instead.

## Generating a `.lib` file

A `.lib` file can be generated from a `.dll` file. The MSVC command line tools (from the Visual Studio installer) are required. Their tools can usually be found at `C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\<some_version>\bin`.

1. `dumpbin /EXPORTS oo2core_9_win64.dll > oo2core_win64.def`
2. Add a line `EXPORTS` at the top of the `.def` file
3. (Optional) Remove unused function definitions
4. `lib /def:oo2core_win64.def /machine:x64 /out:oo2core_win64.lib`

## Acquiring a `.lib` or `.a` file from Unreal Engine

The Unreal Engine source code (currently v5.0.3) includes a version of the Oodle data compression library (at `Engine/Source/Runtime/OodleDataCompression/Sdks/2.9.5`). While this is a never version than the one Darktide uses, they are ABI compatible. Both `.lib` and `.a` files can be found here.