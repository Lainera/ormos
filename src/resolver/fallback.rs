use tracing::{debug, instrument};

use self::future::Fallback;
use std::task::{Context, Poll};

pub struct Layer<D>(D);

impl<D> Layer<D> {
    pub fn new(default: D) -> Self {
        Self(default)
    }
}

impl<S, D: Clone> tower::Layer<S> for Layer<D> {
    type Service = Service<S, D>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(self.0.clone(), inner)
    }
}

pub struct Service<S, D> {
    default: D,
    inner: S,
}

impl<S, D> Service<S, D> {
    pub fn new(default: D, inner: S) -> Self {
        Self { default, inner }
    }
}

impl<R, S, D> tower::Service<R> for Service<S, D>
where
    D: Clone,
    S: tower::Service<R, Response = Option<D>>,
{
    type Response = Option<D>;
    type Error = S::Error;
    type Future = Fallback<D, S::Error, S::Future>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Always ready to provide fallback
        Poll::Ready(Ok(()))
    }

    #[instrument(skip_all)]
    fn call(&mut self, req: R) -> Self::Future {
        debug!("enter");

        let fut = self.inner.call(req);
        future::Fallback::new(fut, self.default.clone())
    }
}

mod future {
    use std::{
        future::Future,
        marker::PhantomData,
        pin::Pin,
        task::{Context, Poll},
    };

    #[pin_project::pin_project]
    pub struct Fallback<D, E, F> {
        #[pin]
        inner: F,
        _err: PhantomData<E>,
        default: D,
    }

    impl<D, E, F> Fallback<D, E, F> {
        pub fn new(inner: F, default: D) -> Self {
            Self {
                inner,
                _err: PhantomData,
                default,
            }
        }
    }

    impl<D, E, F> Future for Fallback<D, E, F>
    where
        F: Future<Output = Result<Option<D>, E>>,
        D: Clone,
    {
        type Output = Result<Option<D>, E>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.project();
            match this.inner.poll(cx) {
                Poll::Ready(Ok(Some(value))) => Poll::Ready(Ok(Some(value))),
                Poll::Pending => Poll::Pending,
                _ => Poll::Ready(Ok(Some(this.default.clone()))),
            }
        }
    }
}
