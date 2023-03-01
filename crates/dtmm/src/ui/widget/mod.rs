use std::path::PathBuf;
use std::sync::Arc;

use druid::text::Formatter;
use druid::{Data, Widget};

pub mod controller;

pub trait ExtraWidgetExt<T: Data>: Widget<T> + Sized + 'static {}

impl<T: Data, W: Widget<T> + 'static> ExtraWidgetExt<T> for W {}

pub(crate) struct PathBufFormatter;

impl PathBufFormatter {
    pub fn new() -> Self {
        Self {}
    }
}

impl Formatter<Arc<PathBuf>> for PathBufFormatter {
    fn format(&self, value: &Arc<PathBuf>) -> String {
        value.display().to_string()
    }

    fn validate_partial_input(
        &self,
        _input: &str,
        _sel: &druid::text::Selection,
    ) -> druid::text::Validation {
        druid::text::Validation::success()
    }

    fn value(&self, input: &str) -> Result<Arc<PathBuf>, druid::text::ValidationError> {
        let p = PathBuf::from(input);
        Ok(Arc::new(p))
    }
}
