local _G = _G
local rawget = rawget
local rawset = rawset

local log = function(category, format, ...)
    local Log = rawget(_G, "Log")
    if Log then
        Log.info(category, format, ...)
    else
        print(string.format("[%s] %s", category or "", string.format(format or "", ...)))
    end
end

-- Patch `GameStateMachine.init` to add our own state for loading mods.
-- In the future, Fatshark might provide us with a dedicated way to do this.
local function patch_mod_loading_state()
    local StateBootSubStateBase = require("scripts/game_states/boot/state_boot_sub_state_base")

    -- A necessary override.
    -- The original does not proxy `dt` to `_state_update`, but we need that.
    StateBootSubStateBase.update = function (self, dt)
        local done, error = self:_state_update(dt)
        local params = self._params

        if error then
            return StateError, { error }
        elseif done then
            local next_index = params.sub_state_index + 1
            params.sub_state_index = next_index
            local next_state_data = params.states[next_index]

            if next_state_data then
                return next_state_data[1], self._params
            else
                self._parent:sub_states_done()
            end
        end
    end

    local StateBootLoadMods = class("StateBootLoadMods", "StateBootSubStateBase")

    StateBootLoadMods.on_enter = function (self, parent, params)
        log("StateBootLoadMods", "Entered")
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
            log("StateBootLoadMods", "Packages loaded, loading mods")
            self._state = "load_mods"
            local mod_loader = require("scripts/mods/dml/init")
            self._mod_loader = mod_loader

            local mod_data = require("scripts/mods/mod_data")
            mod_loader:init(mod_data, self._parent:gui())
        elseif state == "load_mods" and self._mod_loader:update(dt) then
            log("StateBootLoadMods", "Mods loaded, exiting")
            return true, false
        end

        return false, false
    end

    local GameStateMachine = require("scripts/foundation/utilities/game_state_machine")

    local patched = false

    local GameStateMachine_init = GameStateMachine.init
    GameStateMachine.init = function(self, parent, start_state, params, ...)
        if not patched then
            log("mod_main", "Injecting mod loading state")
            patched = true

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
        end

        GameStateMachine_init(self, parent, start_state, params, ...)
    end

    log("mod_main", "Mod patching complete")
end

log("mod_main", "Initializing mods...")

local require_store = {}

Mods = {
    -- Keep a backup of certain system libraries before
    -- Fatshark's code scrubs them.
    -- The loader can then decide to pass them on to mods, or ignore them
    lua = setmetatable({}, {
        io = io,
        debug = debug,
        ffi = ffi,
        os = os,
        load = load,
        loadfile = loadfile,
        loadstring = loadstring,
    }),
    require_store = require_store
}

local can_insert = function(filepath, new_result)
    local store = require_store[filepath]
    if not store or #store then
        return true
    end

    if store[#store] ~= new_result then
        return true
    end
end

local original_require = require
require = function(filepath, ...)
    local result = original_require(filepath, ...)
    if result and type(result) == "table" then
        if can_insert(filepath, result) then
            require_store[filepath] = require_store[filepath] or {}
            local store = require_store[filepath]

            table.insert(store, result)

            if Mods.hook then
                Mods.hook.enable_by_file(filepath, #store)
            end
        end
    end

    return result
end

require("scripts/boot_init")
require("scripts/foundation/utilities/class")

-- The `__index` metamethod maps a proper identifier `CLASS.MyClassName` to the
-- stringified version of the key: `"MyClassName"`.
-- This allows using LuaCheck for the stringified class names in hook parameters.
_G.CLASS = setmetatable({}, {
    __index = function(_, key)
        return key
    end
})

local original_class = class
class = function(class_name, super_name, ...)
    local result = original_class(class_name, super_name, ...)
    if not rawget(_G, class_name) then
        rawset(_G, class_name, result)
    end
    if not rawget(_G.CLASS, class_name) then
        rawset(_G.CLASS, class_name, result)
    end
    return result
end

require("scripts/main")
log("mod_main", "'scripts/main' loaded")

-- Override `init` to run our injection
function init()
    patch_mod_loading_state()

    -- As requested by Fatshark
    local StateRequireScripts = require("scripts/game_states/boot/state_require_scripts")
    StateRequireScripts._get_is_modded = function() return true end

    Main:init()
end
