use std::{
    convert::Infallible,
    future::{ready, Ready},
    net::SocketAddr,
    task::{Context, Poll},
};

/// Leaf resolver that doesn't resolve anything
///
/// Useful in combination with other resolvers
pub struct Service;

impl tower::Service<(String, u16)> for Service {
    type Response = Option<SocketAddr>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: (String, u16)) -> Self::Future {
        ready(Ok(None))
    }
}
