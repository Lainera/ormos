//! Config handles configuration parsing and validation.
//!
//! Sample config file:
//! ```yaml
//!
//! ---
//! ## Provide socket address for custom dns server
//! dns: 8.8.8.8:53
//! ## Provide catch-all socket address destination for traffic w/o SNI
//! default_destination: '[2607:f8b0:400a:807::200e]:80'
//! ## Provide IP address to bind to
//! bind_address: 127.0.0.1
//! ## List of services to route for
//! services:
//!     ## hostname
//!   - name: bepis.com
//!     ## Upon receiving request for 'bepis.com'
//!     ## round robins to addresses below
//!     forward:
//!       - "127.0.0.1"
//!       - "::1"
//!     ## Listens on ports 3333 and 9000
//!     ## Forwards everything from 3333 to 6666
//!     ## Forwards everything from 9000 to 9000
//!     ports:
//!       - 3333:6666
//!       - 9000
//!   - name: google.com
//!     ## No forward section -> will use DNS
//!     ports:
//!       - 8000:443
//!       ## Multiple services can listen on the same port
//!       - 3333:443
//!      
//!   - name: example.com
//!     ## No forward section -> will use DNS
//!     ## No port section -> default to 443:443
//! ```

use clap::Parser;
use serde::{de::Visitor, Deserialize};
use std::{
    collections::HashSet,
    fs::File,
    marker::PhantomData,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use tracing::debug;

const DEFAULT_BIND: &str = "127.0.0.1";

#[derive(Parser, Debug)]
#[clap(version)]
struct CliConfig {
    /// Path to config file.
    /// Defaults to `~/.config/rpx.yaml`
    #[clap(short, long)]
    file: Option<String>,
}

#[derive(Deserialize)]
struct ConfigFile {
    #[serde(default)]
    dns: Option<String>,
    #[serde(default)]
    bind_address: Option<String>,
    #[serde(default)]
    default_destination: Option<String>,
    services: Vec<Service>,
}

pub fn load_config() -> Result<Config, anyhow::Error> {
    let cli = CliConfig::parse();

    let reader = cli
        .file
        .or_else(|| {
            std::env::var("HOME")
                .map(|home| format!("{home}/.config/rpx.yaml"))
                .ok()
        })
        .map(PathBuf::from)
        .and_then(|path| File::open(path).ok());

    if reader.is_none() {
        anyhow::bail!("Failed to open config file");
    }

    let config_file: ConfigFile = serde_yaml::from_reader(reader.unwrap())?;
    let config = config_file.validate()?;

    Ok(config)
}

impl ConfigFile {
    fn validate(self) -> Result<Config, anyhow::Error> {
        let dns: Option<SocketAddr> = self.dns.and_then(|as_string| as_string.parse().ok());

        if dns.is_none() && self.services.is_empty() {
            return Err(anyhow::anyhow!(
                "Config must include at least one service or dns server"
            ));
        }

        let bind_address: IpAddr = self
            .bind_address
            .and_then(|as_string| as_string.parse().ok())
            .unwrap_or_else(|| {
                DEFAULT_BIND
                    .parse()
                    .expect("Failed to parse valid bind address")
            });

        let default_destination = self.default_destination.and_then(|as_string| {
            debug!("{as_string}");
            as_string.parse().ok()
        });

        let config = Config {
            dns,
            bind_address,
            default_destination,
            services: self.services,
            _empty: PhantomData,
        };

        debug!("Generated config: {config:?}");

        Ok(config)
    }
}

/// Used for handling port binding in both formats:
/// - `local:remote`
/// - `port`
///
/// In the latter case binding is interpreted as `port:port`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortBinding(pub u16, pub u16);
struct PortBindingVisitor;

impl<'de> Deserialize<'de> for PortBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(PortBindingVisitor)
    }
}

impl<'de> Visitor<'de> for PortBindingVisitor {
    type Value = PortBinding;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("single port definition or port:port mapping")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let mut split = v.split(':').filter_map(|as_str| as_str.parse::<u16>().ok());

        match (split.next(), split.next()) {
            (Some(port), None) => Ok(PortBinding(port, port)),
            (Some(left), Some(right)) => Ok(PortBinding(left, right)),
            _ => Err(serde::de::Error::custom(format!(
                "Invalid format for port binding: {v}"
            ))),
        }
    }
}

/// Configuration of a single _service_.
#[derive(Deserialize, Debug)]
pub struct Service {
    /// service hostname
    pub name: String,
    #[serde(default = "default_ports")]
    /// Port binding, defaults to 443:443
    pub ports: Vec<PortBinding>,
    /// Hardcoded list of addresses to use. If non-empty,
    /// application does not leverage DNS, dispatching to addresses provided instead.
    #[serde(default)]
    pub forward: Vec<IpAddr>,
}

fn default_ports() -> Vec<PortBinding> {
    vec![PortBinding(443, 443)]
}

/// Parsed and validated config
#[derive(Debug)]
pub struct Config {
    /// Optional SocketAddress of a custom DNS server
    pub dns: Option<SocketAddr>,
    /// Address to bind to
    pub bind_address: IpAddr,
    /// List of [`Service`] definitions
    pub services: Vec<Service>,
    /// Optional SocketAddress to forward unrecognized traffic to.  
    pub default_destination: Option<SocketAddr>,
    // Ensure config could only be generated via [`ConfigFile::validate`]
    _empty: PhantomData<()>,
}

impl Config {
    /// Returns set of all ports forwarder should listen on
    pub fn listening_ports(&self) -> HashSet<u16> {
        self.services
            .iter()
            .flat_map(|service| service.ports.iter().map(|port_binding| port_binding.0))
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::{PortBinding, Service};
    use indoc::indoc;

    #[test]
    fn service_defaults_to_https() {
        let yaml = indoc! {"
        ---
        name: 'first.xyz'
        "};

        let svc: Service = serde_yaml::from_str(yaml).expect("Failed to parse valid yaml");
        assert_eq!(svc.ports.len(), 1);
        assert_eq!(svc.ports.first().unwrap(), &PortBinding(443, 443));
    }

    #[test]
    fn service_no_defaults_when_ports_defined() {
        let yaml = indoc! {"
        ---
        name: 'first.xyz'
        ports:
            - 80 
            - 3333:4444
        "};

        let svc: Service = serde_yaml::from_str(yaml).expect("Failed to parse valid yaml");
        assert_eq!(svc.ports.len(), 2);
        let mut iter = svc.ports.iter();
        assert_eq!(iter.next().unwrap(), &PortBinding(80, 80));
        assert_eq!(iter.next().unwrap(), &PortBinding(3333, 4444));
    }
}
