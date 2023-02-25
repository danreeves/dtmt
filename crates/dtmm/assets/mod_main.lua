print("[mod_main] Initializing mods...")
-- Keep a backup of certain system libraries before
-- Fatshark's code scrubs them.
-- The loader can then decide to pass them on to mods, or ignore them
local libs = {
    io = io,
    debug = debug,
    ffi = ffi,
    os = os,
    load = load,
    loadfile = loadfile,
    loadstring = loadstring,
}

require("scripts/main")
print("[mod_main] 'scripts/main' loaded")

require("scripts/game_states/boot/state_boot_sub_state_base")
local StateBootLoadMods = class("StateBootLoadMods", "StateBootSubStateBase")

StateBootLoadMods.on_enter = function (self, parent, params)
    print("[mod_main][StateBootLoadMods] Entered")
    StateBootLoadMods.super.on_enter(self, parent, params)

    local state_params = self:_state_params()
    local package_manager = state_params.package_manager

    self._state = "load_package"
    self._package_manager = package_manager
    self._package_handles = {
        ["packages/mods"] = package_manager:load("packages/mods", "StateBootLoadMods", nil),
        ["packages/dml"] = package_manager:load("packages/dml", "StateBootLoadMods", nil),
    }
end

StateBootLoadMods._state_update = function (self, dt)
    local state = self._state
    local package_manager = self._package_manager

    if state == "load_package" and package_manager:update() then
        print("[mod_main][StateBootLoadMods] Packages loaded, loading mods")
        self._state = "load_mods"
        local mod_loader = require("scripts/mods/dml/init")
        self._mod_loader = mod_loader

        local mod_data = require("scripts/mods/mod_data")
        mod_loader:init(mod_data, libs, self._parent.gui)
    elseif state == "load_mods" and self._mod_loader:update(dt) then
        print("[mod_main][StateBootLoadMods] Mods loaded, exiting")
        return true, false
    end

    return false, false
end

-- Patch `GameStateMachine.init` to add our own state for loading mods.
-- In the future, Fatshark might provide us with a dedicated way to do this.
local function patch_mod_loading_state()
print("[mod_main] Adding mod loading state")
    local GameStateMachine = require("scripts/foundation/utilities/game_state_machine")

    local GameStateMachine_init = GameStateMachine.init
    GameStateMachine.init = function(self, parent, start_state, params, ...)
        -- Hardcoded position after `StateRequireScripts`.
        -- We do want to wait until then, so that most of the game's core
        -- systems are at least loaded and can be hooked, even if they aren't
        -- running, yet.
        local pos = 4
        table.insert(params.states, pos, {
            StateBootLoadMods,
            {
                package_manager = params.package_manager,
            },
        })

        -- Clean up after us
        GameStateMachine.init = GameStateMachine_init

        return GameStateMachine_init(self, parent, start_state, params, ...)
    end
end

function init()
    patch_mod_loading_state()
    Main:init()
end
