use std::collections::HashMap;
use std::convert::Infallible;

use lazy_static::lazy_static;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, InvalidHeaderValue};
use reqwest::{Client, IntoUrl, RequestBuilder, Url};
use serde::Deserialize;
use thiserror::Error;

mod types;
use time::OffsetDateTime;
pub use types::*;

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
    #[error("invalid NXM URL '{}': {0}", .1.as_str())]
    InvalidNXM(&'static str, Url),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Nxm {
    pub mod_id: u64,
    pub file_id: u64,
    pub user_id: u64,
    pub key: String,
    pub expires: OffsetDateTime,
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

    #[tracing::instrument(skip(self))]
    pub async fn picture(&self, url: impl IntoUrl + std::fmt::Debug) -> Result<Vec<u8>> {
        let res = self.client.get(url).send().await?.error_for_status()?;

        res.bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(From::from)
    }

    pub fn parse_file_name<S: AsRef<str>>(
        name: S,
    ) -> Option<(String, u64, String, OffsetDateTime)> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r#"^(?P<name>.+?)-(?P<mod_id>[1-9]\d*)-(?P<version>.+?)-(?P<updated>[1-9]\d*)(?:\.\w+)?$"#).unwrap();
        }

        RE.captures(name.as_ref()).and_then(|cap| {
            let name = cap.name("name").map(|s| s.as_str().to_string())?;
            let mod_id = cap.name("mod_id").and_then(|s| s.as_str().parse().ok())?;
            let version = cap.name("version").map(|s| s.as_str().replace('-', "."))?;
            let updated = cap
                .name("updated")
                .and_then(|s| s.as_str().parse().ok())
                .and_then(|s| OffsetDateTime::from_unix_timestamp(s).ok())?;

            Some((name, mod_id, version, updated))
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn mods_download_link(
        &self,
        mod_id: u64,
        file_id: u64,
        key: String,
        expires: OffsetDateTime,
    ) -> Result<Vec<DownloadLink>> {
        let url =
            BASE_URL_GAME.join(&format!("mods/{mod_id}/files/{file_id}/download_link.json"))?;
        let req = self
            .client
            .get(url)
            .query(&[("key", key)])
            .query(&[("expires", expires.unix_timestamp())]);
        self.send(req).await
    }

    pub async fn handle_nxm(&self, url: Url) -> Result<(Mod, Vec<u8>)> {
        let nxm = Self::parse_nxm(url.clone())?;

        let user = self.user_validate().await?;

        if nxm.user_id != user.user_id {
            return Err(Error::InvalidNXM("user_id mismtach", url));
        }

        let (mod_data, download_info) = futures::try_join!(
            self.mods_id(nxm.mod_id),
            self.mods_download_link(nxm.mod_id, nxm.file_id, nxm.key, nxm.expires)
        )?;

        let Some(download_url) = download_info.first().map(|i| i.uri.clone()) else {
            return Err(Error::InvalidNXM("no download link", url));
        };

        let req = self.client.get(download_url);
        let data = req.send().await?.bytes().await?;

        Ok((mod_data, data.to_vec()))
    }

    pub fn parse_nxm(nxm: Url) -> Result<Nxm> {
        if nxm.scheme() != "nxm" {
            return Err(Error::InvalidNXM("Invalid scheme", nxm));
        }

        // Now it makes sense, why Nexus calls this field `game_domain_name`, when it's just
        // another path segmentin the regular API calls.
        if nxm.host_str() != Some(GAME_ID) {
            return Err(Error::InvalidNXM("Invalid game domain name", nxm));
        }

        let Some(mut segments) = nxm.path_segments() else {
            return Err(Error::InvalidNXM("Cannot be a base", nxm));
        };

        if segments.next() != Some("mods") {
            return Err(Error::InvalidNXM("Unexpected path segment", nxm));
        }

        let Some(mod_id) = segments.next().and_then(|id| id.parse().ok()) else {
            return Err(Error::InvalidNXM("Invalid mod ID", nxm));
        };

        if segments.next() != Some("files") {
            return Err(Error::InvalidNXM("Unexpected path segment", nxm));
        }

        let Some(file_id) = segments.next().and_then(|id| id.parse().ok()) else {
            return Err(Error::InvalidNXM("Invalid file ID", nxm));
        };

        let mut query = HashMap::new();
        let pairs = nxm.query_pairs();

        for (key, val) in pairs {
            query.insert(key, val);
        }

        let Some(key) = query.get("key") else {
            return Err(Error::InvalidNXM("Missing 'key'", nxm));
        };

        let expires = query
            .get("expires")
            .and_then(|expires| expires.parse().ok())
            .and_then(|expires| OffsetDateTime::from_unix_timestamp(expires).ok());
        let Some(expires) = expires else {
            return Err(Error::InvalidNXM("Missing 'expires'", nxm));
        };

        let user_id = query.get("user_id").and_then(|id| id.parse().ok());
        let Some(user_id) = user_id else {
            return Err(Error::InvalidNXM("Missing 'user_id'", nxm));
        };

        Ok(Nxm {
            mod_id,
            file_id,
            key: key.to_string(),
            expires,
            user_id,
        })
    }
}

#[cfg(test)]
mod test {
    use reqwest::Url;
    use time::OffsetDateTime;

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

    #[test]
    fn parse_file_name() {
        let file = "Darktide Mod Framework-8-23-3-04-1677966575.zip";
        let (name, mod_id, version, updated) = Api::parse_file_name(file).unwrap();

        assert_eq!(name, String::from("Darktide Mod Framework"));
        assert_eq!(mod_id, 8);
        assert_eq!(version, String::from("23-3-04"));
        assert_eq!(
            updated,
            OffsetDateTime::from_unix_timestamp(1677966575).unwrap()
        );
    }

    #[test]
    fn parse_nxm() {
        let nxm = Url::parse("nxm://warhammer40kdarktide/mods/8/files/1000172397?key=VZ86Guj_LosPvtkD90-ZQg&expires=1678359882&user_id=1234567").expect("invalid NXM example");
        Api::parse_nxm(nxm).expect("failed to parse nxm link");
    }
}
