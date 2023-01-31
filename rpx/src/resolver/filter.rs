use serde::Deserialize;
use std::{collections::HashSet, sync::Arc};
use tower::filter::{Filter, Predicate};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Service is not supported `{0}`")]
    NotSupported(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    allowed_domains: Arc<Vec<String>>,
}

impl Layer {
    pub fn new<'a, I>(rules: I) -> Self
    where
        I: Iterator<Item = &'a Config>,
    {
        let allowed_domains = rules
            .map(|rule| rule.names.clone())
            .fold(HashSet::new(), |mut a, b| {
                a.extend(b);
                a
            })
            .into_iter()
            .collect();

        Layer {
            allowed_domains: Arc::new(allowed_domains),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check(Arc<Vec<String>>);

impl Predicate<(String, u16)> for Check {
    type Request = (String, u16);

    fn check(&mut self, (name, port): Self::Request) -> Result<Self::Request, tower::BoxError> {
        if self.0.iter().any(|domain| name.ends_with(domain)) {
            Ok((name, port))
        } else {
            Err(Box::new(Error::NotSupported(name)) as tower::BoxError)
        }
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Filter<S, Check>;

    fn layer(&self, inner: S) -> Self::Service {
        let check = Check(self.allowed_domains.clone());
        Filter::new(inner, check)
    }
}
