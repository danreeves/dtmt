use std::convert::Infallible;

use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderValue, InvalidHeaderValue};
use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;
use thiserror::Error;

mod types;
pub use types::*;

// TODO: Add OS information
const USER_AGENT: &str = concat!("DTMM/", env!("CARGO_PKG_VERSION"));

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
    async fn send<T>(&self, req: RequestBuilder) -> Result<T>
    where
        T: for<'a> Deserialize<'a>,
    {
        let res = req.send().await?.error_for_status()?;
        tracing::trace!(?res);

        let json = res.text().await?;
        serde_json::from_str(&json).map_err(|error| Error::Deserialize { json, error })
    }

    #[tracing::instrument(skip(self))]
    pub async fn user_validate(&self) -> Result<User> {
        let url = BASE_URL.join("users/validate.json")?;
        let req = self.client.get(url);
        self.send(req).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn mods_updated(&self, period: UpdatePeriod) -> Result<Vec<UpdateInfo>> {
        let url = BASE_URL_GAME.join("mods/updated.json")?;
        let req = self.client.get(url).query(&[period]);
        self.send(req).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn mods_id(&self, id: u64) -> Result<Mod> {
        let url = BASE_URL_GAME.join(&format!("mods/{}.json", id))?;
        let req = self.client.get(url);
        self.send(req).await
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

    #[tokio::test]
    async fn user_validate() {
        let client = make_api();
        client
            .user_validate()
            .await
            .expect("failed to query 'user_validate'");
    }

    #[tokio::test]
    async fn mods_id() {
        let client = make_api();
        let dmf_id = 8;
        client
            .mods_id(dmf_id)
            .await
            .expect("failed to query 'mods_id'");
    }
}
