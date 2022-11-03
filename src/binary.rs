use color_eyre::eyre::WrapErr;
use color_eyre::{Help, Result, SectionExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

macro_rules! make_read {
    ($func:ident, $op:ident, $type:ty) => {
        pub(crate) async fn $func<R>(mut r: R) -> Result<$type>
        where
            R: AsyncRead + AsyncSeek + std::marker::Unpin,
        {
            let res = r
                .$op()
                .await
                .wrap_err(concat!("failed to read ", stringify!($type)));

            if res.is_err() {
                let pos = r.stream_position().await;
                if pos.is_ok() {
                    res.with_section(|| {
                        format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                    })
                } else {
                    res
                }
            } else {
                res
            }
        }
    };
}

macro_rules! make_skip {
    ($func:ident, $read:ident, $op:ident, $type:ty) => {
        pub(crate) async fn $func<R>(mut r: R, cmp: $type) -> Result<()>
        where
            R: AsyncRead + AsyncSeek + std::marker::Unpin,
        {
            let val = $read(&mut r).await?;

            if val != cmp {
                let pos = r.stream_position().await.unwrap_or(u64::MAX);
                tracing::debug!(
                    pos,
                    expected = cmp,
                    actual = val,
                    "Unexpected value for skipped {}",
                    stringify!($type)
                );
            }

            Ok(())
        }
    };
}

make_read!(read_u8, read_u8, u8);
make_read!(read_u32, read_u32_le, u32);
make_read!(read_u64, read_u64_le, u64);

make_skip!(skip_u8, read_u8, read_u8, u8);
make_skip!(skip_u32, read_u32, read_u32_le, u32);
