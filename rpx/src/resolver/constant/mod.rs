use futures::future::Either;
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use serde::Deserialize;
use std::{
    collections::HashMap,
    future::{ready, Ready},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, instrument, trace, warn};

mod port_binding;
use port_binding::PortBinding;

#[derive(Debug, Deserialize, PartialEq)]
/// Configuration for a single constant forwarding rule.
#[serde(untagged)]
pub enum Config {
    /// Override for a port number, could be used to map local high port to a remote
    /// low port.
    Port {
        name: String,
        ports: Vec<PortBinding>,
    },
    /// Override for ip address, to bypass any dns lookups
    Ip { name: String, ips: Vec<IpAddr> },
}

#[derive(Debug, Clone)]
pub struct Layer {
    ips: Arc<HashMap<String, Vec<IpAddr>>>,
    ports: Arc<HashMap<(String, u16), u16>>,
}

impl Layer {
    pub fn new<'a, I>(rules: I) -> Self
    where
        I: Iterator<Item = &'a Config>,
    {
        let mut ip_rules: HashMap<String, Vec<IpAddr>> = HashMap::new();
        let mut port_rules = HashMap::new();

        rules.for_each(|config| match config {
            Config::Port { name, ports } => {
                ports.iter().for_each(|PortBinding(from, to)| {
                    if port_rules.insert((name.clone(), *from), *to).is_some() {
                        warn!(name = name, port = from, "Duplicate port mapping detected");
                    }
                });
            }
            Config::Ip { name, ips } => {
                ip_rules.entry(name.clone()).or_default().extend(ips);
            }
        });

        Self {
            ips: Arc::new(ip_rules),
            ports: Arc::new(port_rules),
        }
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(inner, self.ips.clone(), self.ports.clone())
    }
}

#[derive(Debug, Clone)]
pub struct Service<S> {
    inner: S,
    ips: Arc<HashMap<String, Vec<IpAddr>>>,
    ports: Arc<HashMap<(String, u16), u16>>,
}

impl<S> Service<S> {
    pub fn new(
        inner: S,
        ips: Arc<HashMap<String, Vec<IpAddr>>>,
        ports: Arc<HashMap<(String, u16), u16>>,
    ) -> Self {
        Self { inner, ips, ports }
    }
}

impl<S> tower::Service<(String, u16)> for Service<S>
where
    S: tower::Service<(String, u16), Response = Option<SocketAddr>> + Clone,
    S::Future: Send + 'static,
{
    type Response = Option<SocketAddr>;
    type Error = S::Error;
    type Future = Either<Ready<Result<Option<SocketAddr>, S::Error>>, S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    #[instrument(skip(self))]
    fn call(&mut self, request: (String, u16)) -> Self::Future {
        debug!("enter");
        // translate port if any
        let port: u16 = self.ports.get(&request).copied().unwrap_or(request.1);

        trace!(port = port);
        let record = request.0;

        // get the override if any
        let address: Option<SocketAddr> = self
            .ips
            .get(&record)
            .and_then(|existing| {
                let mut rng = SmallRng::from_entropy();
                existing.choose(&mut rng)
            })
            .map(ToOwned::to_owned)
            .map(|ip_addr| (ip_addr, port).into());

        trace!(address = ?address);

        if address.is_some() {
            Either::Left(ready(Ok(address)))
        } else {
            let fut = self.inner.call((record, port));
            Either::Right(fut)
        }
    }
}

#[cfg(test)]
mod test {
    use super::{port_binding::PortBinding, Config, Layer};
    use indoc::indoc;
    use std::{
        convert::Infallible,
        future::{ready, Ready},
        net::SocketAddr,
        task::{Context, Poll},
    };
    use tower::{Layer as _, Service};

    #[derive(Clone)]
    struct S;

    impl tower::Service<(String, u16)> for S {
        type Response = Option<SocketAddr>;
        type Error = Infallible;
        type Future = Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, (_record, port): (String, u16)) -> Self::Future {
            ready(Ok(Some(([1, 2, 3, 4], port).into())))
        }
    }

    #[tokio::test]
    async fn given_no_match_does_nothing() {
        let layer = Layer::new(vec![].into_iter());
        let mut outer = layer.layer(S);

        let outcome = outer
            .call(("example.com".into(), 1234))
            .await
            .expect("Infallible");

        assert_eq!(outcome, Some(([1, 2, 3, 4], 1234).into()));
    }

    #[tokio::test]
    async fn given_match_on_port_overrides_port() {
        let port_rule = Config::Port {
            name: "example.com".to_string(),
            ports: vec![PortBinding(1234, 222)],
        };
        let layer = Layer::new(vec![&port_rule].into_iter());
        let mut outer = layer.layer(S);

        let outcome = outer
            .call(("example.com".into(), 1234))
            .await
            .expect("Infallible");

        assert_eq!(outcome, Some(([1, 2, 3, 4], 222).into()));
    }

    #[tokio::test]
    async fn given_match_on_ip_overrides_ip() {
        let ip_rule = Config::Ip {
            name: "example.com".to_string(),
            ips: vec![[1, 1, 1, 1].into()],
        };
        let layer = Layer::new(vec![&ip_rule].into_iter());
        let mut outer = layer.layer(S);

        let outcome = outer
            .call(("example.com".into(), 1234))
            .await
            .expect("Infallible");

        assert_eq!(outcome, Some(([1, 1, 1, 1], 1234).into()));
    }

    #[tokio::test]
    async fn given_a_match_on_both_overrides_both() {
        let port_rule = Config::Port {
            name: "example.com".to_string(),
            ports: vec![PortBinding(1234, 222)],
        };

        let ip_rule = Config::Ip {
            name: "example.com".to_string(),
            ips: vec![[1, 1, 1, 1].into()],
        };

        let layer = Layer::new(vec![&ip_rule, &port_rule].into_iter());
        let mut outer = layer.layer(S);

        let outcome = outer
            .call(("example.com".into(), 1234))
            .await
            .expect("Infallible");

        assert_eq!(outcome, Some(([1, 1, 1, 1], 222).into()));
    }

    #[test]
    fn deserializes() {
        let yaml = indoc! {"
        ---
        - name: 'first.xyz'
          ports:
            - '80'
            - '3333:4444'

        - name: 'first.xyz'
          ips:
            - '1.2.3.4' 
            - '8.8.8.8'
        "};

        let parsed: Result<Vec<Config>, _> = serde_yaml::from_str(yaml);
        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert_eq!(
            parsed[0],
            Config::Port {
                name: "first.xyz".to_string(),
                ports: vec![PortBinding(80, 80), PortBinding(3333, 4444)]
            }
        );
        assert_eq!(
            parsed[1],
            Config::Ip {
                name: "first.xyz".to_string(),
                ips: vec![[1, 2, 3, 4].into(), [8, 8, 8, 8].into()]
            }
        );
    }
}
