use std::{fmt, path::Path};

use path_slash::PathExt as _;
use serde::{Deserializer, Serializer};

use super::Murmur64;

// This type encodes the fact that when reading in a bundle, we don't always have a dictionary
// entry for every hash in there. So we do want to have the real string available when needed,
// but at the same time retain the original hash information for when we don't.
// This is especially important when wanting to write back the read bundle, as the hashes need to
// stay the same.
// The previous system of always turning hashes into strings worked well for the purpose of
// displaying hashes, but would have made it very hard to turn a stringyfied hash back into
// an actual hash.
#[derive(Clone, Debug, Eq)]
pub enum IdString64 {
    Hash(Murmur64),
    String(String),
}

impl IdString64 {
    pub fn to_murmur64(&self) -> Murmur64 {
        match self {
            Self::Hash(hash) => *hash,
            Self::String(s) => Murmur64::hash(s.as_bytes()),
        }
    }

    pub fn display(&self) -> IdString64Display {
        let s = match self {
            IdString64::Hash(hash) => hash.to_string(),
            IdString64::String(s) => s.clone(),
        };

        IdString64Display(s)
    }

    pub fn is_string(&self) -> bool {
        match self {
            IdString64::Hash(_) => false,
            IdString64::String(_) => true,
        }
    }

    pub fn is_hash(&self) -> bool {
        match self {
            IdString64::Hash(_) => true,
            IdString64::String(_) => false,
        }
    }

    // Would love to have this as a proper `impl From`, but
    // rustc will complain that it overlaps with the `impl From<Into<String>>`.
    pub fn from_path(p: impl AsRef<Path>) -> Self {
        Self::String(p.as_ref().to_slash_lossy().to_string())
    }
}

impl From<String> for IdString64 {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<u64> for IdString64 {
    fn from(value: u64) -> Self {
        Self::Hash(value.into())
    }
}

impl From<Murmur64> for IdString64 {
    fn from(value: Murmur64) -> Self {
        Self::Hash(value)
    }
}

impl From<IdString64> for Murmur64 {
    fn from(value: IdString64) -> Self {
        value.to_murmur64()
    }
}

impl From<IdString64> for u64 {
    fn from(value: IdString64) -> Self {
        value.to_murmur64().into()
    }
}

impl Default for IdString64 {
    fn default() -> Self {
        Self::Hash(0.into())
    }
}

impl PartialEq for IdString64 {
    fn eq(&self, other: &Self) -> bool {
        self.to_murmur64() == other.to_murmur64()
    }
}

impl std::hash::Hash for IdString64 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.to_murmur64().into());
    }
}

impl serde::Serialize for IdString64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.to_murmur64().into())
    }
}

struct IdString64Visitor;

impl<'de> serde::de::Visitor<'de> for IdString64Visitor {
    type Value = IdString64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an u64 or a string")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString64::Hash(value.into()))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString64::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString64::String(v))
    }
}

impl<'de> serde::Deserialize<'de> for IdString64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_u64(IdString64Visitor)
    }
}

pub struct IdString64Display(String);

impl std::fmt::Display for IdString64Display {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::UpperHex for IdString64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::UpperHex::fmt(&self.to_murmur64(), f)
    }
}

impl std::fmt::LowerHex for IdString64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::LowerHex::fmt(&self.to_murmur64(), f)
    }
}
