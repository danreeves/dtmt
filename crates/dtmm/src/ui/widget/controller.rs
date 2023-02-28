use druid::widget::{Button, Controller, Scroll};
use druid::{Data, Env, Event, EventCtx, LifeCycle, LifeCycleCtx, Rect, UpdateCtx, Widget};

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

    fn lifecycle(
        &mut self,
        child: &mut Button<T>,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &T,
        env: &Env,
    ) {
        child.lifecycle(ctx, event, data, env)
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
