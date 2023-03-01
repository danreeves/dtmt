use std::path::PathBuf;
use std::sync::Arc;

use druid::text::Formatter;
use druid::widget::{TextBoxEvent, ValidationDelegate};
use druid::EventCtx;

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

pub struct TextBoxOnChanged<F: Fn(&mut EventCtx, &str)>(F);

impl<F: Fn(&mut EventCtx, &str)> TextBoxOnChanged<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F: Fn(&mut EventCtx, &str)> ValidationDelegate for TextBoxOnChanged<F> {
    fn event(&mut self, ctx: &mut EventCtx, event: TextBoxEvent, current_text: &str) {
        if let TextBoxEvent::Complete = event {
            (self.0)(ctx, current_text)
        }
    }
}
