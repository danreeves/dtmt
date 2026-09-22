The file format is SJSON (like most text files concerning DTMM, DTMT and the game engine are/will be), which is a modified version of JSON.

The gist is:
- comments (line: `//`, block `/* */`)
- strings and object keys only need quotes when they include certain characters (or when they're empty)
- array and object values can be separated by newline instead of comma (or mixed)
- the top level value of a file must be an object, without the outer braces

See [Autodesk Stingray: About the SJSON data format](https://help.autodesk.com/view/Stingray/ENU/?guid=__stingray_help_managing_content_sjson_html) and [github.com/Autodesk/sjson](https://github.com/Autodesk/sjson) for more.

## Example

```sjson
// The unique mod ID. Both DTMM and DMF will run into issues if this overlaps with another mod
id = DMF

// The human-readable name of the mod. This will show up in places like DTMM's mod list and DMF's options view
name = "Darktide Mod Framework"

// A one- or two-sentence summary of what your mod does. Shows up in DTMM
summary = "An open-source, community-run framework that provides enhanced Darktide modding support."

// A version number to track changes in your mod with. Shows up in DTMM
version = "2023-03-12"

// Name of the individual or group that made this mod. Shows up in DTMM
author = "Darktide Modders"

// A file path with extension to an image to show in DTMM. Recommended width: 500 - 1000 px
image = "assets/icon.png"

// Needs to be set to `false` explicitly for mods that don't use `dtmt` to build.
// When `false`, the `packages` field will be ignored.
bundled = false

// A list of arbitrary strings that will show up in DTMM.
// Grouping, filtering and other features may be implemented in the future, so you'll want to match up with other mods
categories = [
    Tools
]

// A list of mod IDs that this mod depends on. DTMM will verify this before deployment
depends = [
    // Just the ID defaults to "before"
    foo
    // Long form of the above
    {
    	id = foo
        order = before
    }
    // Dependencies may be required to come after the current mod
    {
    	id = baz
        order = after
    }
]

// File paths to Lua files (without extension), as passed in the second parameter of DMF's `new_mod`
resources = {
    // The Lua file to be called by DMF when it initializes your mod.
    init = "scripts/mods/DMF/init"
    // (Optional) The Lua file returning the mod's DMF configuration.
    data = "scripts/mods/DMF/data"
    // (Optional) The Lua file returning localization strings to be registered with DMF.
    localization = "scripts/mods/DMF/localization"
}

// Will be skipped when `bundled = false`
packages = [
    // At least one package file (without extension) to load your files.
    // It doesn't have to be named after your mod's ID value, but it is good practice to do so for the "main" package of your mod.
    "packages/mods/DMF"
]
```

**Note:** Unless otherwise noted file paths must be specified _without_ file extension.