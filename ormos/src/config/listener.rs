use super::Kind;
use serde::Deserialize;
use std::net::SocketAddr;

const DEFAULT_BIND: &str = "127.0.0.1:8314";

/// Configuration for a single listener.
///
/// Listeners consist of bind address and collection of
/// incoming traffic parsers to apply.
#[derive(Deserialize, Debug, PartialEq)]
pub struct Listener {
    pub address: SocketAddr,
    // Listener with empty list of parsers can only send all traffic to default destination
    #[serde(default = "default_parsers")]
    pub parsers: Vec<Kind>,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            address: DEFAULT_BIND.parse().expect("Failed to parse valid address"),
            parsers: default_parsers(),
        }
    }
}

impl Listener {
    pub fn parsers(&self) -> &[Kind] {
        self.parsers.as_ref()
    }
}

fn default_parsers() -> Vec<Kind> {
    vec![Kind::H1, Kind::Tls]
}
