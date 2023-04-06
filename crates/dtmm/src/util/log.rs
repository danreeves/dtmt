use clap::ValueEnum;
use tokio::sync::mpsc::UnboundedSender;
use tracing_error::ErrorLayer;
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::debug_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[derive(Clone, Copy, ValueEnum)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for EnvFilter {
    fn from(level: LogLevel) -> Self {
        let filter = match level {
            LogLevel::Trace => "error,dtmm=trace,sdk=trace",
            LogLevel::Debug => "error,dtmm=debug,sdk=debug",
            LogLevel::Info => "error,dtmm=info",
            LogLevel::Warn => "error,dtmm=warn",
            LogLevel::Error => "error",
        };
        EnvFilter::new(filter)
    }
}

pub struct ChannelWriter {
    tx: UnboundedSender<Vec<u8>>,
}

impl ChannelWriter {
    pub fn new(tx: UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let tx = self.tx.clone();
        // The `send` errors when the receiving end has closed.
        // But there's not much we can do at that point, so we just ignore it.
        let _ = tx.send(buf.to_vec());

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn create_tracing_subscriber(tx: UnboundedSender<Vec<u8>>, level: Option<LogLevel>) {
    let env_layer = if let Some(level) = level {
        EnvFilter::from(level)
    } else if cfg!(debug_assertions) {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    } else {
        EnvFilter::new("error,dtmm=info")
    };

    let stdout_layer = if cfg!(debug_assertions) {
        let layer = fmt::layer().pretty();
        Some(layer)
    } else {
        None
    };

    let channel_layer = fmt::layer()
        .event_format(dtmt_shared::Formatter)
        .fmt_fields(debug_fn(dtmt_shared::format_fields))
        .with_writer(move || ChannelWriter::new(tx.clone()))
        .with_filter(FilterFn::new(dtmt_shared::filter_fields));

    tracing_subscriber::registry()
        .with(env_layer)
        .with(channel_layer)
        .with(stdout_layer)
        .with(ErrorLayer::new(fmt::format::Pretty::default()))
        .init();
}
