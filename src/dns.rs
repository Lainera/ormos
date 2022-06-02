//! Deals with looking up static routing overrides from config file and querying DNS server

use rand::{prelude::IteratorRandom, rngs::SmallRng, SeedableRng};
use std::net::SocketAddr;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot,
};
use tracing::{error, info_span, instrument, trace, Instrument};
use trust_dns_resolver::{
    config::{LookupIpStrategy, NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

use crate::config::Config;

#[derive(Debug)]
/// Handle to background DNS resolver task. 
pub struct Resolver<S> {
    state: S,
}

impl core::fmt::Display for Resolver<Running> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            let msg = format!("Resolver<Running>({:?})", self.state.default_destination);
            f.write_str(msg.as_str())
        } else {
            f.write_str("RR")
        }
    }
}

impl Clone for Resolver<Running> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl Resolver<Running> {
    #[instrument]
    /// Resolves hostname from SNI to remote address. 
    pub async fn resolve(
        &self,
        host: String,
        port: u16,
    ) -> Result<Option<SocketAddr>, anyhow::Error> {
        let (tx, rx) = oneshot::channel();
        let msg = Message {
            query: (host, port),
            response: tx,
        };

        self.state.sender.send(msg).await?;
        let address = rx.await?;

        Ok(address)
    }

    #[instrument]
    /// Surface default destination
    pub async fn default_destination(&self) -> Result<Option<SocketAddr>, anyhow::Error> {
        Ok(self.state.default_destination)
    }
}

#[derive(Debug)]
struct Message {
    query: (String, u16),
    response: oneshot::Sender<Option<SocketAddr>>,
}

#[derive(Clone, Debug)]
/// Internal state of configured [`Resolver`].
pub struct Running {
    sender: tokio::sync::mpsc::Sender<Message>,
    default_destination: Option<SocketAddr>,
}

/// Starts a background resolver task by leveraging [`trust_dns_resolver`].
pub fn start<const N: usize>(config: &Config) -> Result<Resolver<Running>, anyhow::Error> {
    let handle = start_background_resolver(config)?;
    let static_routes = config.forward_addr_by_service_name();
    let port_mapping = config.remote_port_by_service_name_and_local();
    let default_destination = config.default_destination;
    let spawn_span = info_span!("spawning-resolver");
    let mut rng = SmallRng::from_entropy();
    let (tx, mut rx): (Sender<Message>, Receiver<Message>) = tokio::sync::mpsc::channel(N);

    tokio::spawn({
        let span = info_span!("resolver");
        span.follows_from(spawn_span);
        async move {
            while let Some(msg) = rx.recv().await {
                trace!(
                    host = msg.query.0.as_str(),
                    port = msg.query.1,
                    "received msg to handle"
                );
                let outcome = match static_routes.get(&msg.query.0) {
                    // DNS lookup
                    Some(existing) if existing.is_empty() => {
                        let port = port_mapping.get(&msg.query).unwrap_or(&msg.query.1);
                        trace!(
                            host = msg.query.0.as_str(),
                            port = msg.query.1,
                            "no existing records found -> DNS lookup"
                        );
                        let dns_span = info_span!("dns-lookup");

                        let answer: Option<SocketAddr> = handle
                            .lookup_ip(format!("{}.", msg.query.0))
                            .instrument(dns_span)
                            .await
                            .ok()
                            // Pick random AAAA record
                            .and_then(|results| results.iter().choose(&mut rng))
                            .map(|ip_addr| (ip_addr, *port).into());

                        msg.response.send(answer)
                    }
                    // Predefined static route
                    Some(existing) => {
                        let port = port_mapping.get(&msg.query).unwrap_or(&msg.query.1);

                        let answer: Option<SocketAddr> = existing
                            .iter()
                            .choose(&mut rng)
                            .map(ToOwned::to_owned)
                            .map(|ip_addr| (ip_addr, *port).into());

                        msg.response.send(answer)
                    }
                    // SNI is not part of the config, fallback to default destination
                    None => msg.response.send(default_destination),
                };

                if outcome.is_err() {
                    error!(
                        "Failed to respond to query for {}:{}",
                        msg.query.0, msg.query.1
                    );
                }
            }
        }
        .instrument(span)
    });

    Ok(Resolver {
        state: Running {
            sender: tx,
            default_destination,
        },
    })
}

fn start_background_resolver(config: &Config) -> Result<TokioAsyncResolver, anyhow::Error> {
    let mut resolver_opts = ResolverOpts::default();
    // This is to avoid recursive calls when A record points to the instance running forwarder
    resolver_opts.ip_strategy = LookupIpStrategy::Ipv6Only;

    if config.dns.is_none() {
        return TokioAsyncResolver::tokio(ResolverConfig::default(), resolver_opts)
            .map_err(|err| anyhow::anyhow!("{err}"));
    }

    let name_server = NameServerConfig {
        socket_addr: config.dns.unwrap(),
        protocol: Protocol::Udp,
        tls_dns_name: None,
        trust_nx_responses: true,
        bind_addr: None,
    };

    let mut resolver_config = ResolverConfig::new();
    resolver_config.add_name_server(name_server);

    TokioAsyncResolver::tokio(resolver_config, resolver_opts)
        .map_err(|err| anyhow::anyhow!("{err}"))
}

