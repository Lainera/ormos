use super::{Error, Resolver};
use core::fmt;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{net::SocketAddr, pin::Pin};
use tracing::{debug, instrument};

/// Looks up records for provided service name
///
/// Leverages [tokio async resolver][trust_dns_resolver::TokioAsyncResolver] from
/// trust_dns_resolver crate.
#[derive(Clone)]
pub struct Service<S> {
    inner: S,
    resolvers: Arc<Vec<Resolver>>,
}

impl<S> Service<S> {
    pub fn new(inner: S, resolvers: Arc<Vec<Resolver>>) -> Self {
        Self { inner, resolvers }
    }

    #[instrument(skip(self), fields(resolvers = self.resolvers.len()))]
    async fn resolve_srv<T, D>(&self, (record, port): (T, u16)) -> Result<Option<SocketAddr>, Error>
    where
        // Some indirection to express deref coercion
        T: fmt::Display + fmt::Debug + Clone + Deref<Target = D>,
        D: Deref<Target = str>,
    {
        let resolvers = self.resolvers.clone();
        let mut futures: FuturesUnordered<_> = resolvers
            .iter()
            .filter(|resolver| resolver.should_lookup_srv(&record))
            .map(|resolver| {
                let record = record.clone();
                Box::pin(async move { resolver.resolve_srv((record, port)).await })
            })
            .collect();

        loop {
            match futures.next().await {
                Some(Ok(Some(address))) => return Ok(Some(address)),
                None => return Ok(None),
                _ => {}
            }
        }
    }

    #[instrument(skip(self), fields(resolvers = self.resolvers.len()))]
    async fn resolve_ip<T, D>(&self, (record, port): (T, u16)) -> Result<Option<SocketAddr>, Error>
    where
        // Some indirection to express deref coercion
        T: fmt::Display + fmt::Debug + Clone + Deref<Target = D>,
        D: Deref<Target = str>,
    {
        let resolvers = self.resolvers.clone();
        let mut futures: FuturesUnordered<_> = resolvers
            .iter()
            .map(|resolver| {
                let record = record.clone();
                Box::pin(async move { resolver.resolve_ip((record, port)).await })
            })
            .collect();

        loop {
            match futures.next().await {
                Some(Ok(Some(address))) => return Ok(Some(address)),
                // log errors here
                None => return Ok(None),
                _ => {}
            }
        }
    }
}

impl<S> tower::Service<(String, u16)> for Service<S>
where
    S: tower::Service<(String, u16), Response = Option<SocketAddr>> + Send + Sync + Clone + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    S::Future: Send + 'static,
{
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
        let should_lookup_srv = self
            .resolvers
            .iter()
            .any(|resolver| resolver.should_lookup_srv(&record));
        let mut this = self.clone();
        let clonable = Arc::new(record.clone());

        Box::pin(async move {
            let address = if should_lookup_srv {
                this.resolve_srv((clonable, port)).await
            } else {
                this.resolve_ip((clonable, port)).await
            };

            match address {
                Ok(Some(address)) => Ok(Some(address)),
                _ => this
                    .inner
                    .call((record, port))
                    .await
                    .map_err(Into::into)
                    .map_err(Error::Other),
            }
        })
    }
}

impl<S> fmt::Display for Service<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            f.write_str("Resolver<Running>")
        } else {
            f.write_str("RR")
        }
    }
}
