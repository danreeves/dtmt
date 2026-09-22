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