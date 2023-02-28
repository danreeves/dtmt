use tokio::sync::mpsc::UnboundedSender;
use tracing_error::ErrorLayer;
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::debug_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

// I currently cannot find a way to add a parameter to `dtmt_shared::create_tracing_subscriber`
// that would allow me to pass an extra `Layer` to that function. So, for now,
// its code has to be duplicated here.

pub struct ChannelWriter {
    tx: UnboundedSender<String>,
}

impl ChannelWriter {
    pub fn new(tx: UnboundedSender<String>) -> Self {
        Self { tx }
    }
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let tx = self.tx.clone();
        let string = String::from_utf8_lossy(buf).to_string();

        // The `send` errors when the receiving end has closed.
        // But there's not much we can do at that point, so we just ignore it.
        let _ = tx.send(string);

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn create_tracing_subscriber(tx: UnboundedSender<String>) {
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
            .event_format(dtmt_shared::Formatter)
            .fmt_fields(debug_fn(dtmt_shared::format_field));

        (
            None,
            Some(fmt_layer),
            Some(FilterFn::new(dtmt_shared::filter)),
        )
    };

    let channel_layer = fmt::layer()
        // TODO: Re-enable and implement a formatter for the Druid widget
        .with_ansi(false)
        .event_format(dtmt_shared::Formatter)
        .fmt_fields(debug_fn(dtmt_shared::format_field))
        .with_writer(move || ChannelWriter::new(tx.clone()));

    tracing_subscriber::registry()
        .with(channel_layer)
        .with(filter_layer)
        .with(env_layer)
        .with(dev_stdout_layer)
        .with(prod_stdout_layer)
        .with(ErrorLayer::new(fmt::format::Pretty::default()))
        .init();
}
