use serde::{de::Visitor, Deserialize};

/// Used for handling port binding in both formats:
/// - `local:remote`
/// - `port`
///
/// In the latter case binding is interpreted as `port:port`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortBinding(pub u16, pub u16);
struct PortBindingVisitor;

impl<'de> Deserialize<'de> for PortBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(PortBindingVisitor)
    }
}

impl<'de> Visitor<'de> for PortBindingVisitor {
    type Value = PortBinding;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("single port definition or port:port mapping")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let mut split = v.split(':').filter_map(|as_str| as_str.parse::<u16>().ok());

        match (split.next(), split.next()) {
            (Some(port), None) => Ok(PortBinding(port, port)),
            (Some(left), Some(right)) => Ok(PortBinding(left, right)),
            _ => Err(serde::de::Error::custom(format!(
                "Invalid format for port binding: {v}"
            ))),
        }
    }
}
