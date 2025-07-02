use druid::widget::{Button, Controller, Image, Scroll};
use druid::{
    Data, Env, Event, EventCtx, ImageBuf, LifeCycle, LifeCycleCtx, Rect, UpdateCtx, Widget,
};

use crate::state::{State, ACTION_SET_DIRTY, ACTION_START_SAVE_SETTINGS};

#[allow(dead_code)]
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
        child.update(ctx, old_data, data, env);

        if !ctx.is_disabled() {
            let size = child.child_size();
            let end_region = Rect::new(size.width - 1., size.height - 1., size.width, size.height);
            child.scroll_to(ctx, end_region);
        }
    }
}

macro_rules! compare_state_fields {
    ($old:ident, $new:ident, $($field:ident),+) => {
        $(!Data::same(&$old.$field, &$new.$field)) || +
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
        // Only start tracking changes after the initial load has finished
        if old_data.loading == data.loading {
            if compare_state_fields!(
                old_data,
                data,
                mods,
                game_dir,
                data_dir,
                nexus_api_key,
                is_io_enabled
            ) {
                ctx.submit_command(ACTION_START_SAVE_SETTINGS);
            }

            if compare_state_fields!(old_data, data, mods, game_dir, is_io_enabled) {
                ctx.submit_command(ACTION_SET_DIRTY);
            }
        }

        child.update(ctx, old_data, data, env)
    }
}

pub struct ImageLensController;

impl Controller<ImageBuf, Image> for ImageLensController {
    fn lifecycle(
        &mut self,
        widget: &mut Image,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &ImageBuf,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            widget.set_image_data(data.clone());
        }

        widget.lifecycle(ctx, event, data, env);
    }

    fn update(
        &mut self,
        widget: &mut Image,
        ctx: &mut UpdateCtx,
        old_data: &ImageBuf,
        data: &ImageBuf,
        env: &Env,
    ) {
        if !Data::same(old_data, data) {
            widget.set_image_data(data.clone());
        }

        widget.update(ctx, old_data, data, env);
    }
}
