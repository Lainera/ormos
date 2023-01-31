use std::{
    borrow::Cow,
    sync::Arc,
    task::{Context, Poll},
};

use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Service<S> {
    rules: Arc<Vec<Config>>,
    inner: S,
}

#[derive(Debug, Clone)]
pub struct Layer {
    rules: Arc<Vec<Config>>,
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(self.rules.clone(), inner)
    }
}

impl Layer {
    pub fn new<'a, I>(rules: I) -> Self
    where
        I: Iterator<Item = &'a Config>,
    {
        let rules = Arc::new(rules.cloned().collect());
        Self { rules }
    }
}

impl<S> Service<S> {
    pub fn new(rules: Arc<Vec<Config>>, inner: S) -> Self {
        Self { rules, inner }
    }

    pub fn apply_all(&self, input: String) -> String {
        self.rules
            .iter()
            .find_map(|rule| match rule.apply(&input) {
                Cow::Borrowed(_) => None,
                Cow::Owned(applied) => Some(applied),
            })
            .unwrap_or(input)
    }
}

impl<S> tower::Service<(String, u16)> for Service<S>
where
    S: tower::Service<(String, u16)>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, (name, port): (String, u16)) -> Self::Future {
        let name = self.apply_all(name);
        self.inner.call((name, port))
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(with = "serde_regex")]
    matcher: Regex,
    replacer: String,
}

impl Config {
    pub fn apply<'s, 'a>(&'s self, input: &'a str) -> Cow<'a, str> {
        self.matcher.replace(input, &self.replacer)
    }
}

#[cfg(test)]
mod test {
    use super::{Config, Service};
    use indoc::indoc;
    use regex::Regex;
    use std::{borrow::Cow, sync::Arc};
    use test_case::test_case;

    #[test_case("likes.some.domain", Cow::Owned("likes.patched".to_owned()); "Patches on a match")]
    #[test_case("follows.some.domain", Cow::Owned("follows.patched".to_owned()); "Patches on a different service match")]
    #[test_case("example.com", Cow::Borrowed("example.com"); "Ignores on no match")]
    #[test_case("some.domain", Cow::Borrowed("some.domain"); "Ignores tld")]
    fn rules(input: &str, output: Cow<'_, str>) {
        let rule = Config {
            matcher: Regex::new(r#"^(?P<svc>[a-z]+)\.some\.domain$"#).expect("Valid regex"),
            replacer: "$svc.patched".to_owned(),
        };

        assert_eq!(rule.apply(input), output);
    }

    #[test]
    fn deserializes() {
        let input = indoc! {r#"
        ---
        matcher: '^(?P<svc>[a-z]+)\.com$'
        replacer: '$svc.internal'
        "#};

        let rule: Result<Config, _> = serde_yaml::from_str(input);
        assert!(rule.is_ok(), "Failed to deserialize rule");
    }

    #[test_case("example.com", "example.internal"; "First match on .com")]
    #[test_case("abc.com", "abc.internal"; "Second match on .com")]
    #[test_case("abc.org", "abc.borg"; "First match on .org")]
    fn config_order(input: &str, output: &str) {
        let config = indoc! {r#"
        ---
        - matcher: '^(?P<svc>[a-z]+)\.com$'
          replacer: '$svc.internal'
        - matcher: '^(?P<svc>[a-z]+)\.com$'
          replacer: '$svc.never-picked'
        - matcher: '^(?P<svc>[a-z]+)\.org$'
          replacer: '$svc.borg'
        "#};
        let rules: Result<Vec<Config>, _> = serde_yaml::from_str(config);
        assert!(rules.is_ok(), "Failed to deserialize rules");
        let rules = rules.map(Arc::new).unwrap();
        let svc = Service::new(rules, ());

        assert_eq!(&svc.apply_all(input.to_string()), output);
    }
}
