use std::fmt;

use color_eyre::eyre::Context;
use color_eyre::{Report, Result};
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod dictionary;
// Currently unused
// mod murmurhash32;
mod idstring32;
mod idstring64;
mod murmurhash64;
mod types;
mod util;

pub const SEED: u32 = 0;

pub use dictionary::{Dictionary, Entry, HashGroup};
pub use idstring32::*;
pub use idstring64::*;
pub use murmurhash64::hash;
pub use murmurhash64::hash_inverse as inverse;
pub use murmurhash64::hash32;

pub use types::*;
