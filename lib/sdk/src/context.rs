use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use crate::murmur::{Dictionary, HashGroup, IdString64, Murmur32, Murmur64};

#[derive(Clone)]
pub struct CmdLine {
    cmd: OsString,
    args: Vec<OsString>,
}

impl CmdLine {
    pub fn new(cmd: impl Into<OsString>) -> Self {
        Self {
            cmd: cmd.into(),
            args: vec![],
        }
    }

    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }
}

impl std::fmt::Debug for CmdLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CmdLine")
            .field("cmd", &self.cmd)
            .field("args", &self.args)
            .finish()
    }
}

impl std::fmt::Display for CmdLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.cmd.to_string_lossy())?;

        for arg in &self.args {
            write!(f, " \"{}\"", arg.to_string_lossy())?;
        }

        Ok(())
    }
}

impl From<&CmdLine> for Command {
    fn from(value: &CmdLine) -> Self {
        let mut cmd = Command::new(&value.cmd);
        cmd.args(&value.args);
        cmd
    }
}

pub struct Context {
    pub lookup: Arc<Dictionary>,
    pub ljd: Option<CmdLine>,
    pub revorb: Option<String>,
    pub ww2ogg: Option<String>,
    pub game_dir: Option<PathBuf>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            lookup: Arc::new(Dictionary::new()),
            ljd: None,
            revorb: None,
            ww2ogg: None,
            game_dir: None,
        }
    }

    pub fn lookup_hash<M>(&self, hash: M, group: HashGroup) -> IdString64
    where
        M: Into<Murmur64>,
    {
        let hash = hash.into();
        if let Some(s) = self.lookup.lookup(hash, group) {
            tracing::debug!(%hash, string = s, "Murmur64 lookup successful");
            s.to_string().into()
        } else {
            tracing::debug!(%hash, "Murmur64 lookup failed");
            hash.into()
        }
    }

    pub fn lookup_hash_short<M>(&self, hash: M, group: HashGroup) -> String
    where
        M: Into<Murmur32>,
    {
        let hash = hash.into();
        if let Some(s) = self.lookup.lookup_short(hash, group) {
            tracing::debug!(%hash, string = s, "Murmur32 lookup successful");
            s.to_owned()
        } else {
            tracing::debug!(%hash, "Murmur32 lookup failed");
            format!("{hash:08X}")
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
