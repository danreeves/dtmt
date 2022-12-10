use std::fmt;
use std::num::ParseIntError;
use std::ops::Deref;

use serde::de::Visitor;
use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};

mod dictionary;
// Currently unused
// mod murmurhash32;
mod murmurhash64;

pub const SEED: u32 = 0;

pub use dictionary::Dictionary;
pub use dictionary::HashGroup;
pub use murmurhash64::hash;
pub use murmurhash64::hash32;
pub use murmurhash64::hash_inverse as inverse;

fn _swap_bytes_u32(value: u32) -> u32 {
    u32::from_le_bytes(value.to_be_bytes())
}

fn _swap_bytes_u64(value: u64) -> u64 {
    u64::from_le_bytes(value.to_be_bytes())
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct Murmur64(u64);

impl Murmur64 {
    pub fn hash<B>(s: B) -> Self
    where
        B: AsRef<[u8]>,
    {
        hash(s.as_ref(), SEED as u64).into()
    }
}

impl Deref for Murmur64 {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<u64> for Murmur64 {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl TryFrom<&str> for Murmur64 {
    type Error = ParseIntError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        u64::from_str_radix(value, 16).map(Self)
    }
}

impl fmt::UpperHex for Murmur64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl fmt::Display for Murmur64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl<'de> Visitor<'de> for Murmur64 {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(
            "an usigned 64 bit integer \
            or a string in hexadecimal format encoding such an integer",
        )
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let bytes = value.to_le_bytes();
        Ok(Self::from(u64::from_le_bytes(bytes)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Self::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match Murmur64::try_from(value) {
            Ok(hash) => Ok(hash),
            Err(err) => Err(E::custom(format!(
                "failed to convert '{value}' to Murmur64: {err}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Murmur64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(Self(0))
    }
}

impl Serialize for Murmur64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self:016X}"))
    }
}

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct Murmur32(u32);

impl Murmur32 {
    pub fn hash<B>(s: B) -> Self
    where
        B: AsRef<[u8]>,
    {
        hash32(s.as_ref(), SEED).into()
    }
}

impl From<u32> for Murmur32 {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl TryFrom<&str> for Murmur32 {
    type Error = ParseIntError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        u32::from_str_radix(value, 16).map(Self)
    }
}

impl fmt::UpperHex for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl fmt::Display for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl Serialize for Murmur32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{self:08X}"))
    }
}

impl<'de> Visitor<'de> for Murmur32 {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(
            "an usigned 32 bit integer \
            or a string in hexadecimal format encoding such an integer",
        )
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let bytes = value.to_le_bytes();
        self.visit_u32(u64::from_le_bytes(bytes) as u32)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_u32(value as u32)
    }

    fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Self::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match Murmur32::try_from(value) {
            Ok(hash) => Ok(hash),
            Err(err) => Err(E::custom(format!(
                "failed to convert '{value}' to Murmur32: {err}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Murmur32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(Self(0))
    }
}
