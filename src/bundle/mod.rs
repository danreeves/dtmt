use std::io::SeekFrom;
use std::sync::Arc;

use color_eyre::eyre::{self, Context, Result};
use color_eyre::{Help, SectionExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::RwLock;

use crate::oodle;

#[derive(Debug, PartialEq)]
enum BundleFormat {
    Darktide,
}

impl TryFrom<u32> for BundleFormat {
    type Error = color_eyre::Report;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0xF0000007 => Ok(Self::Darktide),
            _ => Err(eyre::eyre!("Unknown bundle format '{:08X}'", value)),
        }
    }
}

async fn read_u32<R>(mut r: R) -> Result<u32>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
{
    let res = r.read_u32_le().await.wrap_err("failed to read u32");

    if res.is_err() {
        let pos = r.stream_position().await;
        if pos.is_ok() {
            res.with_section(|| pos.unwrap().to_string().header("Position: "))
        } else {
            res
        }
    } else {
        res
    }
}

/// Returns a decompressed version of the bundle data.
/// This is mainly useful for debugging purposes or
/// to manullay inspect the raw data.
#[tracing::instrument(skip(ctx, r, w))]
pub async fn decompress<R, W>(ctx: Arc<RwLock<crate::Context>>, mut r: R, mut w: W) -> Result<()>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
    W: AsyncWrite + std::marker::Unpin,
{
    let format = read_u32(&mut r).await.and_then(BundleFormat::try_from)?;

    if format != BundleFormat::Darktide {
        return Err(eyre::eyre!("Unknown bundle format: {:?}", format));
    }

    // Skip unknown 4 bytes
    r.seek(SeekFrom::Current(4)).await?;

    let num_entries = read_u32(&mut r).await? as i64;

    // Skip unknown 256 bytes
    r.seek(SeekFrom::Current(256)).await?;
    // Skip file meta
    r.seek(SeekFrom::Current(num_entries * 20)).await?;

    let num_chunks = read_u32(&mut r).await? as usize;
    // Skip chunk sizes
    r.seek(SeekFrom::Current(num_chunks as i64 * 4)).await?;

    {
        let size_1 = read_u32(&mut r).await?;

        // Skip unknown 4 bytes
        r.seek(SeekFrom::Current(4)).await?;

        // NOTE: Unknown why there sometimes is a second value.
        if size_1 == 0x0 {
            // Skip unknown 4 bytes
            r.seek(SeekFrom::Current(8)).await?;
        }
    }

    let chunks_start = r.stream_position().await?;

    {
        // Pipe the header into the output
        r.seek(SeekFrom::Start(0)).await?;
        let mut buf = vec![0; chunks_start as usize];
        r.read_exact(&mut buf).await?;
        w.write_all(&buf).await?;
    }

    oodle::decompress(ctx, r, w, num_chunks).await
}
