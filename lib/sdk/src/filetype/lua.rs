use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::io::Write;

use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::Result;
use luajit2_sys as lua;

use crate::binary::sync::WriteExt;
use crate::bundle::file::{BundleFileVariant, UserFile};
use crate::{BundleFile, BundleFileType};

#[tracing::instrument(skip_all, fields(buf_len = data.as_ref().len()))]
pub(crate) async fn decompile<T>(_ctx: &crate::Context, data: T) -> Result<Vec<UserFile>>
where
    T: AsRef<[u8]>,
{
    let mut _r = Cursor::new(data.as_ref());
    todo!();
}

#[tracing::instrument(skip_all)]
pub fn compile<S, C>(name: S, code: C) -> Result<BundleFile>
where
    S: Into<String>,
    C: AsRef<str>,
{
    let name = name.into();
    let code = code.as_ref();

    let bytecode = unsafe {
        let state = lua::luaL_newstate();
        lua::luaL_openlibs(state);

        let name = CString::new(name.as_bytes())
            .wrap_err_with(|| format!("Cannot convert name into CString: {}", name))?;
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
        lua::lua_setglobal(state, b"fn\0".as_ptr() as _);

        let run = b"return string.dump(fn, false)\0";
        match lua::luaL_loadstring(state, run.as_ptr() as _) as u32 {
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
