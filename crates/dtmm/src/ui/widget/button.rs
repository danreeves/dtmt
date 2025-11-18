use druid::WidgetPod;
use druid::widget::prelude::*;
use druid::widget::{Click, ControllerHost, Label, LabelText};
use druid::{Affine, WidgetExt};

use crate::ui::theme;

pub struct Button<T> {
    inner: WidgetPod<T, Box<dyn Widget<T>>>,
    inner_size: Size,
}

impl<T: Data> Button<T> {
    pub fn new(inner: impl Widget<T> + 'static) -> Self {
        let inner = inner.env_scope(|env, _| {
            env.set(
                druid::theme::TEXT_COLOR,
                env.get(theme::keys::KEY_BUTTON_FG),
            );
            env.set(
                druid::theme::DISABLED_TEXT_COLOR,
                env.get(theme::keys::KEY_BUTTON_FG_DISABLED),
            );
        });
        let inner = WidgetPod::new(inner).boxed();
        Self {
            inner,
            inner_size: Size::ZERO,
        }
    }

    pub fn with_label(text: impl Into<LabelText<T>>) -> Self {
        let inner = Label::new(text);
        Self::new(inner)
    }

    pub fn on_click(
        self,
        f: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ControllerHost<Self, Click<T>> {
        ControllerHost::new(self, Click::new(f))
    }
}

impl<T: Data> Widget<T> for Button<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _: &mut T, _: &Env) {
        match event {
            Event::MouseDown(_) if !ctx.is_disabled() => {
                ctx.set_active(true);
                ctx.request_paint();
            }
            Event::MouseUp(_) => {
                if ctx.is_active() && !ctx.is_disabled() {
                    ctx.request_paint();
                }
                ctx.set_active(false);
            }
            _ => {}
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        if let LifeCycle::HotChanged(_) | LifeCycle::DisabledChanged(_) = event {
            ctx.request_paint();
        }
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _: &T, data: &T, env: &Env) {
        self.inner.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        bc.debug_check("Button");

        let padding = env.get(theme::keys::KEY_BUTTON_PADDING).size();
        let inner_bc = bc.shrink(padding).loosen();

        self.inner_size = self.inner.layout(ctx, &inner_bc, data, env);

        bc.constrain(Size::new(
            self.inner_size.width + padding.width,
            self.inner_size.height + padding.height,
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let size = ctx.size();

        let bg_color = if ctx.is_disabled() {
            env.get(theme::keys::KEY_BUTTON_BG_DISABLED)
        } else if ctx.is_hot() {
            env.get(theme::keys::KEY_BUTTON_BG_HOT)
        } else if ctx.is_active() {
            env.get(theme::keys::KEY_BUTTON_BG_ACTIVE)
        } else {
            env.get(theme::keys::KEY_BUTTON_BG)
        };

        ctx.fill(
            size.to_rect()
                .to_rounded_rect(env.get(druid::theme::BUTTON_BORDER_RADIUS)),
            &bg_color,
        );

        let inner_pos = (size.to_vec2() - self.inner_size.to_vec2()) / 2.;

        ctx.with_save(|ctx| {
            ctx.transform(Affine::translate(inner_pos));
            self.inner.paint(ctx, data, env);
        });
    }
}
