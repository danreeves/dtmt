use reqwest::Url;
use serde::ser::SerializeTuple;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
pub struct User {
    pub user_id: u64,
    pub name: String,
    pub profile_url: Url,
    // pub is_premium: bool,
    // pub is_supporter: bool,
    // pub email: String,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModStatus {
    Published,
}

#[derive(Copy, Clone, Debug, Deserialize)]
pub enum EndorseStatus {
    Undecided,
}

#[derive(Debug, Deserialize)]
pub struct ModEndorsement {
    pub endorse_status: EndorseStatus,
    #[serde(with = "time::serde::timestamp::option")]
    pub timestamp: Option<OffsetDateTime>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Mod {
    pub name: String,
    pub description: String,
    pub summary: String,
    pub picture_url: Url,
    pub uid: u64,
    pub mod_id: u64,
    pub category_id: u64,
    pub version: String,
    #[serde(with = "time::serde::timestamp")]
    pub created_timestamp: OffsetDateTime,
    // created_time: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub updated_timestamp: OffsetDateTime,
    // updated_time: OffsetDateTime,
    pub author: String,
    pub uploaded_by: String,
    pub uploaded_users_profile_url: Url,
    pub status: ModStatus,
    pub available: bool,
    pub endorsement: ModEndorsement,
    // pub mod_downloads: u64,
    // pub mod_unique_downloads: u64,
    // pub game_id: u64,
    // pub allow_rating: bool,
    // pub domain_name: String,
    // pub endorsement_count: u64,
    // pub contains_adult_content: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateInfo {
    pub mod_id: u64,
    #[serde(with = "time::serde::timestamp")]
    pub latest_file_update: OffsetDateTime,
    #[serde(with = "time::serde::timestamp")]
    pub latest_mod_activity: OffsetDateTime,
}

#[derive(Copy, Clone, Debug)]
pub enum UpdatePeriod {
    Day,
    Week,
    Month,
}

impl Default for UpdatePeriod {
    fn default() -> Self {
        Self::Week
    }
}

impl Serialize for UpdatePeriod {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut tup = serializer.serialize_tuple(2)?;
        tup.serialize_element("period")?;
        tup.serialize_element(match self {
            Self::Day => "1d",
            Self::Week => "1w",
            Self::Month => "1m",
        })?;
        tup.end()
    }
}
