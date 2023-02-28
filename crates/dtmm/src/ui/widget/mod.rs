use druid::{Data, Widget};

use self::fill_container::FillContainer;

pub mod container;
pub mod controller;
pub mod fill_container;

pub trait ExtraWidgetExt<T: Data>: Widget<T> + Sized + 'static {
    fn content_must_fill(self) -> FillContainer<T> {
        FillContainer::new(self)
    }
}

impl<T: Data, W: Widget<T> + 'static> ExtraWidgetExt<T> for W {}
