use std::io::{Cursor, Write};

use color_eyre::{eyre::Context, Result};
use tokio::{fs, process::Command};

use crate::{
    binary::sync::WriteExt,
    bundle::file::{BundleFileVariant, UserFile},
    BundleFile, BundleFileType,
};

#[tracing::instrument(skip_all, fields(buf_len = data.as_ref().len()))]
pub(crate) async fn decompile<T>(_ctx: &crate::Context, data: T) -> Result<Vec<UserFile>>
where
    T: AsRef<[u8]>,
{
    let mut _r = Cursor::new(data.as_ref());
    todo!();
}

#[tracing::instrument(skip_all)]
pub(crate) async fn compile<S>(name: String, code: S) -> Result<BundleFile>
where
    S: AsRef<str>,
{
    let in_file_path = {
        let mut path = std::env::temp_dir();
        let name: String = std::iter::repeat_with(fastrand::alphanumeric)
            .take(10)
            .collect();
        path.push(name + "-dtmt.lua");

        path
    };

    let out_file_path = {
        let mut path = std::env::temp_dir();

        let name: String = std::iter::repeat_with(fastrand::alphanumeric)
            .take(10)
            .collect();
        path.push(name + "-dtmt.luab");

        path
    };

    fs::write(&in_file_path, code.as_ref().as_bytes())
        .await
        .wrap_err_with(|| format!("failed to write file {}", in_file_path.display()))?;

    // TODO: Make executable name configurable
    Command::new("luajit")
        .arg("-bg")
        .arg("-F")
        .arg(name.clone() + ".lua")
        .arg("-o")
        .arg("Windows")
        .arg(&in_file_path)
        .arg(&out_file_path)
        .status()
        .await
        .wrap_err("failed to compile to LuaJIT byte code")?;

    let mut data = Cursor::new(Vec::new());

    let bytecode = {
        let mut data = fs::read(&out_file_path)
            .await
            .wrap_err_with(|| format!("failed to read file {}", out_file_path.display()))?;

        // Add Fatshark's custom magic bytes
        data[1] = 0x46;
        data[2] = 0x53;
        data[3] = 0x82;

        data
    };

    data.write_u32(bytecode.len() as u32)?;
    // I believe this is supposed to be a uleb128, but it seems to be always 0x2 in binary.
    data.write_u64(0x2)?;
    data.write_all(&bytecode)?;

    let mut file = BundleFile::new(name, BundleFileType::Lua);
    let mut variant = BundleFileVariant::new();

    variant.set_data(data.into_inner());
    file.add_variant(variant);

    Ok(file)
}
