use std::env;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::process::Command;

use color_eyre::Result;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use luajit2_sys as lua;
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
        r.read_u32()? as usize
    };

    // This skips the unknown bytes 5..12
    let content = &data[12..];
    eyre::ensure!(
        content.len() == length,
        "Content length doesn't match. Expected {}, got {}",
        length,
        content.len()
    );

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

    let bytecode = unsafe {
        let state = lua::luaL_newstate();
        lua::luaL_openlibs(state);

        let name = CString::new(format!("@{}", name.display()).into_bytes())
            .wrap_err_with(|| format!("Cannot convert name into CString: {}", name.display()))?;
        match lua::luaL_loadbuffer(
            state,
            code.as_ptr() as _,
            code.len() as _,
            name.as_ptr() as _,
        ) as u32
        {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to compile LuaJIT bytecode")
            }
            _ => unreachable!(),
        }
        lua::lua_setglobal(state, c"fn".as_ptr());

        let run = c"return string.dump(fn, false)";
        match lua::luaL_loadstring(state, run.as_ptr()) as u32 {
            lua::LUA_OK => {}
            lua::LUA_ERRSYNTAX => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Invalid syntax: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to compile LuaJIT bytecode")
            }
            _ => unreachable!(),
        }

        match lua::lua_pcall(state, 0, 1, 0) as u32 {
            lua::LUA_OK => {
                // The binary data is pretty much guaranteed to contain NUL bytes,
                // so we can't rely on `lua_tostring` and `CStr` here. Instead we have to
                // explicitely query the string length and build our vector from that.
                // However, on the bright side, we don't have to go through any string types anymore,
                // and can instead treat it as raw bytes immediately.
                let mut len = 0;
                let data = lua::lua_tolstring(state, -1, &mut len) as *const u8;
                let data = std::slice::from_raw_parts(data, len).to_vec();

                lua::lua_close(state);

                data
            }
            lua::LUA_ERRRUN => {
                let err = lua::lua_tostring(state, -1);
                let err = CStr::from_ptr(err).to_string_lossy().to_string();

                lua::lua_close(state);

                eyre::bail!("Failed to compile LuaJIT bytecode: {}", err);
            }
            lua::LUA_ERRMEM => {
                lua::lua_close(state);
                eyre::bail!("Failed to allocate sufficient memory to compile LuaJIT bytecode")
            }
            // We don't use an error handler function, so this should be unreachable
            lua::LUA_ERRERR => unreachable!(),
            _ => unreachable!(),
        }
    };

    let mut data = Cursor::new(Vec::with_capacity(bytecode.len() + 12));
    data.write_u32(bytecode.len() as u32)?;
    // TODO: Figure out what these two values are
    data.write_u32(0x2)?;
    data.write_u32(0x0)?;
    // Use Fatshark's custom magic bytes
    data.write_all(&[0x1b, 0x46, 0x53, 0x82])?;
    data.write_all(&bytecode[4..])?;

    let mut file = BundleFile::new(name, BundleFileType::Lua);
    let mut variant = BundleFileVariant::new();

    variant.set_data(data.into_inner());
    file.add_variant(variant);

    Ok(file)
}
