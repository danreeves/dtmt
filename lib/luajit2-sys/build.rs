use cc::Build;
use fs_extra::dir;
use fs_extra::dir::CopyOptions;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const LIB_NAME: &str = "luajit";
const LUAJIT_HEADERS: [&str; 4] = ["lua.h", "lualib.h", "lauxlib.h", "luajit.h"];
const LUAJIT_SRC: [&str; 65] = [
    // LJCORE_O
    // The MSVC toolchain cannot compile this assembler file,
    // as it contains GNU-specific directives
    // "lj_vm.S",
    "lj_gc.c",
    "lj_err.c",
    "lj_char.c",
    "lj_bc.c",
    "lj_obj.c",
    "lj_buf.c",
    "lj_str.c",
    "lj_tab.c",
    "lj_func.c",
    "lj_udata.c",
    "lj_meta.c",
    "lj_debug.c",
    "lj_state.c",
    "lj_dispatch.c",
    "lj_vmevent.c",
    "lj_vmmath.c",
    "lj_strscan.c",
    "lj_strfmt.c",
    "lj_strfmt_num.c",
    "lj_api.c",
    "lj_profile.c",
    "lj_lex.c",
    "lj_parse.c",
    "lj_bcread.c",
    "lj_bcwrite.c",
    "lj_load.c",
    "lj_ir.c",
    "lj_opt_mem.c",
    "lj_opt_fold.c",
    "lj_opt_narrow.c",
    "lj_opt_dce.c",
    "lj_opt_loop.c",
    "lj_opt_split.c",
    "lj_opt_sink.c",
    "lj_mcode.c",
    "lj_snap.c",
    "lj_record.c",
    "lj_crecord.c",
    "lj_ffrecord.c",
    "lj_asm.c",
    "lj_trace.c",
    "lj_gdbjit.c",
    "lj_ctype.c",
    "lj_cdata.c",
    "lj_cconv.c",
    "lj_ccall.c",
    "lj_ccallback.c",
    "lj_carith.c",
    "lj_clib.c",
    "lj_cparse.c",
    "lj_lib.c",
    "lj_alloc.c",
    // LJLIB_O
    "lib_aux.c",
    "lib_base.c",
    "lib_math.c",
    "lib_bit.c",
    "lib_string.c",
    "lib_table.c",
    "lib_io.c",
    "lib_os.c",
    "lib_package.c",
    "lib_debug.c",
    "lib_jit.c",
    "lib_ffi.c",
    "lib_init.c",
];

fn build_gcc(src_dir: &str) {
    let mut buildcmd = Command::new("make");
    buildcmd.current_dir(src_dir);
    buildcmd.stderr(Stdio::inherit());
    buildcmd.arg("--no-silent");

    // We do need to cross-compile even here, so that `lj_vm.o` is created
    // for the correct architecture.
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        buildcmd.arg("TARGET_SYS=Windows");
        buildcmd.arg("CROSS=x86_64-w64-mingw32-");
    }

    if cfg!(target_pointer_width = "32") {
        buildcmd.arg("HOST_CC='gcc -m32'");
        buildcmd.arg("-e");
    } else {
        buildcmd.arg("HOST_CC='gcc'");
    }

    let mut child = buildcmd.spawn().expect("failed to run make");

    if !child
        .wait()
        .map(|status| status.success())
        .map_err(|_| false)
        .unwrap_or(false)
    {
        panic!("Failed to build luajit");
    }
}

fn build_msvc(src_dir: &str, out_dir: &str) {
    let mut cc = Build::new();
    // cc can't handle many of the `cland-dl`-specific flags, so
    // we need to port them manually from a `make -n` run.
    cc.out_dir(out_dir)
        // `llvm-as` (which the clang-based toolchain for MSVC would use to compile `lj_vm.S`
        // assembler) doesn't support some of the GNU-specific directives.
        // However, the previous host-targeted compilation already created the
        // object, so we simply link that.
        .object(format!("{src_dir}/lj_vm.o"))
        .define("_FILE_OFFSET_BITS", "64")
        .define("_LARGEFILE_SOURCE", None)
        .define("LUA_MULTILIB", "\"lib\"")
        .define("LUAJIT_UNWIND_EXTERNAL", None)
        .flag("-fcolor-diagnostics")
        // Disable warnings
        .flag("/W0")
        .flag("/U _FORTIFY_SOURCE")
        // Link statically
        .flag("/MT")
        // Omit frame pointers
        .flag("/Oy");

    for f in LUAJIT_SRC {
        cc.file(format!("{src_dir}/{f}"));
    }

    cc.compile(LIB_NAME);
}

fn main() {
    let luajit_dir = format!("{}/luajit", env!("CARGO_MANIFEST_DIR"));
    let out_dir = env::var("OUT_DIR").unwrap();
    let src_dir = format!("{}/luajit/src", out_dir);

    dbg!(&luajit_dir);
    dbg!(&out_dir);
    dbg!(&src_dir);

    let mut copy_options = CopyOptions::new();
    copy_options.overwrite = true;

    dir::copy(&luajit_dir, &out_dir, &copy_options).expect("Failed to copy LuaJIT source");

    // The first run builds with and for the host architecture.
    // This also creates all the tools and generated sources that a compilation needs.
    build_gcc(&src_dir);

    // Then, for cross-compilation, we can utilize those generated
    // sources to re-compile just the library.
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        build_msvc(&src_dir, &out_dir);
        println!("cargo:rustc-link-search={}", out_dir);
    } else {
        println!("cargo:rustc-link-search=native={}", src_dir);
    }

    println!("cargo:lib-name={}", LIB_NAME);
    println!("cargo:include={}", src_dir);
    println!("cargo:rustc-link-lib=static={}", LIB_NAME);

    let mut bindings = bindgen::Builder::default();

    for header in LUAJIT_HEADERS {
        println!("cargo:rerun-if-changed={}/src/{}", luajit_dir, header);
        bindings = bindings.header(format!("{}/src/{}", luajit_dir, header));
    }

    let bindings = bindings
        .allowlist_var("LUA.*")
        .allowlist_var("LUAJIT.*")
        .allowlist_type("lua_.*")
        .allowlist_type("luaL_.*")
        .allowlist_function("lua_.*")
        .allowlist_function("luaL_.*")
        .allowlist_function("luaJIT.*")
        .ctypes_prefix("libc")
        .impl_debug(true)
        .use_core()
        .detect_include_paths(true)
        .formatter(bindgen::Formatter::Rustfmt)
        .sort_semantically(true)
        .merge_extern_blocks(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    let bindings = if env::var("CARGO_CFG_WINDOWS").is_ok() {
        bindings
            .clang_arg("-I/xwin/sdk/include/ucrt")
            .clang_arg("-I/xwin/sdk/include/um")
            .clang_arg("-I/xwin/sdk/include/shared")
            .clang_arg("-I/xwin/crt/include")
            .generate()
            .expect("Failed to generate bindings")
    } else {
        bindings.generate().expect("Failed to generate bindings")
    };

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Failed to write bindings");
}
