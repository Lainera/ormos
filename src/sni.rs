//! To route by SNI, this field needs to be parsed from TLS handshake. 
//! To avoid reinventing wheels - module leverages [`rustls`].
//!
use anyhow::Error;
use bytes::BufMut;
use rustls::{internal::msgs::message::OpaqueMessage, server::Acceptor};
use std::io::Cursor;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, instrument, trace};

#[instrument]
/// Collects incoming data from [`tokio::io::AsyncReadExt`] into [`bytes::BufMut`], feeding each read into [`rustls::server::Acceptor`]
/// until SNI value is readable from TLS handshake or MAX_WIRE_SIZE exceeded.
pub async fn read_sni<B, R>(buf: &mut B, reader: &mut R) -> Result<String, Error>
where
    R: AsyncReadExt + Unpin + core::fmt::Debug,
    B: BufMut + AsRef<[u8]> + core::fmt::Debug,
{
    // Acceptor has internal buffer with the same data we keep in `buf` but couldn't find
    // a way to extract that buffer from the acceptors underlying connection.
    let mut acceptor = Acceptor::new()?;
    // Used together with Cursor to provide std::io::Read interface to stateful TLS acceptor.
    let mut accepted: usize = 0;

    loop {
        let read = reader.read_buf(buf).await?;
        trace!("Read {read} bytes from reader");
        let mut cursor = Cursor::new(&buf.as_ref()[accepted..]);
        accepted += acceptor.read_tls(&mut cursor)?;
        trace!("Accepted {accepted} total tls bytes");
        debug!("Read/Total: {read}/{accepted}");

        match acceptor.accept() {
            Ok(None) if accepted > OpaqueMessage::MAX_WIRE_SIZE => {
                error!("Buf exceeded max size");
                return Err(anyhow::anyhow!("Buf exceeded max size"));
            }
            Ok(None) => continue,
            Ok(Some(accepted)) => {
                let client_hello = accepted.client_hello();
                debug!(
                    "Got sni from incoming connection: {:?}",
                    client_hello.server_name()
                );
                return Ok(client_hello.server_name().unwrap_or("no-sni").to_owned());
            }
            Err(err) => return Err(anyhow::anyhow!("{err}")),
        }
    }
}
