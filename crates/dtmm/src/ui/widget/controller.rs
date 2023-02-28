use druid::widget::{Button, Controller};
use druid::{Data, Env, Event, EventCtx, LifeCycle, LifeCycleCtx, UpdateCtx, Widget};

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
