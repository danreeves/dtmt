use std::{ffi::OsStr, ptr};

use libloading::Symbol;

use super::Result;
use crate::{types::*, OodleError};

// Hardcoded chunk size of Bitsquid's bundle compression
pub const CHUNK_SIZE: usize = 512 * 1024;
pub const COMPRESSOR: OodleLZ_Compressor = OodleLZ_Compressor::Kraken;
pub const LEVEL: OodleLZ_CompressionLevel = OodleLZ_CompressionLevel::Optimal2;

#[cfg(target_os = "windows")]
const OODLE_LIB_NAME: &str = "oo2core_8_win64";

#[cfg(target_os = "linux")]
const OODLE_LIB_NAME: &str = "liboo2corelinux64.so";

pub struct Library {
    inner: libloading::Library,
}

impl Library {
    /// Load the Oodle library by its default name.
    ///
    /// The default name is platform-specific:
    /// - Windows: `oo2core_8_win64`
    /// - Linux: `liboo2corelinux64.so`
    ///
    /// # Safety
    ///
    /// The safety concerns as described by [`libloading::Library::new`] apply.
    pub unsafe fn new() -> Result<Self> {
        Self::with_name(OODLE_LIB_NAME)
    }

    /// Load the Oodle library by the given name or path.
    ///
    /// See [`libloading::Library::new`] for how the `name` parameter is handled.
    ///
    /// # Safety
    ///
    /// The safety concerns as described by [`libloading::Library::new`] apply.
    pub unsafe fn with_name<P: AsRef<OsStr>>(name: P) -> Result<Self> {
        let inner = libloading::Library::new(name)?;
        Ok(Self { inner })
    }

    #[tracing::instrument(skip(self, data))]
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
            let decompress: Symbol<OodleLZ_Decompress> = self.inner.get(b"OodleLZ_Decompress\0")?;

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
            let err = OodleError::Oodle(String::from("Decompression failed."));
            return Err(err);
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
            let compress: Symbol<OodleLZ_Compress> = self.inner.get(b"OodleLZ_Compress\0")?;

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
            let err = OodleError::Oodle(String::from("Compression failed."));
            return Err(err);
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
                self.inner.get(b"OodleLZ_GetDecodeBufferSize\0")?;

            let size = f(COMPRESSOR, raw_size, corruption_possible);
            Ok(size)
        }
    }
}
