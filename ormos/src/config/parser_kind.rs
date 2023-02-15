use std::{fmt, str::FromStr};

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    H1,
    Tls,
}

struct Visitor;
impl<'de> serde::de::Visitor<'de> for Visitor {
    type Value = Kind;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("String representing parser kind")
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Kind::from_str(&v).map_err(serde::de::Error::custom)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Kind::from_str(v).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(Visitor)
    }
}

impl FromStr for Kind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "h1" | "http/1" => Ok(Kind::H1),
            "tls" => Ok(Kind::Tls),
            _ => anyhow::bail!("Invalid parser kind"),
        }
    }
}

impl From<&Kind>
    for Box<
        dyn rpx::parser::Parser<String, Box<dyn std::error::Error + Send + 'static>>
            + Send
            + 'static,
    >
{
    fn from(kind: &Kind) -> Self {
        match kind {
            Kind::H1 => Box::<rpx::parser::http::Hostname>::default(),
            Kind::Tls => Box::<rpx::parser::tls::ServiceName>::default(),
        }
    }
}
