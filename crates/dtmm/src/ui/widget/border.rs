use druid::kurbo::Line;
use druid::widget::prelude::*;
use druid::{Color, KeyOrValue, Point, WidgetPod};

pub struct Border<T> {
    inner: WidgetPod<T, Box<dyn Widget<T>>>,
    color: BorderColor,
    width: BorderWidths,
    // corner_radius: KeyOrValue<RoundedRectRadii>,
}

impl<T: Data> Border<T> {
    pub fn new(inner: impl Widget<T> + 'static) -> Self {
        let inner = WidgetPod::new(inner).boxed();
        Self {
            inner,
            color: Color::TRANSPARENT.into(),
            width: 0f64.into(),
        }
    }

    pub fn set_color(&mut self, color: impl Into<KeyOrValue<Color>>) {
        self.color = BorderColor::Uniform(color.into());
    }

    pub fn with_color(mut self, color: impl Into<KeyOrValue<Color>>) -> Self {
        self.set_color(color);
        self
    }

    pub fn set_bottom_border(&mut self, width: impl Into<KeyOrValue<f64>>) {
        self.width.bottom = width.into();
    }

    pub fn with_bottom_border(mut self, width: impl Into<KeyOrValue<f64>>) -> Self {
        self.set_bottom_border(width);
        self
    }

    pub fn set_top_border(&mut self, width: impl Into<KeyOrValue<f64>>) {
        self.width.top = width.into();
    }

    pub fn with_top_border(mut self, width: impl Into<KeyOrValue<f64>>) -> Self {
        self.set_top_border(width);
        self
    }
}

impl<T: Data> Widget<T> for Border<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        self.inner.event(ctx, event, data, env)
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _: &T, data: &T, env: &Env) {
        self.inner.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        bc.debug_check("Border");

        let (left, top, right, bottom) = self.width.resolve(env);

        let inner_bc = bc.shrink((left + right, top + bottom));
        let inner_size = self.inner.layout(ctx, &inner_bc, data, env);

        let origin = Point::new(left, top);
        self.inner.set_origin(ctx, origin);

        let size = Size::new(
            inner_size.width + left + right,
            inner_size.height + top + bottom,
        );

        let insets = self.inner.compute_parent_paint_insets(size);
        ctx.set_paint_insets(insets);

        let baseline_offset = self.inner.baseline_offset();
        if baseline_offset > 0. {
            ctx.set_baseline_offset(baseline_offset + bottom);
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let size = ctx.size();
        let (left, top, right, bottom) = self.width.resolve(env);
        let (col_left, col_top, col_right, col_bottom) = self.color.resolve(env);

        self.inner.paint(ctx, data, env);

        // There's probably a more elegant way to create the various `Line`s, but this works for now.
        // The important bit is to move each line inwards by half each side's border width. Otherwise
        // it would draw hald of the border outside of the widget's boundary.

        if left > 0. {
            ctx.stroke(
                Line::new((left / 2., top / 2.), (left / 2., size.height)),
                &col_left,
                left,
            );
        }

        if top > 0. {
            ctx.stroke(
                Line::new((left / 2., top / 2.), (size.width - (right / 2.), top / 2.)),
                &col_top,
                top,
            );
        }

        if right > 0. {
            ctx.stroke(
                Line::new(
                    (size.width - (right / 2.), top / 2.),
                    (size.width - (right / 2.), size.height - (bottom / 2.)),
                ),
                &col_right,
                right,
            );
        }

        if bottom > 0. {
            ctx.stroke(
                Line::new(
                    (left / 2., size.height - (bottom / 2.)),
                    (size.width - (right / 2.), size.height - (bottom / 2.)),
                ),
                &col_bottom,
                bottom,
            );
        }
    }
}

#[derive(Clone, Debug)]
pub enum BorderColor {
    Uniform(KeyOrValue<Color>),
    // Individual {
    //     left: KeyOrValue<Color>,
    //     top: KeyOrValue<Color>,
    //     right: KeyOrValue<Color>,
    //     bottom: KeyOrValue<Color>,
    // },
}

impl BorderColor {
    pub fn resolve(&self, env: &Env) -> (Color, Color, Color, Color) {
        match self {
            Self::Uniform(val) => {
                let color = val.resolve(env);
                (color, color, color, color)
            }
        }
    }
}

impl From<Color> for BorderColor {
    fn from(value: Color) -> Self {
        Self::Uniform(value.into())
    }
}

#[derive(Clone, Debug)]
pub struct BorderWidths {
    pub left: KeyOrValue<f64>,
    pub top: KeyOrValue<f64>,
    pub right: KeyOrValue<f64>,
    pub bottom: KeyOrValue<f64>,
}

impl From<f64> for BorderWidths {
    fn from(value: f64) -> Self {
        Self {
            left: value.into(),
            top: value.into(),
            right: value.into(),
            bottom: value.into(),
        }
    }
}

impl BorderWidths {
    pub fn resolve(&self, env: &Env) -> (f64, f64, f64, f64) {
        (
            self.left.resolve(env),
            self.top.resolve(env),
            self.right.resolve(env),
            self.bottom.resolve(env),
        )
    }
}
