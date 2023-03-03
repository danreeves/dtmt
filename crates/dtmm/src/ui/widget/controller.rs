use druid::widget::{Button, Controller, Scroll};
use druid::{Data, Env, Event, EventCtx, Rect, UpdateCtx, Widget};

use crate::state::{State, ACTION_SET_DIRTY, ACTION_START_SAVE_SETTINGS};

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

macro_rules! compare_state_fields {
    ($old:ident, $new:ident, $($field:ident),+) => {
        $($old.$field != $new.$field) || +
    }
}

/// A controller that tracks state changes for certain fields and submits commands to handle them.
pub struct DirtyStateController;

impl<W: Widget<State>> Controller<State, W> for DirtyStateController {
    fn update(
        &mut self,
        child: &mut W,
        ctx: &mut UpdateCtx,
        old_data: &State,
        data: &State,
        env: &Env,
    ) {
        if compare_state_fields!(old_data, data, mods, game_dir, data_dir) {
            ctx.submit_command(ACTION_START_SAVE_SETTINGS);
        }

        if compare_state_fields!(old_data, data, mods, game_dir) {
            ctx.submit_command(ACTION_SET_DIRTY);
        }

        child.update(ctx, old_data, data, env)
    }
}
