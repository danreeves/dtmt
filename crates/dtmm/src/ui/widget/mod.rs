use std::path::PathBuf;
use std::sync::Arc;

use druid::text::Formatter;

pub mod border;
pub mod button;
pub mod controller;

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
