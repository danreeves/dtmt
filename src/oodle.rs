use std::process::Stdio;
use std::sync::Arc;

use color_eyre::eyre::Context;
use color_eyre::{eyre, Help, Result, SectionExt};
use nanorand::Rng;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt, AsyncWrite, BufReader, BufWriter};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::{fs, io};
use tracing::Instrument;

#[tracing::instrument(level = "debug", skip(ctx, r, w))]
pub(crate) async fn decompress<R, W>(
    ctx: Arc<RwLock<crate::Context>>,
    r: R,
    w: W,
    num_chunks: usize,
) -> Result<()>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
    W: AsyncWrite + std::marker::Unpin,
{
    let mut r = BufReader::new(r);
    let mut w = BufWriter::new(w);

    let padding_start = r.stream_position().await?;

    let mut rng = nanorand::WyRand::new();
    let leaf = rng.generate::<u64>();

    let tmp_dir = std::env::temp_dir().join(format!("dtmt-{}", leaf));

    fs::create_dir(&tmp_dir).await?;
    tracing::trace!(tmp_dir = %tmp_dir.display());

    let in_path = tmp_dir.join("in.bin");
    let out_path = tmp_dir.join("out.bin");

    {
        let mut in_file = File::create(&in_path).await?;
        io::copy(&mut r, &mut in_file)
            .await
            .wrap_err("failed to write compressed data to file")
            .with_section(|| in_path.display().to_string().header("Path"))?;
    }

    {
        let _span = tracing::span!(tracing::Level::INFO, "Run decompression helper");
        async {
            let mut cmd = {
                let ctx = ctx.read().await;
                Command::new(ctx.oodle.as_ref().expect("`oodle` arg not passed through"))
            };

            let cmd = cmd
                .args(["-v", "-v", "-v"])
                .args(["--padding", &padding_start.to_string()])
                .args(["--chunks", &num_chunks.to_string()])
                .arg("decompress")
                .arg(&in_path)
                .arg(&out_path)
                .stdin(Stdio::null());

            tracing::debug!(?cmd, "Running Oodle decompression helper");

            let res = cmd
                .output()
                .await
                .wrap_err("failed to spawn the Oodle decompression helper")?;

            tracing::trace!(
                "Output of Oodle decompression helper:\n{}",
                String::from_utf8_lossy(&res.stdout)
            );

            if !res.status.success() {
                let stderr = String::from_utf8_lossy(&res.stderr);
                let stdout = String::from_utf8_lossy(&res.stdout);
                return Err(eyre::eyre!("failed to run Oodle decompression helper"))
                    .with_section(move || stdout.to_string().header("Logs:"))
                    .with_section(move || stderr.to_string().header("Stderr:"));
            }

            Ok(())
        }
        .instrument(_span)
        .await
        .with_section(|| tmp_dir.display().to_string().header("Temp Dir:"))?
    }

    {
        let mut out_file = File::open(&out_path).await?;
        io::copy(&mut out_file, &mut w)
            .await
            .wrap_err("failed to read decompressed file")
            .with_section(|| out_path.display().to_string().header("Path"))?;
    }

    fs::remove_dir_all(tmp_dir)
        .await
        .wrap_err("failed to remove temporary directory")?;

    Ok(())
}
