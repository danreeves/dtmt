// Rust Analyzer cannot properly determine that `cfg!(debug_assertions)` alone does not make code
// unused. These sections should be small enough that no truly dead code slips in.

#[allow(dead_code)]
mod prod {
    use std::fmt::Result;

    use time::format_description::FormatItem;
    use time::macros::format_description;
    use time::OffsetDateTime;
    use tracing::field::Field;
    use tracing::{Event, Metadata, Subscriber};
    use tracing_error::ErrorLayer;
    use tracing_subscriber::filter::FilterFn;
    use tracing_subscriber::fmt::format::{debug_fn, Writer};
    use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::registry::LookupSpan;
    use tracing_subscriber::EnvFilter;

    const TIME_FORMAT: &[FormatItem] = format_description!("[hour]:[minute]:[second]");

    fn format_field(w: &mut Writer<'_>, field: &Field, val: &dyn std::fmt::Debug) -> Result {
        if field.name() == "message" {
            write!(w, "{:?}", val)
        } else {
            Ok(())
        }
    }

    fn filter(metadata: &Metadata<'_>) -> bool {
        metadata
            .fields()
            .iter()
            .any(|field| field.name() == "message")
    }

    struct Formatter;

    impl<S, N> FormatEvent<S, N> for Formatter
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        N: for<'a> FormatFields<'a> + 'static,
    {
        fn format_event(
            &self,
            ctx: &FmtContext<'_, S, N>,
            mut writer: Writer<'_>,
            event: &Event<'_>,
        ) -> Result {
            let meta = event.metadata();

            let time = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
            let time = time.format(TIME_FORMAT).map_err(|_| std::fmt::Error)?;

            write!(writer, "[{}] [{:>5}] ", time, meta.level())?;

            ctx.field_format().format_fields(writer.by_ref(), event)?;

            writeln!(writer)
        }
    }

    /// Creates a subscriber that
    /// - only prints events that contain a message
    /// - does not print fields
    /// - does not print spans/targets
    /// - only prints time, not date
    pub fn create_tracing_subscriber() {
        let filter_layer = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::try_new("info").unwrap());

        let fmt_layer = tracing_subscriber::fmt::layer()
            .event_format(Formatter)
            .fmt_fields(debug_fn(format_field));

        tracing_subscriber::registry()
            .with(FilterFn::new(filter))
            .with(filter_layer)
            .with(fmt_layer)
            .with(ErrorLayer::new(
                tracing_subscriber::fmt::format::Pretty::default(),
            ))
            .init();
    }
}

#[allow(dead_code)]
mod dev {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    pub fn create_tracing_subscriber() {
        let filter_layer = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::try_new("info").unwrap());
        let fmt_layer = tracing_subscriber::fmt::layer().pretty();

        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .with(ErrorLayer::new(
                tracing_subscriber::fmt::format::Pretty::default(),
            ))
            .init();
    }
}

#[cfg(debug_assertions)]
pub use dev::create_tracing_subscriber;

#[cfg(not(debug_assertions))]
pub use prod::create_tracing_subscriber;
