use std::sync::Arc;

use tokio::sync::RwLock;

use crate::murmur::{Dictionary, HashGroup, Murmur32, Murmur64};

pub struct Context {
    pub lookup: Dictionary,
    pub oodle: Option<String>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            lookup: Dictionary::new(),
            oodle: None,
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
        format!("{:016X}", hash)
    }
}

pub async fn lookup_hash_short<M>(ctx: Arc<RwLock<Context>>, hash: M, group: HashGroup) -> String
where
    M: Into<Murmur32>,
{
    let hash = hash.into();
    if let Some(s) = ctx.read().await.lookup.lookup_short(hash, group) {
        tracing::debug!(%hash, string = s, "Murmur32 lookup successful");
        s.to_owned()
    } else {
        tracing::debug!(%hash, "Murmur32 lookup failed");
        format!("{:08X}", hash)
    }
}
