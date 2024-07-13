use self::util::{parse_hex32, parse_hex64};

use super::*;

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

impl From<u64> for Murmur64 {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Murmur64> for u64 {
    fn from(value: Murmur64) -> Self {
        value.0
    }
}

impl TryFrom<&str> for Murmur64 {
    type Error = Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        parse_hex64(value)
            .map(Self)
            .wrap_err_with(|| format!("Failed to convert value to Murmur64: {value}"))
    }
}

impl fmt::UpperHex for Murmur64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl fmt::LowerHex for Murmur64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::Display for Murmur64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", self)
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

impl From<Murmur32> for u32 {
    fn from(value: Murmur32) -> Self {
        value.0
    }
}

impl TryFrom<&str> for Murmur32 {
    type Error = Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        parse_hex32(value)
            .map(Self)
            .wrap_err_with(|| format!("Failed to convert value to Murmur32: {value}"))
    }
}

impl fmt::UpperHex for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.0, f)
    }
}

impl fmt::Display for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", self)
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
}

impl<S: Into<String>> From<S> for IdString64 {
    fn from(value: S) -> Self {
        Self::String(value.into())
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
