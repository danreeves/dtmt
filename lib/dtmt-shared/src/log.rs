use std::fmt::Result;

use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;
use tracing::field::Field;
use tracing::{Event, Metadata, Subscriber};
use tracing_error::ErrorLayer;
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt::format::{debug_fn, Writer};
use tracing_subscriber::fmt::{self, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

pub const TIME_FORMAT: &[FormatItem] = format_description!("[hour]:[minute]:[second]");

pub fn format_field(w: &mut Writer<'_>, field: &Field, val: &dyn std::fmt::Debug) -> Result {
    if field.name() == "message" {
        write!(w, "{:?}", val)
    } else {
        Ok(())
    }
}

pub fn filter(metadata: &Metadata<'_>) -> bool {
    metadata
        .fields()
        .iter()
        .any(|field| field.name() == "message")
}

pub struct Formatter;

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

pub fn create_tracing_subscriber() {
    let env_layer =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::try_new("info").unwrap());

    let (dev_stdout_layer, prod_stdout_layer, filter_layer) = if cfg!(debug_assertions) {
        let fmt_layer = fmt::layer().pretty();
        (Some(fmt_layer), None, None)
    } else {
        // Creates a layer that
        // - only prints events that contain a message
        // - does not print fields
        // - does not print spans/targets
        // - only prints time, not date
        let fmt_layer = fmt::layer()
            .event_format(Formatter)
            .fmt_fields(debug_fn(format_field));

        (None, Some(fmt_layer), Some(FilterFn::new(filter)))
    };

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(env_layer)
        .with(dev_stdout_layer)
        .with(prod_stdout_layer)
        .with(ErrorLayer::new(fmt::format::Pretty::default()))
        .init();
}
