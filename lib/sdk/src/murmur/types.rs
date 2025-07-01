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
        write!(f, "{self:016X}")
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

impl fmt::LowerHex for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

impl fmt::Display for Murmur32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:08X}")
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
