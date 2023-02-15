//! To route by SNI, field needs to be parsed from TLS handshake.
//! To avoid reinventing wheels - module leverages [`rustls`].
use super::Parser;
use rustls::{internal::msgs::message::OpaqueMessage, server::Acceptor};
use std::io::Cursor;
use tracing::{debug, error, instrument};

/// Parses service name extension
///
/// Stores [acceptor][Acceptor] and bytes accepted so far.
/// Technically could be stateless, but `Acceptor` already
/// has internal state.
pub struct ServiceName {
    acceptor: Acceptor,
    accepted: usize,
}

impl Default for ServiceName {
    fn default() -> Self {
        let acceptor = Acceptor::default();
        Self {
            acceptor,
            accepted: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Buf exceeded max size")]
    MaxSizeExceeded,
}

impl Parser<String, Box<dyn std::error::Error + Send + 'static>> for ServiceName {
    #[instrument(skip_all, fields(input_size = input.len()))]
    fn parse(
        &mut self,
        input: &[u8],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + 'static>> {
        let mut cursor = Cursor::new(&input[self.accepted..]);
        self.accepted += self
            .acceptor
            .read_tls(&mut cursor)
            .expect("Failed to read from in-memory cursor");

        match self.acceptor.accept() {
            Ok(None) if self.accepted > OpaqueMessage::MAX_WIRE_SIZE => {
                error!("Buf exceeded max size: {}", self.accepted);
                Err(Box::new(Error::MaxSizeExceeded))
            }
            Ok(None) => Ok(None),
            Ok(Some(accepted)) => {
                let client_hello = accepted.client_hello();
                let sni = client_hello.server_name().unwrap_or_default().to_owned();
                debug!("Got sni from incoming connection: {sni:?}");

                Ok(Some(sni))
            }
            Err(err) => Err(Box::new(err)),
        }
    }
}
