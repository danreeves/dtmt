Mods = {
    -- Keep a backup of certain system libraries before
    -- Fatshark's code scrubs them.
    -- The metatable setup prevents mods from overwriting them.
    lua = setmetatable({}, {
        __index = { io = io, debug = debug, ffi = ffi, os = os },
    }),
}

require("scripts/game_states/boot/state_boot_sub_state_base")
local StateBootLoadMods = class("StateBootLoadMods", "StateBootSubStateBase")

StateBootLoadMods.on_enter = function (self, parent, params)
    StateBootLoadMods.super.on_enter(self, parent, params)

    local state_params = self:_state_params()
    local package_manager = state_params.package_manager
    self._package_manager = package_manager
    self._package_handles = {
        ["packages/mods"] = package_manager:load("packages/mods", "StateBootLoadMods", nil),
        ["packages/dmf"] = package_manager:load("packages/dmf", "StateBootLoadMods", nil),
    }
end

StateBootLoadMods._state_update = function (self, dt)
    local state = self._state
    local package_manager = self._package_manager

    if state == "load_package" and package_manager:update() then
        self._state = "load_mods"
        local dmf_loader = require("scripts/mods/dmf/dmf_loader")
        self._dmf_loader = dmf_loader

        local mod_data = require("scripts/mods/mod_data")
        dmf_loader:init(self._parent.gui, mod_data)
    elseif state == "load_mods" and self._dmf_loader:update(dt) then
        return true, false
    end

    return false, false
end

require("scripts/main")

-- Patch `GameStateMachine.init` to add our own state for loading mods.
-- In the future, Fatshark might provide us with a dedicated way to do this.
local function patch_mod_loading_state()
    local GameStateMachine = require("scripts/foundations/utilities/game_state_machine")

    local GameStateMachine_init = GameStateMachine.init
    GameStateMachine.init = function(self, parent, start_state, params, ...)
        -- Hardcoded position after `StateRequireScripts`.
        -- We do want to wait until then, so that most of the game's core
        -- systems are at least loaded and can be hooked, even if they aren't
        -- running, yet.
        local pos = 4
        table.insert(params.state, pos, {
            StateBootLoadMods,
            {
                package_manager = params.package_manager,
            },
        })

        return GameStateMachine_init(self, parent, start_state, params, ...)
    end
end

function init()
    Main.init()
    patch_mod_loading_state()
end
