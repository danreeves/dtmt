#![allow(dead_code)]
use core::ffi::{c_char, c_int, c_size_t, c_ulonglong, c_void};

// Type definitions taken from Unreal Engine's `oodle2.h`

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_FuzzSafe {
    No = 0,
    Yes = 1,
}

impl From<bool> for OodleLZ_FuzzSafe {
    fn from(value: bool) -> Self {
        if value {
            Self::Yes
        } else {
            Self::No
        }
    }
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_CheckCRC {
    No = 0,
    Yes = 1,
    Force32 = 0x40000000,
}

impl From<bool> for OodleLZ_CheckCRC {
    fn from(value: bool) -> Self {
        if value {
            Self::Yes
        } else {
            Self::No
        }
    }
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_Verbosity {
    None = 0,
    Minimal = 1,
    Some = 2,
    Lots = 3,
    Force32 = 0x40000000,
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_Decode_ThreadPhase {
    Phase1 = 1,
    Phase2 = 2,
    PhaseAll = 3,
}

impl OodleLZ_Decode_ThreadPhase {
    pub const UNTHREADED: Self = OodleLZ_Decode_ThreadPhase::PhaseAll;
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_Compressor {
    Invalid = -1,
    // None = memcpy, pass through uncompressed bytes
    None = 3,

    // NEW COMPRESSORS:
    // Fast decompression and high compression ratios, amazing!
    Kraken = 8,
    // Leviathan = Kraken's big brother with higher compression, slightly slower decompression.
    Leviathan = 13,
    // Mermaid is between Kraken & Selkie - crazy fast, still decent compression.
    Mermaid = 9,
    // Selkie is a super-fast relative of Mermaid.  For maximum decode speed.
    Selkie = 11,
    // Hydra, the many-headed beast = Leviathan, Kraken, Mermaid, or Selkie (see $OodleLZ_About_Hydra)
    Hydra = 12,
    BitKnit = 10,
    // DEPRECATED but still supported
    Lzb16 = 4,
    Lzna = 7,
    Lzh = 0,
    Lzhlw = 1,
    Lznib = 2,
    Lzblw = 5,
    Lza = 6,
    Count = 14,
    Force32 = 0x40000000,
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum OodleLZ_CompressionLevel {
    // don't compress, just copy raw bytes
    None = 0,
    // super fast mode, lower compression ratio
    SuperFast = 1,
    // fastest LZ mode with still decent compression ratio
    VeryFast = 2,
    // fast - good for daily use
    Fast = 3,
    // standard medium speed LZ mode
    Normal = 4,
    // optimal parse level 1 (faster optimal encoder)
    Optimal1 = 5,
    // optimal parse level 2 (recommended baseline optimal encoder)
    Optimal2 = 6,
    // optimal parse level 3 (slower optimal encoder)
    Optimal3 = 7,
    // optimal parse level 4 (very slow optimal encoder)
    Optimal4 = 8,
    // optimal parse level 5 (don't care about encode speed, maximum compression)
    Optimal5 = 9,
    // faster than SuperFast, less compression
    HyperFast1 = -1,
    // faster than HyperFast1, less compression
    HyperFast2 = -2,
    // faster than HyperFast2, less compression
    HyperFast3 = -3,
    // fastest, less compression
    HyperFast4 = -4,
    Force32 = 0x40000000,
}

impl OodleLZ_CompressionLevel {
    // alias hyperfast base level
    pub const HYPERFAST: Self = OodleLZ_CompressionLevel::HyperFast1;
    // alias optimal standard level
    pub const OPTIMAL: Self = OodleLZ_CompressionLevel::Optimal2;
    // maximum compression level
    pub const MAX: Self = OodleLZ_CompressionLevel::Optimal5;
    // fastest compression level
    pub const MIN: Self = OodleLZ_CompressionLevel::HyperFast4;
    pub const INVALID: Self = OodleLZ_CompressionLevel::Force32;
}

#[allow(non_camel_case_types)]
pub type t_fp_OodleCore_Plugin_Printf =
    extern "C" fn(level: c_int, file: *const c_char, line: c_int, fmt: *const c_char);

#[allow(non_camel_case_types)]
pub type OodleLZ_Decompress = extern "C" fn(
    compressed_buffer: *const c_void,
    compressed_length: c_size_t,
    raw_buffer: *mut c_void,
    raw_length: c_size_t,
    fuzz_safe: OodleLZ_FuzzSafe,
    check_crc: OodleLZ_CheckCRC,
    verbosity: OodleLZ_Verbosity,
    decBufBase: *mut c_void,
    decBufSize: c_size_t,
    callback: *const c_void,
    callback_user_data: *const c_void,
    decoder_memory: *mut c_void,
    decoder_memory_size: c_size_t,
    thread_phase: OodleLZ_Decode_ThreadPhase,
) -> c_ulonglong;

#[allow(non_camel_case_types)]
pub type OodleLZ_Compress = extern "C" fn(
    compressor: OodleLZ_Compressor,
    raw_buffer: *const c_void,
    raw_len: c_size_t,
    compressed_buffer: *mut c_void,
    level: OodleLZ_CompressionLevel,
    options: *const c_void,
    dictionary_base: c_size_t,
    lrm: *const c_void,
    scratch_memory: *mut c_void,
    scratch_size: c_size_t,
) -> c_ulonglong;

#[allow(non_camel_case_types)]
pub type OodleLZ_GetDecodeBufferSize = extern "C" fn(
    compressor: OodleLZ_Compressor,
    raw_size: c_size_t,
    corruption_possible: bool,
) -> c_size_t;

#[allow(non_camel_case_types)]
pub type OodleCore_Plugins_SetPrintf =
    extern "C" fn(f: t_fp_OodleCore_Plugin_Printf) -> t_fp_OodleCore_Plugin_Printf;

#[allow(non_camel_case_types)]
pub type OodleCore_Plugin_Printf_Verbose = t_fp_OodleCore_Plugin_Printf;

#[allow(non_camel_case_types)]
pub type OodleCore_Plugin_Printf_Default = t_fp_OodleCore_Plugin_Printf;
