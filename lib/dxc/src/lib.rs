//! In-process DXC: compiling through `dxcompiler.dll` instead of `dxc.exe`.
//!
//! The shell-out writes a temporary HLSL file and a temporary container per
//! compile and spawns a process; `dxc.exe` has no stdin mode (checked: it
//! answers `Required input file argument is missing` and does not accept `-`),
//! so there is no way around the files with the executable. The DLL is the same
//! compiler the executable wraps, and calling it directly keeps the source and
//! the container in memory.
//!
//! The binding is a hand-written `extern "system"` vtable for the four
//! interfaces the call needs - `IDxcCompiler3`, `IDxcResult`,
//! `IDxcOperationResult` and `IDxcBlob` - transcribed from the Windows SDK's
//! `dxcapi.h` (10.0.22621.0): the CLSIDs and IIDs are the header's, and the
//! method order is the header's. `DxcCreateInstance` is the only export used.
//!
//! The DLL is loaded at runtime (no import library, no build dependency) and
//! searched for in `DTMT_DXC_DLL`, next to the tool, next to the configured
//! `dxc.exe`, and in the newest Windows SDK installation. A caller that cannot
//! find it falls back to the executable.

#![cfg(windows)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use color_eyre::eyre::{Context as _, Result, bail};

type Hresult = i32;

/// A COM GUID, in the header's field order.
#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const CLSID_DXC_COMPILER: Guid = Guid {
    data1: 0x73e22d93,
    data2: 0xe6ce,
    data3: 0x47f3,
    data4: [0xb5, 0xbf, 0xf0, 0x66, 0x4f, 0x39, 0xc1, 0xb0],
};

const IID_IDXC_COMPILER3: Guid = Guid {
    data1: 0x228b4687,
    data2: 0x5a6a,
    data3: 0x4730,
    data4: [0x90, 0x0c, 0x97, 0x02, 0xb2, 0x20, 0x3f, 0x54],
};

const IID_IDXC_RESULT: Guid = Guid {
    data1: 0x58346cda,
    data2: 0xdde7,
    data3: 0x4497,
    data4: [0x94, 0x61, 0x6f, 0x87, 0xaf, 0x5e, 0x06, 0x59],
};

const IID_IDXC_BLOB: Guid = Guid {
    data1: 0x8ba5fb08,
    data2: 0x5195,
    data3: 0x40e2,
    data4: [0xac, 0x58, 0x0d, 0x98, 0x9c, 0x3a, 0x01, 0x02],
};

/// `DXC_OUT_OBJECT`: the compiled shader or library object.
const DXC_OUT_OBJECT: u32 = 1;
/// `DXC_CP_UTF8`: the source is UTF-8.
const DXC_CP_UTF8: u32 = 65001;

/// `DxcBuffer`: the source text, its size and its encoding.
#[repr(C)]
struct DxcBuffer {
    ptr: *const c_void,
    size: usize,
    encoding: u32,
}

#[repr(C)]
struct IUnknownVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct IDxcBlobVtbl {
    base: IUnknownVtbl,
    get_buffer_pointer: unsafe extern "system" fn(*mut c_void) -> *mut c_void,
    get_buffer_size: unsafe extern "system" fn(*mut c_void) -> usize,
}

#[repr(C)]
struct IDxcBlob {
    vtable: *const IDxcBlobVtbl,
}

#[repr(C)]
struct IDxcResultVtbl {
    base: IUnknownVtbl,
    get_status: unsafe extern "system" fn(*mut c_void, *mut Hresult) -> Hresult,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hresult,
    get_error_buffer: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hresult,
    has_output: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    get_output: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *const Guid,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> Hresult,
    get_num_outputs: unsafe extern "system" fn(*mut c_void) -> u32,
    get_output_by_index: unsafe extern "system" fn(*mut c_void, u32) -> u32,
    primary_output: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct IDxcResult {
    vtable: *const IDxcResultVtbl,
}

#[repr(C)]
struct IDxcCompiler3Vtbl {
    base: IUnknownVtbl,
    compile: unsafe extern "system" fn(
        *mut c_void,
        *const DxcBuffer,
        *const *const u16,
        u32,
        *mut c_void,
        *const Guid,
        *mut *mut c_void,
    ) -> Hresult,
    disassemble:
        unsafe extern "system" fn(*mut c_void, *const DxcBuffer, *const Guid, *mut *mut c_void)
            -> Hresult,
}

#[repr(C)]
struct IDxcCompiler3 {
    vtable: *const IDxcCompiler3Vtbl,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

/// A loaded `dxcompiler.dll`.
pub struct Compiler {
    module: *mut c_void,
    create_instance:
        unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> Hresult,
}

impl Compiler {
    /// Loads the DLL and resolves `DxcCreateInstance`.
    pub fn load(path: &Path) -> Result<Self> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            bail!("Failed to load '{}'", path.display());
        }

        let address = unsafe { GetProcAddress(module, c"DxcCreateInstance".as_ptr() as *const u8) };
        if address.is_null() {
            unsafe { FreeLibrary(module) };
            bail!("'{}' has no DxcCreateInstance", path.display());
        }
        let create_instance = unsafe {
            std::mem::transmute::<
                *mut c_void,
                unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> Hresult,
            >(address)
        };

        Ok(Self {
            module,
            create_instance,
        })
    }

    /// Compiles one source for a profile and entry point, in process.
    pub fn compile(&self, source: &str, profile: &str, entry: &str) -> Result<Vec<u8>> {
        let mut compiler: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            (self.create_instance)(&CLSID_DXC_COMPILER, &IID_IDXC_COMPILER3, &mut compiler)
        };
        if hr < 0 || compiler.is_null() {
            bail!("DxcCreateInstance failed ({hr:#010x})");
        }
        let compiler = compiler as *mut IDxcCompiler3;

        let result = unsafe { self.compile_with(compiler, source, profile, entry) };

        unsafe {
            ((*(*compiler).vtable).base.release)(compiler as *mut c_void);
        }
        result
    }

    unsafe fn compile_with(
        &self,
        compiler: *mut IDxcCompiler3,
        source: &str,
        profile: &str,
        entry: &str,
    ) -> Result<Vec<u8>> {
        let target = wide(profile);
        let entry_point = wide(entry);
        let flag_target = wide("-T");
        let flag_entry = wide("-E");
        let arguments: [*const u16; 4] = [
            flag_target.as_ptr(),
            target.as_ptr(),
            flag_entry.as_ptr(),
            entry_point.as_ptr(),
        ];

        let buffer = DxcBuffer {
            ptr: source.as_ptr() as *const c_void,
            size: source.len(),
            encoding: DXC_CP_UTF8,
        };

        let mut result: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*compiler).vtable).compile)(
                compiler as *mut c_void,
                &buffer,
                arguments.as_ptr(),
                arguments.len() as u32,
                ptr::null_mut(),
                &IID_IDXC_RESULT,
                &mut result,
            )
        };
        if hr < 0 || result.is_null() {
            bail!("IDxcCompiler3::Compile failed ({hr:#010x})");
        }
        let result = result as *mut IDxcResult;
        let vtable = unsafe { (*result).vtable };

        let mut status: Hresult = 0;
        unsafe { ((*vtable).get_status)(result as *mut c_void, &mut status) };
        if status < 0 {
            let text = unsafe { error_text(result, vtable) };
            unsafe { ((*vtable).base.release)(result as *mut c_void) };
            bail!("dxcompiler failed as {profile}/{entry}:\n{text}");
        }

        let mut blob: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*vtable).get_output)(
                result as *mut c_void,
                DXC_OUT_OBJECT,
                &IID_IDXC_BLOB,
                &mut blob,
                ptr::null_mut(),
            )
        };
        if hr < 0 || blob.is_null() {
            unsafe { ((*vtable).base.release)(result as *mut c_void) };
            bail!("dxcompiler returned no object output ({hr:#010x})");
        }

        let blob = blob as *mut IDxcBlob;
        let bytes = unsafe {
            let blob_vtable = (*blob).vtable;
            let pointer = ((*blob_vtable).get_buffer_pointer)(blob as *mut c_void) as *const u8;
            let size = ((*blob_vtable).get_buffer_size)(blob as *mut c_void);
            std::slice::from_raw_parts(pointer, size).to_vec()
        };

        unsafe {
            (((*(*blob).vtable).base).release)(blob as *mut c_void);
            ((*vtable).base.release)(result as *mut c_void);
        }
        Ok(bytes)
    }
}

impl Drop for Compiler {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.module) };
    }
}

/// The error text of a failed result, as UTF-8.
unsafe fn error_text(result: *mut IDxcResult, vtable: *const IDxcResultVtbl) -> String {
    let mut errors: *mut c_void = ptr::null_mut();
    let hr = unsafe { ((*vtable).get_error_buffer)(result as *mut c_void, &mut errors) };
    if hr < 0 || errors.is_null() {
        return "[no error text]".to_string();
    }

    let errors = errors as *mut IDxcBlob;
    let text = unsafe {
        let blob_vtable = (*errors).vtable;
        let pointer = ((*blob_vtable).get_buffer_pointer)(errors as *mut c_void) as *const u8;
        let size = ((*blob_vtable).get_buffer_size)(errors as *mut c_void);
        String::from_utf8_lossy(std::slice::from_raw_parts(pointer, size)).into_owned()
    };
    unsafe {
        (((*(*errors).vtable).base).release)(errors as *mut c_void);
    }
    text
}

/// A NUL-terminated UTF-16 copy of `text`.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The `dxcompiler.dll` to use with a configured `dxc.exe`: the environment
/// variable, the tool's own directory, the executable's directory, then the
/// newest Windows SDK installation.
pub fn find_dll(dxc: &Path) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DTMT_DXC_DLL") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();
    if let Some(dir) = dxc.parent() {
        candidates.push(dir.join("dxcompiler.dll"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("dxcompiler.dll"));
    }

    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(kits) {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path().join("x64").join("dxcompiler.dll"))
            .filter(|path| path.exists())
            .collect();
        versions.sort();
        candidates.extend(versions.into_iter().rev());
    }

    candidates.into_iter().find(|path| path.exists())
}
