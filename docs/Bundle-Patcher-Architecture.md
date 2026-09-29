This is a (likely incomplete) documentation of how dtmm's process to deploy mods and patch game bundles works and what data it writes.

## settings.ini

The game's `application_settings/settings-common.ini` is changed with

```diff
-boot_script = "scripts/main"
+boot_script = "scripts/mod_main"
```

This allows us to run our own entry point before the game's code, without touching the actual `scripts/main` file.

## packages/dml

An independent mod that's handled specially. Its purpose is to handle the initial mod loading, what used to be `ModManager` in VT2.
`scripts/mod_main` expects this mod to return an API based on this template:

```lua
local obj = {}

-- @param boot_gui Gui A special Gui object created by the game that can be used to display status on
-- @param mod_data table The contents of `scripts/mods/mod_data.lua` as described below.
-- @param libs table A key-value map of certain Lua libraries/globals that the game strips, but that might be useful during the loading process
function obj:init(boot_gui, mod_data, libs)
end

-- @param dt number The delta time since the last call
function obj:update(dt)
	return is_done
end

return obj
```

## packages/boot

Several files are added to `packages/boot`:

* `scripts/mod_main.lua`: The very first file of code that the game will run at startup. This calls Fatshark's actual `scripts/main` entry point and injects `packages/dml` into the boot process.
* `scripts/mods/mod_data.lua`: This file mod metadata and the load order, all of which the mod manager will use to load the installed mods.
* `packages/mods.package`: The `.package` file that points to the separate mod collection bundle
* `packages/dml.package`: See above

**Example for `scripts/mods/mod_data_lua`:**

```lua
return {
  {
    name = "Darktide Mod Framework",
    id = "dmf",
    run = function()
      return dofile("scripts/mods/dmf/dmf_loader")
    end,
  },
  {
    name = "Test Mod",
    id = "test-mod",
    run = function()
      return new_mod("test-mod", {
        script = "scripts/mods/test-mod/init",
        data = "scripts/mods/test-mod/data",
        localization = "scripts/mods/test-mod/localization",
      })
    end,
  },
}
```

## packages/mods

This bundle serves as the root package from which all mod packages can be reached. Collecting them here avoids bloating `packages/boot` too much.

## settings_common.ini and the client version

`bundle/application_settings/settings_common.ini` is the file the deployment
patches for the boot script (`boot_script = "scripts/mod_main"`). It also
carries the **client version** the game reports to Fatshark's backend:

``
script_data = {
	content_revision = "138030"
	crashify = { branch = "default"  project = "darktide" }
	game_revision = "138030"
	game_version = "1.13.0-b802981"
	teamcity_build_id = "802981"
}
``

The title screen logs `Checking game version for win32 - <game_version>(<game_revision>)`
and the backend's `/game-version/status` answers "did not match" when that is
not the build it expects; the client then raises `GameVersionError`
(`VERSION_ERROR`) and sign-in fails.

DTMM reads the live file (falling back to its backup only when the file cannot
be read) and rewrites only the `boot_script` line, so it does not write a
stale version itself. But because the file is locally modified, a game update
can leave the previous build's `script_data` behind - the practical symptom
being "your game is out of date" right after an update. Steam's **Verify
integrity of game files** restores the current file (it reports exactly that
file as the corrupt chunk), after which the mod can be deployed again.