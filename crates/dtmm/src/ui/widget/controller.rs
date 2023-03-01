use druid::widget::{Button, Controller, Scroll};
use druid::{Data, Env, Event, EventCtx, Rect, UpdateCtx, Widget};

use crate::state::{State, ACTION_START_SAVE_SETTINGS};

pub struct DisabledButtonController;

impl<T: Data> Controller<T, Button<T>> for DisabledButtonController {
    fn event(
        &mut self,
        child: &mut Button<T>,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut T,
        env: &Env,
    ) {
        if !ctx.is_disabled() {
            ctx.set_disabled(true);
            ctx.request_paint();
        }
        child.event(ctx, event, data, env)
    }

    fn update(
        &mut self,
        child: &mut Button<T>,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
        if !ctx.is_disabled() {
            ctx.set_disabled(true);
            ctx.request_paint();
        }
        child.update(ctx, old_data, data, env)
    }
}

pub struct AutoScrollController;

impl<T: Data, W: Widget<T>> Controller<T, Scroll<T, W>> for AutoScrollController {
    fn update(
        &mut self,
        child: &mut Scroll<T, W>,
        ctx: &mut UpdateCtx,
        old_data: &T,
        data: &T,
        env: &Env,
    ) {
        if !ctx.is_disabled() {
            let size = child.child_size();
            let end_region = Rect::new(size.width - 1., size.height - 1., size.width, size.height);
            child.scroll_to(ctx, end_region);
        }
        child.update(ctx, old_data, data, env)
    }
}

/// A controller that submits the command to save settings every time its widget's
/// data changes.
pub struct SaveSettingsController;

impl<W: Widget<State>> Controller<State, W> for SaveSettingsController {
    fn update(
        &mut self,
        child: &mut W,
        ctx: &mut UpdateCtx,
        old_data: &State,
        data: &State,
        env: &Env,
    ) {
        // Only filter for the values that actually go into the settings file.
        if old_data.mods != data.mods
            || old_data.game_dir != data.game_dir
            || old_data.data_dir != data.data_dir
        {
            ctx.submit_command(ACTION_START_SAVE_SETTINGS);
        }
        child.update(ctx, old_data, data, env)
    }
}
