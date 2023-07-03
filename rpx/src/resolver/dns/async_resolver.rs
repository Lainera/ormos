use super::{Config, Error};
use core::fmt;
use rand::{prelude::IteratorRandom, rngs::SmallRng, SeedableRng};
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;
use tracing::{info_span, instrument, Instrument, Span};
use trust_dns_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

#[derive(Clone, Debug)]
pub struct Resolver {
    inner: TokioAsyncResolver,
    srv: Arc<Vec<String>>,
}

impl Resolver {
    /// Starts a background resolver task by leveraging [`trust_dns_resolver`].
    pub fn new(config: &Config) -> Result<Self, Error> {
        let mut resolver_opts = ResolverOpts::default();
        let Config {
            address,
            strategy,
            srv,
        } = config;

        // Be mindful of recursive calls when A record points to the instance running the forwarder
        resolver_opts.ip_strategy = *strategy;
        let name_server = NameServerConfig {
            socket_addr: *address,
            protocol: Protocol::Udp,
            tls_dns_name: None,
            trust_nx_responses: true,
            bind_addr: None,
        };
        let mut resolver_config = ResolverConfig::new();
        resolver_config.add_name_server(name_server);

        TokioAsyncResolver::tokio(resolver_config, resolver_opts)
            .map(|inner| Self {
                inner,
                srv: Arc::new(srv.to_vec()),
            })
            .map_err(Error::TrustDns)
    }

    pub fn should_lookup_srv(&self, record: &str) -> bool {
        self.srv.iter().any(|domain| record.ends_with(domain))
    }

    #[instrument(skip(self))]
    pub async fn resolve_ip<T, D>(
        &self,
        (record, port): (T, u16),
    ) -> Result<Option<SocketAddr>, Error>
    where
        T: fmt::Display + fmt::Debug + Clone + Deref<Target = D>,
        D: Deref<Target = str>,
    {
        let mut rng = SmallRng::from_entropy();
        let dns_span = info_span!("tokio-async-resolver");
        dns_span.follows_from(Span::current());

        let address = self
            .inner
            .lookup_ip(format!("{}.", record))
            .instrument(dns_span)
            .await?
            .iter()
            .choose(&mut rng)
            .map(|ip_addr| SocketAddr::from((ip_addr, port)));

        Ok(address)
    }

    #[instrument(skip(self))]
    pub async fn resolve_srv<T, D>(
        &self,
        (record, _): (T, u16),
    ) -> Result<Option<SocketAddr>, Error>
    where
        T: fmt::Display + fmt::Debug + Clone + Deref<Target = D>,
        D: Deref<Target = str>,
    {
        let mut rng = SmallRng::from_entropy();
        let dns_span = info_span!("tokio-async-resolver");
        dns_span.follows_from(Span::current());
        let response = self
            .inner
            .srv_lookup(format!("{}.", record))
            .instrument(dns_span)
            .await?;

        if let Some((name, port)) = response
            .iter()
            .map(|response| (response.target(), response.port()))
            .choose(&mut rng)
        {
            let address = self
                .inner
                .lookup_ip((*name).clone())
                .await?
                .iter()
                .next()
                .map(|ip| SocketAddr::from((ip, port)));

            Ok(address)
        } else {
            Ok(None)
        }
    }
}
