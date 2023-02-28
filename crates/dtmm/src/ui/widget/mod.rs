use druid::{Data, Widget};

pub mod container;
pub mod controller;

pub trait ExtraWidgetExt<T: Data>: Widget<T> + Sized + 'static {}

impl<T: Data, W: Widget<T> + 'static> ExtraWidgetExt<T> for W {}
