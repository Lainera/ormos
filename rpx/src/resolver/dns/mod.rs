use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use trust_dns_resolver::{config::LookupIpStrategy, error::ResolveError};

mod async_resolver;
mod service;

use async_resolver::Resolver;
pub use service::Service;

#[derive(Debug, Deserialize)]
pub struct Config {
    address: SocketAddr,
    #[serde(default = "default_strategy")]
    strategy: LookupIpStrategy,
    #[serde(default)]
    srv: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    resolvers: Arc<Vec<Resolver>>,
}

impl Layer {
    pub fn new<'a, I>(configs: I) -> Result<Self, Error>
    where
        I: Iterator<Item = &'a Config>,
    {
        let resolvers = configs
            .map(Resolver::new)
            .collect::<Result<Vec<Resolver>, _>>()
            .map(Arc::new)?;

        Ok(Self { resolvers })
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(inner, self.resolvers.clone())
    }
}

const fn default_strategy() -> LookupIpStrategy {
    LookupIpStrategy::Ipv6Only
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    TrustDns(#[from] ResolveError),

    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync + 'static>),
}
