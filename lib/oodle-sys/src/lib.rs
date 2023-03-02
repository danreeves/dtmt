#![feature(c_size_t)]
#![feature(once_cell)]

use std::ffi::OsStr;
use std::sync::OnceLock;

mod library;
mod types;

pub use library::Library;
pub use library::CHUNK_SIZE;
pub use types::*;

#[derive(thiserror::Error, Debug)]
pub enum OodleError {
    #[error("{0}")]
    Oodle(String),
    #[error(transparent)]
    Library(#[from] libloading::Error),
}

type Result<T> = std::result::Result<T, OodleError>;

static LIB: OnceLock<Library> = OnceLock::new();

/// Initialize the global library handle that this module's
/// functions operate on.
///
/// # Safety
///
/// The safety concerns as described by [`libloading::Library::new`] apply.
pub unsafe fn init<P: AsRef<OsStr>>(name: Option<P>) {
    let lib = match name {
        Some(name) => Library::with_name(name),
        None => Library::new(),
    };

    let lib = lib.expect("Failed to load library.");
    if LIB.set(lib).is_err() {
        panic!("Library was already initialized. Did you call `init` twice?");
    }
}

fn get() -> Result<&'static Library> {
    match LIB.get() {
        Some(lib) => Ok(lib),
        None => {
            let err = OodleError::Oodle(String::from("Library has not been initialized, yet."));
            Err(err)
        }
    }
}

pub fn decompress<I>(
    data: I,
    fuzz_safe: OodleLZ_FuzzSafe,
    check_crc: OodleLZ_CheckCRC,
) -> Result<Vec<u8>>
where
    I: AsRef<[u8]>,
{
    let lib = get()?;
    lib.decompress(data, fuzz_safe, check_crc)
}

pub fn compress<I>(data: I) -> Result<Vec<u8>>
where
    I: AsRef<[u8]>,
{
    let lib = get()?;
    lib.compress(data)
}

pub fn get_decode_buffer_size(raw_size: usize, corruption_possible: bool) -> Result<usize> {
    let lib = get()?;
    lib.get_decode_buffer_size(raw_size, corruption_possible)
}
