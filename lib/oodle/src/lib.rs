#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::ptr;

use color_eyre::{Result, eyre};

#[allow(dead_code)]
#[allow(clippy::identity_op)]
mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

// Hardcoded chunk size of Bitsquid's bundle compression
pub const CHUNK_SIZE: usize = 512 * 1024;
pub const COMPRESSOR: bindings::OodleLZ_Compressor =
    bindings::OodleLZ_Compressor_OodleLZ_Compressor_Kraken;
pub const LEVEL: bindings::OodleLZ_CompressionLevel =
    bindings::OodleLZ_CompressionLevel_OodleLZ_CompressionLevel_Optimal2;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OodleLZ_FuzzSafe {
    Yes,
    No,
}

impl From<OodleLZ_FuzzSafe> for bindings::OodleLZ_FuzzSafe {
    fn from(value: OodleLZ_FuzzSafe) -> Self {
        match value {
            OodleLZ_FuzzSafe::Yes => bindings::OodleLZ_FuzzSafe_OodleLZ_FuzzSafe_Yes,
            OodleLZ_FuzzSafe::No => bindings::OodleLZ_FuzzSafe_OodleLZ_FuzzSafe_No,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OodleLZ_CheckCRC {
    Yes,
    No,
}

impl From<OodleLZ_CheckCRC> for bindings::OodleLZ_CheckCRC {
    fn from(value: OodleLZ_CheckCRC) -> Self {
        match value {
            OodleLZ_CheckCRC::Yes => bindings::OodleLZ_CheckCRC_OodleLZ_CheckCRC_Yes,
            OodleLZ_CheckCRC::No => bindings::OodleLZ_CheckCRC_OodleLZ_CheckCRC_No,
        }
    }
}

#[tracing::instrument(skip(data))]
pub fn decompress<I>(
    data: I,
    out_size: usize,
    fuzz_safe: OodleLZ_FuzzSafe,
    check_crc: OodleLZ_CheckCRC,
) -> Result<Vec<u8>>
where
    I: AsRef<[u8]>,
{
    let data = data.as_ref();
    let mut out = vec![0; out_size];

    let verbosity = if tracing::enabled!(tracing::Level::INFO) {
        bindings::OodleLZ_Verbosity_OodleLZ_Verbosity_Minimal
    } else if tracing::enabled!(tracing::Level::DEBUG) {
        bindings::OodleLZ_Verbosity_OodleLZ_Verbosity_Some
    } else if tracing::enabled!(tracing::Level::TRACE) {
        bindings::OodleLZ_Verbosity_OodleLZ_Verbosity_Lots
    } else {
        bindings::OodleLZ_Verbosity_OodleLZ_Verbosity_None
    };

    let ret = unsafe {
        bindings::OodleLZ_Decompress(
            data.as_ptr() as *const _,
            data.len() as isize,
            out.as_mut_ptr() as *mut _,
            out.len() as isize,
            fuzz_safe.into(),
            check_crc.into(),
            verbosity,
            ptr::null_mut(),
            0,
            None,
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            bindings::OodleLZ_Decode_ThreadPhase_OodleLZ_Decode_Unthreaded,
        )
    };

    if ret == 0 {
        eyre::bail!("Decompression failed");
    }

    Ok(out)
}

#[tracing::instrument(skip(data))]
pub fn compress<I>(data: I) -> Result<Vec<u8>>
where
    I: AsRef<[u8]>,
{
    let mut raw = Vec::from(data.as_ref());
    raw.resize(CHUNK_SIZE, 0);

    // Bundle chunks are padded to `CHUNK_SIZE`, but their compressed form may be
    // slightly larger for incompressible data, so leave headroom.
    let mut out = vec![0u8; CHUNK_SIZE + CHUNK_SIZE / 4 + 0x10000];

    let ret = unsafe {
        bindings::OodleLZ_Compress(
            COMPRESSOR,
            raw.as_ptr() as *const _,
            raw.len() as isize,
            out.as_mut_ptr() as *mut _,
            LEVEL,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };

    tracing::debug!(compressed_size = ret, "Compressed chunk");

    if ret == 0 {
        eyre::bail!("Compression failed");
    }

    out.resize(ret as usize, 0);

    Ok(out)
}

#[tracing::instrument(skip(data))]
pub fn compress_exact<I>(data: I) -> Result<Vec<u8>>
where
    I: AsRef<[u8]>,
{
    let raw = data.as_ref();

    // Unlike bundle chunks, textures are stored as a single Oodle block whose
    // decompressed length must be preserved exactly. So we neither pad nor
    // truncate the input.
    //
    // Oodle's own `GetCompressedBufferSizeNeeded` does not reliably match the
    // runtime library, so use a conservative bound instead.
    let out_size = raw.len() + raw.len() / 4 + 0x10000;

    let mut out = vec![0u8; out_size];

    let ret = unsafe {
        bindings::OodleLZ_Compress(
            COMPRESSOR,
            raw.as_ptr() as *const _,
            raw.len() as isize,
            out.as_mut_ptr() as *mut _,
            LEVEL,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };

    tracing::debug!(
        raw_size = raw.len(),
        compressed_size = ret,
        "Compressed data as a single block"
    );

    if ret == 0 {
        eyre::bail!("Compression failed");
    }

    out.resize(ret as usize, 0);

    Ok(out)
}

pub fn get_decode_buffer_size(raw_size: usize, corruption_possible: bool) -> Result<usize> {
    let size = unsafe {
        bindings::OodleLZ_GetDecodeBufferSize(
            COMPRESSOR,
            raw_size as isize,
            if corruption_possible { 1 } else { 0 },
        )
    };
    Ok(size as usize)
}
