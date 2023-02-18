use druid::widget::{Controller, Flex};
use druid::{Data, Widget};

pub struct TableSelect<T> {
    widget: Flex<T>,
    controller: TableSelectController<T>,
}

impl<T: Data> TableSelect<T> {
    pub fn new(values: impl IntoIterator<Item = (impl Widget<T> + 'static)>) -> Self {
        todo!();
    }
}

impl<T: Data> Widget<T> for TableSelect<T> {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut T,
        env: &druid::Env,
    ) {
        todo!()
    }

    fn lifecycle(
        &mut self,
        ctx: &mut druid::LifeCycleCtx,
        event: &druid::LifeCycle,
        data: &T,
        env: &druid::Env,
    ) {
        todo!()
    }

    fn update(&mut self, ctx: &mut druid::UpdateCtx, old_data: &T, data: &T, env: &druid::Env) {
        todo!()
    }

    fn layout(
        &mut self,
        ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        data: &T,
        env: &druid::Env,
    ) -> druid::Size {
        todo!()
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &T, env: &druid::Env) {
        todo!()
    }
}

struct TableSelectController<T> {
    inner: T,
}

impl<T: Data> TableSelectController<T> {}

impl<T: Data> Controller<T, Flex<T>> for TableSelectController<T> {}

pub struct TableItem<T> {
    inner: dyn Widget<T>,
}

impl<T: Data> TableItem<T> {
    pub fn new(inner: impl Widget<T>) -> Self {
        todo!();
    }
}

impl<T: Data> Widget<T> for TableItem<T> {}
