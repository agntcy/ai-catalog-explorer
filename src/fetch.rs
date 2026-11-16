//! An HTTP fetcher that is safe to run for strangers: it refuses private addresses and
//! records every request so the page can show the trace.

use std::cell::{Cell, RefCell};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use ai_catalog_discover::{Fetcher, HttpFetcher, Response};
use ipnet::IpNet;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use serde::Serialize;
use url::{Host, Url};

const BLOCKED: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/3",
    "::/128",
    "::1/128",
    "64:ff9b::/96",
    "2001:db8::/32",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

static BLOCKED_NETS: LazyLock<Vec<IpNet>> =
    LazyLock::new(|| BLOCKED.iter().map(|net| net.parse().unwrap()).collect());

/// Whether `ip` is a public address that may be requested.
pub fn is_public(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    };
    !BLOCKED_NETS.iter().any(|net| net.contains(&ip))
}

/// Only `https` on the default port, and no private IP literal. Host names are checked
/// when they resolve, see [`PublicOnly`].
fn check_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("only https is allowed".into());
    }
    if url.port().is_some() {
        return Err("only the default https port is allowed".into());
    }
    let ip = match url.host() {
        Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        Some(Host::Domain(_)) => None,
        None => return Err("the URL has no host".into()),
    };
    match ip {
        Some(ip) if !is_public(ip) => Err("that address is not public".into()),
        _ => Ok(()),
    }
}

/// Resolves names and drops private addresses, so the check applies to the address that
/// is connected to and a rebinding DNS record gets nowhere.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err("the host resolves to no public address".into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// The discovery fetcher with the guards on.
pub struct Guarded(HttpFetcher);

impl Guarded {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("ai-catalog-explorer")
            .timeout(Duration::from_secs(10))
            .dns_resolver(Arc::new(PublicOnly))
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if let Err(reason) = check_url(attempt.url()) {
                    attempt.error(reason)
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self(HttpFetcher::with_client(client)))
    }
}

impl Fetcher for Guarded {
    fn get(&self, url: &Url) -> Result<Response, String> {
        check_url(url)?;
        self.0.get(url)
    }
}

/// What verification may fetch: DID documents and artifacts, a few per catalog.
pub struct Evidence<'a> {
    inner: &'a dyn Fetcher,
    left: Cell<usize>,
}

impl<'a> Evidence<'a> {
    pub fn new(inner: &'a dyn Fetcher, budget: usize) -> Self {
        Self {
            inner,
            left: Cell::new(budget),
        }
    }
}

impl ai_catalog_validate::Fetch for Evidence<'_> {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let left = self.left.get();
        if left == 0 {
            return Err("too many requests for one catalog".into());
        }
        self.left.set(left - 1);
        let url = Url::parse(url).map_err(|e| e.to_string())?;
        let response = self.inner.get(&url)?;
        if response.is_success() {
            Ok(response.body)
        } else {
            Err(format!("HTTP {}", response.status))
        }
    }
}

/// One request of the discovery, as the page shows it.
#[derive(Debug, Serialize)]
pub struct Hop {
    pub url: String,
    /// Where the response came from, when a redirect changed it.
    pub final_url: Option<String>,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub link_headers: Vec<String>,
    pub error: Option<String>,
    /// What discovery made of the response.
    pub note: Option<String>,
}

/// Records the requests made through `inner`.
pub struct Tracing<'a> {
    inner: &'a dyn Fetcher,
    hops: RefCell<Vec<Hop>>,
}

impl<'a> Tracing<'a> {
    pub fn new(inner: &'a dyn Fetcher) -> Self {
        Self {
            inner,
            hops: RefCell::default(),
        }
    }

    pub fn into_hops(self) -> Vec<Hop> {
        self.hops.into_inner()
    }
}

impl Fetcher for Tracing<'_> {
    fn get(&self, url: &Url) -> Result<Response, String> {
        let result = self.inner.get(url);
        let hop = match &result {
            Ok(response) => Hop {
                url: url.to_string(),
                final_url: (response.url != *url).then(|| response.url.to_string()),
                status: Some(response.status),
                content_type: response.content_type.clone(),
                link_headers: response.link_headers.clone(),
                error: None,
                note: None,
            },
            Err(error) => Hop {
                url: url.to_string(),
                final_url: None,
                status: None,
                content_type: None,
                link_headers: Vec::new(),
                error: Some(error.clone()),
                note: None,
            },
        };
        self.hops.borrow_mut().push(hop);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "2606:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn only_plain_https_to_public_hosts_is_requested() {
        for url in [
            "http://example.com/",
            "https://example.com:8443/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://169.254.169.254/latest",
        ] {
            assert!(check_url(&Url::parse(url).unwrap()).is_err(), "{url}");
        }
        assert!(check_url(&Url::parse("https://example.com/x").unwrap()).is_ok());
    }
}
