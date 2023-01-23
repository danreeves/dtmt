use std::io::{Cursor, Read, Seek, Write};

use color_eyre::Result;

use self::sync::{ReadExt, WriteExt};

pub trait FromBinary: Sized {
    fn from_binary<R: Read + Seek>(r: &mut R) -> Result<Self>;
}

pub trait ToBinary {
    fn to_binary(&self) -> Result<Vec<u8>>;
}

impl<T: ToBinary> ToBinary for Vec<T> {
    fn to_binary(&self) -> Result<Vec<u8>> {
        // TODO: Allocations for the vector could be optimized by first
        // serializing one value, then calculating the size from that.
        let mut bin = Cursor::new(Vec::new());
        bin.write_u32(self.len() as u32)?;

        for val in self.iter() {
            let buf = val.to_binary()?;
            bin.write_all(&buf)?;
        }

        Ok(bin.into_inner())
    }
}

impl<T: FromBinary> FromBinary for Vec<T> {
    fn from_binary<R: Read + Seek>(r: &mut R) -> Result<Self> {
        let size = r.read_u32()? as usize;

        let mut list = Vec::with_capacity(size);

        for _ in 0..size {
            list.push(T::from_binary(r)?);
        }

        Ok(list)
    }
}

pub mod sync {
    use std::io::{self, Read, Seek, SeekFrom};

    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
    use color_eyre::eyre::WrapErr;
    use color_eyre::{Help, Report, Result, SectionExt};

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

        fn read_string_len(&mut self, len: usize) -> Result<String> {
            let mut buf = vec![0; len];
            let res = self
                .read_exact(&mut buf)
                .map_err(Report::new)
                .and_then(|_| {
                    String::from_utf8(buf).map_err(|err| {
                        let ascii = String::from_utf8_lossy(err.as_bytes()).to_string();
                        let bytes = format!("{:?}", err.as_bytes());
                        Report::new(err)
                            .with_section(move || bytes.header("Bytes:"))
                            .with_section(move || ascii.header("ASCII:"))
                    })
                });

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
    }

    pub trait WriteExt: WriteBytesExt + Seek {
        fn write_u8(&mut self, val: u8) -> io::Result<()> {
            WriteBytesExt::write_u8(self, val)
        }

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
