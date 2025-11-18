use clap::ValueEnum;
use tokio::sync::mpsc::UnboundedSender;
use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::debug_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;

#[derive(Clone, Copy, Debug, ValueEnum)]
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

pub fn create_tracing_subscriber(level: Option<LogLevel>, tx: Option<UnboundedSender<Vec<u8>>>) {
    let mut env_layer = if let Some(level) = level {
        EnvFilter::from(level)
    } else if cfg!(debug_assertions) {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    } else {
        EnvFilter::new("error,dtmm=info")
    };

    // The internal implementation of Druid's GTK file dialog turns
    // cancelling the dialog into an error. The, also internal, wrapper
    // then logs and swallows the error.
    // Therefore, as a consumer of the library, we don't have any way
    // to customize this behavior, and instead have to filter out the
    // tracing event.
    env_layer = env_layer.add_directive(
        "druid_shell::backend::gtk::window=off"
            .parse()
            .expect("Invalid env filter directive"),
    );

    let stdout_layer = fmt::layer().pretty();

    let channel_layer = tx.map(|tx| {
        fmt::layer()
            .event_format(dtmt_shared::Formatter)
            .fmt_fields(debug_fn(dtmt_shared::format_fields))
            .with_writer(move || ChannelWriter::new(tx.clone()))
            .with_filter(FilterFn::new(dtmt_shared::filter_fields))
    });

    tracing_subscriber::registry()
        .with(env_layer)
        .with(channel_layer)
        .with(stdout_layer)
        .with(ErrorLayer::new(fmt::format::Pretty::default()))
        .init();
}
