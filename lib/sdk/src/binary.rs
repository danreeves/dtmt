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
    use std::ffi::CStr;
    use std::io::{self, Read, Seek, SeekFrom, Write};

    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
    use color_eyre::eyre::{self, WrapErr};
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

    pub trait ReadExt: Read + Seek {
        fn read_u8(&mut self) -> io::Result<u8> {
            ReadBytesExt::read_u8(self)
        }

        make_read!(read_u32, read_u32_le, u32);
        make_read!(read_u64, read_u64_le, u64);

        make_skip!(skip_u32, read_u32, u32);

        // Implementation based on https://en.wikipedia.com/wiki/LEB128
        fn read_uleb128(&mut self) -> io::Result<u64> {
            let mut result: u64 = 0;
            let mut shift: u64 = 0;

            loop {
                let byte = ReadExt::read_u8(self)? as u64;
                result |= (byte & 0x7f) << shift;

                if byte < 0x80 {
                    return Ok(result);
                }

                shift += 7;
            }
        }

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
            let pos = self.stream_position();

            let res = read_string_len(self, len);
            if res.is_ok() {
                return res;
            }

            if pos.is_ok() {
                res.with_section(|| {
                    format!("{pos:#X} ({pos})", pos = pos.unwrap()).header("Position: ")
                })
            } else {
                res
            }
        }

        fn read_bool(&mut self) -> Result<bool> {
            match ReadExt::read_u8(self)? {
                0 => Ok(false),
                1 => Ok(true),
                v => eyre::bail!("Invalid value for boolean '{}'", v),
            }
        }
    }

    pub trait WriteExt: Write + Seek {
        fn write_u8(&mut self, val: u8) -> io::Result<()> {
            WriteBytesExt::write_u8(self, val)
        }

        make_write!(write_u32, write_u32_le, u32);
        make_write!(write_u64, write_u64_le, u64);

        fn write_bool(&mut self, val: bool) -> io::Result<()> {
            WriteBytesExt::write_u8(self, if val { 1 } else { 0 })
        }

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

    impl<R: Read + Seek + ?Sized> ReadExt for R {}
    impl<W: Write + Seek + ?Sized> WriteExt for W {}

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

    fn read_string_len(mut r: impl Read, len: usize) -> Result<String> {
        let mut buf = vec![0; len];
        r.read_exact(&mut buf)
            .wrap_err_with(|| format!("Failed to read {} bytes", len))?;

        let res = match CStr::from_bytes_until_nul(&buf) {
            Ok(s) => {
                let s = s.to_str()?;
                Ok(s.to_string())
            }
            Err(_) => String::from_utf8(buf.clone()).map_err(Report::new),
        };

        res.wrap_err("Invalid binary for UTF8 string")
            .with_section(|| format!("{}", String::from_utf8_lossy(&buf)).header("ASCI:"))
            .with_section(|| format!("{:x?}", buf).header("Bytes:"))
    }
}
