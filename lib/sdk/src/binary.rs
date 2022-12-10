use std::io::SeekFrom;

use color_eyre::eyre::WrapErr;
use color_eyre::{Help, Result, SectionExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt};

// TODO: Add versions for each write and read function that can work without `AsyncSeek`

macro_rules! make_read {
    ($func:ident, $op:ident, $type:ty) => {
        pub(crate) async fn $func<R>(r: &mut R) -> Result<$type>
        where
            R: AsyncRead + AsyncSeek + std::marker::Unpin,
        {
            let res = r
                .$op()
                .await
                .wrap_err(concat!("failed to read ", stringify!($type)));

            if res.is_ok() {
                return res;
            }

            let pos = r.stream_position().await;
            if pos.is_ok() {
                res.with_section(|| {
                    format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                })
            } else {
                res
            }
        }
    };
}

macro_rules! make_write {
    ($func:ident, $op:ident, $type:ty) => {
        pub(crate) async fn $func<W>(w: &mut W, val: $type) -> Result<()>
        where
            W: AsyncWrite + AsyncSeek + std::marker::Unpin,
        {
            let res = w
                .$op(val)
                .await
                .wrap_err(concat!("failed to write ", stringify!($type)));

            if res.is_ok() {
                return res;
            }

            let pos = w.stream_position().await;
            if pos.is_ok() {
                res.with_section(|| {
                    format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                })
            } else {
                res
            }
        }
    };
}

macro_rules! make_skip {
    ($func:ident, $read:ident, $type:ty) => {
        pub(crate) async fn $func<R>(r: &mut R, cmp: $type) -> Result<()>
        where
            R: AsyncRead + AsyncSeek + std::marker::Unpin,
        {
            let val = $read(r).await?;

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

make_write!(write_u8, write_u8, u8);
make_write!(write_u32, write_u32_le, u32);
make_write!(write_u64, write_u64_le, u64);

make_skip!(skip_u8, read_u8, u8);
make_skip!(skip_u32, read_u32, u32);

pub(crate) async fn skip_padding<S>(stream: &mut S) -> Result<()>
where
    S: AsyncSeek + std::marker::Unpin,
{
    let pos = stream.stream_position().await?;
    let padding_size = 16 - (pos % 16);

    if padding_size < 16 && padding_size > 0 {
        tracing::trace!(pos, padding_size, "Skipping padding");
        stream.seek(SeekFrom::Current(padding_size as i64)).await?;
    } else {
        tracing::trace!(pos, padding_size, "No padding to skip");
    }

    Ok(())
}

pub(crate) async fn _read_up_to<R>(r: &mut R, buf: &mut Vec<u8>) -> Result<usize>
where
    R: AsyncRead + AsyncSeek + std::marker::Unpin,
{
    let pos = r.stream_position().await?;

    let err = {
        match r.read_exact(buf).await {
            Ok(_) => return Ok(buf.len()),
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                r.seek(SeekFrom::Start(pos)).await?;
                match r.read_to_end(buf).await {
                    Ok(read) => return Ok(read),
                    Err(err) => err,
                }
            }
            Err(err) => err,
        }
    };

    Err(err).with_section(|| format!("{pos:#X} ({pos})").header("Position: "))
}

pub(crate) async fn write_padding<W>(w: &mut W) -> Result<usize>
where
    W: AsyncWrite + AsyncSeek + std::marker::Unpin,
{
    let pos = w.stream_position().await?;
    let size = 16 - (pos % 16) as usize;

    tracing::trace!(padding_size = size, "Writing padding");

    if size > 0 && size < 16 {
        let buf = vec![0; size];
        w.write_all(&buf).await?;
        Ok(size)
    } else {
        Ok(0)
    }
}

pub mod sync {
    use std::io::{self, Read, Seek, SeekFrom};

    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
    use color_eyre::eyre::WrapErr;
    use color_eyre::{Help, Result, SectionExt};

    macro_rules! make_read {
        ($func:ident, $read:ident, $type:ty) => {
            fn $read(&mut self) -> io::Result<$type> {
                ReadBytesExt::$func::<LittleEndian>(self)
            }

            fn $func(&mut self) -> Result<$type> {
                let res =
                    ReadExt::$read(self).wrap_err(concat!("failed to read ", stringify!($type)));

                if res.is_ok() {
                    return res;
                }

                let pos = self.stream_position();
                if pos.is_ok() {
                    res.with_section(|| {
                        format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                    })
                } else {
                    res
                }
            }
        };
    }

    macro_rules! make_write {
        ($func:ident, $write:ident, $type:ty) => {
            fn $write(&mut self, val: $type) -> io::Result<()> {
                WriteBytesExt::$func::<LittleEndian>(self, val)
            }

            fn $func(&mut self, val: $type) -> Result<()> {
                let res = WriteExt::$write(self, val)
                    .wrap_err(concat!("failed to write ", stringify!($type)));

                if res.is_ok() {
                    return res;
                }

                let pos = self.stream_position();
                if pos.is_ok() {
                    res.with_section(|| {
                        format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                    })
                } else {
                    res
                }
            }
        };
    }

    macro_rules! make_skip {
        ($func:ident, $read:ident, $type:ty) => {
            fn $func(&mut self, cmp: $type) -> Result<()> {
                let val = ReadExt::$read(self)?;

                if val != cmp {
                    let pos = self.stream_position().unwrap_or(u64::MAX);
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

    pub trait ReadExt: ReadBytesExt + Seek {
        fn read_u8(&mut self) -> io::Result<u8> {
            ReadBytesExt::read_u8(self)
        }

        make_read!(read_u32, read_u32_le, u32);
        make_read!(read_u64, read_u64_le, u64);

        make_skip!(skip_u8, read_u8, u8);
        make_skip!(skip_u32, read_u32, u32);

        fn skip_padding(&mut self) -> io::Result<()> {
            let pos = self.stream_position()?;
            let padding_size = 16 - (pos % 16);

            if padding_size < 16 && padding_size > 0 {
                tracing::trace!(pos, padding_size, "Skipping padding");
                self.seek(SeekFrom::Current(padding_size as i64))?;
            } else {
                tracing::trace!(pos, padding_size, "No padding to skip");
            }

            Ok(())
        }
    }

    pub trait WriteExt: WriteBytesExt + Seek {
        make_write!(write_u32, write_u32_le, u32);
        make_write!(write_u64, write_u64_le, u64);

        fn write_padding(&mut self) -> io::Result<usize> {
            let pos = self.stream_position()?;
            let size = 16 - (pos % 16) as usize;

            tracing::trace!(padding_size = size, "Writing padding");

            if size > 0 && size < 16 {
                let buf = vec![0; size];
                self.write_all(&buf)?;
                Ok(size)
            } else {
                Ok(0)
            }
        }
    }

    impl<R: ReadBytesExt + Seek + ?Sized> ReadExt for R {}
    impl<W: WriteBytesExt + Seek + ?Sized> WriteExt for W {}

    pub(crate) fn _read_up_to<R>(r: &mut R, buf: &mut Vec<u8>) -> Result<usize>
    where
        R: Read + Seek,
    {
        let pos = r.stream_position()?;

        let err = {
            match r.read_exact(buf) {
                Ok(_) => return Ok(buf.len()),
                Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                    r.seek(SeekFrom::Start(pos))?;
                    match r.read_to_end(buf) {
                        Ok(read) => return Ok(read),
                        Err(err) => err,
                    }
                }
                Err(err) => err,
            }
        };

        Err(err).with_section(|| format!("{pos:#X} ({pos})").header("Position: "))
    }
}
