use rand::{prelude::IteratorRandom, rngs::SmallRng, SeedableRng};
use std::future::Future;
use std::task::{Context, Poll};
use std::{net::SocketAddr, pin::Pin};
use tracing::{debug, info_span, instrument, trace, Instrument, Span};
use trust_dns_resolver::error::ResolveError;
use trust_dns_resolver::{
    config::{LookupIpStrategy, NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

/// Starts a background resolver task by leveraging [`trust_dns_resolver`].
pub fn start(address: Option<SocketAddr>) -> Result<Service, Error> {
    let mut resolver_opts = ResolverOpts::default();
    // This is to avoid recursive calls when A record points to the instance running forwarder
    resolver_opts.ip_strategy = LookupIpStrategy::Ipv6Only;

    let resolver_config = match address {
        Some(socket_addr) => {
            let name_server = NameServerConfig {
                socket_addr,
                protocol: Protocol::Udp,
                tls_dns_name: None,
                trust_nx_responses: true,
                bind_addr: None,
            };

            let mut resolver_config = ResolverConfig::new();
            resolver_config.add_name_server(name_server);
            resolver_config
        }
        None => ResolverConfig::default(),
    };

    TokioAsyncResolver::tokio(resolver_config, resolver_opts)
        .map(Service)
        .map_err(Error::TrustDns)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    TrustDns(#[from] ResolveError),
}

/// Looks up AAAA records for provided service name
///
/// Leverages [tokio async resolver][TokioAsyncResolver] from
/// trust_dns_resolver crate.
#[derive(Clone)]
pub struct Service(TokioAsyncResolver);

impl tower::Service<(String, u16)> for Service {
    type Response = Option<SocketAddr>;
    type Error = Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Option<SocketAddr>, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[instrument(skip(self))]
    fn call(&mut self, (record, port): (String, u16)) -> Self::Future {
        debug!("enter");
        let handle = self.0.clone();
        let mut rng = SmallRng::from_entropy();
        trace!("rng");
        let dns_span = info_span!("tokio-async-resolver");
        dns_span.follows_from(Span::current());

        Box::pin(async move {
            let address = handle
                .lookup_ip(format!("{}.", &record))
                .instrument(dns_span)
                .await?
                // Pick random AAAA record
                .iter()
                .choose(&mut rng)
                .map(|ip_addr| SocketAddr::from((ip_addr, port)));

            Ok(address)
        })
    }
}

impl core::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            f.write_str("Resolver<Running>")
        } else {
            f.write_str("RR")
        }
    }
}
