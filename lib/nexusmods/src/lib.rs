use std::convert::Infallible;

use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue, InvalidHeaderValue};
use reqwest::{Client, Url};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

// TODO: Add OS information
const USER_AGENT: &str = concat!("DTMM/", env!("CARGO_PKG_VERSION"));
const GAME_ID: &str = "warhammer40kdarktide";

lazy_static! {
    static ref BASE_URL: Url = Url::parse("https://api.nexusmods.com/v1/").unwrap();
    static ref BASE_URL_GAME: Url =
        Url::parse("https://api.nexusmods.com/v1/games/warhammer40kdarktide/").unwrap();
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0:?}")]
    HTTP(#[from] reqwest::Error),
    #[error("invalid URL: {0:?}")]
    URLParseError(#[from] url::ParseError),
    #[error("failed to deserialize '{error}': {json}")]
    Deserialize {
        json: String,
        error: serde_json::Error,
    },
    #[error(transparent)]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
    #[error("this error cannot happen")]
    Infallible(#[from] Infallible),
}

pub type Result<T> = std::result::Result<T, Error>;

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

pub struct Api {
    client: Client,
}

impl Api {
    pub fn new(key: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert("accept", HeaderValue::from_static("application/json"));
        headers.insert("apikey", HeaderValue::from_str(&key)?);

        let client = Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .build()?;

        Ok(Self { client })
    }

    #[tracing::instrument(skip(self))]
    pub async fn mods_updated(&self, period: UpdatePeriod) -> Result<Vec<UpdateInfo>> {
        let url = BASE_URL_GAME.join("mods/updated.json")?;

        let res = self
            .client
            .get(url)
            .query(&[period])
            .send()
            .await?
            .error_for_status()?;

        tracing::trace!(?res);
        let json = res.text().await?;

        serde_json::from_str(&json).map_err(|error| Error::Deserialize { json, error })
    }
}

#[cfg(test)]
mod test {
    use crate::Api;

    fn make_api() -> Api {
        let key = std::env::var("NEXUSMODS_API_KEY").expect("'NEXUSMODS_API_KEY' env var missing");
        Api::new(key).expect("failed to build API client")
    }

    #[tokio::test]
    async fn mods_updated() {
        let client = make_api();
        client
            .mods_updated(Default::default())
            .await
            .expect("failed to query 'mods_updated'");
    }
}
