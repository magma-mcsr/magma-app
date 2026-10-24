//! Which network connection the app's traffic goes out through.
//!
//! By default the system decides. On a computer with more than one connection (for example
//! wired and Wi-Fi) the user can pick one in the settings, the way streaming software lets
//! you pick a network for the stream. The app then sends every request from that
//! connection's address, and Windows routes it out through that connection.

use std::net::IpAddr;

use serde::Serialize;

/// A connection the user can choose: its name as Windows shows it, and its IPv4 address.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Connection {
    pub name: String,
    pub address: String,
}

/// Network connections with an IPv4 address, excluding loopback and link-local ones.
pub fn connections() -> Vec<Connection> {
    let Ok(list) = if_addrs::get_if_addrs() else { return vec![] };
    let mut out: Vec<Connection> = list
        .into_iter()
        .filter(|i| !i.is_loopback() && !i.is_link_local())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => Some(Connection { name: i.name, address: ip.to_string() }),
            IpAddr::V6(_) => None,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// The local address to send from for the chosen connection. None means "let the system
/// decide": nothing chosen, or the chosen connection is not available right now.
/// Looked up by name every time, because a connection's address can change.
pub fn local_address(choice: Option<&str>) -> Option<IpAddr> {
    let name = choice?;
    connections().into_iter().find(|c| c.name == name).and_then(|c| c.address.parse().ok())
}

/// An HTTP client bound to the chosen connection. `bind` is called at the start of every
/// sync round; the client is rebuilt only when the address actually changed.
pub struct BoundClient {
    timeout: std::time::Duration,
    inner: std::sync::Mutex<(Option<IpAddr>, reqwest::Client)>,
}

impl BoundClient {
    pub fn new(timeout: std::time::Duration) -> Self {
        BoundClient { timeout, inner: std::sync::Mutex::new((None, build(None, timeout))) }
    }

    pub fn bind(&self, address: Option<IpAddr>) {
        let mut inner = self.inner.lock().expect("client lock");
        if inner.0 != address {
            *inner = (address, build(address, self.timeout));
        }
    }

    /// The current client (cheap to clone; clones share one connection pool).
    pub fn get(&self) -> reqwest::Client {
        self.inner.lock().expect("client lock").1.clone()
    }
}

fn build(address: Option<IpAddr>, timeout: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("magma-app/", env!("CARGO_PKG_VERSION"), " (magmamcsr.com)"))
        .timeout(timeout)
        .local_address(address)
        .build()
        .expect("HTTP client builds")
}
