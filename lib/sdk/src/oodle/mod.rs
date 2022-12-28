use std::ffi::OsStr;
use std::ops::Deref;
use std::ptr;

use color_eyre::eyre;
use color_eyre::Result;
use libloading::{Library, Symbol};

pub mod types;
use types::*;

// Hardcoded chunk size of Bitsquid's bundle compression
pub const CHUNK_SIZE: usize = 512 * 1024;
pub const COMPRESSOR: OodleLZ_Compressor = OodleLZ_Compressor::Kraken;
pub const LEVEL: OodleLZ_CompressionLevel = OodleLZ_CompressionLevel::Optimal2;

pub struct Oodle {
    lib: Library,
}

impl Oodle {
    pub fn new<P>(lib: P) -> Result<Self>
    where
        P: AsRef<OsStr>,
    {
        let lib = unsafe { Library::new(lib)? };

        unsafe {
            let fun: Symbol<OodleCore_Plugins_SetPrintf> =
                lib.get(b"OodleCore_Plugins_SetPrintf\0")?;
            let printf: Symbol<t_fp_OodleCore_Plugin_Printf> =
                lib.get(b"OodleCore_Plugin_Printf_Verbose\0")?;

            fun(*printf.deref());
        }

        Ok(Self { lib })
    }

    #[tracing::instrument(name = "Oodle::decompress", skip(self, data))]
    pub fn decompress<I>(
        &self,
        data: I,
        fuzz_safe: OodleLZ_FuzzSafe,
        check_crc: OodleLZ_CheckCRC,
    ) -> Result<Vec<u8>>
    where
        I: AsRef<[u8]>,
    {
        let data = data.as_ref();
        let mut out = vec![0; CHUNK_SIZE];

        let verbosity = if tracing::enabled!(tracing::Level::INFO) {
            OodleLZ_Verbosity::Minimal
        } else if tracing::enabled!(tracing::Level::DEBUG) {
            OodleLZ_Verbosity::Some
        } else if tracing::enabled!(tracing::Level::TRACE) {
            OodleLZ_Verbosity::Lots
        } else {
            OodleLZ_Verbosity::None
        };

        let ret = unsafe {
            let decompress: Symbol<OodleLZ_Decompress> = self.lib.get(b"OodleLZ_Decompress\0")?;

            decompress(
                data.as_ptr() as *const _,
                data.len(),
                out.as_mut_ptr() as *mut _,
                out.len(),
                fuzz_safe,
                check_crc,
                verbosity,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                OodleLZ_Decode_ThreadPhase::UNTHREADED,
            )
        };

        if ret == 0 {
            eyre::bail!("Decompression failed.");
        }

        Ok(out)
    }

    #[tracing::instrument(name = "Oodle::compress", skip(self, data))]
    pub fn compress<I>(&self, data: I) -> Result<Vec<u8>>
    where
        I: AsRef<[u8]>,
    {
        let mut raw = Vec::from(data.as_ref());
        raw.resize(CHUNK_SIZE, 0);

        // TODO: Query oodle for buffer size
        let mut out = vec![0u8; CHUNK_SIZE];

        let ret = unsafe {
            let compress: Symbol<OodleLZ_Compress> = self.lib.get(b"OodleLZ_Compress\0")?;

            compress(
                COMPRESSOR,
                raw.as_ptr() as *const _,
                raw.len(),
                out.as_mut_ptr() as *mut _,
                LEVEL,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            )
        };

        tracing::debug!(compressed_size = ret, "Compressed chunk");

        if ret == 0 {
            eyre::bail!("Compression failed.");
        }

        out.resize(ret as usize, 0);

        Ok(out)
    }

    pub fn get_decode_buffer_size(
        &self,
        raw_size: usize,
        corruption_possible: bool,
    ) -> Result<usize> {
        unsafe {
            let f: Symbol<OodleLZ_GetDecodeBufferSize> =
                self.lib.get(b"OodleLZ_GetDecodeBufferSize\0")?;

            let size = f(COMPRESSOR, raw_size, corruption_possible);
            Ok(size)
        }
    }
}
