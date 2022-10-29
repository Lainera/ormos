use crate::config;
use futures::future::Either;
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use std::{
    collections::HashMap,
    future::{ready, Ready},
    net::{IpAddr, SocketAddr},
    task::{Context, Poll},
};
use tracing::{debug, instrument, trace, warn};

pub struct Layer {
    routes: HashMap<String, Vec<IpAddr>>,
    port_mapping: HashMap<(String, u16), u16>,
}

impl Layer {
    pub fn new<'a, I>(services: I) -> Self
    where
        I: Iterator<Item = &'a config::Service> + Clone,
    {
        Self {
            routes: forward_addr_by_service_name(services.clone()),
            port_mapping: remote_port_by_service_name_and_local(services),
        }
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(inner, self.routes.clone(), self.port_mapping.clone())
    }
}

#[derive(Debug)]
pub struct Service<S> {
    inner: S,
    routes: HashMap<String, Vec<IpAddr>>,
    port_mapping: HashMap<(String, u16), u16>,
}

impl<R> Service<R> {
    pub fn new(
        inner: R,
        routes: HashMap<String, Vec<IpAddr>>,
        port_mapping: HashMap<(String, u16), u16>,
    ) -> Self {
        Self {
            inner,
            routes,
            port_mapping,
        }
    }
}

fn forward_addr_by_service_name<'a, I: Iterator<Item = &'a config::Service>>(
    services: I,
) -> HashMap<String, Vec<IpAddr>> {
    services.filter(|service| !service.forward.is_empty()).fold(
        HashMap::new(),
        |mut map, service| {
            map.insert(service.name.clone(), service.forward.clone());
            map
        },
    )
}

fn remote_port_by_service_name_and_local<'a, I: Iterator<Item = &'a config::Service>>(
    services: I,
) -> HashMap<(String, u16), u16> {
    services.fold(HashMap::new(), |mut map, service| {
        for port_binding in service.ports.iter() {
            if map
                .insert((service.name.clone(), port_binding.0), port_binding.1)
                .is_some()
            {
                warn!(
                    name = service.name.as_str(),
                    port = port_binding.0,
                    "Duplicate port mapping detected"
                );
            }
        }
        map
    })
}

impl<S> tower::Service<(String, u16)> for Service<S>
where
    S: tower::Service<(String, u16), Response = Option<SocketAddr>>,
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
        let port: u16 = self
            .port_mapping
            .get(&request)
            .copied()
            .unwrap_or(request.1);

        trace!(port = port);
        let record = request.0;

        // get the override if any
        let address: Option<SocketAddr> = self
            .routes
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
    use super::{forward_addr_by_service_name, remote_port_by_service_name_and_local};
    use crate::config::{PortBinding, Service};
    use std::net::IpAddr;

    #[test]
    fn config_keys_by_name() {
        let services = vec![
            Service {
                name: "first.xyz".into(),
                ports: vec![PortBinding(80, 80)],
                forward: Vec::new(),
            },
            Service {
                name: "second.xyz".into(),
                ports: vec![PortBinding(80, 80)],
                forward: vec!["127.0.0.1".parse().unwrap()],
            },
        ];

        let by_name = forward_addr_by_service_name(services.iter());
        assert_eq!(by_name.len(), 1);

        let second = by_name.get("second.xyz").unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            second.first().unwrap(),
            &"127.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn config_keys_by_name_and_port() {
        let services = vec![
            Service {
                name: "first.xyz".into(),
                ports: vec![PortBinding(80, 80), PortBinding(3333, 4444)],
                forward: Vec::new(),
            },
            Service {
                name: "second.xyz".into(),
                ports: vec![PortBinding(80, 80), PortBinding(3333, 5555)],
                forward: vec!["127.0.0.1".parse().unwrap()],
            },
        ];

        let by_name_and_port = remote_port_by_service_name_and_local(services.iter());

        assert_eq!(
            by_name_and_port
                .get(&("first.xyz".to_string(), 80))
                .unwrap(),
            &80
        );
        assert_eq!(
            by_name_and_port
                .get(&("first.xyz".to_string(), 3333))
                .unwrap(),
            &4444
        );
        assert_eq!(
            by_name_and_port
                .get(&("second.xyz".to_string(), 80))
                .unwrap(),
            &80
        );
        assert_eq!(
            by_name_and_port
                .get(&("second.xyz".to_string(), 3333))
                .unwrap(),
            &5555
        );
    }

    #[test]
    fn config_keys_by_name_and_port_latter_takes_precedence() {
        let services = vec![Service {
            name: "first.xyz".into(),
            ports: vec![PortBinding(80, 80), PortBinding(80, 4444)],
            forward: Vec::new(),
        }];

        let by_name_and_port = remote_port_by_service_name_and_local(services.iter());

        assert_eq!(
            by_name_and_port
                .get(&("first.xyz".to_string(), 80))
                .unwrap(),
            &4444
        );
    }
}
