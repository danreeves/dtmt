# Create your first mod

### tl;dr:

1. Open a Command Prompt/terminal (on Windows, I suggest installing [Git for Windows](https://gitforwindows.org/) and running that through [Windows Terminal](https://learn.microsoft.com/en-us/windows/terminal/install))
2. Run `dtmt new <path/to/new/folder>` and fill out the prompts
3. Edit the file `scripts/mods/<id>/init.lua`
4. Run `dtmt build`, which will produce some files in `out/`
5. Run `dtmt package`, which will produce a zip file
6. Import that zip file in DTMM and deploy it
7. Check [Continued Development](Building-your-first-mod#continued-development) below

---


In a Command Prompt/terminal, run `dtmt new <path/to/new/folder>` to create a skeleton structure from the default template.
During this, DTMT will prompt for several fields, including:
   
   - `name`: The human-readable name of your mod. This will show up in various places like DTMM's mod list or DMF's mod options view.
   - `id`: A unique ID for your mod. The default is generated from `name` and should be good enough in most cases (clicking `Enter` without typing accepts the default value). This will be used throughout your code (e.g. in `get_mod()` calls).

DTMT will now create the given directory from the mod template:

```
<path/to/new/folder>
└──<id>
   ├──.luacheckrc
   ├──dtmt.cfg
   ├──packages
   │  └──mods
   │     └──<id>.package
   └──scripts
      └──mods
         └──<id>
            ├──data.lua
            ├──init.lua
            └──localization.lua
```

You're now ready to start editing the `init.lua` file. For starters, you might want to just add a simple `print("Hello, world!")`, which will print this string in your console logs once you've loaded the mod.

With the file edited, you now need to turn your mod into something that DTMM can load into the game. To do that, navigate your terminal to your mod's folder (where the `dtmt.cfg` is) and run in sequence `dtmt build` and `dtmt package`. The first command will compile your mod files into game bundles, the second will package things into a single `<id>.zip` for distribution. Watch the output for any errors, but if all goes well, you can then import that `<id>.zip` fiel into DTMM and deploy.

# Continued development

After you've verified that your new mod has been loaded in the game, you will want to start the actual implementation of your mod. Since it would be rather tedious to  create a new zip file, import that in DTMM and deploy it for every change, there is one more command to learn: `dtmt watch --deploy <path/to/Warhammer 40k Darktide>`.

This will make DTMT continuously watch for changes in your mod's files, re-build the mod bundles and immediately deploy them to the given game directory. The path given should be the directory _above_ `bundle/`, e.g. `C:\Program Files\Steam\steamapps\common\Warhammer 40k Darktide`.

But there are two limitations, since DTMT cannot do a full deployment like DTMM:

- The mod has to be deployed once through DTMM first
- It only works as long as the list of files doesn't change. I.e. if you add a new file to your mod, or remove one, you will have to deploy that change through DTMM again, once

# Dev tools

### Lua

DTMT will already run a basic syntax check on your Lua files. For additional static analysis, install [luacheck](https://github.com/mpeterv/luacheck) and integrate it with your chosen editor.
[selene](https://github.com/Kampfkarren/selene) exists, too, and might work as an addition for its extra lints.