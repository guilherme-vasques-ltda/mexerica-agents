use anyhow::{Context, Result};
use std::path::PathBuf;

pub(crate) const SOCKET_ENV: &str = "MEXERICA_DESKTOP_SOCKET";
pub(crate) const SOCKET_PORT: u16 = 0;

#[derive(Clone)]
pub(crate) struct BridgeHttp {
    client: reqwest::Client,
    port: u16,
}

impl BridgeHttp {
    pub(crate) fn new(port: u16) -> Result<Self> {
        let client = if port == SOCKET_PORT {
            let path = PathBuf::from(std::env::var_os(SOCKET_ENV).context("private Hub socket absent")?);
            Self::unix(path)?
        } else {
            crate::http().clone()
        };
        Ok(Self { client, port })
    }

    #[cfg(unix)]
    fn unix(path: PathBuf) -> Result<reqwest::Client> {
        crate::fork::desktop_socket::validate_endpoint(&path)?;
        Ok(reqwest::Client::builder()
            .unix_socket(path)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?)
    }

    #[cfg(not(unix))]
    fn unix(_: PathBuf) -> Result<reqwest::Client> {
        anyhow::bail!("private Hub socket requires Unix")
    }

    pub(crate) fn post(&self, method: &str) -> reqwest::RequestBuilder {
        if self.port == SOCKET_PORT {
            self.client.post(format!("http://mexerica-local.example.invalid/api/{method}"))
        } else {
            self.client.post(format!("http://127.0.0.1:{}/api/{method}", self.port))
        }
    }
}

#[cfg(all(test, unix))]
#[path = "bridge_http_tests.rs"]
mod tests;
