use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::murmur::{Dictionary, HashGroup, Murmur32, Murmur64};
use crate::oodle::Oodle;

pub struct Context {
    pub lookup: Dictionary,
    pub oodle: Option<Oodle>,
    pub ljd: Option<String>,
    pub revorb: Option<String>,
    pub ww2ogg: Option<String>,
    pub game_dir: Option<PathBuf>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            lookup: Dictionary::new(),
            oodle: None,
            ljd: None,
            revorb: None,
            ww2ogg: None,
            game_dir: None,
        }
    }

    pub fn lookup_hash<M>(&self, hash: M, group: HashGroup) -> String
    where
        M: Into<Murmur64>,
    {
        let hash = hash.into();
        if let Some(s) = self.lookup.lookup(hash, group) {
            tracing::debug!(%hash, string = s, "Murmur64 lookup successful");
            s.to_owned()
        } else {
            tracing::debug!(%hash, "Murmur64 lookup failed");
            format!("{hash:016X}")
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

pub async fn lookup_hash<M>(ctx: Arc<RwLock<Context>>, hash: M, group: HashGroup) -> String
where
    M: Into<Murmur64>,
{
    let hash = hash.into();
    if let Some(s) = ctx.read().await.lookup.lookup(hash, group) {
        tracing::debug!(%hash, string = s, "Murmur64 lookup successful");
        s.to_owned()
    } else {
        tracing::debug!(%hash, "Murmur64 lookup failed");
        format!("{hash:016X}")
    }
}
