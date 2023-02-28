use druid::{Data, Widget, WidgetPod};

pub struct Container<T> {
    child: WidgetPod<T, Box<dyn Widget<T>>>,
}

impl<T: Data> Container<T> {}
