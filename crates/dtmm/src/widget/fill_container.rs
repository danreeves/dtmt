use std::f64::INFINITY;

use druid::widget::prelude::*;
use druid::{Point, WidgetPod};

pub struct FillContainer<T> {
    child: WidgetPod<T, Box<dyn Widget<T>>>,
}

impl<T: Data> FillContainer<T> {
    pub fn new(child: impl Widget<T> + 'static) -> Self {
        Self {
            child: WidgetPod::new(child).boxed(),
        }
    }
}

impl<T: Data> Widget<T> for FillContainer<T> {
    #[tracing::instrument(name = "FillContainer", level = "trace", skip_all)]
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        self.child.event(ctx, event, data, env);
    }

    #[tracing::instrument(name = "FillContainer", level = "trace", skip_all)]
    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        self.child.lifecycle(ctx, event, data, env)
    }

    #[tracing::instrument(name = "FillContainer", level = "trace", skip_all)]
    fn update(&mut self, ctx: &mut UpdateCtx, _: &T, data: &T, env: &Env) {
        self.child.update(ctx, data, env);
    }

    #[tracing::instrument(name = "FillContainer", level = "trace", skip_all)]
    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        bc.debug_check("FillContainer");

        let child_size = self.child.layout(ctx, bc, data, env);

        let w = if bc.is_width_bounded() {
            INFINITY
        } else {
            child_size.width
        };

        let h = if bc.is_height_bounded() {
            INFINITY
        } else {
            child_size.height
        };

        let my_size = bc.constrain(Size::new(w, h));

        self.child.set_origin(ctx, Point::new(0.0, 0.0));
        tracing::trace!("Computed layout: size={}", my_size);
        my_size
    }

    #[tracing::instrument(name = "FillContainer", level = "trace", skip_all)]
    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.child.paint(ctx, data, env);
    }
}
