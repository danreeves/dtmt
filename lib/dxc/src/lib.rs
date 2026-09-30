//! In-process DXC: compiling through the DXC library instead of a subprocess.
//!
//! The binding is a hand-written `extern "system"` vtable for the four
//! interfaces the call needs - `IDxcCompiler3`, `IDxcResult`,
//! `IDxcOperationResult` and `IDxcBlob` - transcribed from the Windows SDK's
//! `dxcapi.h` (10.0.22621.0): the CLSIDs and IIDs are the header's, and the
//! method order is the header's. `DxcCreateInstance` is the only export used.
//!
//! The library is the same compiler `dxc.exe` wraps, so calling it directly
//! keeps the source and the container in memory - no temporary files, no
//! process per compile - and it works wherever DXC is available: Windows, and
//! Linux (the `dxcompiler.dll` and `libdxcompiler.so` builds Microsoft
//! publishes, or a distro package).
//!
//! The **validator library is required too**: `IDxcValidator` lives in
//! `dxil.dll` (`libdxil.so`), which validates and signs the DXIL. D3D12 refuses
//! to create a pipeline state from unsigned DXIL with `E_INVALIDARG`, and the
//! compiler's API leaves the container unsigned (the `dxc.exe` path signs
//! through the `dxil.dll` beside it for the same reason). [`Compiler::load`]
//! therefore loads the validator from beside the compiler library, and a
//! compile without it fails with a message naming the file.
//!
//! # Where the libraries go
//!
//! [`find_library`] searches, in order:
//!
//! 1. the `dxc` path in `dtmt.cfg`, when set (a file, or the directory holding
//!    it) - [`set_library`] pins it for the process;
//! 2. the `DTMT_DXC_DLL` environment variable (the same: a file or a directory);
//! 3. next to the `dtmt` executable;
//! 4. Windows: the newest Windows SDK installation
//!    (`C:\Program Files (x86)\Windows Kits\10\bin\<version>\x64\dxcompiler.dll`);
//!    Linux: `/opt/dxc/lib` and `/usr/lib/dxc`, then the system loader's own
//!    paths, by name.
//!
//! The validator is looked up beside whichever compiler library was found, so
//! both files travel together.
//!
//! # Linux, and Linux with Proton
//!
//! The game runs under Proton, and DTMT is the Windows build in that setup, so
//! the Windows branch above applies: drop `dxcompiler.dll` and `dxil.dll` next
//! to `dtmt.exe` (Wine searches the program's directory first), or set `dxc` in
//! `dtmt.cfg` to the directory holding them. The Windows SDK fallback does not
//! exist inside a Wine prefix, so the two DLLs are the reliable arrangement.
//!
//! A native Linux build loads `libdxcompiler.so` and `libdxil.so` instead -
//! unpack the DXC release beside the tool, or install the distro package. Both
//! names are the DXC release's own.
//!
//! A machine without the compiler library cannot compile shaders; the error
//! names the library and every place that was searched.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;

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

const CLSID_DXC_VALIDATOR: Guid = Guid {
    data1: 0x8ca3e215,
    data2: 0xf728,
    data3: 0x4cf3,
    data4: [0x8c, 0xdd, 0x88, 0xaf, 0x91, 0x75, 0x87, 0xa1],
};

const IID_IDXC_VALIDATOR: Guid = Guid {
    data1: 0xa6e82bd2,
    data2: 0x1fd7,
    data3: 0x4826,
    data4: [0x98, 0x11, 0x28, 0x57, 0xe7, 0x97, 0xf4, 0x9a],
};

const CLSID_DXC_UTILS: Guid = Guid {
    data1: 0x6245d6af,
    data2: 0x66e0,
    data3: 0x48fd,
    data4: [0x80, 0xb4, 0x4d, 0x27, 0x17, 0x96, 0x74, 0x8c],
};

const IID_IDXC_UTILS: Guid = Guid {
    data1: 0x4605c4cb,
    data2: 0x2019,
    data3: 0x492a,
    data4: [0xad, 0xa4, 0x65, 0xf2, 0x0b, 0xb7, 0xd6, 0x7f],
};

/// `IID_ID3D12ShaderReflection`: the reflection interface
/// `IDxcUtils::CreateReflection` hands back.
const IID_ID3D12_SHADER_REFLECTION: Guid = Guid {
    data1: 0x5a58_797d,
    data2: 0xa72c,
    data3: 0x478d,
    data4: [0x8b, 0xa2, 0xef, 0xc6, 0xb0, 0xef, 0xe8, 0x8e],
};

/// `DXC_OUT_OBJECT`: the compiled shader or library object.
const DXC_OUT_OBJECT: u32 = 1;
/// `DXC_CP_UTF8`: the source is UTF-8.
const DXC_CP_UTF8: u32 = 65001;
/// `DxcValidatorFlags_InPlaceEdit`: the validator edits the input blob in place.
const DXC_VALIDATOR_FLAGS_IN_PLACE_EDIT: u32 = 1;

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

#[repr(C)]
struct IDxcValidatorVtbl {
    base: IUnknownVtbl,
    validate: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut *mut c_void) -> Hresult,
}

#[repr(C)]
struct IDxcValidator {
    vtable: *const IDxcValidatorVtbl,
}

/// The `IDxcUtils` vtable, up to `CreateReflection` (the methods never called
/// stand in as opaque slots so the offsets line up with the header's order).
#[repr(C)]
struct IDxcUtilsVtbl {
    base: IUnknownVtbl,
    create_blob_from_blob: *const c_void,
    create_blob_from_pinned: *const c_void,
    move_to_blob: *const c_void,
    create_blob: unsafe extern "system" fn(
        *mut c_void,
        *const c_void,
        u32,
        u32,
        *mut *mut c_void,
    ) -> Hresult,
    load_file: *const c_void,
    create_read_only_stream_from_blob: *const c_void,
    create_default_include_handler: *const c_void,
    get_blob_as_utf8: *const c_void,
    get_blob_as_utf16: *const c_void,
    get_dxil_container_part: *const c_void,
    create_reflection: unsafe extern "system" fn(
        *mut c_void,
        *const DxcBuffer,
        *const Guid,
        *mut *mut c_void,
    ) -> Hresult,
}

#[repr(C)]
struct IDxcUtils {
    vtable: *const IDxcUtilsVtbl,
}

/// `D3D12_SHADER_INPUT_BIND_DESC`, as `d3d12shader.h` lays it out.
#[repr(C)]
#[derive(Default)]
struct ShaderInputBindDesc {
    name: *const u8,
    kind: u32,
    bind_point: u32,
    bind_count: u32,
    flags: u32,
    return_type: u32,
    dimension: u32,
    samples: u32,
    space: u32,
    id: u32,
}

/// The `ID3D12ShaderReflection` methods this crate calls, in the header's
/// order: `GetDesc` then the two constant-buffer getters (never called, kept
/// for the offsets) then `GetResourceBindingDesc`.
#[repr(C)]
struct ID3D12ShaderReflectionVtbl {
    base: IUnknownVtbl,
    get_desc: *const c_void,
    get_constant_buffer_by_index: *const c_void,
    get_constant_buffer_by_name: *const c_void,
    get_resource_binding_desc: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *mut ShaderInputBindDesc,
    ) -> Hresult,
}

#[repr(C)]
struct ID3D12ShaderReflection {
    vtable: *const ID3D12ShaderReflectionVtbl,
}

/// One resource a shader binds, as the reflection reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundResource {
    /// The name the HLSL gave it.
    pub name: String,
    /// `D3D_SHADER_INPUT_TYPE`: 0 cbuffer, 2 texture, 3 sampler, 4+ UAV kinds.
    pub kind: u32,
    /// The register the resource binds at.
    pub bind_point: u32,
    /// The number of contiguous registers (1, or the array's size).
    pub bind_count: u32,
    /// `D3D_SIF_*` flags.
    pub flags: u32,
    /// The register space.
    pub space: u32,
}

/// The `DxcCreateInstance` export's signature.
type CreateInstance =
    unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> Hresult;

/// The library's name on this platform. Windows covers Proton and Wine too:
/// they run the Windows build and its DLLs.
pub const LIBRARY_NAME: &str = if cfg!(windows) {
    "dxcompiler.dll"
} else {
    "libdxcompiler.so"
};

/// The validator's library name on this platform. `IDxcValidator` lives in
/// `dxil.dll`, not in the compiler library: it validates and signs the DXIL,
/// and D3D12 rejects unsigned DXIL with `E_INVALIDARG`.
pub const VALIDATOR_NAME: &str = if cfg!(windows) {
    "dxil.dll"
} else {
    "libdxil.so"
};

/// The path [`set_library`] pinned, if any.
static LIBRARY_PATH: OnceLock<PathBuf> = OnceLock::new();
/// The shared compiler, loaded on first use. The error is cached with it so a
/// missing library reports once and consistently.
static COMPILER: OnceLock<Result<Compiler, String>> = OnceLock::new();

/// Pins the library path to use; the first call wins. `dtmt` passes its
/// `dtmt.cfg` path here before compiling, so the config takes precedence over
/// the other search places.
pub fn set_library(path: impl Into<PathBuf>) {
    let _ = LIBRARY_PATH.set(path.into());
}

/// The DXC library to load: the explicit path or [`set_library`]'s, then
/// `DTMT_DXC_DLL`, then the tool's directory, then the platform's usual places.
pub fn find_library(explicit: Option<&Path>) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    let mut push = |path: PathBuf| {
        if path.is_dir() {
            candidates.push(path.join(LIBRARY_NAME));
        } else {
            candidates.push(path);
        }
    };

    if let Some(path) = explicit {
        push(path.to_path_buf());
    }
    if let Ok(path) = std::env::var("DTMT_DXC_DLL") {
        push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(LIBRARY_NAME));
    }
    #[cfg(windows)]
    {
        let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
        if let Ok(entries) = std::fs::read_dir(kits) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path().join("x64").join(LIBRARY_NAME))
                .filter(|path| path.is_file())
                .collect();
            versions.sort();
            candidates.extend(versions.into_iter().rev());
        }
    }
    if !cfg!(windows) {
        // The DXC release unpacks flat, and `/usr/lib/dxc` is the usual place a
        // hand install lands; the loader covers the distro packages.
        for dir in ["/opt/dxc/lib", "/usr/lib/dxc", "/usr/local/lib/dxc"] {
            candidates.push(Path::new(dir).join(LIBRARY_NAME));
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    // Let the system loader search its own paths for the bare name: the distro
    // packages and the DXC release archives install it where it is found.
    if unsafe { libloading::Library::new(LIBRARY_NAME) }.is_ok() {
        return Ok(PathBuf::from(LIBRARY_NAME));
    }

    let searched: Vec<String> = candidates
        .iter()
        .map(|path| path.display().to_string())
        .chain([LIBRARY_NAME.to_string()])
        .collect();
    bail!(
        "Could not find the DXC library '{LIBRARY_NAME}'. Put it next to the tool, set \
         `dxc` in dtmt.cfg, or set DTMT_DXC_DLL. Searched: {}",
        searched.join(", ")
    )
}

/// Compiles one source for a profile and entry point with the shared compiler,
/// loading the library on first use.
pub fn compile(source: &str, profile: &str, entry: &str) -> Result<Vec<u8>> {
    let compiler = COMPILER.get_or_init(|| {
        find_library(LIBRARY_PATH.get().map(PathBuf::as_path))
            .and_then(|path| Compiler::load(&path))
            .map_err(|err| err.to_string())
    });
    match compiler {
        Ok(compiler) => compiler.compile(source, profile, entry),
        Err(err) => bail!("{err}"),
    }
}

/// Reflects a compiled container with the shared compiler, loading the library
/// on first use: the resources it binds, with the names, registers and spaces
/// the compiler recorded - per stage, without reading the source.
pub fn reflect(container: &[u8]) -> Result<Vec<BoundResource>> {
    let compiler = COMPILER.get_or_init(|| {
        find_library(LIBRARY_PATH.get().map(PathBuf::as_path))
            .and_then(|path| Compiler::load(&path))
            .map_err(|err| err.to_string())
    });
    match compiler {
        Ok(compiler) => compiler.reflect(container),
        Err(err) => bail!("{err}"),
    }
}

/// A loaded DXC library.
pub struct Compiler {
    /// Held so the library stays loaded for the compiler's lifetime.
    _library: libloading::Library,
    create_instance: CreateInstance,
    /// The validator library (`dxil.dll`), when it sits next to the compiler.
    _validator_library: Option<libloading::Library>,
    /// `DxcCreateInstance` of the validator library.
    validate_instance: Option<CreateInstance>,
}

impl Compiler {
    /// Loads the library and resolves `DxcCreateInstance`, and the validator
    /// library beside it when present.
    pub fn load(path: &Path) -> Result<Self> {
        let library = unsafe { libloading::Library::new(path) }
            .wrap_err_with(|| format!("Failed to load '{}'", path.display()))?;
        let create_instance: CreateInstance = unsafe {
            *library
                .get(b"DxcCreateInstance\0")
                .wrap_err_with(|| format!("'{}' has no DxcCreateInstance", path.display()))?
        };

        // The validator is `dxil.dll`; without it the DXIL stays unsigned and
        // D3D12 refuses to create a pipeline state from it.
        let validator = validator_path(path);
        let (validator_library, validate_instance) =
            match unsafe { libloading::Library::new(&validator) } {
                Ok(library) => {
                    let create: CreateInstance = unsafe {
                        *library.get(b"DxcCreateInstance\0").wrap_err_with(|| {
                            format!("'{}' has no DxcCreateInstance", validator.display())
                        })?
                    };
                    (Some(library), Some(create))
                }
                Err(_) => (None, None),
            };

        Ok(Self {
            _library: library,
            create_instance,
            _validator_library: validator_library,
            validate_instance,
        })
    }

    /// Compiles one source for a profile and entry point, in process.
    pub fn compile(&self, source: &str, profile: &str, entry: &str) -> Result<Vec<u8>> {        let mut compiler: *mut c_void = ptr::null_mut();
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

    /// The resources a compiled container binds, through the compiler's own
    /// reflection (`IDxcUtils::CreateReflection`): per stage, names, kinds,
    /// registers and spaces - the source the engine's tail lists derive from,
    /// without reading the HLSL text.
    pub fn reflect(&self, container: &[u8]) -> Result<Vec<BoundResource>> {
        let mut utils: *mut c_void = ptr::null_mut();
        let hr = unsafe { (self.create_instance)(&CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut utils) };
        if hr < 0 || utils.is_null() {
            bail!("DxcCreateInstance(IDxcUtils) failed ({hr:#010x})");
        }
        let utils = utils as *mut IDxcUtils;

        // A container is raw binary: the encoding is 0, not a code page.
        let buffer = DxcBuffer {
            ptr: container.as_ptr() as *const c_void,
            size: container.len(),
            encoding: 0,
        };
        let mut reflection: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*utils).vtable).create_reflection)(
                utils as *mut c_void,
                &buffer,
                &IID_ID3D12_SHADER_REFLECTION,
                &mut reflection,
            )
        };
        unsafe { ((*(*utils).vtable).base.release)(utils as *mut c_void) };
        if hr < 0 || reflection.is_null() {
            bail!("IDxcUtils::CreateReflection failed ({hr:#010x})");
        }
        let reflection = reflection as *mut ID3D12ShaderReflection;
        let vtable = unsafe { (*reflection).vtable };

        // The count lives in `D3D12_SHADER_DESC`, whose layout is ABI-sized;
        // asking until the call fails avoids depending on it.
        let mut bindings = Vec::new();
        for index in 0..1024u32 {
            let mut desc = ShaderInputBindDesc::default();
            let hr = unsafe {
                ((*vtable).get_resource_binding_desc)(reflection as *mut c_void, index, &mut desc)
            };
            if hr < 0 {
                break;
            }
            let name = if desc.name.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(desc.name as *const i8) }
                    .to_string_lossy()
                    .into_owned()
            };
            bindings.push(BoundResource {
                name,
                kind: desc.kind,
                bind_point: desc.bind_point,
                bind_count: desc.bind_count,
                flags: desc.flags,
                space: desc.space,
            });
        }

        unsafe { ((*vtable).base.release)(reflection as *mut c_void) };
        Ok(bindings)
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
        let validated = unsafe { self.validate(blob) };

        unsafe {
            (((*(*blob).vtable).base).release)(blob as *mut c_void);
            ((*vtable).base.release)(result as *mut c_void);
        }
        validated
    }

    /// Runs the DXIL validator over a compiled container and returns the
    /// validated container.
    ///
    /// The validator is `dxil.dll`, which signs the DXIL and fills the
    /// container header's 16 byte hash; D3D12 refuses to create a pipeline
    /// state from unsigned DXIL with `E_INVALIDARG`, and the compiler's API
    /// leaves the container unsigned (the `dxc.exe` path signs for the same
    /// reason). It also rejects malformed DXIL with the validator's message.
    ///
    /// The container is copied into a heap blob first: the validator edits the
    /// blob in place, and the compiler's own output blob is not writable.
    unsafe fn validate(&self, blob: *mut IDxcBlob) -> Result<Vec<u8>> {
        let Some(create_instance) = self.validate_instance else {
            bail!(
                "The DXIL validator '{VALIDATOR_NAME}' was not found next to '{LIBRARY_NAME}'. \
                 Without it the compiled DXIL is unsigned and D3D12 refuses it; copy the DXC \
                 release's '{VALIDATOR_NAME}' beside '{LIBRARY_NAME}'."
            );
        };

        let mut validator: *mut c_void = ptr::null_mut();
        let hr = unsafe { create_instance(&CLSID_DXC_VALIDATOR, &IID_IDXC_VALIDATOR, &mut validator) };
        if hr < 0 || validator.is_null() {
            bail!("DxcCreateInstance(IDxcValidator) failed ({hr:#010x})");
        }
        let validator = validator as *mut IDxcValidator;

        // A writable copy of the container for the in-place edit.
        let bytes = unsafe { read_blob(blob) };
        let mut utils: *mut c_void = ptr::null_mut();
        let hr = unsafe { (self.create_instance)(&CLSID_DXC_UTILS, &IID_IDXC_UTILS, &mut utils) };
        let mut copy: *mut c_void = ptr::null_mut();
        if hr >= 0 && !utils.is_null() {
            let utils = utils as *mut IDxcUtils;
            unsafe {
                ((*(*utils).vtable).create_blob)(
                    utils as *mut c_void,
                    bytes.as_ptr() as *const c_void,
                    bytes.len() as u32,
                    0,
                    &mut copy,
                )
            };
            unsafe { ((*(*utils).vtable).base.release)(utils as *mut c_void) };
        }
        let target = if copy.is_null() {
            blob
        } else {
            copy as *mut IDxcBlob
        };

        let mut result: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ((*(*validator).vtable).validate)(
                validator as *mut c_void,
                target as *mut c_void,
                DXC_VALIDATOR_FLAGS_IN_PLACE_EDIT,
                &mut result,
            )
        };
        if hr < 0 || result.is_null() {
            unsafe {
                if !copy.is_null() {
                    (((*(*(copy as *mut IDxcBlob)).vtable).base).release)(copy);
                }
                ((*(*validator).vtable).base.release)(validator as *mut c_void);
            }
            bail!("IDxcValidator::Validate failed ({hr:#010x})");
        }
        let result = result as *mut IDxcResult;
        let vtable = unsafe { (*result).vtable };

        let mut status: Hresult = 0;
        unsafe { ((*vtable).get_status)(result as *mut c_void, &mut status) };
        let validated = if status < 0 {
            let text = unsafe { error_text(result, vtable) };
            Err(color_eyre::eyre::eyre!(
                "dxcompiler validation failed:\n{text}"
            ))
        } else {
            Ok(unsafe { read_blob(target) })
        };

        unsafe {
            ((*vtable).base.release)(result as *mut c_void);
            ((*(*validator).vtable).base.release)(validator as *mut c_void);
            if !copy.is_null() {
                (((*(*(copy as *mut IDxcBlob)).vtable).base).release)(copy);
            }
        }
        validated
    }
}

/// The bytes of a blob.
unsafe fn read_blob(blob: *mut IDxcBlob) -> Vec<u8> {
    let vtable = unsafe { (*blob).vtable };
    let pointer = unsafe { ((*vtable).get_buffer_pointer)(blob as *mut c_void) as *const u8 };
    let size = unsafe { ((*vtable).get_buffer_size)(blob as *mut c_void) };
    unsafe { std::slice::from_raw_parts(pointer, size).to_vec() }
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

/// The validator library that sits next to a compiler library, or its platform
/// name for the system loader.
fn validator_path(compiler: &Path) -> PathBuf {
    match compiler
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => parent.join(VALIDATOR_NAME),
        None => PathBuf::from(VALIDATOR_NAME),
    }
}

/// A NUL-terminated UTF-16 copy of `text`.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
