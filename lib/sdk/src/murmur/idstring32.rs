use std::fmt;

use serde::{Deserializer, Serializer};

use super::Murmur32;

// This type encodes the fact that when reading in a bundle, we don't always have a dictionary
// entry for every hash in there. So we do want to have the real string available when needed,
// but at the same time retain the original hash information for when we don't.
// This is especially important when wanting to write back the read bundle, as the hashes need to
// stay the same.
// The previous system of always turning hashes into strings worked well for the purpose of
// displaying hashes, but would have made it very hard to turn a stringyfied hash back into
// an actual hash.
#[derive(Clone, Debug, Eq)]
pub enum IdString32 {
    Hash(Murmur32),
    String(String),
}

impl IdString32 {
    pub fn to_murmur32(&self) -> Murmur32 {
        match self {
            Self::Hash(hash) => *hash,
            Self::String(s) => Murmur32::hash(s.as_bytes()),
        }
    }

    pub fn display(&self) -> IdString32Display {
        let s = match self {
            IdString32::Hash(hash) => hash.to_string(),
            IdString32::String(s) => s.clone(),
        };

        IdString32Display(s)
    }

    pub fn is_string(&self) -> bool {
        match self {
            IdString32::Hash(_) => false,
            IdString32::String(_) => true,
        }
    }

    pub fn is_hash(&self) -> bool {
        match self {
            IdString32::Hash(_) => true,
            IdString32::String(_) => false,
        }
    }
}

impl From<String> for IdString32 {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<u32> for IdString32 {
    fn from(value: u32) -> Self {
        Self::Hash(value.into())
    }
}

impl From<IdString32> for u32 {
    fn from(value: IdString32) -> Self {
        value.to_murmur32().into()
    }
}

impl From<Murmur32> for IdString32 {
    fn from(value: Murmur32) -> Self {
        Self::Hash(value)
    }
}

impl From<IdString32> for Murmur32 {
    fn from(value: IdString32) -> Self {
        value.to_murmur32()
    }
}

impl PartialEq for IdString32 {
    fn eq(&self, other: &Self) -> bool {
        self.to_murmur32() == other.to_murmur32()
    }
}

impl std::hash::Hash for IdString32 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.to_murmur32().into());
    }
}

impl serde::Serialize for IdString32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.to_murmur32().into())
    }
}

struct IdString32Visitor;

impl<'de> serde::de::Visitor<'de> for IdString32Visitor {
    type Value = IdString32;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an u32 or a string")
    }

    fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString32::Hash(value.into()))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString32::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(IdString32::String(v))
    }
}

impl<'de> serde::Deserialize<'de> for IdString32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_u32(IdString32Visitor)
    }
}

pub struct IdString32Display(String);

impl std::fmt::Display for IdString32Display {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::UpperHex for IdString32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::UpperHex::fmt(&self.to_murmur32(), f)
    }
}

impl std::fmt::LowerHex for IdString32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::LowerHex::fmt(&self.to_murmur32(), f)
    }
}
