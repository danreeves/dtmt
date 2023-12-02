local StateGame = require("scripts/game_states/state_game")
local StateSplash = require("scripts/game_states/game/state_splash")
local GameStateMachine = require("scripts/foundation/utilities/game_state_machine")

local function hook(obj, fn_name, cb)
    local orig = obj[fn_name]

    obj[fn_name] = function(...)
        return cb(orig, ...)
    end
end

function init(mod_data, boot_gui)
    local ModLoader = require("scripts/mods/mod_loader")
    local mod_loader = ModLoader:new(mod_data, boot_gui)

    -- The mod loader needs to remain active during game play, to
    -- enable reloads
    hook(StateGame, "update", function(func, dt, ...)
        mod_loader:update(dt)
        return func(dt, ...)
    end)

    -- Skip splash view
    hook(StateSplash, "on_enter", function(func, self, ...)
        local result = func(self, ...)

        self._should_skip = true
        self._continue = true

        return result
    end)

    -- Trigger state change events
    hook(GameStateMachine, "_change_state", function(func, self, ...)
        local old_state = self._state
        local old_state_name = old_state and self:current_state_name()

        if old_state_name then
            mod_loader:on_game_state_changed("exit", old_state_name, old_state)
        end

        local result = func(self, ...)

        local new_state = self._state
        local new_state_name = new_state and self:current_state_name()

        if new_state_name then
            mod_loader:on_game_state_changed("enter", new_state_name, new_state)
        end

        return result
    end)

    -- Trigger ending state change event
    hook(GameStateMachine, "destroy", function(func, self, ...)
        local old_state = self._state
        local old_state_name = old_state and self:current_state_name()

        if old_state_name then
            mod_loader:on_game_state_changed("exit", old_state_name)
        end

        return func(self, ...)
    end)

    return mod_loader
end

return init
