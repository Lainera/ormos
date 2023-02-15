//! Config handles configuration parsing and validation.
//!
//! Sample config file:
//! ```yaml
#![doc = include_str!("../../../sample_config.yml")]
//! ```

use clap::Parser;
use rpx::resolver;
use serde::Deserialize;
use std::{fs::File, marker::PhantomData, net::SocketAddr, path::PathBuf};
use tracing::debug;

mod listener;
mod parser_kind;

use listener::Listener;
use parser_kind::Kind;

#[derive(Parser, Debug)]
#[clap(version)]
struct CliConfig {
    /// Path to config file.
    /// Defaults to `~/.config/ormos.yaml`
    #[clap(short, long)]
    file: Option<String>,
}

#[derive(Deserialize)]
// Parsed config file contents
struct ConfigFile {
    listen: Vec<Listener>,
    rules: Vec<Rule>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Rule {
    Constant(resolver::constant::Config),
    Dns(resolver::dns::Config),
    Fallback(resolver::fallback::Config),
    Filter(resolver::filter::Config),
    Rewrite(resolver::rewrite::Config),
}

pub fn load_config() -> Result<Config, anyhow::Error> {
    let cli = CliConfig::parse();

    let reader = cli
        .file
        .or_else(|| {
            std::env::var("HOME")
                .map(|home| format!("{home}/.config/ormos.yaml"))
                .ok()
        })
        .map(PathBuf::from)
        .and_then(|path| File::open(path).ok());

    if reader.is_none() {
        anyhow::bail!("Failed to open config file");
    }

    let config_file: ConfigFile = serde_yaml::from_reader(reader.unwrap())?;
    let config = config_file.validate()?;

    debug!("Generated config: {config:?}");

    Ok(config)
}

impl ConfigFile {
    fn validate(self) -> Result<Config, anyhow::Error> {
        if self.rules.is_empty() {
            return Err(anyhow::anyhow!("Config must include at least one rule"));
        }

        let listen = if self.listen.is_empty() {
            vec![Listener::default()]
        } else {
            self.listen
        };

        let dns = {
            let mut dns_rules = self
                .rules
                .iter()
                .filter_map(|rule| match rule {
                    Rule::Dns(config) => Some(config),
                    _ => None,
                })
                .peekable();

            if dns_rules.peek().is_none() {
                None
            } else {
                Some(resolver::dns::Layer::new(dns_rules)?)
            }
        };

        // override is a keyword :(
        let override_rules = {
            let mut override_rules = self
                .rules
                .iter()
                .filter_map(|rule| match rule {
                    Rule::Constant(config) => Some(config),
                    _ => None,
                })
                .peekable();

            if override_rules.peek().is_none() {
                None
            } else {
                Some(resolver::constant::Layer::new(override_rules))
            }
        };

        let rewrite = {
            let mut rewrite_rules = self
                .rules
                .iter()
                .filter_map(|rule| match rule {
                    Rule::Rewrite(config) => Some(config),
                    _ => None,
                })
                .peekable();

            if rewrite_rules.peek().is_none() {
                None
            } else {
                Some(resolver::rewrite::Layer::new(rewrite_rules))
            }
        };

        let fallback = self
            .rules
            .iter()
            // One fallback is good enough
            .find_map(|rule| match rule {
                Rule::Fallback(config) => Some(config.address()),
                _ => None,
            })
            .map(resolver::fallback::Layer::new);

        let filter = {
            let mut filter_rules = self
                .rules
                .iter()
                .filter_map(|rule| match rule {
                    Rule::Filter(config) => Some(config),
                    _ => None,
                })
                .peekable();

            if filter_rules.peek().is_none() {
                None
            } else {
                Some(resolver::filter::Layer::new(filter_rules))
            }
        };

        Ok(Config {
            dns,
            override_rules,
            rewrite,
            fallback,
            filter,
            listen,
            _empty: PhantomData,
        })
    }
}

/// Parsed and initialized configuration of the app.
///
/// Includes optional layers used to compose the Resolver stack
#[derive(Debug)]
pub struct Config {
    /// Consult dns servers  
    pub dns: Option<resolver::dns::Layer>,
    /// Translate ports and explicitly specify destination
    pub override_rules: Option<resolver::constant::Layer>,
    /// Patch requested domain name
    pub rewrite: Option<resolver::rewrite::Layer>,
    /// Fallback if all else fails
    pub fallback: Option<resolver::fallback::Layer<SocketAddr>>,
    /// Only allow domains from the explicit list
    pub filter: Option<resolver::filter::Layer>,
    /// Addresses to bind to
    pub listen: Vec<Listener>,
    // Ensure config could only be generated via [`ConfigFile::validate`]
    _empty: PhantomData<()>,
}

#[cfg(test)]
mod test {
    use super::{Kind, Listener};
    use indoc::indoc;

    #[test]
    fn listener_deserializes() {
        let yaml = indoc! {"
        ---
        - address: '127.0.0.1:1234'
          parsers: ['h1', 'tls']
        - address: '127.0.0.1:3333'
          parsers: ['http/1']
        "};

        let parsed: Result<Vec<Listener>, _> = serde_yaml::from_str(yaml);

        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert_eq!(
            parsed[0],
            Listener {
                address: "127.0.0.1:1234".parse().expect("valid address"),
                parsers: vec![Kind::H1, Kind::Tls]
            }
        )
    }
}
