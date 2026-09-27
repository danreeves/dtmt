//! Compiling the dialect's programs with DXC.
//!
//! The invocation is small: write the assembled source to a temporary file,
//! run `dxc.exe -T <profile> -E <entry> -Fo <out>`, read the container back.
//! Discovery and the call live here so `dtmt build` and the tooling share one
//! truth about where DXC is and how it is invoked.
//!
//! The 6.0 profiles emit a `DXBC` container holding DXIL, which is what the
//! engine's programs are (see [`super::shader_node::profile_for`]).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use color_eyre::eyre::{Context as _, Result, bail};

/// Finds `dxc.exe` without a configured path: the `DTMT_DXC` environment
/// variable, then the newest Windows SDK installation. A caller that has its
/// own configured path checks that first.
pub fn find_dxc() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DTMT_DXC") {
        return Some(PathBuf::from(path));
    }
    if !cfg!(windows) {
        return None;
    }

    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("x64").join("dxc.exe"))
        .filter(|path| path.exists())
        .collect();
    candidates.sort();
    candidates.pop()
}

/// Compiles one HLSL source for a profile and entry point, returning the
/// container DXC wrote.
///
/// The in-process `dxcompiler.dll` is used when it can be found (it keeps the
/// source and the container in memory); otherwise the `dxc.exe` at `dxc` is
/// run with temporary files, because the executable has no stdin mode.
///
/// On failure the temporary source is kept and named in the error, so the
/// message points at the exact translation unit that did not compile.
pub fn compile(dxc: &Path, source: &str, profile: &str, entry: &str) -> Result<Vec<u8>> {
    #[cfg(windows)]
    if let Some(dll) = dxc::find_dll(dxc) {
        match dxc::Compiler::load(&dll).and_then(|compiler| compiler.compile(source, profile, entry))
        {
            Ok(container) => {
                tracing::debug!("Compiled in process with '{}'", dll.display());
                return Ok(container);
            }
            Err(err) => {
                tracing::debug!(
                    "In-process compile with '{}' failed, falling back to '{}': {err}",
                    dll.display(),
                    dxc.display()
                );
            }
        }
    }

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    profile.hash(&mut hasher);
    entry.hash(&mut hasher);
    let stem = format!(
        "dtmt-shader-{}-{:08x}",
        std::process::id(),
        hasher.finish() as u32
    );
    let source_path = std::env::temp_dir().join(format!("{stem}.hlsl"));
    let out_path = std::env::temp_dir().join(format!("{stem}.dxbc"));

    std::fs::write(&source_path, source)
        .wrap_err_with(|| format!("Failed to write '{}'", source_path.display()))?;

    let output = Command::new(dxc)
        .arg("-T")
        .arg(profile)
        .arg("-E")
        .arg(entry)
        .arg("-Fo")
        .arg(&out_path)
        .arg(&source_path)
        .output()
        .wrap_err_with(|| format!("Failed to run '{}'", dxc.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to compile '{}' as {profile}/{entry}:\n{}",
            source_path.display(),
            stderr.trim()
        );
    }

    let data = std::fs::read(&out_path)
        .wrap_err_with(|| format!("Failed to read '{}'", out_path.display()))?;
    let _ = std::fs::remove_file(&source_path);
    let _ = std::fs::remove_file(&out_path);
    Ok(data)
}
