use std::env;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::process::Command;

use color_eyre::Result;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use tokio::fs;

use crate::binary::sync::ReadExt;
use crate::binary::sync::WriteExt;
use crate::bundle::file::{BundleFileVariant, UserFile};
use crate::murmur::IdString64;
use crate::{BundleFile, BundleFileType};

const BITSQUID_LUAJIT_HEADER: u32 = 0x8253461B;

#[tracing::instrument(skip_all, fields(buf_len = data.as_ref().len()))]
pub(crate) async fn decompile<T>(ctx: &crate::Context, data: T) -> Result<Vec<UserFile>>
where
    T: AsRef<[u8]>,
{
    let data = data.as_ref();
    let length = {
        let mut r = Cursor::new(data);
        // The first u32 is always zero, the second is the length of the
        // payload that follows the 24-byte header.
        r.skip_u32(0)?;
        r.read_u32()? as usize
    };

    // The game wraps Lua files in a 24-byte header (see `compile`).
    let content = &data[24..];
    eyre::ensure!(
        content.len() == length,
        "Content length doesn't match. Expected {}, got {}",
        length,
        content.len()
    );

    // Lua files may contain plain source instead of LuaJIT bytecode. Those can
    // simply be written out as-is.
    let is_bytecode = content.len() >= 4
        && u32::from_le_bytes([content[0], content[1], content[2], content[3]])
            == BITSQUID_LUAJIT_HEADER;

    if !is_bytecode {
        return Ok(vec![UserFile::new(content.to_vec())]);
    }

    let name = {
        let mut r = Cursor::new(content);

        eyre::ensure!(
            r.read_u32()? == BITSQUID_LUAJIT_HEADER,
            "Invalid magic bytes"
        );

        // Skip additional header bytes
        let _ = r.read_uleb128()?;
        let length = r.read_uleb128()? as usize;

        let mut buf = vec![0u8; length];
        r.read_exact(&mut buf)?;
        let mut s =
            String::from_utf8(buf).wrap_err("Invalid byte sequence for LuaJIT bytecode name")?;
        // Remove the leading `@`
        s.remove(0);
        s
    };

    let mut temp = env::temp_dir();
    // Using the actual file name and keeping it in case of an error makes debugging easier.
    // But to avoid creating a bunch of folders, we flatten the name.
    temp.push(name.replace('/', "_"));
    temp.set_extension("luao");

    tracing::debug!(
        "Writing temporary LuaJIT bytecode file to '{}'",
        temp.display()
    );

    fs::write(&temp, content)
        .await
        .wrap_err_with(|| format!("Failed to write LuaJIT bytecode to '{}'", temp.display()))?;

    let mut cmd = ctx
        .ljd
        .as_ref()
        .map(|c| c.into())
        .unwrap_or_else(|| Command::new("ljd"));

    cmd.arg("--catch_asserts")
        .args(["--function_def_sugar", "false"])
        .args(["--function_def_self_arg", "true"])
        .args(["--unsafe", "false"])
        .arg("-f")
        .arg(&temp);

    tracing::debug!("Executing command: '{:?}'", cmd);

    let output = cmd.output().wrap_err("Failed to run ljd")?;

    if !output.status.success() {
        let err = eyre::eyre!(
            "LJD exited with code {:?}:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        tracing::error!("Failed to decompile '{}':\n{:?}", name, err);
    }

    let content = output.stdout;

    // No need to wait for this, so we move it to a separate task.
    tokio::spawn(async move {
        if let Err(err) = fs::remove_file(&temp)
            .await
            .wrap_err_with(|| format!("Failed to remove temporary file '{}'", temp.display()))
        {
            tracing::warn!("{:?}", err);
        }
    });

    Ok(vec![UserFile::with_name(content, name)])
}

#[tracing::instrument(skip_all)]
pub fn compile(name: impl Into<IdString64>, code: impl AsRef<str>) -> Result<BundleFile> {
    let name = name.into();
    let code = code.as_ref();

    tracing::trace!(
        "Compiling '{}', {} bytes of code",
        name.display(),
        code.len()
    );

    // The game's Lua loader also accepts plain source files (this is what DML
    // ships in its patch bundle), which avoids having to match Fatshark's
    // LuaJIT bytecode version. So we store the source directly instead of
    // pre-compiling it to bytecode.
    let mut data = Cursor::new(Vec::with_capacity(code.len() + 24));
    data.write_u32(0)?;
    data.write_u32(code.len() as u32)?;
    // TODO: Figure out what these values are
    data.write_u32(0x1c)?;
    data.write_u32(0x2)?;
    data.write_u32(0x0)?;
    data.write_u32(0x0)?;
    data.write_all(code.as_bytes())?;

    let mut file = BundleFile::new(name, BundleFileType::Lua);
    let mut variant = BundleFileVariant::new();

    variant.set_data(data.into_inner());
    file.add_variant(variant);

    Ok(file)
}
