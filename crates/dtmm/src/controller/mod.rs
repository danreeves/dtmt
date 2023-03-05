use std::path::Path;

use color_eyre::{eyre::Context, Result};
use serde::Deserialize;
use tokio::fs;

pub mod app;
pub mod game;
pub mod worker;

#[tracing::instrument]
async fn read_sjson_file<P, T>(path: P) -> Result<T>
where
    T: for<'a> Deserialize<'a>,
    P: AsRef<Path> + std::fmt::Debug,
{
    let path = path.as_ref();
    let buf = fs::read(path)
        .await
        .wrap_err_with(|| format!("failed to read file '{}'", path.display()))?;
    let data = String::from_utf8(buf).wrap_err("invalid UTF8")?;
    serde_sjson::from_str(&data).wrap_err("failed to deserialize SJSON")
}
