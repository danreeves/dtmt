> **NOTE**: If this is your first time migrating a mod, I strongly suggest going through [Building your first mod](Building-your-first-mod) once, to play around with and get a feel for the new file structure, as well as how to use DTMT to build the mod bundles.

Migration requires two steps:

1. Rework the project structure and create necessary files to package files into bundles
2. Adjust certain parts of your code

## 1. Rework project structure

### Automatically

For most projects, the built-in migration command should be enough to convert a mod to the DTMT project structure:

1. Determine a directory to migrate the mod to. The migration will create a new folder named after the mod here. In-place migrations are not supported.
   E.g.: `C:\projects`
2. Locate the `<mod_id>.mod` file for your mod.
3. In a Command Prompt (or similar terminal application, e.g. [Windows Terminal](https://learn.microsoft.com/en-us/windows/terminal/install)), execute `dtmt migrate C:\path\to\my_mod.id C:\projects`
4. Navigate to `C:\projects\<mod_id>` and execute `dtmt build` and `dtmt package`

### Manually

If the automated way doesn't work for you, or you want to dig in and see how it's done, you'll need the following:

- a `dtmt.cfg` that defines the mod's metadata, entry points and packages
- at least one `.package` file that lists your files to be bundled

One way to quickly generate a skeleton would be to use `dtmt new` in a new location as outlined in [Building your first mod](Building-your-first-mod), then move files over from your old location.

If you want to migrate manually, or need to edit things, read on:

To create your `dtmt.cfg` use the file reference at [`dtmt.cfg` Reference](dtmt.cfg-Reference).

The `resources` section can be adapted from the contents of your `.mod` file. The example uses `DMF` for the mod ID, but you'll want to use your own here, which must be the same value you pass to DMF's `new_mod` and `get_mod` in your code.

The `.package` file supports wildcards, so the most simple version would look like this:

```sjson
lua = [
    "scripts/mods/DMF/*"
]
```

A suggested final file structure looks as follows:

```none
<path/to/new/folder>
└──DMF
   ├──dtmt.cfg
   ├──packages
   │  └──mods
   │     └──DMF.package
   └──scripts
      └──mods
         └──DMF
            ├──data.lua
            ├──init.lua
            └──localization.lua
```

All additional Lua files can then be dropped into `scripts/mods/DMF` (sub-directories are supported) and can be `require`d in-game with the full path, e.g. `require("scripts/mods/DMF/my_extra_file")`.

Modding veterans from VT2 may recognize this file structure 🙂

## 2. Adjust code

- All script loading must happen via the regular Lua `require` or `dofile`. Functions like `mod:io_dofile` don't work anymore.
  You can default to using `mod:require` and `mod:dofile`, which switch internally to do "the right thing" for either bundled or non-bundled mods.
- `mod:add_require_path()` only works for non-bundled mods. Any scripts in bundled mods are already available via `require` (hence the point above)