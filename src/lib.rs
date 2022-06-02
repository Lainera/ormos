#![doc = include_str!("../Readme.md")]

pub mod config;
pub mod dns;
pub mod sni;

use bytes::BytesMut;
use dns::{Resolver, Running};
use sni::read_sni;
use tokio::{
    io::{self, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, instrument, warn};

#[instrument(skip(incoming), fields(incoming = ?incoming.local_addr(), resolver = %resolver))]
pub async fn forward(
    incoming: &mut TcpStream,
    resolver: Resolver<Running>,
) -> Result<(), anyhow::Error> {
    debug!("Started forwarder for {incoming:?}");
    let port = incoming.local_addr()?.port();
    debug!("Got port from incoming connection: {port}");

    let mut buf = BytesMut::with_capacity(256);
    let outgoing = match read_sni(&mut buf, incoming).await {
        Err(err) => {
            warn!("Failed to resolve sni: {err}, falling back to default destination");
            resolver.default_destination().await
        }
        Ok(sni) => {
            debug!(host = sni.as_str(), "resolved  sni");
            resolver.resolve(sni, port).await
        }
    }?;

    if outgoing.is_none() {
        warn!("Requested service for {incoming:?} is not configured, dropping request");
        incoming.shutdown().await?;
        return Ok(());
    }

    let outgoing = outgoing.unwrap();
    debug!("Forwarding request to {outgoing}");
    let mut outgoing = TcpStream::connect(outgoing).await?;

    // copy everything that was already read;
    outgoing.write_all(buf.as_ref()).await?;
    let (incoming, outgoing) = io::copy_bidirectional(incoming, &mut outgoing).await?;

    debug!(incoming, outgoing, "After copy_bidirectional");
    Ok(())
}
